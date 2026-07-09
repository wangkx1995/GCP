use chrono::{DateTime, FixedOffset};

pub fn offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

pub fn now() -> DateTime<FixedOffset> {
    chrono::Utc::now().with_timezone(&offset())
}

pub struct East8Timer;

impl tracing_subscriber::fmt::time::FormatTime for East8Timer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", now().format("%Y-%m-%dT%H:%M:%S%.3f+08:00"))
    }
}
