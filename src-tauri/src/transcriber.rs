use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::ipc::Channel;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::sasayaki::{self, SasayakiToken};
use crate::{aligner, library};

#[cfg(target_os = "macos")]
#[path = "transcriber/macos.rs"]
mod backend;
#[cfg(windows)]
#[path = "transcriber/nakuru.rs"]
mod backend;

const TRANSCRIPT_FILE: &str = "sasayaki_transcript.json";

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

static RUNNING: Mutex<Option<CancellationToken>> = Mutex::new(None);

struct Job;

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(cancel) = RUNNING.lock().unwrap().take() {
            cancel.cancel();
        }
    }
}

#[tauri::command]
pub async fn sasayaki_transcriber_status(app: AppHandle) -> &'static str {
    backend::status(app).await
}

#[tauri::command(async)]
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
    if let Some(cancel) = RUNNING.lock().unwrap().as_ref() {
        cancel.cancel();
    }
}

#[tauri::command]
pub async fn sasayaki_transcribe(
    app: AppHandle,
    id: String,
    path: String,
    performance: i32,
    on_progress: Channel<Progress>,
) -> Result<(), String> {
    let cancel = CancellationToken::new();
    {
        let mut running = RUNNING.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        *running = Some(cancel.clone());
    }
    let _job = Job;
    let root = library::book_dir(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let progress = |progress: Progress| on_progress.send(progress).ok();
    let audio = backend::prepare(&path)?;
    let duration = audio.duration;

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

    progress(Progress::Transcribing {
        through: transcript.through,
        duration,
        remaining: None,
    });
    if !transcript.is_complete() {
        let (sender, mut events) = mpsc::unbounded_channel();
        backend::start(&app, audio, transcript.through, performance, sender, cancel)?;

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
        sasayaki::build_source(&book, &id).map(|source| aligner::align(&source, &transcript.tokens))
    })
    .await
    .unwrap()?;
    let folder = library::load_metadata_at(&root).unwrap().folder;
    crate::sync::storage::shared()
        .save_sasayaki_match(&result, &folder, &root)
        .map_err(|error| error.to_string())
}
