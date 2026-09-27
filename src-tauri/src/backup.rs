use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::Fallible;
use crate::dict::LookupState;

fn storage_dir(app: &AppHandle, folder: &str) -> Result<PathBuf, String> {
    match folder {
        "Books" => Ok(crate::library::books_dir(app)),
        "Dictionaries" => Ok(crate::dict::dictionaries_dir(app)),
        _ => Err(format!("unsupported backup folder: {folder}")),
    }
}

fn archive_name(path: &Path, root: &Path) -> Fallible<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn add_directory(
    writer: &mut ZipWriter<fs::File>,
    root: &Path,
    directory: &Path,
    options: SimpleFileOptions,
) -> Fallible {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let name = archive_name(&path, root)?;
        if path.is_dir() {
            writer.add_directory(&name, options)?;
            add_directory(writer, root, &path, options)?;
        } else if path.is_file() {
            writer.start_file(&name, options)?;
            io::copy(&mut fs::File::open(&path)?, writer)?;
        }
    }
    Ok(())
}

fn archive_directory(root: &Path, destination: &Path) -> Fallible {
    let mut writer = ZipWriter::new(fs::File::create(destination)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_directory(&mut writer, root, root, options)?;
    writer.finish()?;
    Ok(())
}

fn restore_archive(archive_path: &Path, destination: &Path) -> Fallible {
    let staging = destination.with_file_name(format!(".restore-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let restored = staging.join("restored");
    let previous = staging.join("previous");
    let result = (|| -> Fallible {
        fs::create_dir(&restored)?;
        ZipArchive::new(fs::File::open(archive_path)?)?.extract(&restored)?;
        if destination.exists() {
            fs::rename(destination, &previous)?;
        }
        if let Err(error) = fs::rename(&restored, destination) {
            if previous.exists() {
                fs::rename(&previous, destination).map_err(|rollback| {
                    format!(
                        "Restore failed ({error}); previous data remains at {} ({rollback})",
                        previous.display()
                    )
                })?;
            }
            return Err(error.into());
        }
        Ok(())
    })();
    if result.is_ok() || !previous.exists() {
        fs::remove_dir_all(&staging).ok();
    }
    result
}

#[tauri::command]
pub fn backup_folder(app: AppHandle, folder: String, destination: String) -> Result<(), String> {
    let root = storage_dir(&app, &folder)?;
    archive_directory(&root, Path::new(&destination)).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn restore_folder(
    app: AppHandle,
    state: State<'_, LookupState>,
    folder: String,
    path: String,
) -> Result<(), String> {
    let destination = storage_dir(&app, &folder)?;
    if folder == "Books" {
        crate::sync::gdrive::manager::stop().await;
    }
    {
        let _engine = (folder == "Dictionaries").then(|| state.lock_for_update());
        restore_archive(Path::new(&path), &destination).map_err(|error| error.to_string())?;
    }
    if folder == "Books" {
        crate::sync::gdrive::manager::reset_connection(true).map_err(|error| error.to_string())?;
    }
    Ok(())
}
