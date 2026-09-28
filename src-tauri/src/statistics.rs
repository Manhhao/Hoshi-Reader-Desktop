use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{Duration, LocalResult, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::library::{self, APPLE_EPOCH, BookMetadata};
use crate::sync::model::{SyncBook, Timestamped};
use crate::ttu_statistics::TtuStatistics;

const STATISTICS_FILE: &str = "statistics.json";
const STATISTICS_ARCHIVE: &str = "statistics_archive";
const STATISTICS_COVER: &str = "cover.jpg";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReadingSession {
    pub started_at: i64,
    pub ended_at: i64,
    pub characters_read: i64,
    pub reading_time: f64,
}

impl ReadingSession {
    pub fn has_activity(&self) -> bool {
        self.characters_read > 0 || self.reading_time > 0.0
    }
}

pub type Sessions = HashMap<String, Timestamped<Option<ReadingSession>>>;

#[derive(Default)]
pub struct ReadingTotal {
    pub characters_read: i64,
    pub reading_time: f64,
}

impl ReadingTotal {
    fn add(&mut self, session: &ReadingSession) {
        self.characters_read += session.characters_read;
        self.reading_time += session.reading_time;
    }
}

pub struct StatisticsDay {
    pub date: NaiveDate,
    pub sessions: Sessions,
}

impl StatisticsDay {
    pub fn total(&self) -> ReadingTotal {
        let mut total = ReadingTotal::default();
        for change in self.sessions.values() {
            total.add(change.value.as_ref().unwrap());
        }
        total
    }

    pub fn date(apple: f64, reset_time: i64) -> NaiveDate {
        local_date(apple - reset_time as f64 * 60.0)
    }

    pub fn grouped(sessions: &Sessions, reset_time: i64) -> Vec<StatisticsDay> {
        let mut days: HashMap<NaiveDate, StatisticsDay> = HashMap::new();
        for (id, change) in sessions {
            let Some(session) = &change.value else {
                continue;
            };
            let date = Self::date(library::ms_to_apple(session.started_at), reset_time);
            days.entry(date)
                .or_insert_with(|| StatisticsDay {
                    date,
                    sessions: HashMap::new(),
                })
                .sessions
                .insert(id.clone(), change.clone());
        }
        let mut days: Vec<StatisticsDay> = days.into_values().collect();
        days.sort_by_key(|day| day.date);
        days
    }
}

pub fn local_date(apple: f64) -> NaiveDate {
    let millis = ((apple + APPLE_EPOCH) * 1000.0).floor() as i64;
    chrono::Local
        .timestamp_millis_opt(millis)
        .earliest()
        .unwrap()
        .date_naive()
}

pub fn local_apple(date: NaiveDate, hour: u32, minute: u32) -> f64 {
    let naive = date.and_hms_opt(hour, minute, 0).unwrap();
    let local = match chrono::Local.from_local_datetime(&naive) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => time,
        LocalResult::None => chrono::Local
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .unwrap(),
    };
    local.timestamp() as f64 - APPLE_EPOCH
}

static RESET_TIME: AtomicI64 = AtomicI64::new(0);

pub fn statistics_reset_time() -> i64 {
    RESET_TIME.load(Ordering::SeqCst)
}

pub fn set_statistics_reset_time(minutes: i64) {
    RESET_TIME.store(minutes, Ordering::SeqCst);
}

pub fn load(root: &Path) -> Sessions {
    let path = root.join(STATISTICS_FILE);
    if let Some(sessions) = library::read_json::<Sessions>(&path) {
        return sessions;
    }

    let Some(daily) = library::read_json::<Vec<TtuStatistics>>(&path) else {
        return HashMap::new();
    };
    let key = root.file_name().unwrap().to_string_lossy();
    let sessions = TtuStatistics::legacy_sessions(&daily, &key);
    library::write_json(&path, &sessions).ok();
    sessions
}

pub fn load_folder(app: &AppHandle, folder: &str) -> Sessions {
    let Some(root) = book_directory(app, folder).or_else(|| archived_book_directory(app, folder))
    else {
        return HashMap::new();
    };
    load(&root)
}

pub fn save(app: &AppHandle, sessions: &Sessions, folder: &str) {
    let Some(root) = book_directory(app, folder).or_else(|| archived_book_directory(app, folder))
    else {
        return;
    };
    library::write_json(&root.join(STATISTICS_FILE), sessions).ok();
    if root
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == STATISTICS_ARCHIVE)
        && sessions.values().all(|change| change.value.is_none())
    {
        fs::remove_file(root.join(STATISTICS_COVER)).ok();
    }
    crate::sync::storage::shared()
        .handle_sessions_change(folder, sessions)
        .ok();
}

pub fn delete(app: &AppHandle, ids: &[String], folder: &str) {
    let mut sessions = load_folder(app, folder);
    for id in ids {
        if sessions
            .get(id)
            .is_some_and(|change| change.value.is_some())
        {
            sessions.insert(
                id.clone(),
                Timestamped {
                    modified: library::now_ms(),
                    value: None,
                },
            );
        }
    }
    save(app, &sessions, folder);
}

pub fn archive(app: &AppHandle, book: &BookMetadata) -> Result<(), String> {
    let Some(root) = book_directory(app, &book.folder) else {
        return Ok(());
    };
    let destination = archived_book_directory(app, &book.folder)
        .unwrap_or_else(|| archive_directory(app).join(&book.folder));

    let sessions = SyncBook::merge_records(&load(&root), &load(&destination));
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    library::write_json(&destination.join(STATISTICS_FILE), &sessions)?;

    let mut metadata = BookMetadata::new(
        library::display_title(book),
        book.author.clone(),
        if sessions.values().any(|change| change.value.is_some()) {
            write_archived_cover(app, book, &destination)
        } else {
            None
        },
        book.folder.clone(),
        book.last_access,
    );
    metadata.modified = book.modified;
    metadata.character_count = book.character_count.or_else(|| {
        library::read_json::<library::BookInfo>(&root.join(library::BOOKINFO_FILE))
            .map(|info| info.character_count as i64)
    });
    library::save_metadata(&metadata, &destination)
}

pub fn restore(app: &AppHandle, folder: &str) -> Result<(), String> {
    let (Some(archived), Some(root)) = (
        archived_book_directory(app, folder),
        book_directory(app, folder),
    ) else {
        return Ok(());
    };

    let sessions = SyncBook::merge_records(&load(&root), &load(&archived));
    library::write_json(&root.join(STATISTICS_FILE), &sessions)?;
    fs::remove_dir_all(&archived).map_err(|error| error.to_string())
}

fn write_archived_cover(app: &AppHandle, book: &BookMetadata, directory: &Path) -> Option<String> {
    let destination = directory.join(STATISTICS_COVER);
    fs::remove_file(&destination).ok();

    let bytes = fs::read(library::cover_url(app, book)?).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let image = image.thumbnail(240, 240).to_rgb8();
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 85)
        .encode_image(&image)
        .ok()?;
    fs::write(&destination, encoded).ok()?;
    Some(format!(
        "Books/{STATISTICS_ARCHIVE}/{}/{STATISTICS_COVER}",
        book.folder
    ))
}

pub fn archive_directory(app: &AppHandle) -> PathBuf {
    library::books_dir(app).join(STATISTICS_ARCHIVE)
}

fn book_directory(app: &AppHandle, folder: &str) -> Option<PathBuf> {
    library::find_path(&library::books_dir(app), folder)
}

fn archived_book_directory(app: &AppHandle, folder: &str) -> Option<PathBuf> {
    library::find_path(&archive_directory(app), folder)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingDay {
    pub date_key: String,
    pub characters_read: i64,
    pub reading_time: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookStatistics {
    pub id: String,
    pub folder: String,
    pub title: String,
    pub cover: bool,
    pub is_deleted: bool,
    pub days: Vec<ReadingDay>,
}

fn book_statistics(
    book: &BookMetadata,
    root: &Path,
    is_deleted: bool,
    reset_time: i64,
) -> Option<BookStatistics> {
    let days: Vec<ReadingDay> = StatisticsDay::grouped(&load(root), reset_time)
        .into_iter()
        .map(|day| (day.date, day.total()))
        .filter(|(_, total)| total.characters_read > 0 || total.reading_time > 0.0)
        .map(|(date, total)| ReadingDay {
            date_key: date.format("%Y-%m-%d").to_string(),
            characters_read: total.characters_read,
            reading_time: total.reading_time,
        })
        .collect();
    if days.is_empty() {
        return None;
    }
    Some(BookStatistics {
        id: book.id.clone(),
        folder: book.folder.clone(),
        title: library::display_title(book),
        cover: book.cover.is_some(),
        is_deleted,
        days,
    })
}

pub fn load_archived(app: &AppHandle, reset_time: i64) -> Vec<BookStatistics> {
    let Ok(contents) = fs::read_dir(archive_directory(app)) else {
        return Vec::new();
    };
    contents
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|url| {
            let book = library::load_metadata_at(&url)?;
            book_statistics(&book, &url, true, reset_time)
        })
        .collect()
}

#[tauri::command(async)]
pub fn load_all_statistics(app: AppHandle, reset_time: i64) -> Vec<BookStatistics> {
    let books_directory = library::books_dir(&app);
    let mut books: Vec<BookStatistics> = library::load_all_books(&app)
        .iter()
        .filter_map(|book| {
            let root = library::find_path(&books_directory, &book.folder)?;
            book_statistics(book, &root, false, reset_time)
        })
        .collect();
    books.extend(load_archived(&app, reset_time));
    books
}

#[tauri::command]
pub fn load_statistics(app: AppHandle, folder: String) -> Sessions {
    load_folder(&app, &folder)
}

#[tauri::command]
pub fn save_reading_session(
    app: AppHandle,
    folder: String,
    session_id: String,
    session: ReadingSession,
) -> Sessions {
    let mut sessions = load_folder(&app, &folder);
    if sessions
        .get(&session_id)
        .is_some_and(|change| change.value.is_none())
    {
        return sessions;
    }
    if session.has_activity()
        && sessions
            .get(&session_id)
            .and_then(|change| change.value.as_ref())
            != Some(&session)
    {
        sessions.insert(
            session_id,
            Timestamped {
                modified: library::now_ms(),
                value: Some(session),
            },
        );
        save(&app, &sessions, &folder);
    }
    sessions
}

#[tauri::command]
pub fn edit_reading_session(
    app: AppHandle,
    folder: String,
    id: String,
    characters_read: Option<i64>,
    reading_time: Option<f64>,
) {
    let mut sessions = load_folder(&app, &folder);
    let Some(Some(mut session)) = sessions.get(&id).map(|change| change.value.clone()) else {
        return;
    };
    if let Some(characters_read) = characters_read {
        session.characters_read = characters_read;
    }
    if let Some(reading_time) = reading_time {
        session.reading_time = reading_time;
    }
    if Some(&session) == sessions[&id].value.as_ref() {
        return;
    }
    sessions.insert(
        id,
        Timestamped {
            modified: library::now_ms(),
            value: Some(session),
        },
    );
    save(&app, &sessions, &folder);
}

#[tauri::command]
pub fn delete_reading_sessions(app: AppHandle, folder: String, ids: Vec<String>) {
    delete(&app, &ids, &folder);
}

#[tauri::command]
pub fn clear_statistics_archive(app: AppHandle) {
    let Ok(contents) = fs::read_dir(archive_directory(&app)) else {
        return;
    };
    for url in contents.filter_map(Result::ok).map(|entry| entry.path()) {
        let ids: Vec<String> = load(&url).into_keys().collect();
        delete(&app, &ids, &url.file_name().unwrap().to_string_lossy());
    }
}
