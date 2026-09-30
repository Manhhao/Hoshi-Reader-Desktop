use std::collections::HashMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::highlights::{Highlight, HighlightColor};
use crate::library::{apple_to_ms, ms_to_apple};
use crate::statistics::ReadingSession;
use crate::sync::client::GoogleDriveError;

#[derive(Debug, Clone)]
pub enum SyncError {
    UnsupportedVersion,
    Drive(GoogleDriveError),
    Other(String),
}

impl SyncError {
    pub fn is_format_error(&self) -> bool {
        matches!(self, SyncError::UnsupportedVersion)
    }

    pub fn stops_run(&self) -> bool {
        matches!(
            self,
            SyncError::UnsupportedVersion
                | SyncError::Drive(GoogleDriveError::Unavailable(_) | GoogleDriveError::Cancelled)
        )
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::UnsupportedVersion => write!(f, "Unsupported sync format."),
            SyncError::Drive(error) => write!(f, "{error}"),
            SyncError::Other(message) => write!(f, "{message}"),
        }
    }
}

impl From<GoogleDriveError> for SyncError {
    fn from(error: GoogleDriveError) -> Self {
        SyncError::Drive(error)
    }
}

impl From<std::io::Error> for SyncError {
    fn from(error: std::io::Error) -> Self {
        SyncError::Other(error.to_string())
    }
}

impl From<serde_json::Error> for SyncError {
    fn from(error: serde_json::Error) -> Self {
        SyncError::Other(error.to_string())
    }
}

impl From<String> for SyncError {
    fn from(message: String) -> Self {
        SyncError::Other(message)
    }
}

pub type SyncResult<T> = Result<T, SyncError>;

pub mod sync_format {
    use super::*;

    pub fn decode<T: DeserializeOwned>(data: &[u8]) -> SyncResult<T> {
        let document: Value = serde_json::from_slice(data)?;
        if document.get("formatVersion").and_then(Value::as_f64) != Some(1.0) {
            return Err(SyncError::UnsupportedVersion);
        }
        Ok(T::deserialize(document)?)
    }

    pub fn encode<T: Serialize>(value: &T) -> SyncResult<Vec<u8>> {
        let mut document = serde_json::to_value(value)?;
        document["formatVersion"] = Value::from(1);
        Ok(serde_json::to_vec(&document)?)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Timestamped<V> {
    pub modified: i64,
    pub value: V,
}

impl<V: Clone> Timestamped<V> {
    pub fn replacing<T>(&self, value: T) -> Timestamped<T> {
        Timestamped {
            modified: self.modified,
            value,
        }
    }

    pub fn newest(first: &Self, second: &Self) -> Self {
        if first.modified >= second.modified {
            first.clone()
        } else {
            second.clone()
        }
    }

    pub fn newest_option(first: &Option<Self>, second: &Option<Self>) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => Some(Self::newest(first, second)),
            (Some(first), None) => Some(first.clone()),
            (None, Some(second)) => Some(second.clone()),
            (None, None) => None,
        }
    }

    pub fn merge<K: Clone + Eq + std::hash::Hash>(
        first: &HashMap<K, Self>,
        second: &HashMap<K, Self>,
    ) -> HashMap<K, Self> {
        let mut result = first.clone();
        for (key, value) in second {
            let merged = match result.get(key) {
                Some(existing) => Self::newest(existing, value),
                None => value.clone(),
            };
            result.insert(key.clone(), merged);
        }
        result
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SyncMetadata {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SyncFileType {
    Epub,
    Cover,
    Sasayaki,
}

impl SyncFileType {
    pub const ALL_CASES: [SyncFileType; 3] = [
        SyncFileType::Epub,
        SyncFileType::Cover,
        SyncFileType::Sasayaki,
    ];
}

pub type SyncFiles = HashMap<SyncFileType, Timestamped<Option<String>>>;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncBookmark {
    pub character_count: i64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlayback {
    pub last_position: f64,
    pub delay: f64,
    pub rate: f64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncHighlight {
    pub character: i64,
    pub offset: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_furigana: Option<String>,
    pub color: String,
    pub created_at: i64,
}

impl SyncHighlight {
    pub fn new(highlight: &Highlight) -> Self {
        SyncHighlight {
            character: highlight.character,
            offset: highlight.offset,
            text: highlight.text.clone(),
            text_furigana: highlight.text_furigana.clone(),
            color: highlight.color.raw_value().to_string(),
            created_at: apple_to_ms(highlight.created_at),
        }
    }

    pub fn highlight(&self, id: &str) -> Highlight {
        Highlight {
            id: id.to_string(),
            character: self.character,
            offset: self.offset,
            text: self.text.clone(),
            text_furigana: self.text_furigana.clone(),
            color: HighlightColor::from_raw_value(&self.color).unwrap(),
            created_at: ms_to_apple(self.created_at),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncBook {
    pub generation: i64,
    pub deleted: bool,
    pub metadata: Timestamped<SyncMetadata>,
    pub character_count: i64,
    pub files: SyncFiles,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<Timestamped<SyncBookmark>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiobook: Option<Timestamped<SyncPlayback>>,
    pub highlights: HashMap<String, Timestamped<Option<SyncHighlight>>>,
    pub sessions: HashMap<String, Timestamped<Option<ReadingSession>>>,
    pub shelves: HashMap<String, Timestamped<bool>>,
}

impl SyncBook {
    pub fn new(generation: i64, deleted: bool, metadata: Timestamped<SyncMetadata>) -> Self {
        SyncBook {
            generation,
            deleted,
            metadata,
            character_count: 0,
            files: HashMap::new(),
            bookmark: None,
            audiobook: None,
            highlights: HashMap::new(),
            sessions: HashMap::new(),
            shelves: HashMap::new(),
        }
    }

    pub fn delete(&mut self) {
        self.deleted = true;
        self.files.remove(&SyncFileType::Epub);
        self.files.remove(&SyncFileType::Sasayaki);
        self.bookmark = None;
        self.audiobook = None;
        self.highlights = HashMap::new();
        self.shelves = HashMap::new();
    }

    pub fn needs_upload(&self, remote: Option<&SyncBook>) -> bool {
        let Some(remote) = remote else {
            return true;
        };
        &Self::merge(remote, self) != remote
    }

    pub fn merge(first: &Self, second: &Self) -> Self {
        let mut result;
        if first.generation != second.generation {
            result = if first.generation > second.generation {
                first.clone()
            } else {
                second.clone()
            };
        } else {
            result = first.clone();
            result.metadata = Timestamped::newest(&first.metadata, &second.metadata);
            result.character_count = first.character_count.max(second.character_count);

            result.files = Timestamped::merge(&first.files, &second.files);

            result.bookmark = Timestamped::newest_option(&first.bookmark, &second.bookmark);
            result.audiobook = Timestamped::newest_option(&first.audiobook, &second.audiobook);
            result.highlights = Self::merge_records(&first.highlights, &second.highlights);
            result.shelves = Timestamped::merge(&first.shelves, &second.shelves);

            if first.deleted || second.deleted {
                result.delete();
            }
        }

        result.sessions = Self::merge_records(&first.sessions, &second.sessions);
        result
    }

    pub fn merge_records<T: Clone>(
        first: &HashMap<String, Timestamped<Option<T>>>,
        second: &HashMap<String, Timestamped<Option<T>>>,
    ) -> HashMap<String, Timestamped<Option<T>>> {
        let mut result = first.clone();
        for (key, second) in second {
            let merged = match result.get(key) {
                Some(first) => {
                    if first.value.is_none() && second.value.is_some() {
                        first.clone()
                    } else if second.value.is_none() && first.value.is_some() {
                        second.clone()
                    } else {
                        Timestamped::newest(first, second)
                    }
                }
                None => second.clone(),
            };
            result.insert(key.clone(), merged);
        }
        result
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SyncShelves {
    pub shelves: HashMap<String, Timestamped<Option<i64>>>,
    pub orders: HashMap<String, Timestamped<Option<Vec<String>>>>,
}

impl SyncShelves {
    pub fn new(shelves: HashMap<String, Timestamped<Option<i64>>>) -> Self {
        SyncShelves {
            shelves,
            orders: HashMap::new(),
        }
    }

    pub fn merge(first: &Self, second: &Self) -> Self {
        SyncShelves {
            shelves: Timestamped::merge(&first.shelves, &second.shelves),
            orders: Timestamped::merge(&first.orders, &second.orders),
        }
    }
}
