use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::json;
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::statistics;
use crate::sync::client::{self, GoogleDriveError};
use crate::sync::gdrive::handler as drive;
use crate::sync::gdrive::listing::Listing;
use crate::sync::gdrive::manager::{
    self, Direction, Phase, Progress, QueueItem, book_title, publish, record_book, shared,
    state_folder, store,
};
use crate::sync::gdrive::state::{merge_book, read_state};
use crate::sync::model::{SyncBook, SyncError, SyncFileType, SyncResult, Timestamped};
use crate::sync::storage::{self, SASAYAKI_MATCH, SyncRecord, SyncStorage};
use crate::sync::{app, task};

pub(super) async fn run() -> SyncResult<bool> {
    let mut keys: Vec<String> = store().state.books.keys().cloned().collect();
    keys.sort();
    begin_transfers(&keys);
    let listing = Arc::new(if shared().progress.is_some() {
        Listing::list().await?
    } else {
        Listing::default()
    });
    let transfers = keys
        .into_iter()
        .map(|key| {
            let listing = listing.clone();
            async move {
                task::check_cancellation()?;
                set_current_transfer(&key);
                let result = sync_files(&key, &listing).await;
                record_book(&key, Phase::File, result)?;
                finish_transfer(&key);
                SyncResult::Ok(())
            }
        })
        .collect();
    let result = task::concurrent(transfers).await;
    manager::save_cache()?;
    result?;
    Ok(listing.published.load(Ordering::SeqCst))
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
        download_file(&key, SyncFileType::Epub, on_progress, &Listing::default()).await?;
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

    let data = tauri::async_runtime::spawn_blocking(move || std::fs::read(url))
        .await
        .map_err(|error| SyncError::Other(error.to_string()))??;
    task::check_cancellation()?;

    if !can_publish(key, file_type, source, record.generation) {
        return Ok(());
    }

    listing.upload(key, record.generation, &name, data).await?;
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
    let data = drive::download(&file, on_progress).await?;

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
