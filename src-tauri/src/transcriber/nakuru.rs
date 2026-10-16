use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use libloading::os::windows::Library;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

use super::{Event, Events};
use crate::pcm::{MonoReader, with_media_foundation};
use crate::sasayaki::SasayakiToken;

const NAKURU_FAILED: c_int = -1;
const NAKURU_FINISHED: c_int = 1;

#[repr(C)]
struct NakuruToken {
    text: *const c_char,
    start: f64,
    end: f64,
}

#[repr(C)]
struct NakuruSegment {
    text: *const c_char,
    start: f64,
    end: f64,
    tokens: *const NakuruToken,
    token_count: usize,
}

type ReadFn = unsafe extern "C" fn(*mut c_void, usize, usize, *mut f32) -> c_int;
type EmitFn = unsafe extern "C" fn(*mut c_void, *const NakuruSegment) -> c_int;
type Create = unsafe extern "C" fn(*const c_char, c_int) -> *mut c_void;
type Run = unsafe extern "C" fn(*mut c_void, ReadFn, usize, EmitFn, *mut c_void) -> c_int;
type Error = unsafe extern "C" fn() -> *const c_char;
type Destroy = unsafe extern "C" fn(*mut c_void);

fn locate(app: &AppHandle) -> Option<PathBuf> {
    let root = match std::env::var_os("HOSHI_NAKURU_DIR") {
        Some(root) => PathBuf::from(root),
        None => app.path().app_data_dir().ok()?.join("Nakuru"),
    };
    (root.join("runtime/nakuru.dll").is_file() && root.join("nakuru.pack").is_file())
        .then_some(root)
}

pub async fn status(app: AppHandle) -> &'static str {
    if locate(&app).is_some() {
        "available"
    } else {
        "nakuruUnavailable"
    }
}

pub struct Audio {
    path: PathBuf,
    mime: &'static str,
    pub duration: f64,
}

pub fn prepare(path: &str) -> Result<Audio, String> {
    let path = PathBuf::from(path);
    let mime = crate::sasayaki::audio_mime(&path);
    let duration = with_media_foundation(|| MonoReader::duration(&path, mime))
        .map_err(|error| error.to_string())?;
    Ok(Audio {
        path,
        mime,
        duration,
    })
}

pub fn start(
    app: &AppHandle,
    audio: Audio,
    from: f64,
    speed: i32,
    events: Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let root = locate(app).ok_or("Nakuru is not installed")?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = with_media_foundation(|| Ok(run(&root, audio, from, speed, &events, cancel)))
            .unwrap_or_else(|error| Err(error.to_string()));
        events.send(Event::Finish(result.err())).ok();
    });
    Ok(())
}

struct Recognition {
    audio: Mutex<MonoReader>,
    error: Mutex<Option<String>>,
    offset: usize,
    from: f64,
    events: Events,
    cancel: CancellationToken,
}

unsafe extern "C" fn read(
    context: *mut c_void,
    offset: usize,
    count: usize,
    pcm16: *mut f32,
) -> c_int {
    let job = unsafe { &*context.cast::<Recognition>() };
    if job.cancel.is_cancelled() {
        return 0;
    }
    let samples = unsafe { std::slice::from_raw_parts_mut(pcm16, count) };
    if let Err(error) = job.audio.lock().unwrap().read(job.offset + offset, samples) {
        *job.error.lock().unwrap() = Some(error.to_string());
        return 0;
    }
    1
}

unsafe extern "C" fn emit(context: *mut c_void, segment: *const NakuruSegment) -> c_int {
    let job = unsafe { &*context.cast::<Recognition>() };
    if job.cancel.is_cancelled() {
        return 0;
    }
    let segment = unsafe { &*segment };
    let mut tokens = Vec::with_capacity(segment.token_count);
    if segment.token_count > 0 {
        for token in unsafe { std::slice::from_raw_parts(segment.tokens, segment.token_count) } {
            tokens.push(SasayakiToken {
                text: unsafe { CStr::from_ptr(token.text) }
                    .to_string_lossy()
                    .into_owned(),
                start: job.from + token.start,
                end: job.from + token.end,
            });
        }
    }
    i32::from(
        job.events
            .send(Event::Tokens(tokens, job.from + segment.end))
            .is_ok(),
    )
}

fn run(
    root: &Path,
    audio: Audio,
    from: f64,
    speed: i32,
    events: &Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let library_path = root.join("runtime/nakuru.dll");
    let library = unsafe {
        Library::load_with_flags(
            &library_path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    }
    .map_err(|error| format!("Could not load {}: {error}", library_path.display()))?;
    let (create, transcribe, error, destroy) = unsafe {
        (
            library.get::<Create>(b"nakuru_create"),
            library.get::<Run>(b"nakuru_transcribe"),
            library.get::<Error>(b"nakuru_error"),
            library.get::<Destroy>(b"nakuru_destroy"),
        )
    };
    let create = create.map_err(|error| error.to_string())?;
    let transcribe = transcribe.map_err(|error| error.to_string())?;
    let error = error.map_err(|error| error.to_string())?;
    let destroy = destroy.map_err(|error| error.to_string())?;
    let failure = || {
        unsafe { CStr::from_ptr(error()) }
            .to_string_lossy()
            .into_owned()
    };
    let pack = CString::new(root.join("nakuru.pack").to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    let recognizer = unsafe { create(pack.as_ptr(), speed) };
    if recognizer.is_null() {
        return Err(failure());
    }
    let count = (audio.duration * MonoReader::RATE as f64).round() as usize;
    let offset = ((from * MonoReader::RATE as f64).round() as usize).min(count);
    let from = offset as f64 / MonoReader::RATE as f64;
    let reader =
        MonoReader::open(&audio.path, audio.mime, from).map_err(|error| error.to_string())?;
    let job = Recognition {
        audio: Mutex::new(reader),
        error: Mutex::new(None),
        offset,
        from,
        events: events.clone(),
        cancel,
    };
    let result = unsafe {
        transcribe(
            recognizer,
            read,
            count - offset,
            emit,
            (&raw const job).cast_mut().cast(),
        )
    };
    let message = (result == NAKURU_FAILED).then(failure);
    unsafe { destroy(recognizer) };
    if let Some(error) = job.error.into_inner().unwrap().or(message) {
        return Err(error);
    }
    if result == NAKURU_FINISHED {
        events.send(Event::Tokens(Vec::new(), audio.duration)).ok();
    } else if !job.cancel.is_cancelled() {
        return Err("Nakuru stopped before transcription finished".into());
    }
    Ok(())
}
