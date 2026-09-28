use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rbook::Epub;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::http::{Request, Response, header::CONTENT_TYPE};

use crate::library;

const MATCH_FILE: &str = "sasayaki_match.json";
const PLAYBACK_FILE: &str = "sasayaki_playback.json";
const SEARCH_WINDOW: usize = 200;
const MAX_MISSES: usize = 4;
const CHUNK_SIZE: u64 = 4 * 1024 * 1024;

struct SasayakiCue {
    id: String,
    start_time: f64,
    end_time: f64,
    text: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SasayakiMatch {
    pub id: String,
    pub start_time: f64,
    pub end_time: f64,
    pub text: String,
    pub chapter_index: usize,
    pub start: usize,
    pub length: usize,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SasayakiImage {
    pub chapter_index: usize,
    pub image_index: usize,
    pub offset: usize,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SasayakiMatchData {
    pub matches: Vec<SasayakiMatch>,
    pub unmatched: usize,
    #[serde(default)]
    pub images: Vec<SasayakiImage>,
}

#[derive(Serialize)]
pub struct AudioChapter {
    start: f64,
    title: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct SasayakiPlayback {
    pub last_position: f64,
    pub delay: f64,
    pub rate: f32,
    pub volume: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
}

impl Default for SasayakiPlayback {
    fn default() -> Self {
        SasayakiPlayback {
            last_position: 0.0,
            delay: 0.0,
            rate: 1.0,
            volume: 1.0,
            audio_path: None,
            modified: None,
        }
    }
}

fn parse_timestamp(timestamp: &str) -> f64 {
    let normalized = timestamp.replace(',', ".");
    let parts: Vec<&str> = normalized.trim().split(':').collect();
    let value = |index: usize| {
        parts
            .get(index)
            .and_then(|p| p.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    value(0) * 3600.0 + value(1) * 60.0 + value(2)
}

fn parse_cues(data: &[u8]) -> Vec<SasayakiCue> {
    String::from_utf8_lossy(data)
        .replace("\r\n", "\n")
        .split("\n\n")
        .enumerate()
        .filter_map(|(index, block)| {
            let lines: Vec<&str> = block.split('\n').collect();
            if lines.len() < 3 {
                return None;
            }
            let (start, end) = lines[1].split_once("-->")?;
            Some(SasayakiCue {
                id: index.to_string(),
                start_time: parse_timestamp(start),
                end_time: parse_timestamp(end),
                text: lines[2].trim_matches([' ', '\t']).to_string(),
            })
        })
        .collect()
}

fn find_text(source: &[char], text: &[char], start: usize, end: usize) -> Option<usize> {
    if end < text.len() {
        return None;
    }
    (start..=(end - text.len())).find(|&index| &source[index..index + text.len()] == text)
}

fn find_unique(source: &[char], text: &[char], start: usize) -> Option<usize> {
    let index = find_text(source, text, start, source.len())?;
    find_text(source, text, index + 1, source.len())
        .is_none()
        .then_some(index)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    chapter_index: usize,
    start: usize,
    length: usize,
}

impl Chapter {
    fn end(&self) -> usize {
        self.start + self.length
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    text: Vec<char>,
    sentence_ends: Vec<bool>,
    segment_ends: Vec<bool>,
    chapters: Vec<Chapter>,
    images: Vec<SasayakiImage>,
}

static RE_CHARACTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{}]", library::CHARACTER_CLASS)).unwrap());
static RE_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</?(?:p|div|h[1-6]|li|ul|ol|blockquote|section|article|table|tr|td|th|hr|br|img|image)\b[^>]*>").unwrap()
});
const SENTENCE_ENDERS: [char; 11] = [
    '。', '！', '？', '!', '?', '…', '」', '』', '「', '『', '（',
];
const SEGMENT_BREAKS: [char; 4] = ['、', ',', '，', '\u{2500}'];
const BLOCK_BREAK: char = '\u{1}';

fn flatten(html: &str) -> (Vec<char>, Vec<bool>, Vec<bool>) {
    let markup = library::strip(&RE_BLOCK.replace_all(library::body(html), "\u{1}"));
    let matches: Vec<_> = RE_CHARACTER.find_iter(&markup).collect();
    let mut text = Vec::with_capacity(matches.len());
    let mut sentence_ends = Vec::with_capacity(matches.len());
    let mut segment_ends = Vec::with_capacity(matches.len());
    for (index, found) in matches.iter().enumerate() {
        text.push(found.as_str().chars().next().unwrap());
        let gap_end = matches
            .get(index + 1)
            .map_or(markup.len(), |next| next.start());
        let gap = &markup[found.end()..gap_end];
        let sentence_end = gap
            .chars()
            .any(|c| SENTENCE_ENDERS.contains(&c) || c == BLOCK_BREAK);
        sentence_ends.push(sentence_end);
        segment_ends.push(!sentence_end && gap.chars().any(|c| SEGMENT_BREAKS.contains(&c)));
    }
    (text, sentence_ends, segment_ends)
}

pub fn build_source(app: &AppHandle, id: &str) -> Result<Source, String> {
    let path =
        library::book_epub_path(app, id).ok_or_else(|| format!("Book {id} was not found"))?;
    let epub = Epub::open(path).map_err(|error| error.to_string())?;

    let guide_toc: HashSet<String> = epub
        .toc()
        .landmarks()
        .iter()
        .flat_map(|landmarks| landmarks.flatten())
        .filter(|entry| {
            entry
                .kind_raw()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("toc"))
        })
        .filter_map(|entry| {
            entry.manifest_entry().and_then(|item| {
                item.resource()
                    .key()
                    .value()
                    .map(library::normalize_resource_key)
            })
        })
        .collect();

    let mut source = Source {
        text: Vec::new(),
        sentence_ends: Vec::new(),
        segment_ends: Vec::new(),
        chapters: Vec::new(),
        images: Vec::new(),
    };
    for (spine_index, entry) in epub.spine().iter().enumerate() {
        if !entry.is_linear() {
            continue;
        }
        let Some(item) = entry.manifest_entry() else {
            continue;
        };
        if item.properties().as_str().contains("nav") {
            continue;
        }
        let key = item
            .resource()
            .key()
            .value()
            .map(library::normalize_resource_key);
        if key.is_some_and(|key| {
            let path = key.to_lowercase();
            guide_toc.contains(&key)
                || path.contains("toc")
                || path.contains("caution")
                || path.contains("colophon")
        }) {
            continue;
        }
        let Ok(content) = item.read_str() else {
            continue;
        };

        let body = library::body(&content);
        for (image_index, offset) in library::image_starts(body).enumerate() {
            source.images.push(SasayakiImage {
                chapter_index: spine_index,
                image_index,
                offset: library::count_chars(&body[..offset]),
            });
        }

        let (text, sentence_ends, segment_ends) = flatten(&content);
        source.chapters.push(Chapter {
            chapter_index: spine_index,
            start: source.text.len(),
            length: text.len(),
        });
        source.text.extend(text);
        source.sentence_ends.extend(sentence_ends);
        source.segment_ends.extend(segment_ends);
    }
    Ok(source)
}

fn match_cues(source: Source, cues: &[SasayakiCue]) -> SasayakiMatchData {
    let mut candidates = Vec::new();
    for cue in cues.iter().take(15) {
        if cue.text.starts_with('＊') {
            continue;
        }
        let text: Vec<char> = library::filtered(&cue.text).chars().collect();
        if text.len() < 6 {
            continue;
        }
        if let Some(index) = find_text(&source.text, &text, 0, source.text.len()) {
            candidates.push(index);
        }
    }
    let mut start = 0;
    let mut best_votes = 0;
    for &candidate in &candidates {
        let votes = candidates
            .iter()
            .filter(|&&index| index >= candidate && index <= candidate + 2000)
            .count();
        if votes > best_votes {
            best_votes = votes;
            start = candidate;
        }
    }

    let mut matches: Vec<SasayakiMatch> = Vec::new();
    let mut unmatched = 0;
    let mut cursor = start;
    let mut misses = 0;

    for cue in cues {
        let chars: Vec<char> = library::filtered(&cue.text).chars().collect();
        if chars.is_empty() {
            unmatched += 1;
            continue;
        }

        if cue.text.starts_with('＊') && chars.len() < 5 {
            unmatched += 1;
            continue;
        }

        let mut found = find_text(
            &source.text,
            &chars,
            cursor,
            source.text.len().min(cursor + chars.len() + SEARCH_WINDOW),
        );
        if found.is_none() && misses >= MAX_MISSES && chars.len() >= 10 {
            found = find_unique(&source.text, &chars, cursor);
        }
        let Some(index) = found else {
            unmatched += 1;
            misses += 1;
            continue;
        };

        let end = index + chars.len();
        let range = source
            .chapters
            .iter()
            .find(|chapter| index >= chapter.start && index < chapter.end())
            .unwrap();
        if end > range.end() {
            unmatched += 1;
            misses += 1;
            continue;
        }

        cursor = end;
        misses = 0;
        matches.push(SasayakiMatch {
            id: cue.id.clone(),
            start_time: cue.start_time,
            end_time: cue.end_time,
            text: cue.text.clone(),
            chapter_index: range.chapter_index,
            start: index - range.start,
            length: chars.len(),
        });
    }

    SasayakiMatchData {
        matches,
        unmatched,
        images: source.images,
    }
}

pub fn load_match(app: &AppHandle, id: &str) -> Option<SasayakiMatchData> {
    library::read_book_json(app, id, MATCH_FILE)
}

pub fn load_playback(app: &AppHandle, id: &str) -> Option<SasayakiPlayback> {
    library::read_book_json(app, id, PLAYBACK_FILE)
}

pub fn load_sasayaki_playback(root: &Path) -> Option<SasayakiPlayback> {
    library::read_json(&root.join(PLAYBACK_FILE))
}

pub fn save_sasayaki_playback(playback: &SasayakiPlayback, root: &Path) -> Result<(), String> {
    library::write_json(&root.join(PLAYBACK_FILE), playback)
}

pub fn save_playback(playback: &mut SasayakiPlayback, root: &Path) -> Result<bool, String> {
    let stored = load_sasayaki_playback(root);
    let changed = stored.as_ref().is_none_or(|stored| {
        stored.last_position != playback.last_position
            || stored.delay != playback.delay
            || stored.rate != playback.rate
    });
    playback.modified = if changed {
        Some(library::now_ms())
    } else {
        stored.and_then(|stored| stored.modified)
    };
    save_sasayaki_playback(playback, root)?;
    Ok(changed)
}

#[tauri::command]
pub fn sasayaki_match(
    app: AppHandle,
    id: String,
    path: String,
) -> Result<SasayakiMatchData, String> {
    let data = fs::read(&path).map_err(|error| error.to_string())?;
    let cues = parse_cues(&data);
    let result = match_cues(build_source(&app, &id)?, &cues);
    let dir = library::book_dir(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
    let folder = library::load_metadata_at(&dir).unwrap().folder;
    crate::sync::storage::shared()
        .save_sasayaki_match(&result, &folder, &dir)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn sasayaki_load_match(app: AppHandle, id: String) -> Option<SasayakiMatchData> {
    load_match(&app, &id)
}

#[tauri::command]
pub fn sasayaki_load_playback(app: AppHandle, id: String) -> Option<SasayakiPlayback> {
    load_playback(&app, &id)
}

#[tauri::command]
pub fn sasayaki_save_playback(app: AppHandle, id: String, mut playback: SasayakiPlayback) {
    let Some(dir) = library::book_dir(&app, &id) else {
        return;
    };
    if save_playback(&mut playback, &dir) == Ok(true) {
        let folder = library::load_metadata_at(&dir).unwrap().folder;
        crate::sync::storage::shared()
            .handle_book_change(&folder)
            .ok();
    }
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn sasayaki_transcriber_status() -> &'static str {
    "unsupported"
}

#[tauri::command]
pub fn sasayaki_audio_chapters(app: AppHandle, id: String) -> Vec<AudioChapter> {
    audio_path(&app, &id)
        .filter(|path| audio_mime(path) == "audio/mp4")
        .and_then(|path| audio_chapters(&path))
        .unwrap_or_default()
}

fn find_atom(file: &mut File, start: u64, end: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut position = start;
    while position + 8 <= end {
        let mut header = [0u8; 8];
        file.seek(SeekFrom::Start(position)).ok()?;
        file.read_exact(&mut header).ok()?;
        let mut body = position + 8;
        let size = match u32::from_be_bytes(header[..4].try_into().unwrap()) {
            0 => end - position,
            1 => {
                let mut large = [0u8; 8];
                file.read_exact(&mut large).ok()?;
                body += 8;
                u64::from_be_bytes(large)
            }
            size => u64::from(size),
        };
        if &header[4..] == kind {
            return Some((body, position + size));
        }
        position += size;
    }
    None
}

fn audio_chapters(path: &Path) -> Option<Vec<AudioChapter>> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let (start, end) = find_atom(&mut file, 0, len, b"moov")?;
    let (start, end) = find_atom(&mut file, start, end, b"udta")?;
    let (start, end) = find_atom(&mut file, start, end, b"chpl")?;
    let data = read_range(&mut file, start, end)?;

    let mut position = if data[0] == 0 { 4 } else { 8 };
    let count = data[position];
    position += 1;
    (0..count)
        .map(|_| {
            let start = u64::from_be_bytes(data.get(position..position + 8)?.try_into().unwrap());
            let length = usize::from(*data.get(position + 8)?);
            let title = data.get(position + 9..position + 9 + length)?;
            position += 9 + length;
            Some(AudioChapter {
                start: start as f64 / 10_000_000.0,
                title: String::from_utf8_lossy(title).into_owned(),
            })
        })
        .collect()
}

fn audio_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let path = PathBuf::from(load_playback(app, id)?.audio_path?);
    path.is_file().then_some(path)
}

fn audio_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        _ => "audio/mp4",
    }
}

fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        (start + CHUNK_SIZE - 1).min(len - 1)
    } else {
        end.parse::<u64>().ok()?.min(len - 1)
    };
    (start <= end).then_some((start, end))
}

pub fn audiobook_protocol(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let not_found = || Response::builder().status(404).body(Vec::new()).unwrap();
    let id = request.uri().path().trim_start_matches('/');
    if let Some(id) = id.strip_suffix("/cover") {
        let Some((mime, cover)) = audio_path(app, id).and_then(|path| audio_cover(&path)) else {
            return not_found();
        };
        return Response::builder()
            .header(CONTENT_TYPE, mime)
            .header("Access-Control-Allow-Origin", "*")
            .body(cover)
            .unwrap();
    }
    let Some(path) = audio_path(app, id) else {
        return not_found();
    };
    let (Ok(mut file), Ok(metadata)) = (File::open(&path), fs::metadata(&path)) else {
        return not_found();
    };
    let len = metadata.len();
    if len == 0 {
        return not_found();
    }
    let mime = audio_mime(&path);

    let range = request
        .headers()
        .get("Range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, len));

    match range {
        Some((start, end)) => {
            let mut body = vec![0u8; (end - start + 1) as usize];
            if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut body).is_err() {
                return not_found();
            }
            Response::builder()
                .status(206)
                .header(CONTENT_TYPE, mime)
                .header("Access-Control-Allow-Origin", "*")
                .header("Accept-Ranges", "bytes")
                .header("Content-Range", format!("bytes {start}-{end}/{len}"))
                .body(body)
                .unwrap()
        }
        None => {
            let mut body = Vec::new();
            if file.read_to_end(&mut body).is_err() {
                return not_found();
            }
            Response::builder()
                .header(CONTENT_TYPE, mime)
                .header("Access-Control-Allow-Origin", "*")
                .header("Accept-Ranges", "bytes")
                .body(body)
                .unwrap()
        }
    }
}

fn expand_cue(data: &SasayakiMatchData, cue: &SasayakiMatch, sentence: &str) -> (f64, f64) {
    let cues: Vec<&SasayakiMatch> = data
        .matches
        .iter()
        .filter(|item| item.chapter_index == cue.chapter_index)
        .collect();
    let Some(index) = cues.iter().position(|item| item.id == cue.id) else {
        return (cue.start_time, cue.end_time);
    };

    let mut start = index;
    let mut end = index;
    let filtered_sentence = library::filtered(sentence);
    while start > 0 && filtered_sentence.contains(&library::filtered(&cues[start - 1].text)) {
        start -= 1;
    }
    while end + 1 < cues.len()
        && filtered_sentence.contains(&library::filtered(&cues[end + 1].text))
    {
        end += 1;
    }
    (cues[start].start_time, cues[end].end_time)
}

pub fn cue_sentence_audio(
    app: &AppHandle,
    id: &str,
    cue_id: &str,
    sentence: &str,
) -> Option<Vec<u8>> {
    let data = load_match(app, id)?;
    let cue = data.matches.iter().find(|item| item.id == cue_id)?;
    let playback = load_playback(app, id)?;
    let path = audio_path(app, id)?;

    let range = expand_cue(&data, cue, sentence);
    let start = (range.0 + playback.delay).max(0.0);
    let end = (range.1 + playback.delay).max(start);
    encode_clip(&path, start, end)
}

fn audio_cover(path: &Path) -> Option<(&'static str, Vec<u8>)> {
    let mut file = File::open(path).ok()?;
    let data = if audio_mime(path) == "audio/mp4" {
        mp4_cover(&mut file)?
    } else {
        id3_cover(&mut file)?
    };
    let mime = if data.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some((mime, data))
}

fn read_range(file: &mut File, start: u64, end: u64) -> Option<Vec<u8>> {
    let mut data = vec![0u8; end.checked_sub(start)? as usize];
    file.seek(SeekFrom::Start(start)).ok()?;
    file.read_exact(&mut data).ok()?;
    Some(data)
}

fn mp4_cover(file: &mut File) -> Option<Vec<u8>> {
    let len = file.metadata().ok()?.len();
    let (start, end) = find_atom(file, 0, len, b"moov")?;
    let (start, end) = find_atom(file, start, end, b"udta")?;
    let (start, end) = find_atom(file, start, end, b"meta")?;
    let (start, end) = find_atom(file, start + 4, end, b"ilst")?;
    let (start, end) = find_atom(file, start, end, b"covr")?;
    let (start, end) = find_atom(file, start, end, b"data")?;
    read_range(file, start + 8, end)
}

fn id3_cover(file: &mut File) -> Option<Vec<u8>> {
    let syncsafe = |bytes: &[u8]| {
        bytes
            .iter()
            .fold(0usize, |n, &byte| n << 7 | usize::from(byte & 0x7f))
    };
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;
    if &header[..3] != b"ID3" {
        return None;
    }
    let mut tag = vec![0u8; syncsafe(&header[6..10])];
    file.read_exact(&mut tag).ok()?;

    let mut position = 0;
    while let Some(frame) = tag
        .get(position..position + 10)
        .filter(|frame| frame[0] != 0)
    {
        let size = if header[3] == 4 {
            syncsafe(&frame[4..8])
        } else {
            u32::from_be_bytes(frame[4..8].try_into().unwrap()) as usize
        };
        let body = tag.get(position + 10..position + 10 + size)?;
        if &frame[..4] == b"APIC" {
            let image = [&b"\xFF\xD8\xFF"[..], b"\x89PNG"]
                .iter()
                .filter_map(|signature| body.windows(signature.len()).position(|w| w == *signature))
                .min()?;
            return Some(body[image..].to_vec());
        }
        position += 10 + size;
    }
    None
}

fn encode_clip(path: &Path, start: f64, end: f64) -> Option<Vec<u8>> {
    let (samples, rate, channels) = crate::pcm::decode(path, start, end, audio_mime(path))?;
    if channels == 0 || channels > 2 || samples.is_empty() {
        return None;
    }

    use mp3lame_encoder::{Builder, FlushNoGap, InterleavedPcm, Quality, VbrMode};
    let mut builder = Builder::new()?;
    builder.set_num_channels(channels as u8).ok()?;
    builder.set_sample_rate(rate).ok()?;
    builder.set_vbr_mode(VbrMode::Mtrh).ok()?;
    builder.set_vbr_quality(Quality::Best).ok()?;
    builder.set_to_write_vbr_tag(true).ok()?;
    builder.set_quality(Quality::NearBest).ok()?;
    let mut encoder = builder.build().ok()?;

    let mut mp3 = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(samples.len()));
    encoder
        .encode_to_vec(InterleavedPcm(&samples), &mut mp3)
        .ok()?;
    encoder.flush_to_vec::<FlushNoGap>(&mut mp3).ok()?;

    let mut tag = Vec::with_capacity(encoder.lame_tag_size());
    if encoder.lame_tag_encode_to_vec(&mut tag).is_some() && tag.len() <= mp3.len() {
        mp3[..tag.len()].copy_from_slice(&tag);
    }
    Some(mp3)
}
