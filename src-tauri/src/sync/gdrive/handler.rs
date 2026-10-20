use std::future::Future;
use std::pin::Pin;

use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use unicode_normalization::UnicodeNormalization;

use crate::sync::client::{self, GoogleDriveError, GoogleDriveFile, Request, Result};
use crate::sync::task;

const REPLACED_ROOT: &str = "Hoshi Reader (replaced)";
const FILE_FIELDS: &str = "id,name,mimeType,md5Checksum,size,parents,trashed,createdTime";

impl GoogleDriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == "application/vnd.google-apps.folder"
    }

    pub fn is_recent(&self) -> bool {
        chrono::DateTime::parse_from_rfc3339(&self.created_time).is_ok_and(|created| {
            (chrono::Utc::now() - created.with_timezone(&chrono::Utc)).num_milliseconds()
                < 86_400_000
        })
    }

    pub fn is_newer_format(&self) -> bool {
        self.name
            .strip_prefix(".format-")
            .and_then(|version| version.parse::<i64>().ok())
            .is_some_and(|version| version > 1)
    }

    pub fn state_key(&self) -> Option<String> {
        (self.name.ends_with(".json") && !self.is_folder())
            .then(|| self.name[..self.name.len() - 5].nfc().collect())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleDriveFileList {
    files: Vec<GoogleDriveFile>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
pub struct Change {
    pub removed: bool,
    pub file: Option<GoogleDriveFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveChanges {
    pub changes: Vec<Change>,
    pub next_page_token: Option<String>,
    pub new_start_page_token: Option<String>,
}

pub struct Layout {
    pub root: String,
    pub state: String,
    pub books: String,
    pub created: bool,
}

fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T> {
    serde_json::from_slice(data).map_err(|error| GoogleDriveError::Api(error.to_string()))
}

pub async fn start_token() -> Result<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Token {
        start_page_token: String,
    }

    let data = client::get("changes/startPageToken", &[]).await?;
    Ok(decode::<Token>(&data)?.start_page_token)
}

pub async fn changes(cursor: &str) -> Result<GoogleDriveChanges> {
    let fields = format!("nextPageToken,newStartPageToken,changes(removed,file({FILE_FIELDS}))");
    let data = client::get(
        "changes",
        &[
            ("pageToken", cursor),
            ("pageSize", "1000"),
            ("spaces", "drive"),
            ("includeRemoved", "true"),
            ("fields", &fields),
        ],
    )
    .await?;
    decode(&data)
}

pub async fn layout() -> Result<Layout> {
    let existing = folder("root", "Hoshi Reader", false).await?;
    let created = existing.is_none();
    let root = match existing {
        Some(root) => root,
        None => create_folder("root", "Hoshi Reader").await?,
    };
    let state = folder(&root, "state", true).await?.unwrap();
    let books = folder(&root, "books", true).await?.unwrap();
    Ok(Layout {
        root,
        state,
        books,
        created,
    })
}

pub async fn rename_root() -> Result<()> {
    for folder in children("root", Some("Hoshi Reader")).await? {
        if folder.is_folder() {
            client::rename_file(&folder.id, REPLACED_ROOT).await?;
        }
    }
    Ok(())
}

pub async fn remove_replaced() -> Result<()> {
    for folder in children("root", Some(REPLACED_ROOT)).await? {
        delete_tree(folder.id).await?;
    }
    Ok(())
}

fn delete_tree(id: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    Box::pin(async move {
        match client::delete_file(&id).await {
            Err(GoogleDriveError::Api(_)) => {}
            result => return result,
        }
        let files = children(&id, None)
            .await?
            .into_iter()
            .map(|file| delete_tree(file.id))
            .collect();
        task::concurrent(files).await?;
        client::delete_file(&id).await
    })
}

pub async fn folder(parent: &str, name: &str, create: bool) -> Result<Option<String>> {
    if let Some(folder) = children(parent, Some(name))
        .await?
        .into_iter()
        .find(GoogleDriveFile::is_folder)
    {
        return Ok(Some(folder.id));
    }

    if !create {
        return Ok(None);
    }
    create_folder(parent, name).await.map(Some)
}

pub async fn create_folder(parent: &str, name: &str) -> Result<String> {
    let body = json!({
        "name": name,
        "parents": [parent],
        "mimeType": "application/vnd.google-apps.folder"
    });

    let data = client::request(Request {
        path: "files",
        query: &[("fields", FILE_FIELDS)],
        method: Method::POST,
        body: Some(body.to_string().into_bytes()),
        content_type: Some("application/json"),
        upload: false,
    })
    .await?;
    Ok(decode::<GoogleDriveFile>(&data)?.id)
}

pub async fn children(parent: &str, name: Option<&str>) -> Result<Vec<GoogleDriveFile>> {
    let mut query = format!("'{}' in parents", escape(parent));
    if let Some(name) = name {
        query += &format!(" and name='{}'", escape(name));
    }
    list(&query).await
}

pub async fn list(query: &str) -> Result<Vec<GoogleDriveFile>> {
    let mut result: Vec<GoogleDriveFile> = Vec::new();
    let mut cursor: Option<String> = None;
    let q = format!("trashed=false and ({query})");
    let fields = format!("nextPageToken,files({FILE_FIELDS})");

    loop {
        let mut items = vec![
            ("q", q.as_str()),
            ("pageSize", "1000"),
            ("spaces", "drive"),
            ("fields", fields.as_str()),
        ];

        if let Some(cursor) = &cursor {
            items.push(("pageToken", cursor.as_str()));
        }

        let data = client::get("files", &items).await?;
        let page: GoogleDriveFileList = decode(&data)?;

        result.extend(page.files);
        cursor = page.next_page_token;
        if cursor.is_none() {
            break;
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

pub async fn read(file: &GoogleDriveFile) -> Result<Vec<u8>> {
    client::get(&format!("files/{}", file.id), &[("alt", "media")]).await
}

pub async fn upload(data: Vec<u8>, file_name: &str, folder: &str) -> Result<()> {
    let existing = children(folder, Some(file_name)).await?;
    if !existing.is_empty() {
        return Ok(());
    }
    client::write(data, file_name, folder, None, "application/octet-stream").await?;
    Ok(())
}

pub async fn download(
    file: &GoogleDriveFile,
    on_progress: &(dyn Fn(f64) + Sync),
) -> Result<Vec<u8>> {
    client::download_file(
        &file.id,
        file.size
            .as_ref()
            .and_then(|size| size.parse().ok())
            .unwrap(),
        on_progress,
    )
    .await
}

pub async fn trash(file: &GoogleDriveFile) -> Result<()> {
    client::trash_file(&file.id).await
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
