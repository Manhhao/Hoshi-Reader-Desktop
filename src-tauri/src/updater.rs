use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

async fn available(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    Ok(available(&app).await?.map(|update| update.version))
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let Some(update) = available(&app).await? else {
        return Ok(());
    };
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk;
                app.emit(
                    "update://progress",
                    json!({ "downloaded": downloaded, "total": total }),
                )
                .ok();
            },
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(
        Duration::from_secs(10),
        crate::sync::gdrive::manager::pause(),
    )
    .await
    .ok();
    update.install(bytes).map_err(|error| error.to_string())?;
    app.request_restart();
    Ok(())
}
