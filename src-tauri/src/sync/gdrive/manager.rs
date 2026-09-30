use std::collections::HashMap;
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
use crate::sync::storage::{self, SASAYAKI_MATCH, SyncStorage};
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
    state_task: Option<SyncTask>,
    file_transfer_task: Option<SyncTask>,
    poll_task: Option<SyncTask>,
    debounce_task: Option<SyncTask>,
    pub download_task: Option<SyncTask>,
    stopped: bool,
    unsupported_format: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_sync: Option<i64>,
    pub is_syncing: bool,
    pub error_message: Option<String>,
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
        is_syncing: manager.state_task.is_some(),
        error_message: manager.error_message.clone(),
    }
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
            manager.error_message = Some(error.to_string());
            if error.is_format_error() {
                manager.unsupported_format = true;
            }
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
        sync_book(&key, None).await?;
        return Ok(());
    }

    let (mut remote, listed, cursor) = changes().await?;
    let pending: Vec<String> = store()
        .state
        .books
        .iter()
        .filter(|(_, record)| record.pending)
        .map(|(key, _)| key.clone())
        .collect();
    for key in pending {
        remote.entry(key).or_insert_with(|| listed.then(Vec::new));
    }
    let shelves_changed = remote.contains_key(".shelves");
    let mut sorted: Vec<(String, Option<Vec<GoogleDriveFile>>)> = remote
        .into_iter()
        .filter(|(key, _)| key != ".shelves")
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, files) in sorted {
        sync_book(&key, files).await?;
    }
    let unattached = store()
        .state
        .books
        .values()
        .any(|record| !record.attached && !record.deleted);
    if !unattached {
        if shelves_changed || store().state.shelves_pending {
            sync_shelves().await?;
        }
        shared().cache.cursor = Some(cursor);
        save_cache()?;
        {
            let mut manager = shared();
            manager.last_sync = Some(library::now_ms());
            manager.unsupported_format = false;
        }
        publish();
    }
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
        run_file_sync().await;
        shared().file_transfer_task = None;
    });
    manager.file_transfer_task = Some(file_transfer_task.clone());
    drop(manager);
    file_transfer_task.start();
}

async fn run_file_sync() {
    let mut folders = HashMap::new();
    let mut keys: Vec<String> = store().state.books.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if task::is_cancelled() {
            return;
        }

        for file_type in SyncFileType::ALL_CASES {
            let result = async {
                upload_file(&key, file_type, &mut folders).await?;
                if file_type != SyncFileType::Epub {
                    download_file(&key, file_type, &|_| {}, &mut folders).await?;
                }
                SyncResult::Ok(())
            }
            .await;
            if let Err(error) = result
                && stops_file_sync(&error)
            {
                return;
            }
        }

        if let Err(error) = cleanup_files(&key, &mut folders).await
            && stops_file_sync(&error)
        {
            return;
        }
    }
}

fn stops_file_sync(error: &SyncError) -> bool {
    if task::is_cancelled() {
        return true;
    }

    let format_error = error.is_format_error();
    {
        let mut manager = shared();
        manager.error_message = Some(error.to_string());
        manager.unsupported_format |= format_error;
    }
    publish();
    format_error
}

pub async fn download_book(
    book: &BookMetadata,
    on_progress: &(dyn Fn(f64) + Sync),
) -> SyncResult<BookMetadata> {
    let key: String = book.folder.nfc().collect();
    sync(Some(book.clone())).await;
    let (unsupported_format, error_message) = {
        let manager = shared();
        (manager.unsupported_format, manager.error_message.clone())
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

async fn changes() -> SyncResult<(HashMap<String, Option<Vec<GoogleDriveFile>>>, bool, String)> {
    let mut keys: HashMap<String, Option<Vec<GoogleDriveFile>>> = HashMap::new();
    let mut listed = false;
    let saved = shared().cache.cursor.clone();
    let mut cursor = match saved {
        Some(saved) => saved,
        None => {
            let cursor = drive::start_token().await?;
            keys.extend(
                list_remote()
                    .await?
                    .into_iter()
                    .map(|(key, files)| (key, Some(files))),
            );
            listed = true;
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
            for (key, files) in list_remote().await? {
                keys.entry(key).or_insert(Some(files));
            }
            listed = true;
        }
        let state_folder = state_folder();
        let changed: Vec<String> = page
            .changes
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
            .filter_map(|file| file.state_key())
            .collect();
        for key in changed {
            keys.insert(key, None);
        }
        match page.next_page_token {
            Some(next) => cursor = next,
            None => return Ok((keys, listed, page.new_start_page_token.unwrap())),
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

    let mut versions: HashMap<String, String> = files
        .iter()
        .map(|file| (file.id.clone(), file.version.clone()))
        .collect();
    let pending = store().state.books.get(key).map(|record| record.pending);
    if files.len() == 1
        && pending == Some(false)
        && shared().cache.book_versions.get(key) == Some(&versions)
    {
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

    let Some(book) = store().load_book(key) else {
        return Ok(());
    };

    if book.needs_upload(remote.as_ref()) || files.len() > 1 {
        let written = write_state(&book, &format!("{key}.json"), &files).await?;
        versions = HashMap::from([(written.id, written.version)]);
        remote = Some(book.clone());
    }

    {
        let mut store = store();
        if store.state.books[key].pending && store.load_book(key).as_ref() == Some(&book) {
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
            None => store.load_book(key),
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

    let mut local = store.load_book(key).unwrap();
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

    let folder = file_folder(folders, key, record.generation, true).await?;
    drive::upload(data, &name, &folder.unwrap()).await?;
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
    let folder = drive::file_folder(&book_folder(), key, generation, create).await?;
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

    let data = drive::download(&name, folder.as_deref(), on_progress).await?;

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

        let book = store().load_book(key).unwrap();
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
