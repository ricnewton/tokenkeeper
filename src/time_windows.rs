//! Date helpers. Timestamps are Unix milliseconds (UTC); display uses the
//! user's local time zone via the Win32 time APIs.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

pub const DAY_MS: i64 = 86_400_000;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Parses `2026-04-02T14:11:55.000Z`-style UTC timestamps, or a plain
/// millisecond count (Cursor uses both).
pub fn parse_timestamp_ms(s: &str) -> Option<i64> {
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms);
    }
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    if b.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        return Some(days_from_civil(num(0..4)?, num(5..7)?, num(8..10)?) * DAY_MS);
    }
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = s
        .get(19..)
        .and_then(|f| f.strip_prefix('.'))
        .map_or(0, |f| {
            let digits: String = f
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .take(3)
                .collect();
            format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
        });
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec) * 1000 + frac)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day) for days since 1970-01-01 (Howard Hinnant).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// The same UTC instant `months` calendar months earlier (day clamped to 28
/// so it always exists); used to step back through monthly billing cycles.
pub fn months_before(ms: i64, months: i64) -> i64 {
    let day_ms = ms.rem_euclid(DAY_MS);
    let (y, m, d) = civil_from_days(ms.div_euclid(DAY_MS));
    let total = y * 12 + (m - 1) - months;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    days_from_civil(ny, nm, d.min(28)) * DAY_MS + day_ms
}

/// Milliseconds since the epoch as read on a local wall clock, so that
/// `div_euclid(DAY_MS)` buckets by local calendar day.
pub fn local_wall_ms(ms: i64) -> i64 {
    let t = local(ms);
    let days = days_from_civil(t.wYear as i64, t.wMonth as i64, t.wDay as i64);
    days * DAY_MS
        + (t.wHour as i64 * 3600 + t.wMinute as i64 * 60 + t.wSecond as i64) * 1000
        + t.wMilliseconds as i64
}

pub fn local(ms: i64) -> SYSTEMTIME {
    let ticks = (ms + 11_644_473_600_000) as u64 * 10_000;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut loc = SYSTEMTIME::default();
    unsafe {
        let _ = FileTimeToSystemTime(&ft, &mut utc);
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).is_err() {
            loc = utc;
        }
    }
    loc
}

/// "20 Oct" in local time.
pub fn day_month(ms: i64) -> String {
    let t = local(ms);
    format!(
        "{} {}",
        t.wDay,
        MONTHS[(t.wMonth as usize).saturating_sub(1).min(11)]
    )
}

/// "Oct" in local time.
pub fn month(ms: i64) -> &'static str {
    MONTHS[(local(ms).wMonth as usize).saturating_sub(1).min(11)]
}

/// Human "x min ago" for a past timestamp.
pub fn ago(ms: i64) -> String {
    let mins = (now_ms() - ms).max(0) / 60_000;
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        _ => format!("{} h ago", mins / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_and_ms() {
        assert_eq!(parse_timestamp_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            parse_timestamp_ms("2026-04-02T14:11:55.000Z"),
            Some(1_775_139_115_000)
        );
        assert_eq!(
            parse_timestamp_ms("2026-04-02T14:11:55Z"),
            Some(1_775_139_115_000)
        );
        assert_eq!(parse_timestamp_ms("1768399334000"), Some(1_768_399_334_000));
        assert_eq!(parse_timestamp_ms("2026-04-02"), Some(1_775_088_000_000));
        assert_eq!(parse_timestamp_ms("nonsense"), None);
    }

    #[test]
    fn steps_back_months() {
        let oct1 = parse_timestamp_ms("2026-10-01T00:00:00.000Z").unwrap();
        assert_eq!(
            months_before(oct1, 1),
            parse_timestamp_ms("2026-09-01T00:00:00.000Z").unwrap()
        );
        assert_eq!(
            months_before(oct1, 10),
            parse_timestamp_ms("2025-12-01T00:00:00.000Z").unwrap()
        );
        let d = 20_000;
        assert_eq!(days_from_civil_roundtrip(d), d);
    }

    fn days_from_civil_roundtrip(d: i64) -> i64 {
        let (y, m, dd) = civil_from_days(d);
        days_from_civil(y, m, dd)
    }
}
