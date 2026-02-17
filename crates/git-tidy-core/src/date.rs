/// Compute the number of days between an ISO 8601 date string and today.
///
/// Expects the `YYYY-MM-DDTHH:MM:SS+HH:MM` format produced by `git log --format='%aI'`.
/// Returns `None` if the date portion cannot be parsed.
pub fn days_since_iso_date(iso: &str) -> Option<u64> {
    let date_part = iso.split('T').next()?;
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return None;
    }

    let year: i64 = parts[0].parse().ok()?;
    let month: i64 = parts[1].parse().ok()?;
    let day: i64 = parts[2].parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let then_days = days_from_civil(year, month, day);

    let now = now_date();
    let now_days = days_from_civil(now.0, now.1, now.2);

    if now_days >= then_days {
        Some((now_days - then_days) as u64)
    } else {
        Some(0)
    }
}

/// Convert a civil date to a day count (days since epoch).
/// Algorithm from Howard Hinnant's date algorithms.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Get today's date as (year, month, day) using the system clock.
fn now_date() -> (i64, i64, i64) {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    civil_from_days(secs / 86400)
}

/// Convert a day count (days since Unix epoch) to a civil date.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_today_is_zero() {
        let (y, m, d) = now_date();
        let iso = format!("{y:04}-{m:02}-{d:02}T12:00:00+00:00");
        assert_eq!(days_since_iso_date(&iso), Some(0));
    }

    #[test]
    fn days_since_known_date() {
        // days_from_civil for a known epoch date
        let epoch_days = days_from_civil(1970, 1, 1);
        assert_eq!(epoch_days, 0);
    }

    #[test]
    fn round_trip_civil() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));

        let (y2, m2, d2) = civil_from_days(365);
        assert_eq!((y2, m2, d2), (1971, 1, 1));
    }

    #[test]
    fn invalid_date_returns_none() {
        assert_eq!(days_since_iso_date("not-a-date"), None);
        assert_eq!(days_since_iso_date("2024-13-01T00:00:00+00:00"), None);
        assert_eq!(days_since_iso_date(""), None);
    }

    #[test]
    fn past_date_returns_positive_days() {
        // Use a date that's definitely in the past
        let result = days_since_iso_date("2020-01-01T12:00:00+00:00");
        assert!(result.is_some());
        assert!(result.unwrap() > 365); // at least 1 year ago
    }
}
