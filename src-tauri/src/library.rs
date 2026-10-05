use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rbook::Epub;
use rbook::epub::toc::EpubTocEntry;
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::http::{Request, Response, header::CONTENT_TYPE};
use tauri::{AppHandle, Manager};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::sync::model::Timestamped;

const BOOKS_DIR: &str = "Books";
const STATISTICS_COVER: &str = "cover.jpg";
pub const BOOKINFO_FILE: &str = "bookinfo.json";
pub const BOOKMARK_FILE: &str = "bookmark.json";
pub const PAGES_FILE: &str = "pages.json";
const METADATA_FILE: &str = "metadata.json";
const SHELVES_FILE: &str = "shelves.json";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadata {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub last_access: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shelves: Option<HashMap<String, Timestamped<bool>>>,
}

impl BookMetadata {
    pub fn new(
        title: String,
        author: Option<String>,
        cover: Option<String>,
        folder: String,
        last_access: f64,
    ) -> Self {
        BookMetadata {
            id: Uuid::new_v4().to_string().to_uppercase(),
            title,
            author,
            epub: None,
            cover,
            folder,
            last_access,
            renamed_title: None,
            modified: None,
            character_count: None,
            shelves: None,
        }
    }
}

pub fn display_title(book: &BookMetadata) -> String {
    book.renamed_title
        .clone()
        .unwrap_or_else(|| book.title.clone())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BookListItem {
    #[serde(flatten)]
    pub meta: BookMetadata,
    pub progress: f64,
    pub characters_read: usize,
    pub characters_total: usize,
    pub has_book_info: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BookInfo {
    pub character_count: usize,
    pub chapter_info: HashMap<String, ChapterInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

impl BookInfo {
    pub fn resolve_character_position(&self, character_count: usize) -> Option<(usize, f64)> {
        let clamped = character_count.min(self.character_count.saturating_sub(1));
        for chapter in self.chapter_info.values() {
            let Some(spine_index) = chapter.spine_index else {
                continue;
            };
            if chapter.chapter_count == 0 {
                continue;
            }
            let start = chapter.current_total;
            let end = start + chapter.chapter_count;
            if clamped >= start && clamped < end {
                let progress = (clamped - start) as f64 / chapter.chapter_count as f64;
                return Some((spine_index, progress));
            }
        }
        None
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChapterInfo {
    pub spine_index: Option<usize>,
    pub current_total: usize,
    pub chapter_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment_offsets: Option<HashMap<String, usize>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub chapter_index: usize,
    pub progress: f64,
    pub character_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BookShelf {
    pub name: String,
    pub book_ids: Vec<String>,
}

fn archived_book_dir(app: &AppHandle, id: &str) -> Option<PathBuf> {
    fs::read_dir(crate::statistics::archive_directory(app))
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|dir| {
            read_json::<BookMetadata>(&dir.join(METADATA_FILE))
                .is_some_and(|metadata| metadata.id == id)
        })
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, data).map_err(|error| error.to_string())
}

pub(crate) fn read_book_json<T: DeserializeOwned>(
    app: &AppHandle,
    id: &str,
    file: &str,
) -> Option<T> {
    read_json(&book_dir(app, id)?.join(file))
}

static RE_BODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<body.*?</body>").unwrap());
static RE_RT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<rt[^>]*>.*?</rt>").unwrap());
static RE_RP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<rp[^>]*>.*?</rp>").unwrap());
static RE_SCRIPT_STYLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<(?:script|style)[^>]*>.*?</(?:script|style)>").unwrap());
static RE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static RE_NUMERIC_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&#[xX]?[0-9A-Fa-f]+;").unwrap());
pub const CHARACTER_CLASS: &str = r"0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\p{Radical}\p{Unified_Ideograph}";
static RE_NON_MATCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[^{CHARACTER_CLASS}]")).unwrap());

pub fn body(html: &str) -> &str {
    RE_BODY.find(html).map_or(html, |m| m.as_str())
}

pub fn filtered(html: &str) -> String {
    RE_NON_MATCH.replace_all(&strip(html), "").into_owned()
}

pub fn strip(html: &str) -> String {
    let s = RE_RT.replace_all(body(html), "");
    let s = RE_RP.replace_all(&s, "");
    let s = RE_SCRIPT_STYLE.replace_all(&s, "");
    let s = RE_TAG.replace_all(&s, "");
    RE_NUMERIC_ENTITY
        .replace_all(&s, "")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

pub fn count_chars(html: &str) -> usize {
    UnicodeSegmentation::graphemes(filtered(html).as_str(), true).count()
}

#[tauri::command(async)]
pub fn load_book_info(app: AppHandle, id: String) -> BookInfo {
    read_book_json(&app, &id, BOOKINFO_FILE).unwrap_or_default()
}

#[tauri::command(async)]
pub fn load_bookmark(app: AppHandle, id: String) -> Option<Bookmark> {
    read_book_json(&app, &id, BOOKMARK_FILE)
}

#[tauri::command(async)]
pub fn load_pages(app: AppHandle, id: String) -> Option<serde_json::Value> {
    read_book_json(&app, &id, PAGES_FILE)
}

#[tauri::command(async)]
pub fn save_pages(app: AppHandle, id: String, pages: serde_json::Value) {
    if let Some(dir) = book_dir(&app, &id) {
        write_json(&dir.join(PAGES_FILE), &pages).ok();
    }
}

#[tauri::command]
pub fn mark_book_read(app: AppHandle, id: String) {
    let Some(dir) = book_dir(&app, &id) else {
        return;
    };
    let Some(info) = read_json::<BookInfo>(&dir.join(BOOKINFO_FILE)) else {
        return;
    };
    let bookmark = Bookmark {
        chapter_index: info
            .chapter_info
            .values()
            .filter_map(|chapter| chapter.spine_index)
            .max()
            .unwrap_or(0),
        progress: 1.0,
        character_count: info.character_count,
        last_modified: Some(now_apple()),
    };
    write_json(&dir.join(BOOKMARK_FILE), &bookmark).ok();
    let folder = load_metadata_at(&dir).unwrap().folder;
    crate::sync::storage::shared()
        .handle_book_change(&folder)
        .ok();
}

#[tauri::command]
pub fn save_bookmark(
    app: AppHandle,
    id: String,
    chapter_index: usize,
    progress: f64,
    character_count: usize,
) {
    let Some(dir) = book_dir(&app, &id) else {
        return;
    };
    let path = dir.join(BOOKMARK_FILE);
    let stored = read_json::<Bookmark>(&path);
    let bookmark = Bookmark {
        chapter_index,
        progress,
        character_count,
        last_modified: if stored.as_ref().map(|stored| stored.character_count)
            == Some(character_count)
        {
            stored.and_then(|stored| stored.last_modified)
        } else {
            Some(now_apple())
        },
    };
    write_json(&path, &bookmark).ok();
    let folder = load_metadata_at(&dir).unwrap().folder;
    crate::sync::storage::shared()
        .handle_book_change(&folder)
        .ok();
}

pub const APPLE_EPOCH: f64 = 978_307_200.0;
pub const DISTANT_PAST: f64 = -63_114_076_800.0;

pub fn now_apple() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - APPLE_EPOCH
}

pub fn now_ms() -> i64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.0)
        .round() as i64
}

pub fn apple_to_ms(apple: f64) -> i64 {
    ((apple + APPLE_EPOCH) * 1000.0).round() as i64
}

pub fn ms_to_apple(ms: i64) -> f64 {
    ms as f64 / 1000.0 - APPLE_EPOCH
}

pub(crate) fn app_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap()
}

pub(crate) fn books_dir(app: &AppHandle) -> PathBuf {
    let dir = app_dir(app).join(BOOKS_DIR);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn title_from_epub(epub: &Epub, epub_name: &str) -> String {
    epub.metadata()
        .title()
        .map(|title| title.value().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| {
            Path::new(epub_name)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_string())
        })
}

fn author_from_epub(epub: &Epub) -> Option<String> {
    epub.metadata()
        .creators()
        .next()
        .map(|creator| creator.value().trim().to_string())
        .filter(|author| !author.is_empty())
}

static RE_FILE_NAME_SEPARATORS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[\\/:*?"<>|\p{Cc}\p{Cf}\u{2028}\u{2029}]"#).unwrap());

pub(crate) fn sanitize_file_name(title: &str) -> String {
    RE_FILE_NAME_SEPARATORS
        .replace_all(title, "_")
        .trim()
        .to_string()
}

pub(crate) fn book_dir(app: &AppHandle, id: &str) -> Option<PathBuf> {
    fs::read_dir(books_dir(app))
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .find(|path| {
            read_json::<BookMetadata>(&path.join(METADATA_FILE))
                .is_some_and(|metadata| metadata.id == id)
        })
}

pub(crate) fn load_metadata(app: &AppHandle, id: &str) -> Option<BookMetadata> {
    read_book_json(app, id, METADATA_FILE)
}

pub(crate) fn load_metadata_at(root: &Path) -> Option<BookMetadata> {
    read_json(&root.join(METADATA_FILE))
}

pub(crate) fn save_metadata(metadata: &BookMetadata, directory: &Path) -> Result<(), String> {
    write_json(&directory.join(METADATA_FILE), metadata)
}

pub(crate) fn load_book_info_at(root: &Path) -> Option<BookInfo> {
    read_json(&root.join(BOOKINFO_FILE))
}

pub(crate) fn load_bookmark_at(root: &Path) -> Option<Bookmark> {
    read_json(&root.join(BOOKMARK_FILE))
}

pub(crate) fn delete(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| error.to_string())
}

pub(crate) fn load_all_books(app: &AppHandle) -> Vec<BookMetadata> {
    fs::read_dir(books_dir(app))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| load_metadata_at(&path))
        .collect()
}

pub(crate) fn cover_url(app: &AppHandle, book: &BookMetadata) -> Option<PathBuf> {
    let cover = book.cover.as_ref()?;
    if cover.starts_with('/') {
        return Some(PathBuf::from(cover));
    }
    let app_dir = app_dir(app);
    Some(find_path(&app_dir, cover).unwrap_or_else(|| app_dir.join(cover)))
}

pub(crate) fn find_path(directory: &Path, path: &str) -> Option<PathBuf> {
    path.split('/')
        .try_fold(directory.to_path_buf(), |dir, name| {
            fs::read_dir(dir)
                .ok()?
                .filter_map(Result::ok)
                .find_map(|entry| {
                    entry
                        .file_name()
                        .to_str()?
                        .nfc()
                        .eq(name.nfc())
                        .then(|| entry.path())
                })
        })
}

pub fn book_epub_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let dir = book_dir(app, id)?;
    let metadata = load_metadata_at(&dir)?;
    find_path(&dir, &metadata.epub?)
}

pub(crate) fn cover_bytes(app: &AppHandle, id: &str) -> Option<(Vec<u8>, String)> {
    let path = cover_path(app, id)?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some((fs::read(path).ok()?, ext))
}

pub(crate) fn cover_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let metadata = load_metadata(app, id)?;
    let path = find_path(&app_dir(app), &metadata.cover?)?;
    path.is_file().then_some(path)
}

pub(crate) fn thumb_size(request: &Request<Vec<u8>>) -> Option<u32> {
    request.uri().query()?.strip_prefix("w=")?.parse().ok()
}

pub(crate) fn fit_image(bytes: Vec<u8>, max: u32) -> Vec<u8> {
    let Ok(img) = image::load_from_memory(&bytes) else {
        return bytes;
    };
    if img.width() <= max && img.height() <= max {
        return bytes;
    }

    let scaled = img.resize(max, max, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    let encoded = if matches!(image::guess_format(&bytes), Ok(image::ImageFormat::Png)) {
        scaled
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .is_ok()
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&scaled.to_rgb8())
            .is_ok()
    };
    if encoded { out } else { bytes }
}

fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "image/jpeg",
    }
}

static RE_BODY_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<body[^>]*>").unwrap());
static RE_IMAGE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(?:img|image)\b[^>]*>").unwrap());
static RE_IMAGE_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:src|xlink:href)="([^"]+)""#).unwrap());
static RE_GAIJI_CLASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"class="[^"]*\bgaiji"#).unwrap());
static RE_IMAGE_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(?:img|image)\b").unwrap());

pub fn image_starts(body: &str) -> impl Iterator<Item = usize> + '_ {
    RE_IMAGE_START.find_iter(body).map(|m| m.start())
}

pub(crate) fn normalize_resource_key(value: &str) -> String {
    value.trim_start_matches('/').to_string()
}

fn collect_toc_fragments(entry: &EpubTocEntry, map: &mut HashMap<String, Vec<String>>) {
    for child in entry.iter() {
        if let (Some(key), Some(fragment)) = (
            child
                .manifest_entry()
                .and_then(|m| m.resource().key().value().map(normalize_resource_key)),
            child
                .href()
                .and_then(|href| href.fragment())
                .map(|f| urlencoding::decode(f).map_or_else(|_| f.to_string(), |d| d.into_owned())),
        ) {
            map.entry(key).or_default().push(fragment);
        }
        collect_toc_fragments(&child, map);
    }
}

fn fragment_offsets(content: &str, fragments: &[String]) -> HashMap<String, usize> {
    let mut offsets = HashMap::new();
    let Some(open) = RE_BODY_OPEN.find(content) else {
        return offsets;
    };
    let body = &content[open.end()..];
    for fragment in fragments {
        let Some(pos) = body.find(&format!("id=\"{fragment}\"")) else {
            continue;
        };
        let Some(tag) = body[..pos].rfind('<') else {
            continue;
        };
        offsets.insert(fragment.clone(), count_chars(&body[..tag]));
    }
    offsets
}

fn image_paths(content: &str, chapter_key: &str, manifest_keys: &HashSet<String>) -> Vec<String> {
    RE_IMAGE_TAG
        .find_iter(content)
        .filter(|tag| !RE_GAIJI_CLASS.is_match(tag.as_str()))
        .filter_map(|tag| {
            RE_IMAGE_SRC
                .captures(tag.as_str())
                .map(|c| c[1].to_string())
        })
        .filter_map(|src| {
            let mut components: Vec<&str> =
                chapter_key.split('/').filter(|p| !p.is_empty()).collect();
            components.pop();
            for part in src.split('/').filter(|p| !p.is_empty() && *p != ".") {
                if part == ".." {
                    components.pop();
                } else {
                    components.push(part);
                }
            }
            let path = components.join("/");
            let ext_ok = matches!(
                path.rsplit_once('.').map(|(_, e)| e.to_lowercase()),
                Some(ext) if matches!(ext.as_str(), "jpg" | "jpeg" | "png")
            );
            (ext_ok && manifest_keys.contains(&path)).then_some(path)
        })
        .collect()
}

pub(crate) fn process_book(epub: &Epub) -> BookInfo {
    let mut info = BookInfo::default();
    let mut toc_fragments: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(root) = epub.toc().contents() {
        collect_toc_fragments(&root, &mut toc_fragments);
    }
    let manifest_keys: HashSet<String> = epub
        .manifest()
        .iter()
        .filter_map(|entry| entry.resource().key().value().map(normalize_resource_key))
        .collect();
    let mut images = Vec::new();
    let mut seen_images: HashSet<String> = HashSet::new();
    for (index, entry) in epub.spine().iter().enumerate() {
        let Some(item) = entry.manifest_entry() else {
            continue;
        };
        let Some(href) = item.resource().key().value().map(normalize_resource_key) else {
            continue;
        };
        let Ok(bytes) = item.read_bytes() else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let count = count_chars(&content);
        for image in image_paths(&content, &href, &manifest_keys) {
            if seen_images.insert(image.clone()) {
                images.push(image);
            }
        }
        let offsets = toc_fragments
            .get(&href)
            .map(|fragments| fragment_offsets(&content, fragments));
        info.chapter_info.insert(
            href,
            ChapterInfo {
                spine_index: Some(index),
                current_total: info.character_count,
                chapter_count: count,
                fragment_offsets: offsets,
            },
        );
        info.character_count += count;
    }
    info.images = Some(images);
    info
}

fn find_cover_in_manifest(epub: &Epub) -> Option<rbook::epub::manifest::EpubManifestEntry<'_>> {
    let manifest = epub.manifest();
    manifest
        .by_property("cover-image")
        .next()
        .or_else(|| manifest.cover_image())
        .or_else(|| {
            manifest.iter().find(|entry| {
                entry.id().to_ascii_lowercase().contains("cover")
                    && matches!(
                        entry.media_type().to_ascii_lowercase().as_str(),
                        "image/jpeg" | "image/png" | "image/gif" | "image/svg+xml"
                    )
            })
        })
}

#[tauri::command(async)]
pub fn import_book(app: AppHandle, path: String) -> Result<BookMetadata, String> {
    let epub = Epub::open(&path).map_err(|error| error.to_string())?;
    let source_name = Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let title = title_from_epub(&epub, &source_name);
    let author = author_from_epub(&epub);

    let folder = sanitize_file_name(&title);
    let root = books_dir(&app);
    let dir = find_path(&root, &folder).unwrap_or_else(|| root.join(&folder));
    if load_metadata_at(&dir).is_some_and(|existing| existing.epub.is_some()) {
        return Err(format!(
            "A book with the title \"{title}\" is already imported"
        ));
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    if let Err(error) = fs::copy(&path, dir.join(&source_name)) {
        fs::remove_dir_all(&dir).ok();
        return Err(error.to_string());
    }

    let cover = find_cover_in_manifest(&epub).and_then(|entry| {
        let resource = entry.resource();
        let name = Path::new(resource.key().value()?).file_name()?.to_str()?;
        let bytes = entry.read_bytes().ok()?;
        fs::write(dir.join(name), bytes).ok()?;
        Some(format!("{BOOKS_DIR}/{folder}/{name}"))
    });

    let existing = load_metadata_at(&dir);
    let mut meta = BookMetadata::new(title, author, cover, folder.clone(), now_apple());
    if let Some(existing) = &existing {
        meta.id = existing.id.clone();
    }
    meta.epub = Some(source_name);
    meta.renamed_title = existing
        .as_ref()
        .and_then(|existing| existing.renamed_title.clone());
    meta.shelves = existing.and_then(|existing| existing.shelves);

    let info = process_book(&epub);
    let saved = save_metadata(&meta, &dir)
        .and_then(|_| write_json(&dir.join(BOOKINFO_FILE), &info))
        .and_then(|_| match load_bookmark_at(&dir) {
            Some(bookmark) => {
                let position = info.resolve_character_position(bookmark.character_count);
                write_json(
                    &dir.join(BOOKMARK_FILE),
                    &Bookmark {
                        chapter_index: position.map_or(0, |position| position.0),
                        progress: position.map_or(0.0, |position| position.1),
                        character_count: bookmark.character_count,
                        last_modified: bookmark.last_modified,
                    },
                )
            }
            None => Ok(()),
        });
    if let Err(error) = saved {
        fs::remove_dir_all(&dir).ok();
        return Err(error);
    }
    crate::sync::storage::shared()
        .handle_book_import(&meta, &dir)
        .map_err(|error| error.to_string())?;
    Ok(load_metadata_at(&dir).unwrap())
}

fn book_characters(dir: &Path, meta: &BookMetadata) -> (usize, usize) {
    let info: Option<BookInfo> = read_json(&dir.join(BOOKINFO_FILE));
    let bookmark: Option<Bookmark> = read_json(&dir.join(BOOKMARK_FILE));
    let total = info
        .map(|info| info.character_count)
        .or(meta.character_count.map(|count| count as usize))
        .unwrap_or(0);
    (
        bookmark.map_or(0, |bookmark| bookmark.character_count),
        total,
    )
}

fn repair_missing_cover(dir: &Path, meta: &mut BookMetadata) {
    if meta.cover.is_some() {
        return;
    }
    let Some(folder) = dir.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(name) = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|name| name.starts_with("cover."))
    else {
        return;
    };
    meta.cover = Some(format!("{BOOKS_DIR}/{folder}/{name}"));
    write_json(&dir.join(METADATA_FILE), meta).ok();
}

#[tauri::command(async)]
pub fn list_books(app: AppHandle) -> Vec<BookListItem> {
    load_shelf_list(&app);
    fs::read_dir(books_dir(&app))
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                return None;
            }
            let dir = entry.path();
            if !dir.is_dir() {
                return None;
            }
            let mut meta: BookMetadata = read_json(&dir.join(METADATA_FILE))?;
            repair_missing_cover(&dir, &mut meta);
            let (characters_read, characters_total) = book_characters(&dir, &meta);
            Some(BookListItem {
                progress: if characters_total > 0 {
                    characters_read as f64 / characters_total as f64
                } else {
                    0.0
                },
                characters_read,
                characters_total,
                has_book_info: dir.join(BOOKINFO_FILE).exists(),
                meta,
            })
        })
        .collect()
}

#[tauri::command]
pub fn delete_book(app: AppHandle, id: String) -> Result<(), String> {
    let Some(dir) = book_dir(&app, &id) else {
        return Ok(());
    };
    let key: String = load_metadata_at(&dir).unwrap().folder.nfc().collect();
    crate::sync::storage::shared().prepare_book(&dir);
    crate::sync::storage::shared()
        .delete_book(&key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_local_book(app: AppHandle, id: String) -> Result<(), String> {
    let Some(dir) = book_dir(&app, &id) else {
        return Ok(());
    };
    let key: String = load_metadata_at(&dir).unwrap().folder.nfc().collect();
    crate::sync::storage::shared()
        .delete_local_book(&key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn rename_book(app: AppHandle, id: String, title: String) {
    let Some(dir) = book_dir(&app, &id) else {
        return;
    };
    let mut meta = load_metadata_at(&dir).unwrap();
    meta.renamed_title = (!title.is_empty()).then_some(title);
    meta.modified = Some(now_ms());
    save_metadata(&meta, &dir).ok();
    crate::sync::storage::shared()
        .handle_book_change(&meta.folder)
        .ok();
}

#[tauri::command(async)]
pub fn epub_author(app: AppHandle, id: String) -> Option<String> {
    author_from_epub(&Epub::open(book_epub_path(&app, &id)?).ok()?)
}

#[tauri::command]
pub fn set_book_author(app: AppHandle, id: String, author: String) {
    let Some(dir) = book_dir(&app, &id) else {
        return;
    };
    let mut meta = load_metadata_at(&dir).unwrap();
    meta.author = (!author.is_empty()).then_some(author);
    meta.modified = Some(now_ms());
    save_metadata(&meta, &dir).ok();
    crate::sync::storage::shared()
        .handle_book_change(&meta.folder)
        .ok();
}

fn shelves_path(app: &AppHandle) -> PathBuf {
    books_dir(app).join(SHELVES_FILE)
}

pub type ShelfList = HashMap<String, Timestamped<Option<i64>>>;

pub(crate) fn load_shelf_list(app: &AppHandle) -> ShelfList {
    let books_directory = books_dir(app);
    let url = shelves_path(app);
    if let Some(shelves) = read_json::<ShelfList>(&url) {
        return shelves;
    }

    let Some(legacy) = read_json::<Vec<BookShelf>>(&url) else {
        return HashMap::new();
    };
    let mut shelves = HashMap::new();
    let mut books: HashMap<String, BookMetadata> = load_all_books(app)
        .into_iter()
        .map(|book| (book.id.clone(), book))
        .collect();
    for (index, shelf) in legacy.iter().enumerate() {
        let name: String = shelf.name.nfc().collect();
        shelves.insert(
            name.clone(),
            Timestamped {
                modified: 0,
                value: Some(index as i64),
            },
        );
        for id in &shelf.book_ids {
            if let Some(book) = books.get_mut(id) {
                book.shelves.get_or_insert_with(HashMap::new).insert(
                    name.clone(),
                    Timestamped {
                        modified: 0,
                        value: true,
                    },
                );
            }
        }
    }
    books
        .values()
        .filter(|book| book.shelves.is_some())
        .try_for_each(|book| {
            save_metadata(
                book,
                &find_path(&books_directory, &book.folder)
                    .unwrap_or_else(|| books_directory.join(&book.folder)),
            )
        })
        .and_then(|_| save_shelf_list(app, &shelves))
        .ok();
    shelves
}

pub(crate) fn save_shelf_list(app: &AppHandle, shelves: &ShelfList) -> Result<(), String> {
    write_json(&shelves_path(app), shelves)
}

#[tauri::command(async)]
pub fn load_shelves(app: AppHandle) -> Vec<BookShelf> {
    let list = load_shelf_list(&app);
    let books = load_all_books(&app);
    let mut shelves: Vec<(String, i64)> = list
        .into_iter()
        .filter_map(|(name, shelf)| shelf.value.map(|position| (name, position)))
        .collect();
    shelves.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
    shelves
        .into_iter()
        .map(|(name, _)| BookShelf {
            book_ids: books
                .iter()
                .filter(|book| is_member(book, &name))
                .map(|book| book.id.clone())
                .collect(),
            name,
        })
        .collect()
}

fn is_member(book: &BookMetadata, name: &str) -> bool {
    book.shelves
        .as_ref()
        .and_then(|shelves| shelves.get(name))
        .is_some_and(|member| member.value)
}

fn update_shelf_list(app: &AppHandle, update: impl FnOnce(&mut ShelfList)) {
    let mut list = load_shelf_list(app);
    update(&mut list);
    save_shelf_list(app, &list).ok();
    crate::sync::storage::shared().handle_shelves_change().ok();
}

fn update_memberships(app: &AppHandle, memberships: &[(String, bool)], book_id: &str) {
    let Some(root) = book_dir(app, book_id) else {
        return;
    };
    let mut metadata = load_metadata_at(&root).unwrap();
    let shelves = metadata.shelves.get_or_insert_default();
    let mut changed = false;
    for (name, member) in memberships {
        if shelves.get(name).is_some_and(|shelf| shelf.value) != *member {
            shelves.insert(
                name.clone(),
                Timestamped {
                    modified: now_ms(),
                    value: *member,
                },
            );
            changed = true;
        }
    }
    if changed {
        save_metadata(&metadata, &root).ok();
        crate::sync::storage::shared()
            .handle_book_change(&metadata.folder)
            .ok();
    }
}

#[tauri::command]
pub fn create_shelf(app: AppHandle, name: String) {
    let name: String = name.nfc().collect();
    if load_shelves(app.clone())
        .iter()
        .any(|shelf| shelf.name == name)
    {
        return;
    }
    for book in load_all_books(&app) {
        if is_member(&book, &name) {
            update_memberships(&app, &[(name.clone(), false)], &book.id);
        }
    }
    update_shelf_list(&app, |list| {
        let position = list
            .values()
            .filter_map(|shelf| shelf.value)
            .max()
            .unwrap_or(-1)
            + 1;
        list.insert(
            name,
            Timestamped {
                modified: now_ms(),
                value: Some(position),
            },
        );
    });
}

#[tauri::command]
pub fn delete_shelf(app: AppHandle, name: String) {
    let Some(shelf) = load_shelves(app.clone())
        .into_iter()
        .find(|shelf| shelf.name == name)
    else {
        return;
    };
    for id in &shelf.book_ids {
        update_memberships(&app, &[(name.clone(), false)], id);
    }
    update_shelf_list(&app, |list| {
        list.insert(
            name,
            Timestamped {
                modified: now_ms(),
                value: None,
            },
        );
    });
}

#[tauri::command]
pub fn rename_shelf(app: AppHandle, name: String, new_name: String) {
    let new_name: String = new_name.nfc().collect();
    let shelves = load_shelves(app.clone());
    if shelves.iter().any(|shelf| shelf.name == new_name) {
        return;
    }
    let Some(shelf) = shelves.into_iter().find(|shelf| shelf.name == name) else {
        return;
    };
    for book in load_all_books(&app) {
        let member = shelf.book_ids.contains(&book.id);
        if member || is_member(&book, &new_name) {
            update_memberships(
                &app,
                &[(name.clone(), false), (new_name.clone(), member)],
                &book.id,
            );
        }
    }
    update_shelf_list(&app, |list| {
        let position = list.get(&name).and_then(|shelf| shelf.value);
        list.insert(
            name,
            Timestamped {
                modified: now_ms(),
                value: None,
            },
        );
        list.insert(
            new_name,
            Timestamped {
                modified: now_ms(),
                value: position,
            },
        );
    });
}

#[tauri::command]
pub fn move_shelves(app: AppHandle, names: Vec<String>) {
    update_shelf_list(&app, |list| {
        for (index, name) in names.into_iter().enumerate() {
            list.insert(
                name,
                Timestamped {
                    modified: now_ms(),
                    value: Some(index as i64),
                },
            );
        }
    });
}

#[tauri::command]
pub fn move_book(app: AppHandle, id: String, name: Option<String>) {
    let memberships: Vec<(String, bool)> = load_shelves(app.clone())
        .into_iter()
        .map(|shelf| {
            let member = Some(&shelf.name) == name.as_ref();
            (shelf.name, member)
        })
        .collect();
    update_memberships(&app, &memberships, &id);
}

pub fn cover_protocol(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let id = request.uri().path().trim_start_matches('/');
    let max = thumb_size(&request);

    let found = if let Some(book_id) = id.strip_prefix("archive/") {
        archived_book_dir(app, book_id)
            .and_then(|dir| fs::read(dir.join(STATISTICS_COVER)).ok())
            .map(|bytes| (mime_for("jpg"), bytes))
    } else {
        cover_bytes(app, id).map(|(bytes, ext)| (mime_for(&ext), bytes))
    };
    let Some((mime, bytes)) = found else {
        return Response::builder().status(404).body(Vec::new()).unwrap();
    };
    Response::builder()
        .header(CONTENT_TYPE, mime)
        .body(match max {
            Some(m) => fit_image(bytes, m),
            None => bytes,
        })
        .unwrap()
}
