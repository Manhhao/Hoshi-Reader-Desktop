use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_tungstenite::accept_hdr_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_util::sync::CancellationToken;

use crate::dict::{self, FrequencySortOrder};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HTTP_HEAD_BYTES: usize = 16 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn start(app: AppHandle, port: u16) -> Result<CancellationToken, String> {
    if port == 0 {
        return Err("Choose a sharing port between 1 and 65535".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| format!("Could not start dictionary sharing: {error}"))?;
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    tauri::async_runtime::spawn(async move {
        let slots = Arc::new(Semaphore::new(16));
        loop {
            let incoming = tokio::select! {
                _ = token.cancelled() => break,
                incoming = listener.accept() => incoming,
            };
            let Ok((stream, _)) = incoming else {
                break;
            };
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let app = app.clone();
            let connection_token = token.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                tokio::select! {
                    _ = connection_token.cancelled() => {},
                    _ = connection(app, stream, port) => {},
                }
            });
        }
    });
    Ok(cancellation)
}

fn allowed_origin(origin: &str) -> bool {
    if origin == "hoshi://hoshidicts" {
        return true;
    }
    let extension = origin
        .strip_prefix("chrome-extension://")
        .or_else(|| origin.strip_prefix("moz-extension://"));
    extension.is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

async fn connection(app: AppHandle, mut stream: TcpStream, port: u16) -> Result<(), String> {
    let mut probe = [0_u8; MAX_HTTP_HEAD_BYTES];
    let line = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        loop {
            let length = stream
                .peek(&mut probe)
                .await
                .map_err(|error| error.to_string())?;
            if length == 0 {
                return Err("The sharing connection closed".to_string());
            }
            if let Some(end) = probe[..length].windows(2).position(|part| part == b"\r\n") {
                return Ok(String::from_utf8_lossy(&probe[..end]).into_owned());
            }
            if length == probe.len() {
                return Err("The sharing request headers are too large".into());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "The sharing handshake timed out".to_string())??;
    if line.split_whitespace().nth(1) == Some("/link") {
        websocket(app, stream, port).await
    } else {
        http(app, &mut stream, port).await
    }
}

async fn websocket(app: AppHandle, stream: TcpStream, port: u16) -> Result<(), String> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let mut socket = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        accept_hdr_async_with_config(
            stream,
            |request: &Request, response: Response| {
                let allowed = request.uri().path() == "/link"
                    && request.uri().query().is_none()
                    && request.headers().get_all("origin").iter().count() == 1
                    && request
                        .headers()
                        .get("origin")
                        .and_then(|origin| origin.to_str().ok())
                        .is_some_and(allowed_origin);
                if allowed {
                    Ok(response)
                } else {
                    Err(tokio_tungstenite::tungstenite::http::Response::builder()
                        .status(403)
                        .body(Some(
                            "Dictionary sharing accepts local dictionary apps only".into(),
                        ))
                        .unwrap())
                }
            },
            Some(config),
        ),
    )
    .await
    .map_err(|_| "The sharing handshake timed out".to_string())?
    .map_err(|error| error.to_string())?;
    let first = tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.next())
        .await
        .map_err(|_| "The sharing hello timed out".to_string())?
        .ok_or_else(|| "The sharing connection closed".to_string())?
        .map_err(|error| error.to_string())?;
    let Message::Text(first) = first else {
        return Err("A sharing hello is required".into());
    };
    let hello: Value = serde_json::from_str(first.as_str()).map_err(|error| error.to_string())?;
    if hello["kind"] != "hello" || hello["protocol"] != 1 {
        return Err("Unsupported dictionary sharing protocol".into());
    }
    let state_app = app.clone();
    let (state, generation, count) =
        tauri::async_runtime::spawn_blocking(move || dictionary_state(&state_app, port))
            .await
            .map_err(|error| error.to_string())??;
    let response = json!({
        "kind": "hello", "protocol": 1, "version": env!("CARGO_PKG_VERSION"),
        "name": "Hoshi Reader Desktop", "dictionaryCount": count, "capabilities": [],
        "snapshot": {"dictionaryState": state, "options": null,
            "customDictionarySource": null, "dictionaryUpdates": null, "lookupStats": null}
    });
    socket
        .send(Message::Text(response_text(&response)?.into()))
        .await
        .map_err(|error| error.to_string())?;
    let mut known_generation = generation;
    while let Some(frame) = socket.next().await {
        match frame.map_err(|error| error.to_string())? {
            Message::Text(text) => {
                let frame: Value =
                    serde_json::from_str(text.as_str()).map_err(|error| error.to_string())?;
                match frame["kind"].as_str() {
                    Some("pong") => continue,
                    Some("request") => {}
                    _ => return Err("Unsupported sharing frame".into()),
                }
                if !(frame["id"].is_string() || frame["id"].is_number())
                    || !frame["message"].is_object()
                {
                    return Err("Malformed sharing request".into());
                }
                let request_app = app.clone();
                let message = frame["message"].clone();
                let dispatched = tokio::time::timeout(
                    Duration::from_secs(60),
                    tauri::async_runtime::spawn_blocking(move || {
                        dispatch(&request_app, &message, port)
                    }),
                )
                .await
                .map_err(|_| "Dictionary lookup timed out".to_string())?
                .map_err(|error| error.to_string())?;
                let (response, state, generation) = dispatched;
                if state.is_object() && generation != known_generation {
                    let changes = json!({"kind": "storage", "changes": {"dictionaryState": state}});
                    socket
                        .send(Message::Text(response_text(&changes)?.into()))
                        .await
                        .map_err(|error| error.to_string())?;
                    known_generation = generation;
                }
                let reply = json!({"kind": "reply", "id": frame["id"], "response": response});
                let text = response_text(&reply)?;
                socket
                    .send(Message::Text(text.into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .map_err(|error| error.to_string())?,
            Message::Close(_) => break,
            Message::Pong(_) => {}
            _ => return Err("Dictionary sharing requires text frames".into()),
        }
    }
    Ok(())
}

fn response_text(value: &Value) -> Result<String, String> {
    let text = value.to_string();
    if text.len() > MAX_RESPONSE_BYTES {
        return Err("The dictionary response exceeds the sharing limit".into());
    }
    Ok(text)
}

fn dictionary_state(app: &AppHandle, port: u16) -> Result<(Value, u64, usize), String> {
    let dictionaries = dict::shared_dictionaries(app)?;
    let count = dictionaries
        .iter()
        .filter(|dictionary| dictionary.enabled)
        .count();
    let mut metadata = serde_json::to_value(dictionaries).map_err(|error| error.to_string())?;
    let encoded = metadata.to_string();
    let mut hasher = DefaultHasher::new();
    encoded.hash(&mut hasher);
    let generation = hasher.finish() & 0x1f_ffff_ffff_ffff;
    for dictionary in metadata.as_array_mut().into_iter().flatten() {
        let id = dictionary["id"].as_str().unwrap_or_default().to_owned();
        let download = format!(
            "http://127.0.0.1:{port}/dictionaries/{}",
            urlencoding::encode(&id)
        );
        dictionary["downloadUrl"] = json!(download);
    }
    Ok((
        json!({"schemaVersion": 1, "revision": generation, "dictionaries": metadata, "groups": []}),
        generation,
        count,
    ))
}

fn dispatch(app: &AppHandle, message: &Value, port: u16) -> (Value, Value, u64) {
    let kind = message["type"].as_str().unwrap_or("hd_unknown");
    let state = dictionary_state(app, port);
    let (state, generation, count) = match state {
        Ok(state) => state,
        Err(error) => {
            return (
                json!({"type": format!("{kind}_result"), "requestId": message["requestId"],
            "ok": false, "error": error, "generation": 0}),
                Value::Null,
                0,
            );
        }
    };
    let mut response = json!({"type": format!("{kind}_result"), "requestId": message["requestId"],
        "ok": true, "error": null, "generation": generation});
    let payload = read_request(app, message, &state, generation, count);
    match payload {
        Ok(payload) => response
            .as_object_mut()
            .unwrap()
            .extend(payload.as_object().unwrap().clone()),
        Err(error) => {
            response["ok"] = json!(false);
            response["error"] = json!(error);
        }
    }
    (response, state, generation)
}

fn bounded_text(message: &Value, field: &str, limit: usize) -> Result<String, String> {
    let text = message[field].as_str().unwrap_or_default();
    if text.len() > limit || text.contains('\0') {
        return Err(format!(
            "Invalid {field}: dictionary sharing allows at most {limit} bytes"
        ));
    }
    Ok(text.to_owned())
}

fn lookup_response(app: &AppHandle, message: &Value) -> Result<Value, String> {
    let text = bounded_text(message, "text", 4096)?;
    let max_results = message["maxResults"].as_i64().unwrap_or(32).clamp(1, 256) as i32;
    let scan_length = message["scanLength"].as_u64().unwrap_or(16).clamp(1, 64) as usize;
    let options = &message["options"];
    let order = match options["frequencyOrder"].as_str() {
        Some("ascending") => FrequencySortOrder::Ascending,
        Some("descending") => FrequencySortOrder::Descending,
        Some("disabled") => FrequencySortOrder::Disabled,
        _ => FrequencySortOrder::Auto,
    };
    let dictionary = bounded_text(options, "frequencyDictionary", 4096)?;
    serde_json::to_value(dict::lookup(
        app.clone(),
        app.state(),
        text,
        max_results,
        scan_length,
        order,
        dictionary,
    ))
    .map_err(|error| error.to_string())
}

fn read_request(
    app: &AppHandle,
    message: &Value,
    state: &Value,
    generation: u64,
    count: usize,
) -> Result<Value, String> {
    match (message["target"].as_str(), message["type"].as_str()) {
        (Some("hoshidicts-worker"), Some("hd_state_read")) => {
            Ok(json!({"state": state, "legacyDictionaries": null}))
        }
        (Some("hoshidicts-offscreen"), Some("hd_status")) => {
            Ok(json!({"ready": true, "loading": false,
            "dictionaryCount": count, "failedDictionaries": [], "storageBackend": "hoshi-reader"}))
        }
        (Some("hoshidicts-offscreen"), Some("hd_lookup")) => {
            let lookup = lookup_response(app, message)?;
            let entries = lookup["entries"].as_array().cloned().unwrap_or_default();
            let results: Vec<Value> = entries.into_iter().map(native_result).collect();
            Ok(json!({"results": results, "dictionaryCount": count}))
        }
        (Some("hoshidicts-offscreen"), Some("hd_styles")) => {
            let lookup = lookup_response(app, &json!({"text": "", "maxResults": 1}))?;
            let styles: Vec<Value> = lookup["styles"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(dictionary, styles)| json!({"dictionary": dictionary, "styles": styles}))
                .collect();
            Ok(json!({"styles": styles}))
        }
        (Some("hoshidicts-offscreen"), Some("hd_kanji")) => {
            let character = bounded_text(message, "character", 4096)?;
            let kanji =
                serde_json::to_value(dict::lookup_kanji(app.clone(), app.state(), character))
                    .map_err(|error| error.to_string())?;
            if kanji.is_null() {
                return Ok(json!({"kanji": null}));
            }
            let entries: Vec<Value> = kanji["entries"].as_array().into_iter().flatten().map(|entry|
                json!({"dictionary": entry["dictName"], "onyomi": entry["onyomi"], "kunyomi": entry["kunyomi"],
                    "tags": entry["tags"].as_str().unwrap_or_default(), "definitions": entry["meanings"],
                    "stats": entry["stats"].as_array().cloned().unwrap_or_default()})).collect();
            Ok(json!({"kanji": {"character": kanji["character"], "entries": entries}}))
        }
        (Some("hoshidicts-offscreen"), Some("hd_media")) => {
            if message["generation"].as_u64() != Some(generation) {
                return Err("Media generation no longer matches the shared dictionaries".into());
            }
            let dictionary = bounded_text(message, "dictionary", 4096)?;
            let path = bounded_text(message, "path", 4096)?;
            let bytes = dict::media_file(app, &dictionary, &path);
            if bytes.is_empty() {
                return Ok(json!({"dataUrl": null}));
            }
            if bytes.len() > (MAX_RESPONSE_BYTES - 4096) * 3 / 4 {
                return Err("The dictionary media exceeds the sharing limit".into());
            }
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(json!({"dataUrl": format!("data:{};base64,{encoded}", media_mime(&path))}))
        }
        _ => Err(
            "Hoshi Reader dictionary sharing is read-only; this request is not supported".into(),
        ),
    }
}

fn native_result(entry: Value) -> Value {
    let trace: Vec<Value> = entry["deinflectionTrace"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .cloned()
        .collect();
    let glossaries: Vec<Value> = entry["glossaries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|glossary| {
            json!({"dictionary": glossary["dictionary"], "glossary": glossary["content"],
            "definitionTags": glossary["definitionTags"], "termTags": glossary["termTags"]})
        })
        .collect();
    let pitches: Vec<Value> = entry["pitches"].as_array().into_iter().flatten().map(|group| {
        let pitches: Vec<Value> = group["pitches"].as_array().into_iter().flatten().map(|pitch| {
            let (position, pattern) = if let Some(pattern) = pitch["position"].as_str() {
                (json!(0), json!(pattern))
            } else {
                (pitch["position"].clone(), json!(""))
            };
            json!({"position": position, "pattern": pattern, "nasal": pitch["nasal"], "devoice": pitch["devoice"]})
        }).collect();
        json!({"dictionary": group["dictionary"], "pitches": pitches, "transcriptions": group["transcriptions"]})
    }).collect();
    let rules = entry["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    json!({"matched": entry["matched"], "deinflected": entry["deinflected"].as_str().unwrap_or_else(|| entry["expression"].as_str().unwrap_or_default()), "trace": trace,
        "preprocessorSteps": entry["preprocessorSteps"].as_i64().unwrap_or(0), "term": {"expression": entry["expression"], "reading": entry["reading"],
            "rules": rules, "score": entry["score"].as_i64().unwrap_or(0), "glossaries": glossaries, "frequencies": entry["frequencies"], "pitches": pitches}})
}

fn media_mime(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

struct TemporaryExport(PathBuf);

impl Drop for TemporaryExport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn http(app: AppHandle, stream: &mut TcpStream, port: u16) -> Result<(), String> {
    let mut head = Vec::new();
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut buffer = [0_u8; 1024];
        loop {
            let length = stream
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if length == 0 || head.len() + length > MAX_HTTP_HEAD_BYTES {
                return Err("Invalid sharing HTTP headers".to_string());
            }
            head.extend_from_slice(&buffer[..length]);
            if head.windows(4).any(|part| part == b"\r\n\r\n") {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| "The dictionary HTTP request timed out".to_string())??;
    let head = std::str::from_utf8(&head).map_err(|error| error.to_string())?;
    let mut lines = head.split("\r\n");
    let mut request = lines.next().unwrap_or_default().split_whitespace();
    let method = request.next().unwrap_or_default();
    let path = request.next().unwrap_or_default();
    let origins: Vec<&str> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("origin"))
        .map(|(_, origin)| origin.trim())
        .collect();
    let origin = match origins.as_slice() {
        [] => "",
        [origin] if allowed_origin(origin) => origin,
        _ => {
            return http_response(
                stream,
                403,
                "",
                "text/plain",
                b"A local dictionary app origin is required",
            )
            .await;
        }
    };
    if method == "OPTIONS" {
        return http_response(stream, 204, origin, "text/plain", &[]).await;
    }
    if method != "GET" {
        return http_response(
            stream,
            405,
            origin,
            "text/plain",
            b"Dictionary sharing is read-only",
        )
        .await;
    }
    if path == "/dictionaries" {
        let state = tauri::async_runtime::spawn_blocking(move || dictionary_state(&app, port))
            .await
            .map_err(|error| error.to_string())??;
        let body = response_text(&json!({"dictionaries": state.0["dictionaries"]}))?;
        return http_response(stream, 200, origin, "application/json", body.as_bytes()).await;
    }
    let Some(id) = path.strip_prefix("/dictionaries/") else {
        return http_response(
            stream,
            404,
            origin,
            "text/plain",
            b"Dictionary sharing route not found",
        )
        .await;
    };
    let id = urlencoding::decode(id)
        .map_err(|error| error.to_string())?
        .into_owned();
    let archive = tauri::async_runtime::spawn_blocking(move || {
        dict::export_shared_dictionary(&app, &id).map(TemporaryExport)
    })
    .await
    .map_err(|error| error.to_string())?;
    let archive = match archive {
        Ok(archive) => archive,
        Err(error) => {
            return http_response(stream, 404, origin, "text/plain", error.as_bytes()).await;
        }
    };
    let mut file = tokio::fs::File::open(&archive.0)
        .await
        .map_err(|error| error.to_string())?;
    let size = file
        .metadata()
        .await
        .map_err(|error| error.to_string())?
        .len();
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Disposition: attachment; filename=\"hoshi-dictionary.zip\"\r\nContent-Length: {size}\r\nAccess-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if length == 0 {
            break;
        }
        tokio::time::timeout(Duration::from_secs(60), stream.write_all(&buffer[..length]))
            .await
            .map_err(|_| "Dictionary download timed out".to_string())?
            .map_err(|error| error.to_string())?;
    }
    stream.shutdown().await.map_err(|error| error.to_string())
}

async fn http_response(
    stream: &mut TcpStream,
    status: u16,
    origin: &str,
    mime: &str,
    body: &[u8],
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let cors = if origin.is_empty() {
        String::new()
    } else {
        format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nVary: Origin\r\n"
        )
    };
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{cors}Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())?;
    stream.shutdown().await.map_err(|error| error.to_string())
}
