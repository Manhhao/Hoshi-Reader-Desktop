use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::statistics;
use crate::sync::client::{self, Meter};
use crate::sync::gdrive::handler as drive;
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
    #[serde(default)]
    pub book_folders: HashMap<String, String>,
    #[serde(default)]
    pub attach_pending: bool,
    #[serde(default)]
    pub replaced_root: bool,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub upload_sessions: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum AttachChoice {
    Merge,
    Device,
    Cloud,
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
    stopped: bool,
    pub(super) unsupported_format: bool,
    pub(super) jobs: VecDeque<Job>,
    pub(super) active: usize,
    pub(super) epubs: usize,
    pub(super) priority: usize,
    pub(super) downloads: HashSet<String>,
    pub(super) locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
    pub(super) transfers: Vec<QueueItem>,
    pub(super) progress: Option<Progress>,
    pub(super) book_errors: HashMap<(String, Phase), BookError>,
    pub(super) moving: HashMap<String, Moving>,
    pub(super) interrupted: HashMap<String, Transfer>,
    pub(super) upload_flow: Flow,
    pub(super) download_flow: Flow,
}

pub(super) struct Moving {
    pub(super) direction: Direction,
    pub(super) meter: Arc<Meter>,
    pub(super) total: u64,
}

#[derive(Default)]
pub(super) struct Flow {
    pub(super) done: u64,
    pub(super) total: u64,
    pub(super) counted: HashSet<String>,
    speed: f64,
    sampled: Option<(Instant, u64)>,
    started: Option<Instant>,
}

impl Flow {
    fn view(&self, moving: u64) -> Option<FlowView> {
        let done = (self.done + moving).min(self.total);
        if done >= self.total {
            return None;
        }
        let remaining = self
            .started
            .filter(|started| started.elapsed() >= Duration::from_secs(60) && self.speed > 1.0)
            .map(|_| ((self.total - done) as f64 / self.speed) as u64);
        Some(FlowView {
            done,
            total: self.total,
            speed: self.speed as u64,
            remaining,
        })
    }

    fn sample(&mut self, moving: u64) {
        let now = Instant::now();
        let done = self.done + moving;
        if let Some((at, previous)) = self.sampled {
            let elapsed = now.duration_since(at).as_secs_f64();
            if elapsed > 0.0 {
                let rate = done.saturating_sub(previous) as f64 / elapsed;
                self.speed += (rate - self.speed) * (elapsed / 30.0).min(1.0);
            }
        }
        if done > 0 {
            self.started.get_or_insert(now);
        }
        self.sampled = Some((now, done));
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct Job {
    pub(super) key: String,
    pub(super) epub: bool,
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
    pub book_id: Option<String>,
    pub cover: bool,
    pub size: Option<u64>,
    pub direction: Option<Direction>,
    pub error: Option<String>,
    pub transfer: Option<Transfer>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub direction: Direction,
    pub done: u64,
    pub total: u64,
    pub state: TransferState,
}

#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransferState {
    Moving,
    Interrupted,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct FlowView {
    pub done: u64,
    pub total: u64,
    pub speed: u64,
    pub remaining: Option<u64>,
}

#[derive(Serialize, Clone, Copy, PartialEq)]
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
    pub upload: Option<FlowView>,
    pub download: Option<FlowView>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_sync: Option<i64>,
    pub is_syncing: bool,
    pub error_message: Option<String>,
    pub queue: Vec<QueueItem>,
    pub progress: Option<Progress>,
    pub attach_pending: bool,
    pub missing_books: usize,
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
    let missing_books = files::missing_books().len();
    let manager = shared();
    SyncStatus {
        last_sync: manager.last_sync,
        is_syncing: manager.state_task.is_some() || manager.file_transfer_task.is_some(),
        error_message: manager.error_message.clone(),
        queue: queue(&manager),
        progress: manager.progress.clone().map(|mut progress| {
            progress.upload = manager
                .upload_flow
                .view(moving(&manager, Direction::Upload));
            progress.download = manager
                .download_flow
                .view(moving(&manager, Direction::Download));
            progress
        }),
        attach_pending: manager.cache.attach_pending,
        missing_books,
    }
}

fn moving(manager: &GoogleDriveSyncManager, direction: Direction) -> u64 {
    manager
        .moving
        .values()
        .filter(|moving| moving.direction == direction)
        .map(|moving| moving.meter.bytes())
        .sum()
}

pub(super) fn sample() {
    let mut manager = shared();
    let up = moving(&manager, Direction::Upload);
    let down = moving(&manager, Direction::Download);
    manager.upload_flow.sample(up);
    manager.download_flow.sample(down);
}

fn transfer(manager: &GoogleDriveSyncManager, key: &str) -> Option<Transfer> {
    if let Some(moving) = manager.moving.get(key) {
        return Some(Transfer {
            direction: moving.direction,
            done: moving.meter.bytes().min(moving.total),
            total: moving.total,
            state: TransferState::Moving,
        });
    }
    manager.interrupted.get(key).copied()
}

fn queue(manager: &GoogleDriveSyncManager) -> Vec<QueueItem> {
    let mut queue = manager.transfers.clone();
    for item in &mut queue {
        item.transfer = transfer(manager, &item.key);
    }
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
                book_id: None,
                cover: false,
                size: None,
                direction: None,
                error: Some(error.message.clone()),
                transfer: None,
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
    crate::sync::settings().enable_sync
        && auth::is_authenticated()
        && !manager.stopped
        && !manager.cache.attach_pending
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
            if task::sleep(Duration::from_secs(120)).await.is_err() {
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
    let (state_task, file_transfer_task) = {
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
        (
            manager.state_task.clone(),
            manager.file_transfer_task.clone(),
        )
    };

    client::stop();
    if let Some(state_task) = state_task {
        state_task.value().await;
    }
    if let Some(file_transfer_task) = file_transfer_task {
        file_transfer_task.value().await;
    }

    let mut manager = shared();
    manager.state_task = None;
    manager.file_transfer_task = None;
    manager.downloads.clear();
    manager.transfers.clear();
    manager.progress = None;
    manager.book_errors.clear();
    manager.moving.clear();
    manager.interrupted.clear();
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
    {
        let mut manager = shared();
        manager.cache = GoogleDriveSyncCache {
            root: std::mem::take(&mut manager.cache.root),
            attach_pending: manager.cache.attach_pending,
            replaced_root: manager.cache.replaced_root,
            ..Default::default()
        };
    }
    save_cache()?;

    start();
    Ok(())
}

pub(super) fn set_attach_pending() {
    let mut manager = shared();
    manager.cache.attach_pending = true;
    if let Some(file_transfer_task) = &manager.file_transfer_task {
        file_transfer_task.cancel();
    }
}

pub async fn attach(choice: AttachChoice) -> SyncResult<()> {
    stop().await;
    client::resume();
    let result = resolve_attach(choice).await;
    start();
    publish();
    result
}

async fn resolve_attach(choice: AttachChoice) -> SyncResult<()> {
    if let AttachChoice::Device = choice {
        {
            let mut manager = shared();
            manager.cache.attach_pending = false;
            manager.cache.replaced_root = true;
        }
        save_cache()?;
        if let Err(error) = drive::rename_root().await {
            shared().cache.attach_pending = true;
            save_cache()?;
            return Err(error.into());
        }
        store().remove_deletion_records()?;
    }
    let layout = drive::layout().await?;
    if let AttachChoice::Cloud = choice {
        clear_library()?;
    }
    {
        let mut manager = shared();
        manager.cache.root = layout.root;
        manager.cache.state_folder = layout.state;
        manager.cache.book_folder = layout.books;
        manager.cache.attach_pending = false;
    }
    save_cache()
}

fn clear_library() -> SyncResult<()> {
    for book in library::load_all_books(app()) {
        let key: String = book.folder.nfc().collect();
        app()
            .emit("sync://book-replaced", json!({ "key": key }))
            .ok();
    }
    let books = library::books_dir(app());
    std::fs::remove_dir_all(&books)?;
    std::fs::create_dir_all(&books)?;
    store().reload();
    storage::post_books_changed();
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
        manager.cache = GoogleDriveSyncCache {
            replaced_root: manager.cache.replaced_root,
            ..Default::default()
        };
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
        task::sleep(Duration::from_secs(30)).await.ok();
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

    if let Some(debounce_task) = shared().debounce_task.take() {
        debounce_task.cancel();
    }
    if book.is_some()
        && let Some(previous) = &previous
    {
        previous.cancel();
    }

    let is_book = book.is_some();
    let task = SyncTask::spawn(async move {
        if let Some(previous) = &previous {
            previous.value().await;
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
        start_file_sync();
    }
}

pub fn start_file_sync() {
    let mut manager = shared();
    if !enabled_with(&manager)
        || manager.unsupported_format
        || manager.error_message.is_some()
        || manager.state_task.is_some()
        || manager.cache.book_folder.is_empty()
    {
        return;
    }
    if manager.file_transfer_task.is_some() {
        drop(manager);
        files::enqueue(false);
        return;
    }
    let file_transfer_task = SyncTask::spawn(async {
        let result = files::run().await;
        let queued = {
            let mut manager = shared();
            manager.file_transfer_task = None;
            manager.progress = None;
            if let Err(error) = &result
                && !task::is_cancelled()
            {
                fail_run(&mut manager, error);
            }
            !std::mem::take(&mut manager.jobs).is_empty()
        };
        publish();
        if task::is_cancelled() {
            return;
        }
        if result.is_ok_and(|published| published) {
            tauri::async_runtime::spawn(sync(None));
        } else if queued {
            start_file_sync();
        }
    });
    manager.file_transfer_task = Some(file_transfer_task.clone());
    drop(manager);
    file_transfer_task.start();
}

pub fn download_all() {
    let keys = files::missing_books();
    shared().downloads.extend(keys);
    start_file_sync();
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
