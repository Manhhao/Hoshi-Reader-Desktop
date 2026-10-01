use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::statistics;
use crate::sync::client::{self, GoogleDriveError, GoogleDriveFile};
use crate::sync::gdrive::handler as drive;
use crate::sync::model::{
    SyncBook, SyncError, SyncFileType, SyncResult, SyncShelves, Timestamped, sync_format,
};
use crate::sync::storage::{self, SASAYAKI_MATCH, SyncRecord, SyncStorage};
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
    remote_books: HashMap<String, (HashMap<String, String>, SyncBook)>,
    listed_files: HashMap<(String, String), GoogleDriveFile>,
    state_task: Option<SyncTask>,
    file_transfer_task: Option<SyncTask>,
    poll_task: Option<SyncTask>,
    debounce_task: Option<SyncTask>,
    pub download_task: Option<SyncTask>,
    stopped: bool,
    unsupported_format: bool,
    transfers: Vec<QueueItem>,
    progress: Option<Progress>,
    book_errors: HashMap<(String, Phase), BookError>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Phase {
    State,
    File,
}

struct BookError {
    title: String,
    message: String,
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

struct RemoteChanges {
    listed: Option<HashMap<String, Vec<GoogleDriveFile>>>,
    changed: HashSet<String>,
    cursor: String,
}

impl RemoteChanges {
    fn contains(&self, key: &str) -> bool {
        self.changed.contains(key)
            || self
                .listed
                .as_ref()
                .is_some_and(|listed| listed.contains_key(key))
    }

    fn files(&self, key: &str) -> Option<Vec<GoogleDriveFile>> {
        if self.changed.contains(key) {
            return None;
        }
        self.listed
            .as_ref()
            .map(|listed| listed.get(key).cloned().unwrap_or_default())
    }
}

fn record_book(key: &str, phase: Phase, result: SyncResult<()>) -> SyncResult<()> {
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

type Folders = HashMap<(String, i64), String>;

static SHARED: LazyLock<Mutex<GoogleDriveSyncManager>> =
    LazyLock::new(|| Mutex::new(GoogleDriveSyncManager::default()));

pub fn shared() -> MutexGuard<'static, GoogleDriveSyncManager> {
    SHARED.lock().unwrap()
}

fn store() -> MutexGuard<'static, SyncStorage> {
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

fn book_title(key: &str, deleted: bool) -> String {
    library::load_metadata_at(&SyncStorage::book_directory(key, deleted))
        .map(|metadata| metadata.title)
        .unwrap_or_else(|| key.to_string())
}

fn fail_run(manager: &mut GoogleDriveSyncManager, error: &SyncError) {
    manager.error_message = Some(error.to_string());
    manager.unsupported_format |= error.is_format_error();
}

fn publish() {
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

        if let Err(error) = run_sync(book).await
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

async fn run_sync(book: Option<BookMetadata>) -> SyncResult<()> {
    task::check_cancellation()?;
    shared().error_message = None;
    publish();

    if let Some(book) = book {
        if shared().cache.state_folder.is_empty() {
            load_layout().await?;
        }

        let key: String = book.folder.nfc().collect();
        return record_book(&key, Phase::State, sync_book(&key, None).await);
    }

    let remote = changes().await?;
    let pending: Vec<String> = store()
        .state
        .books
        .iter()
        .filter(|(_, record)| record.pending)
        .map(|(key, _)| key.clone())
        .collect();
    let keys: BTreeSet<String> = remote
        .changed
        .iter()
        .chain(remote.listed.iter().flat_map(HashMap::keys))
        .cloned()
        .chain(pending)
        .filter(|key| key != ".shelves")
        .collect();
    prefetch(&remote, &keys).await?;
    for key in &keys {
        record_book(key, Phase::State, sync_book(key, remote.files(key)).await)?;
    }

    let failed = {
        let manager = shared();
        keys.iter().any(|key| {
            manager
                .book_errors
                .contains_key(&(key.clone(), Phase::State))
        })
    };
    let unattached = store()
        .state
        .books
        .values()
        .any(|record| !record.attached && !record.deleted);
    if failed || unattached {
        return Ok(());
    }
    if remote.contains(".shelves") || store().state.shelves_pending {
        sync_shelves().await?;
    }
    shared().cache.cursor = Some(remote.cursor);
    save_cache()?;
    {
        let mut manager = shared();
        manager.last_sync = Some(library::now_ms());
        manager.unsupported_format = false;
    }
    publish();
    Ok(())
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
        let result = run_file_sync().await;
        {
            let mut manager = shared();
            manager.file_transfer_task = None;
            manager.progress = None;
            manager.listed_files.clear();
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

async fn run_file_sync() -> SyncResult<()> {
    let mut keys: Vec<String> = store().state.books.keys().cloned().collect();
    keys.sort();
    begin_transfers(&keys);
    if shared().progress.is_some() {
        let files = list_files().await?;
        shared().listed_files = files;
    }
    let transfers = keys
        .into_iter()
        .map(|key| async move {
            task::check_cancellation()?;
            set_current_transfer(&key);
            let result = sync_files(&key, &mut HashMap::new()).await;
            record_book(&key, Phase::File, result)?;
            finish_transfer(&key);
            Ok(())
        })
        .collect();
    task::concurrent(transfers).await
}

async fn list_files() -> SyncResult<HashMap<(String, String), GoogleDriveFile>> {
    let mut listed = HashMap::new();
    for file in drive::list("'me' in owners").await? {
        if let Some(parent) = file.parents.as_ref().and_then(|parents| parents.first()) {
            listed
                .entry((parent.clone(), file.name.clone()))
                .or_insert(file);
        }
    }
    Ok(listed)
}

fn listed_file(parent: &str, name: &str) -> Option<GoogleDriveFile> {
    shared()
        .listed_files
        .get(&(parent.to_string(), name.to_string()))
        .cloned()
}

async fn sync_files(key: &str, folders: &mut Folders) -> SyncResult<()> {
    let mut result = Ok(());
    for file_type in SyncFileType::ALL_CASES {
        task::check_cancellation()?;
        let transferred = async {
            upload_file(key, file_type, folders).await?;
            if file_type != SyncFileType::Epub {
                download_file(key, file_type, &|_| {}, folders).await?;
            }
            SyncResult::Ok(())
        }
        .await;
        result = result.and(transferred);
    }
    result.and(cleanup_files(key, folders).await)
}

fn transfer_direction(record: &SyncRecord) -> Option<Direction> {
    let (upload, download) = SyncFileType::ALL_CASES
        .into_iter()
        .filter(|&file_type| !record.deleted || file_type == SyncFileType::Cover)
        .map(|file_type| {
            let source = record.sources.get(&file_type).copied();
            let published = record.files.get(&file_type).map(|file| file.modified);
            (
                record.attached
                    && source
                        .is_some_and(|source| published.is_none_or(|published| published < source)),
                file_type != SyncFileType::Epub
                    && published
                        .is_some_and(|published| source.is_none_or(|source| source < published)),
            )
        })
        .fold(
            (false, false),
            |(upload, download), (file_upload, file_download)| {
                (upload || file_upload, download || file_download)
            },
        );
    match (upload, download) {
        (true, true) => Some(Direction::Both),
        (true, false) => Some(Direction::Upload),
        (false, true) => Some(Direction::Download),
        (false, false) => None,
    }
}

fn begin_transfers(keys: &[String]) {
    let transfers: Vec<QueueItem> = {
        let store = store();
        keys.iter()
            .filter_map(|key| {
                let record = &store.state.books[key];
                transfer_direction(record).map(|direction| QueueItem {
                    key: key.clone(),
                    title: book_title(key, record.deleted),
                    direction: Some(direction),
                    error: None,
                })
            })
            .collect()
    };
    {
        let mut manager = shared();
        manager.progress = (!transfers.is_empty()).then(|| Progress {
            done: 0,
            total: transfers.len(),
            current: Vec::new(),
        });
        manager.transfers = transfers;
    }
    publish();
}

fn set_current_transfer(key: &str) {
    if let Some(progress) = &mut shared().progress {
        progress.current.push(key.to_string());
    }
    publish();
}

fn finish_transfer(key: &str) {
    {
        let mut manager = shared();
        if let Some(progress) = &mut manager.progress {
            progress.current.retain(|current| current != key);
        }
        if !manager.transfers.iter().any(|item| item.key == key) {
            return;
        }
        if let Some(progress) = &mut manager.progress {
            progress.done += 1;
        }
        if !manager
            .book_errors
            .contains_key(&(key.to_string(), Phase::File))
        {
            manager.transfers.retain(|item| item.key != key);
        }
    }
    publish();
}

pub async fn download_book(
    book: &BookMetadata,
    on_progress: &(dyn Fn(f64) + Sync),
) -> SyncResult<BookMetadata> {
    let key: String = book.folder.nfc().collect();
    sync(Some(book.clone())).await;
    let (unsupported_format, error_message) = {
        let manager = shared();
        let book_error = manager.book_errors.get(&(key.clone(), Phase::State));
        (
            manager.unsupported_format,
            manager
                .error_message
                .clone()
                .or_else(|| book_error.map(|error| error.message.clone())),
        )
    };
    if unsupported_format {
        return Err(SyncError::UnsupportedVersion);
    }
    if let Some(error_message) = error_message {
        return Err(GoogleDriveError::Api(error_message).into());
    }

    task::check_cancellation()?;

    let root = SyncStorage::book_directory(&book.folder, false);
    if store()
        .state
        .books
        .get(&key)
        .is_some_and(|record| record.deleted)
    {
        return Err(GoogleDriveError::Api("This book was deleted.".to_string()).into());
    }

    let has_epub = store()
        .state
        .books
        .get(&key)
        .and_then(|record| record.files.get(&SyncFileType::Epub))
        .is_some_and(|file| file.value.is_some());
    if enabled() && has_epub {
        download_file(&key, SyncFileType::Epub, on_progress, &mut HashMap::new()).await?;
    }

    task::check_cancellation()?;

    let metadata = library::load_metadata_at(&root).unwrap_or_else(|| book.clone());
    if metadata.epub.is_none() {
        return Err(
            GoogleDriveError::Api("This book has not been uploaded yet.".to_string()).into(),
        );
    }
    Ok(metadata)
}

async fn changes() -> SyncResult<RemoteChanges> {
    let mut listed = None;
    let mut changed = HashSet::new();
    let saved = shared().cache.cursor.clone();
    let mut cursor = match saved {
        Some(saved) => saved,
        None => {
            let cursor = drive::start_token().await?;
            listed = Some(list_remote().await?);
            cursor
        }
    };
    loop {
        let page = drive::changes(&cursor).await?;
        task::check_cancellation()?;
        let root = shared().cache.root.clone();
        if page.changes.iter().any(|change| {
            change.file.as_ref().is_some_and(|file| {
                file.is_folder()
                    && (file.name == "Hoshi Reader"
                        || file
                            .parents
                            .as_ref()
                            .is_some_and(|parents| parents.contains(&root)))
            })
        }) {
            listed = Some(list_remote().await?);
        }
        let state_folder = state_folder();
        changed.extend(
            page.changes
                .iter()
                .filter(|change| !change.removed)
                .filter_map(|change| change.file.as_ref())
                .filter(|file| {
                    file.trashed != Some(true)
                        && file
                            .parents
                            .as_ref()
                            .is_some_and(|parents| parents.contains(&state_folder))
                })
                .filter_map(|file| file.state_key()),
        );
        match page.next_page_token {
            Some(next) => cursor = next,
            None => {
                return Ok(RemoteChanges {
                    listed,
                    changed,
                    cursor: page.new_start_page_token.unwrap(),
                });
            }
        }
    }
}

async fn load_layout() -> SyncResult<()> {
    let layout = drive::layout().await?;
    task::check_cancellation()?;
    let mut manager = shared();
    manager.cache.root = layout.root;
    manager.cache.state_folder = layout.state;
    manager.cache.book_folder = layout.books;
    Ok(())
}

async fn list_remote() -> SyncResult<HashMap<String, Vec<GoogleDriveFile>>> {
    load_layout().await?;
    let state_folder = shared().cache.state_folder.clone();
    let files = drive::children(&state_folder, None).await?;
    task::check_cancellation()?;
    let mut grouped: HashMap<String, Vec<GoogleDriveFile>> = HashMap::new();
    for file in files {
        if let Some(key) = file.state_key() {
            grouped.entry(key).or_default().push(file);
        }
    }
    Ok(grouped)
}

fn state_folder() -> String {
    shared().cache.state_folder.clone()
}

fn book_folder() -> String {
    shared().cache.book_folder.clone()
}

async fn sync_book(key: &str, listed: Option<Vec<GoogleDriveFile>>) -> SyncResult<()> {
    let files = match listed {
        Some(files) => files,
        None => drive::children(&state_folder(), Some(&format!("{key}.json"))).await?,
    };
    task::check_cancellation()?;

    let mut versions = file_versions(&files);
    if unchanged(key, &files) {
        return Ok(());
    }
    if shared().cache.book_versions.remove(key).is_some() {
        save_cache()?;
    }

    let cached = shared()
        .remote_books
        .get(key)
        .filter(|(cached, _)| *cached == versions)
        .map(|(_, book)| book.clone());
    let mut remote = match cached {
        Some(book) => Some(book),
        None => read_state(&files, SyncBook::merge).await?,
    };
    merge_book(key, remote.as_ref())?;

    let Some(book) = store().load_book(key, remote.as_ref()) else {
        let mut store = store();
        if let Some(record) = store.state.books.get_mut(key) {
            record.pending = false;
            record.cleanup.clear();
            store.save()?;
        }
        return Ok(());
    };

    if book.needs_upload(remote.as_ref()) || files.len() > 1 {
        let written = write_state(&book, &format!("{key}.json"), &files).await?;
        versions = HashMap::from([(written.id, written.version)]);
        remote = Some(book.clone());
    }

    {
        let mut store = store();
        if store.state.books[key].pending
            && store.load_book(key, remote.as_ref()).as_ref() == Some(&book)
        {
            store.state.books.get_mut(key).unwrap().pending = false;
            store.save()?;
        }
    }
    shared()
        .remote_books
        .insert(key.to_string(), (versions.clone(), remote.unwrap()));
    shared()
        .cache
        .book_versions
        .insert(key.to_string(), versions);
    save_cache()
}

fn file_versions(files: &[GoogleDriveFile]) -> HashMap<String, String> {
    files
        .iter()
        .map(|file| (file.id.clone(), file.version.clone()))
        .collect()
}

fn unchanged(key: &str, files: &[GoogleDriveFile]) -> bool {
    files.len() == 1
        && store()
            .state
            .books
            .get(key)
            .is_some_and(|record| !record.pending)
        && shared().cache.book_versions.get(key) == Some(&file_versions(files))
}

async fn prefetch(remote: &RemoteChanges, keys: &BTreeSet<String>) -> SyncResult<()> {
    let downloads = keys
        .iter()
        .filter_map(|key| {
            let files = remote.files(key).filter(|files| !files.is_empty())?;
            let versions = file_versions(&files);
            if unchanged(key, &files)
                || shared()
                    .remote_books
                    .get(key)
                    .is_some_and(|(cached, _)| *cached == versions)
            {
                return None;
            }
            let key = key.clone();
            Some(async move {
                match read_state(&files, SyncBook::merge).await {
                    Ok(Some(book)) => {
                        shared().remote_books.insert(key, (versions, book));
                    }
                    Err(error) if error.stops_run() => return Err(error),
                    _ => {}
                }
                Ok(())
            })
        })
        .collect();
    task::concurrent(downloads).await
}

async fn read_state<T: serde::de::DeserializeOwned>(
    files: &[GoogleDriveFile],
    merge: impl Fn(&T, &T) -> T,
) -> SyncResult<Option<T>> {
    let mut state: Option<T> = None;

    for file in files {
        let data = drive::read(file).await?;
        task::check_cancellation()?;
        let incoming: T = sync_format::decode(&data)?;
        state = Some(match state {
            Some(state) => merge(&state, &incoming),
            None => incoming,
        });
    }
    Ok(state)
}

async fn write_state<T: Serialize>(
    state: &T,
    name: &str,
    files: &[GoogleDriveFile],
) -> SyncResult<GoogleDriveFile> {
    let written = client::write(
        sync_format::encode(state)?,
        name,
        &state_folder(),
        files.first().map(|file| file.id.as_str()),
        "application/octet-stream",
    )
    .await?;
    for duplicate in files.iter().skip(1) {
        task::check_cancellation()?;
        drive::trash(duplicate).await?;
    }
    Ok(written)
}

fn merge_book(key: &str, remote: Option<&SyncBook>) -> SyncResult<()> {
    let mut store = store();
    let root = SyncStorage::resolve_book_directory(key);
    if library::load_metadata_at(&root).is_some() {
        store.prepare_book(&root);
    }
    let (Some(remote), Some(book)) = (remote, store.state.books.get(key).cloned()) else {
        let merged = match remote {
            Some(remote) => Some(remote.clone()),
            None => store.load_book(key, None),
        };
        if let Some(merged) = merged {
            store.apply_book(key, &merged)?;
        }
        return Ok(());
    };

    let replaced = remote.generation > book.generation && (book.attached || book.deleted);
    if replaced || (remote.deleted && remote.generation >= book.generation) {
        app()
            .emit("sync://book-replaced", json!({ "key": key }))
            .ok();
    }

    let mut local = store.load_book(key, Some(remote)).unwrap();
    if replaced {
        store.remove_book_files(key)?;
        store
            .state
            .books
            .get_mut(key)
            .unwrap()
            .cleanup
            .insert(book.generation);
    }
    if !book.attached && book.generation == 0 {
        local.metadata = remote.metadata.clone();
    }
    if !book.attached && !book.deleted && !remote.deleted {
        local.generation = remote.generation;
    }
    store.apply_book(key, &SyncBook::merge(&local, remote))
}

async fn sync_shelves() -> SyncResult<()> {
    let files = drive::children(&state_folder(), Some(".shelves.json")).await?;
    let remote = read_state(&files, SyncShelves::merge).await?;
    let local = SyncShelves::new(library::load_shelf_list(app()));
    let merged = match &remote {
        Some(remote) => SyncShelves::merge(remote, &local),
        None => local,
    };
    store().apply_shelves(&merged.shelves)?;
    if (Some(&merged) != remote.as_ref() && !merged.shelves.is_empty()) || files.len() > 1 {
        write_state(&merged, ".shelves.json", &files).await?;
    }

    let mut store = store();
    store.state.shelves_pending = library::load_shelf_list(app()) != merged.shelves;
    store.save()
}

async fn upload_file(key: &str, file_type: SyncFileType, folders: &mut Folders) -> SyncResult<()> {
    let record = store().state.books[key].clone();
    if !record.attached || (record.deleted && file_type != SyncFileType::Cover) {
        return Ok(());
    }

    let Some(&source) = record.sources.get(&file_type) else {
        return Ok(());
    };
    if record
        .files
        .get(&file_type)
        .is_some_and(|published| published.modified >= source)
    {
        return Ok(());
    }

    let url = store().source_url(key, file_type);
    let Some(url) = url else {
        let mut store = store();
        let record = store.state.books.get_mut(key).unwrap();
        record.files.insert(
            file_type,
            Timestamped {
                modified: source,
                value: None,
            },
        );
        record.pending = true;
        return store.save_changes(false);
    };

    let file_name: String = url.file_name().unwrap().to_string_lossy().nfc().collect();
    let name = if file_type == SyncFileType::Sasayaki {
        format!("{source}-{file_name}")
    } else {
        file_name
    };

    let data = tauri::async_runtime::spawn_blocking(move || std::fs::read(url))
        .await
        .map_err(|error| SyncError::Other(error.to_string()))??;
    task::check_cancellation()?;

    if !can_publish(key, file_type, source, record.generation) {
        return Ok(());
    }

    let folder = file_folder(folders, key, record.generation, true)
        .await?
        .unwrap();
    if listed_file(&folder, &name).is_none() {
        drive::upload(data, &name, &folder).await?;
    }
    task::check_cancellation()?;
    if !can_publish(key, file_type, source, record.generation) {
        return Ok(());
    }

    let mut store = store();
    let current = store.state.books.get_mut(key).unwrap();
    if file_type == SyncFileType::Sasayaki
        && current
            .files
            .get(&SyncFileType::Sasayaki)
            .and_then(|file| file.value.as_ref())
            .is_some_and(|published| *published != name)
    {
        current.cleanup.insert(record.generation);
    }
    current.files.insert(
        file_type,
        Timestamped {
            modified: source,
            value: Some(name),
        },
    );
    current.pending = true;
    store.save_changes(false)
}

async fn file_folder(
    folders: &mut Folders,
    key: &str,
    generation: i64,
    create: bool,
) -> SyncResult<Option<String>> {
    let id = (key.to_string(), generation);
    if let Some(folder) = folders.get(&id) {
        return Ok(Some(folder.clone()));
    }
    let listed = listed_file(&book_folder(), key)
        .and_then(|book| listed_file(&book.id, &generation.to_string()));
    let folder = match listed {
        Some(folder) => Some(folder.id),
        None => drive::file_folder(&book_folder(), key, generation, create).await?,
    };
    if let Some(folder) = &folder {
        folders.insert(id, folder.clone());
    }
    Ok(folder)
}

fn can_publish(key: &str, file_type: SyncFileType, source: i64, generation: i64) -> bool {
    let store = store();
    let record = &store.state.books[key];
    record.generation == generation
        && record.sources.get(&file_type) == Some(&source)
        && (!record.deleted || file_type == SyncFileType::Cover)
        && record
            .files
            .get(&file_type)
            .map_or(i64::MIN, |file| file.modified)
            <= source
}

async fn download_file(
    key: &str,
    file_type: SyncFileType,
    on_progress: &(dyn Fn(f64) + Sync),
    folders: &mut Folders,
) -> SyncResult<()> {
    let record = store().state.books[key].clone();
    if record.deleted && file_type != SyncFileType::Cover {
        return Ok(());
    }

    let Some(reference) = record.files.get(&file_type).cloned() else {
        return Ok(());
    };
    if record
        .sources
        .get(&file_type)
        .is_some_and(|&source| source >= reference.modified)
    {
        return Ok(());
    }

    let root = SyncStorage::book_directory(key, record.deleted);

    let Some(name) = reference.value.clone() else {
        return apply_downloaded_file(key, file_type, None, &reference);
    };
    if record.deleted
        && statistics::load_folder(app(), key)
            .values()
            .all(|change| change.value.is_none())
    {
        return Ok(());
    }

    let folder = file_folder(folders, key, record.generation, false).await?;

    let listed = folder
        .as_deref()
        .and_then(|folder| listed_file(folder, &name));
    let data = drive::download(&name, folder.as_deref(), listed, on_progress).await?;

    task::check_cancellation()?;

    {
        let store = store();
        let current = &store.state.books[key];
        if current.generation != record.generation
            || current.deleted != record.deleted
            || current.files.get(&file_type) != Some(&reference)
        {
            return Ok(());
        }
        if current
            .sources
            .get(&file_type)
            .is_some_and(|&source| source >= reference.modified)
        {
            return Ok(());
        }
    }

    std::fs::create_dir_all(&root)?;
    let file_name = if file_type == SyncFileType::Sasayaki {
        SASAYAKI_MATCH.to_string()
    } else {
        name
    };
    let destination = root.join(&file_name);
    storage::write_atomic(&destination, &data)?;
    let relative = format!(
        "Books/{}{}/{}",
        if record.deleted {
            "statistics_archive/"
        } else {
            ""
        },
        root.file_name().unwrap().to_string_lossy(),
        file_name
    );
    apply_downloaded_file(key, file_type, Some(&relative), &reference)
}

fn apply_downloaded_file(
    key: &str,
    file_type: SyncFileType,
    path: Option<&str>,
    reference: &Timestamped<Option<String>>,
) -> SyncResult<()> {
    let mut store = store();
    let root = SyncStorage::book_directory(key, store.state.books[key].deleted);

    if let Some(existing) = library::load_metadata_at(&root)
        && file_type != SyncFileType::Sasayaki
    {
        let app_directory = library::app_dir(app());
        let old_path = if file_type == SyncFileType::Epub {
            existing.epub.as_ref().map(|epub| root.join(epub))
        } else {
            existing.cover.as_ref().map(|cover| {
                if cover.starts_with('/') {
                    std::path::PathBuf::from(cover)
                } else {
                    app_directory.join(cover)
                }
            })
        };
        let new_path = path.map(|path| app_directory.join(path));
        if let Some(old_path) = &old_path
            && Some(old_path) != new_path.as_ref()
        {
            library::delete(old_path)?;
        }

        let mut metadata = existing;
        if file_type == SyncFileType::Epub {
            metadata.epub = path.map(|path| path.rsplit('/').next().unwrap().to_string());
        }
        if file_type == SyncFileType::Cover {
            metadata.cover = path.map(str::to_string);
        }

        library::save_metadata(&metadata, &root)?;
    }
    if file_type == SyncFileType::Sasayaki && path.is_none() {
        library::delete(&root.join(SASAYAKI_MATCH))?;
    }

    store
        .state
        .books
        .get_mut(key)
        .unwrap()
        .sources
        .insert(file_type, reference.modified);

    store.save()?;
    if file_type == SyncFileType::Sasayaki {
        app()
            .emit("sync://match-changed", json!({ "key": key }))
            .ok();
    } else {
        storage::post_books_changed();
    }
    Ok(())
}

async fn cleanup_files(key: &str, folders: &mut Folders) -> SyncResult<()> {
    let generations: Vec<i64> = store().state.books[key].cleanup.iter().copied().collect();
    for generation in generations {
        if store().state.books[key].pending {
            return Ok(());
        }

        let files = drive::children(&state_folder(), Some(&format!("{key}.json"))).await?;
        let remote = read_state(&files, SyncBook::merge).await?;
        merge_book(key, remote.as_ref())?;

        let Some(book) = store().load_book(key, remote.as_ref()) else {
            return Ok(());
        };
        if book.needs_upload(remote.as_ref()) {
            let mut store = store();
            store.state.books.get_mut(key).unwrap().pending = true;

            return store.save_changes(false);
        }

        let folder = file_folder(folders, key, generation, false).await?;

        let mut recent = false;
        if let Some(folder) = &folder {
            if generation < book.generation {
                client::trash_file(folder).await?;
                folders.remove(&(key.to_string(), generation));
                task::check_cancellation()?;
            } else {
                let files = drive::children(folder, None).await?;

                let cover = book
                    .files
                    .get(&SyncFileType::Cover)
                    .and_then(|file| file.value.clone());
                for file in files
                    .iter()
                    .filter(|file| !file.is_folder() && Some(&file.name) != cover.as_ref())
                {
                    let current = store().state.books[key].clone();
                    let stale = file.name.ends_with(SASAYAKI_MATCH)
                        && Some(&file.name)
                            != current
                                .files
                                .get(&SyncFileType::Sasayaki)
                                .and_then(|file| file.value.as_ref());
                    if !current.deleted && !stale {
                        continue;
                    }
                    if file.is_recent() {
                        recent = true;
                        continue;
                    }

                    drive::trash(file).await?;
                    task::check_cancellation()?;
                }
            }
        }
        if recent {
            continue;
        }

        let mut store = store();
        store
            .state
            .books
            .get_mut(key)
            .unwrap()
            .cleanup
            .remove(&generation);

        store.save()?;
    }
    Ok(())
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

fn save_cache() -> SyncResult<()> {
    let data = sync_format::encode(&shared().cache)?;
    storage::write_atomic(&cache_url(), &data)
}
