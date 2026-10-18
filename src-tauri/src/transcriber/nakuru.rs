use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::{Duration, SystemTime};

use libloading::os::windows::Library;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

use super::{Event, Events};
use crate::library;
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

const RELEASE: &str = "https://huggingface.co/Oxydlate/Nakuru/resolve/233f7b962e511d7886868a22c263b47a8c2e66e7/nakuru";

struct Asset {
    name: &'static str,
    path: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const ASSETS: [Asset; 2] = [
    Asset {
        name: "nakuru.pack",
        path: "model/v1/nakuru.pack",
        bytes: 79608510,
        sha256: "85d870ed7bd6162284b0b0b7daf8bde2b101c19308a04af413e7470cf4490c29",
    },
    Asset {
        name: "nakuru.dll",
        path: "runtime/windows-x64/1/nakuru.dll",
        bytes: 335872,
        sha256: "8a4fe121a25f30780d452cf1a4c02b49d004c498bbd4b16a1845121984582a11",
    },
];

#[derive(Serialize, Deserialize, PartialEq)]
struct Installed {
    sha256: String,
    bytes: u64,
    modified: SystemTime,
}

impl Installed {
    fn stamp(path: &Path, sha256: &str) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Installed {
            sha256: sha256.into(),
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

fn hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hex(hasher))
}

fn supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    return is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma");
    #[cfg(not(target_arch = "x86_64"))]
    false
}

pub async fn status(_app: AppHandle) -> &'static str {
    if supported() {
        "available"
    } else {
        "unsupportedCpu"
    }
}

async fn install(
    app: &AppHandle,
    events: &Events,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("Nakuru");
    let marker = root.join("installed.json");
    let mut installed: HashMap<String, Installed> = library::read_json(&marker).unwrap_or_default();
    let mut verified = false;
    let pending: Vec<_> = ASSETS
        .iter()
        .filter(|asset| {
            let path = root.join(asset.name);
            let Some(current) = Installed::stamp(&path, asset.sha256) else {
                return true;
            };
            if current.bytes != asset.bytes {
                return true;
            }
            if installed
                .get(asset.name)
                .is_some_and(|entry| *entry == current)
            {
                return false;
            }
            if digest(&path).as_deref() != Some(asset.sha256) {
                return true;
            }
            installed.insert(asset.name.into(), current);
            verified = true;
            false
        })
        .collect();
    if verified {
        library::write_json(&marker, &installed)?;
    }
    if pending.is_empty() {
        return Ok(root);
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let total: u64 = pending.iter().map(|asset| asset.bytes).sum();
    let mut received = 0;
    let mut percent = None;
    let client = reqwest::Client::new();
    for asset in pending {
        let part = root.join(format!("{}.part", asset.name));
        let mut response = client
            .get(format!("{RELEASE}/{}", asset.path))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| format!("Could not download {}: {error}", asset.name))?;
        let mut file = File::create(&part).map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    drop(file);
                    fs::remove_file(&part).ok();
                    return Ok(root);
                }
                chunk = response.chunk() => chunk
                    .map_err(|error| format!("Could not download {}: {error}", asset.name))?,
            };
            let Some(chunk) = chunk else { break };
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|error| error.to_string())?;
            received += chunk.len() as u64;
            let step = received * 100 / total;
            if percent != Some(step) {
                percent = Some(step);
                events
                    .send(Event::Download(received as f64 / total as f64))
                    .ok();
            }
        }
        drop(file);
        if hex(hasher) != asset.sha256 {
            fs::remove_file(&part).ok();
            return Err(format!("Downloaded {} failed verification", asset.name));
        }
        fs::rename(&part, root.join(asset.name)).map_err(|error| error.to_string())?;
        let stamp = Installed::stamp(&root.join(asset.name), asset.sha256)
            .ok_or_else(|| format!("Could not read {}", asset.name))?;
        installed.insert(asset.name.into(), stamp);
        library::write_json(&marker, &installed)?;
    }
    Ok(root)
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
    performance: i32,
    events: Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match install(&app, &events, &cancel).await {
            Ok(_) if cancel.is_cancelled() => Ok(()),
            Ok(root) => {
                let events = events.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    with_media_foundation(|| {
                        Ok(run(&root, audio, from, performance, &events, cancel))
                    })
                    .unwrap_or_else(|error| Err(error.to_string()))
                })
                .await
                .unwrap_or_else(|error| Err(error.to_string()))
            }
            Err(error) => Err(error),
        };
        events.send(Event::Finish(result.err())).ok();
    });
    Ok(())
}

struct Buffer {
    blocks: Receiver<Result<Vec<f32>, String>>,
    samples: Vec<f32>,
    start: usize,
}

struct Recognition {
    audio: Mutex<Buffer>,
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
    let index = job.offset + offset;
    let mut audio = job.audio.lock().unwrap();
    let skip = index.saturating_sub(audio.start).min(audio.samples.len());
    audio.samples.drain(..skip);
    audio.start += skip;
    while audio.start + audio.samples.len() < index + count {
        match audio.blocks.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(block)) => audio.samples.extend(block),
            Ok(Err(error)) => {
                *job.error.lock().unwrap() = Some(error);
                return 0;
            }
            Err(RecvTimeoutError::Timeout) if job.cancel.is_cancelled() => return 0,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for (position, value) in (index..).zip(samples.iter_mut()) {
        *value = audio
            .samples
            .get(position - audio.start)
            .copied()
            .unwrap_or(0.0);
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
    performance: i32,
    events: &Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let library_path = root.join("nakuru.dll");
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
    let recognizer = unsafe { create(pack.as_ptr(), performance) };
    if recognizer.is_null() {
        return Err(failure());
    }
    let count = (audio.duration * MonoReader::RATE as f64).round() as usize;
    let offset = ((from * MonoReader::RATE as f64).round() as usize).min(count);
    let from = offset as f64 / MonoReader::RATE as f64;
    let (sender, blocks) = sync_channel(120);
    let (path, mime) = (audio.path.clone(), audio.mime);
    std::thread::spawn(move || {
        let result = with_media_foundation(|| {
            let mut reader = MonoReader::open(&path, mime, from)?;
            for position in (offset..count).step_by(MonoReader::RATE) {
                let mut block = vec![0.0; MonoReader::RATE.min(count - position)];
                reader.read(position, &mut block)?;
                if sender.send(Ok(block)).is_err() {
                    break;
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            sender
                .send(Err(format!("The audio file could not be read: {error}")))
                .ok();
        }
    });
    let job = Recognition {
        audio: Mutex::new(Buffer {
            blocks,
            samples: Vec::new(),
            start: offset,
        }),
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
