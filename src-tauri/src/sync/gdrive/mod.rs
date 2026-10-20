mod files;
pub mod handler;
mod listing;
pub mod manager;
mod state;

use serde_json::json;
use tauri::{AppHandle, Emitter};
use unicode_normalization::UnicodeNormalization;

use crate::library::{self, BookMetadata};
use crate::sync::client::GoogleDriveError;
use crate::sync::model::{SyncError, SyncFileType};
use crate::sync::storage;
use crate::sync::task::SyncTask;

#[tauri::command]
pub fn gdrive_sync_start() {
    manager::start();
}

#[tauri::command]
pub async fn gdrive_sync_pause() {
    manager::pause().await;
}

#[tauri::command]
pub async fn gdrive_sync_stop() {
    manager::stop().await;
}

#[tauri::command]
pub async fn gdrive_sync_now() {
    manager::sync(None).await;
}

#[tauri::command]
pub async fn gdrive_sync_book(app: AppHandle, id: String) -> Option<BookMetadata> {
    manager::sync(Some(library::load_metadata(&app, &id)?)).await;
    library::load_metadata(&app, &id)
}

#[tauri::command]
pub async fn gdrive_attach(choice: manager::AttachChoice) -> Result<(), String> {
    manager::attach(choice)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn gdrive_sync_state() -> manager::SyncStatus {
    manager::status()
}

#[tauri::command(async)]
pub fn gdrive_published_epub(app: AppHandle, id: String) -> bool {
    let Some(book) = library::load_metadata(&app, &id) else {
        return false;
    };
    let key: String = book.folder.nfc().collect();
    storage::shared()
        .state
        .books
        .get(&key)
        .and_then(|record| record.files.get(&SyncFileType::Epub))
        .is_some_and(|file| file.value.is_some())
}

#[tauri::command]
pub fn gdrive_download_all() {
    manager::download_all();
}

#[tauri::command]
pub async fn gdrive_download_book(
    app: AppHandle,
    id: String,
) -> Result<Option<BookMetadata>, String> {
    let book =
        library::load_metadata(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let download_task = SyncTask::spawn(async move {
        let emitter = app.clone();
        let progress_id = id.clone();
        let on_progress = move |progress: f64| {
            emitter
                .emit(
                    "sync://download-progress",
                    json!({ "id": progress_id, "progress": progress }),
                )
                .ok();
        };
        let result = match files::download_book(&book, &on_progress).await {
            Ok(downloaded) => Ok(Some(downloaded)),
            Err(SyncError::Drive(GoogleDriveError::Cancelled)) => Ok(None),
            Err(error) => Err(error.to_string()),
        };
        manager::start_file_sync();
        sender.send(result).ok();
    });
    download_task.start();
    receiver.await.unwrap_or(Ok(None))
}
