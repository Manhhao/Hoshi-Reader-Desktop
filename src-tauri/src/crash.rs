use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic;

use tauri::{AppHandle, Manager};

pub fn init(app: &AppHandle) {
    let dir = app.path().app_log_dir().unwrap();
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crash.log");
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let entry = format!(
            "{} {info}\n{}\n\n",
            chrono::Local::now(),
            Backtrace::force_capture()
        );
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(entry.as_bytes()));
        default_hook(info);
    }));
}
