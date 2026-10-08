use std::sync::LazyLock;

use regex::{Captures, Regex};

static CALIBRE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^(\s*\.(?:calibre\d*|body|c\d*|p\d+)\s*)\{(.*?)\}").unwrap()
});

static RULE_BODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^{}]*)\}").unwrap());

static CSS_BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)([^{}]+)\{").unwrap());

static HTML_SELECTOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[\s>+~,(])(?:html|:root)($|[\s>+~.#:\[,(])").unwrap());

static BODY_SELECTOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[\s>+~,(])body($|[\s>+~.#:\[,(])").unwrap());

static QUOTED_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(@import\s+(?:url\(\s*)?)(["'])([^"']+)(["'])"#).unwrap());

static UNQUOTED_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(@import\s+url\(\s*)([^"'\s)]+)"#).unwrap());

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
    let css = QUOTED_IMPORT.replace_all(css, |caps: &Captures| {
        format!(
            "{}{}{}{}",
            &caps[1],
            &caps[2],
            continuous_import(&caps[3], scope),
            &caps[4]
        )
    });
    let css = UNQUOTED_IMPORT.replace_all(&css, |caps: &Captures| {
        format!("{}{}", &caps[1], continuous_import(&caps[2], scope))
    });
    let start = css_scope_start(&css);
    let rules = CSS_BLOCK.replace_all(&css[start..], |caps: &Captures| {
        let selector = &caps[1];
        if selector.trim_start().starts_with('@') {
            return caps[0].to_string();
        }
        let selector = HTML_SELECTOR.replace_all(selector, "$1.hoshi-html$2");
        let selector = BODY_SELECTOR.replace_all(&selector, "$1.hoshi-body$2");
        format!("{selector}{{")
    });
    format!(
        "{}\n@scope ([data-hoshi-styles~=\"{scope}\"]) {{\n{rules}\n}}\n",
        &css[..start]
    )
}

fn continuous_import(url: &str, scope: &str) -> String {
    if url.contains("continuous=")
        || url.contains("://")
        || url.starts_with("//")
        || url.starts_with("data:")
    {
        return url.to_string();
    }
    let (url, fragment) = url.split_once('#').unwrap_or((url, ""));
    let separator = if url.contains('?') { '&' } else { '?' };
    let fragment = if fragment.is_empty() {
        String::new()
    } else {
        format!("#{fragment}")
    };
    format!("{url}{separator}continuous={scope}{fragment}")
}

fn css_scope_start(css: &str) -> usize {
    let bytes = css.as_bytes();
    let mut position = 0;
    loop {
        while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
            position += 1;
        }
        if css[position..].starts_with('\u{feff}') {
            position += '\u{feff}'.len_utf8();
            continue;
        }
        if bytes.get(position..position + 2) == Some(b"/*") {
            let Some(end) = css[position + 2..].find("*/") else {
                return position;
            };
            position += end + 4;
            continue;
        }
        let rest = &css[position..];
        let directive = rest
            .get(..8)
            .is_some_and(|value| value.eq_ignore_ascii_case("@charset"))
            || rest
                .get(..7)
                .is_some_and(|value| value.eq_ignore_ascii_case("@import"))
            || rest
                .get(..10)
                .is_some_and(|value| value.eq_ignore_ascii_case("@namespace"))
            || rest
                .get(..6)
                .is_some_and(|value| value.eq_ignore_ascii_case("@layer"));
        if !directive {
            return position;
        }
        let Some(length) = css_statement_length(rest) else {
            return position;
        };
        position += length;
    }
}

fn css_statement_length(css: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut parentheses = 0;
    for (index, byte) in css.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if byte == current {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => parentheses += 1,
            b')' => parentheses -= 1,
            b'{' if parentheses == 0 => return None,
            b';' if parentheses == 0 => return Some(index + 1),
            _ => {}
        }
    }
    None
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
