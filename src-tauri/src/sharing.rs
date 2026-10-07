use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use crate::dict::{self, FrequencySortOrder, LookupState};
use crate::library::{read_json, write_json};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SharingSettings {
    remote_address: String,
    remote_api_url: String,
    lookup_source: String,
    server_enabled: bool,
    server_port: u16,
}

impl Default for SharingSettings {
    fn default() -> Self {
        Self {
            remote_address: "ws://127.0.0.1:8771/link".into(),
            remote_api_url: "http://127.0.0.1:19633".into(),
            lookup_source: "local".into(),
            server_enabled: false,
            server_port: 8772,
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct RemoteSession {
    address: String,
    socket: Socket,
    hello: Value,
    next_id: u64,
}

#[derive(Default)]
struct ServerSession {
    port: u16,
    cancel: Option<CancellationToken>,
}

#[derive(Default)]
pub struct SharingState {
    settings: Mutex<SharingSettings>,
    media_generation: Mutex<Option<i64>>,
    remote: AsyncMutex<Option<RemoteSession>>,
    server: AsyncMutex<ServerSession>,
}

fn settings_path(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap().join("sharing.json")
}

fn validate_settings(settings: &SharingSettings) -> Result<(), String> {
    let address = Url::parse(&settings.remote_address).map_err(|error| error.to_string())?;
    if address.scheme() != "ws"
        || address.host_str().is_none()
        || address.path() != "/link"
        || !address.username().is_empty()
        || address.password().is_some()
        || address.query().is_some()
        || address.fragment().is_some()
    {
        return Err("Enter a ws://host:port/link sharing address.".into());
    }
    let api = Url::parse(&settings.remote_api_url).map_err(|error| error.to_string())?;
    if !matches!(api.scheme(), "http" | "https")
        || api.host_str().is_none()
        || !api.username().is_empty()
        || api.password().is_some()
        || api.query().is_some()
        || api.fragment().is_some()
    {
        return Err("Enter an http://host:port dictionary API address.".into());
    }
    if !matches!(settings.lookup_source.as_str(), "local" | "remote") {
        return Err("Choose local or remote word lookup.".into());
    }
    if settings.server_port == 0 {
        return Err("The sharing port must be between 1 and 65535.".into());
    }
    Ok(())
}

async fn configure(app: &AppHandle, settings: SharingSettings) -> Result<(), String> {
    validate_settings(&settings)?;
    let state = app.state::<SharingState>();
    let mut server = state.server.lock().await;
    let restart =
        settings.server_enabled && (server.cancel.is_none() || server.port != settings.server_port);
    let replacement = if restart {
        Some(crate::sharing_server::start(app.clone(), settings.server_port).await?)
    } else {
        None
    };
    if let Err(error) = write_json(&settings_path(app), &settings) {
        if let Some(cancel) = replacement {
            cancel.cancel();
        }
        return Err(error);
    }
    if restart || !settings.server_enabled {
        if let Some(cancel) = server.cancel.take() {
            cancel.cancel();
        }
        server.cancel = replacement;
        server.port = settings.server_port;
    }
    *state.settings.lock().unwrap() = settings;
    *state.media_generation.lock().unwrap() = None;
    *state.remote.lock().await = None;
    Ok(())
}

pub(crate) fn initialize(app: &AppHandle) {
    let settings = read_json::<SharingSettings>(&settings_path(app)).unwrap_or_default();
    *app.state::<SharingState>().settings.lock().unwrap() = settings.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = configure(&app, settings).await {
            app.emit("sharing-error", error).ok();
        }
    });
}

#[tauri::command]
pub fn sharing_get_settings(state: State<SharingState>) -> SharingSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub async fn sharing_configure(
    app: AppHandle,
    settings: SharingSettings,
) -> Result<SharingSettings, String> {
    configure(&app, settings.clone()).await?;
    Ok(settings)
}

async fn next_frame(socket: &mut Socket) -> Result<Value, String> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|error| error.to_string());
            }
            Some(Ok(Message::Ping(bytes))) => {
                socket
                    .send(Message::Pong(bytes))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err("The shared dictionary disconnected.".into());
            }
            Some(Err(error)) => return Err(error.to_string()),
            _ => {}
        }
    }
}

async fn connect_remote(address: &str) -> Result<RemoteSession, String> {
    let mut request = address
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request
        .headers_mut()
        .insert("Origin", "hoshi://hoshidicts".parse().unwrap());
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(32 * 1024 * 1024))
        .max_frame_size(Some(32 * 1024 * 1024));
    let (mut socket, _) =
        tokio_tungstenite::connect_async_with_config(request, Some(config), false)
            .await
            .map_err(|error| format!("Could not reach the shared dictionary: {error}"))?;
    socket
        .send(Message::Text(
            json!({
                "kind": "hello", "protocol": 1, "name": "Hoshi Reader Desktop",
                "version": env!("CARGO_PKG_VERSION"), "capabilities": []
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(|error| error.to_string())?;
    loop {
        let hello = next_frame(&mut socket).await?;
        match hello["kind"].as_str() {
            Some("hello") if hello["protocol"] == 1 && hello["snapshot"].is_object() => {
                return Ok(RemoteSession {
                    address: address.into(),
                    socket,
                    hello,
                    next_id: 0,
                });
            }
            Some("ping") => {
                socket
                    .send(Message::Text(json!({"kind":"pong"}).to_string().into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Some("bye") => {
                return Err(hello["reason"]
                    .as_str()
                    .unwrap_or("Sharing is unavailable.")
                    .into());
            }
            _ => return Err("The dictionary app uses an unsupported sharing protocol.".into()),
        }
    }
}

async fn remote_request(app: &AppHandle, message: Option<Value>) -> Result<Value, String> {
    let state = app.state::<SharingState>();
    let address = state.settings.lock().unwrap().remote_address.clone();
    let mut slot = state.remote.lock().await;
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        if slot
            .as_ref()
            .is_none_or(|session| session.address != address)
        {
            *slot = Some(connect_remote(&address).await?);
        }
        let session = slot.as_mut().unwrap();
        let Some(message) = message else {
            return Ok(session.hello.clone());
        };
        session.next_id += 1;
        let id = session.next_id;
        session
            .socket
            .send(Message::Text(
                json!({"kind":"request","id":id,"message":message})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| error.to_string())?;
        loop {
            let frame = next_frame(&mut session.socket).await?;
            match frame["kind"].as_str() {
                Some("reply") if frame["id"] == id => {
                    let response = frame["response"].clone();
                    if response["ok"] == false || response["error"].as_str().is_some() {
                        return Err(response["error"]
                            .as_str()
                            .unwrap_or("Remote lookup failed.")
                            .into());
                    }
                    return Ok(response);
                }
                Some("ping") => {
                    session
                        .socket
                        .send(Message::Text(json!({"kind":"pong"}).to_string().into()))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Some("bye") => {
                    return Err(frame["reason"]
                        .as_str()
                        .unwrap_or("Sharing stopped.")
                        .into());
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("The shared dictionary did not answer within 15 seconds.".into()));
    if result.is_err() {
        *slot = None;
    }
    result
}

#[tauri::command]
pub async fn sharing_remote_status(app: AppHandle) -> Result<Value, String> {
    let hello = remote_request(&app, None).await?;
    let status = remote_request(&app, Some(sharing_message("hd_status"))).await?;
    Ok(
        json!({"name":hello["name"],"version":hello["version"],"dictionaryCount":status["dictionaryCount"]}),
    )
}

fn api_url(app: &AppHandle, id: Option<&str>) -> Result<Url, String> {
    let value = app
        .state::<SharingState>()
        .settings
        .lock()
        .unwrap()
        .remote_api_url
        .clone();
    let mut url = Url::parse(&value).map_err(|error| error.to_string())?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "Invalid dictionary API address.")?;
    path.pop_if_empty().push("dictionaries");
    if let Some(id) = id {
        path.push(id);
    }
    drop(path);
    Ok(url)
}

async fn api_response(app: &AppHandle, id: Option<&str>) -> Result<reqwest::Response, String> {
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|error| error.to_string())?
        .get(api_url(app, id)?)
        .header("Origin", "hoshi://hoshidicts")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len() + chunk.len() > 8192 {
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let message = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| value["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| status.to_string());
    Err(format!("Dictionary download failed: {message}"))
}

#[tauri::command]
pub async fn sharing_remote_dictionaries(app: AppHandle) -> Result<Value, String> {
    let mut response = api_response(&app, None).await?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("The shared dictionary list is too large.".into());
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    let dictionaries = if value.is_array() {
        value
    } else {
        value["dictionaries"].clone()
    };
    if !dictionaries.is_array() {
        return Err("The dictionary app returned an invalid dictionary list.".into());
    }
    Ok(dictionaries)
}

struct TemporaryArchive(PathBuf);

impl Drop for TemporaryArchive {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

#[tauri::command]
pub async fn sharing_import_dictionary(app: AppHandle, id: String) -> Result<Value, String> {
    if id.is_empty() || id.len() > 1024 {
        return Err("Invalid dictionary id.".into());
    }
    let mut response = api_response(&app, Some(&id)).await?;
    let archive = TemporaryArchive(
        std::env::temp_dir().join(format!("hoshi-sharing-{}.zip", uuid::Uuid::new_v4())),
    );
    let mut output = tokio::fs::File::create(&archive.0)
        .await
        .map_err(|error| error.to_string())?;
    let mut size = 0_u64;
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        size += chunk.len() as u64;
        if size > 16 * 1024 * 1024 * 1024 {
            return Err("The shared dictionary exceeds 16 GiB.".into());
        }
        output
            .write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    output.flush().await.map_err(|error| error.to_string())?;
    drop(output);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<LookupState>();
        let is_backup = zip::ZipArchive::new(
            std::fs::File::open(&archive.0).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?
        .by_name("hachidori-backup.json")
        .is_ok();
        if is_backup {
            let title = dict::import_shared_dictionary(&app, &state, &archive.0)?;
            Ok(json!({"imported":[title],"failed":[]}))
        } else {
            serde_json::to_value(dict::import_dictionaries(
                app.clone(),
                state,
                vec![archive.0.to_string_lossy().into_owned()],
            ))
            .map_err(|error| error.to_string())
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

fn sharing_message(kind: &str) -> Value {
    json!({"target":"hoshidicts-offscreen","type":kind})
}

#[tauri::command]
pub async fn sharing_lookup(
    app: AppHandle,
    text: String,
    max_results: i32,
    scan_length: usize,
    frequency_sort_order: FrequencySortOrder,
    frequency_sort_dictionary: String,
) -> Result<Value, String> {
    let order = match frequency_sort_order {
        FrequencySortOrder::Auto => "auto",
        FrequencySortOrder::Ascending => "ascending",
        FrequencySortOrder::Descending => "descending",
        FrequencySortOrder::Disabled => "disabled",
    };
    let mut message = sharing_message("hd_lookup");
    message["text"] = json!(text);
    message["maxResults"] = json!(max_results.clamp(1, 256));
    message["scanLength"] = json!(scan_length.clamp(1, 64));
    message["options"] =
        json!({"frequencyOrder":order,"frequencyDictionary":frequency_sort_dictionary});
    let response = remote_request(&app, Some(message)).await?;
    *app.state::<SharingState>().media_generation.lock().unwrap() = response["generation"].as_i64();
    let results = response["results"]
        .as_array()
        .ok_or("The shared dictionary returned invalid lookup results.")?;
    let entries: Vec<Value> = results.iter().map(|result| {
        let term = &result["term"];
        let glossaries: Vec<Value> = term["glossaries"].as_array().into_iter().flatten().map(|glossary| json!({
            "dictionary":glossary["dictionary"], "content":glossary["glossary"],
            "definitionTags":glossary["definitionTags"], "termTags":glossary["termTags"]
        })).collect();
        let pitches: Vec<Value> = term["pitches"].as_array().into_iter().flatten().map(|group| {
            let accents: Vec<Value> = group["pitches"].as_array().into_iter().flatten().map(|pitch| {
                let position = pitch["pattern"].as_str().filter(|pattern| !pattern.is_empty())
                    .map(|pattern| json!(pattern)).unwrap_or_else(|| pitch["position"].clone());
                json!({"position":position,"nasal":pitch["nasal"],"devoice":pitch["devoice"]})
            }).collect();
            json!({"dictionary":group["dictionary"],"pitches":accents,"transcriptions":group["transcriptions"]})
        }).collect();
        let mut trace = result["trace"].as_array().cloned().unwrap_or_default();
        trace.reverse();
        json!({"expression":term["expression"],"reading":term["reading"],"matched":result["matched"],
            "deinflectionTrace":trace,"glossaries":glossaries,"frequencies":term["frequencies"],
            "pitches":pitches,"rules":term["rules"].as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>()})
    }).collect();
    let mut styles_request = sharing_message("hd_styles");
    styles_request["generation"] = response["generation"].clone();
    let styles_response = remote_request(&app, Some(styles_request)).await?;
    let styles: HashMap<String, String> = styles_response["styles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|style| {
            Some((
                style["dictionary"].as_str()?.to_string(),
                style["styles"].as_str()?.to_string(),
            ))
        })
        .collect();
    Ok(json!({"entries":entries,"styles":styles}))
}

#[tauri::command]
pub async fn sharing_kanji(app: AppHandle, character: String) -> Result<Value, String> {
    let mut message = sharing_message("hd_kanji");
    message["character"] = json!(character);
    let response = remote_request(&app, Some(message)).await?;
    if response["kanji"].is_null() {
        return Ok(Value::Null);
    }
    let kanji = &response["kanji"];
    let entries: Vec<Value> = kanji["entries"].as_array().into_iter().flatten().map(|entry| json!({
        "dictName":entry["dictionary"],"onyomi":entry["onyomi"],"kunyomi":entry["kunyomi"],"meanings":entry["definitions"]
    })).collect();
    Ok(json!({"character":kanji["character"],"entries":entries}))
}

pub(crate) async fn image_response(
    app: AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let remote = app
        .state::<SharingState>()
        .settings
        .lock()
        .unwrap()
        .lookup_source
        == "remote";
    if !remote {
        return tauri::async_runtime::spawn_blocking(move || dict::image_protocol(&app, request))
            .await
            .unwrap_or_else(|_| {
                tauri::http::Response::builder()
                    .status(500)
                    .body(Vec::new())
                    .unwrap()
            });
    }
    let query = request.uri().query().unwrap_or_default();
    let parameter = |key: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{key}=")))
            .and_then(|value| urlencoding::decode(value).ok())
            .map(|value| value.into_owned())
            .unwrap_or_default()
    };
    let result = async {
        let (mime, bytes) =
            remote_media(&app, &parameter("dictionary"), &parameter("path")).await?;
        Ok::<_, String>(
            tauri::http::Response::builder()
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(bytes)
                .unwrap(),
        )
    }
    .await;
    result.unwrap_or_else(|_| {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap()
    })
}

async fn remote_media(
    app: &AppHandle,
    dictionary: &str,
    path: &str,
) -> Result<(tauri::http::HeaderValue, Vec<u8>), String> {
    let mut message = sharing_message("hd_media");
    message["dictionary"] = json!(dictionary);
    message["path"] = json!(path);
    message["generation"] = json!(*app.state::<SharingState>().media_generation.lock().unwrap());
    let response = remote_request(app, Some(message)).await?;
    let data = response["dataUrl"]
        .as_str()
        .ok_or("Missing dictionary media.")?;
    let (mime, encoded) = data
        .strip_prefix("data:")
        .and_then(|data| data.split_once(";base64,"))
        .ok_or("Invalid dictionary media.")?;
    let mime = tauri::http::HeaderValue::from_str(mime).map_err(|error| error.to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    Ok((mime, bytes))
}

pub(crate) async fn media_file(
    app: &AppHandle,
    dictionary: &str,
    path: &str,
) -> Result<Vec<u8>, String> {
    let remote = app
        .state::<SharingState>()
        .settings
        .lock()
        .unwrap()
        .lookup_source
        == "remote";
    if remote {
        return remote_media(app, dictionary, path)
            .await
            .map(|(_, bytes)| bytes);
    }
    let app = app.clone();
    let dictionary = dictionary.to_string();
    let path = path.to_string();
    tauri::async_runtime::spawn_blocking(move || dict::media_file(&app, &dictionary, &path))
        .await
        .map_err(|error| error.to_string())
}
