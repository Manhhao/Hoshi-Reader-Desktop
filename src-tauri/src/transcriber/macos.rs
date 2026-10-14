use std::ffi::{CStr, CString, c_char, c_void};

use tauri::AppHandle;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::{Event, Events};

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
}

pub struct Audio {
    path: CString,
    pub duration: f64,
}

pub fn prepare(path: &str, _cancel: &CancellationToken) -> Result<Audio, String> {
    let path = CString::new(path).map_err(|error| error.to_string())?;
    let duration = unsafe { hoshi_audio_duration(path.as_ptr()) };
    if duration < 0.0 {
        return Err("Could not read audio".into());
    }
    Ok(Audio { path, duration })
}

struct Transcription(*mut c_void);

unsafe impl Send for Transcription {}

impl Drop for Transcription {
    fn drop(&mut self) {
        unsafe { hoshi_transcribe_release(self.0) }
    }
}

pub fn start(
    _app: &AppHandle,
    audio: Audio,
    from: f64,
    _speed: i32,
    events: Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    let transcription = Transcription(unsafe {
        hoshi_transcribe(
            audio.path.as_ptr(),
            from,
            Box::into_raw(Box::new(events)).cast(),
            on_download,
            on_tokens,
            on_finish,
        )
    });
    tauri::async_runtime::spawn(async move {
        cancel.cancelled().await;
        unsafe { hoshi_transcribe_cancel(transcription.0) };
        drop(transcription);
    });
    Ok(())
}

pub async fn status(_app: AppHandle) -> &'static str {
    let (sender, receiver) = oneshot::channel();
    unsafe { hoshi_transcriber_status(Box::into_raw(Box::new(sender)).cast(), on_status) };
    match receiver.await.unwrap() {
        0 => "available",
        1 => "requiresMacos26",
        2 => "unavailable",
        _ => "localeUnavailable",
    }
}

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
