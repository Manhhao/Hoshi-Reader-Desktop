use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

const FONT_EXTENSIONS: [&str; 5] = ["ttf", "otf", "ttc", "woff", "woff2"];
const FONTS_DIR: &str = "Fonts";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontInfo {
    pub name: String,
    pub file_name: String,
}

fn fonts_dir(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().unwrap().join(FONTS_DIR);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn font_path(app: &AppHandle, file_name: &str) -> PathBuf {
    fonts_dir(app).join(file_name)
}

fn font_info(path: &Path) -> Option<FontInfo> {
    if !path.is_file() {
        return None;
    }
    let ext = path.extension()?.to_str()?.to_lowercase();
    if !FONT_EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    Some(FontInfo {
        name: path.file_stem()?.to_str()?.to_string(),
        file_name: path.file_name()?.to_str()?.to_string(),
    })
}

#[tauri::command]
pub fn list_fonts(app: AppHandle) -> Vec<FontInfo> {
    let mut fonts: Vec<FontInfo> = fs::read_dir(fonts_dir(&app))
        .into_iter()
        .flatten()
        .filter_map(|entry| font_info(&entry.ok()?.path()))
        .collect();
    fonts.sort_by(|a, b| a.name.cmp(&b.name));
    fonts
}

#[tauri::command]
pub fn import_fonts(app: AppHandle, paths: Vec<String>) {
    let dir = fonts_dir(&app);
    for path in paths {
        let src = PathBuf::from(path);
        let Some(info) = font_info(&src) else {
            continue;
        };
        let destination = dir.join(info.file_name);
        if destination.exists() {
            continue;
        }
        fs::copy(&src, &destination).ok();
    }
}

#[tauri::command]
pub async fn download_stroke_order_font(app: AppHandle) -> Result<(), String> {
    let bytes = reqwest::get(
        "https://drive.google.com/uc?export=download&id=1TELymEhF0YMK0Ma-fQlpHNmZLg9Xw3zx",
    )
    .await
    .map_err(|error| error.to_string())?
    .error_for_status()
    .map_err(|error| error.to_string())?
    .bytes()
    .await
    .map_err(|error| error.to_string())?;
    fs::write(fonts_dir(&app).join("KanjiStrokeOrders_v4.005.ttf"), bytes)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_font(app: AppHandle, file_name: String) {
    if file_name.contains(['/', '\\']) {
        return;
    }
    let path = font_path(&app, &file_name);
    fs::remove_file(&path).ok();
}

pub fn font_bytes(app: &AppHandle, file_name: &str) -> Option<(String, Vec<u8>)> {
    if file_name.contains(['/', '\\']) {
        return None;
    }
    let path = font_path(app, file_name);
    let mime = match path.extension()?.to_str()?.to_lowercase().as_str() {
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" => "font/collection",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => return None,
    };
    Some((mime.to_string(), fs::read(path).ok()?))
}
