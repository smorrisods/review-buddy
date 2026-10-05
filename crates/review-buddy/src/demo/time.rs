//! A tiny ISO 8601 reader for `--frozen-time`, so the binary needs no date crate.

use rb_core::Timestamp;

/// The default frozen time: the date on the design board.
pub const DEFAULT_FROZEN: &str = "2026-10-05T10:00";

/// Parses `YYYY-MM-DDTHH:MM[:SS][Z]` (UTC). A space may stand in for the `T`, and the time may
/// be left out for midnight.
pub fn parse_iso(text: &str) -> Option<Timestamp> {
    let text = text.trim().trim_end_matches(['Z', 'z']);
    let (date, time) = match text.split_once(['T', 't', ' ']) {
        Some((date, time)) => (date, time),
        None => (text, "00:00"),
    };
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if d.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=days_in(year, month)).contains(&day)
    {
        return None;
    }
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next().map_or(Some(0), |s| s.parse().ok())?;
    if t.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(Timestamp(
        days * 86_400 + hour * 3_600 + minute * 60 + second,
    ))
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_instants() {
        assert_eq!(parse_iso("1970-01-01T00:00"), Some(Timestamp(0)));
        assert_eq!(
            parse_iso("2000-03-01T00:00:00Z"),
            Some(Timestamp(951_868_800))
        );
        assert_eq!(
            parse_iso("2026-10-05T10:00"),
            Some(Timestamp(1_791_194_400))
        );
    }

    #[test]
    fn accepts_space_separator_and_date_only() {
        assert_eq!(
            parse_iso("2026-10-05 10:00:30"),
            parse_iso("2026-10-05T10:00:30")
        );
        assert_eq!(parse_iso("2026-10-05"), parse_iso("2026-10-05T00:00"));
    }

    #[test]
    fn rejects_nonsense() {
        for bad in [
            "",
            "tomorrow",
            "2026-13-01T00:00",
            "2026-02-30",
            "2026-10-05T25:00",
            "2026-10-05T10",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad}");
        }
    }

    #[test]
    fn leap_days_count() {
        assert!(parse_iso("2028-02-29").is_some());
        assert!(parse_iso("2027-02-29").is_none());
    }

    #[test]
    fn default_frozen_time_parses() {
        assert!(parse_iso(DEFAULT_FROZEN).is_some());
    }
}
