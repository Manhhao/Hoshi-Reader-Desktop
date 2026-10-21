use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use reqwest::Method;
use reqwest::header::CONTENT_LENGTH;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};
use tokio::sync::Semaphore;
use tokio_util::io::ReaderStream;
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

static UPLOAD_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
});

const STALL_LIMIT: Duration = Duration::from_secs(150);

pub struct Meter {
    bytes: AtomicU64,
    moved: Mutex<Instant>,
}

impl Meter {
    pub fn new() -> Arc<Meter> {
        Arc::new(Meter {
            bytes: AtomicU64::new(0),
            moved: Mutex::new(Instant::now()),
        })
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::SeqCst)
    }

    pub fn set(&self, bytes: u64) {
        if self.bytes.swap(bytes, Ordering::SeqCst) != bytes {
            *self.moved.lock().unwrap() = Instant::now();
        }
    }

    pub fn touch(&self) {
        *self.moved.lock().unwrap() = Instant::now();
    }

    pub fn idle(&self) -> Duration {
        self.moved.lock().unwrap().elapsed()
    }
}

struct Counted<R> {
    inner: R,
    meter: Arc<Meter>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Counted<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        let read = (buf.filled().len() - before) as u64;
        if read > 0 {
            self.meter.set(self.meter.bytes() + read);
        }
        poll
    }
}

async fn watch<T>(future: impl std::future::Future<Output = T>, meter: &Meter) -> Result<T> {
    tokio::select! {
        value = future => Ok(value),
        _ = async {
            while meter.idle() < STALL_LIMIT {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        } => Err(GoogleDriveError::Unavailable(
            "The upload stopped making progress.".to_string(),
        )),
    }
}

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

pub enum Body<'a> {
    Data(Vec<u8>),
    File {
        prefix: Vec<u8>,
        path: &'a Path,
        suffix: Vec<u8>,
        meter: Arc<Meter>,
    },
}

pub enum Content<'a> {
    Data(Vec<u8>),
    File(&'a Path, &'a Arc<Meter>),
}

pub struct Request<'a> {
    pub path: &'a str,
    pub query: &'a [(&'a str, &'a str)],
    pub method: Method,
    pub body: Option<Body<'a>>,
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
static LIMIT: Semaphore = Semaphore::const_new(8);

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
    let uploading = matches!(request.body, Some(Body::File { .. }));
    let client = if uploading {
        &UPLOAD_CLIENT
    } else {
        &SESSION_CLIENT
    };
    let meter = match &request.body {
        Some(Body::File { meter, .. }) => meter.clone(),
        _ => Meter::new(),
    };
    let mut builder = client
        .request(request.method.clone(), url)
        .query(request.query)
        .bearer_auth(&token);
    if let Some(content_type) = request.content_type {
        builder = builder.header("Content-Type", content_type);
    }
    match &request.body {
        Some(Body::Data(data)) => builder = builder.body(data.clone()),
        Some(Body::File {
            prefix,
            path,
            suffix,
            ..
        }) => {
            let file = tokio::fs::File::open(path)
                .await
                .map_err(|error| GoogleDriveError::Api(error.to_string()))?;
            let length = file
                .metadata()
                .await
                .map_err(|error| GoogleDriveError::Api(error.to_string()))?
                .len();
            let reader = Cursor::new(prefix.clone())
                .chain(Counted {
                    inner: file,
                    meter: meter.clone(),
                })
                .chain(Cursor::new(suffix.clone()));
            builder = builder
                .header(
                    CONTENT_LENGTH,
                    prefix.len() as u64 + length + suffix.len() as u64,
                )
                .body(reqwest::Body::wrap_stream(ReaderStream::with_capacity(
                    reader,
                    256 * 1024,
                )));
        }
        None => {}
    }
    let permit = if on_progress.is_some() {
        None
    } else {
        Some(cancellable(LIMIT.acquire()).await?.unwrap())
    };
    let response = if uploading {
        cancellable(watch(builder.send(), &meter)).await???
    } else {
        cancellable(builder.send()).await??
    };
    check_connection(connection)?;
    crate::sync::task::check_cancellation()?;

    let status = response.status().as_u16();
    if status == 401 && retry {
        drop(permit);
        let token = auth::refresh_access_token()
            .await
            .map_err(GoogleDriveError::unavailable)?;
        check_connection(connection)?;
        crate::sync::task::check_cancellation()?;
        return Box::pin(perform_request(request, token, false, on_progress, attempt)).await;
    }

    let data = read_body(response, on_progress).await?;
    drop(permit);
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
    content: Content<'_>,
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
    let mut prefix =
        format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n")
            .into_bytes();
    prefix.extend_from_slice(metadata.to_string().as_bytes());
    prefix.extend_from_slice(
        format!("\r\n--{boundary}\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
    );
    let suffix = format!("\r\n--{boundary}--\r\n").into_bytes();
    let body = match content {
        Content::Data(data) => Body::Data([prefix, data, suffix].concat()),
        Content::File(path, meter) => Body::File {
            prefix,
            path,
            suffix,
            meter: meter.clone(),
        },
    };
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
    update_file(file_id, json!({ "trashed": true })).await
}

pub async fn rename_file(file_id: &str, name: &str) -> Result<()> {
    update_file(file_id, json!({ "name": name })).await
}

async fn update_file(file_id: &str, metadata: Value) -> Result<()> {
    let path = format!("files/{file_id}");
    request(Request {
        path: &path,
        query: &[],
        method: Method::PATCH,
        body: Some(Body::Data(metadata.to_string().into_bytes())),
        content_type: Some("application/json"),
        upload: false,
    })
    .await?;
    Ok(())
}

pub async fn delete_file(file_id: &str) -> Result<()> {
    let path = format!("files/{file_id}");
    request(Request {
        path: &path,
        query: &[],
        method: Method::DELETE,
        body: None,
        content_type: None,
        upload: false,
    })
    .await?;
    Ok(())
}

pub enum Upload {
    Done,
    Received(u64),
    Expired,
}

async fn send_upload(
    build: impl Fn(&str) -> Result<reqwest::RequestBuilder>,
    meter: &Meter,
) -> Result<reqwest::Response> {
    let mut token = auth::access_token().map_err(GoogleDriveError::Unavailable)?;
    for attempt in 0..5u32 {
        if STOPPED.load(Ordering::SeqCst) {
            return Err(GoogleDriveError::Cancelled);
        }
        if !crate::sync::settings().online {
            return Err(GoogleDriveError::Unavailable(
                "No Internet connection.".to_string(),
            ));
        }
        let connection = connection_id();
        let permit = cancellable(LIMIT.acquire()).await?.unwrap();
        meter.touch();
        let response = cancellable(watch(build(&token)?.send(), meter)).await???;
        drop(permit);
        check_connection(connection)?;
        crate::sync::task::check_cancellation()?;
        let status = response.status().as_u16();
        if status == 401 && attempt == 0 {
            token = auth::refresh_access_token()
                .await
                .map_err(GoogleDriveError::unavailable)?;
            continue;
        }
        if (status == 429 || status == 403) && attempt < 4 {
            let error: Value =
                serde_json::from_slice(&cancellable(response.bytes()).await??).unwrap_or_default();
            if is_transient(&Method::PUT, status, &error) {
                let jitter = Uuid::new_v4().as_u128() % BACKOFF.as_millis();
                crate::sync::task::sleep(
                    BACKOFF * 2u32.pow(attempt) + Duration::from_millis(jitter as u64),
                )
                .await?;
                continue;
            }
            let message = error["error"]["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("Request failed with status {status}"));
            return Err(GoogleDriveError::Api(message));
        }
        return Ok(response);
    }
    Err(GoogleDriveError::Unavailable(
        "Too many requests.".to_string(),
    ))
}

pub async fn start_upload(name: &str, parent: &str, total: u64) -> Result<String> {
    let body = json!({ "name": name, "parents": [parent] }).to_string();
    let response = send_upload(
        |token| {
            Ok(UPLOAD_CLIENT
                .post("https://www.googleapis.com/upload/drive/v3/files")
                .query(&[("uploadType", "resumable")])
                .bearer_auth(token)
                .header("Content-Type", "application/json; charset=UTF-8")
                .header("X-Upload-Content-Type", "application/octet-stream")
                .header("X-Upload-Content-Length", total)
                .body(body.clone()))
        },
        &Meter::new(),
    )
    .await?;
    let status = response.status().as_u16();
    match response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
    {
        Some(location) if status < 300 => Ok(location.to_string()),
        _ => Err(GoogleDriveError::Api(format!(
            "Request failed with status {status}"
        ))),
    }
}

pub async fn upload_status(session: &str, total: u64) -> Result<Upload> {
    let response = send_upload(
        |token| {
            Ok(UPLOAD_CLIENT
                .put(session)
                .bearer_auth(token)
                .header("Content-Range", format!("bytes */{total}"))
                .header(CONTENT_LENGTH, 0))
        },
        &Meter::new(),
    )
    .await?;
    upload_result(response).await
}

pub async fn upload_from(
    session: &str,
    path: &Path,
    offset: u64,
    total: u64,
    meter: &Arc<Meter>,
) -> Result<Upload> {
    let response = send_upload(
        |token| {
            meter.set(offset);
            let mut file = std::fs::File::open(path)
                .map_err(|error| GoogleDriveError::Api(error.to_string()))?;
            std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(offset))
                .map_err(|error| GoogleDriveError::Api(error.to_string()))?;
            Ok(UPLOAD_CLIENT
                .put(session)
                .bearer_auth(token)
                .header(
                    "Content-Range",
                    format!("bytes {offset}-{}/{total}", total.saturating_sub(1)),
                )
                .header(CONTENT_LENGTH, total - offset)
                .body(reqwest::Body::wrap_stream(ReaderStream::with_capacity(
                    Counted {
                        inner: tokio::fs::File::from_std(file),
                        meter: meter.clone(),
                    },
                    64 * 1024,
                ))))
        },
        meter,
    )
    .await?;
    if response.status().is_server_error() {
        return upload_status(session, total).await;
    }
    upload_result(response).await
}

async fn upload_result(response: reqwest::Response) -> Result<Upload> {
    let status = response.status().as_u16();
    let received = response
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit('-').next())
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(0, |last| last + 1);
    cancellable(response.bytes()).await??;
    match status {
        200 | 201 => Ok(Upload::Done),
        308 => Ok(Upload::Received(received)),
        404 | 410 => Ok(Upload::Expired),
        status => Err(GoogleDriveError::Api(format!(
            "Request failed with status {status}"
        ))),
    }
}
