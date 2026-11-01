//! Timestamps: milliseconds since the Unix epoch, UTC. Formatting goes through chrono.

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

/// A moment in time, in milliseconds since 1970-01-01T00:00:00Z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Ts(pub i64);

impl Ts {
    /// The current wall-clock time (works in the browser too).
    pub fn now() -> Self {
        let ms = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Ts(i64::try_from(ms).unwrap_or(i64::MAX))
    }

    /// From whole seconds.
    pub const fn from_secs(s: i64) -> Self {
        Ts(s.saturating_mul(1000))
    }

    pub fn millis_since(self, earlier: Ts) -> i64 {
        self.0.saturating_sub(earlier.0)
    }

    pub fn plus_millis(self, ms: i64) -> Ts {
        Ts(self.0.saturating_add(ms))
    }

    fn utc(self) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp_millis(self.0)
    }

    /// RFC 3339 (`2026-10-31T09:30:00Z`), or an empty string for out-of-range values.
    pub fn rfc3339(self) -> String {
        self.utc().map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
    }

    /// `9:30 AM` style clock time (UTC; the UI applies the viewer's offset first).
    pub fn clock(self) -> String {
        self.utc().map(|d| d.format("%-I:%M %p").to_string()).unwrap_or_default()
    }

    /// `Saturday, October 31` style day label.
    pub fn day_label(self) -> String {
        self.utc().map(|d| d.format("%A, %B %-d").to_string()).unwrap_or_default()
    }

    /// The UTC calendar day as `(year, ordinal day)`, for grouping messages by day.
    pub fn day_key(self) -> (i32, u32) {
        self.utc().map(|d| (d.year(), d.ordinal())).unwrap_or((0, 0))
    }

    /// Parses `YYYY-MM-DD` as midnight UTC.
    pub fn parse_date(s: &str) -> Option<Ts> {
        let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
        let dt = d.and_hms_opt(0, 0, 0)?.and_utc();
        Some(Ts(dt.timestamp_millis()))
    }

    /// A short relative description: `just now`, `5m ago`, `3h ago`, `2d ago`.
    pub fn ago(self, now: Ts) -> String {
        let secs = now.millis_since(self) / 1000;
        match secs {
            s if s < 60 => "just now".into(),
            s if s < 3600 => format!("{}m ago", s / 60),
            s if s < 86_400 => format!("{}h ago", s / 3600),
            s => format!("{}d ago", s / 86_400),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses() {
        let t = Ts::parse_date("2026-10-31").unwrap();
        assert_eq!(t.rfc3339(), "2026-10-31T00:00:00Z");
        assert_eq!(t.plus_millis(9 * 3_600_000 + 30 * 60_000).clock(), "9:30 AM");
        assert_eq!(t.day_label(), "Saturday, October 31");
        assert_eq!(Ts::parse_date("2026-02-30"), None);
        assert_eq!(Ts(i64::MAX).rfc3339(), "");
        assert_eq!(Ts(0).ago(Ts::from_secs(7200)), "2h ago");
    }

    #[test]
    fn now_is_after_2020() {
        assert!(Ts::now() > Ts::parse_date("2020-01-01").unwrap());
    }
}
