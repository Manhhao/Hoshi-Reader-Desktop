use std::sync::Mutex;

use rbook::Epub;
use rbook::epub::toc::EpubTocEntry;
use serde::Serialize;
use tauri::http::{Request, Response, header::CONTENT_TYPE};
use tauri::{AppHandle, Manager, State};

use crate::library::{self, normalize_resource_key};

const READER_STYLE: &str = r#"<style data-hoshi="">:root{color-scheme:light dark}html{opacity:0;background:transparent!important}html,body{overflow:hidden}</style>"#;
const READER_SCRIPTS: &str = concat!(
    r#"<script src="/__hoshi/reader.js"></script>"#,
    r#"<script src="/__hoshi/selection.js"></script>"#,
    r#"<script src="/__hoshi/highlights.js"></script>"#,
    r#"<script src="/__hoshi/paragraph.js"></script>"#,
);
const CONTINUOUS_SCRIPT: &str = r#"<script src="/__hoshi/continuous.js"></script>"#;
const BOOT_SCRIPT: &str = r#"<script src="/__hoshi/boot.js"></script>"#;

#[derive(Default)]
pub struct OpenBook(pub Mutex<Option<(String, Epub)>>);

#[derive(Serialize)]
pub struct BookDocument {
    pub title: String,
    pub spine: Vec<String>,
    pub toc: Vec<TocItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TocItem {
    pub label: String,
    pub spine_index: usize,
    pub fragment: Option<String>,
    pub indent_level: usize,
}

fn collect_toc(entry: &EpubTocEntry, spine: &[String], level: usize, out: &mut Vec<TocItem>) {
    for child in entry.iter() {
        let spine_index = child
            .manifest_entry()
            .and_then(|m| m.resource().key().value().map(normalize_resource_key))
            .and_then(|key| spine.iter().position(|s| s == &key));
        if let Some(spine_index) = spine_index {
            let fragment = child
                .href()
                .and_then(|href| href.fragment())
                .map(|f| urlencoding::decode(f).map_or_else(|_| f.to_string(), |d| d.into_owned()));
            out.push(TocItem {
                label: child.label().to_string(),
                spine_index,
                fragment,
                indent_level: level,
            });
        }
        collect_toc(&child, spine, level + 1, out);
    }
}

#[tauri::command]
pub fn open_book(
    app: AppHandle,
    id: String,
    state: State<OpenBook>,
) -> Result<BookDocument, String> {
    let path =
        library::book_epub_path(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let epub = Epub::open(path).map_err(|error| error.to_string())?;
    let root = library::book_dir(&app, &id).unwrap();
    let document = book_document(&epub);
    let info = library::load_book_info_at(&root);
    if info.is_none() {
        let processed = library::process_book(&epub);
        library::write_json(&root.join(library::BOOKINFO_FILE), &processed).ok();
        if let Some(bookmark) = library::load_bookmark_at(&root) {
            let position = processed.resolve_character_position(bookmark.character_count);
            let resolved = library::Bookmark {
                chapter_index: position.map_or(0, |position| position.0),
                progress: position.map_or(0.0, |position| position.1),
                character_count: bookmark.character_count,
                last_modified: bookmark.last_modified,
            };
            library::write_json(&root.join(library::BOOKMARK_FILE), &resolved).ok();
        }
    } else if info.is_some_and(|info| {
        info.images.is_none()
            || !document
                .spine
                .iter()
                .any(|href| info.chapter_info.contains_key(href))
    }) {
        let processed = library::process_book(&epub);
        library::write_json(&root.join(library::BOOKINFO_FILE), &processed).ok();
    }
    let mut metadata = library::load_metadata_at(&root).unwrap();
    metadata.last_access = library::now_apple();
    library::save_metadata(&metadata, &root).ok();

    *state.0.lock().unwrap() = Some((id, epub));
    Ok(document)
}

fn book_document(epub: &Epub) -> BookDocument {
    let title = epub
        .metadata()
        .title()
        .map(|t| t.value().to_string())
        .unwrap_or_default();

    let spine: Vec<String> = epub
        .spine()
        .iter()
        .filter_map(|entry| entry.manifest_entry())
        .filter_map(|entry| entry.resource().key().value().map(normalize_resource_key))
        .collect();

    let mut toc = Vec::new();
    if let Some(root) = epub.toc().contents() {
        collect_toc(&root, &spine, 0, &mut toc);
    }

    BookDocument { title, spine, toc }
}

#[tauri::command(async)]
pub fn load_contents(app: AppHandle, id: String) -> Result<BookDocument, String> {
    let path =
        library::book_epub_path(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let epub = Epub::open(path).map_err(|error| error.to_string())?;
    Ok(book_document(&epub))
}

#[tauri::command(async)]
pub fn save_book_image(app: AppHandle, path: String, destination: String) -> Result<(), String> {
    let (_, bytes) = serve_resource(&app, &path, false).ok_or("Image not found")?;
    std::fs::write(destination, bytes).map_err(|error| error.to_string())
}

pub fn book_protocol(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = request.uri().path();
    let continuous = request.uri().query().and_then(|query| {
        query
            .split('&')
            .find_map(|item| item.strip_prefix("continuous="))
            .filter(|scope| {
                scope.strip_prefix('s').is_some_and(|number| {
                    !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
                })
            })
    });
    let shell_resource = request
        .uri()
        .query()
        .is_some_and(|query| query.split('&').any(|item| item == "shell=1"));

    if path
        .trim_start_matches('/')
        .split_once('/')
        .is_some_and(|(_, href)| href == "__continuous.html")
    {
        return Response::builder()
            .header(CONTENT_TYPE, "text/html")
            .body(continuous_shell())
            .unwrap();
    }

    if let Some(asset) = path.strip_prefix("/__hoshi/") {
        if let Some(file) = asset.strip_prefix("Fonts/") {
            let file = urlencoding::decode(file)
                .map(|f| f.into_owned())
                .unwrap_or_default();
            return match crate::fonts::font_bytes(app, &file) {
                Some((mime, bytes)) => Response::builder()
                    .header(CONTENT_TYPE, mime)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(bytes)
                    .unwrap(),
                None => Response::builder().status(404).body(Vec::new()).unwrap(),
            };
        }
        let js = match asset {
            "reader.js" => include_str!("../../src/reader/reader.js"),
            "highlights.js" => include_str!("../../src/reader/highlights.js"),
            "selection.js" => include_str!("../../src/reader/selection.js"),
            "paragraph.js" => include_str!("../../src/reader/paragraph.js"),
            "continuous.js" => include_str!("../../src/reader/continuous.js"),
            "boot.js" => include_str!("../../src/reader/boot.js"),
            _ => "",
        };
        return Response::builder()
            .header(CONTENT_TYPE, "text/javascript")
            .header("Access-Control-Allow-Origin", "*")
            .body(js.as_bytes().to_vec())
            .unwrap();
    }

    match serve_resource(app, path, !shell_resource) {
        Some((mime, bytes)) => {
            let bytes = match (continuous, mime.contains("css")) {
                (Some(scope), true) => {
                    crate::css::scope_continuous_css(&String::from_utf8_lossy(&bytes), scope)
                        .into_bytes()
                }
                _ => bytes,
            };
            let bytes = match library::thumb_size(&request) {
                Some(max) if mime.starts_with("image/") => library::fit_image(bytes, max),
                _ => bytes,
            };
            Response::builder()
                .header(CONTENT_TYPE, mime)
                .body(bytes)
                .unwrap()
        }
        None => Response::builder().status(404).body(Vec::new()).unwrap(),
    }
}

fn serve_resource(app: &AppHandle, path: &str, inject_reader: bool) -> Option<(String, Vec<u8>)> {
    let (id, href) = path.trim_start_matches('/').split_once('/')?;
    let href = urlencoding::decode(href).ok()?;

    let state = app.state::<OpenBook>();
    let mut guard = state.0.lock().unwrap();
    if !matches!(guard.as_ref(), Some((open_id, _)) if open_id == id) {
        let epub_path = library::book_epub_path(app, id)?;
        let epub = Epub::open(epub_path).ok()?;
        *guard = Some((id.to_string(), epub));
    }
    let epub = &guard.as_ref().unwrap().1;

    let entry = epub.manifest().iter().find(|entry| {
        entry
            .resource()
            .key()
            .value()
            .map(normalize_resource_key)
            .as_deref()
            == Some(href.as_ref())
    })?;

    let mime = entry.kind().as_str().to_string();
    let bytes = entry.read_bytes().ok()?;

    let bytes = if inject_reader && mime.contains("html") {
        inject_scripts(bytes)
    } else if mime.contains("css") {
        crate::css::sanitize_css(&String::from_utf8_lossy(&bytes)).into_bytes()
    } else {
        bytes
    };

    Some((mime, bytes))
}

fn inject_scripts(bytes: Vec<u8>) -> Vec<u8> {
    let reader_tags = reader_tags(false);
    let mut html = String::from_utf8_lossy(&bytes).into_owned();
    let head_end = html
        .find("<head>")
        .map(|pos| pos + "<head>".len())
        .or_else(|| {
            html.find("<head ")
                .and_then(|pos| html[pos..].find('>').map(|end| pos + end + 1))
        });
    if let Some(pos) = head_end {
        html.insert_str(pos, READER_STYLE);
    }
    let tags = if head_end.is_some() {
        reader_tags
    } else {
        format!("{READER_STYLE}{reader_tags}")
    };
    match html.rfind("</body>") {
        Some(pos) => html.insert_str(pos, &tags),
        None => html.push_str(&tags),
    }
    html.into_bytes()
}

fn continuous_shell() -> Vec<u8> {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">{READER_STYLE}</head><body>{}</body></html>",
        reader_tags(true)
    )
        .into_bytes()
}

fn reader_tags(continuous: bool) -> String {
    format!(
        "{READER_SCRIPTS}{}{BOOT_SCRIPT}",
        if continuous { CONTINUOUS_SCRIPT } else { "" }
    )
}
