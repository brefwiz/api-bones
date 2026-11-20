// SPDX-License-Identifier: MIT
//! RFC 3339 text of a `google.protobuf.Timestamp`, as the proto3 JSON mapping
//! writes it: UTC with a `Z` suffix and 0, 3, 6 or 9 fraction digits.
//!
//! The range is the protobuf one, `0001-01-01T00:00:00Z` through
//! `9999-12-31T23:59:59.999999999Z`.

use crate::error::timestamp_in_range as in_range;

// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The RFC 3339 text, or `None` when out of the protobuf range.
#[must_use]
pub fn format(seconds: i64, nanos: i32) -> Option<String> {
    if !in_range(seconds, nanos) {
        return None;
    }
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rem = seconds.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let fraction = if nanos == 0 {
        String::new()
    } else if nanos % 1_000_000 == 0 {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos % 1000 == 0 {
        format!(".{:06}", nanos / 1000)
    } else {
        format!(".{nanos:09}")
    };
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}{fraction}Z"
    ))
}

fn digits(bytes: &[u8], at: usize, n: usize) -> Option<i64> {
    let slice = bytes.get(at..at + n)?;
    if !slice.iter().all(u8::is_ascii_digit) {
        return None;
    }
    slice
        .iter()
        .try_fold(0_i64, |acc, d| Some(acc * 10 + i64::from(d - b'0')))
}

/// Parse RFC 3339 text with a `Z` or `+hh:mm` / `-hh:mm` offset and an
/// optional fraction of up to nine digits. `None` when malformed or out of
/// the protobuf range.
#[must_use]
pub fn parse(text: &str) -> Option<(i64, i32)> {
    let b = text.as_bytes();
    let (year, month, day) = (digits(b, 0, 4)?, digits(b, 5, 2)?, digits(b, 8, 2)?);
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || b.get(10) != Some(&b'T') {
        return None;
    }
    let (h, m, s) = (digits(b, 11, 2)?, digits(b, 14, 2)?, digits(b, 17, 2)?);
    if b.get(13) != Some(&b':') || b.get(16) != Some(&b':') {
        return None;
    }
    if !(1..=12).contains(&month) || h > 23 || m > 59 || s > 59 || !(1..=31).contains(&day) {
        return None;
    }
    let mut at = 19;
    let mut nanos = 0_i32;
    if b.get(at) == Some(&b'.') {
        let start = at + 1;
        let mut end = start;
        while b.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        let len = end - start;
        if len == 0 || len > 9 {
            return None;
        }
        let value = digits(b, start, len)?;
        nanos = i32::try_from(value * 10_i64.pow(u32::try_from(9 - len).ok()?)).ok()?;
        at = end;
    }
    let offset = match b.get(at)? {
        b'Z' => {
            at += 1;
            0
        }
        sign @ (b'+' | b'-') => {
            let (oh, om) = (digits(b, at + 1, 2)?, digits(b, at + 4, 2)?);
            if b.get(at + 3) != Some(&b':') || oh > 23 || om > 59 {
                return None;
            }
            at += 6;
            let total = oh * 3600 + om * 60;
            if *sign == b'+' { total } else { -total }
        }
        _ => return None,
    };
    if at != b.len() {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // Reject a day that does not exist in its month.
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let seconds = days * 86_400 + h * 3600 + m * 60 + s - offset;
    in_range(seconds, nanos).then_some((seconds, nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{
        TIMESTAMP_MAX_SECONDS as MAX_SECONDS, TIMESTAMP_MIN_SECONDS as MIN_SECONDS,
    };

    #[test]
    fn epoch_and_bounds_format() {
        assert_eq!(format(0, 0).as_deref(), Some("1970-01-01T00:00:00Z"));
        assert_eq!(
            format(MIN_SECONDS, 0).as_deref(),
            Some("0001-01-01T00:00:00Z")
        );
        assert_eq!(
            format(MAX_SECONDS, 999_999_999).as_deref(),
            Some("9999-12-31T23:59:59.999999999Z")
        );
        assert_eq!(format(MAX_SECONDS + 1, 0), None);
        assert_eq!(format(0, -1), None);
        assert_eq!(format(0, 1_000_000_000), None);
    }

    #[test]
    fn fraction_width_follows_precision() {
        assert_eq!(
            format(1, 500_000_000).as_deref(),
            Some("1970-01-01T00:00:01.500Z")
        );
        assert_eq!(
            format(1, 500_000).as_deref(),
            Some("1970-01-01T00:00:01.000500Z")
        );
        assert_eq!(
            format(1, 5).as_deref(),
            Some("1970-01-01T00:00:01.000000005Z")
        );
    }

    #[test]
    fn negative_seconds_floor_into_the_previous_day() {
        assert_eq!(
            format(-1, 500_000_000).as_deref(),
            Some("1969-12-31T23:59:59.500Z")
        );
        assert_eq!(parse("1969-12-31T23:59:59.500Z"), Some((-1, 500_000_000)));
    }

    #[test]
    fn parse_round_trips_format() {
        for (s, n) in [
            (0, 0),
            (1_700_000_000, 123_456_789),
            (-86_401, 1),
            (MIN_SECONDS, 0),
            (MAX_SECONDS, 999_999_999),
        ] {
            let text = format(s, n).unwrap();
            assert_eq!(parse(&text), Some((s, n)), "{text}");
        }
    }

    #[test]
    fn parse_applies_the_offset() {
        assert_eq!(parse("1970-01-01T01:00:00+01:00"), Some((0, 0)));
        assert_eq!(parse("1969-12-31T23:00:00-01:00"), Some((0, 0)));
    }

    #[test]
    fn parse_rejects_malformed_text() {
        for bad in [
            "",
            "1970-01-01",
            "1970-01-01T00:00:00",
            "1970-01-01 00:00:00Z",
            "1970-13-01T00:00:00Z",
            "1970-02-30T00:00:00Z",
            "1970-01-01T24:00:00Z",
            "1970-01-01T00:00:00.Z",
            "1970-01-01T00:00:00.1234567890Z",
            "1970-01-01T00:00:00Zjunk",
            "0000-12-31T23:59:59Z",
            "10000-01-01T00:00:00Z",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
