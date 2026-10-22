//! Date / time arguments of MessageFormatter.
//!
//! Formatting goes through IntlDateFormatter's SimpleDateFormat
//! implementation (dateformat.rs); until that module provides
//! `format_date_pattern()` / `style_pattern()`, the `fallback` functions
//! below (same signatures) stand in, with English names only.
//!
//! Parsing (MessageFormatter::parse() of a date argument) is a small
//! SimpleDateFormat parser for numeric fields, eras, AM/PM, month and day
//! names and GMT offsets.

use goro_core::vm::Vm;

use crate::datecore::{self, CalInfo};

/// SimpleDateFormat::format() with `pattern` for `locale`.
pub fn format_date_pattern(vm: &mut Vm, locale: &str, pattern: &str, info: &CalInfo) -> String {
    crate::dateformat::format_date_pattern(vm, locale, pattern, info)
}

/// The pattern of DateFormat::createDateTimeInstance(date_style,
/// time_style) for `locale` (IntlDateFormatter::FULL..SHORT, NONE = -1).
pub fn style_pattern(locale: &str, date_style: i64, time_style: i64) -> String {
    crate::dateformat::style_pattern(locale, date_style, time_style)
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub mod fallback {
    use super::*;

    pub fn style_pattern(_locale: &str, date_style: i64, time_style: i64) -> String {
        let d = match date_style {
            0 => "EEEE, MMMM d, y",
            1 => "MMMM d, y",
            2 => "MMM d, y",
            3 => "M/d/yy",
            _ => "",
        };
        let t = match time_style {
            0 => "h:mm:ss\u{202f}a zzzz",
            1 => "h:mm:ss\u{202f}a z",
            2 => "h:mm:ss\u{202f}a",
            3 => "h:mm\u{202f}a",
            _ => "",
        };
        match (d.is_empty(), t.is_empty()) {
            (false, false) => {
                let sep = if date_style <= 1 { " 'at' " } else { ", " };
                format!("{}{}{}", d, sep, t)
            }
            (false, true) => d.to_string(),
            (true, false) => t.to_string(),
            _ => String::new(),
        }
    }

    fn pad(n: i64, w: usize) -> String {
        let s = n.unsigned_abs().to_string();
        let mut out = String::new();
        if n < 0 {
            out.push('-');
        }
        for _ in s.len()..w {
            out.push('0');
        }
        out.push_str(&s);
        out
    }

    fn gmt(off_ms: i32, long: bool) -> String {
        if off_ms == 0 {
            return "GMT".to_string();
        }
        let m = off_ms / 60_000;
        let sign = if m < 0 { '-' } else { '+' };
        let m = m.abs();
        if long || m % 60 != 0 {
            format!("GMT{}{:02}:{:02}", sign, m / 60, m % 60)
        } else {
            format!("GMT{}{}", sign, m / 60)
        }
    }

    pub fn format_date_pattern(
        _vm: &mut Vm,
        _locale: &str,
        pattern: &str,
        info: &CalInfo,
    ) -> String {
        let f = datecore::compute_fields(info);
        let cs: Vec<char> = pattern.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '\'' {
                if cs.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < cs.len() {
                    if cs[i] == '\'' {
                        if cs.get(i + 1) == Some(&'\'') {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    out.push(cs[i]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            if !c.is_ascii_alphabetic() {
                out.push(c);
                i += 1;
                continue;
            }
            let mut n = 1;
            while i + n < cs.len() && cs[i + n] == c {
                n += 1;
            }
            i += n;
            let off = f[datecore::ZONE_OFFSET] + f[datecore::DST_OFFSET];
            let s = match c {
                'G' => {
                    let ad = f[datecore::ERA] == 1;
                    match n {
                        4 => if ad { "Anno Domini" } else { "Before Christ" }.to_string(),
                        5 => if ad { "A" } else { "B" }.to_string(),
                        _ => if ad { "AD" } else { "BC" }.to_string(),
                    }
                }
                'y' | 'Y' | 'u' => {
                    let y = if c == 'u' {
                        f[datecore::EXTENDED_YEAR]
                    } else if c == 'Y' {
                        f[datecore::YEAR_WOY]
                    } else {
                        f[datecore::YEAR]
                    } as i64;
                    if n == 2 { pad(y % 100, 2) } else { pad(y, n) }
                }
                'M' | 'L' => {
                    let m = f[datecore::MONTH] as usize;
                    match n {
                        1 | 2 => pad(m as i64 + 1, n),
                        3 => MONTHS[m][..3].to_string(),
                        5 => MONTHS[m][..1].to_string(),
                        _ => MONTHS[m].to_string(),
                    }
                }
                'd' => pad(f[datecore::DATE] as i64, n),
                'D' => pad(f[datecore::DAY_OF_YEAR] as i64, n),
                'E' | 'e' | 'c' => {
                    let d = (f[datecore::DAY_OF_WEEK] - 1).clamp(0, 6) as usize;
                    if (c == 'e' || c == 'c') && n <= 2 {
                        pad(f[datecore::DOW_LOCAL] as i64, n)
                    } else {
                        match n {
                            4 => DAYS[d].to_string(),
                            5 => DAYS[d][..1].to_string(),
                            6 => DAYS[d][..2].to_string(),
                            _ => DAYS[d][..3].to_string(),
                        }
                    }
                }
                'a' | 'b' | 'B' => if f[datecore::AM_PM] == 0 { "AM" } else { "PM" }.to_string(),
                'h' => {
                    let h = f[datecore::HOUR];
                    pad(if h == 0 { 12 } else { h } as i64, n)
                }
                'H' => pad(f[datecore::HOUR_OF_DAY] as i64, n),
                'K' => pad(f[datecore::HOUR] as i64, n),
                'k' => {
                    let h = f[datecore::HOUR_OF_DAY];
                    pad(if h == 0 { 24 } else { h } as i64, n)
                }
                'm' => pad(f[datecore::MINUTE] as i64, n),
                's' => pad(f[datecore::SECOND] as i64, n),
                'S' => {
                    let ms = format!("{:03}", f[datecore::MILLISECOND]);
                    let mut s: String = ms.chars().take(n).collect();
                    while s.len() < n {
                        s.push('0');
                    }
                    s
                }
                'A' => pad(f[datecore::MILLISECONDS_IN_DAY] as i64, n),
                'w' => pad(f[datecore::WEEK_OF_YEAR] as i64, n),
                'W' => pad(f[datecore::WEEK_OF_MONTH] as i64, n),
                'F' => pad(f[datecore::DAY_OF_WEEK_IN_MONTH] as i64, n),
                'z' | 'O' | 'v' => gmt(off, n >= 4),
                'Z' => {
                    if n == 4 {
                        gmt(off, true)
                    } else {
                        let m = off / 60_000;
                        let sign = if m < 0 { '-' } else { '+' };
                        let m = m.abs();
                        format!("{}{:02}{:02}", sign, m / 60, m % 60)
                    }
                }
                'X' | 'x' => {
                    let m = off / 60_000;
                    if m == 0 && c == 'X' {
                        "Z".to_string()
                    } else {
                        let sign = if m < 0 { '-' } else { '+' };
                        let m = m.abs();
                        match n {
                            1 => format!("{}{:02}", sign, m / 60),
                            2 | 4 => format!("{}{:02}{:02}", sign, m / 60, m % 60),
                            _ => format!("{}{:02}:{:02}", sign, m / 60, m % 60),
                        }
                    }
                }
                'V' => info.tz_id.clone(),
                _ => String::new(),
            };
            out.push_str(&s);
        }
        out
    }
}

/// Parse a date with a SimpleDateFormat pattern from `pos`: (UDate millis,
/// end) or None. Lenient about the field widths, as ICU's lenient parse.
pub fn parse_date_pattern(
    pattern: &str,
    text: &[char],
    pos: usize,
    tz_id: &str,
) -> Option<(f64, usize)> {
    let cs: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut p = pos;
    let mut year: Option<i64> = None;
    let mut month: i64 = 0;
    let mut day: i64 = 1;
    let mut hour: i64 = 0;
    let mut pm: Option<bool> = None;
    let mut hour12 = false;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut millis: i64 = 0;
    let mut bc = false;
    let mut offset_ms: Option<i64> = None;
    let read_num = |p: &mut usize, max: usize| -> Option<i64> {
        let st = *p;
        let mut neg = false;
        if *p < text.len() && text[*p] == '-' {
            neg = true;
            *p += 1;
        }
        let mut v: i64 = 0;
        let mut n = 0;
        while *p < text.len() && text[*p].is_ascii_digit() && n < max {
            v = v * 10 + (text[*p] as u8 - b'0') as i64;
            *p += 1;
            n += 1;
        }
        if n == 0 {
            *p = st;
            return None;
        }
        Some(if neg { -v } else { v })
    };
    let match_ci = |p: &mut usize, w: &str| -> bool {
        let wc: Vec<char> = w.chars().collect();
        if *p + wc.len() > text.len() {
            return false;
        }
        for (k, c) in wc.iter().enumerate() {
            if !text[*p + k].to_lowercase().eq(c.to_lowercase()) {
                return false;
            }
        }
        *p += wc.len();
        true
    };
    while i < cs.len() {
        let c = cs[i];
        if c == '\'' {
            let mut lit = String::new();
            if cs.get(i + 1) == Some(&'\'') {
                lit.push('\'');
                i += 2;
            } else {
                i += 1;
                while i < cs.len() {
                    if cs[i] == '\'' {
                        if cs.get(i + 1) == Some(&'\'') {
                            lit.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    lit.push(cs[i]);
                    i += 1;
                }
                i += 1;
            }
            for ch in lit.chars() {
                if p < text.len() && text[p] == ch {
                    p += 1;
                } else {
                    return None;
                }
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            if c.is_whitespace() || c == '\u{202f}' {
                while p < text.len() && (text[p].is_whitespace() || text[p] == '\u{202f}') {
                    p += 1;
                }
            } else if p < text.len() && text[p] == c {
                p += 1;
            } else {
                return None;
            }
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < cs.len() && cs[i + n] == c {
            n += 1;
        }
        i += n;
        // an adjacent numeric field limits the width
        let next_numeric = cs.get(i).is_some_and(|x| x.is_ascii_alphabetic());
        let width = if next_numeric { n.max(1) } else { 10 };
        match c {
            'G' => {
                if match_ci(&mut p, "AD")
                    || match_ci(&mut p, "Anno Domini")
                    || match_ci(&mut p, "CE")
                {
                } else if match_ci(&mut p, "BC")
                    || match_ci(&mut p, "Before Christ")
                    || match_ci(&mut p, "BCE")
                {
                    bc = true;
                } else {
                    return None;
                }
            }
            'y' | 'Y' | 'u' => {
                let st = p;
                let v = read_num(&mut p, width)?;
                year = Some(if n == 2 && p - st <= 2 { 2000 + v } else { v });
            }
            'M' | 'L' => {
                if n <= 2 {
                    month = read_num(&mut p, width)? - 1;
                } else {
                    let mut found = false;
                    for (k, m) in MONTHS.iter().enumerate() {
                        if match_ci(&mut p, m) || match_ci(&mut p, &m[..3]) {
                            month = k as i64;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return None;
                    }
                }
            }
            'd' => day = read_num(&mut p, width)?,
            'E' | 'e' | 'c' => {
                let mut found = false;
                for d in DAYS.iter() {
                    if match_ci(&mut p, d) || match_ci(&mut p, &d[..3]) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return None;
                }
            }
            'a' => {
                if match_ci(&mut p, "AM") {
                    pm = Some(false);
                } else if match_ci(&mut p, "PM") {
                    pm = Some(true);
                } else {
                    return None;
                }
            }
            'h' | 'K' => {
                hour = read_num(&mut p, width)?;
                hour12 = true;
            }
            'H' | 'k' => hour = read_num(&mut p, width)?,
            'm' => minute = read_num(&mut p, width)?,
            's' => second = read_num(&mut p, width)?,
            'S' => {
                let st = p;
                let v = read_num(&mut p, width)?;
                let digits = (p - st) as u32;
                millis = if digits >= 3 {
                    v / 10i64.pow(digits - 3)
                } else {
                    v * 10i64.pow(3 - digits)
                };
            }
            'z' | 'Z' | 'O' | 'v' | 'V' | 'X' | 'x' => {
                if match_ci(&mut p, "GMT") || match_ci(&mut p, "UTC") || match_ci(&mut p, "UT") {
                    offset_ms = Some(0);
                }
                if p < text.len() && (text[p] == '+' || text[p] == '-') {
                    let neg = text[p] == '-';
                    p += 1;
                    let h = read_num(&mut p, 2)?;
                    let mut m = 0;
                    if p < text.len() && text[p] == ':' {
                        p += 1;
                        m = read_num(&mut p, 2)?;
                    } else if p + 1 < text.len() && text[p].is_ascii_digit() {
                        m = read_num(&mut p, 2)?;
                    }
                    let o = (h * 60 + m) * 60_000;
                    offset_ms = Some(if neg { -o } else { o });
                } else if offset_ms.is_none() {
                    if p < text.len() && text[p] == 'Z' {
                        p += 1;
                        offset_ms = Some(0);
                    } else {
                        return None;
                    }
                }
            }
            _ => {
                read_num(&mut p, width)?;
            }
        }
    }
    let mut y = year.unwrap_or(1970);
    if bc {
        y = 1 - y;
    }
    if hour12 {
        if hour == 12 {
            hour = 0;
        }
        if pm == Some(true) {
            hour += 12;
        }
    } else if pm == Some(true) && hour < 12 {
        hour += 12;
    }
    let days = datecore::days_from_civil(y, month + 1, day);
    let local = days as f64 * 86_400_000.0
        + (hour * 3_600_000 + minute * 60_000 + second * 1000 + millis) as f64;
    let ms = match offset_ms {
        Some(o) => local - o as f64,
        None => {
            let info = CalInfo::gregorian(local, tz_id, "");
            let mut f = datecore::compute_fields(&info);
            f[datecore::EXTENDED_YEAR] = y as i32;
            f[datecore::MONTH] = month as i32;
            f[datecore::DATE] = day as i32;
            f[datecore::HOUR_OF_DAY] = hour as i32;
            f[datecore::MINUTE] = minute as i32;
            f[datecore::SECOND] = second as i32;
            f[datecore::MILLISECOND] = millis as i32;
            datecore::fields_to_millis(&info, &f)
        }
    };
    Some((ms, p))
}
