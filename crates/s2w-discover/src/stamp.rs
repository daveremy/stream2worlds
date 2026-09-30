//! RFC 3339 date-times, a format the profiler may read (decision 0030, under decision 0018's
//! "transports and formats"). [`shaped`] is the profiler's only use of a value's text: a path
//! whose every value has this shape names a moment, never a thing. [`shift`] is the evaluation
//! contract's timestamp obfuscation (one constant shift that keeps the format), for obfuscation
//! replays such as `cargo xtask check` 12 and the unit tests; the profiler never calls it.

/// The constant shift obfuscation replays use: 100 years of 365.25 days and 12,345 seconds, so
/// no shifted date-time from 2001 to 2026 (the recorded fixture's span) equals an original one.
/// `cargo xtask check` 12 and the unit tests share it; the profiler never reads it.
pub const REPLAY_SHIFT: i64 = 3_155_760_000 + 12_345;

/// The fields of one RFC 3339 `date-time` (§5.6). `rest` is the fraction and offset, verbatim.
struct Stamp<'a> {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    sep: u8,
    rest: &'a str,
}

/// Whether `text` is an RFC 3339 `date-time`: `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)`,
/// `T` and `Z` in either case, every field in range, the day valid for its month and year.
#[must_use]
pub fn shaped(text: &str) -> bool {
    parse(text).is_some()
}

/// `text` moved by `seconds`, keeping its format: the fraction, the offset and the letter case
/// are kept verbatim, so shifting the local time moves the instant by the same amount. `None`
/// when `text` is not [`shaped`], has a leap second (60, which a shift cannot keep one-to-one),
/// or would leave years 0000 to 9999. Otherwise one-to-one and order-preserving within one offset.
#[must_use]
pub fn shift(text: &str, seconds: i64) -> Option<String> {
    let s = parse(text)?;
    if s.second == 60 {
        return None;
    }
    let local = days_from_civil(s.year, s.month, s.day) * 86_400
        + s.hour * 3_600
        + s.minute * 60
        + s.second;
    let moved = local.checked_add(seconds)?;
    let (days, secs) = (moved.div_euclid(86_400), moved.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    if !(0..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}{}{:02}:{:02}:{:02}{}",
        char::from(s.sep),
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60,
        s.rest
    ))
}

fn parse(text: &str) -> Option<Stamp<'_>> {
    let b = text.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<i64> {
        let digits = b.get(from..from + len)?;
        digits.iter().try_fold(0_i64, |n, &d| {
            d.is_ascii_digit().then(|| n * 10 + i64::from(d - b'0'))
        })
    };
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    if !(at(4, b'-') && at(7, b'-') && at(13, b':') && at(16, b':')) {
        return None;
    }
    let sep = b[10];
    if sep != b'T' && sep != b't' {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        // 60 is accepted at any minute: `shaped` is a shape test, not a leap-second table.
        || second > 60
    {
        return None;
    }
    let rest = &text[19..];
    let mut offset = rest.as_bytes();
    if offset.first() == Some(&b'.') {
        let digits = offset[1..]
            .iter()
            .take_while(|d| d.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        offset = &offset[1 + digits..];
    }
    let offset_ok = match offset {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            [h1, h2, m1, m2].iter().all(|d| d.is_ascii_digit())
                && (h1 - b'0') * 10 + (h2 - b'0') <= 23
                && (m1 - b'0') * 10 + (m2 - b'0') <= 59
        }
        _ => false,
    };
    offset_ok.then_some(Stamp {
        year,
        month,
        day,
        hour,
        minute,
        second,
        sep,
        rest,
    })
}

fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, days_from_civil, shaped, shift};

    #[test]
    fn shape_accepts_rfc3339_date_times_only() {
        for ok in [
            "2026-09-27T18:16:53Z",
            "2026-09-27T18:16:53.123Z",
            "2026-09-27t18:16:53z",
            "2024-02-29T00:00:00+05:30",
            "1999-12-31T23:59:60-00:00",
            "2026-09-27T18:16:53.1+23:59",
        ] {
            assert!(shaped(ok), "{ok}");
        }
        for bad in [
            "",
            "20260927181653",
            "2026-09-27",
            "2026-09-27 18:16:53Z",
            "2026-09-27T18:16:53",
            "2026-13-01T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-09-27T24:00:00Z",
            "2026-09-27T18:60:00Z",
            "2026-09-27T18:16:61Z",
            "2026-09-27T18:16:53.Z",
            "2026-09-27T18:16:53+24:00",
            "2026-09-27T18:16:53+05:60",
            "2026-09-27T18:16:53+0530",
            "2026-09-27T18:16:53Z ",
            "x2026-09-27T18:16:53Z",
            "2026-09-2٧T18:16:53Z",
        ] {
            assert!(!shaped(bad), "{bad}");
        }
    }

    #[test]
    fn shift_crosses_every_calendar_boundary_and_keeps_the_format() {
        let day = 86_400;
        assert_eq!(
            shift("2026-12-31T23:59:59.500Z", 1).as_deref(),
            Some("2027-01-01T00:00:00.500Z")
        );
        assert_eq!(
            shift("2024-02-28t12:00:00+05:30", day).as_deref(),
            Some("2024-02-29t12:00:00+05:30")
        );
        assert_eq!(
            shift("2023-02-28T12:00:00z", day).as_deref(),
            Some("2023-03-01T12:00:00z")
        );
        assert_eq!(
            shift("2000-03-01T00:00:00Z", -day).as_deref(),
            Some("2000-02-29T00:00:00Z")
        );
        assert_eq!(
            shift("1970-01-01T00:00:00Z", -1).as_deref(),
            Some("1969-12-31T23:59:59Z")
        );
        assert_eq!(shift("1999-12-31T23:59:60Z", 1), None);
        assert_eq!(shift("9999-12-31T23:59:59Z", 1), None);
        assert_eq!(shift("0000-01-01T00:00:00Z", -1), None);
        assert_eq!(shift("not a stamp", 1), None);
    }

    #[test]
    fn shift_keeps_order_and_is_one_to_one() {
        let c = super::REPLAY_SHIFT;
        let mut stamps: Vec<String> = (0..2_000_i64)
            .map(|i| {
                let t = i * 7_919_993 - 3_000_000_000;
                let (y, m, d) = civil_from_days(t.div_euclid(86_400));
                let s = t.rem_euclid(86_400);
                format!(
                    "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
                    s / 3_600,
                    s % 3_600 / 60,
                    s % 60
                )
            })
            .collect();
        stamps.sort();
        let moved: Vec<String> = stamps.iter().map(|s| shift(s, c).unwrap()).collect();
        assert!(moved.windows(2).all(|w| w[0] < w[1]));
        assert!(moved.iter().all(|s| shaped(s)));
        let back: Vec<String> = moved.iter().map(|s| shift(s, -c).unwrap()).collect();
        assert_eq!(back, stamps);
    }

    #[test]
    fn civil_days_round_trip() {
        for days in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }
}
