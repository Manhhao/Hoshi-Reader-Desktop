use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use tauri::AppHandle;

use crate::anki;

const DEFAULT_SOURCES: [&str; 10] = [
    "nhk16",
    "daijisen",
    "shinmeikai8",
    "jpod",
    "jpod_alternate",
    "taas",
    "ozk5",
    "forvo",
    "forvo_ext",
    "forvo_ext2",
];

const AUDIO_FILE_FILTER: &str = "(file LIKE '%.mp3' OR file LIKE '%.opus' OR file LIKE '%.ogg')";

const EMPTY_RESPONSE: &str = r#"{"type":"audioSourceList","audioSources":[]}"#;

fn db_path(app: &AppHandle) -> Option<PathBuf> {
    anki::load_config(app)
        .local_audio_path
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

fn open_db(app: &AppHandle) -> Option<Connection> {
    Connection::open_with_flags(db_path(app)?, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

fn katakana_to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| {
            let value = c as u32;
            if (0x30A1..=0x30F6).contains(&value) {
                char::from_u32(value - 0x60).unwrap()
            } else {
                c
            }
        })
        .collect()
}

fn query_param(uri: &str, key: &str) -> Option<String> {
    let query = uri.split_once('?').map(|(_, q)| q)?;
    let prefix = format!("{key}=");
    let value = query
        .split('&')
        .find_map(|p| p.strip_prefix(prefix.as_str()))?;
    Some(urlencoding::decode(value).ok()?.into_owned())
}

fn scheme_base() -> &'static str {
    if cfg!(windows) {
        "http://audio.localhost/"
    } else {
        "audio://localhost/"
    }
}

pub fn file_params(url: &str) -> Option<(String, String)> {
    if !url.starts_with("audio://localhost/") && !url.starts_with(scheme_base()) {
        return None;
    }
    Some((query_param(url, "source")?, query_param(url, "file")?))
}

pub fn source_list(app: &AppHandle, url: &str) -> Vec<u8> {
    lookup(app, url).unwrap_or_else(|| EMPTY_RESPONSE.as_bytes().to_vec())
}

fn lookup(app: &AppHandle, url: &str) -> Option<Vec<u8>> {
    let term = query_param(url, "term").unwrap_or_default();
    let reading = katakana_to_hiragana(&query_param(url, "reading").unwrap_or_default());

    let sort_order = format!(
        "CASE source {}ELSE 999 END",
        DEFAULT_SOURCES
            .iter()
            .enumerate()
            .map(|(i, _)| format!("WHEN ? THEN {i} "))
            .collect::<String>()
    );
    let sql;
    let mut params: Vec<&str>;
    if reading.is_empty() {
        sql = format!(
            "SELECT source, display, file, expression, reading, 0 AS rank FROM entries
            WHERE expression = ? AND {AUDIO_FILE_FILTER}
            ORDER BY {sort_order}, reading;"
        );
        params = vec![term.as_str()];
    } else {
        sql = format!(
            "SELECT source, display, file, expression, reading, CASE
                WHEN expression = ? AND (reading IS NULL OR reading = ?) THEN 0
                WHEN reading = ? THEN 1
                ELSE 2
            END AS rank FROM entries
            WHERE (expression = ? OR reading = ?) AND {AUDIO_FILE_FILTER}
            ORDER BY rank, {sort_order}, reading;"
        );
        params = vec![
            term.as_str(),
            reading.as_str(),
            reading.as_str(),
            term.as_str(),
            reading.as_str(),
        ];
    }
    params.extend(DEFAULT_SOURCES);

    let conn = open_db(app)?;
    let mut stmt = conn.prepare(&sql).ok()?;
    let mut rows = stmt.query(rusqlite::params_from_iter(params.iter())).ok()?;
    let mut sources = Vec::new();
    while let Some(row) = rows.next().ok()? {
        let source: String = row.get(0).ok()?;
        let display = row.get::<_, Option<String>>(1).ok()?.unwrap_or_default();
        let file: String = row.get(2).ok()?;
        let expression: String = row.get(3).ok()?;
        let row_reading = row.get::<_, Option<String>>(4).ok()?.unwrap_or_default();
        let rank: i32 = row.get(5).ok()?;

        let name = match source.as_str() {
            "nhk16" => format!("NHK16 {display}"),
            "daijisen" => format!("Daijisen {display}"),
            "shinmeikai8" => format!("SMK8 {display}"),
            "jpod" => "JPod101".to_string(),
            "jpod_alternate" => "JPod101 Alt".to_string(),
            "taas" => "TAAS".to_string(),
            "ozk5" => format!("OZK5 {display}"),
            "forvo" => format!("Forvo ({display})"),
            "forvo_ext" => "Forvo Ext".to_string(),
            "forvo_ext2" => "Forvo Ext2".to_string(),
            _ => format!("{source} {display}"),
        };
        let matched = match rank {
            1 => format!(" ({expression})"),
            2 => format!(" ({row_reading})"),
            _ => String::new(),
        };
        let audio_url = format!(
            "{}?source={}&file={}",
            scheme_base(),
            urlencoding::encode(&source),
            urlencoding::encode(&file)
        );
        sources.push(json!({ "name": format!("{}{matched}", name.trim()), "url": audio_url }));
    }
    let response = json!({
        "type": "audioSourceList",
        "audioSources": sources
    });
    Some(serde_json::to_vec(&response).unwrap())
}

pub fn audio_format(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(b"OggS") {
        if bytes.windows(8).take(64).any(|w| w == b"OpusHead") {
            return ("opus", "audio/ogg");
        }
        return ("ogg", "audio/ogg");
    }
    ("mp3", "audio/mpeg")
}

pub fn audio_bytes(app: &AppHandle, source: &str, file: &str) -> Option<Vec<u8>> {
    let conn = open_db(app)?;
    conn.query_row(
        "SELECT data FROM android WHERE source = ? AND file = ?;",
        rusqlite::params![source, file],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .ok()
}
