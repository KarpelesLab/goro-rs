//! DatePeriod: iteration over recurring dates.
//!
//! PHP manual: DatePeriod (constructor forms, EXCLUDE_START_DATE,
//! INCLUDE_END_DATE, createFromISO8601String). Iteration details observed
//! on PHP 8.5: each date is the previous one plus the interval, applied on
//! the wall clock for every unit (a PT30M step through a fall-back overlap
//! goes 01:30 EDT → 02:00 EST) and ignoring `invert`.

use crate::civil;
use crate::interval::Interval;
use crate::parse::{Parsed, Relative};
use crate::resolve::{self, Mode};
use crate::time::DateTime;
use crate::tz::TimeZone;

/// `DatePeriod::EXCLUDE_START_DATE`.
pub const EXCLUDE_START_DATE: i64 = 1;
/// `DatePeriod::INCLUDE_END_DATE`.
pub const INCLUDE_END_DATE: i64 = 2;

/// The bound on recurrence counts: "Recurrence count must be greater or
/// equal to 1 and lower than 2147483640" (observed on PHP 8.5).
pub const MAX_RECURRENCES: i64 = 2_147_483_640;

/// A DatePeriod.
#[derive(Debug, Clone, PartialEq)]
pub struct Period {
    /// `start`.
    pub start: DateTime,
    /// `interval`.
    pub interval: Interval,
    /// `end`, if given.
    pub end: Option<DateTime>,
    /// The recurrence count as given (`getRecurrences()`), if any.
    pub recurrences: Option<i64>,
    /// `include_start_date`.
    pub include_start: bool,
    /// `include_end_date`.
    pub include_end: bool,
}

/// Why a period could not be made (the exception class and message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeriodError {
    /// DateMalformedPeriodStringException.
    MalformedPeriod(String),
    /// DateMalformedStringException.
    Malformed(String),
}

impl Period {
    /// `new DatePeriod($start, $interval, $recurrences|$end, $options)`.
    /// `func` prefixes the recurrence errors ("DatePeriod::__construct").
    pub fn new(
        start: DateTime,
        interval: Interval,
        end: Option<DateTime>,
        recurrences: Option<i64>,
        options: i64,
        func: &str,
    ) -> Result<Period, PeriodError> {
        let include_start = options & EXCLUDE_START_DATE == 0;
        let include_end = options & INCLUDE_END_DATE != 0;
        if end.is_none() {
            let r = recurrences.unwrap_or(0);
            if !(1..MAX_RECURRENCES).contains(&r) {
                return Err(PeriodError::MalformedPeriod(format!(
                    "{func}(): Recurrence count must be greater or equal to 1 and lower than {MAX_RECURRENCES}"
                )));
            }
            if r + i64::from(include_start) + i64::from(include_end) >= MAX_RECURRENCES {
                return Err(PeriodError::Malformed(format!(
                    "{func}(): Recurrence count must be greater or equal to 1 and lower than {MAX_RECURRENCES} (including options)"
                )));
            }
        }
        Ok(Period {
            start,
            interval,
            end,
            recurrences,
            include_start,
            include_end,
        })
    }

    /// `DatePeriod::createFromISO8601String()` / the ISO string
    /// constructor: "Rn/start/interval[/end]" with dates written
    /// "YYYY-MM-DDTHH:MM:SSZ" or "YYYYMMDDTHHMMSSZ" (observed on PHP 8.5:
    /// the count after R is read up to nine digits, later R and P parts
    /// replace and add to earlier ones, a second date is the end).
    pub fn from_iso(spec: &[u8], options: i64, func: &str) -> Result<Period, PeriodError> {
        let bad = || {
            PeriodError::MalformedPeriod(format!(
                "Unknown or bad format ({})",
                String::from_utf8_lossy(spec)
            ))
        };
        let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
        let a = spec.iter().position(|c| !ws(c)).unwrap_or(spec.len());
        let b = spec.iter().rposition(|c| !ws(c)).map_or(a, |e| e + 1);
        let s = &spec[a..b.max(a)];
        let mut recurrences: Option<i64> = None;
        let mut interval: Option<Interval> = None;
        let mut start: Option<DateTime> = None;
        let mut end: Option<DateTime> = None;
        for part in s.split(|&c| c == b'/') {
            if part.is_empty() {
                continue;
            }
            match part[0] {
                b'R' => {
                    let digits = &part[1..];
                    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                        return Err(bad());
                    }
                    let n: i64 = std::str::from_utf8(&digits[..digits.len().min(9)])
                        .unwrap()
                        .parse()
                        .map_err(|_| bad())?;
                    recurrences = Some(n);
                }
                b'P' => {
                    let iv = Interval::parse_duration(part).map_err(|_| bad())?;
                    interval = Some(match interval {
                        None => iv,
                        Some(prev) => Interval {
                            y: prev.y + iv.y,
                            m: prev.m + iv.m,
                            d: prev.d + iv.d,
                            h: prev.h + iv.h,
                            i: prev.i + iv.i,
                            s: prev.s + iv.s,
                            ..prev
                        },
                    });
                }
                _ => {
                    let d = iso_date(part).ok_or_else(bad)?;
                    if start.is_none() {
                        start = Some(d);
                    } else {
                        end = Some(d);
                    }
                }
            }
        }
        let quoted = String::from_utf8_lossy(spec);
        let start = start.ok_or_else(|| {
            PeriodError::MalformedPeriod(format!(
                "{func}(): ISO interval must contain a start date, \"{quoted}\" given"
            ))
        })?;
        let interval = interval.ok_or_else(|| {
            PeriodError::MalformedPeriod(format!(
                "{func}(): ISO interval must contain an interval, \"{quoted}\" given"
            ))
        })?;
        if end.is_none() && !(1..MAX_RECURRENCES).contains(&recurrences.unwrap_or(0)) {
            return Err(PeriodError::MalformedPeriod(format!(
                "{func}(): Recurrence count must be greater or equal to 1 and lower than {MAX_RECURRENCES}"
            )));
        }
        Ok(Period {
            start,
            interval,
            end,
            recurrences,
            include_start: options & EXCLUDE_START_DATE == 0,
            include_end: options & INCLUDE_END_DATE != 0,
        })
    }

    /// The `recurrences` property: the count given plus one for each of
    /// the included start and end (observed on PHP 8.5).
    pub fn recurrences_property(&self) -> i64 {
        self.recurrences.unwrap_or(0) + i64::from(self.include_start) + i64::from(self.include_end)
    }

    /// `getRecurrences()`: the count given, or null when an end was given.
    pub fn get_recurrences(&self) -> Option<i64> {
        if self.end.is_some() && self.recurrences.is_none() {
            return None;
        }
        self.recurrences
    }

    /// One step: `d` plus the interval.
    pub fn step(&self, d: &DateTime) -> DateTime {
        let r = match &self.interval.from_string {
            Some(fs) => fs.relative,
            None => Relative {
                y: self.interval.y,
                m: self.interval.m,
                d: self.interval.d,
                h: self.interval.h,
                i: self.interval.i,
                s: self.interval.s,
                us: self.interval.us,
                ..Default::default()
            },
        };
        resolve::resolve(&Parsed::from_relative(r), d.ts, d.us, &d.zone, Mode::Modify)
    }

    /// The dates, in order, and the value of the `current` property after
    /// the iteration (the first date not yielded).
    pub fn iter(&self) -> PeriodIter<'_> {
        PeriodIter {
            p: self,
            cur: None,
            yielded: 0,
            done: false,
        }
    }
}

/// Iterator over a period's dates.
pub struct PeriodIter<'a> {
    p: &'a Period,
    cur: Option<DateTime>,
    yielded: i64,
    done: bool,
}

impl PeriodIter<'_> {
    /// The `current` property: the last date computed.
    pub fn current(&self) -> Option<&DateTime> {
        self.cur.as_ref()
    }
}

impl Iterator for PeriodIter<'_> {
    type Item = DateTime;

    fn next(&mut self) -> Option<DateTime> {
        if self.done {
            return None;
        }
        let next = match &self.cur {
            None => {
                if self.p.include_start {
                    self.p.start.clone()
                } else {
                    self.p.step(&self.p.start)
                }
            }
            Some(c) => self.p.step(c),
        };
        self.cur = Some(next.clone());
        let stop = match &self.p.end {
            Some(e) => {
                let k = (next.ts, next.us);
                let ke = (e.ts, e.us);
                k > ke || (k == ke && !self.p.include_end)
            }
            None => {
                // the start plus `recurrences` dates
                self.yielded >= self.p.recurrences.unwrap_or(0) + i64::from(self.p.include_start)
            }
        };
        if stop {
            self.done = true;
            return None;
        }
        self.yielded += 1;
        Some(next)
    }
}

/// "YYYY-MM-DDTHH:MM:SSZ" or "YYYYMMDDTHHMMSSZ" as a +00:00 value; fields
/// carry ("24:00:00" is the next day).
fn iso_date(s: &[u8]) -> Option<DateTime> {
    let digits = |r: &[u8]| -> Option<i64> {
        if r.is_empty() || !r.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(r).ok()?.parse().ok()
    };
    let (y, m, d, h, i, sec) = match s.len() {
        20 if s[4] == b'-'
            && s[7] == b'-'
            && s[10] == b'T'
            && s[13] == b':'
            && s[16] == b':'
            && s[19] == b'Z' =>
        {
            (
                digits(&s[0..4])?,
                digits(&s[5..7])?,
                digits(&s[8..10])?,
                digits(&s[11..13])?,
                digits(&s[14..16])?,
                digits(&s[17..19])?,
            )
        }
        16 if s[8] == b'T' && s[15] == b'Z' => (
            digits(&s[0..4])?,
            digits(&s[4..6])?,
            digits(&s[6..8])?,
            digits(&s[9..11])?,
            digits(&s[11..13])?,
            digits(&s[13..15])?,
        ),
        _ => return None,
    };
    let ts = civil::civil_to_ts(y, m, d, h, i, sec);
    Some(DateTime::from_ts(ts, 0, TimeZone::Offset(0)))
}
