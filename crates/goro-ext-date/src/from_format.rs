//! `DateTime::createFromFormat()`, `DateTimeImmutable::createFromFormat()`,
//! `date_create_from_format()` and `date_parse_from_format()`.
//!
//! PHP manual (DateTimeImmutable::createFromFormat): the format is read
//! character by character against the input; fields the format does not
//! give are taken from the current time unless `!` or `|` reset them to the
//! Unix epoch. Everything finer (how each reader skips and consumes input,
//! which error message lands at which position, how failed fields are
//! left, the end-of-input rules) was observed on PHP 8.5 with a few
//! thousand format/input pairs (tests/fixtures/from_format_php85.tsv) and
//! the ext/date tests that use these functions.
//!
//! The parse ([`parse_from_format`]) does not depend on "now" or on a
//! zone; [`create_from_format`] combines it with both.

use crate::civil;
use crate::parse::{DateParse, DateParseRelative};
use crate::time::{DateTime, LocalTime};
use crate::tz::{self, TimeZone};

/// `DateTime::getLastErrors()` content: (position, message) lists, in the
/// order PHP adds them.
///
/// PHP builds its arrays from these with the position as key, so a later
/// message at the same position replaces an earlier one while the count
/// (`warning_count` / `error_count`) still counts both (observed on PHP
/// 8.5; test ext/date/tests/bug67251.phpt shows `error_count` 2 with one
/// array entry).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LastErrors {
    /// Warnings.
    pub warnings: Vec<(usize, String)>,
    /// Errors.
    pub errors: Vec<(usize, String)>,
}

impl LastErrors {
    /// No warning and no error: `getLastErrors()` then returns `false`
    /// (PHP 8.2+, observed on PHP 8.5).
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty() && self.errors.is_empty()
    }

    /// The `warnings` array as PHP shows it: one entry per position, the
    /// last message written there, in first-insertion order.
    pub fn warnings_array(&self) -> Vec<(usize, &str)> {
        php_array(&self.warnings)
    }

    /// The `errors` array as PHP shows it (see [`LastErrors::warnings_array`]).
    pub fn errors_array(&self) -> Vec<(usize, &str)> {
        php_array(&self.errors)
    }
}

/// Position-keyed array view of a message list (later messages at a
/// position replace earlier ones, keeping the first position order).
pub fn php_array(list: &[(usize, String)]) -> Vec<(usize, &str)> {
    let mut out: Vec<(usize, &str)> = Vec::with_capacity(list.len());
    for (p, m) in list {
        match out.iter_mut().find(|(q, _)| q == p) {
            Some(e) => e.1 = m.as_str(),
            None => out.push((*p, m.as_str())),
        }
    }
    out
}

/// The zone part of a parse, field by field as `date_parse_from_format()`
/// reports it (observed on PHP 8.5: a failed name keeps the kind of an
/// earlier zone but zeroes its offset, an identifier keeps an earlier
/// abbreviation, `!` forgets an identifier but keeps the kind).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormatZone {
    /// `zone_type`: 0 (not found), 1 (offset), 2 (abbreviation), 3 (id).
    pub zone_type: u8,
    /// `zone`, in seconds (the standard offset for an abbreviation: a DST
    /// one reports its offset minus one hour).
    pub z: i64,
    /// `is_dst`.
    pub dst: bool,
    /// `tz_abbr` (shown for types 2 and 3).
    pub abbr: Option<String>,
    /// The identifier zone (`tz_id`, type 3).
    pub id: Option<TimeZone>,
    /// The zone a created object gets (types 1 and 2; type 3 uses `id`).
    tz: Option<TimeZone>,
}

/// The result of reading `input` against `format`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormatParsed {
    /// Year (`None` is PHP's "unset", `false` in date_parse_from_format()).
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
    /// Microseconds.
    pub us: Option<i64>,
    /// The zone, when the format read one (even unsuccessfully).
    pub zone: Option<FormatZone>,
    /// Day of the week from `D` / `l` (0 = Sunday), applied as "this or
    /// the next such day" after the date is known.
    pub weekday: Option<i64>,
    /// Warnings.
    pub warnings: Vec<(usize, String)>,
    /// Errors.
    pub errors: Vec<(usize, String)>,
}

// Messages (exact texts observed on PHP 8.5).
const E_TRAILING: &str = "Trailing data";
const E_NOT_ENOUGH: &str = "Not enough data available to satisfy format";
const E_UNEXPECTED: &str = "Unexpected data found.";
const E_TZ: &str = "The timezone could not be found in the database";
const E_FORMAT_SEP: &str = "The format separator does not match";
const E_SEP: &str = "The separation symbol could not be found";
const E_SEP_ANY: &str = "The separation symbol ([;:/.,-]) could not be found";
const E_ESCAPED: &str = "The escaped character could not be found";
const E_FOUND_UNEXPECTED: &str = "Found unexpected data";
const E_RANGE: &str = "Number out of range";
const W_TIME: &str = "The parsed time was invalid";
const W_DATE: &str = "The parsed date was invalid";

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const ROMAN: [&str; 12] = [
    "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii",
];
const DAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// A textual month (`M` / `F`): full and three-letter English names,
/// "sept" and the roman numerals I-XII, any case (observed on PHP 8.5:
/// "x" reads as October, "Janu" and "XIII" are rejected).
fn month_by_name(w: &[u8]) -> Option<i64> {
    let lw: Vec<u8> = w.to_ascii_lowercase();
    for (n, full) in MONTHS.iter().enumerate() {
        if lw == full.as_bytes() || lw == full.as_bytes()[..3] {
            return Some(n as i64 + 1);
        }
    }
    if lw == b"sept" {
        return Some(9);
    }
    ROMAN
        .iter()
        .position(|r| lw == r.as_bytes())
        .map(|n| n as i64 + 1)
}

/// Relative-format unit words that `D` / `l` also accept, with the day
/// number they stand for: the unit's count in the strtotime() relative
/// formats (PHP manual, "Relative Formats": a week is 7 days, a
/// fortnight 14, a millisecond 1000 microseconds), observed on PHP 8.5
/// ("week" read as a day name is 7, "msec" 1000, "day" 1).
const UNIT_WORDS: &[(&str, i64)] = &[
    ("ms", 1000),
    ("msec", 1000),
    ("msecs", 1000),
    ("millisecond", 1000),
    ("milliseconds", 1000),
    ("\u{b5}s", 1),
    ("usec", 1),
    ("usecs", 1),
    ("\u{b5}sec", 1),
    ("\u{b5}secs", 1),
    ("microsecond", 1),
    ("microseconds", 1),
    ("sec", 1),
    ("secs", 1),
    ("second", 1),
    ("seconds", 1),
    ("min", 1),
    ("mins", 1),
    ("minute", 1),
    ("minutes", 1),
    ("hour", 1),
    ("hours", 1),
    ("day", 1),
    ("days", 1),
    ("week", 7),
    ("weeks", 7),
    ("fortnight", 14),
    ("fortnights", 14),
    ("forthnight", 14),
    ("forthnights", 14),
    ("month", 1),
    ("months", 1),
    ("year", 1),
    ("years", 1),
    ("weekday", 1),
    ("weekdays", 1),
];

/// A textual day (`D` / `l`): full and three-letter names and the plural
/// of the full names (any case), plus the unit words of [`UNIT_WORDS`]
/// (observed on PHP 8.5: "Tues" and "mons" are rejected, "Sundays" and
/// "WEEKDAYS" accepted).
fn day_by_name(w: &[u8]) -> Option<i64> {
    let lw: Vec<u8> = w.to_ascii_lowercase();
    for (n, full) in DAYS.iter().enumerate() {
        let f = full.as_bytes();
        if lw == f
            || lw == f[..3]
            || (lw.len() == f.len() + 1 && lw.starts_with(f) && lw[f.len()] == b's')
        {
            return Some(n as i64);
        }
    }
    UNIT_WORDS
        .iter()
        .find(|(u, _)| lw == u.as_bytes())
        .map(|&(_, n)| n)
}

/// Two-digit years: 0-69 are 2000-2069, 70-99 are 1970-1999 (PHP manual,
/// DateTimeImmutable::createFromFormat, "y").
fn two_digit_year(y: i64) -> i64 {
    if y < 70 { y + 2000 } else { y + 1900 }
}

struct Reader<'a> {
    s: &'a [u8],
    p: usize,
    out: FormatParsed,
    allow_extra: bool,
}

impl<'a> Reader<'a> {
    fn at(&self, p: usize) -> u8 {
        self.s.get(p).copied().unwrap_or(0)
    }

    fn error(&mut self, pos: usize, msg: &str) {
        self.out.errors.push((pos, msg.to_string()));
    }

    fn warning(&mut self, pos: usize, msg: &str) {
        self.out.warnings.push((pos, msg.to_string()));
    }

    /// The common number reader: skips anything up to the first digit
    /// ("Unexpected data found." at the start when it skipped something),
    /// then reads up to `max` digits. Returns the value and the digit
    /// count, `None` when no digit was found (observed on PHP 8.5: "-1"
    /// read as a day is 1 with that error, "a" gives the error and the
    /// field's own "could not be found" one).
    fn number(&mut self, max: usize) -> Option<(i64, usize)> {
        let start = self.p;
        while self.p < self.s.len() && !self.s[self.p].is_ascii_digit() {
            self.p += 1;
        }
        if self.p > start {
            self.error(start, E_UNEXPECTED);
        }
        let ds = self.p;
        let mut v: i64 = 0;
        while self.p < self.s.len() && self.p - ds < max && self.s[self.p].is_ascii_digit() {
            v = v * 10 + (self.s[self.p] - b'0') as i64;
            self.p += 1;
        }
        if self.p == ds {
            None
        } else {
            Some((v, self.p - ds))
        }
    }

    /// A signed number (`U`, `X`, `x`): anything before the first sign or
    /// digit is skipped with "Unexpected data found.", then any run of
    /// signs (each '-' flips), then anything up to the digits silently.
    /// A missing number is "Found unexpected data" and a value outside
    /// the 64-bit range "Number out of range", both reported at position 0
    /// whatever the token's position, and the value is then 0 (observed on
    /// PHP 8.5: "--1" is 1, "- 1" is -1, "x5" is 5 with the first error).
    fn signed(&mut self, max: usize) -> i64 {
        let start = self.p;
        while self.p < self.s.len() && !matches!(self.s[self.p], b'0'..=b'9' | b'+' | b'-') {
            self.p += 1;
        }
        if self.p > start {
            self.error(start, E_UNEXPECTED);
        }
        let mut neg = false;
        while self.p < self.s.len() && matches!(self.s[self.p], b'+' | b'-') {
            if self.s[self.p] == b'-' {
                neg = !neg;
            }
            self.p += 1;
        }
        while self.p < self.s.len() && !self.s[self.p].is_ascii_digit() {
            self.p += 1;
        }
        let ds = self.p;
        let mut v: i128 = 0;
        while self.p < self.s.len() && self.p - ds < max && self.s[self.p].is_ascii_digit() {
            if v <= i64::MAX as i128 + 1 {
                v = v * 10 + (self.s[self.p] - b'0') as i128;
            }
            self.p += 1;
        }
        if self.p == ds {
            self.error(0, E_FOUND_UNEXPECTED);
            return 0;
        }
        let v = if neg { -v } else { v };
        if v > i64::MAX as i128 || v < i64::MIN as i128 {
            self.error(0, E_RANGE);
            return 0;
        }
        v as i64
    }

    /// `u` (up to 6 digits) and `v` (up to 3): the value is scaled to
    /// microseconds by the number of bytes the reader consumed, skipped
    /// ones included (observed on PHP 8.5: "e5" is 50000 for both, seven
    /// bytes "x123456" give 12345, "pm 3" read by `v` is 300).
    fn fraction(&mut self, digits: usize, msg: &str) {
        let start = self.p;
        match self.number(digits) {
            None => {
                self.error(start, msg);
                self.out.us = None;
            }
            Some((v, _)) => {
                let used = (self.p - start) as i32;
                let scale = 6 - used;
                let mut v = v as i128;
                if scale >= 0 {
                    v *= 10i128.pow(scale as u32);
                } else {
                    v /= 10i128.pow((-scale) as u32);
                }
                self.out.us = Some(v as i64);
            }
        }
    }

    /// `a` / `A`: skips to the first 'a', 'A', 'p' or 'P', then needs "m"
    /// or ".m." after it (any case). Without an hour first the meridian
    /// is still read but "Meridian can only come after an hour has been
    /// found" is reported (observed on PHP 8.5).
    fn meridian(&mut self) {
        let start = self.p;
        if self.out.h.is_none() {
            self.error(start, "Meridian can only come after an hour has been found");
        }
        while self.p < self.s.len() && !matches!(self.s[self.p], b'a' | b'A' | b'p' | b'P') {
            self.p += 1;
        }
        let ok = if self.p >= self.s.len() {
            false
        } else {
            let pm = matches!(self.s[self.p], b'p' | b'P');
            self.p += 1;
            let good = match self.at(self.p) {
                b'm' | b'M' => {
                    self.p += 1;
                    true
                }
                b'.' => {
                    self.p += 1;
                    if matches!(self.at(self.p), b'm' | b'M') {
                        self.p += 1;
                        if self.at(self.p) == b'.' {
                            self.p += 1;
                            true
                        } else {
                            self.p = self.p.min(self.s.len());
                            false
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if good && let Some(h) = self.out.h {
                // 12 am is 0, 12 pm is 12, other pm hours add 12 ("13 pm"
                // is 25, observed on PHP 8.5)
                let h = if h == 12 { 0 } else { h };
                self.out.h = Some(if pm { h + 12 } else { h });
            }
            good
        };
        if !ok {
            self.error(start, "A meridian could not be found");
        }
    }

    /// A day name (`D` / `l`): a word up to a blank or one of ",.:;/-()",
    /// any other byte included (observed on PHP 8.5: "Mon5" and "Mon_"
    /// are one unknown word, "Mon," is Monday then trailing data).
    fn day_name(&mut self) {
        let start = self.p;
        while self.p < self.s.len()
            && !matches!(
                self.s[self.p],
                b' ' | b'\t' | b',' | b'.' | b':' | b';' | b'/' | b'-' | b'(' | b')'
            )
        {
            self.p += 1;
        }
        match day_by_name(&self.s[start..self.p]) {
            Some(n) => self.out.weekday = Some(n),
            None => self.error(start, "A textual day could not be found"),
        }
    }

    /// A month name (`M` / `F`): the run of letters at the position.
    fn month_name(&mut self) {
        let start = self.p;
        while self.p < self.s.len() && self.s[self.p].is_ascii_alphabetic() {
            self.p += 1;
        }
        match month_by_name(&self.s[start..self.p]) {
            Some(n) => self.out.m = Some(n),
            None => self.error(start, "A textual month could not be found"),
        }
    }

    /// A zone (`e`, `T`, `O`, `P`, `p`): blanks and '(' are skipped; a
    /// sign (or "GMT" and a sign) starts an offset, anything else is a
    /// name made of letters, digits and "/_+-" looked up as "UTC", an
    /// abbreviation or an identifier; as many ')' as '(' were skipped are
    /// consumed after it (observed on PHP 8.5: "((EST))" is EST, "(EST))"
    /// leaves trailing data).
    fn zone(&mut self) {
        let start = self.p;
        let mut parens = 0usize;
        while self.p < self.s.len() && matches!(self.s[self.p], b' ' | b'\t' | b'(') {
            if self.s[self.p] == b'(' {
                parens += 1;
            }
            self.p += 1;
        }
        let rest = &self.s[self.p..];
        let gmt = rest.starts_with(b"GMT+") || rest.starts_with(b"GMT-");
        if matches!(self.at(self.p), b'+' | b'-') || gmt {
            if gmt {
                self.p += 3;
            }
            let off = self.offset();
            let z = self.out.zone.get_or_insert_with(FormatZone::default);
            z.zone_type = 1;
            z.dst = false;
            match off {
                Some(o) => {
                    z.z = o as i64;
                    z.tz = Some(TimeZone::Offset(o));
                }
                None => {
                    z.z = 0;
                    z.tz = Some(TimeZone::Offset(0));
                    self.error(start, E_TZ);
                }
            }
        } else {
            let ws = self.p;
            while self.p < self.s.len()
                && (self.s[self.p].is_ascii_alphanumeric()
                    || matches!(self.s[self.p], b'/' | b'_' | b'+' | b'-'))
            {
                self.p += 1;
            }
            let word = &self.s[ws..self.p];
            let found = lookup_zone(word);
            let z = self.out.zone.get_or_insert_with(FormatZone::default);
            match found {
                Some(Found::Utc(tz)) => {
                    z.zone_type = 3;
                    z.abbr = Some("UTC".to_string());
                    z.id = Some(tz);
                }
                Some(Found::Abbr(e)) => {
                    z.zone_type = 2;
                    z.dst = e.dst;
                    z.z = e.offset as i64 - if e.dst { 3600 } else { 0 };
                    z.abbr = Some(e.upper.to_string());
                    z.tz = Some(TimeZone::from_abbr(e));
                }
                Some(Found::Id(tz)) => {
                    z.zone_type = 3;
                    z.id = Some(tz);
                }
                None => {
                    // the offset is zeroed, the DST flag kept ("EDT Foo"
                    // reports zone 0 with is_dst, observed on PHP 8.5)
                    z.z = 0;
                    if let Some(TimeZone::Offset(_)) = z.tz {
                        z.tz = Some(TimeZone::Offset(0));
                    }
                    self.error(start, E_TZ);
                }
            }
        }
        while parens > 0 && self.at(self.p) == b')' {
            self.p += 1;
            parens -= 1;
        }
    }

    /// The digits and colons of an offset after its sign at `self.p`
    /// (observed on PHP 8.5: "+5", "+05", "+123", "+0530", "+053030",
    /// "+5:30", "+05:30:15" are offsets, minutes are not range checked,
    /// "+:30" is zero, five or seven digits and "+1:2:3" are rejected).
    fn offset(&mut self) -> Option<i32> {
        let neg = self.at(self.p) == b'-';
        self.p += 1;
        let start = self.p;
        while self.p < self.s.len() && (self.s[self.p].is_ascii_digit() || self.s[self.p] == b':') {
            self.p += 1;
        }
        let body = &self.s[start..self.p];
        let num = |t: &[u8]| -> i64 { t.iter().fold(0i64, |a, c| a * 10 + (c - b'0') as i64) };
        let parts: Vec<&[u8]> = body.split(|&c| c == b':').collect();
        let secs = match parts.as_slice() {
            [b] => match b.len() {
                1 | 2 => num(b) * 3600,
                3 => num(&b[..1]) * 3600 + num(&b[1..]) * 60,
                4 => num(&b[..2]) * 3600 + num(&b[2..]) * 60,
                6 => num(&b[..2]) * 3600 + num(&b[2..4]) * 60 + num(&b[4..]),
                _ => return None,
            },
            [h, m] => {
                if h.is_empty() {
                    0
                } else if h.len() <= 2 && m.len() <= 2 {
                    num(h) * 3600 + num(m) * 60
                } else {
                    return None;
                }
            }
            [h, m, s] if h.len() == 2 && m.len() == 2 && s.len() == 2 => {
                num(h) * 3600 + num(m) * 60 + num(s)
            }
            _ => return None,
        };
        let secs = secs as i32;
        Some(if neg { -secs } else { secs })
    }

    /// `!`: every field to the Unix epoch; a zone identifier is forgotten
    /// (the kind and an abbreviation or offset stay, observed on PHP 8.5).
    fn reset_all(&mut self) {
        let o = &mut self.out;
        o.y = Some(1970);
        o.m = Some(1);
        o.d = Some(1);
        o.h = Some(0);
        o.i = Some(0);
        o.s = Some(0);
        o.us = Some(0);
        if let Some(z) = &mut o.zone {
            z.id = None;
        }
    }

    /// `|`: the fields not read yet to the Unix epoch.
    fn reset_unset(&mut self) {
        let o = &mut self.out;
        o.y.get_or_insert(1970);
        o.m.get_or_insert(1);
        o.d.get_or_insert(1);
        o.h.get_or_insert(0);
        o.i.get_or_insert(0);
        o.s.get_or_insert(0);
        o.us.get_or_insert(0);
    }
}

enum Found {
    Utc(TimeZone),
    Abbr(&'static tz::abbr::AbbrEntry),
    Id(TimeZone),
}

/// A zone name read by a zone format character: "UTC" exactly is the
/// identifier, then abbreviations (any case), then identifiers (any case,
/// keeping the spelling) (observed on PHP 8.5).
fn lookup_zone(word: &[u8]) -> Option<Found> {
    let w = std::str::from_utf8(word).ok()?;
    if w.is_empty() {
        return None;
    }
    if w == "UTC" {
        return Some(Found::Utc(TimeZone::utc()));
    }
    if let Some(e) = tz::abbr::find(w) {
        return Some(Found::Abbr(e));
    }
    TimeZone::from_id(w).map(Found::Id)
}

/// Reads `input` against `format` (no "now", no zone): the fields read,
/// the zone, and the warnings and errors (the same ones
/// `date_parse_from_format()` and `getLastErrors()` show).
///
/// A NUL byte ends the format (observed on PHP 8.5: "Y\0" reads like
/// "Y"); a NUL in the input is the caller's ValueError ("Argument #2
/// ($datetime) must not contain any null bytes").
pub fn parse_from_format(format: &[u8], input: &[u8]) -> FormatParsed {
    let f = match format.iter().position(|&c| c == 0) {
        Some(n) => &format[..n],
        None => format,
    };
    let mut r = Reader {
        s: input,
        p: 0,
        out: FormatParsed::default(),
        allow_extra: false,
    };
    let mut fp = 0usize;
    while fp < f.len() && r.p < input.len() {
        let begin = r.p;
        match f[fp] {
            b'd' | b'j' => {
                r.out.d = r.number(2).map(|v| v.0);
                if r.out.d.is_none() {
                    r.error(begin, "A two digit day could not be found");
                }
            }
            b'S' => {
                // an English suffix, two letters, skipped when present
                let two = &input[r.p..(r.p + 2).min(input.len())];
                if two.len() == 2
                    && matches!(
                        two.to_ascii_lowercase().as_slice(),
                        b"st" | b"nd" | b"rd" | b"th"
                    )
                {
                    r.p += 2;
                }
            }
            b'z' => {
                // the year check comes between the reader's own errors
                // (test ext/date/tests/bug79580.phpt; observed on PHP 8.5:
                // "pm" gives "Unexpected data found.", this one, then "A
                // three digit day-of-year could not be found")
                let n = r.number(3);
                if r.out.y.is_none() {
                    r.error(
                        begin,
                        "A 'day of year' can only come after a year has been found",
                    );
                }
                match (n, r.out.y) {
                    (None, _) => {
                        r.error(begin, "A three digit day-of-year could not be found");
                    }
                    (Some((v, _)), Some(y)) => {
                        let days = civil::days_from_civil(y, 1, 1) + v;
                        let (y, m, d) = civil::civil_from_days(days);
                        r.out.y = Some(y);
                        r.out.m = Some(m as i64);
                        r.out.d = Some(d as i64);
                    }
                    _ => {}
                }
            }
            b'm' | b'n' => {
                r.out.m = r.number(2).map(|v| v.0);
                if r.out.m.is_none() {
                    r.error(begin, "A two digit month could not be found");
                }
            }
            b'M' | b'F' => r.month_name(),
            b'y' => {
                r.out.y = r.number(2).map(|v| two_digit_year(v.0));
                if r.out.y.is_none() {
                    r.error(begin, "A two digit year could not be found");
                }
            }
            b'Y' => {
                r.out.y = r.number(4).map(|v| v.0);
                if r.out.y.is_none() {
                    r.error(begin, "A four digit year could not be found");
                }
            }
            b'X' | b'x' => {
                let v = r.signed(19);
                r.out.y = Some(v);
            }
            b'a' | b'A' => r.meridian(),
            b'g' | b'h' | b'G' | b'H' => {
                r.out.h = r.number(2).map(|v| v.0);
                match r.out.h {
                    None => r.error(begin, "A two digit hour could not be found"),
                    Some(h) if h > 12 && matches!(f[fp], b'g' | b'h') => {
                        r.error(begin, "Hour cannot be higher than 12")
                    }
                    _ => {}
                }
            }
            b'i' => {
                r.out.i = match r.number(2) {
                    Some((v, 2)) => Some(v),
                    _ => {
                        r.error(begin, "A two digit minute could not be found");
                        None
                    }
                };
            }
            b's' => {
                r.out.s = match r.number(2) {
                    Some((v, 2)) => Some(v),
                    _ => {
                        r.error(begin, "A two digit second could not be found");
                        None
                    }
                };
            }
            b'v' => r.fraction(3, "A three digit millisecond could not be found"),
            b'u' => r.fraction(6, "A six digit microsecond could not be found"),
            b' ' => {
                while matches!(r.at(r.p), b' ' | b'\t') {
                    r.p += 1;
                }
            }
            b'U' => {
                // the fields of the timestamp in UTC and a +00:00 zone
                // (test ext/date/tests/bug76770.phpt: later fields
                // override them)
                // up to 24 digits (observed on PHP 8.5: 31 zeros and a 1
                // leave trailing data at 24)
                let ts = r.signed(24);
                let c = civil::Civil::from_ts(ts);
                let o = &mut r.out;
                o.y = Some(c.y);
                o.m = Some(c.m as i64);
                o.d = Some(c.d as i64);
                o.h = Some(c.h as i64);
                o.i = Some(c.i as i64);
                o.s = Some(c.s as i64);
                let z = o.zone.get_or_insert_with(FormatZone::default);
                z.zone_type = 1;
                z.z = 0;
                z.dst = false;
                z.tz = Some(TimeZone::Offset(0));
            }
            b'e' | b'P' | b'p' | b'T' | b'O' => r.zone(),
            b'#' => {
                if matches!(
                    r.at(r.p),
                    b';' | b':' | b'/' | b'.' | b',' | b'-' | b'(' | b')'
                ) {
                    r.p += 1;
                } else {
                    r.error(begin, E_SEP_ANY);
                }
            }
            c @ (b';' | b':' | b'/' | b'.' | b',' | b'-' | b'(' | b')') => {
                if r.at(r.p) == c {
                    r.p += 1;
                } else {
                    r.error(begin, E_SEP);
                }
            }
            b'!' => r.reset_all(),
            b'|' => r.reset_unset(),
            b'?' => r.p += 1,
            b'\\' => {
                fp += 1;
                match f.get(fp) {
                    Some(&c) if r.at(r.p) == c => r.p += 1,
                    _ => r.error(begin, E_ESCAPED),
                }
            }
            b'*' => {
                // one byte, then up to a separator or a digit
                r.p += 1;
                while r.p < input.len()
                    && !matches!(
                        input[r.p],
                        b' ' | b'\t' | b',' | b';' | b':' | b'/' | b'.' | b'-' | b'0'..=b'9'
                    )
                {
                    r.p += 1;
                }
            }
            b'+' => r.allow_extra = true,
            b'D' | b'l' => r.day_name(),
            c => {
                // any other byte must match, and is consumed even when it
                // does not (observed on PHP 8.5: "Yqm" on "2021;06" reads
                // the month)
                if r.at(r.p) != c {
                    r.error(begin, E_FORMAT_SEP);
                }
                r.p += 1;
            }
        }
        fp += 1;
    }
    if r.p < input.len() {
        // test ext/date/tests/date-lenient.phpt: '+' makes it a warning
        if r.allow_extra {
            r.warning(r.p, E_TRAILING);
        } else {
            r.error(r.p, E_TRAILING);
        }
    } else {
        // the input ended: what is left of the format may only reset
        // fields or allow extra data (test ext/date/tests/bug51866.phpt:
        // a trailing blank needs data)
        while fp < f.len() {
            match f[fp] {
                b'!' => r.reset_all(),
                b'|' => r.reset_unset(),
                b'+' => r.allow_extra = true,
                _ => {
                    r.error(r.p, E_NOT_ENOUGH);
                    break;
                }
            }
            fp += 1;
        }
    }
    // a time field given makes the others zero
    let o = &mut r.out;
    if o.h.is_some() || o.i.is_some() || o.s.is_some() || o.us.is_some() {
        o.h.get_or_insert(0);
        o.i.get_or_insert(0);
        o.s.get_or_insert(0);
        o.us.get_or_insert(0);
    }
    let end = r.p;
    if let (Some(h), Some(i), Some(s)) = (r.out.h, r.out.i, r.out.s)
        && (!(0..=23).contains(&h) || !(0..=59).contains(&i) || !(0..=59).contains(&s))
    {
        r.warning(end, W_TIME);
    }
    if let (Some(y), Some(m), Some(d)) = (r.out.y, r.out.m, r.out.d)
        && (!(1..=12).contains(&m) || d < 1 || d > civil::days_in_month(y, m as u32) as i64)
    {
        r.warning(end, W_DATE);
    }
    r.out
}

impl FormatParsed {
    /// The `date_parse_from_format()` array.
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
            is_localtime: self.zone.is_some(),
            zone_type: self.zone.as_ref().map(|z| z.zone_type),
            zone: None,
            is_dst: None,
            tz_abbr: None,
            tz_id: None,
            relative: None,
        };
        if let Some(z) = &self.zone {
            match z.zone_type {
                1 => {
                    out.zone = Some(z.z);
                    out.is_dst = Some(z.dst);
                }
                2 => {
                    out.zone = Some(z.z);
                    out.is_dst = Some(z.dst);
                    out.tz_abbr = z.abbr.clone();
                }
                3 => {
                    out.tz_abbr = z.abbr.clone();
                    out.tz_id = z.id.as_ref().map(|t| t.name());
                }
                _ => {}
            }
        }
        if let Some(w) = self.weekday {
            out.relative = Some(DateParseRelative {
                ymdhis: [0; 6],
                weekday: Some(w),
                weekdays: None,
                first_day_of_month: false,
                last_day_of_month: false,
            });
        }
        out
    }

    /// The warnings and errors.
    pub fn last_errors(&self) -> LastErrors {
        LastErrors {
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
        }
    }

    /// The zone a created object gets, `None` for the default one.
    fn target_zone(&self) -> Option<TimeZone> {
        let z = self.zone.as_ref()?;
        match z.zone_type {
            1 | 2 => z.tz.clone(),
            3 => z.id.clone(),
            _ => None,
        }
    }

    /// Builds the instant: fields not read come from "now" (`now_ts`,
    /// `now_us`) seen in `zone`, the zone read (if any) replaces `zone`.
    /// Returns `None` when there were errors.
    ///
    /// Observed on PHP 8.5: "now" is taken in the given zone even when the
    /// string names another one; the time of day comes from now only when
    /// no time field was read, and its microseconds only when no date or
    /// time field at all was read; a day name moves forward to that day
    /// unless the date already is one; the local time resolves like the
    /// DateTime constructor's.
    pub fn to_datetime(&self, zone: &TimeZone, now_ts: i64, now_us: u32) -> Option<DateTime> {
        if !self.errors.is_empty() {
            return None;
        }
        let target = self.target_zone().unwrap_or_else(|| zone.clone());
        let now = LocalTime::new(now_ts, now_us, zone);
        let any_field = self.y.is_some()
            || self.m.is_some()
            || self.d.is_some()
            || self.h.is_some()
            || self.i.is_some()
            || self.s.is_some();
        let y = self.y.unwrap_or(now.y);
        let m = self.m.unwrap_or(now.m as i64);
        let mut d = self.d.unwrap_or(now.d as i64);
        let (h, i, s) = match (self.h, self.i, self.s) {
            (None, None, None) => (now.h as i64, now.i as i64, now.s as i64),
            (h, i, s) => (h.unwrap_or(0), i.unwrap_or(0), s.unwrap_or(0)),
        };
        let us = self.us.unwrap_or(if any_field { 0 } else { now_us as i64 });
        if let Some(w) = self.weekday {
            // forward to that day, the same day when it already is; the
            // unit words count as large "day numbers" ("fortnight" from a
            // Thursday is ten days later, observed on PHP 8.5)
            let days = civil::days_from_civil(y, m, d);
            let cur = civil::weekday(days) as i64;
            let diff = w - cur;
            d += if diff < 0 { diff + 7 } else { diff };
        }
        let from_string = matches!(&self.zone, Some(z) if z.zone_type == 3 && z.id.is_some());
        if !from_string {
            return Some(DateTime::from_local(y, m, d, h, i, s, us, target));
        }
        // A zone identifier read from the string takes the earlier reading
        // of a fall-back overlap (observed on PHP 8.5: "2021-10-31 02:30
        // Europe/Paris" is CEST, "... Europe/London" BST, while the same
        // times with the zone given as the argument are CET and GMT).
        let local = civil::civil_to_ts(y, m, d, h, i, s.saturating_add(us.div_euclid(1_000_000)));
        let us = us.rem_euclid(1_000_000) as u32;
        let def = target.local_to_ts(local);
        let mut best = def;
        for probe in [def.saturating_sub(86_400), def.saturating_add(86_400)] {
            let o = target.offset_at(probe) as i64;
            let ts = local.saturating_sub(o);
            if target.offset_at(ts) as i64 == o && ts < best {
                best = ts;
            }
        }
        Some(DateTime::from_ts(best, us, target))
    }
}

/// `DateTime::createFromFormat()` / `date_create_from_format()`.
///
/// `zone` is the zone given to the call, or the default zone
/// (`date_default_timezone_get()`) when it is null; `now_ts` / `now_us`
/// are the current time. `Ok` carries the warnings (`getLastErrors()`
/// shows them as an array, or `false` when there are none); `Err` means
/// the call returns `false` with these warnings and errors.
pub fn create_from_format(
    format: &[u8],
    input: &[u8],
    zone: &TimeZone,
    now_ts: i64,
    now_us: u32,
) -> Result<(DateTime, LastErrors), LastErrors> {
    let p = parse_from_format(format, input);
    match p.to_datetime(zone, now_ts, now_us) {
        Some(dt) => Ok((dt, p.last_errors())),
        None => Err(p.last_errors()),
    }
}
