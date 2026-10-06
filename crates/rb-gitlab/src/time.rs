use rb_core::Timestamp;

/// Parses GitLab's `2026-01-31T14:05:09Z` (a numeric offset is also accepted).
pub(crate) fn parse_rfc3339(s: &str) -> Option<Timestamp> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (oh, om) = (
                rest.get(1..3)?.parse::<i64>().ok()?,
                rest.get(4..6)?.parse::<i64>().ok()?,
            );
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(Timestamp(
        days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset,
    ))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_and_offsets() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(Timestamp(0)));
        assert_eq!(
            parse_rfc3339("2023-11-14T22:13:20Z"),
            Some(Timestamp(1_700_000_000))
        );
        assert_eq!(
            parse_rfc3339("2023-11-14T23:13:20.500+01:00"),
            Some(Timestamp(1_700_000_000))
        );
        assert_eq!(
            parse_rfc3339("2024-02-29T00:00:00Z"),
            Some(Timestamp(1_709_164_800))
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_rfc3339("yesterday"), None);
        assert_eq!(parse_rfc3339("2023-13-14T22:13:20Z"), None);
        assert_eq!(parse_rfc3339("2023-11-14T22:13:20"), None);
    }
}
