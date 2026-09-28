pub type Fallible<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod anki;
mod backup;
mod book;
mod crash;
mod css;
mod dict;
mod fonts;
mod highlights;
mod library;
mod local_audio;
mod menu_theme;
mod pcm;
mod sasayaki;
mod search;
mod statistics;
mod sync;
#[cfg(target_os = "macos")]
mod transcriber;
mod ttu_statistics;
mod updater;
#[cfg(target_os = "macos")]
mod writing_tools;

use book::OpenBook;
use dict::LookupState;

static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn run() {
    #[cfg(target_os = "macos")]
    writing_tools::disable();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .manage(OpenBook::default())
        .manage(LookupState::default())
        .manage(anki::AnkiState::default())
        .register_asynchronous_uri_scheme_protocol("book", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(book::book_protocol(&app, request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(library::cover_protocol(&app, request));
            });
        })
        .register_asynchronous_uri_scheme_protocol("audiobook", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(sasayaki::audiobook_protocol(&app, request));
            });
        })
        .register_uri_scheme_protocol("image", dict::image_protocol)
        .register_asynchronous_uri_scheme_protocol("audio", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                let (status, content_type, body) =
                    if let Some((source, file)) = local_audio::file_params(&uri) {
                        match local_audio::audio_bytes(&app, &source, &file) {
                            Some(body) => (200, local_audio::audio_format(&body).1, body),
                            None => (404, "text/plain", Vec::new()),
                        }
                    } else {
                        match anki::fetch_audio_source_list(&app, &uri).await {
                            Ok(body) => (200, "application/json", body),
                            Err(_) => (502, "application/json", Vec::new()),
                        }
                    };
                let response = tauri::http::Response::builder()
                    .status(status)
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Content-Type", content_type)
                    .body(body)
                    .unwrap();
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            menu_theme::set_menu_theme,
            book::open_book,
            book::load_contents,
            book::save_book_image,
            library::import_book,
            library::list_books,
            library::delete_book,
            library::rename_book,
            library::epub_author,
            library::set_book_author,
            library::load_book_info,
            search::search_book,
            highlights::load_highlights,
            highlights::save_highlights,
            library::load_bookmark,
            library::mark_book_read,
            library::save_bookmark,
            library::delete_local_book,
            library::load_shelves,
            library::create_shelf,
            library::delete_shelf,
            library::rename_shelf,
            library::move_shelves,
            library::move_book,
            statistics::load_statistics,
            statistics::save_reading_session,
            statistics::edit_reading_session,
            statistics::delete_reading_sessions,
            statistics::load_all_statistics,
            statistics::clear_statistics_archive,
            sasayaki::sasayaki_match,
            sasayaki::sasayaki_load_match,
            sasayaki::sasayaki_load_playback,
            sasayaki::sasayaki_save_playback,
            sasayaki::sasayaki_audio_chapters,
            #[cfg(target_os = "macos")]
            transcriber::sasayaki_transcriber_status,
            #[cfg(not(target_os = "macos"))]
            sasayaki::sasayaki_transcriber_status,
            #[cfg(target_os = "macos")]
            transcriber::sasayaki_load_transcript,
            #[cfg(target_os = "macos")]
            transcriber::sasayaki_clear_transcript,
            #[cfg(target_os = "macos")]
            transcriber::sasayaki_pause_transcription,
            #[cfg(target_os = "macos")]
            transcriber::sasayaki_transcribe,
            backup::backup_folder,
            backup::restore_folder,
            dict::load_collapsed_dictionaries,
            dict::save_collapsed_dictionaries,
            dict::lookup,
            dict::lookup_kanji,
            dict::import_dictionaries,
            dict::update_dictionaries,
            dict::list_dictionaries,
            dict::set_dictionary_enabled,
            dict::set_dictionary_category,
            dict::delete_dictionary,
            dict::reorder_dictionaries,
            fonts::list_fonts,
            fonts::import_fonts,
            fonts::download_stroke_order_font,
            fonts::delete_font,
            anki::anki_config,
            anki::anki_get_settings,
            anki::anki_reachable,
            anki::anki_save_settings,
            anki::anki_ping,
            anki::anki_fetch,
            anki::anki_autofill_fields,
            anki::anki_check_duplicates,
            anki::anki_mine,
            anki::anki_show_notes,
            sync::sync_configure,
            sync::sync_connect,
            sync::sync_disconnect,
            sync::sync_clear_cache,
            sync::sync_authenticated,
            sync::gdrive::gdrive_sync_start,
            sync::gdrive::gdrive_sync_pause,
            sync::gdrive::gdrive_sync_stop,
            sync::gdrive::gdrive_sync_now,
            sync::gdrive::gdrive_sync_book,
            sync::gdrive::gdrive_sync_state,
            sync::gdrive::gdrive_published_epub,
            sync::gdrive::gdrive_cancel_download,
            sync::gdrive::gdrive_download_book,
            sync::open_external,
            updater::check_update,
            updater::install_update
        ])
        .setup(|app| {
            crash::init(app.handle());
            sync::init(app.handle());
            dict::initialize(app.handle());
            Ok(())
        })
        .on_window_event(|_window, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = _event {
                use tauri::Emitter;
                api.prevent_close();
                let _ = _window.hide();
                let _ = _window.emit("hoshi://hidden", ());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = &event
                && !EXITING.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        sync::gdrive::manager::pause(),
                    )
                    .await
                    .ok();
                    app.exit(0);
                });
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                use tauri::Manager;
                for window in handle.webview_windows().values() {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
