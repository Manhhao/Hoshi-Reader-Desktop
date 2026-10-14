use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use libloading::os::windows::Library;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

use super::{Event, Events};
use crate::sasayaki::SasayakiToken;

const SAMPLE_RATE: usize = 16000;
const NAKURU_BALANCED: c_int = 1;
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
    (root.join("runtime/nakuru.dll").is_file() && root.join("bundle/manifest.json").is_file())
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
    file: File,
    count: usize,
    pub duration: f64,
}

pub fn prepare(path: &str, cancel: &CancellationToken) -> Result<Audio, String> {
    let mut file = tempfile::tempfile().map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(&mut file);
    let mut count = 0;
    let mut failure = None;
    let path = Path::new(path);
    crate::pcm::decode_mono(path, crate::sasayaki::audio_mime(path), &mut |samples| {
        if cancel.is_cancelled() {
            return false;
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(samples.as_ptr().cast(), samples.len() * 2) };
        if let Err(error) = writer.write_all(bytes) {
            failure = Some(error.to_string());
            return false;
        }
        count += samples.len();
        true
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    writer.flush().map_err(|error| error.to_string())?;
    drop(writer);
    Ok(Audio {
        file,
        count,
        duration: count as f64 / SAMPLE_RATE as f64,
    })
}

pub fn start(
    app: &AppHandle,
    audio: Audio,
    from: f64,
    events: Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let root = locate(app).ok_or("Nakuru is not installed")?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = run(&root, audio, from, &events, cancel);
        events.send(Event::Finish(result.err())).ok();
    });
    Ok(())
}

struct Recognition {
    audio: Mutex<File>,
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
    let mut bytes = vec![0; count * 2];
    let mut file = job.audio.lock().unwrap();
    let result = file
        .seek(SeekFrom::Start((job.offset + offset) as u64 * 2))
        .and_then(|_| file.read_exact(&mut bytes));
    if let Err(error) = result {
        *job.error.lock().unwrap() = Some(error.to_string());
        return 0;
    }
    let samples = unsafe { std::slice::from_raw_parts_mut(pcm16, count) };
    for (sample, pair) in samples.iter_mut().zip(bytes.chunks_exact(2)) {
        *sample = i16::from_le_bytes([pair[0], pair[1]]) as f32;
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
    events: &Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let runtime_path = root.join("runtime/onnxruntime.dll");
    let library_path = root.join("runtime/nakuru.dll");
    let _runtime = unsafe {
        Library::load_with_flags(
            &runtime_path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    }
    .map_err(|error| format!("Could not load {}: {error}", runtime_path.display()))?;
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
    let bundle = CString::new(root.join("bundle").to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    let recognizer = unsafe { create(bundle.as_ptr(), NAKURU_BALANCED) };
    if recognizer.is_null() {
        return Err(failure());
    }
    let offset = ((from * SAMPLE_RATE as f64).round() as usize).min(audio.count);
    let job = Recognition {
        audio: Mutex::new(audio.file),
        error: Mutex::new(None),
        offset,
        from: offset as f64 / SAMPLE_RATE as f64,
        events: events.clone(),
        cancel,
    };
    let result = unsafe {
        transcribe(
            recognizer,
            read,
            audio.count - offset,
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
