use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::sync::auth;

#[derive(Debug, Clone)]
pub enum GoogleDriveError {
    Api(String),
    Unavailable(String),
    Cancelled,
}

impl GoogleDriveError {
    fn unavailable(self) -> Self {
        match self {
            GoogleDriveError::Api(message) => GoogleDriveError::Unavailable(message),
            error => error,
        }
    }
}

impl fmt::Display for GoogleDriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoogleDriveError::Api(message) | GoogleDriveError::Unavailable(message) => {
                write!(f, "{message}")
            }
            GoogleDriveError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl From<reqwest::Error> for GoogleDriveError {
    fn from(error: reqwest::Error) -> Self {
        GoogleDriveError::Unavailable(error.to_string())
    }
}

impl From<String> for GoogleDriveError {
    fn from(message: String) -> Self {
        GoogleDriveError::Api(message)
    }
}

pub type Result<T> = std::result::Result<T, GoogleDriveError>;

#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    #[serde(default, rename = "md5Checksum")]
    pub checksum: String,
    pub size: Option<String>,
    pub parents: Option<Vec<String>>,
    pub trashed: Option<bool>,
    pub created_time: String,
}

static STOPPED: AtomicBool = AtomicBool::new(false);
static CONNECTION_ID: AtomicU64 = AtomicU64::new(0);
static SESSION: LazyLock<Mutex<CancellationToken>> =
    LazyLock::new(|| Mutex::new(CancellationToken::new()));

static SESSION_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(60))
        .read_timeout(Duration::from_secs(60))
        .build()
        .unwrap()
});

pub fn connection_id() -> u64 {
    CONNECTION_ID.load(Ordering::SeqCst)
}

pub fn stop() {
    STOPPED.store(true, Ordering::SeqCst);
    CONNECTION_ID.fetch_add(1, Ordering::SeqCst);
    let session = std::mem::take(&mut *SESSION.lock().unwrap());
    session.cancel();
}

pub fn resume() {
    STOPPED.store(false, Ordering::SeqCst);
}

pub fn check_connection(connection: u64) -> Result<()> {
    if connection != connection_id() {
        return Err(GoogleDriveError::Cancelled);
    }
    Ok(())
}

pub async fn cancellable<T>(future: impl std::future::Future<Output = T>) -> Result<T> {
    let session = SESSION.lock().unwrap().clone();
    let task = crate::sync::task::current().unwrap_or_default();
    tokio::select! {
        value = future => Ok(value),
        _ = session.cancelled() => Err(GoogleDriveError::Cancelled),
        _ = task.cancelled() => Err(GoogleDriveError::Cancelled),
    }
}

pub struct Request<'a> {
    pub path: &'a str,
    pub query: &'a [(&'a str, &'a str)],
    pub method: Method,
    pub body: Option<Vec<u8>>,
    pub content_type: Option<&'a str>,
    pub upload: bool,
}

impl<'a> Request<'a> {
    pub fn get(path: &'a str, query: &'a [(&'a str, &'a str)]) -> Self {
        Request {
            path,
            query,
            method: Method::GET,
            body: None,
            content_type: Some("application/json"),
            upload: false,
        }
    }
}

pub async fn request(request: Request<'_>) -> Result<Vec<u8>> {
    perform_request(
        &request,
        auth::access_token().map_err(GoogleDriveError::Unavailable)?,
        true,
        None,
        0,
    )
    .await
}

pub async fn get(path: &str, query: &[(&str, &str)]) -> Result<Vec<u8>> {
    request(Request::get(path, query)).await
}

const BACKOFF: Duration = Duration::from_secs(1);

fn is_transient(method: &Method, status: u16, error: &Value) -> bool {
    let limited = status == 429
        || (status == 403
            && error["error"]["errors"][0]["reason"]
                .as_str()
                .is_some_and(|reason| reason.ends_with("ateLimitExceeded")));
    limited || (status >= 500 && *method != Method::POST)
}

async fn perform_request(
    request: &Request<'_>,
    token: String,
    retry: bool,
    on_progress: Option<&(dyn Fn(f64) + Sync)>,
    attempt: u32,
) -> Result<Vec<u8>> {
    if STOPPED.load(Ordering::SeqCst) {
        return Err(GoogleDriveError::Cancelled);
    }
    if !crate::sync::settings().online {
        return Err(GoogleDriveError::Unavailable(
            "No Internet connection.".to_string(),
        ));
    }

    let connection = connection_id();
    let url = format!(
        "https://www.googleapis.com/{}drive/v3/{}",
        if request.upload { "upload/" } else { "" },
        request.path
    );
    let mut builder = SESSION_CLIENT
        .request(request.method.clone(), url)
        .query(request.query)
        .bearer_auth(&token);
    if let Some(content_type) = request.content_type {
        builder = builder.header("Content-Type", content_type);
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }
    let response = cancellable(builder.send()).await??;
    check_connection(connection)?;
    crate::sync::task::check_cancellation()?;

    let status = response.status().as_u16();
    if status == 401 && retry {
        let token = auth::refresh_access_token()
            .await
            .map_err(GoogleDriveError::unavailable)?;
        check_connection(connection)?;
        crate::sync::task::check_cancellation()?;
        return Box::pin(perform_request(request, token, false, on_progress, attempt)).await;
    }

    let data = read_body(response, on_progress).await?;
    check_connection(connection)?;
    crate::sync::task::check_cancellation()?;
    if status >= 400 {
        let error = serde_json::from_slice::<Value>(&data).unwrap_or_default();
        if attempt < 4 && is_transient(&request.method, status, &error) {
            let jitter = Uuid::new_v4().as_u128() % BACKOFF.as_millis();
            let delay = BACKOFF * 2u32.pow(attempt) + Duration::from_millis(jitter as u64);
            crate::sync::task::sleep(delay).await?;
            check_connection(connection)?;
            return Box::pin(perform_request(
                request,
                token,
                retry,
                on_progress,
                attempt + 1,
            ))
            .await;
        }
        let message = error["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Request failed with status {status}"));
        return Err(GoogleDriveError::Api(message));
    }
    Ok(data)
}

async fn read_body(
    mut response: reqwest::Response,
    on_progress: Option<&(dyn Fn(f64) + Sync)>,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    while let Some(chunk) = cancellable(response.chunk()).await?? {
        data.extend_from_slice(&chunk);
        if let Some(on_progress) = on_progress {
            on_progress(data.len() as f64);
        }
    }
    Ok(data)
}

pub async fn write(
    data: Vec<u8>,
    name: &str,
    parent: &str,
    file_id: Option<&str>,
    content_type: &str,
) -> Result<GoogleDriveFile> {
    let metadata = if file_id.is_none() {
        json!({ "name": name, "parents": [parent] })
    } else {
        json!({ "name": name })
    };
    let boundary = Uuid::new_v4().to_string().to_uppercase();
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n")
        .into_bytes();
    body.extend_from_slice(metadata.to_string().as_bytes());
    body.extend_from_slice(
        format!("\r\n--{boundary}\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(&data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let path = file_id.map_or_else(|| "files".to_string(), |id| format!("files/{id}"));
    let content_type = format!("multipart/related; boundary={boundary}");
    let response = request(Request {
        path: &path,
        query: &[
            ("uploadType", "multipart"),
            ("fields", "id,name,mimeType,md5Checksum,createdTime"),
        ],
        method: if file_id.is_none() {
            Method::POST
        } else {
            Method::PATCH
        },
        body: Some(body),
        content_type: Some(&content_type),
        upload: true,
    })
    .await?;
    serde_json::from_slice(&response).map_err(|error| GoogleDriveError::Api(error.to_string()))
}

pub async fn download_file(
    file_id: &str,
    file_size: i64,
    on_progress: &(dyn Fn(f64) + Sync),
) -> Result<Vec<u8>> {
    let path = format!("files/{file_id}");
    let progress = |received: f64| {
        if file_size > 0 {
            on_progress(received / file_size as f64);
        }
    };
    perform_request(
        &Request {
            path: &path,
            query: &[("alt", "media")],
            method: Method::GET,
            body: None,
            content_type: None,
            upload: false,
        },
        auth::access_token().map_err(GoogleDriveError::Unavailable)?,
        true,
        Some(&progress),
        0,
    )
    .await
}

pub async fn trash_file(file_id: &str) -> Result<()> {
    let path = format!("files/{file_id}");
    request(Request {
        path: &path,
        query: &[],
        method: Method::PATCH,
        body: Some(json!({ "trashed": true }).to_string().into_bytes()),
        content_type: Some("application/json"),
        upload: false,
    })
    .await?;
    Ok(())
}
