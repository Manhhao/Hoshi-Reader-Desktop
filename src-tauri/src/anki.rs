use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha1::{Digest, Sha1};
use tauri::{AppHandle, Manager, State};

use crate::dict;
use crate::library;
use crate::local_audio;
use crate::sasayaki;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

#[derive(Default)]
pub struct AnkiState {
    pub reachable: AtomicBool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnkiCardFormat {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub selected_deck: Option<String>,
    #[serde(default)]
    pub selected_note_type: Option<String>,
    #[serde(default)]
    pub field_mappings: HashMap<String, String>,
    #[serde(default)]
    pub tags: String,
    #[serde(default = "default_format_icon")]
    pub icon: String,
}

fn default_format_icon() -> String {
    "plus-square".to_string()
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoteType {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum DuplicateScope {
    #[default]
    Collection,
    Deck,
    Deckroot,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioSource {
    pub name: String,
    pub url: String,
    pub is_enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AnkiConfig {
    pub disabled: bool,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub duplicate_scope: DuplicateScope,
    pub check_all_models: bool,
    pub force_sync: bool,
    pub allow_dupes: bool,
    pub disable_show_notes: bool,
    pub compact_glossaries: bool,
    pub card_formats: Vec<AnkiCardFormat>,
    pub available_decks: Vec<String>,
    pub available_note_types: Vec<NoteType>,
    pub selected_glossary_fallback: String,
    pub show_all_handlebars: bool,
    pub audio_sources: Vec<AudioSource>,
    pub audio_enable_autoplay: bool,
    pub enable_local_audio: bool,
    pub local_audio_path: Option<String>,
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().unwrap();
    fs::create_dir_all(&dir).unwrap();
    dir.join("anki_config.json")
}

pub fn load_config(app: &AppHandle) -> AnkiConfig {
    library::read_json(&config_path(app)).unwrap_or_else(|| AnkiConfig {
        url: Some("http://127.0.0.1:8765".to_string()),
        ..AnkiConfig::default()
    })
}

fn save_config(app: &AppHandle, config: &AnkiConfig) {
    let _ = fs::write(config_path(app), serde_json::to_vec_pretty(config).unwrap());
}

pub(crate) fn update_handlebar(app: &AppHandle, old: &str, new: &str) {
    let mut config = load_config(app);
    for card_format in &mut config.card_formats {
        for mapping in card_format.field_mappings.values_mut() {
            for suffix in ["", "-brief", "-no-dictionary"] {
                *mapping = mapping.replace(
                    &format!("{{single-glossary-{old}{suffix}}}"),
                    &format!("{{single-glossary-{new}{suffix}}}"),
                );
            }
        }
    }
    save_config(app, &config);
}

fn note_type<'a>(config: &'a AnkiConfig, format: &AnkiCardFormat) -> Option<&'a NoteType> {
    let name = format.selected_note_type.as_deref()?;
    config.available_note_types.iter().find(|n| n.name == name)
}

fn first_field<'a>(config: &'a AnkiConfig, format: &AnkiCardFormat) -> Option<&'a String> {
    note_type(config, format).and_then(|n| n.fields.first())
}

fn valid_format_flags(config: &AnkiConfig) -> Vec<bool> {
    config
        .card_formats
        .iter()
        .map(|f| {
            first_field(config, f)
                .and_then(|field| f.field_mappings.get(field))
                .is_some_and(|m| !m.is_empty())
        })
        .collect()
}

fn needs_audio(config: &AnkiConfig) -> bool {
    config
        .card_formats
        .iter()
        .any(|f| f.field_mappings.values().any(|v| v == "{audio}"))
}

async fn anki_request_timeout(
    config: &AnkiConfig,
    action: &str,
    params: Value,
    timeout_secs: u64,
) -> Result<Value, String> {
    let url = config.url.as_deref().ok_or("Invalid URL specified")?;
    let mut body = Map::new();
    body.insert("action".into(), json!(action));
    body.insert("version".into(), json!(6));
    if !params.is_null() {
        body.insert("params".into(), params);
    }
    if let Some(key) = &config.api_key
        && !key.is_empty()
    {
        body.insert("key".into(), json!(key));
    }

    let resp = CLIENT
        .post(url)
        .json(&Value::Object(body))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(err) = json.get("error").and_then(|e| e.as_str())
        && !err.is_empty()
    {
        return Err(err.to_string());
    }
    Ok(json.get("result").cloned().unwrap_or(Value::Null))
}

async fn anki_request(config: &AnkiConfig, action: &str, params: Value) -> Result<Value, String> {
    anki_request_timeout(config, action, params, 10).await
}

fn local_anki(config: &AnkiConfig) -> bool {
    config
        .url
        .as_deref()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]"))
}

fn duplicate_options(config: &AnkiConfig, deck: &str) -> Value {
    let mut options = Map::new();
    match config.duplicate_scope {
        DuplicateScope::Collection => {
            options.insert("duplicateScope".into(), json!("collection"));
        }
        DuplicateScope::Deck | DuplicateScope::Deckroot => {
            options.insert("duplicateScope".into(), json!("deck"));
            if matches!(config.duplicate_scope, DuplicateScope::Deckroot) {
                let root = deck.split("::").next().unwrap_or(deck);
                options.insert(
                    "duplicateScopeOptions".into(),
                    json!({ "deckName": root, "checkChildren": true }),
                );
            }
        }
    }
    if config.check_all_models {
        let mut dso = options
            .get("duplicateScopeOptions")
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        dso.insert("checkAllModels".into(), json!(true));
        options.insert("duplicateScopeOptions".into(), Value::Object(dso));
    }
    Value::Object(options)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiningContext {
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub cloze_offset: Option<i64>,
    #[serde(default)]
    pub document_title: Option<String>,
    #[serde(default)]
    pub book_id: Option<String>,
    #[serde(default)]
    pub sasayaki_cue: Option<String>,
}

#[derive(Deserialize)]
struct DictionaryMedia {
    dictionary: String,
    path: String,
    filename: String,
}

static RE_GLOSSARY_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(<li data-dictionary="[^"]*">)<i>[^<]*</i> "#).unwrap());
static RE_GLOSSARY_DICT_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<li data-dictionary="(?P<dict>[^"]+)"><i>(?P<label>[^<]*)</i> "#).unwrap()
});
static RE_SVG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<svg\b.*?</svg>").unwrap());
static RE_HANDLEBAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]*\}").unwrap());

fn strip_glossary_headers(html: &str) -> String {
    RE_GLOSSARY_HEADER.replace_all(html, "$1").into_owned()
}

fn strip_dictionary_name(html: &str) -> String {
    RE_GLOSSARY_DICT_NAME
        .replace_all(html, |caps: &Captures| {
            let dict = &caps["dict"];
            let label = &caps["label"];
            let stripped = label.replace(&format!(", {dict})"), ")");
            if stripped == format!("({dict})") {
                format!("<li data-dictionary=\"{dict}\">")
            } else {
                format!("<li data-dictionary=\"{dict}\"><i>{stripped}</i> ")
            }
        })
        .into_owned()
}

fn first_pitch_accent_graph(html: &str) -> String {
    RE_SVG
        .find(html)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

fn cloze_parts(sentence: &str, matched: &str, offset: Option<i64>) -> (String, String, String) {
    if matched.is_empty() {
        return (sentence.to_string(), String::new(), String::new());
    }
    let byte_pos = offset
        .filter(|o| *o >= 0)
        .and_then(|o| utf16_to_byte(sentence, o as usize))
        .filter(|&p| sentence[p..].starts_with(matched))
        .or_else(|| sentence.find(matched));
    if let Some(pos) = byte_pos {
        let prefix = sentence[..pos].to_string();
        let body = matched.to_string();
        let suffix = sentence[pos + matched.len()..].to_string();
        (prefix, body, suffix)
    } else {
        (sentence.to_string(), String::new(), String::new())
    }
}

fn utf16_to_byte(s: &str, utf16_offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte_idx, ch) in s.char_indices() {
        if units == utf16_offset {
            return Some(byte_idx);
        }
        units += ch.len_utf16();
    }
    if units == utf16_offset {
        Some(s.len())
    } else {
        None
    }
}

fn first_glossary(
    app: &AppHandle,
    category: Option<dict::DictionaryCategory>,
    single_glossaries: &HashMap<String, String>,
) -> String {
    for (title, dict_category) in dict::term_dict_categories(app) {
        if dict_category == dict::DictionaryCategory::Exclude {
            continue;
        }
        if category.is_some_and(|c| c != dict_category) {
            continue;
        }
        if let Some(g) = single_glossaries.get(&title) {
            return g.clone();
        }
    }
    String::new()
}

fn handlebar_to_value(
    app: &AppHandle,
    config: &AnkiConfig,
    handlebar: &str,
    context: &MiningContext,
    content: &HashMap<String, String>,
    single_glossaries: &HashMap<String, String>,
) -> String {
    let get = |k: &str| content.get(k).cloned().unwrap_or_default();

    if let Some(rest) = handlebar.strip_prefix("{single-glossary-") {
        let name = rest.trim_end_matches('}');
        if let Some(base) = name.strip_suffix("-brief") {
            return strip_glossary_headers(
                single_glossaries
                    .get(base)
                    .map(String::as_str)
                    .unwrap_or(""),
            );
        }
        if let Some(base) = name.strip_suffix("-no-dictionary") {
            return strip_dictionary_name(
                single_glossaries
                    .get(base)
                    .map(String::as_str)
                    .unwrap_or(""),
            );
        }
        return single_glossaries.get(name).cloned().unwrap_or_default();
    }

    let selected_glossary = || {
        let selected = get("selectedDictionary");
        single_glossaries
            .get(&selected)
            .cloned()
            .unwrap_or_else(|| {
                let fb = &config.selected_glossary_fallback;
                if fb.is_empty() || fb.starts_with("{selected-glossary") {
                    String::new()
                } else {
                    handlebar_to_value(app, config, fb, context, content, single_glossaries)
                }
            })
    };

    let definition =
        |category: dict::DictionaryCategory| first_glossary(app, Some(category), single_glossaries);
    let definition_fallback = |primary: dict::DictionaryCategory,
                               secondary: dict::DictionaryCategory| {
        let glossary = definition(primary);
        if glossary.is_empty() {
            definition(secondary)
        } else {
            glossary
        }
    };
    use dict::DictionaryCategory::{Bilingual, Monolingual};

    match handlebar {
        "{expression}" => get("expression"),
        "{reading}" => get("reading"),
        "{furigana-plain}" => get("furiganaPlain"),
        "{glossary}" => get("glossary"),
        "{glossary-brief}" => strip_glossary_headers(&get("glossary")),
        "{glossary-no-dictionary}" => strip_dictionary_name(&get("glossary")),
        "{glossary-first}" => first_glossary(app, None, single_glossaries),
        "{glossary-first-brief}" => {
            strip_glossary_headers(&first_glossary(app, None, single_glossaries))
        }
        "{glossary-first-no-dictionary}" => {
            strip_dictionary_name(&first_glossary(app, None, single_glossaries))
        }
        "{monolingual-definition}" => definition(Monolingual),
        "{monolingual-definition-brief}" => strip_glossary_headers(&definition(Monolingual)),
        "{monolingual-definition-no-dictionary}" => strip_dictionary_name(&definition(Monolingual)),
        "{bilingual-definition}" => definition(Bilingual),
        "{bilingual-definition-brief}" => strip_glossary_headers(&definition(Bilingual)),
        "{bilingual-definition-no-dictionary}" => strip_dictionary_name(&definition(Bilingual)),
        "{monolingual-definition-fallback}" => definition_fallback(Monolingual, Bilingual),
        "{monolingual-definition-fallback-brief}" => {
            strip_glossary_headers(&definition_fallback(Monolingual, Bilingual))
        }
        "{monolingual-definition-fallback-no-dictionary}" => {
            strip_dictionary_name(&definition_fallback(Monolingual, Bilingual))
        }
        "{bilingual-definition-fallback}" => definition_fallback(Bilingual, Monolingual),
        "{bilingual-definition-fallback-brief}" => {
            strip_glossary_headers(&definition_fallback(Bilingual, Monolingual))
        }
        "{bilingual-definition-fallback-no-dictionary}" => {
            strip_dictionary_name(&definition_fallback(Bilingual, Monolingual))
        }
        "{selected-glossary}" => selected_glossary(),
        "{selected-glossary-brief}" => strip_glossary_headers(&selected_glossary()),
        "{selected-glossary-no-dictionary}" => strip_dictionary_name(&selected_glossary()),
        "{frequencies}" => get("frequenciesHtml"),
        "{frequency-harmonic-rank}" => get("freqHarmonicRank"),
        "{pitch-accent-positions}" => get("pitchPositions"),
        "{pitch-accent-categories}" => get("pitchCategories"),
        "{pitch-accent-graphs}" => get("pitchAccentGraphs"),
        "{pitch-accent-graphs-first}" => first_pitch_accent_graph(&get("pitchAccentGraphs")),
        "{sentence}" => {
            let (p, b, s) = cloze_parts(&context.sentence, &get("matched"), context.cloze_offset);
            format!("{p}<b>{b}</b>{s}")
        }
        "{cloze-prefix}" => cloze_parts(&context.sentence, &get("matched"), context.cloze_offset).0,
        "{cloze-body}" => cloze_parts(&context.sentence, &get("matched"), context.cloze_offset).1,
        "{cloze-suffix}" => cloze_parts(&context.sentence, &get("matched"), context.cloze_offset).2,
        "{popup-selection-text}" => get("popupSelectionText"),
        "{document-title}" => context.document_title.clone().unwrap_or_default(),
        "{audio}" => get("audio"),
        _ => String::new(),
    }
}

fn parse_single_glossaries(content: &HashMap<String, String>) -> HashMap<String, String> {
    content
        .get("singleGlossaries")
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupAnkiConfig {
    use_anki_connect: bool,
    is_anki_connect_reachable: bool,
    allow_dupes: bool,
    disable_show_notes: bool,
    compact_glossaries_anki: bool,
    card_format_count: usize,
    valid_format_flags: Vec<bool>,
    card_format_icons: Vec<String>,
    excluded_dictionaries: Vec<String>,
    needs_audio: bool,
    audio_sources: Vec<AudioSource>,
    audio_enable_autoplay: bool,
}

#[tauri::command(async)]
pub fn anki_config(app: AppHandle, state: State<AnkiState>) -> PopupAnkiConfig {
    let config = load_config(&app);
    PopupAnkiConfig {
        use_anki_connect: true,
        is_anki_connect_reachable: state.reachable.load(Ordering::Relaxed),
        allow_dupes: config.allow_dupes,
        disable_show_notes: config.disable_show_notes,
        compact_glossaries_anki: config.compact_glossaries,
        card_format_count: if config.disabled {
            0
        } else {
            config.card_formats.len()
        },
        valid_format_flags: valid_format_flags(&config),
        card_format_icons: config.card_formats.iter().map(|f| f.icon.clone()).collect(),
        excluded_dictionaries: dict::term_dict_categories(&app)
            .into_iter()
            .filter(|(_, c)| *c == dict::DictionaryCategory::Exclude)
            .map(|(title, _)| title)
            .collect(),
        needs_audio: needs_audio(&config),
        audio_sources: config
            .audio_sources
            .iter()
            .filter(|s| s.is_enabled)
            .cloned()
            .collect(),
        audio_enable_autoplay: config.audio_enable_autoplay,
    }
}

pub async fn fetch_audio_source_list(app: &AppHandle, uri: &str) -> Result<Vec<u8>, String> {
    let query = uri.split_once('?').map(|(_, q)| q).unwrap_or("");
    let target = query
        .split('&')
        .find_map(|p| p.strip_prefix("url="))
        .ok_or("missing url")?;
    let target = urlencoding::decode(target)
        .map_err(|e| e.to_string())?
        .into_owned();
    if target.starts_with("local-audio://") {
        return Ok(local_audio::source_list(app, &target));
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(&target)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

#[tauri::command(async)]
pub fn anki_get_settings(app: AppHandle) -> AnkiConfig {
    load_config(&app)
}

#[tauri::command]
pub fn anki_reachable(state: State<AnkiState>) -> bool {
    state.reachable.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn anki_save_settings(app: AppHandle, config: AnkiConfig) {
    save_config(&app, &config);
}

#[tauri::command]
pub async fn anki_ping(app: AppHandle) -> bool {
    let config = load_config(&app);
    let reachable = !config.disabled
        && anki_request_timeout(&config, "version", Value::Null, 2)
            .await
            .is_ok();
    app.state::<AnkiState>()
        .reachable
        .store(reachable, Ordering::Relaxed);
    reachable
}

fn field_template(note_type: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match note_type {
        "Lapis" | "Kiku" => Some(vec![
            ("Expression", "{expression}"),
            ("ExpressionFurigana", "{furigana-plain}"),
            ("ExpressionReading", "{reading}"),
            ("ExpressionAudio", "{audio}"),
            ("SelectionText", "{popup-selection-text}"),
            ("MainDefinition", "{glossary-first}"),
            ("Sentence", "{sentence}"),
            ("SentenceAudio", "{sasayaki-audio}"),
            ("Picture", "{book-cover}"),
            ("Glossary", "{glossary}"),
            ("PitchPosition", "{pitch-accent-positions}"),
            ("PitchCategories", "{pitch-accent-categories}"),
            ("Frequency", "{frequencies}"),
            ("FreqSort", "{frequency-harmonic-rank}"),
            ("MiscInfo", "{document-title}"),
        ]),
        "Senren" => Some(vec![
            ("word", "{expression}"),
            ("reading", "{reading}"),
            (
                "sentence",
                "<span class=\"group\">{cloze-prefix}<span class=\"highlight\">{cloze-body}</span>{cloze-suffix}</span>",
            ),
            ("selectionText", "{popup-selection-text}"),
            ("definition", "{glossary-first}"),
            ("wordAudio", "{audio}"),
            ("sentenceAudio", "{sasayaki-audio}"),
            ("picture", "{book-cover}"),
            ("glossary", "{glossary}"),
            ("pitchPositions", "{pitch-accent-positions}"),
            ("pitchCategories", "{pitch-accent-categories}"),
            ("frequencies", "{frequencies}"),
            ("freqSort", "{frequency-harmonic-rank}"),
            ("miscInfo", "{document-title}"),
        ]),
        _ => None,
    }
}

fn autofill_field_mappings(config: &mut AnkiConfig, index: usize) {
    let format = &mut config.card_formats[index];
    let Some(note_type_name) = format.selected_note_type.as_deref() else {
        return;
    };
    let Some(template) = field_template(note_type_name) else {
        return;
    };
    let Some(note_type) = config
        .available_note_types
        .iter()
        .find(|n| n.name == note_type_name)
    else {
        return;
    };
    if note_type
        .fields
        .iter()
        .any(|f| format.field_mappings.contains_key(f))
    {
        return;
    }
    let template: HashMap<&str, &str> = template.into_iter().collect();
    for field in &note_type.fields {
        if let Some(mapping) = template.get(field.as_str()) {
            format
                .field_mappings
                .insert(field.clone(), mapping.to_string());
        }
    }
}

fn update_card_formats(config: &mut AnkiConfig) {
    if config.card_formats.is_empty() {
        config.card_formats.push(AnkiCardFormat {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            selected_deck: None,
            selected_note_type: None,
            field_mappings: HashMap::new(),
            tags: "hoshi".to_string(),
            icon: default_format_icon(),
        });
    }

    let default_deck = config
        .available_decks
        .iter()
        .find(|d| !d.eq_ignore_ascii_case("Default"))
        .or_else(|| config.available_decks.first())
        .cloned();
    let default_note_type = config.available_note_types.first().map(|n| n.name.clone());
    for index in 0..config.card_formats.len() {
        let deck_valid = config.card_formats[index]
            .selected_deck
            .as_ref()
            .is_some_and(|d| config.available_decks.contains(d));
        if !deck_valid {
            config.card_formats[index].selected_deck = default_deck.clone();
        }
        let model_valid = config.card_formats[index]
            .selected_note_type
            .as_ref()
            .is_some_and(|m| config.available_note_types.iter().any(|n| &n.name == m));
        if !model_valid {
            config.card_formats[index].selected_note_type = default_note_type.clone();
            config.card_formats[index].field_mappings.clear();
        }
        autofill_field_mappings(config, index);
    }
}

#[tauri::command]
pub fn anki_autofill_fields(app: AppHandle, format_id: String) -> AnkiConfig {
    let mut config = load_config(&app);
    if let Some(index) = config.card_formats.iter().position(|f| f.id == format_id) {
        autofill_field_mappings(&mut config, index);
        save_config(&app, &config);
    }
    config
}

async fn fetch_decks_and_note_types(
    config: &AnkiConfig,
) -> Result<(Vec<String>, Vec<NoteType>), String> {
    let decks: Vec<String> =
        serde_json::from_value(anki_request(config, "deckNames", Value::Null).await?)
            .map_err(|e| e.to_string())?;
    let models: Vec<String> =
        serde_json::from_value(anki_request(config, "modelNames", Value::Null).await?)
            .map_err(|e| e.to_string())?;

    let mut note_types = Vec::new();
    for model in models {
        let fields: Vec<String> = serde_json::from_value(
            anki_request(config, "modelFieldNames", json!({ "modelName": model })).await?,
        )
        .map_err(|e| e.to_string())?;
        note_types.push(NoteType {
            name: model,
            fields,
        });
    }
    Ok((decks, note_types))
}

#[tauri::command]
pub async fn anki_fetch(app: AppHandle, state: State<'_, AnkiState>) -> Result<AnkiConfig, String> {
    let mut config = load_config(&app);
    match fetch_decks_and_note_types(&config).await {
        Ok((decks, note_types)) => {
            config.available_decks = decks;
            config.available_note_types = note_types;
            state.reachable.store(true, Ordering::Relaxed);
            update_card_formats(&mut config);
            save_config(&app, &config);
            Ok(config)
        }
        Err(e) => {
            state.reachable.store(false, Ordering::Relaxed);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn anki_check_duplicates(app: AppHandle, fields: HashMap<String, String>) -> Vec<bool> {
    let config = load_config(&app);
    let mut results = vec![false; config.card_formats.len()];

    let mut notes: Vec<Value> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    for (i, format) in config.card_formats.iter().enumerate() {
        let (Some(deck), Some(model), Some(first)) = (
            format.selected_deck.clone(),
            format.selected_note_type.clone(),
            first_field(&config, format).cloned(),
        ) else {
            continue;
        };
        let word = format
            .field_mappings
            .get(&first)
            .and_then(|handlebar| fields.get(handlebar))
            .cloned()
            .unwrap_or_default();
        if word.is_empty() {
            continue;
        }
        notes.push(json!({
            "deckName": deck,
            "modelName": model,
            "fields": { first: word },
            "options": duplicate_options(&config, &deck),
        }));
        indices.push(i);
    }

    if notes.is_empty() {
        return results;
    }

    match anki_request(
        &config,
        "canAddNotesWithErrorDetail",
        json!({ "notes": notes }),
    )
    .await
    {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                for (idx, note_result) in indices.iter().zip(arr) {
                    if let Some(can_add) = note_result.get("canAdd").and_then(|v| v.as_bool()) {
                        results[*idx] = !can_add;
                    }
                }
            }
        }
        Err(_) => {
            anki_ping(app).await;
        }
    }
    results
}

#[tauri::command]
pub async fn anki_mine(
    app: AppHandle,
    content: HashMap<String, String>,
    context: MiningContext,
    slot_index: usize,
) -> bool {
    let config = load_config(&app);
    let Some(format) = config.card_formats.get(slot_index) else {
        return false;
    };
    let (Some(deck), Some(model)) = (
        format.selected_deck.clone(),
        format.selected_note_type.clone(),
    ) else {
        return false;
    };

    let single_glossaries = parse_single_glossaries(&content);

    let mut fields: HashMap<String, String> = HashMap::new();
    let mut audio_fields: Vec<String> = Vec::new();
    let mut sasayaki_audio_fields: Vec<String> = Vec::new();
    let mut picture_fields: Vec<String> = Vec::new();
    for (field, mapping) in &format.field_mappings {
        if mapping == "{audio}" {
            audio_fields.push(field.clone());
            continue;
        }
        if mapping == "{sasayaki-audio}" {
            sasayaki_audio_fields.push(field.clone());
            continue;
        }
        if mapping == "{book-cover}" {
            picture_fields.push(field.clone());
            continue;
        }
        let value = RE_HANDLEBAR
            .replace_all(mapping, |caps: &Captures| {
                handlebar_to_value(
                    &app,
                    &config,
                    &caps[0],
                    &context,
                    &content,
                    &single_glossaries,
                )
            })
            .into_owned();
        fields.insert(field.clone(), value);
    }

    let mut options = duplicate_options(&config, &deck);
    options["allowDuplicate"] = json!(config.allow_dupes);

    let mut note = json!({
        "deckName": deck,
        "modelName": model,
        "options": options,
    });

    let mut audio_media: Vec<Value> = Vec::new();

    if !audio_fields.is_empty()
        && let Some(url) = content.get("audio").filter(|u| !u.is_empty())
    {
        let bytes = if let Some((source, file)) = local_audio::file_params(url) {
            local_audio::audio_bytes(&app, &source, &file)
        } else {
            match reqwest::get(url).await {
                Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                Err(_) => None,
            }
        };
        if let Some(bytes) = bytes {
            let data = base64(&bytes);
            let filename = format!(
                "hoshi_audio_{}.{}",
                sha1_hex(&bytes),
                local_audio::audio_format(&bytes).0
            );
            audio_media.push(json!({ "data": data, "filename": filename, "fields": audio_fields }));
        }
    }

    if !sasayaki_audio_fields.is_empty()
        && let (Some(book_id), Some(cue_id)) = (&context.book_id, &context.sasayaki_cue)
    {
        let app_clone = app.clone();
        let book_id = book_id.clone();
        let cue_id = cue_id.clone();
        let sentence = context.sentence.clone();
        let bytes = tauri::async_runtime::spawn_blocking(move || {
            sasayaki::cue_sentence_audio(&app_clone, &book_id, &cue_id, &sentence)
        })
        .await
        .ok()
        .flatten();
        if let Some(bytes) = bytes {
            let data = base64(&bytes);
            let filename = format!("hoshi_sasayaki_{}.mp3", sha1_hex(&bytes));
            audio_media.push(
                json!({ "data": data, "filename": filename, "fields": sasayaki_audio_fields }),
            );
        }
    }

    if !audio_media.is_empty() {
        note["audio"] = Value::Array(audio_media);
    }

    if !picture_fields.is_empty() {
        let cover = context
            .book_id
            .as_deref()
            .and_then(|book_id| library::cover_path(&app, book_id))
            .and_then(|path| {
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                let bytes = fs::read(&path).ok()?;
                Some((path, ext, bytes))
            });
        if let Some((path, ext, bytes)) = cover {
            let filename = format!("hoshi_cover_{}.{ext}", sha1_hex(&bytes));
            if local_anki(&config) {
                let path = path.to_string_lossy();
                note["picture"] =
                    json!([{ "path": path, "filename": filename, "fields": picture_fields }]);
            } else {
                let stored = anki_request(
                    &config,
                    "getMediaFilesNames",
                    json!({ "pattern": filename }),
                )
                .await
                .ok()
                .and_then(|value| Some(!value.as_array()?.is_empty()))
                .unwrap_or(false);
                if stored {
                    let tag = format!("<img src=\"{filename}\">");
                    for field in &picture_fields {
                        fields.insert(field.clone(), tag.clone());
                    }
                } else {
                    let data = base64(&bytes);
                    note["picture"] =
                        json!([{ "data": data, "filename": filename, "fields": picture_fields }]);
                }
            }
        }
    }

    if let Some(media_json) = content.get("dictionaryMedia")
        && let Ok(media_list) = serde_json::from_str::<Vec<DictionaryMedia>>(media_json)
    {
        for media in media_list {
            if !fields.values().any(|value| value.contains(&media.filename)) {
                continue;
            }
            let bytes = dict::media_file(&app, &media.dictionary, &media.path);
            let ext = media.path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            let filename = format!("hoshi_dict_{}.{ext}", sha1_hex(&bytes));
            for value in fields.values_mut() {
                *value = value.replace(&media.filename, &filename);
            }
            let data = base64(&bytes);
            let _ = anki_request(
                &config,
                "storeMediaFile",
                json!({ "filename": filename, "data": data }),
            )
            .await;
        }
    }

    note["fields"] = json!(fields);

    let tags: Vec<String> = RE_HANDLEBAR
        .replace_all(&format.tags, |caps: &Captures| {
            handlebar_to_value(
                &app,
                &config,
                &caps[0],
                &context,
                &content,
                &single_glossaries,
            )
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
        })
        .split_whitespace()
        .map(String::from)
        .collect();
    if !tags.is_empty() {
        note["tags"] = json!(tags);
    }

    let ok = anki_request(&config, "addNote", json!({ "note": note }))
        .await
        .is_ok();
    if ok && config.force_sync {
        let _ = anki_request(&config, "sync", Value::Null).await;
    }
    ok
}

#[tauri::command]
pub async fn anki_show_notes(app: AppHandle, fields: HashMap<String, String>, slot_index: usize) {
    let config = load_config(&app);
    let Some(format) = config.card_formats.get(slot_index) else {
        return;
    };
    let (Some(deck), Some(model), Some(first)) = (
        format.selected_deck.clone(),
        format.selected_note_type.clone(),
        first_field(&config, format).cloned(),
    ) else {
        return;
    };
    let word = format
        .field_mappings
        .get(&first)
        .and_then(|handlebar| fields.get(handlebar))
        .cloned()
        .unwrap_or_default();
    if word.is_empty() {
        return;
    }
    let escaped = word.replace('"', "");

    let mut search = vec![format!("\"{first}:{escaped}\"")];
    if !config.check_all_models {
        search.push(format!("\"note:{model}\""));
    }
    match config.duplicate_scope {
        DuplicateScope::Collection => {}
        DuplicateScope::Deck => search.push(format!("\"deck:{deck}\"")),
        DuplicateScope::Deckroot => {
            let root = deck.split("::").next().unwrap_or(&deck);
            search.push(format!("\"deck:{root}\""));
        }
    }
    let _ = anki_request(&config, "guiBrowse", json!({ "query": search.join(" ") })).await;
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
