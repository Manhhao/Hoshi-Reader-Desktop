use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::statistics;
use crate::sync::client;
use crate::sync::gdrive::{files, state};
use crate::sync::model::{SyncBook, SyncError, SyncResult, sync_format};
use crate::sync::storage::{self, SyncStorage};
use crate::sync::task::{self, SyncTask};
use crate::sync::{app, auth};

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveSyncCache {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub root: String,
    pub state_folder: String,
    pub book_folder: String,
    pub book_versions: HashMap<String, HashMap<String, String>>,
}

#[derive(Default)]
pub struct GoogleDriveSyncManager {
    pub error_message: Option<String>,
    pub last_sync: Option<i64>,
    pub cache: GoogleDriveSyncCache,
    pub(super) remote_books: HashMap<String, (HashMap<String, String>, SyncBook)>,
    state_task: Option<SyncTask>,
    file_transfer_task: Option<SyncTask>,
    poll_task: Option<SyncTask>,
    debounce_task: Option<SyncTask>,
    pub download_task: Option<SyncTask>,
    stopped: bool,
    pub(super) unsupported_format: bool,
    pub(super) transfers: Vec<QueueItem>,
    pub(super) progress: Option<Progress>,
    pub(super) book_errors: HashMap<(String, Phase), BookError>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Phase {
    State,
    File,
}

pub(super) struct BookError {
    title: String,
    pub(super) message: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub key: String,
    pub title: String,
    pub direction: Option<Direction>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Upload,
    Download,
    Both,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_sync: Option<i64>,
    pub is_syncing: bool,
    pub error_message: Option<String>,
    pub queue: Vec<QueueItem>,
    pub progress: Option<Progress>,
}

pub(super) fn record_book(key: &str, phase: Phase, result: SyncResult<()>) -> SyncResult<()> {
    let Err(error) = result else {
        shared().book_errors.remove(&(key.to_string(), phase));
        return Ok(());
    };
    if task::is_cancelled() || error.stops_run() {
        return Err(error);
    }
    let deleted = store()
        .state
        .books
        .get(key)
        .is_some_and(|record| record.deleted);
    let title = book_title(key, deleted);
    shared().book_errors.insert(
        (key.to_string(), phase),
        BookError {
            title,
            message: error.to_string(),
        },
    );
    Ok(())
}

static SHARED: LazyLock<Mutex<GoogleDriveSyncManager>> =
    LazyLock::new(|| Mutex::new(GoogleDriveSyncManager::default()));

pub fn shared() -> MutexGuard<'static, GoogleDriveSyncManager> {
    SHARED.lock().unwrap()
}

pub(super) fn store() -> MutexGuard<'static, SyncStorage> {
    storage::shared()
}

pub fn status() -> SyncStatus {
    let manager = shared();
    SyncStatus {
        last_sync: manager.last_sync,
        is_syncing: manager.state_task.is_some() || manager.file_transfer_task.is_some(),
        error_message: manager.error_message.clone(),
        queue: queue(&manager),
        progress: manager.progress.clone(),
    }
}

fn queue(manager: &GoogleDriveSyncManager) -> Vec<QueueItem> {
    let mut queue = manager.transfers.clone();
    let mut errors: Vec<_> = manager.book_errors.iter().collect();
    errors.sort_by(|a, b| a.0.cmp(b.0));
    for ((key, _), error) in errors {
        match queue.iter_mut().find(|item| &item.key == key) {
            Some(item) => {
                item.error.get_or_insert_with(|| error.message.clone());
            }
            None => queue.push(QueueItem {
                key: key.clone(),
                title: error.title.clone(),
                direction: None,
                error: Some(error.message.clone()),
            }),
        }
    }
    queue
}

pub(super) fn book_title(key: &str, deleted: bool) -> String {
    library::load_metadata_at(&SyncStorage::book_directory(key, deleted))
        .map(|metadata| metadata.title)
        .unwrap_or_else(|| key.to_string())
}

fn fail_run(manager: &mut GoogleDriveSyncManager, error: &SyncError) {
    manager.error_message = Some(error.to_string());
    manager.unsupported_format |= error.is_format_error();
}

pub(super) fn publish() {
    app().emit("sync://status", status()).ok();
}

fn enabled_with(manager: &GoogleDriveSyncManager) -> bool {
    crate::sync::settings().enable_sync && auth::is_authenticated() && !manager.stopped
}

pub fn enabled() -> bool {
    enabled_with(&shared())
}

pub fn init() {
    shared().cache = std::fs::read(cache_url())
        .ok()
        .and_then(|data| sync_format::decode(&data).ok())
        .unwrap_or_default();
    store().prepare_library().ok();
}

pub fn start() {
    client::resume();
    let previous = {
        let mut manager = shared();
        manager.stopped = false;
        manager.poll_task.take()
    };
    if let Some(previous) = previous {
        previous.cancel();
    }

    if !enabled() {
        return;
    }
    let poll_task = SyncTask::spawn(async {
        sync(None).await;
        while !task::is_cancelled() {
            if task::sleep(120).await.is_err() {
                return;
            }
            sync(None).await;
        }
    });
    shared().poll_task = Some(poll_task.clone());
    poll_task.start();
}

pub async fn pause() {
    if let Some(poll_task) = shared().poll_task.take() {
        poll_task.cancel();
    }
    sync(None).await;
}

pub async fn stop() {
    let (state_task, file_transfer_task, download_task) = {
        let mut manager = shared();
        manager.stopped = true;
        if let Some(poll_task) = &manager.poll_task {
            poll_task.cancel();
        }
        if let Some(debounce_task) = manager.debounce_task.take() {
            debounce_task.cancel();
        }
        if let Some(state_task) = &manager.state_task {
            state_task.cancel();
        }
        if let Some(file_transfer_task) = &manager.file_transfer_task {
            file_transfer_task.cancel();
        }
        if let Some(download_task) = &manager.download_task {
            download_task.cancel();
        }
        (
            manager.state_task.clone(),
            manager.file_transfer_task.clone(),
            manager.download_task.clone(),
        )
    };

    client::stop();
    if let Some(state_task) = state_task {
        state_task.value().await;
    }
    if let Some(file_transfer_task) = file_transfer_task {
        file_transfer_task.value().await;
    }
    if let Some(download_task) = download_task {
        download_task.value().await;
    }

    let mut manager = shared();
    manager.state_task = None;
    manager.file_transfer_task = None;
    manager.download_task = None;
    manager.transfers.clear();
    manager.progress = None;
    manager.book_errors.clear();
    drop(manager);
    publish();
}

pub async fn sign_out() -> SyncResult<()> {
    stop().await;
    reset_connection(false)?;
    auth::clear_tokens();
    Ok(())
}

pub async fn clear_cache() -> SyncResult<()> {
    stop().await;
    shared().cache = GoogleDriveSyncCache::default();
    save_cache()?;

    start();
    Ok(())
}

pub fn reset_connection(restoring_backup: bool) -> SyncResult<()> {
    if restoring_backup {
        store().reload();
    }

    store().prepare_library()?;
    remove_placeholders()?;
    store().reset_sync_state()?;
    {
        let mut manager = shared();
        manager.cache = GoogleDriveSyncCache::default();
        manager.unsupported_format = false;
        manager.error_message = None;
        manager.last_sync = None;
    }
    save_cache()?;
    publish();
    Ok(())
}

pub fn schedule() {
    if !enabled() {
        return;
    }
    let mut manager = shared();
    if manager.state_task.is_some() || manager.debounce_task.is_some() {
        return;
    }
    let debounce_task = SyncTask::spawn(async {
        task::sleep(30).await.ok();
        if task::is_cancelled() {
            return;
        }
        shared().debounce_task = None;
        sync(None).await;
    });
    manager.debounce_task = Some(debounce_task.clone());
    drop(manager);
    debounce_task.start();
}

pub async fn sync(book: Option<BookMetadata>) {
    if !enabled() {
        return;
    }
    let previous = shared().state_task.clone();
    if book.is_none()
        && let Some(previous) = previous
    {
        previous.value().await;
        return;
    }

    let previous_file_transfers = {
        let mut manager = shared();
        if let Some(debounce_task) = manager.debounce_task.take() {
            debounce_task.cancel();
        }
        if book.is_none() {
            None
        } else {
            manager.file_transfer_task.clone()
        }
    };
    if book.is_some() {
        if let Some(previous) = &previous {
            previous.cancel();
        }
        if let Some(previous_file_transfers) = &previous_file_transfers {
            previous_file_transfers.cancel();
        }
    }

    let is_book = book.is_some();
    let task = SyncTask::spawn(async move {
        if let Some(previous) = &previous {
            previous.value().await;
        }
        if let Some(previous_file_transfers) = &previous_file_transfers {
            previous_file_transfers.value().await;
        }

        if let Err(error) = state::run(book).await
            && !task::is_cancelled()
        {
            let mut manager = shared();
            fail_run(&mut manager, &error);
            if let Some(file_transfer_task) = &manager.file_transfer_task {
                file_transfer_task.cancel();
            }
        }
    });

    shared().state_task = Some(task.clone());
    task.start();
    publish();
    task.value().await;

    if !task.is_cancelled() {
        let succeeded = {
            let mut manager = shared();
            manager.state_task = None;
            manager.error_message.is_none()
                && !manager
                    .book_errors
                    .keys()
                    .any(|(_, phase)| *phase == Phase::State)
        };
        publish();

        let pending = {
            let store = store();
            store.state.books.values().any(|record| record.pending) || store.state.shelves_pending
        };
        if is_book || (succeeded && pending) {
            schedule();
        }
        if !is_book {
            start_file_sync();
        }
    }
}

pub fn start_file_sync() {
    let mut manager = shared();
    if !enabled_with(&manager)
        || manager.unsupported_format
        || manager.error_message.is_some()
        || manager.state_task.is_some()
        || manager.file_transfer_task.is_some()
        || manager.download_task.is_some()
        || manager.cache.book_folder.is_empty()
    {
        return;
    }
    let file_transfer_task = SyncTask::spawn(async {
        let result = files::run().await;
        {
            let mut manager = shared();
            manager.file_transfer_task = None;
            manager.progress = None;
            if let Err(error) = result
                && !task::is_cancelled()
            {
                fail_run(&mut manager, &error);
            }
        }
        publish();
    });
    manager.file_transfer_task = Some(file_transfer_task.clone());
    drop(manager);
    file_transfer_task.start();
}

pub(super) fn state_folder() -> String {
    shared().cache.state_folder.clone()
}

pub(super) fn book_folder() -> String {
    shared().cache.book_folder.clone()
}

fn remove_placeholders() -> SyncResult<()> {
    for book in library::load_all_books(app()) {
        if book.epub.is_some() {
            continue;
        }
        let root = SyncStorage::book_directory(&book.folder, false);
        if statistics::load(&root)
            .values()
            .any(|change| change.value.is_some())
        {
            statistics::archive(app(), &book)?;
        } else {
            let key: String = book.folder.nfc().collect();
            store().state.books.remove(&key);
        }
        library::delete(&root)?;
    }

    storage::post_books_changed();
    Ok(())
}

fn cache_url() -> std::path::PathBuf {
    library::app_dir(app()).join("drive-sync.json")
}

pub(super) fn save_cache() -> SyncResult<()> {
    let data = sync_format::encode(&shared().cache)?;
    storage::write_atomic(&cache_url(), &data)
}
