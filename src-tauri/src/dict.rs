use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hoshidicts::{Deinflector, LookupFrequencyOrder, LookupOptions, OwnedLookup, Query};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::http::{Request, Response, header::CONTENT_TYPE};
use tauri::{AppHandle, Emitter, Manager, State, UriSchemeContext, Wry};

use crate::library::{read_json, write_json};

const DICTIONARIES_DIR: &str = "Dictionaries";
const COLLAPSED_FILE: &str = "collapsed.json";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DictionaryCategory {
    #[default]
    None,
    Monolingual,
    Bilingual,
    Exclude,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct DictConfigEntry {
    file_name: String,
    #[serde(default = "default_enabled")]
    is_enabled: bool,
    #[serde(default)]
    order: i64,
    #[serde(default)]
    category: DictionaryCategory,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct DictConfig {
    term_dictionaries: Vec<DictConfigEntry>,
    frequency_dictionaries: Vec<DictConfigEntry>,
    pitch_dictionaries: Vec<DictConfigEntry>,
    kanji_dictionaries: Vec<DictConfigEntry>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct DictionaryIndex {
    title: String,
    revision: String,
    is_updatable: Option<bool>,
    index_url: Option<String>,
    download_url: Option<String>,
}

impl DictionaryIndex {
    fn update_urls(&self) -> Option<(&str, &str)> {
        let index_url = self.index_url.as_deref().unwrap_or_default();
        let download_url = self.download_url.as_deref().unwrap_or_default();
        if self.is_updatable == Some(true) && !index_url.is_empty() && !download_url.is_empty() {
            Some((index_url, download_url))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictionaryType {
    Term,
    Frequency,
    Pitch,
    Kanji,
}

impl DictionaryType {
    fn all() -> [Self; 4] {
        [Self::Term, Self::Frequency, Self::Pitch, Self::Kanji]
    }

    fn directory(self) -> &'static str {
        match self {
            Self::Term => "Term",
            Self::Frequency => "Frequency",
            Self::Pitch => "Pitch",
            Self::Kanji => "Kanji",
        }
    }
}

fn default_enabled() -> bool {
    true
}

pub(crate) fn dictionaries_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap().join(DICTIONARIES_DIR)
}

pub(crate) fn initialize(app: &AppHandle) {
    let root = dictionaries_dir(app);
    fs::create_dir_all(&root).ok();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<LookupState>().with_engine(&app, |_| ());
    });
}

fn load_config(app: &AppHandle) -> DictConfig {
    let root = dictionaries_dir(app);
    let mut config = read_json(&root.join("config.json")).unwrap_or_default();
    append_unconfigured(&root, &mut config);
    update_orders(&mut config);
    config
}

fn append_unconfigured(root: &Path, config: &mut DictConfig) {
    for kind in DictionaryType::all() {
        let stored: Vec<String> = fs::read_dir(root.join(kind.directory()))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir() && path.join("index.json").is_file())
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .collect();
        let entries = entries_for_mut(config, kind);
        let configured: HashSet<String> = entries
            .iter()
            .map(|entry| entry.file_name.clone())
            .collect();
        let mut order = next_order(entries);
        for file_name in stored {
            if configured.contains(&file_name) {
                continue;
            }
            entries.push(DictConfigEntry {
                file_name,
                is_enabled: true,
                order,
                category: DictionaryCategory::None,
            });
            order += 1;
        }
    }
}

fn save_config(app: &AppHandle, config: &DictConfig) -> Result<(), String> {
    let root = dictionaries_dir(app);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    write_json(&root.join("config.json"), config)
}

#[tauri::command]
pub fn load_collapsed_dictionaries(app: AppHandle) -> Vec<String> {
    read_json::<Vec<String>>(&dictionaries_dir(&app).join(COLLAPSED_FILE)).unwrap_or_default()
}

#[tauri::command]
pub fn save_collapsed_dictionaries(app: AppHandle, titles: Vec<String>) {
    let mut collapsed = Vec::new();
    for title in titles {
        if !collapsed.contains(&title) {
            collapsed.push(title);
        }
    }
    let root = dictionaries_dir(&app);
    if fs::create_dir_all(&root).is_err() {
        return;
    }
    let path = root.join(COLLAPSED_FILE);
    write_json(&path, &collapsed).ok();
}

fn update_orders(config: &mut DictConfig) {
    for kind in DictionaryType::all() {
        update_order(entries_for_mut(config, kind));
    }
}

fn update_order(entries: &mut [DictConfigEntry]) {
    entries.sort_by_key(|entry| entry.order);
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.order = index as i64;
    }
}

fn entries_for(config: &DictConfig, kind: DictionaryType) -> &[DictConfigEntry] {
    match kind {
        DictionaryType::Term => &config.term_dictionaries,
        DictionaryType::Frequency => &config.frequency_dictionaries,
        DictionaryType::Pitch => &config.pitch_dictionaries,
        DictionaryType::Kanji => &config.kanji_dictionaries,
    }
}

fn entries_for_mut(config: &mut DictConfig, kind: DictionaryType) -> &mut Vec<DictConfigEntry> {
    match kind {
        DictionaryType::Term => &mut config.term_dictionaries,
        DictionaryType::Frequency => &mut config.frequency_dictionaries,
        DictionaryType::Pitch => &mut config.pitch_dictionaries,
        DictionaryType::Kanji => &mut config.kanji_dictionaries,
    }
}

fn read_index(path: &Path) -> DictionaryIndex {
    read_json(&path.join("index.json")).unwrap_or_default()
}

fn dictionary_path(app: &AppHandle, kind: DictionaryType, file_name: &str) -> PathBuf {
    dictionaries_dir(app).join(kind.directory()).join(file_name)
}

fn title_for_entry(app: &AppHandle, kind: DictionaryType, entry: &DictConfigEntry) -> String {
    let index = read_index(&dictionary_path(app, kind, &entry.file_name));
    if index.title.is_empty() {
        entry.file_name.clone()
    } else {
        index.title
    }
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let entries = fs::read_dir(source).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

struct Engine {
    lookup: OwnedLookup,
    styles: HashMap<String, String>,
}

impl Engine {
    fn build(app: &AppHandle, config: &DictConfig) -> Self {
        let mut query = Query::new();
        for kind in DictionaryType::all() {
            for entry in entries_for(config, kind) {
                if !entry.is_enabled {
                    continue;
                }
                let path = dictionary_path(app, kind, &entry.file_name);
                match kind {
                    DictionaryType::Term => {
                        let _ = query.add_term_dict(path);
                    }
                    DictionaryType::Frequency => {
                        let _ = query.add_freq_dict(path);
                    }
                    DictionaryType::Pitch => {
                        let _ = query.add_pitch_dict(path);
                    }
                    DictionaryType::Kanji => {
                        let _ = query.add_kanji_dict(path);
                    }
                }
            }
        }
        let styles = query
            .styles()
            .ok()
            .map(|styles| {
                styles
                    .styles()
                    .iter()
                    .map(|style| (style.dict_name().to_string(), style.styles().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            lookup: OwnedLookup::new(query, Deinflector::new()),
            styles,
        }
    }
}

#[derive(Default)]
pub struct LookupState(Mutex<Option<Engine>>);

impl LookupState {
    pub(crate) fn lock_for_update(&self) -> impl Drop + '_ {
        let mut guard = self.0.lock().unwrap();
        *guard = None;
        guard
    }

    fn with_engine<T>(&self, app: &AppHandle, use_engine: impl FnOnce(&Engine) -> T) -> T {
        let mut guard = self.0.lock().unwrap();
        use_engine(guard.get_or_insert_with(|| Engine::build(app, &load_config(app))))
    }
}

pub(crate) fn invalidate(state: &State<LookupState>) {
    *state.0.lock().unwrap() = None;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    name: String,
    description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Glossary {
    dictionary: String,
    content: String,
    definition_tags: String,
    term_tags: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Freq {
    value: i32,
    display_value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreqGroup {
    dictionary: String,
    frequencies: Vec<Freq>,
}

#[derive(Serialize, PartialEq)]
#[serde(untagged)]
enum PitchPosition {
    Pattern(String),
    Downstep(i32),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PitchAccent {
    position: PitchPosition,
    nasal: Vec<i32>,
    devoice: Vec<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PitchGroup {
    dictionary: String,
    pitches: Vec<PitchAccent>,
    transcriptions: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    expression: String,
    reading: String,
    matched: String,
    deinflection_trace: Vec<Trace>,
    glossaries: Vec<Glossary>,
    frequencies: Vec<FreqGroup>,
    pitches: Vec<PitchGroup>,
    rules: Vec<String>,
}

#[derive(Serialize)]
pub struct LookupResponse {
    entries: Vec<Entry>,
    styles: HashMap<String, String>,
}

fn build_term(
    term: &hoshidicts::Term,
) -> (Vec<Glossary>, Vec<FreqGroup>, Vec<PitchGroup>, Vec<String>) {
    let glossaries = term
        .glossaries()
        .iter()
        .map(|glossary| Glossary {
            dictionary: glossary.dict_name().to_string(),
            content: glossary.glossary().to_string(),
            definition_tags: glossary.definition_tags().to_string(),
            term_tags: glossary.term_tags().to_string(),
        })
        .collect();

    let frequencies = term
        .frequencies()
        .iter()
        .map(|group| FreqGroup {
            dictionary: group.dict_name().to_string(),
            frequencies: group
                .frequencies()
                .iter()
                .map(|frequency| Freq {
                    value: frequency.value(),
                    display_value: frequency.display_value().to_string(),
                })
                .collect(),
        })
        .collect();

    let pitches = term
        .pitches()
        .iter()
        .map(|group| {
            let mut accents: Vec<PitchAccent> = Vec::new();
            for pitch in group.pitches() {
                let pattern = pitch.pattern();
                let position = if pattern.is_empty() {
                    PitchPosition::Downstep(pitch.position())
                } else {
                    PitchPosition::Pattern(pattern.to_string())
                };
                if accents.iter().any(|accent| accent.position == position) {
                    continue;
                }
                accents.push(PitchAccent {
                    position,
                    nasal: pitch.nasal().to_vec(),
                    devoice: pitch.devoice().to_vec(),
                });
            }
            let mut transcriptions: Vec<String> = Vec::new();
            for transcription in group.transcriptions() {
                if !transcriptions.iter().any(|t| t == transcription) {
                    transcriptions.push(transcription.to_string());
                }
            }
            PitchGroup {
                dictionary: group.dict_name().to_string(),
                pitches: accents,
                transcriptions,
            }
        })
        .collect();

    let rules = term
        .rules()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    (glossaries, frequencies, pitches, rules)
}

#[derive(Deserialize)]
pub enum FrequencySortOrder {
    Auto,
    Ascending,
    Descending,
    Disabled,
}

#[tauri::command(async)]
pub fn lookup(
    app: AppHandle,
    state: State<LookupState>,
    text: String,
    max_results: i32,
    scan_length: usize,
    frequency_sort_order: FrequencySortOrder,
    frequency_sort_dictionary: String,
) -> LookupResponse {
    state.with_engine(&app, |engine| {
        let styles = engine.styles.clone();
        let mut entries = Vec::new();
        let frequency_order = match frequency_sort_order {
            FrequencySortOrder::Auto => LookupFrequencyOrder::Auto,
            FrequencySortOrder::Ascending => LookupFrequencyOrder::Ascending,
            FrequencySortOrder::Descending => LookupFrequencyOrder::Descending,
            FrequencySortOrder::Disabled => LookupFrequencyOrder::Disabled,
        };
        let frequency_dictionary = (matches!(
            frequency_order,
            LookupFrequencyOrder::Ascending | LookupFrequencyOrder::Descending
        ) && !frequency_sort_dictionary.is_empty())
        .then_some(frequency_sort_dictionary.as_str());
        let options = LookupOptions {
            frequency_order,
            frequency_dictionary,
            ..Default::default()
        };
        let Ok(results) = engine
            .lookup
            .run_with_options(&text, max_results, scan_length, &options)
        else {
            return LookupResponse { entries, styles };
        };
        for result in results.results() {
            let term = result.term();
            let (glossaries, frequencies, pitches, rules) = build_term(term);
            let mut deinflection_trace: Vec<Trace> = result
                .trace()
                .iter()
                .map(|trace| Trace {
                    name: trace.name().to_string(),
                    description: trace.description().to_string(),
                })
                .collect();
            deinflection_trace.reverse();
            entries.push(Entry {
                expression: term.expression().to_string(),
                reading: term.reading().to_string(),
                matched: result.matched().to_string(),
                deinflection_trace,
                glossaries,
                frequencies,
                pitches,
                rules,
            });
        }
        LookupResponse { entries, styles }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanjiEntry {
    dict_name: String,
    onyomi: String,
    kunyomi: String,
    meanings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanjiResponse {
    character: String,
    entries: Vec<KanjiEntry>,
}

#[tauri::command(async)]
pub fn lookup_kanji(
    app: AppHandle,
    state: State<LookupState>,
    character: String,
) -> Option<KanjiResponse> {
    state.with_engine(&app, |engine| {
        let results = engine.lookup.query().run_kanji(&character).ok()?;
        let entries: Vec<KanjiEntry> = results
            .entries()
            .iter()
            .map(|entry| KanjiEntry {
                dict_name: entry.dict_name().to_string(),
                onyomi: entry.onyomi().to_string(),
                kunyomi: entry.kunyomi().to_string(),
                meanings: entry.definitions().map(str::to_string).collect(),
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        Some(KanjiResponse { character, entries })
    })
}

pub(crate) fn term_dict_categories(app: &AppHandle) -> Vec<(String, DictionaryCategory)> {
    let config = load_config(app);
    entries_for(&config, DictionaryType::Term)
        .iter()
        .map(|entry| {
            (
                title_for_entry(app, DictionaryType::Term, entry),
                entry.category,
            )
        })
        .collect()
}

pub(crate) fn media_file(app: &AppHandle, dictionary: &str, path: &str) -> Vec<u8> {
    let state: State<LookupState> = app.state();
    state.with_engine(app, |engine| {
        engine
            .lookup
            .query()
            .media_file(dictionary, path)
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    })
}

fn media_mime(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("heic") => "image/heic",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

pub fn image_protocol(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let query = request.uri().query().unwrap_or("");
    let param = |key: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{key}=")))
            .and_then(|value| urlencoding::decode(value).ok())
            .map(|value| value.into_owned())
            .unwrap_or_default()
    };
    let path = param("path");
    let bytes = media_file(ctx.app_handle(), &param("dictionary"), &path);
    if bytes.is_empty() {
        return Response::builder().status(404).body(Vec::new()).unwrap();
    }
    Response::builder()
        .header(CONTENT_TYPE, media_mime(&path))
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .unwrap()
}

#[derive(Serialize, Default)]
pub struct ImportSummary {
    imported: Vec<String>,
    failed: Vec<String>,
}

fn has_kind(result: &hoshidicts::Import, kind: DictionaryType) -> bool {
    match kind {
        DictionaryType::Term => result.terms > 0,
        DictionaryType::Frequency => result.freq > 0,
        DictionaryType::Pitch => result.pitch > 0,
        DictionaryType::Kanji => result.kanji > 0,
    }
}

fn remove_entries_for_title(
    app: &AppHandle,
    config: &mut DictConfig,
    title: &str,
) -> Vec<(DictionaryType, DictConfigEntry)> {
    let mut matches = Vec::new();
    for kind in DictionaryType::all() {
        entries_for_mut(config, kind).retain(|entry| {
            let keep = title_for_entry(app, kind, entry) != title;
            if !keep {
                matches.push((kind, entry.clone()));
            }
            keep
        });
    }
    matches
}

fn next_order(entries: &[DictConfigEntry]) -> i64 {
    entries.iter().map(|entry| entry.order).max().unwrap_or(-1) + 1
}

fn add_import_entry(
    config: &mut DictConfig,
    kind: DictionaryType,
    file_name: String,
    previous: Option<&DictConfigEntry>,
) {
    let entries = entries_for_mut(config, kind);
    let (is_enabled, order, category) = previous
        .map(|entry| (entry.is_enabled, entry.order, entry.category))
        .unwrap_or((true, next_order(entries), DictionaryCategory::None));
    entries.push(DictConfigEntry {
        file_name,
        is_enabled,
        order,
        category,
    });
}

fn replace_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        fs::remove_dir_all(destination).map_err(|error| error.to_string())?;
    }
    copy_directory(source, destination)
}

#[tauri::command(async)]
pub fn import_dictionaries(
    app: AppHandle,
    state: State<LookupState>,
    paths: Vec<String>,
) -> ImportSummary {
    let root = dictionaries_dir(&app);
    fs::create_dir_all(&root).unwrap();
    for kind in DictionaryType::all() {
        fs::create_dir_all(root.join(kind.directory())).unwrap();
    }
    let mut summary = ImportSummary::default();

    for path in paths {
        let file_name = Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let temporary = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let imported = hoshidicts::import(Path::new(&path), &temporary, false);
        let result = match imported {
            Ok(result) => result,
            Err(error) => {
                summary.failed.push(format!("{file_name}: {error}"));
                fs::remove_dir_all(&temporary).ok();
                continue;
            }
        };
        let title = result.title.clone();
        let imported_dir = temporary.join(&title);
        let _engine = state.lock_for_update();
        let mut config = load_config(&app);
        let previous = remove_entries_for_title(&app, &mut config, &title);
        let folder = previous
            .first()
            .map_or(title.as_str(), |(_, entry)| entry.file_name.as_str());
        let mut import_ok = true;
        let mut kinds = Vec::new();
        for kind in DictionaryType::all() {
            if !has_kind(&result, kind) {
                continue;
            }
            let destination = root.join(kind.directory()).join(folder);
            if replace_directory(&imported_dir, &destination).is_err() {
                import_ok = false;
                break;
            }
            kinds.push(kind);
        }
        fs::remove_dir_all(&temporary).ok();
        if !import_ok || kinds.is_empty() {
            summary
                .failed
                .push(format!("{file_name}: could not store imported dictionary"));
            continue;
        }

        for kind in &kinds {
            let old = previous.iter().find(|(old_kind, _)| old_kind == kind);
            add_import_entry(
                &mut config,
                *kind,
                folder.to_string(),
                old.map(|(_, entry)| entry),
            );
        }
        update_orders(&mut config);
        if let Err(error) = save_config(&app, &config) {
            summary.failed.push(format!("{file_name}: {error}"));
            continue;
        }
        summary.imported.push(title);
    }

    summary
}

struct UpdatableDictionary {
    title: String,
    revision: String,
    index_url: String,
    folders: Vec<(DictionaryType, String)>,
}

fn updatable_dictionaries(app: &AppHandle, config: &DictConfig) -> Vec<UpdatableDictionary> {
    let mut dictionaries: Vec<UpdatableDictionary> = Vec::new();
    for kind in DictionaryType::all() {
        for entry in entries_for(config, kind) {
            let index = read_index(&dictionary_path(app, kind, &entry.file_name));
            let Some((index_url, _)) = index.update_urls() else {
                continue;
            };
            let title = if index.title.is_empty() {
                entry.file_name.clone()
            } else {
                index.title.clone()
            };
            match dictionaries.iter_mut().find(|item| item.title == title) {
                Some(existing) => existing.folders.push((kind, entry.file_name.clone())),
                None => dictionaries.push(UpdatableDictionary {
                    title,
                    revision: index.revision.clone(),
                    index_url: index_url.to_string(),
                    folders: vec![(kind, entry.file_name.clone())],
                }),
            }
        }
    }
    dictionaries
}

fn rename_collapsed_dictionary(app: &AppHandle, old: &str, new: &str) {
    let path = dictionaries_dir(app).join(COLLAPSED_FILE);
    let Some(mut titles) = read_json::<Vec<String>>(&path) else {
        return;
    };
    let Some(index) = titles.iter().position(|title| title == old) else {
        return;
    };
    titles[index] = new.to_string();
    write_json(&path, &titles).ok();
}

fn install_updated_dictionary(
    app: &AppHandle,
    imported: &Path,
    folders: &[(DictionaryType, String)],
    new: &str,
) -> Result<(), String> {
    let root = dictionaries_dir(app);
    for (kind, folder) in folders {
        if folder != new && root.join(kind.directory()).join(new).exists() {
            return Err(format!(
                "cannot rename to {new}, a dictionary with that title already exists"
            ));
        }
    }
    let mut written: Vec<PathBuf> = Vec::new();
    for (kind, folder) in folders {
        let destination = root.join(kind.directory()).join(new);
        if let Err(error) = replace_directory(imported, &destination) {
            for path in &written {
                fs::remove_dir_all(path).ok();
            }
            return Err(error);
        }
        if folder != new {
            written.push(destination);
        }
    }
    for (kind, folder) in folders {
        if folder != new {
            fs::remove_dir_all(root.join(kind.directory()).join(folder)).ok();
        }
    }
    Ok(())
}

fn rename_dictionary_entries(
    config: &mut DictConfig,
    folders: &[(DictionaryType, String)],
    new: &str,
) {
    for (kind, folder) in folders {
        if let Some(entry) = entries_for_mut(config, *kind)
            .iter_mut()
            .find(|entry| &entry.file_name == folder)
        {
            entry.file_name = new.to_string();
        }
    }
    update_orders(config);
}

async fn fetch_remote_index(
    client: &reqwest::Client,
    url: &str,
) -> Result<DictionaryIndex, String> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?
        .json::<DictionaryIndex>()
        .await
        .map_err(|error| error.to_string())
}

async fn download_archive(client: &reqwest::Client, url: &str) -> Result<PathBuf, String> {
    let bytes = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?
        .bytes()
        .await
        .map_err(|error| error.to_string())?;
    let path = std::env::temp_dir().join(format!("{}.zip", uuid::Uuid::new_v4()));
    fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSummary {
    updated: Vec<String>,
    failed: Vec<String>,
    renamed: Vec<(String, String)>,
}

#[tauri::command]
pub async fn update_dictionaries(
    app: AppHandle,
    state: State<'_, LookupState>,
    low_ram: bool,
) -> Result<UpdateSummary, String> {
    let dictionaries = updatable_dictionaries(&app, &load_config(&app));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| error.to_string())?;
    let mut summary = UpdateSummary::default();

    for dictionary in &dictionaries {
        let progress = |status: String| {
            app.emit("dictionary-update-progress", json!({ "status": status }))
                .ok();
        };
        progress(format!("Checking {}", dictionary.title));

        let remote = match fetch_remote_index(&client, &dictionary.index_url).await {
            Ok(remote) => remote,
            Err(error) => {
                summary
                    .failed
                    .push(format!("{}: {error}", dictionary.title));
                continue;
            }
        };
        if remote.revision == dictionary.revision {
            continue;
        }
        let remote_title = if remote.title.is_empty() {
            dictionary.title.clone()
        } else {
            remote.title.clone()
        };

        progress(format!("Downloading {remote_title}"));
        let download_url = remote.download_url.as_deref().unwrap_or_default();
        let archive = match download_archive(&client, download_url).await {
            Ok(archive) => archive,
            Err(error) => {
                summary
                    .failed
                    .push(format!("{}: {error}", dictionary.title));
                continue;
            }
        };

        progress(format!("Importing {remote_title}"));
        let output = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let imported = hoshidicts::import(&archive, &output, low_ram);
        fs::remove_file(&archive).ok();
        let result = match imported {
            Ok(result) => result,
            Err(error) => {
                summary
                    .failed
                    .push(format!("{}: {error}", dictionary.title));
                fs::remove_dir_all(&output).ok();
                continue;
            }
        };
        let new = result.title.clone();
        let imported_dir = output.join(&new);
        let _engine = state.lock_for_update();
        let mut config = load_config(&app);
        let installed = install_updated_dictionary(&app, &imported_dir, &dictionary.folders, &new);
        fs::remove_dir_all(&output).ok();
        if let Err(error) = installed {
            summary
                .failed
                .push(format!("{}: {error}", dictionary.title));
            continue;
        }

        if new != dictionary.title {
            rename_dictionary_entries(&mut config, &dictionary.folders, &new);
            if let Err(error) = save_config(&app, &config) {
                summary
                    .failed
                    .push(format!("{}: {error}", dictionary.title));
                continue;
            }
            rename_collapsed_dictionary(&app, &dictionary.title, &new);
            crate::anki::update_handlebar(&app, &dictionary.title, &new);
            summary
                .renamed
                .push((dictionary.title.clone(), new.clone()));
        }
        summary.updated.push(new);
    }

    Ok(summary)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DictionaryInfo {
    title: String,
    revision: String,
    file_name: String,
    is_enabled: bool,
    order: i64,
    category: DictionaryCategory,
    is_updatable: bool,
}

#[derive(Serialize, Default)]
pub struct DictionaryLists {
    term: Vec<DictionaryInfo>,
    frequency: Vec<DictionaryInfo>,
    pitch: Vec<DictionaryInfo>,
    kanji: Vec<DictionaryInfo>,
}

#[tauri::command(async)]
pub fn list_dictionaries(app: AppHandle) -> DictionaryLists {
    let config = load_config(&app);
    let list = |kind: DictionaryType| {
        entries_for(&config, kind)
            .iter()
            .map(|entry| {
                let index = read_index(&dictionary_path(&app, kind, &entry.file_name));
                DictionaryInfo {
                    title: if index.title.is_empty() {
                        entry.file_name.clone()
                    } else {
                        index.title.clone()
                    },
                    is_updatable: index.update_urls().is_some(),
                    revision: index.revision,
                    file_name: entry.file_name.clone(),
                    is_enabled: entry.is_enabled,
                    order: entry.order,
                    category: entry.category,
                }
            })
            .collect()
    };
    DictionaryLists {
        term: list(DictionaryType::Term),
        frequency: list(DictionaryType::Frequency),
        pitch: list(DictionaryType::Pitch),
        kanji: list(DictionaryType::Kanji),
    }
}

#[tauri::command]
pub fn reorder_dictionaries(
    app: AppHandle,
    state: State<LookupState>,
    kind: DictionaryType,
    file_names: Vec<String>,
) {
    let mut config = load_config(&app);
    let entries = entries_for_mut(&mut config, kind);
    for (index, name) in file_names.iter().enumerate() {
        if let Some(entry) = entries.iter_mut().find(|entry| &entry.file_name == name) {
            entry.order = index as i64;
        }
    }
    update_orders(&mut config);
    save_config(&app, &config).unwrap();
    invalidate(&state);
}

#[tauri::command]
pub fn set_dictionary_category(app: AppHandle, file_name: String, category: DictionaryCategory) {
    let mut config = load_config(&app);
    if let Some(entry) = config
        .term_dictionaries
        .iter_mut()
        .find(|entry| entry.file_name == file_name)
    {
        entry.category = category;
    }
    save_config(&app, &config).unwrap();
}

#[tauri::command]
pub fn set_dictionary_enabled(
    app: AppHandle,
    state: State<LookupState>,
    kind: DictionaryType,
    file_name: String,
    enabled: bool,
) {
    let mut config = load_config(&app);
    if let Some(entry) = entries_for_mut(&mut config, kind)
        .iter_mut()
        .find(|entry| entry.file_name == file_name)
    {
        entry.is_enabled = enabled;
    }
    save_config(&app, &config).unwrap();
    invalidate(&state);
}

#[tauri::command]
pub fn delete_dictionary(
    app: AppHandle,
    state: State<LookupState>,
    kind: DictionaryType,
    file_name: String,
) {
    let _engine = state.lock_for_update();
    let mut config = load_config(&app);
    entries_for_mut(&mut config, kind).retain(|entry| entry.file_name != file_name);
    update_orders(&mut config);
    save_config(&app, &config).unwrap();
    for candidate_kind in DictionaryType::all() {
        if !entries_for(&config, candidate_kind)
            .iter()
            .any(|entry| entry.file_name == file_name)
        {
            let path = dictionary_path(&app, candidate_kind, &file_name);
            fs::remove_dir_all(path).ok();
        }
    }
}
