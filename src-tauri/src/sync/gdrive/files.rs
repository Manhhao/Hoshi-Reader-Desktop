use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use serde_json::json;
use tauri::Emitter;
use tokio::sync::Notify;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::statistics;
use crate::sync::client::{self, GoogleDriveError, Meter};
use crate::sync::gdrive::handler as drive;
use crate::sync::gdrive::listing::Listing;
use crate::sync::gdrive::manager::{
    self, Direction, Job, Moving, Phase, Progress, QueueItem, Transfer, TransferState, book_title,
    publish, record_book, shared, state_folder, store,
};
use crate::sync::gdrive::state::{merge_book, read_state};
use crate::sync::model::{SyncBook, SyncError, SyncFileType, SyncResult, Timestamped};
use crate::sync::storage::{self, SASAYAKI_MATCH, SyncRecord, SyncStorage};
use crate::sync::{app, task};

static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
const EPUB_SLOTS: usize = 4;

pub(super) async fn run() -> SyncResult<bool> {
    enqueue(true);
    let listing = Arc::new(if shared().progress.is_some() {
        Listing::list().await?
    } else {
        Listing::default()
    });
    count_sizes(&listing);
    let workers = (0..8)
        .map(|_| {
            let listing = listing.clone();
            async move { work(&listing).await }
        })
        .collect();
    let ticker = async {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            manager::sample();
            publish();
        }
    };
    let result = tokio::select! {
        result = task::concurrent(workers) => result,
        _ = ticker => Ok(()),
    };
    manager::save_cache()?;
    result?;
    if shared().cache.replaced_root && drive::remove_replaced().await.is_ok() {
        shared().cache.replaced_root = false;
        manager::save_cache()?;
    }
    Ok(listing.published.load(Ordering::SeqCst))
}

async fn work(listing: &Listing) -> SyncResult<()> {
    loop {
        let notified = WAKE.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let next = {
            let mut manager = shared();
            let epubs = manager.epubs < EPUB_SLOTS && manager.priority == 0;
            let index = manager.jobs.iter().position(|job| !job.epub || epubs);
            if index.is_none() && manager.jobs.is_empty() && manager.active == 0 {
                return Ok(());
            }
            index.map(|index| {
                let job = manager.jobs.remove(index).unwrap();
                manager.active += 1;
                manager.epubs += job.epub as usize;
                job
            })
        };
        let Some(job) = next else {
            client::cancellable(notified).await?;
            continue;
        };

        set_current_transfer(&job.key);
        let lock = lock(&job.key);
        let result = async {
            let _guard = client::cancellable(lock.lock()).await?;
            let synced = sync_files(&job.key, listing).await;
            if job.epub && wants_epub(&job.key) {
                synced.and(download_file(&job.key, SyncFileType::Epub, &|_| {}, listing).await)
            } else {
                synced
            }
        }
        .await;
        let deferred = !job.epub && result.is_ok() && wants_epub(&job.key);
        let succeeded = result.is_ok();
        let recorded = record_book(&job.key, Phase::File, result);
        {
            let mut manager = shared();
            manager.active -= 1;
            manager.epubs -= job.epub as usize;
            if job.epub && succeeded {
                manager.downloads.remove(&job.key);
            }
            if deferred {
                manager.jobs.push_back(Job {
                    key: job.key.clone(),
                    epub: true,
                });
            }
            if recorded.is_err() {
                manager.jobs.clear();
            }
        }
        finish_transfer(&job.key, !deferred && recorded.is_ok());
        WAKE.notify_waiters();
        recorded?;
    }
}

pub(super) fn lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    shared().locks.entry(key.to_string()).or_default().clone()
}

pub(super) fn missing_books() -> Vec<String> {
    store()
        .state
        .books
        .iter()
        .filter(|(_, record)| missing_epub(record))
        .map(|(key, _)| key.clone())
        .collect()
}

fn missing_epub(record: &SyncRecord) -> bool {
    !record.deleted
        && record.files.get(&SyncFileType::Epub).is_some_and(|file| {
            file.value.is_some()
                && record
                    .sources
                    .get(&SyncFileType::Epub)
                    .is_none_or(|&source| source < file.modified)
        })
}

type PendingFile = (SyncFileType, Direction, Option<u64>);

fn pending_files(
    store: &SyncStorage,
    key: &str,
    record: &SyncRecord,
    epub: bool,
    listing: Option<&Listing>,
) -> Vec<PendingFile> {
    let mut files = Vec::new();
    for file_type in SyncFileType::ALL_CASES
        .into_iter()
        .filter(|&file_type| !record.deleted || file_type == SyncFileType::Cover)
    {
        let source = record.sources.get(&file_type).copied();
        let published = record.files.get(&file_type);
        if record.attached
            && source.is_some_and(|source| published.is_none_or(|file| file.modified < source))
        {
            let size = store
                .source_url(key, file_type)
                .and_then(|url| std::fs::metadata(url).ok())
                .map_or(0, |metadata| metadata.len());
            files.push((file_type, Direction::Upload, Some(size)));
        } else if (file_type != SyncFileType::Epub || epub)
            && published.is_some_and(|file| source.is_none_or(|source| source < file.modified))
            && let Some(name) = published.and_then(|file| file.value.as_ref())
        {
            let size = listing.and_then(|listing| listing.size(key, record.generation, name));
            files.push((file_type, Direction::Download, size));
        }
    }
    files
}

fn count(manager: &mut manager::GoogleDriveSyncManager, key: &str, files: &[PendingFile]) {
    for &(file_type, direction, size) in files {
        let Some(size) = size else { continue };
        let flow = match direction {
            Direction::Download => &mut manager.download_flow,
            _ => &mut manager.upload_flow,
        };
        if flow.counted.insert(format!("{key}/{file_type:?}")) {
            flow.total += size;
        }
    }
}

fn count_sizes(listing: &Listing) {
    let downloads = shared().downloads.clone();
    let keys: Vec<String> = shared()
        .transfers
        .iter()
        .map(|item| item.key.clone())
        .collect();
    let pending: Vec<(String, Vec<PendingFile>)> = {
        let store = store();
        keys.into_iter()
            .filter_map(|key| {
                let record = store.state.books.get(&key)?;
                let files = pending_files(
                    &store,
                    &key,
                    record,
                    downloads.contains(&key),
                    Some(listing),
                );
                Some((key, files))
            })
            .collect()
    };
    let mut manager = shared();
    for (key, files) in pending {
        count(&mut manager, &key, &files);
        if let Some(item) = manager.transfers.iter_mut().find(|item| item.key == key) {
            item.size = files.iter().map(|file| file.2).sum();
        }
    }
}

fn begin(key: &str, file_type: SyncFileType, direction: Direction, total: u64) -> Arc<Meter> {
    let meter = Meter::new();
    let mut manager = shared();
    count(&mut manager, key, &[(file_type, direction, Some(total))]);
    manager.interrupted.remove(key);
    manager.moving.insert(
        key.to_string(),
        Moving {
            direction,
            meter: meter.clone(),
            total,
        },
    );
    meter
}

fn end<T, E>(key: &str, result: &Result<T, E>) {
    let mut manager = shared();
    let Some(moving) = manager.moving.remove(key) else {
        return;
    };
    if result.is_ok() {
        match moving.direction {
            Direction::Download => manager.download_flow.done += moving.total,
            _ => manager.upload_flow.done += moving.total,
        }
        return;
    }
    let transfer = Transfer {
        direction: moving.direction,
        done: moving.meter.bytes().min(moving.total),
        total: moving.total,
        state: TransferState::Interrupted,
    };
    manager.interrupted.insert(key.to_string(), transfer);
}

fn wants_epub(key: &str) -> bool {
    let requested = shared().downloads.contains(key);
    requested && store().state.books.get(key).is_some_and(missing_epub)
}

async fn sync_files(key: &str, listing: &Listing) -> SyncResult<()> {
    let mut result = Ok(());
    for file_type in SyncFileType::ALL_CASES {
        task::check_cancellation()?;
        let transferred = async {
            upload_file(key, file_type, listing).await?;
            if file_type != SyncFileType::Epub {
                download_file(key, file_type, &|_| {}, listing).await?;
            }
            SyncResult::Ok(())
        }
        .await;
        result = result.and(transferred);
    }
    result.and(cleanup_files(key, listing).await)
}

fn transfer_direction(record: &SyncRecord, epub: bool) -> Option<Direction> {
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
                (file_type != SyncFileType::Epub || epub)
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

pub(super) fn enqueue(reset: bool) {
    let downloads = shared().downloads.clone();
    let mut keys: Vec<String> = store().state.books.keys().cloned().collect();
    keys.sort();
    let (transfers, epubs, pending) = {
        let store = store();
        let pending: Vec<(String, Vec<PendingFile>)> = keys
            .iter()
            .map(|key| {
                let record = &store.state.books[key];
                let files = pending_files(&store, key, record, downloads.contains(key), None);
                (key.clone(), files)
            })
            .collect();
        let transfers: Vec<QueueItem> = keys
            .iter()
            .zip(&pending)
            .filter_map(|(key, (_, files))| {
                let record = &store.state.books[key];
                let direction = transfer_direction(record, downloads.contains(key))?;
                let metadata = (!record.deleted)
                    .then(|| library::load_metadata_at(&SyncStorage::book_directory(key, false)))
                    .flatten();
                Some(QueueItem {
                    key: key.clone(),
                    title: metadata
                        .as_ref()
                        .map(|metadata| metadata.title.clone())
                        .unwrap_or_else(|| book_title(key, record.deleted)),
                    book_id: metadata.as_ref().map(|metadata| metadata.id.clone()),
                    cover: metadata.is_some_and(|metadata| metadata.cover.is_some()),
                    size: files.iter().map(|file| file.2).sum(),
                    direction: Some(direction),
                    error: None,
                    transfer: None,
                })
            })
            .collect();
        let epubs: HashSet<String> = pending
            .iter()
            .filter(|(_, files)| files.iter().any(|file| file.0 == SyncFileType::Epub))
            .map(|(key, _)| key.clone())
            .collect();
        (transfers, epubs, pending)
    };
    {
        let mut manager = shared();
        if reset {
            manager.jobs.clear();
            manager.active = 0;
            manager.epubs = 0;
            manager.transfers.clear();
            manager.progress = None;
            manager.moving.clear();
            manager.interrupted.clear();
            manager.upload_flow = Default::default();
            manager.download_flow = Default::default();
        }
        for (key, files) in &pending {
            count(&mut manager, key, files);
        }
        for key in keys {
            if manager.jobs.iter().any(|job| job.key == key) {
                continue;
            }
            let epub = epubs.contains(&key);
            manager.jobs.push_back(Job { key, epub });
        }
        for item in transfers {
            if manager
                .transfers
                .iter()
                .any(|queued| queued.key == item.key)
            {
                continue;
            }
            manager.transfers.push(item);
            manager
                .progress
                .get_or_insert(Progress {
                    done: 0,
                    total: 0,
                    current: Vec::new(),
                    upload: None,
                    download: None,
                })
                .total += 1;
        }
    }
    WAKE.notify_waiters();
    publish();
}

fn set_current_transfer(key: &str) {
    if let Some(progress) = &mut shared().progress {
        progress.current.push(key.to_string());
    }
    publish();
}

fn finish_transfer(key: &str, finished: bool) {
    {
        let mut manager = shared();
        if let Some(progress) = &mut manager.progress {
            progress.current.retain(|current| current != key);
        }
        if !finished || !manager.transfers.iter().any(|item| item.key == key) {
            drop(manager);
            publish();
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
    manager::sync(Some(book.clone())).await;
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
    if manager::enabled() && has_epub {
        shared().priority += 1;
        let result = async {
            let lock = lock(&key);
            let _guard = client::cancellable(lock.lock()).await?;
            download_file(&key, SyncFileType::Epub, on_progress, &Listing::default()).await
        }
        .await;
        shared().priority -= 1;
        WAKE.notify_waiters();
        result?;
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

async fn upload_file(key: &str, file_type: SyncFileType, listing: &Listing) -> SyncResult<()> {
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
        listing.published.store(true, Ordering::SeqCst);
        return store.save_changes(false);
    };

    let file_name: String = url.file_name().unwrap().to_string_lossy().nfc().collect();
    let name = if file_type == SyncFileType::Sasayaki {
        format!("{source}-{file_name}")
    } else {
        file_name
    };

    if !can_publish(key, file_type, source, record.generation) {
        return Ok(());
    }

    let total = std::fs::metadata(&url).map_or(0, |metadata| metadata.len());
    let meter = begin(key, file_type, Direction::Upload, total);
    let uploaded = listing
        .upload(
            key,
            record.generation,
            &name,
            &url,
            &meter,
            file_type == SyncFileType::Epub,
        )
        .await;
    end(key, &uploaded);
    uploaded?;
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
    listing.published.store(true, Ordering::SeqCst);
    store.save_changes(false)
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
    listing: &Listing,
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

    let Some(file) = listing.find(key, record.generation, &name).await? else {
        return Err(GoogleDriveError::Api(format!("{name} is missing from Google Drive.")).into());
    };
    let size = file
        .size
        .as_ref()
        .and_then(|size| size.parse::<u64>().ok())
        .unwrap_or(0);
    let meter = begin(key, file_type, Direction::Download, size);
    let progress = |fraction: f64| {
        meter.set((fraction * size as f64) as u64);
        on_progress(fraction);
    };
    let downloaded = drive::download(&file, &progress).await;
    end(key, &downloaded);
    let data = downloaded?;

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

async fn cleanup_files(key: &str, listing: &Listing) -> SyncResult<()> {
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

        let folder = listing.folder(key, generation).await?;

        let mut recent = false;
        if let Some(folder) = &folder {
            if generation < book.generation {
                client::trash_file(folder).await?;
                listing.forget(key, generation);
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
