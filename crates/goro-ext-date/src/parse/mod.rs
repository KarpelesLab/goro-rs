//! Parsing of free-form date/time strings: `strtotime()`, the DateTime
//! constructor, `modify()` and `date_parse()`.
//!
//! The grammar is the PHP manual's "Supported Date and Time Formats"
//! (see [`grammar`]); the scanner takes the longest matching format at each
//! position, as that page's tables read, and every behaviour detail
//! (which fields a format sets, the error and warning texts and their
//! positions) was checked against `date_parse()` on PHP 8.5.

mod grammar;
pub(crate) mod pat;

use crate::tz::{self, TimeZone};
use grammar::Rule;

/// A relative weekday/weekday-count adjustment kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Special {
    /// None.
    #[default]
    None,
    /// "+N weekdays": skip weekends.
    Weekdays(i64),
    /// "Nth dayname of": day-of-week in the month (`day` holds 7*(n-1)).
    DayOfWeekInMonth,
    /// "last dayname of".
    LastDayOfWeekInMonth,
}

/// How a relative weekday is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeekdayBehavior {
    /// Move forward to the day unless today is that day ("monday",
    /// "+1 monday").
    #[default]
    Forward,
    /// Move forward to the day, a week ahead when today is that day
    /// ("next monday").
    Strict,
    /// Within the ISO week ("monday next week", "this week").
    Week,
}

/// The relative part of a parsed string (`date_parse()['relative']`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Relative {
    /// Years.
    pub y: i64,
    /// Months.
    pub m: i64,
    /// Days.
    pub d: i64,
    /// Hours.
    pub h: i64,
    /// Minutes.
    pub i: i64,
    /// Seconds.
    pub s: i64,
    /// Microseconds.
    pub us: i64,
    /// Relative weekday (0 = Sunday … 6, 7 for "weekday" counts), if any.
    pub weekday: Option<i64>,
    /// How to resolve `weekday`.
    pub weekday_behavior: WeekdayBehavior,
    /// Special relative kind.
    pub special: Special,
    /// 1 for "first day of", 2 for "last day of".
    pub first_last_day_of: u8,
}

/// The zone found in a string.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedZone {
    /// A zone (offset, abbreviation or identifier).
    Zone(TimeZone),
    /// A zone name that is not in the database (`zone_type` 0).
    NotFound,
}

/// The result of parsing a date/time string, as `date_parse()` reports
/// it, plus what DateTime needs to resolve it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    /// Year.
    pub y: Option<i64>,
    /// Month.
    pub m: Option<i64>,
    /// Day.
    pub d: Option<i64>,
    /// Hour.
    pub h: Option<i64>,
    /// Minute.
    pub i: Option<i64>,
    /// Second.
    pub s: Option<i64>,
    /// Microseconds (`fraction` × 1e6).
    pub us: Option<i64>,
    /// The zone, if the string gave one.
    pub zone: Option<ParsedZone>,
    /// The relative part, if any.
    pub relative: Option<Relative>,
    /// Warnings: (position, message).
    pub warnings: Vec<(usize, String)>,
    /// Errors: (position, message).
    pub errors: Vec<(usize, String)>,
    /// Whether a date was given.
    pub have_date: bool,
    /// Whether a time was given.
    pub have_time: bool,
    /// A four-digit number after a time was read as the year.
    time_year_used: bool,
    /// An "@timestamp" was read.
    saw_timestamp: bool,
    zone_count: u32,
}

impl Parsed {
    fn rel(&mut self) -> &mut Relative {
        self.relative.get_or_insert_with(Relative::default)
    }

    fn error(&mut self, pos: usize, msg: &str) {
        self.errors.push((pos, msg.to_string()));
    }

    fn warning(&mut self, pos: usize, msg: &str) {
        self.warnings.push((pos, msg.to_string()));
    }

    /// Marks the time as set; errors on a second time.
    fn have_time(&mut self, pos: usize) -> bool {
        if self.have_time {
            self.error(pos, "Double time specification");
            return false;
        }
        self.have_time = true;
        self.h = Some(0);
        self.i = Some(0);
        self.s = Some(0);
        self.us = Some(0);
        true
    }

    /// Clears the time (formats that reset it: "today", day names).
    fn unhave_time(&mut self) {
        self.have_time = false;
        self.time_year_used = false;
        self.h = Some(0);
        self.i = Some(0);
        self.s = Some(0);
        self.us = Some(0);
    }

    fn have_date(&mut self, pos: usize) -> bool {
        if self.have_date {
            self.error(pos, "Double date specification");
            return false;
        }
        self.have_date = true;
        true
    }

    fn unhave_date(&mut self) {
        self.have_date = false;
        self.y = Some(0);
        self.m = Some(0);
        self.d = Some(0);
    }

    /// Marks the zone as set. The first repeat is a warning, later ones
    /// errors (observed on PHP 8.5).
    fn have_zone(&mut self, pos: usize) -> bool {
        self.zone_count += 1;
        match self.zone_count {
            1 => true,
            2 => {
                self.warning(pos, "Double timezone specification");
                false
            }
            _ => {
                self.error(pos, "Double timezone specification");
                false
            }
        }
    }

    /// A result holding only a relative part (for intervals made by
    /// `DateInterval::createFromDateString()`).
    pub fn from_relative(r: Relative) -> Parsed {
        Parsed {
            relative: Some(r),
            ..Default::default()
        }
    }

    /// Whether the string is an "@timestamp" with no time given after it
    /// (`modify()` then switches to its UTC offset).
    pub fn timestamp_only(&self) -> bool {
        self.saw_timestamp && !self.have_time
    }

    /// `date_parse()`'s `is_localtime`.
    pub fn is_localtime(&self) -> bool {
        self.zone.is_some()
    }

    /// `date_parse()`'s `zone_type` (0 for an unknown zone name).
    pub fn zone_type(&self) -> Option<u8> {
        match &self.zone {
            None => None,
            Some(ParsedZone::NotFound) => Some(0),
            Some(ParsedZone::Zone(z)) => Some(z.kind()),
        }
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c | b',' | b'.')
}

/// Parses a date/time string the way `date_parse()` and `strtotime()` do.
pub fn parse(input: &[u8]) -> Parsed {
    let mut t = Parsed::default();
    // the string ends at a NUL byte
    let s = match input.iter().position(|&c| c == 0) {
        Some(n) => &input[..n],
        None => input,
    };
    // Surrounding white space is dropped first: positions in messages
    // count from the first non-blank byte (observed on PHP 8.5).
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    let stop = s.iter().rposition(|c| !ws(c)).map_or(start, |e| e + 1);
    let s = &s[start..stop.max(start)];
    if input.is_empty() {
        t.error(0, "Empty string");
        return t;
    }
    let mut p = 0;
    while p < s.len() {
        if is_space(s[p]) {
            p += 1;
            continue;
        }
        let mut best: Option<(usize, Rule)> = None;
        for &rule in grammar::RULES {
            if let Some(n) = grammar::match_rule(rule, s, p)
                && n > 0
                && best.is_none_or(|(bn, _)| n > bn)
            {
                best = Some((n, rule));
            }
        }
        match best {
            None => {
                t.error(p, "Unexpected character");
                p += 1;
            }
            Some((n, rule)) => {
                let end = action(&mut t, rule, s, p, p + n);
                p = end.max(p + 1);
            }
        }
    }
    finish(&mut t, s.len());
    t
}

fn finish(t: &mut Parsed, len: usize) {
    // validity warnings are reported past the end of the string
    // Only one of the two warnings is given, the date's first (observed on
    // PHP 8.5). A lone year ("1978") does not count as a date here.
    let mut date_bad = false;
    if t.have_date {
        let y = t.y.unwrap_or(-9_999_999);
        let valid = match (t.m, t.d) {
            (Some(m), Some(d)) => {
                (1..=12).contains(&m)
                    && d >= 1
                    && d <= crate::civil::days_in_month(y, m as u32) as i64
            }
            _ => false,
        };
        if !valid {
            t.warning(len + 1, "The parsed date was invalid");
            date_bad = true;
        }
    }
    if let (false, true, Some(h), Some(i), Some(s)) = (date_bad, t.have_time, t.h, t.i, t.s)
        && (!(0..=23).contains(&h) || !(0..=59).contains(&i) || !(0..=59).contains(&s))
    {
        t.warning(len + 1, "The parsed time was invalid");
    }
}

/// Reads up to `max` digits at or after `*p` (skipping non-digits),
/// stopping at `end`.
fn get_nr(s: &[u8], p: &mut usize, end: usize, max: usize) -> Option<i64> {
    while *p < end && !s[*p].is_ascii_digit() {
        *p += 1;
    }
    let start = *p;
    let mut v: i64 = 0;
    while *p < end && *p - start < max && s[*p].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add((s[*p] - b'0') as i64);
        *p += 1;
    }
    if *p == start { None } else { Some(v) }
}

/// Like [`get_nr`] but also returns the digit count.
fn get_nr_len(s: &[u8], p: &mut usize, end: usize, max: usize) -> Option<(i64, usize)> {
    while *p < end && !s[*p].is_ascii_digit() {
        *p += 1;
    }
    let start = *p;
    let v = get_nr(s, p, end, max)?;
    Some((v, *p - start))
}

/// Two-digit years: 0-69 are 2000-2069, 70-99 are 1970-1999 (PHP
/// manual: "Supported Date and Time Formats", note on two digit years).
fn process_year(y: i64, len: usize) -> i64 {
    if len >= 4 {
        return y;
    }
    if y < 70 {
        y + 2000
    } else if y < 100 {
        y + 1900
    } else {
        y
    }
}

/// Skips to the next letter and reads a month name (full, abbreviated or
/// roman).
fn get_month(s: &[u8], p: &mut usize, end: usize) -> Option<i64> {
    while *p < end && !s[*p].is_ascii_alphabetic() {
        *p += 1;
    }
    let start = *p;
    while *p < end && s[*p].is_ascii_alphabetic() {
        *p += 1;
    }
    let w = &s[start..*p];
    month_from_name(w)
}

pub(crate) fn month_from_name(w: &[u8]) -> Option<i64> {
    for (i, n) in grammar::MONTH_FULL.iter().enumerate() {
        if w.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(i as i64 + 1);
        }
    }
    for n in grammar::MONTH_ABBR {
        if w.eq_ignore_ascii_case(n.as_bytes()) {
            let i = grammar::MONTH_FULL
                .iter()
                .position(|f| f.starts_with(&n[..3]))
                .unwrap();
            return Some(i as i64 + 1);
        }
    }
    for (i, n) in grammar::MONTH_ROMAN.iter().enumerate() {
        if w.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(i as i64 + 1);
        }
    }
    None
}

fn day_from_name(w: &[u8]) -> Option<i64> {
    for (i, n) in grammar::DAY_FULL.iter().enumerate() {
        if w.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(i as i64);
        }
    }
    for (i, n) in grammar::DAY_ABBR.iter().enumerate() {
        if w.eq_ignore_ascii_case(n.as_bytes()) {
            return Some(i as i64);
        }
    }
    None
}

/// Reads a meridian at or after `*p` and applies it to `h`.
fn apply_meridian(s: &[u8], p: &mut usize, end: usize, h: i64) -> i64 {
    while *p < end && !matches!(s[*p], b'a' | b'A' | b'p' | b'P') {
        *p += 1;
    }
    if *p >= end {
        return h;
    }
    let pm = matches!(s[*p], b'p' | b'P');
    *p += 1;
    while *p < end && matches!(s[*p], b'.' | b'm' | b'M') {
        *p += 1;
    }
    let h = if h == 12 { 0 } else { h };
    if pm { h + 12 } else { h }
}

/// Reads the fraction after a '.' or ':' at `*p` as microseconds.
fn get_frac(s: &[u8], p: &mut usize, end: usize) -> i64 {
    while *p < end && !s[*p].is_ascii_digit() {
        *p += 1;
    }
    let mut v = 0i64;
    let mut n = 0;
    while *p < end && s[*p].is_ascii_digit() {
        if n < 6 {
            v = v * 10 + (s[*p] - b'0') as i64;
        }
        n += 1;
        *p += 1;
    }
    while n < 6 {
        v *= 10;
        n += 1;
    }
    v
}

/// Parses a "+hh:mm" style correction at `*p`; returns seconds.
fn get_tz_correction(s: &[u8], p: &mut usize, end: usize) -> Option<i32> {
    if s[*p..end].starts_with(b"GMT") {
        *p += 3;
    }
    let neg = s.get(*p) == Some(&b'-');
    *p += 1;
    let start = *p;
    while *p < end && (s[*p].is_ascii_digit() || s[*p] == b':') {
        *p += 1;
    }
    let body = &s[start..*p];
    let num = |t: &[u8]| -> i64 { t.iter().fold(0i64, |a, c| a * 10 + (c - b'0') as i64) };
    let (h, m, sec) = if !body.contains(&b':') {
        match body.len() {
            1 | 2 => (num(body), 0, 0),
            3 => (num(&body[..1]), num(&body[1..]), 0),
            4 => (num(&body[..2]), num(&body[2..]), 0),
            6 => (num(&body[..2]), num(&body[2..4]), num(&body[4..])),
            _ => (num(&body[..2.min(body.len())]), 0, 0),
        }
    } else {
        let parts: Vec<&[u8]> = body.split(|&c| c == b':').collect();
        match parts.as_slice() {
            [h, m] => (num(h), num(m), 0),
            [h, m, sec] => (num(h), num(m), num(sec)),
            _ => (0, 0, 0),
        }
    };
    let total = (h * 3600 + m * 60 + sec) as i32;
    Some(if neg { -total } else { total })
}

/// Looks up a zone name or abbreviation from a string (the `tz` rule).
fn lookup_zone(name: &[u8]) -> Option<TimeZone> {
    let n = std::str::from_utf8(name).ok()?;
    if n == "UTC" {
        return TimeZone::from_id("UTC");
    }
    if let Some(e) = tz::abbr::find(n) {
        return Some(TimeZone::from_abbr(e));
    }
    let z = tz::ZoneId::find(n)?;
    Some(TimeZone::Id(tz::IdZone::new(z)))
}

/// Applies a zone token (`tz` or `tzcorrection`) spanning `p..end`.
fn zone_token(t: &mut Parsed, s: &[u8], p: usize, end: usize) -> usize {
    if !t.have_zone(p) {
        return end;
    }
    set_zone(t, s, p, end)
}

/// Sets the zone from `p..end` without counting it as a zone token: the
/// zone inside a Common Log Format or SOAP date replaces any other one and
/// a later zone is no "double" one (observed on PHP 8.5).
fn set_zone(t: &mut Parsed, s: &[u8], p: usize, end: usize) -> usize {
    let mut q = p;
    let c = s[q];
    if c == b'+' || c == b'-' || s[q..end].starts_with(b"GMT+") || s[q..end].starts_with(b"GMT-") {
        let off = get_tz_correction(s, &mut q, end).unwrap_or(0);
        t.zone = Some(ParsedZone::Zone(TimeZone::Offset(off)));
        return q;
    }
    // "(" abbr ")" or an identifier
    let mut a = p;
    let mut b = end;
    if s[a] == b'(' {
        a += 1;
    }
    if b > a && s[b - 1] == b')' {
        b -= 1;
    }
    match lookup_zone(&s[a..b]) {
        Some(z) => t.zone = Some(ParsedZone::Zone(z)),
        None => {
            // An unknown name zeroes an offset zone set by a Common Log
            // Format or SOAP date, keeping its kind (observed on PHP 8.5).
            t.zone = match t.zone {
                Some(ParsedZone::Zone(TimeZone::Offset(_))) => {
                    Some(ParsedZone::Zone(TimeZone::Offset(0)))
                }
                _ => Some(ParsedZone::NotFound),
            };
            t.error(p, "The timezone could not be found in the database");
        }
    }
    end
}

/// Sets the time fields; the fraction is zero unless given.
fn set_time(t: &mut Parsed, h: i64, i: i64, s: i64, us: Option<i64>) {
    t.h = Some(h);
    t.i = Some(i);
    t.s = Some(s);
    t.us = Some(us.unwrap_or(0));
}

/// Applies the rule matched at `p..end`; returns where scanning resumes.
fn action(t: &mut Parsed, rule: Rule, s: &[u8], p: usize, end: usize) -> usize {
    use Rule::*;
    let mut q = p;
    match rule {
        Now => {}
        Noon => {
            t.unhave_time();
            if t.have_time(p) {
                t.h = Some(12);
            }
        }
        MidnightToday => t.unhave_time(),
        Yesterday => {
            t.unhave_time();
            t.rel().d = -1;
        }
        Tomorrow => {
            t.unhave_time();
            t.rel().d = 1;
        }
        Timestamp | TimestampMs => {
            t.rel();
            t.unhave_date();
            t.unhave_time();
            if !t.have_zone(p) {
                return end;
            }
            q = p + 1;
            let neg = s.get(q) == Some(&b'-');
            if neg {
                q += 1;
            }
            // accumulate with the sign so that PHP_INT_MIN fits
            let mut v: i64 = 0;
            while q < end && s[q].is_ascii_digit() {
                let d = (s[q] - b'0') as i64;
                v = if neg {
                    v.saturating_mul(10).saturating_sub(d)
                } else {
                    v.saturating_mul(10).saturating_add(d)
                };
                q += 1;
            }
            let us = if rule == TimestampMs {
                if end == p || !s[end - 1].is_ascii_digit() {
                    t.error(p, "Found unexpected data");
                }
                get_frac(s, &mut q, end)
            } else {
                0
            };
            t.saw_timestamp = true;
            t.y = Some(1970);
            t.m = Some(1);
            t.d = Some(1);
            set_time(t, 0, 0, 0, Some(0));
            let r = t.rel();
            r.s = r.s.saturating_add(v);
            r.us += if neg { -us } else { us };
            t.zone = Some(ParsedZone::Zone(TimeZone::Offset(0)));
        }
        FirstLastDayOf => {
            let first = s[p].eq_ignore_ascii_case(&b'f');
            t.rel().first_last_day_of = if first { 1 } else { 2 };
        }
        BackFrontOf => {
            t.unhave_time();
            if !t.have_time(p) {
                return end;
            }
            let back = s[p].eq_ignore_ascii_case(&b'b');
            let mut h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            // "front of 12am" is 11:45 (observed on PHP 8.5): pm adds 12
            // and am changes nothing
            while q < end && !matches!(s[q], b'a' | b'A' | b'p' | b'P') {
                q += 1;
            }
            if q < end && matches!(s[q], b'p' | b'P') && h < 12 {
                h += 12;
            }
            if back {
                set_time(t, h, 15, 0, Some(0));
            } else {
                set_time(t, h - 1, 45, 0, Some(0));
            }
        }
        WeekdayOf => {
            let (n, behavior_last) = reltext_amount(s, &mut q, end);
            let dow = read_day_name(s, &mut q, end).unwrap_or(0);
            t.unhave_time();
            let r = t.rel();
            r.weekday = Some(dow);
            // resets an earlier "this week" (observed on PHP 8.5: "last
            // week this fri of" counts forward, "this monday of this week"
            // within the week)
            r.weekday_behavior = WeekdayBehavior::Forward;
            if behavior_last || n <= 0 {
                // "last", "previous" and "this": counted back from the
                // next month's first such day (observed on PHP 8.5: "this
                // monday of" is the first Monday of next month)
                r.d += if behavior_last { -7 } else { n * 7 };
                r.special = Special::LastDayOfWeekInMonth;
            } else {
                r.d += (n.max(1) - 1) * 7;
                r.special = Special::DayOfWeekInMonth;
            }
        }
        TimeTiny12 | TimeShort12 | TimeLong12 | MsSqlTime => {
            if !t.have_time(p) {
                return end;
            }
            let mut h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let mut i = 0;
            let mut sec = 0;
            let mut us = Some(0);
            if rule != TimeTiny12 {
                i = get_nr(s, &mut q, end, 2).unwrap_or(0);
            }
            if rule == TimeLong12 || rule == MsSqlTime {
                sec = get_nr(s, &mut q, end, 2).unwrap_or(0);
            }
            if rule == MsSqlTime {
                us = Some(get_frac(s, &mut q, end));
            }
            h = apply_meridian(s, &mut q, end, h);
            set_time(t, h, i, sec, us);
        }
        GnuNoColon => {
            if t.have_time {
                // Four digits after a time are a year ("Tue Jul 1
                // 22:35:17 2008"), once; a second time they are a double
                // time (observed on PHP 8.5).
                if t.time_year_used {
                    t.error(p, "Double time specification");
                } else {
                    t.time_year_used = true;
                    t.y = get_nr(s, &mut q, end, 4);
                }
                return end;
            }
            // "0408" leaves the fraction alone (observed on PHP 8.5)
            t.have_time = true;
            t.h = get_nr(s, &mut q, end, 2);
            t.i = get_nr(s, &mut q, end, 2);
            t.s = Some(0);
        }
        TimeTiny24 | TimeShort24 | TimeLong24 | Iso8601Long | Iso8601NoColon => {
            if !t.have_time(p) {
                return end;
            }
            let h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let i = if rule == TimeTiny24 {
                0
            } else {
                get_nr(s, &mut q, end, 2).unwrap_or(0)
            };
            let sec = if matches!(rule, TimeLong24 | Iso8601Long | Iso8601NoColon) {
                get_nr(s, &mut q, end, 2).unwrap_or(0)
            } else {
                0
            };
            let us = if rule == Iso8601Long {
                Some(get_frac(s, &mut q, end))
            } else {
                None
            };
            set_time(t, h, i, sec, us);
        }
        AmericanShort | American => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_nr(s, &mut q, end, 2);
            t.d = get_nr(s, &mut q, end, 2);
            if rule == American
                && let Some((y, n)) = get_nr_len(s, &mut q, end, 4)
            {
                t.y = Some(process_year(y, n));
            }
        }
        Iso8601DateSlash | DateSlash | GnuDateShorter | Iso8601Date4 | Iso8601Date2
        | GnuDateShort | DateNoColon => {
            if !t.have_date(p) {
                return end;
            }
            let neg = s[p] == b'-';
            let ymax = match rule {
                Iso8601Date2 => 2,
                _ => 4,
            };
            if let Some((y, n)) = get_nr_len(s, &mut q, end, ymax) {
                let y = if matches!(rule, GnuDateShort | Iso8601Date2) {
                    process_year(y, n)
                } else {
                    y
                };
                t.y = Some(if neg { -y } else { y });
            }
            t.m = get_nr(s, &mut q, end, 2);
            t.d = if rule == GnuDateShorter {
                Some(1)
            } else {
                get_nr(s, &mut q, end, 2)
            };
        }
        Iso8601DateX => {
            if !t.have_date(p) {
                return end;
            }
            let neg = s[p] == b'-';
            let y = get_nr(s, &mut q, end, 19).unwrap_or(0);
            t.y = Some(if neg { -y } else { y });
            t.m = get_nr(s, &mut q, end, 2);
            t.d = get_nr(s, &mut q, end, 2);
        }
        PointedDate4 | PointedDate2 => {
            if !t.have_date(p) {
                return end;
            }
            t.d = get_nr(s, &mut q, end, 2);
            t.m = get_nr(s, &mut q, end, 2);
            if let Some((y, n)) = get_nr_len(s, &mut q, end, 4) {
                t.y = Some(process_year(y, n));
            }
        }
        DateFull => {
            if !t.have_date(p) {
                return end;
            }
            t.d = get_nr(s, &mut q, end, 2);
            skip_day_suffix(s, &mut q, end);
            t.m = get_month(s, &mut q, end);
            if let Some((y, n)) = get_nr_len(s, &mut q, end, 4) {
                t.y = Some(process_year(y, n));
            }
        }
        DateNoDay => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_month(s, &mut q, end);
            t.y = get_nr(s, &mut q, end, 4);
            t.d = Some(1);
        }
        DateNoDayRev => {
            if !t.have_date(p) {
                return end;
            }
            t.y = get_nr(s, &mut q, end, 4);
            t.m = get_month(s, &mut q, end);
            t.d = Some(1);
        }
        DateTextual | DateNoYear => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_month(s, &mut q, end);
            t.d = get_nr(s, &mut q, end, 2);
            // "Apr 28" unsets a year set by "@ts" (observed on PHP 8.5)
            t.y = None;
            if rule == DateTextual
                && let Some((y, n)) = get_nr_len(s, &mut q, end, 4)
            {
                t.y = Some(process_year(y, n));
            }
        }
        DateNoYearRev => {
            if !t.have_date(p) {
                return end;
            }
            t.d = get_nr(s, &mut q, end, 2);
            skip_day_suffix(s, &mut q, end);
            t.m = get_month(s, &mut q, end);
        }
        XmlRpc | XmlRpcNoColon | Soap | Wddx | Exif => {
            if !t.have_time(p) {
                return end;
            }
            if !t.have_date(p) {
                return end;
            }
            t.y = get_nr(s, &mut q, end, 4);
            t.m = get_nr(s, &mut q, end, 2);
            t.d = get_nr(s, &mut q, end, 2);
            let h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let i = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let sec = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let us = if rule == Soap {
                Some(get_frac(s, &mut q, end))
            } else {
                None
            };
            set_time(t, h, i, sec, us);
            if rule == Soap && q < end {
                return set_zone(t, s, q, end);
            }
        }
        PgYearDotDay => {
            if !t.have_date(p) {
                return end;
            }
            t.y = get_nr(s, &mut q, end, 4);
            t.d = get_nr(s, &mut q, end, 3);
            t.m = Some(1);
        }
        PgTextShort => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_month(s, &mut q, end);
            t.d = get_nr(s, &mut q, end, 2);
            if let Some((y, n)) = get_nr_len(s, &mut q, end, 4) {
                t.y = Some(process_year(y, n));
            }
        }
        PgTextReverse => {
            if !t.have_date(p) {
                return end;
            }
            if let Some((y, n)) = get_nr_len(s, &mut q, end, 4) {
                t.y = Some(process_year(y, n));
            }
            t.m = get_month(s, &mut q, end);
            t.d = get_nr(s, &mut q, end, 2);
        }
        Clf => {
            if !t.have_time(p) || !t.have_date(p) {
                return end;
            }
            t.d = get_nr(s, &mut q, end, 2);
            t.m = get_month(s, &mut q, end);
            t.y = get_nr(s, &mut q, end, 4);
            let h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let i = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let sec = get_nr(s, &mut q, end, 2).unwrap_or(0);
            set_time(t, h, i, sec, None);
            while q < end && matches!(s[q], b' ' | b'\t') {
                q += 1;
            }
            return set_zone(t, s, q, end);
        }
        Year4 => {
            // a lone year does not count as a date: "1978 2008-01-01" is
            // no double date and "1978" is no invalid date (observed on
            // PHP 8.5)
            t.y = get_nr(s, &mut q, end, 4);
        }
        IsoWeekDay | IsoWeek => {
            if !t.have_date(p) {
                return end;
            }
            let y = get_nr(s, &mut q, end, 4).unwrap_or(0);
            let w = get_nr(s, &mut q, end, 2).unwrap_or(1);
            let wd = if rule == IsoWeekDay {
                get_nr(s, &mut q, end, 1).unwrap_or(1)
            } else {
                1
            };
            t.y = Some(y);
            t.m = Some(1);
            t.d = Some(1);
            let days =
                crate::civil::days_from_iso_week(y, w, wd) - crate::civil::days_from_civil(y, 1, 1);
            // the week replaces earlier relative days (observed on PHP
            // 8.5: "+90 day 2016W47" is 325 days)
            t.rel().d = days;
        }
        DateShortWithTimeShort12
        | DateShortWithTimeLong12
        | DateShortWithTimeShort
        | DateShortWithTimeLong => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_month(s, &mut q, end);
            t.d = get_nr(s, &mut q, end, 2);
            if !t.have_time(p) {
                return end;
            }
            let mut h = get_nr(s, &mut q, end, 2).unwrap_or(0);
            let i = get_nr(s, &mut q, end, 2).unwrap_or(0);
            if rule == DateShortWithTimeLong {
                // Seconds only after a colon; whatever follows is read as
                // a zone, reported at the start of the date when it is not
                // one: "Jan 22 10.30.45 UTC" is 10:30 with an unknown zone
                // (observed on PHP 8.5).
                let mut sec = 0;
                if s.get(q) == Some(&b':') {
                    sec = get_nr(s, &mut q, end, 2).unwrap_or(0);
                }
                set_time(t, h, i, sec, None);
                while q < end && matches!(s[q], b' ' | b'\t') {
                    q += 1;
                }
                if q < end {
                    if matches!(s[q], b'+' | b'-') || s[q].is_ascii_alphabetic() || s[q] == b'(' {
                        set_zone(t, s, q, end);
                        if let Some((pos, msg)) = t.errors.last_mut()
                            && msg == "The timezone could not be found in the database"
                            && *pos == q
                        {
                            *pos = p;
                        }
                    } else {
                        t.zone = Some(ParsedZone::NotFound);
                        t.error(p, "The timezone could not be found in the database");
                    }
                }
                return end;
            }
            let sec = if rule == DateShortWithTimeLong12 {
                get_nr(s, &mut q, end, 2).unwrap_or(0)
            } else {
                0
            };
            if matches!(rule, DateShortWithTimeShort12 | DateShortWithTimeLong12) {
                h = apply_meridian(s, &mut q, end, h);
            }
            set_time(t, h, i, sec, None);
        }
        RelativeTextWeek => {
            let (n, _) = reltext_amount(s, &mut q, end);
            let r = t.rel();
            r.d += n * 7;
            if r.weekday.is_none() {
                r.weekday = Some(1);
            }
            r.weekday_behavior = WeekdayBehavior::Week;
        }
        RelativeText => {
            let (n, _) = reltext_amount(s, &mut q, end);
            relative_unit(t, s, &mut q, end, n, true);
        }
        MonthFull | MonthAbbr => {
            if !t.have_date(p) {
                return end;
            }
            t.m = get_month(s, &mut q, end);
        }
        DayFull | DayAbbr | WeekdaySpecial => {
            t.unhave_time();
            let dow = read_day_name(s, &mut q, end).unwrap_or(1);
            let r = t.rel();
            r.weekday = Some(dow);
            // a plain day name keeps "this week" but not the "next"
            // reading of an earlier "second monday" (observed on PHP 8.5)
            if r.weekday_behavior == WeekdayBehavior::Strict {
                r.weekday_behavior = WeekdayBehavior::Forward;
            }
        }
        Ago => {
            if let Some(r) = t.relative.as_mut() {
                r.y = -r.y;
                r.m = -r.m;
                r.d = -r.d;
                r.h = -r.h;
                r.i = -r.i;
                r.s = -r.s;
                r.us = -r.us;
                // Sunday counts as 7 here: "sunday ago" is -7 (observed on
                // PHP 8.5)
                if let Some(w) = r.weekday.as_mut() {
                    *w = if *w == 0 { -7 } else { -*w };
                }
                if let Special::Weekdays(n) = r.special {
                    r.special = Special::Weekdays(-n);
                }
            }
        }
        Relative => {
            let mut neg = false;
            while q < end && matches!(s[q], b'+' | b'-' | b' ' | b'\t') {
                if s[q] == b'-' {
                    neg = !neg;
                }
                q += 1;
            }
            let n = get_nr(s, &mut q, end, 13).unwrap_or(0);
            let n = if neg { -n } else { n };
            relative_unit(t, s, &mut q, end, n, false);
        }
        TzCorrection | Tz => return zone_token(t, s, p, end),
    }
    end
}

fn skip_day_suffix(s: &[u8], p: &mut usize, end: usize) {
    if *p + 2 <= end {
        let w = &s[*p..*p + 2];
        if [b"st", b"nd", b"rd", b"th"]
            .iter()
            .any(|x| w.eq_ignore_ascii_case(*x))
        {
            *p += 2;
        }
    }
}

fn read_word<'a>(s: &'a [u8], p: &mut usize, end: usize) -> &'a [u8] {
    while *p < end && !s[*p].is_ascii_alphabetic() && s[*p] < 0x80 {
        *p += 1;
    }
    let start = *p;
    while *p < end && (s[*p].is_ascii_alphabetic() || s[*p] >= 0x80) {
        *p += 1;
    }
    &s[start..*p]
}

fn read_day_name(s: &[u8], p: &mut usize, end: usize) -> Option<i64> {
    let w = read_word(s, p, end);
    if w.eq_ignore_ascii_case(b"weekday") || w.eq_ignore_ascii_case(b"weekdays") {
        return Some(1);
    }
    day_from_name(w)
}

/// "first" … "twelfth", "next", "last", "previous", "this": the amount,
/// and whether the word was "last" (for "last dayname of").
fn reltext_amount(s: &[u8], p: &mut usize, end: usize) -> (i64, bool) {
    let w = read_word(s, p, end);
    let lw = w.to_ascii_lowercase();
    let n = match lw.as_slice() {
        b"next" => 1,
        b"last" | b"previous" => -1,
        b"this" => 0,
        _ => match grammar::RELTEXT_NUMBER
            .iter()
            .position(|x| x.as_bytes() == lw.as_slice())
        {
            Some(i) if i <= 7 => i as i64 + 1,
            // "eight" and "eighth" are both 8
            Some(i) => i as i64,
            None => 0,
        },
    };
    (n, lw.as_slice() == b"last")
}

/// Applies "N unit" (unit at `*p`).
fn relative_unit(t: &mut Parsed, s: &[u8], p: &mut usize, end: usize, n: i64, text: bool) {
    let w = read_word(s, p, end).to_ascii_lowercase();
    let w = w.as_slice();
    let unit = w
        .strip_suffix(b"s")
        .filter(|u| !u.is_empty() && *u != b"m" && *u != b"\xc2\xb5" && *u != b"week")
        .unwrap_or(w);
    // weekday names as units: "+1 monday", "next sat", "4 sundays"
    let dow = day_from_name(w).or_else(|| w.strip_suffix(b"s").and_then(day_from_name));
    if let Some(dow) = dow {
        // "next sat" resets the time, "+1 sat" does not (observed on
        // PHP 8.5)
        if text {
            t.unhave_time();
        }
        let r = t.rel();
        if n > 0 {
            r.d += (n - 1) * 7;
        } else if n < 0 {
            r.d += n * 7;
        }
        r.weekday = Some(dow);
        // "next sat" moves a week on a Saturday, "+1 sat" does not
        // (observed on PHP 8.5)
        // and a counted day name drops an earlier "this week" ("first
        // TUESDAY next week May 309 wed")
        r.weekday_behavior = if text && n > 0 {
            WeekdayBehavior::Strict
        } else {
            WeekdayBehavior::Forward
        };
        return;
    }
    let r = t.rel();
    match unit {
        b"ms" | b"msec" | b"millisecond" => r.us += n * 1000,
        b"\xc2\xb5s" | b"\xc2\xb5sec" | b"usec" | b"microsecond" => r.us += n,
        b"sec" | b"second" => r.s += n,
        b"min" | b"minute" => r.i += n,
        b"hour" => r.h += n,
        b"day" => r.d += n,
        b"week" | b"weeks" => r.d += n * 7,
        b"fortnight" | b"forthnight" => r.d += n * 14,
        b"month" => r.m += n,
        b"year" => r.y += n,
        b"weekday" | b"weekdays" => {
            if text {
                t.unhave_time();
            }
            let r = t.rel();
            let cur = match r.special {
                Special::Weekdays(c) => c,
                _ => 0,
            };
            r.special = Special::Weekdays(cur + n);
        }
        _ => {}
    }
}

/// The `date_parse()` array, field by field (`None` is PHP's `false` or
/// an absent key).
#[derive(Debug, Clone, PartialEq)]
pub struct DateParse {
    /// year
    pub year: Option<i64>,
    /// month
    pub month: Option<i64>,
    /// day
    pub day: Option<i64>,
    /// hour
    pub hour: Option<i64>,
    /// minute
    pub minute: Option<i64>,
    /// second
    pub second: Option<i64>,
    /// fraction, in microseconds (PHP prints it as a float of seconds)
    pub fraction_us: Option<i64>,
    /// warnings
    pub warnings: Vec<(usize, String)>,
    /// errors
    pub errors: Vec<(usize, String)>,
    /// is_localtime
    pub is_localtime: bool,
    /// zone_type (present when is_localtime)
    pub zone_type: Option<u8>,
    /// zone, in seconds (types 1 and 2)
    pub zone: Option<i64>,
    /// is_dst (types 1 and 2)
    pub is_dst: Option<bool>,
    /// tz_abbr
    pub tz_abbr: Option<String>,
    /// tz_id
    pub tz_id: Option<String>,
    /// relative, when present
    pub relative: Option<DateParseRelative>,
}

/// `date_parse()['relative']`.
#[derive(Debug, Clone, PartialEq)]
pub struct DateParseRelative {
    /// year, month, day, hour, minute, second
    pub ymdhis: [i64; 6],
    /// weekday (present when a relative weekday was given)
    pub weekday: Option<i64>,
    /// weekdays (present for "N weekdays")
    pub weekdays: Option<i64>,
    /// first_day_of_month
    pub first_day_of_month: bool,
    /// last_day_of_month
    pub last_day_of_month: bool,
}

impl Parsed {
    /// The `date_parse()` view of this result.
    pub fn date_parse(&self) -> DateParse {
        let mut out = DateParse {
            year: self.y,
            month: self.m,
            day: self.d,
            hour: self.h,
            minute: self.i,
            second: self.s,
            fraction_us: self.us,
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
            is_localtime: self.is_localtime(),
            zone_type: self.zone_type(),
            zone: None,
            is_dst: None,
            tz_abbr: None,
            tz_id: None,
            relative: None,
        };
        if let Some(ParsedZone::Zone(z)) = &self.zone {
            match z {
                TimeZone::Offset(o) => {
                    out.zone = Some(*o as i64);
                    out.is_dst = Some(false);
                }
                TimeZone::Abbr { abbr, offset, dst } => {
                    out.zone = Some(*offset as i64 - if *dst { 3600 } else { 0 });
                    out.is_dst = Some(*dst);
                    out.tz_abbr = Some(abbr.as_str().to_string());
                }
                TimeZone::Id(id) => {
                    if id.name() == "UTC" {
                        out.tz_abbr = Some("UTC".to_string());
                    }
                    out.tz_id = Some(id.name().to_string());
                }
            }
        }
        if let Some(r) = &self.relative {
            out.relative = Some(DateParseRelative {
                ymdhis: [r.y, r.m, r.d, r.h, r.i, r.s],
                weekday: r.weekday,
                weekdays: match r.special {
                    Special::Weekdays(n) => Some(n),
                    _ => None,
                },
                first_day_of_month: r.first_last_day_of == 1,
                last_day_of_month: r.first_last_day_of == 2,
            });
        }
        out
    }
}
