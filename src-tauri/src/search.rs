use std::collections::HashMap;
use std::ops::Range;

use rbook::Epub;
use rbook::epub::toc::EpubTocEntry;
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use tauri::AppHandle;

use crate::library::{self, BookInfo};

const LIMIT: usize = 100;
const SENTENCE_DELIMITERS: &str = "。！？.!?\n\r";
const TRAILING_SENTENCE_CHARS: &str = "。、！？」』）)】〉》〕｝}］]";
const BRACKETS: [(char, char); 12] = [
    ('「', '」'),
    ('『', '』'),
    ('（', '）'),
    ('(', ')'),
    ('【', '】'),
    ('〈', '〉'),
    ('《', '》'),
    ('〔', '〕'),
    ('｛', '｝'),
    ('{', '}'),
    ('［', '］'),
    ('[', ']'),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    chapter: String,
    character: usize,
    prefix: String,
    matched: String,
    suffix: String,
    length: usize,
}

fn chapter_labels(
    entry: &EpubTocEntry,
    parent: Option<&str>,
    labels: &mut HashMap<String, String>,
) {
    for child in entry.iter() {
        let label = parent.unwrap_or_else(|| child.label());
        if let Some(key) = child.manifest_entry().and_then(|item| {
            item.resource()
                .key()
                .value()
                .map(library::normalize_resource_key)
        }) {
            labels.entry(key).or_insert_with(|| label.to_string());
        }
        chapter_labels(&child, Some(label), labels);
    }
}

fn plain_text(html: &str) -> String {
    let mut text = library::body(html).to_string();
    for (pattern, replacement) in [
        (r"(?s)<rt[^>]*>.*?</rt>|<rp[^>]*>.*?</rp>", ""),
        (r"(?s)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>", ""),
        (
            r"(?i)<br[^>]*>|</(?:p|div|h[1-6]|li|blockquote|section|td|tr)>",
            "\n",
        ),
        (r"<[^>]+>", ""),
        (r"&#[xX]?[0-9A-Fa-f]+;", ""),
    ] {
        text = Regex::new(pattern)
            .unwrap()
            .replace_all(&text, replacement)
            .into_owned();
    }
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn sentence(line: &str, range: Range<usize>) -> Range<usize> {
    let mut start = line[..range.start]
        .char_indices()
        .rev()
        .find(|(_, ch)| SENTENCE_DELIMITERS.contains(*ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    let mut end = line[range.end..]
        .char_indices()
        .find(|(_, ch)| SENTENCE_DELIMITERS.contains(*ch))
        .map_or(line.len(), |(index, ch)| range.end + index + ch.len_utf8());
    while let Some(ch) = line[end..]
        .chars()
        .next()
        .filter(|ch| TRAILING_SENTENCE_CHARS.contains(*ch))
    {
        end += ch.len_utf8();
    }
    start += line[start..range.start].len() - line[start..range.start].trim_start().len();
    end -= line[range.end..end].len() - line[range.end..end].trim_end().len();

    let mut stack = Vec::new();
    let mut unmatched = Vec::new();
    for ch in line[start..end].chars() {
        if BRACKETS.iter().any(|&(open, _)| open == ch) {
            stack.push(ch);
        } else if BRACKETS.iter().any(|&(_, close)| close == ch) {
            if stack
                .last()
                .is_some_and(|&open| BRACKETS.contains(&(open, ch)))
            {
                stack.pop();
            } else {
                unmatched.push(ch);
            }
        }
    }
    while let Some(&open) = stack.first() {
        if start >= range.start || !line[start..].starts_with(open) {
            break;
        }
        stack.remove(0);
        start += open.len_utf8();
    }
    let mut cursor = end;
    while let Some(&close) = unmatched.last() {
        if cursor <= range.end {
            break;
        }
        let ch = line[..cursor].chars().next_back().unwrap();
        let previous = cursor - ch.len_utf8();
        if ch == close {
            unmatched.pop();
            end = previous;
        } else if !SENTENCE_DELIMITERS.contains(ch) {
            break;
        }
        cursor = previous;
    }
    start += line[start..range.start].len() - line[start..range.start].trim_start().len();
    end -= line[range.end..end].len() - line[range.end..end].trim_end().len();
    start..end
}

fn search_chapter(
    content: &str,
    label: &str,
    start: usize,
    query: &Regex,
    results: &mut Vec<SearchResult>,
) {
    let mut offset = start;
    for paragraph in plain_text(content).split('\n') {
        let line = paragraph.trim_matches(|ch: char| ch.is_whitespace() && ch != '\r');
        for found in query.find_iter(line) {
            let bounds = sentence(line, found.range());
            results.push(SearchResult {
                chapter: label.to_string(),
                character: offset + library::count_chars(&line[..found.start()]),
                prefix: line[bounds.start..found.start()].to_string(),
                matched: found.as_str().to_string(),
                suffix: line[found.end()..bounds.end].to_string(),
                length: library::count_chars(found.as_str()),
            });
            if results.len() >= LIMIT {
                return;
            }
        }
        offset += library::count_chars(line);
    }
}

#[tauri::command]
pub async fn search_book(
    app: AppHandle,
    id: String,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path =
            library::book_epub_path(&app, &id).ok_or_else(|| format!("Book {id} was not found"))?;
        let epub = Epub::open(path).map_err(|error| error.to_string())?;
        let info: BookInfo = library::read_book_json(&app, &id, library::BOOKINFO_FILE).unwrap();
        let query = RegexBuilder::new(&regex::escape(&query))
            .case_insensitive(true)
            .build()
            .unwrap();
        let mut labels = HashMap::new();
        if let Some(root) = epub.toc().contents() {
            chapter_labels(&root, None, &mut labels);
        }
        let mut label = "";
        let mut results = Vec::new();
        for entry in epub.spine().iter() {
            let Some(item) = entry.manifest_entry() else {
                continue;
            };
            let href = library::normalize_resource_key(item.resource().key().value().unwrap());
            if let Some(next_label) = labels.get(&href) {
                label = next_label;
            }
            let Some(chapter) = info.chapter_info.get(&href) else {
                continue;
            };
            let Ok(content) = item.read_str() else {
                continue;
            };
            search_chapter(&content, label, chapter.current_total, &query, &mut results);
            if results.len() >= LIMIT {
                break;
            }
        }
        Ok(results)
    })
    .await
    .map_err(|error| error.to_string())?
}
