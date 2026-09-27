use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;
use unicode_normalization::UnicodeNormalization;

use crate::highlights::{self, HighlightRecords};
use crate::library::{self, BookMetadata, Bookmark, DISTANT_PAST, ShelfList};
use crate::sasayaki::{self, SasayakiMatchData};
use crate::statistics::{self, Sessions};
use crate::sync::app;
use crate::sync::model::{
    SyncBook, SyncBookmark, SyncFileType, SyncFiles, SyncHighlight, SyncMetadata, SyncPlayback,
    SyncResult, Timestamped,
};

pub const SASAYAKI_MATCH: &str = "sasayaki_match.json";
const SASAYAKI_TRANSCRIPT: &str = "sasayaki_transcript.json";
const HIGHLIGHTS: &str = "highlights.json";
const STATISTICS: &str = "statistics.json";
const STATISTICS_ARCHIVE: &str = "statistics_archive";
pub const BOOKS_CHANGED: &str = "sync://books-changed";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncRecord {
    pub generation: i64,
    pub deleted: bool,
    pub files: SyncFiles,
    pub sources: HashMap<SyncFileType, i64>,
    pub attached: bool,
    pub pending: bool,
    pub cleanup: HashSet<i64>,
}

impl SyncRecord {
    pub fn new(generation: i64, deleted: bool) -> Self {
        SyncRecord {
            generation,
            deleted,
            files: HashMap::new(),
            sources: HashMap::new(),
            attached: false,
            pending: true,
            cleanup: HashSet::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub books: HashMap<String, SyncRecord>,
    pub shelves_pending: bool,
}

#[derive(Default)]
pub struct SyncStorage {
    pub state: SyncState,
}

static SHARED: LazyLock<Mutex<SyncStorage>> = LazyLock::new(Mutex::default);

pub fn shared() -> MutexGuard<'static, SyncStorage> {
    SHARED.lock().unwrap()
}

pub fn write_atomic(path: &Path, data: &[u8]) -> SyncResult<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, data)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

pub fn post_books_changed() {
    app().emit(BOOKS_CHANGED, ()).ok();
}

impl SyncStorage {
    fn on_change(&self) {
        crate::sync::gdrive::manager::schedule();
    }

    pub fn reload(&mut self) {
        self.state = library::read_json(&self.storage_url()).unwrap_or_default();
    }

    pub fn save(&self) -> SyncResult<()> {
        write_atomic(&self.storage_url(), &serde_json::to_vec(&self.state)?)
    }

    pub fn save_changes(&self, books_changed: bool) -> SyncResult<()> {
        self.save()?;
        if books_changed {
            post_books_changed();
        }
        self.on_change();
        Ok(())
    }

    pub fn mark_pending(&mut self, key: &str) -> SyncResult<()> {
        if !self.state.books[key].pending {
            self.state.books.get_mut(key).unwrap().pending = true;
            self.save()?;
        }
        self.on_change();
        Ok(())
    }

    pub fn reset_sync_state(&mut self) -> SyncResult<()> {
        for (key, record) in &mut self.state.books {
            let archived = library::load_metadata_at(&Self::book_directory(key, false)).is_none();
            record.generation = if archived { 0 } else { 1 };
            record.deleted = archived;
            record.files = HashMap::new();
            record.attached = false;
            record.pending = true;
        }

        self.state.shelves_pending = true;
        self.save_changes(true)
    }

    pub fn prepare_library(&mut self) -> SyncResult<()> {
        for root in Self::book_directories() {
            statistics::load(&root);
            self.prepare_book(&root);
        }

        library::load_shelf_list(app());
        self.save()
    }

    pub fn prepare_book(&mut self, root: &Path) {
        let key: String = root.file_name().unwrap().to_string_lossy().nfc().collect();
        if self.state.books.contains_key(&key) {
            return;
        }

        let archived = root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == STATISTICS_ARCHIVE);
        let mut record = SyncRecord::new(if archived { 0 } else { 1 }, archived);
        for file_type in SyncFileType::ALL_CASES {
            if self.source_url(&key, file_type).is_some() {
                record.sources.insert(file_type, library::now_ms());
            }
        }
        self.state.books.insert(key, record);
    }

    pub fn load_book(&self, key: &str) -> Option<SyncBook> {
        let record = self.state.books.get(key)?;
        let root = Self::resolve_book_directory(key);
        let metadata = library::load_metadata_at(&root)?;
        let mut book = SyncBook::new(
            record.generation,
            record.deleted,
            Timestamped {
                modified: metadata.modified.unwrap_or(0),
                value: SyncMetadata {
                    title: library::display_title(&metadata),
                    author: metadata.author.clone(),
                },
            },
        );
        book.character_count = metadata.character_count.unwrap_or(0).max(
            library::load_book_info_at(&root)
                .map(|info| info.character_count as i64)
                .unwrap_or(0),
        );
        book.files = record.files.clone();
        book.sessions = statistics::load(&root);

        if !record.deleted {
            if let Some(bookmark) = library::load_bookmark_at(&root) {
                book.bookmark = Some(Timestamped {
                    modified: library::apple_to_ms(bookmark.last_modified.unwrap_or(DISTANT_PAST)),
                    value: SyncBookmark {
                        character_count: bookmark.character_count as i64,
                    },
                });
            }
            if let Some(playback) = sasayaki::load_sasayaki_playback(&root) {
                book.audiobook = Some(Timestamped {
                    modified: playback.modified.unwrap_or(0),
                    value: SyncPlayback {
                        last_position: playback.last_position,
                        delay: playback.delay,
                        rate: f64::from(playback.rate),
                    },
                });
            }
            book.highlights = sync_highlights(&highlights::load_highlight_records(&root));
            book.shelves = metadata.shelves.clone().unwrap_or_default();
        }
        Some(book)
    }

    pub fn apply_book(&mut self, key: &str, book: &SyncBook) -> SyncResult<()> {
        let old_record = self.state.books.get(key).cloned();
        let book_url = Self::book_directory(key, false);
        let existing = library::load_metadata_at(&book_url);
        let folder = existing.as_ref().map_or_else(
            || book_url.file_name().unwrap().to_string_lossy().into_owned(),
            |existing| existing.folder.clone(),
        );
        let mut books_changed = existing.is_some()
            && (book.deleted
                || old_record.as_ref().map(|record| record.generation) != Some(book.generation));
        if book.deleted
            && let Some(existing) = &existing
        {
            statistics::archive(app(), existing)?;
            library::delete(&book_url)?;
        }

        let root = Self::book_directory(&folder, book.deleted);
        let old_metadata = library::load_metadata_at(&root);
        let mut metadata = BookMetadata::new(
            old_metadata.as_ref().map_or_else(
                || book.metadata.value.title.clone(),
                |old| old.title.clone(),
            ),
            book.metadata.value.author.clone(),
            old_metadata.as_ref().and_then(|old| old.cover.clone()),
            old_metadata
                .as_ref()
                .map_or_else(|| folder.clone(), |old| old.folder.clone()),
            old_metadata
                .as_ref()
                .map_or(DISTANT_PAST, |old| old.last_access),
        );
        if let Some(old) = &old_metadata {
            metadata.id = old.id.clone();
            metadata.epub = old.epub.clone();
        }
        metadata.renamed_title = (metadata.title != book.metadata.value.title)
            .then(|| book.metadata.value.title.clone());
        metadata.modified = Some(book.metadata.modified);
        metadata.character_count = Some(book.character_count);
        if !book.deleted {
            metadata.shelves = Some(book.shelves.clone());
            if let Some(bookmark) = &book.bookmark {
                metadata.last_access = metadata
                    .last_access
                    .max(library::ms_to_apple(bookmark.modified));
            }
        }

        if Some(&metadata) != old_metadata.as_ref() {
            fs::create_dir_all(&root)?;
            library::save_metadata(&metadata, &root)?;
            books_changed = true;
        }

        let mut record = self
            .state
            .books
            .get(key)
            .cloned()
            .unwrap_or_else(|| SyncRecord::new(book.generation, book.deleted));

        record.generation = book.generation;
        record.deleted = book.deleted;
        record.files = book.files.clone();
        record.attached = true;
        let old_sessions = statistics::load(&root);
        if book.sessions != old_sessions {
            library::write_json(&root.join(STATISTICS), &book.sessions)?;
            books_changed = true;
        }
        if !book.deleted {
            statistics::restore(app(), &folder)?;

            let mut bookmark = library::load_bookmark_at(&root);
            let bookmark_changed = book.bookmark.as_ref().is_some_and(|change| {
                bookmark
                    .as_ref()
                    .map(|bookmark| bookmark.character_count as i64)
                    != Some(change.value.character_count)
            });
            if let Some(change) = &book.bookmark {
                let modified = library::apple_to_ms(
                    bookmark
                        .as_ref()
                        .and_then(|bookmark| bookmark.last_modified)
                        .unwrap_or(DISTANT_PAST),
                );
                if bookmark_changed {
                    books_changed = true;
                    let position = library::load_book_info_at(&root).and_then(|info| {
                        info.resolve_character_position(change.value.character_count as usize)
                    });
                    bookmark = Some(Bookmark {
                        chapter_index: position.map_or(0, |position| position.0),
                        progress: position.map_or(0.0, |position| position.1),
                        character_count: change.value.character_count as usize,
                        last_modified: None,
                    });
                }
                if bookmark_changed || modified != change.modified {
                    let bookmark = bookmark.as_mut().unwrap();
                    bookmark.last_modified = Some(library::ms_to_apple(change.modified));
                    library::write_json(&root.join(library::BOOKMARK_FILE), bookmark)?;
                }
            }

            if sync_highlights(&highlights::load_highlight_records(&root)) != book.highlights {
                let records: HighlightRecords = book
                    .highlights
                    .iter()
                    .map(|(id, record)| {
                        (
                            id.clone(),
                            record
                                .replacing(record.value.as_ref().map(|value| value.highlight(id))),
                        )
                    })
                    .collect();
                highlights::save_records(&records, &root)?;
            }

            if let Some(change) = &book.audiobook {
                let mut playback = sasayaki::load_sasayaki_playback(&root).unwrap_or_default();
                let synced_playback = &change.value;
                if playback.modified != Some(change.modified)
                    || playback.last_position != synced_playback.last_position
                    || playback.delay != synced_playback.delay
                    || f64::from(playback.rate) != synced_playback.rate
                {
                    playback.last_position = synced_playback.last_position;
                    playback.delay = synced_playback.delay;
                    playback.rate = synced_playback.rate as f32;
                    playback.modified = Some(change.modified);
                    sasayaki::save_sasayaki_playback(&playback, &root)?;
                }
            }
            app()
                .emit(
                    "sync://book-applied",
                    json!({ "key": key, "book": book, "bookmarkChanged": bookmark_changed }),
                )
                .ok();
        } else {
            for file_type in [SyncFileType::Epub, SyncFileType::Sasayaki] {
                record.sources.remove(&file_type);
            }
            if old_record
                .as_ref()
                .is_none_or(|old| !old.deleted || old.generation != book.generation)
            {
                record.sources.remove(&SyncFileType::Cover);
            }
        }

        self.state.books.insert(key.to_string(), record);
        self.clear_unused_cover(key, Some(&book.sessions))?;
        if self.state.books.get(key) != old_record.as_ref() {
            self.save()?;
        }
        if books_changed {
            post_books_changed();
        }
        Ok(())
    }

    pub fn handle_book_import(&mut self, book: &BookMetadata, root: &Path) -> SyncResult<()> {
        let key: String = book.folder.nfc().collect();
        let archive_url = Self::book_directory(&book.folder, true);
        if library::load_metadata_at(&archive_url).is_some() {
            self.prepare_book(&archive_url);
        }
        if let Some(old_record) = self
            .state
            .books
            .get(&key)
            .filter(|record| record.deleted)
            .cloned()
        {
            let mut record = SyncRecord::new((old_record.generation + 1).max(1), false);
            record.attached = old_record.attached || old_record.generation > 0;
            record.cleanup = old_record.cleanup;
            record.cleanup.insert(old_record.generation);
            self.state.books.insert(key.clone(), record);
        }

        let mut metadata = book.clone();
        metadata.modified = Some(library::now_ms());
        library::save_metadata(&metadata, root)?;
        statistics::restore(app(), &book.folder)?;
        self.prepare_book(root);

        for file_type in SyncFileType::ALL_CASES {
            if self.source_url(&key, file_type).is_some() {
                self.mark_file_changed(&key, file_type);
            }
        }

        self.state.books.get_mut(&key).unwrap().pending = true;
        self.save_changes(true)
    }

    pub fn delete_local_book(&mut self, key: &str) -> SyncResult<()> {
        let root = Self::book_directory(key, false);
        let mut metadata = library::load_metadata_at(&root).unwrap();
        library::delete(&epub_url(&root, metadata.epub.as_ref().unwrap()))?;
        metadata.epub = None;
        library::save_metadata(&metadata, &root)?;

        self.state
            .books
            .get_mut(key)
            .unwrap()
            .sources
            .remove(&SyncFileType::Epub);
        self.save()?;
        post_books_changed();
        Ok(())
    }

    pub fn delete_book(&mut self, key: &str) -> SyncResult<()> {
        let root = Self::book_directory(key, false);
        statistics::archive(app(), &library::load_metadata_at(&root).unwrap())?;

        let record = self.state.books.get_mut(key).unwrap();
        record.deleted = true;
        record.pending = true;
        record.cleanup.insert(record.generation);
        record.files.remove(&SyncFileType::Epub);
        record.files.remove(&SyncFileType::Sasayaki);
        record.sources.remove(&SyncFileType::Epub);
        record.sources.remove(&SyncFileType::Sasayaki);

        self.save_changes(true)?;
        library::delete(&root)?;

        self.clear_unused_cover(key, None)?;
        self.save_changes(true)
    }

    pub fn handle_book_change(&mut self, folder: &str) -> SyncResult<()> {
        let key: String = folder.nfc().collect();
        if !self.state.books.contains_key(&key) {
            let root = Self::resolve_book_directory(folder);
            self.prepare_book(&root);
            self.save()?;
        }
        self.mark_pending(&key)
    }

    pub fn mark_file_changed(&mut self, key: &str, file_type: SyncFileType) {
        let record = self.state.books.get_mut(key).unwrap();
        record.sources.insert(file_type, library::now_ms());
        record.pending = true;
    }

    pub fn save_sasayaki_match(
        &mut self,
        match_data: &SasayakiMatchData,
        folder: &str,
        root: &Path,
    ) -> SyncResult<()> {
        let key: String = folder.nfc().collect();
        self.prepare_book(root);
        library::write_json(&root.join(SASAYAKI_MATCH), match_data)?;
        self.mark_file_changed(&key, SyncFileType::Sasayaki);
        self.save_changes(false)
    }

    pub fn handle_shelves_change(&mut self) -> SyncResult<()> {
        self.state.shelves_pending = true;
        self.save_changes(true)
    }

    pub fn apply_shelves(&self, shelves: &ShelfList) -> SyncResult<()> {
        if shelves != &library::load_shelf_list(app()) {
            library::save_shelf_list(app(), shelves)?;
            post_books_changed();
        }
        Ok(())
    }

    pub fn source_url(&self, key: &str, file_type: SyncFileType) -> Option<PathBuf> {
        let root = Self::resolve_book_directory(key);
        let metadata = library::load_metadata_at(&root);
        match file_type {
            SyncFileType::Epub => metadata?.epub.map(|epub| epub_url(&root, &epub)),
            SyncFileType::Cover => library::cover_url(app(), &metadata?),
            SyncFileType::Sasayaki => {
                let url = root.join(SASAYAKI_MATCH);
                url.exists().then_some(url)
            }
        }
    }

    pub fn clear_unused_cover(&mut self, key: &str, sessions: Option<&Sessions>) -> SyncResult<()> {
        let record = self.state.books.get_mut(key).unwrap();
        if !record.deleted {
            return Ok(());
        }

        let loaded;
        let sessions = match sessions {
            Some(sessions) => sessions,
            None => {
                loaded = statistics::load_folder(app(), key);
                &loaded
            }
        };
        if sessions.values().all(|change| change.value.is_none()) {
            let root = Self::resolve_book_directory(key);
            if let Some(mut metadata) = library::load_metadata_at(&root)
                && let Some(cover) = library::cover_url(app(), &metadata)
            {
                library::delete(&cover)?;
                metadata.cover = None;
                library::save_metadata(&metadata, &root)?;
            }
            let published = record.files.get(&SyncFileType::Cover);
            if published
                .and_then(|published| published.value.as_ref())
                .is_none()
                && record
                    .sources
                    .get(&SyncFileType::Cover)
                    .copied()
                    .unwrap_or(i64::MIN)
                    <= published.map_or(i64::MIN, |published| published.modified)
            {
                return Ok(());
            }

            let modified = library::now_ms();
            record.files.insert(
                SyncFileType::Cover,
                Timestamped {
                    modified,
                    value: None,
                },
            );
            record.sources.insert(SyncFileType::Cover, modified);
            record.cleanup.insert(record.generation);
            record.pending = true;
        }
        Ok(())
    }

    pub fn remove_book_files(&mut self, key: &str) -> SyncResult<()> {
        let root = Self::resolve_book_directory(key);
        if let Some(mut metadata) = library::load_metadata_at(&root) {
            if let Some(epub) = &metadata.epub {
                library::delete(&epub_url(&root, epub))?;
            }
            if let Some(cover) = library::cover_url(app(), &metadata) {
                library::delete(&cover)?;
            }

            library::delete(&root.join(library::BOOKINFO_FILE))?;
            library::delete(&root.join(SASAYAKI_MATCH))?;
            library::delete(&root.join(SASAYAKI_TRANSCRIPT))?;
            library::delete(&root.join(library::BOOKMARK_FILE))?;
            library::delete(&root.join(HIGHLIGHTS))?;

            if let Some(mut playback) = sasayaki::load_sasayaki_playback(&root) {
                playback.last_position = 0.0;
                playback.delay = 0.0;
                playback.rate = 1.0;
                playback.modified = None;
                sasayaki::save_sasayaki_playback(&playback, &root)?;
            }

            metadata.epub = None;
            metadata.cover = None;
            library::save_metadata(&metadata, &root)?;
        }

        self.state.books.get_mut(key).unwrap().sources = HashMap::new();
        Ok(())
    }

    pub fn book_directories() -> Vec<PathBuf> {
        let books_directory = library::books_dir(app());
        [
            books_directory.clone(),
            books_directory.join(STATISTICS_ARCHIVE),
        ]
        .iter()
        .flat_map(|directory| {
            fs::read_dir(directory)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| library::load_metadata_at(path).is_some())
                .collect::<Vec<_>>()
        })
        .collect()
    }

    pub fn book_directory(folder: &str, archived: bool) -> PathBuf {
        let mut parent = library::books_dir(app());
        if archived {
            parent = parent.join(STATISTICS_ARCHIVE);
        }
        library::find_path(&parent, folder).unwrap_or_else(|| parent.join(folder))
    }

    pub fn resolve_book_directory(folder: &str) -> PathBuf {
        let book_url = Self::book_directory(folder, false);
        if library::load_metadata_at(&book_url).is_some() {
            book_url
        } else {
            Self::book_directory(folder, true)
        }
    }

    pub fn handle_sessions_change(&mut self, folder: &str, sessions: &Sessions) -> SyncResult<()> {
        let key: String = folder.nfc().collect();
        if !self.state.books.contains_key(&key) {
            let root = Self::resolve_book_directory(folder);
            self.prepare_book(&root);
            self.save()?;
        }
        if self.state.books[&key].deleted {
            self.clear_unused_cover(&key, Some(sessions))?;
            self.state.books.get_mut(&key).unwrap().pending = true;
            self.save_changes(false)
        } else {
            self.mark_pending(&key)
        }
    }

    fn storage_url(&self) -> PathBuf {
        library::books_dir(app()).join(".sync.json")
    }
}

pub fn sync_highlights(
    records: &HighlightRecords,
) -> HashMap<String, Timestamped<Option<SyncHighlight>>> {
    records
        .iter()
        .map(|(id, record)| {
            (
                id.clone(),
                record.replacing(record.value.as_ref().map(SyncHighlight::new)),
            )
        })
        .collect()
}

fn epub_url(root: &Path, epub: &str) -> PathBuf {
    library::find_path(root, epub).unwrap_or_else(|| root.join(epub))
}
