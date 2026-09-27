use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::library;
use crate::sync::model::Timestamped;

const HIGHLIGHTS_FILE: &str = "highlights.json";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum HighlightColor {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl HighlightColor {
    pub fn raw_value(&self) -> &'static str {
        match self {
            HighlightColor::Yellow => "yellow",
            HighlightColor::Green => "green",
            HighlightColor::Blue => "blue",
            HighlightColor::Pink => "pink",
            HighlightColor::Purple => "purple",
        }
    }

    pub fn from_raw_value(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub character: i64,
    pub offset: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_furigana: Option<String>,
    pub color: HighlightColor,
    pub created_at: f64,
}

pub type HighlightRecords = HashMap<String, Timestamped<Option<Highlight>>>;

pub fn load_highlight_records(root: &Path) -> HighlightRecords {
    let path = root.join(HIGHLIGHTS_FILE);
    if let Some(records) = library::read_json::<HighlightRecords>(&path) {
        return records;
    }
    let highlights = library::read_json::<Vec<Highlight>>(&path).unwrap_or_default();
    highlights
        .into_iter()
        .map(|highlight| {
            (
                highlight.id.clone(),
                Timestamped {
                    modified: library::apple_to_ms(highlight.created_at),
                    value: Some(highlight),
                },
            )
        })
        .collect()
}

pub fn load(root: &Path) -> Vec<Highlight> {
    let mut highlights: Vec<Highlight> = load_highlight_records(root)
        .into_values()
        .filter_map(|record| record.value)
        .collect();
    highlights.sort_by(|a, b| a.created_at.total_cmp(&b.created_at));
    highlights
}

pub fn save(highlights: &[Highlight], root: &Path) -> Result<(), String> {
    let mut records = load_highlight_records(root);
    let now = library::now_ms();
    let ids: HashSet<&str> = highlights
        .iter()
        .map(|highlight| highlight.id.as_str())
        .collect();
    for (id, record) in records.iter_mut() {
        if record.value.is_some() && !ids.contains(id.as_str()) {
            *record = Timestamped {
                modified: now,
                value: None,
            };
        }
    }
    for highlight in highlights {
        if records.get(&highlight.id).is_none_or(|record| {
            record
                .value
                .as_ref()
                .is_some_and(|value| value != highlight)
        }) {
            records.insert(
                highlight.id.clone(),
                Timestamped {
                    modified: now,
                    value: Some(highlight.clone()),
                },
            );
        }
    }
    library::write_json(&root.join(HIGHLIGHTS_FILE), &records)
}

pub fn save_records(records: &HighlightRecords, root: &Path) -> Result<(), String> {
    library::write_json(&root.join(HIGHLIGHTS_FILE), records)
}

#[tauri::command]
pub fn load_highlights(app: AppHandle, id: String) -> Vec<Highlight> {
    library::book_dir(&app, &id)
        .map(|root| load(&root))
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_highlights(
    app: AppHandle,
    id: String,
    highlights: Vec<Highlight>,
) -> Result<(), String> {
    let root = library::book_dir(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    save(&highlights, &root)?;
    let folder = library::load_metadata_at(&root).unwrap().folder;
    crate::sync::storage::shared()
        .handle_book_change(&folder)
        .map_err(|error| error.to_string())
}
