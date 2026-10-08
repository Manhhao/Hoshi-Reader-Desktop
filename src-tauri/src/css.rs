use std::sync::LazyLock;

use regex::{Captures, Regex};

static CALIBRE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^(\s*\.(?:calibre\d*|body|c\d*|p\d+)\s*)\{(.*?)\}").unwrap()
});

static RULE_BODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^{}]*)\}").unwrap());

static CSS_PRELUDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)^\x{feff}?(?:\s+|/\*.*?\*/|@(?:charset|import|namespace)\b[^;{]*;)*").unwrap()
});

static CSS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(@import\s+(?:url\(\s*)?(?:"[^"]+|'[^']+|[^"'\s);]+))"#).unwrap()
});

static CSS_SELECTOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^{}]+)\{").unwrap());

static SELECTOR_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[\[.#:"'=]*[\w-]+"#).unwrap());

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
    let result = RULE_BODY.replace_all(&result, |caps: &Captures| match content_limit(&caps[1]) {
        Some(limit) => format!("{{{};{limit}}}", &caps[1]),
        None => caps[0].to_string(),
    });
    if did_strip_line_height {
        format!("{result}\nbody {{ line-height: 1.65; }}\n")
    } else {
        result.into_owned()
    }
}

pub fn scope_continuous_css(css: &str, scope: &str) -> String {
    let rules = CSS_PRELUDE.find(css).map_or(0, |prelude| prelude.end());
    let prelude = CSS_IMPORT.replace_all(&css[..rules], format!("${{1}}?continuous={scope}"));
    let rules = CSS_SELECTOR.replace_all(&css[rules..], |caps: &Captures| {
        if caps[1].trim_start().starts_with('@') {
            return caps[0].to_string();
        }
        let selector = SELECTOR_NAME.replace_all(&caps[1], |name: &Captures| {
            match name[0].to_ascii_lowercase().as_str() {
                "html" => "hoshi-html".to_string(),
                "body" => "hoshi-body".to_string(),
                ":root" => ".hoshi-root".to_string(),
                _ => name[0].to_string(),
            }
        });
        format!("{selector}{{")
    });
    format!("{prelude}@scope ([data-hoshi-styles~=\"{scope}\"]) {{\n{rules}\n}}\n")
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

fn content_limit(body: &str) -> Option<String> {
    let declarations: Vec<&str> = body.split(';').collect();
    let value = |names: &[&str]| {
        declarations
            .iter()
            .find(|d| names.contains(&property_name(d).as_str()))
            .and_then(|d| d.split_once(':'))
            .map(|(_, value)| value.trim().to_lowercase())
    };
    let writing_mode = value(&WRITING_MODE_PROPERTIES)?;
    let size = if writing_mode.starts_with("horizontal") {
        "width"
    } else if writing_mode.starts_with("vertical") {
        "height"
    } else {
        return None;
    };
    let percent: f64 = value(&[size])?.strip_suffix('%')?.trim().parse().ok()?;
    Some(format!(
        "max-{size}:calc(var(--hoshi-content-{size}) * {})",
        percent / 100.0
    ))
}

fn property_name(declaration: &str) -> String {
    declaration
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}
