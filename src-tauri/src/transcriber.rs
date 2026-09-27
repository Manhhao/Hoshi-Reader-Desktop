use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::ipc::Channel;
use tokio::sync::{mpsc, oneshot};

use crate::library;
use crate::sasayaki::{self, SasayakiMatchData, Source};

const TRANSCRIPT_FILE: &str = "sasayaki_transcript.json";

unsafe extern "C" {
    fn hoshi_transcriber_status(context: *mut c_void, callback: extern "C" fn(*mut c_void, i32));
    fn hoshi_audio_duration(path: *const c_char) -> f64;
    fn hoshi_transcribe(
        path: *const c_char,
        from: f64,
        context: *mut c_void,
        on_download: extern "C" fn(*mut c_void, f64),
        on_tokens: extern "C" fn(*mut c_void, *const c_char, f64),
        on_finish: extern "C" fn(*mut c_void, *const c_char),
    ) -> *mut c_void;
    fn hoshi_transcribe_cancel(handle: *mut c_void);
    fn hoshi_transcribe_release(handle: *mut c_void);
    fn hoshi_align(
        source: *const c_char,
        tokens: *const c_char,
        context: *mut c_void,
        callback: extern "C" fn(*mut c_void, *const c_char),
    );
}

#[derive(Serialize, Deserialize)]
struct SasayakiToken {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Serialize, Deserialize)]
struct SasayakiTranscript {
    through: f64,
    duration: f64,
    tokens: Vec<SasayakiToken>,
}

#[derive(Serialize, Deserialize)]
pub struct TranscriptProgress {
    through: f64,
    duration: f64,
}

impl SasayakiTranscript {
    fn is_complete(&self) -> bool {
        self.duration > 0.0 && self.through + 1.5 >= self.duration
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Progress {
    Downloading {
        fraction: f64,
    },
    Transcribing {
        through: f64,
        duration: f64,
        remaining: Option<f64>,
    },
    Aligning,
}

enum Event {
    Download(f64),
    Tokens(Vec<SasayakiToken>, f64),
    Finish(Option<String>),
}

type Events = mpsc::UnboundedSender<Event>;

struct Transcription(*mut c_void);

unsafe impl Send for Transcription {}

impl Drop for Transcription {
    fn drop(&mut self) {
        unsafe { hoshi_transcribe_release(self.0) }
    }
}

static RUNNING: Mutex<Option<Transcription>> = Mutex::new(None);

extern "C" fn on_status(context: *mut c_void, status: i32) {
    let sender = unsafe { Box::from_raw(context.cast::<oneshot::Sender<i32>>()) };
    sender.send(status).ok();
}

extern "C" fn on_download(context: *mut c_void, fraction: f64) {
    let events = unsafe { &*context.cast::<Events>() };
    events.send(Event::Download(fraction)).ok();
}

extern "C" fn on_tokens(context: *mut c_void, json: *const c_char, through: f64) {
    let events = unsafe { &*context.cast::<Events>() };
    let tokens = serde_json::from_slice(unsafe { CStr::from_ptr(json) }.to_bytes()).unwrap();
    events.send(Event::Tokens(tokens, through)).ok();
}

extern "C" fn on_finish(context: *mut c_void, error: *const c_char) {
    let events = unsafe { Box::from_raw(context.cast::<Events>()) };
    let error = (!error.is_null()).then(|| {
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    });
    events.send(Event::Finish(error)).ok();
}

extern "C" fn on_aligned(context: *mut c_void, json: *const c_char) {
    let result = unsafe { &mut *context.cast::<Option<SasayakiMatchData>>() };
    *result = Some(serde_json::from_slice(unsafe { CStr::from_ptr(json) }.to_bytes()).unwrap());
}

fn align(source: &Source, tokens: &[SasayakiToken]) -> SasayakiMatchData {
    let source = CString::new(serde_json::to_vec(source).unwrap()).unwrap();
    let tokens = CString::new(serde_json::to_vec(tokens).unwrap()).unwrap();
    let mut result = None;
    unsafe {
        hoshi_align(
            source.as_ptr(),
            tokens.as_ptr(),
            (&raw mut result).cast(),
            on_aligned,
        )
    };
    result.unwrap()
}

#[tauri::command]
pub async fn sasayaki_transcriber_status() -> &'static str {
    let (sender, receiver) = oneshot::channel();
    unsafe { hoshi_transcriber_status(Box::into_raw(Box::new(sender)).cast(), on_status) };
    match receiver.await.unwrap() {
        0 => "available",
        1 => "requiresMacos26",
        2 => "unavailable",
        _ => "localeUnavailable",
    }
}

#[tauri::command]
pub fn sasayaki_load_transcript(app: AppHandle, id: String) -> Option<TranscriptProgress> {
    library::read_book_json(&app, &id, TRANSCRIPT_FILE)
}

#[tauri::command]
pub fn sasayaki_clear_transcript(app: AppHandle, id: String) -> Result<(), String> {
    let root = library::book_dir(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    library::delete(&root.join(TRANSCRIPT_FILE))
}

#[tauri::command]
pub fn sasayaki_pause_transcription() {
    if let Some(transcription) = RUNNING.lock().unwrap().as_ref() {
        unsafe { hoshi_transcribe_cancel(transcription.0) }
    }
}

#[tauri::command]
pub async fn sasayaki_transcribe(
    app: AppHandle,
    id: String,
    path: String,
    on_progress: Channel<Progress>,
) -> Result<(), String> {
    let root = library::book_dir(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let audio = CString::new(path.as_str()).unwrap();
    let duration = unsafe { hoshi_audio_duration(audio.as_ptr()) };
    if duration < 0.0 {
        return Err(format!("Could not read {path}"));
    }

    let mut playback = sasayaki::load_sasayaki_playback(&root).unwrap_or_default();
    playback.audio_path = Some(path);
    sasayaki::save_sasayaki_playback(&playback, &root).ok();

    let transcript_path = root.join(TRANSCRIPT_FILE);
    let mut transcript = library::read_json::<SasayakiTranscript>(&transcript_path)
        .filter(|saved| saved.duration == duration)
        .unwrap_or(SasayakiTranscript {
            through: 0.0,
            duration,
            tokens: Vec::new(),
        });

    let progress = |progress: Progress| on_progress.send(progress).ok();
    progress(Progress::Transcribing {
        through: transcript.through,
        duration,
        remaining: None,
    });
    if !transcript.is_complete() {
        let (sender, mut events) = mpsc::unbounded_channel();
        let context = Box::into_raw(Box::new(sender)).cast();
        let handle = unsafe {
            hoshi_transcribe(
                audio.as_ptr(),
                transcript.through,
                context,
                on_download,
                on_tokens,
                on_finish,
            )
        };
        *RUNNING.lock().unwrap() = Some(Transcription(handle));

        let start = transcript.through;
        let mut first_result = None;
        let mut last_persist = Instant::now();
        let mut error = None;
        while let Some(event) = events.recv().await {
            match event {
                Event::Download(fraction) => {
                    progress(Progress::Downloading { fraction });
                }
                Event::Tokens(tokens, through) => {
                    transcript.tokens.extend(tokens);
                    transcript.through = through;
                    let elapsed = first_result
                        .get_or_insert_with(Instant::now)
                        .elapsed()
                        .as_secs_f64();
                    let processed = through - start;
                    let remaining = (elapsed > 3.0 && processed > 30.0)
                        .then(|| (duration - through) * elapsed / processed);
                    progress(Progress::Transcribing {
                        through,
                        duration,
                        remaining,
                    });
                    if last_persist.elapsed() > Duration::from_secs(15) {
                        last_persist = Instant::now();
                        library::write_json(&transcript_path, &transcript).ok();
                    }
                }
                Event::Finish(result) => error = result,
            }
        }
        RUNNING.lock().unwrap().take();
        if let Some(error) = error {
            return Err(error);
        }
    }

    if transcript.tokens.is_empty() {
        return Ok(());
    }
    library::write_json(&transcript_path, &transcript).ok();

    progress(Progress::Aligning);
    let book = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        sasayaki::build_source(&book, &id).map(|source| align(&source, &transcript.tokens))
    })
    .await
    .unwrap()?;
    let folder = library::load_metadata_at(&root).unwrap().folder;
    crate::sync::storage::shared()
        .save_sasayaki_match(&result, &folder, &root)
        .map_err(|error| error.to_string())
}
