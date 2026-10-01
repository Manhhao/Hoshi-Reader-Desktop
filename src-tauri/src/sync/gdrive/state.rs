use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;
use serde_json::json;
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::sync::app;
use crate::sync::client::{self, GoogleDriveFile};
use crate::sync::gdrive::handler as drive;
use crate::sync::gdrive::manager::{
    Phase, publish, record_book, save_cache, shared, state_folder, store,
};
use crate::sync::model::{SyncBook, SyncResult, SyncShelves, sync_format};
use crate::sync::storage::SyncStorage;
use crate::sync::task;

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

pub(super) async fn run(book: Option<BookMetadata>) -> SyncResult<()> {
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

pub(super) async fn read_state<T: serde::de::DeserializeOwned>(
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

pub(super) fn merge_book(key: &str, remote: Option<&SyncBook>) -> SyncResult<()> {
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
