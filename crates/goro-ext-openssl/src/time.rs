//! Civil-time helpers for ASN.1 UTCTime / GeneralizedTime (X.680 §46-47,
//! RFC 5280 §4.1.2.5).

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (year, month, day, hour, minute, second) of a Unix time.
pub fn civil_from_unix(t: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, secs / 3600, (secs / 60) % 60, secs % 60)
}

/// The Unix time of an ASN.1 time's text (`generalized` for
/// GeneralizedTime). Accepts the optional seconds, fractional seconds and
/// `+hhmm`/`-hhmm` offsets that BER allows; none when malformed.
pub fn parse_asn1_time(s: &[u8], generalized: bool) -> Option<i64> {
    let digits = |b: &[u8]| -> Option<i64> {
        if b.iter().all(|c| c.is_ascii_digit()) {
            Some(b.iter().fold(0i64, |a, c| a * 10 + (c - b'0') as i64))
        } else {
            None
        }
    };
    let mut i;
    let year = if generalized {
        i = 4;
        digits(s.get(0..4)?)?
    } else {
        i = 2;
        let y = digits(s.get(0..2)?)?;
        if y < 50 { 2000 + y } else { 1900 + y }
    };
    let mon = digits(s.get(i..i + 2)?)?;
    let day = digits(s.get(i + 2..i + 4)?)?;
    let hour = digits(s.get(i + 4..i + 6)?)?;
    let min = digits(s.get(i + 6..i + 8)?)?;
    i += 8;
    let mut sec = 0;
    if s.len() >= i + 2 && s[i].is_ascii_digit() {
        sec = digits(&s[i..i + 2])?;
        i += 2;
    }
    if generalized && i < s.len() && (s[i] == b'.' || s[i] == b',') {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    if !(1..=12).contains(&mon) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    let mut t = days_from_civil(year, mon, day) * 86400 + hour * 3600 + min * 60 + sec;
    match s.get(i) {
        Some(b'Z') if i + 1 == s.len() => {}
        Some(&c) if (c == b'+' || c == b'-') && s.len() == i + 5 => {
            let oh = digits(&s[i + 1..i + 3])?;
            let om = digits(&s[i + 3..i + 5])?;
            let off = oh * 3600 + om * 60;
            if c == b'+' {
                t -= off;
            } else {
                t += off;
            }
        }
        None if generalized => {}
        _ => return None,
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        assert_eq!(parse_asn1_time(b"080630102843Z", false), Some(1214821723));
        assert_eq!(civil_from_unix(1214821723), (2008, 6, 30, 10, 28, 43));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }
}
