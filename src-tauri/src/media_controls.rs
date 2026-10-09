use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Foundation::TypedEventHandler;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{HSTRING, Ref, factory};

use crate::{Fallible, library, sasayaki};

static CONTROLS: OnceLock<SystemMediaTransportControls> = OnceLock::new();

pub const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,HardwareMediaKeyHandling --enable-smooth-scrolling";

pub fn set_app_id(identifier: &str) -> Fallible {
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(identifier))? };
    Ok(())
}

pub fn init(window: &WebviewWindow) -> Fallible {
    let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(window.hwnd()?)? };
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    let app = window.app_handle().clone();
    controls.ButtonPressed(&TypedEventHandler::new(
        move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            let action = match args.ok()?.Button()? {
                SystemMediaTransportControlsButton::Play => "play",
                SystemMediaTransportControlsButton::Pause => "pause",
                SystemMediaTransportControlsButton::Previous => "previoustrack",
                SystemMediaTransportControlsButton::Next => "nexttrack",
                _ => return Ok(()),
            };
            let _ = app.emit("hoshi://media-control", action);
            Ok(())
        },
    ))?;
    let _ = CONTROLS.set(controls);
    Ok(())
}

fn thumbnail(bytes: &[u8]) -> windows::core::Result<RandomAccessStreamReference> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.get()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}

fn update_metadata(app: &AppHandle, id: &str, title: &str, artist: &str) -> Fallible {
    let Some(controls) = CONTROLS.get() else {
        return Ok(());
    };
    let updater = controls.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?;
    let cover = sasayaki::audio_path(app, id)
        .and_then(|path| sasayaki::audio_cover(&path))
        .map(|(_, cover)| cover)
        .or_else(|| library::cover_bytes(app, id).map(|(cover, _)| cover));
    if let Some(cover) = cover {
        updater.SetThumbnail(&thumbnail(&cover)?)?;
    }
    let properties = updater.MusicProperties()?;
    properties.SetTitle(&HSTRING::from(title))?;
    properties.SetArtist(&HSTRING::from(artist))?;
    updater.Update()?;
    controls.SetIsEnabled(true)?;
    Ok(())
}

#[tauri::command]
pub fn media_controls_metadata(app: AppHandle, id: String, title: String, artist: String) {
    let _ = update_metadata(&app, &id, &title, &artist);
}

#[tauri::command]
pub fn media_controls_playing(playing: bool) {
    if let Some(controls) = CONTROLS.get() {
        let _ = controls.SetPlaybackStatus(if playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        });
    }
}

#[tauri::command]
pub fn media_controls_clear() {
    if let Some(controls) = CONTROLS.get() {
        let _ = controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = controls
            .DisplayUpdater()
            .and_then(|updater| updater.ClearAll());
        let _ = controls.SetIsEnabled(false);
    }
}
