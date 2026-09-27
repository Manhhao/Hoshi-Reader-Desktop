use std::sync::LazyLock;

use regex::{Captures, Regex};

static CALIBRE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^(\s*\.(?:calibre\d*|body|c\d*|p\d+)\s*)\{(.*?)\}").unwrap()
});

const WRITING_MODE_PROPERTIES: [&str; 3] =
    ["writing-mode", "-webkit-writing-mode", "-epub-writing-mode"];

pub fn sanitize_css(css: &str) -> String {
    let mut did_strip_line_height = false;
    let result = CALIBRE_RULE.replace_all(css, |caps: &Captures| {
        let selector = &caps[1];
        let declarations: Vec<&str> = caps[2].split(';').collect();
        if declarations
            .iter()
            .any(|d| property_name(d) == "line-height")
        {
            did_strip_line_height = true;
        }
        let strip_height = declarations
            .iter()
            .any(|d| WRITING_MODE_PROPERTIES.contains(&property_name(d).as_str()));
        let cleaned = declarations
            .iter()
            .filter_map(|d| sanitize_declaration(d, strip_height))
            .collect::<Vec<_>>()
            .join(";");
        format!("{selector}{{{cleaned}}}")
    });
    if did_strip_line_height {
        format!("{result}\nbody {{ line-height: 1.65; }}\n")
    } else {
        result.into_owned()
    }
}

fn sanitize_declaration(declaration: &str, strip_height: bool) -> Option<String> {
    let name = property_name(declaration);
    if WRITING_MODE_PROPERTIES.contains(&name.as_str()) {
        return None;
    }
    match name.as_str() {
        "line-height" => None,
        "height" if strip_height => None,
        "text-indent" => {
            let value = declaration.split_once(':').map_or("", |(_, v)| v.trim());
            if value.starts_with('-') {
                Some(declaration.to_string())
            } else {
                Some(" text-indent: 0".to_string())
            }
        }
        _ => Some(declaration.to_string()),
    }
}

fn property_name(declaration: &str) -> String {
    declaration
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}
