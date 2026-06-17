use anyhow::{bail, Context, Result};
use chrono::{Duration, NaiveDateTime};
use regex::Regex;

pub(crate) fn render_scan_start_time(pattern: &str, scan_start_time: &str) -> Result<String> {
    let time = NaiveDateTime::parse_from_str(scan_start_time, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("invalid --scan-start-time: {scan_start_time}"))?;
    let re = Regex::new(r"\$\{SCAN_START_TIME([+-]\d+[mhd])?,([^}]+)\}")?;
    let mut rendered = String::with_capacity(pattern.len());
    let mut last = 0;
    for captures in re.captures_iter(pattern) {
        let matched = captures.get(0).expect("full match");
        rendered.push_str(&pattern[last..matched.start()]);
        let offset = captures.get(1).map(|m| m.as_str());
        let format = captures.get(2).expect("format").as_str();
        let adjusted = apply_offset(time, offset)?;
        rendered.push_str(&format_scan_start_time(&adjusted, format)?);
        last = matched.end();
    }
    rendered.push_str(&pattern[last..]);
    if rendered.contains("${SCAN_START_TIME") {
        bail!("unsupported SCAN_START_TIME template syntax in pattern: {pattern}");
    }
    Ok(rendered)
}

pub(crate) fn infer_scan_dir(rendered_pattern: &str) -> String {
    let Some(last_slash) = rendered_pattern.rfind('/') else {
        return ".".to_string();
    };
    let dir_part = &rendered_pattern[..last_slash];
    if dir_part.is_empty() {
        return "/".to_string();
    }

    let regex_chars = [
        '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '^', '$', '\\',
    ];
    if let Some(regex_idx) = dir_part.rfind(|ch| regex_chars.contains(&ch)) {
        return dir_part[..regex_idx]
            .rfind('/')
            .map(|idx| {
                if idx == 0 {
                    "/".to_string()
                } else {
                    dir_part[..idx].to_string()
                }
            })
            .unwrap_or_else(|| ".".to_string());
    }
    dir_part.to_string()
}

fn format_scan_start_time(time: &NaiveDateTime, format: &str) -> Result<String> {
    let chrono_format = match format {
        "yyyyMMdd" => "%Y%m%d",
        "yyyy-MM-dd" => "%Y-%m-%d",
        "yyyyMMddHH" => "%Y%m%d%H",
        "yyyyMMddHHmm" => "%Y%m%d%H%M",
        "yyyyMMddHHmmss" => "%Y%m%d%H%M%S",
        _ => bail!("unsupported SCAN_START_TIME format: {format}"),
    };
    Ok(time.format(chrono_format).to_string())
}

fn apply_offset(time: NaiveDateTime, offset: Option<&str>) -> Result<NaiveDateTime> {
    let Some(offset) = offset else {
        return Ok(time);
    };
    let sign = &offset[..1];
    let unit = offset
        .chars()
        .last()
        .expect("offset regex guarantees a unit");
    let amount = offset[1..offset.len() - 1]
        .parse::<i64>()
        .with_context(|| format!("invalid SCAN_START_TIME offset: {offset}"))?;
    let duration = match unit {
        'm' => Duration::minutes(amount),
        'h' => Duration::hours(amount),
        'd' => Duration::days(amount),
        _ => bail!("unsupported SCAN_START_TIME offset unit: {unit}"),
    };
    if sign == "-" {
        Ok(time - duration)
    } else {
        Ok(time + duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_scan_start_time_templates() {
        let rendered = render_scan_start_time(
            "/data/${SCAN_START_TIME,yyyyMMdd}/FILE_${SCAN_START_TIME,yyyyMMddHHmmss}.*\\.csv",
            "2026-06-15 13:45:00",
        )
        .unwrap();
        assert_eq!(rendered, "/data/20260615/FILE_20260615134500.*\\.csv");
    }

    #[test]
    fn renders_scan_start_time_with_minute_offset() {
        let rendered = render_scan_start_time(
            "FILE_${SCAN_START_TIME+15m,yyyyMMddHHmm}_${SCAN_START_TIME-15m,yyyyMMddHHmm}",
            "2026-06-15 13:45:00",
        )
        .unwrap();
        assert_eq!(rendered, "FILE_202606151400_202606151330");
    }

    #[test]
    fn renders_scan_start_time_with_hour_and_day_offset() {
        let rendered = render_scan_start_time(
            "${SCAN_START_TIME+1h,yyyyMMddHHmm}/${SCAN_START_TIME+1d,yyyyMMddHHmm}",
            "2026-06-15 13:45:00",
        )
        .unwrap();
        assert_eq!(rendered, "202606151445/202606161345");
    }

    #[test]
    fn rejects_unsupported_offset_syntax() {
        let err =
            render_scan_start_time("${SCAN_START_TIME+1w,yyyyMMddHHmm}", "2026-06-15 13:45:00")
                .unwrap_err();
        assert!(err.to_string().contains("SCAN_START_TIME"));
    }

    #[test]
    fn infers_plain_scan_dir() {
        assert_eq!(
            infer_scan_dir("/data/pm/20260615/FILE_.*\\.zip"),
            "/data/pm/20260615"
        );
    }

    #[test]
    fn infers_scan_dir_before_regex_dir_part() {
        assert_eq!(infer_scan_dir("/data/pm/.*/FILE_.*"), "/data/pm");
    }
}
