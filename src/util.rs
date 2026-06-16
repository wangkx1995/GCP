use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::Result;
use encoding_rs::GBK;

pub fn read_text(path: &Path, encoding: &str) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if encoding.eq_ignore_ascii_case("GBK") || encoding.eq_ignore_ascii_case("GB2312") {
        let (text, _, _) = GBK.decode(&bytes);
        return Ok(text.into_owned());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn looks_like_delimited(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buf = [0_u8; 512];
    let len = file.read(&mut buf)?;
    Ok(buf[..len].contains(&b'|'))
}

pub fn normalize_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "/0" {
        return "0".to_string();
    }
    if trimmed.eq_ignore_ascii_case("NIL")
        || trimmed.eq_ignore_ascii_case("NULL")
        || trimmed == "\"\""
        || trimmed.eq_ignore_ascii_case("N/A")
        || trimmed == "-"
    {
        return String::new();
    }
    if trimmed.contains('"') {
        trimmed.replace('"', "")
    } else {
        trimmed.to_string()
    }
}

pub fn column_name_format(value: &str) -> String {
    value
        .trim()
        .replace("&gt;&lt;", "_")
        .replace("&gt;", "")
        .replace("&lt;", "")
        .replace("><", "_")
        .replace('>', "")
        .replace('<', "")
        .replace("][", "_")
        .replace('[', "")
        .replace(']', "")
        .replace('.', "_")
}

pub fn normalize_lookup_name(value: &str) -> String {
    column_name_format(value)
        .to_ascii_uppercase()
        .replace(' ', "")
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub fn strip_suffix(value: &str, suffix: &str) -> String {
    value
        .strip_suffix(suffix)
        .or_else(|| value.strip_suffix(&suffix.to_ascii_uppercase()))
        .unwrap_or(value)
        .to_string()
}

pub fn sanitize_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch == ':' {
                '_'
            } else {
                ch
            }
        })
        .collect()
}
