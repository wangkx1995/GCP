use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike};

const BEIJING_OFFSET_SECS: i32 = 8 * 3600;
const MINUTE_MS: i64 = 60_000;

fn beijing() -> FixedOffset {
    FixedOffset::east_opt(BEIJING_OFFSET_SECS).unwrap()
}

/// 计算扫描时间窗 [start, end]（毫秒）
pub fn get_scan_scope(fire_time_ms: i64, period_sec: i64, delay_sec: i64) -> Option<(i64, i64)> {
    if period_sec <= 0 {
        return None;
    }
    let delay_sec = delay_sec.max(0);
    let fire_min = (fire_time_ms / MINUTE_MS) * MINUTE_MS;
    let delay_ms = delay_sec * 1000;

    if period_sec == 2_592_000 {
        let aligned = align_month(fire_min);
        let start = prev_month_first_day_ms(aligned) - delay_ms;
        let end = aligned - delay_ms;
        return Some((start, end));
    }

    let aligned = match period_sec {
        3_600 => align_hour(fire_min),
        86_400 => align_day(fire_min),
        604_800 => align_week(fire_min),
        _ if period_sec >= 3_600 => align_to_period_grid(fire_min, period_sec),
        _ => align_to_period(fire_min, period_sec),
    };

    let period_ms = period_sec * 1000;
    let end = aligned - delay_ms;
    let start = aligned - period_ms - delay_ms;
    Some((start, end))
}

/// 亚小时周期：按北京时间 +8h 对齐后取模 floor
fn align_to_period(t: i64, period_sec: i64) -> i64 {
    let p = period_sec * 1000;
    let shift = BEIJING_OFFSET_SECS as i64 * 1000;
    t - ((t + shift) % p)
}

/// 长周期（>=3600 非精确值）：直接按周期毫秒取模 floor
fn align_to_period_grid(t: i64, period_sec: i64) -> i64 {
    let p = period_sec * 1000;
    t - (t % p)
}

fn align_hour(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_day(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_week(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    let days_since_monday = dt.weekday().num_days_from_monday();
    let date = NaiveDate::from_ymd_opt(dt.year(), dt.month(), dt.day()).unwrap()
        - chrono::Duration::days(days_since_monday as i64);
    beijing()
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn align_month(t: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(t).unwrap().with_timezone(&beijing());
    beijing()
        .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn prev_month_first_day_ms(current_month_first_ms: i64) -> i64 {
    let dt = DateTime::from_timestamp_millis(current_month_first_ms)
        .unwrap()
        .with_timezone(&beijing());
    let year = dt.year();
    let month = dt.month();
    let (prev_year, prev_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    beijing()
        .with_ymd_and_hms(prev_year, prev_month, 1, 0, 0, 0)
        .unwrap()
        .timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(h: u32, m: u32, s: u32) -> i64 {
        beijing()
            .with_ymd_and_hms(2026, 7, 9, h, m, s)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn period_zero_returns_none() {
        assert_eq!(get_scan_scope(ts(5, 0, 0), 0, 0), None);
        assert_eq!(get_scan_scope(ts(5, 0, 0), -1, 0), None);
    }

    #[test]
    fn fifteen_minutes_no_delay() {
        let (start, end) = get_scan_scope(ts(5, 0, 0), 900, 0).unwrap();
        assert_eq!(start, ts(4, 45, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn fifteen_minutes_with_delay() {
        let (start, end) = get_scan_scope(ts(5, 0, 0), 900, 300).unwrap();
        assert_eq!(start, ts(4, 40, 0));
        assert_eq!(end, ts(4, 55, 0));
    }

    #[test]
    fn two_hour_grid() {
        let (start, end) = get_scan_scope(ts(5, 30, 0), 7200, 0).unwrap();
        assert_eq!(start, ts(2, 0, 0));
        assert_eq!(end, ts(4, 0, 0));
    }

    #[test]
    fn one_hour() {
        let (start, end) = get_scan_scope(ts(5, 23, 0), 3600, 0).unwrap();
        assert_eq!(start, ts(4, 0, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn one_day() {
        let (start, end) = get_scan_scope(ts(9, 12, 0), 86400, 0).unwrap();
        assert_eq!(end, ts(0, 0, 0));
        assert_eq!(start, ts(0, 0, 0) - 86400 * 1000);
    }

    #[test]
    fn one_week_monday() {
        // 2026-07-09 is Thursday; Monday is 2026-07-06
        let (start, end) = get_scan_scope(ts(9, 12, 0), 604800, 0).unwrap();
        assert_eq!(end, beijing().with_ymd_and_hms(2026, 7, 6, 0, 0, 0).unwrap().timestamp_millis());
        assert_eq!(start, end - 604800 * 1000);
    }

    #[test]
    fn calendar_month() {
        // fire = 2026-07-09 12:00, aligned = 2026-07-01, start = 2026-06-01
        let (start, end) = get_scan_scope(ts(9, 12, 0), 2_592_000, 0).unwrap();
        let expected_end = beijing().with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap().timestamp_millis();
        let expected_start = beijing().with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap().timestamp_millis();
        assert_eq!(end, expected_end);
        assert_eq!(start, expected_start);
    }

    #[test]
    fn negative_delay_is_clamped_to_zero() {
        let (start, end) = get_scan_scope(ts(5, 0, 0), 900, -300).unwrap();
        assert_eq!(start, ts(4, 45, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn sub_minute_fire_time_is_floored() {
        let fire = ts(5, 0, 30) + 123;
        let (start, end) = get_scan_scope(fire, 900, 0).unwrap();
        assert_eq!(start, ts(4, 45, 0));
        assert_eq!(end, ts(5, 0, 0));
    }

    #[test]
    fn five_minute_period() {
        // fire = 05:07 -> align to 05:05 Beijing
        let (start, end) = get_scan_scope(ts(5, 7, 0), 300, 0).unwrap();
        assert_eq!(start, ts(5, 0, 0));
        assert_eq!(end, ts(5, 5, 0));
    }

    #[test]
    fn three_hour_grid() {
        // fire = 08:30 Beijing = 00:30 UTC -> align to 00:00 UTC = 08:00 Beijing
        let (start, end) = get_scan_scope(ts(8, 30, 0), 10_800, 0).unwrap();
        assert_eq!(start, ts(5, 0, 0));
        assert_eq!(end, ts(8, 0, 0));
    }

    #[test]
    fn calendar_month_january_rolls_to_previous_year() {
        let fire = beijing()
            .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let (start, end) = get_scan_scope(fire, 2_592_000, 0).unwrap();
        let expected_end = beijing().with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap().timestamp_millis();
        let expected_start = beijing().with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap().timestamp_millis();
        assert_eq!(end, expected_end);
        assert_eq!(start, expected_start);
    }

    #[test]
    fn one_day_with_delay() {
        let (start, end) = get_scan_scope(ts(9, 12, 0), 86_400, 3_600).unwrap();
        assert_eq!(end, ts(0, 0, 0) - 3_600 * 1000);
        assert_eq!(start, end - 86_400 * 1000);
    }
}
