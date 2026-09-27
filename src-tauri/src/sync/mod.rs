pub mod auth;
pub mod client;
pub mod gdrive;
pub mod model;
pub mod storage;
pub mod task;

use std::sync::{Mutex, Once, OnceLock};

use serde::Deserialize;
use tauri::AppHandle;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enable_sync: bool,
    pub statistics_reset_time: i64,
    pub online: bool,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    enable_sync: false,
    statistics_reset_time: 0,
    online: true,
});

static APP: OnceLock<AppHandle> = OnceLock::new();
static INIT: Once = Once::new();

pub fn settings() -> Settings {
    SETTINGS.lock().unwrap().clone()
}

pub fn app() -> &'static AppHandle {
    APP.get().unwrap()
}

pub fn init(app: &AppHandle) {
    APP.set(app.clone()).ok();
    auth::init_store();
    storage::shared().reload();
}

#[tauri::command]
pub fn sync_configure(settings: Settings) {
    crate::statistics::set_statistics_reset_time(settings.statistics_reset_time);
    *SETTINGS.lock().unwrap() = settings;
    INIT.call_once(gdrive::manager::init);
}

#[tauri::command]
pub fn sync_authenticated() -> bool {
    auth::is_authenticated()
}

#[tauri::command]
pub async fn sync_connect() -> Result<(), String> {
    auth::authenticate().await
}

#[tauri::command]
pub async fn sync_disconnect() -> Result<(), String> {
    gdrive::manager::sign_out()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sync_clear_cache() -> Result<(), String> {
    gdrive::manager::clear_cache()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_external(url: String) {
    if url.starts_with("https://") || url.starts_with("http://") {
        open::that(url).ok();
    }
}
