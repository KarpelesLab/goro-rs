//! Date formatting and parsing (PHP manual: IntlDateFormatter; patterns
//! are ICU SimpleDateFormat's, i.e. UTS #35 Part 4 "Date Format Patterns").
//!
//! Style patterns come from the CLDR dateFormats/timeFormats of the
//! formatter locale's calendar, joined with the dateTimeFormats glue of the
//! date style (the "atTime" variant where CLDR has one, as ICU 72+ uses:
//! expected output of ext/intl/tests/dateformat_*_icu72-1.phpt). The
//! pattern is fixed when the formatter is created; the names (months,
//! days, eras) follow the type of the formatter's calendar (expected
//! output of dateformat_get_set_calendar_variant_icu70.phpt).

pub mod data;
pub mod format;
pub mod locale;
pub mod parse;
#[rustfmt::skip]
pub mod supplemental;

use crate::calendar::{CalType, Calendar};
use crate::timezone::TimeZone;
use data::Loc;
use format::{Symbols, Tok};
pub use locale::LocaleId;

/// IntlDateFormatter::FULL … SHORT, NONE, PATTERN, RELATIVE_* flag.
pub const FULL: i64 = 0;
pub const LONG: i64 = 1;
pub const MEDIUM: i64 = 2;
pub const SHORT: i64 = 3;
pub const NONE: i64 = -1;
pub const PATTERN: i64 = -2;
pub const RELATIVE: i64 = 128;

/// ICU's pattern when both styles are NONE (observed in
/// ext/intl/tests/dateformat_format_variant3.phpt).
const NONE_NONE_PATTERN: &str = "yyyyMMdd hh:mm a";

/// Whether a style value is valid for a date (`date`) or time style.
pub fn valid_style(s: i64, date: bool) -> bool {
    matches!(s, FULL | LONG | MEDIUM | SHORT | NONE) || (date && matches!(s, 128..=131))
}

fn style_name(s: i64) -> &'static str {
    match s & !RELATIVE {
        0 => "full",
        1 => "long",
        2 => "medium",
        _ => "short",
    }
}

/// The date pattern of a style in a calendar section.
fn date_pattern(loc: Loc, sec: &str, style: i64) -> String {
    let k = format!("dateFormats/{}", style_name(style));
    loc.get(sec, &k)
        .or_else(|| loc.get("generic", &k))
        .or_else(|| loc.get("gregorian", &k))
        .unwrap_or("y-MM-dd")
        .to_string()
}

fn time_pattern(loc: Loc, sec: &str, style: i64) -> String {
    let k = format!("timeFormats/{}", style_name(style));
    loc.get(sec, &k)
        .or_else(|| loc.get("gregorian", &k))
        .unwrap_or("HH:mm:ss")
        .to_string()
}

/// The date-time glue of a date style ("{1} 'at' {0}").
fn glue(loc: Loc, sec: &str, style: i64) -> &'static str {
    let s = style_name(style);
    loc.get(sec, &format!("dateTimeFormats-atTime/standard/{s}"))
        .or_else(|| loc.get(sec, &format!("dateTimeFormats/{s}")))
        .or_else(|| loc.get("gregorian", &format!("dateTimeFormats/{s}")))
        .unwrap_or("{1} {0}")
}

fn join(glue: &str, date: &str, time: &str) -> String {
    glue.replace("{1}", "\u{0}").replace("{0}", time).replace('\u{0}', date)
}

/// The pattern of a (date style, time style) pair for a locale and the
/// calendar its patterns come from.
pub fn styles_pattern(locale: &str, cal: CalType, date_style: i64, time_style: i64) -> String {
    let loc = Loc::resolve(locale);
    let sec = cal.data_section();
    match (date_style, time_style) {
        (NONE, NONE) => NONE_NONE_PATTERN.to_string(),
        (NONE, t) => time_pattern(loc, sec, t),
        (d, NONE) => date_pattern(loc, sec, d),
        (d, t) => join(glue(loc, sec, d), &date_pattern(loc, sec, d), &time_pattern(loc, sec, t)),
    }
}

/// A date formatter (ICU SimpleDateFormat / RelativeDateFormat).
#[derive(Clone, Debug)]
pub struct DateFormatter {
    pub locale: LocaleId,
    pub date_style: i64,
    pub time_style: i64,
    pattern: String,
    toks: Vec<Tok>,
    /// The calendar (type for the names, time zone, week rules).
    pub calendar: Calendar,
    pub lenient: bool,
}

impl DateFormatter {
    /// A formatter for styles or an explicit pattern. The pattern data
    /// comes from the locale's own calendar (`pattern_cal`).
    pub fn new(
        locale: &LocaleId,
        date_style: i64,
        time_style: i64,
        pattern: Option<&str>,
        pattern_cal: CalType,
        calendar: Calendar,
    ) -> DateFormatter {
        let pattern = match pattern {
            Some(p) => p.to_string(),
            None => styles_pattern(&locale.base, pattern_cal, date_style, time_style),
        };
        DateFormatter {
            locale: locale.clone(),
            date_style,
            time_style,
            toks: format::tokenize(&pattern),
            pattern,
            calendar,
            lenient: true,
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn set_pattern(&mut self, p: &str) {
        self.pattern = p.to_string();
        self.toks = format::tokenize(p);
        // an explicit pattern turns relative formatting off
        self.date_style &= !RELATIVE;
    }

    pub fn symbols(&self) -> Symbols {
        Symbols::new(&self.locale.base, self.calendar.cal_type)
    }

    pub fn time_zone(&self) -> &TimeZone {
        self.calendar.time_zone()
    }

    /// Formats an instant in the formatter's calendar and time zone.
    pub fn format_ms(&self, ms: f64) -> Option<String> {
        let c = self.calendar.at(ms).ok()?;
        Some(self.format_cal(&c))
    }

    /// Formats a calendar whose fields are complete.
    pub fn format_cal(&self, c: &Calendar) -> String {
        let sy = Symbols::new(&self.locale.base, c.cal_type);
        if self.date_style & RELATIVE != 0 && self.date_style != NONE {
            if let Some(s) = self.relative(&sy, c) {
                return s;
            }
        }
        format::format(&sy, &self.toks, c)
    }

    fn relative(&self, sy: &Symbols, c: &Calendar) -> Option<String> {
        let now = self.calendar.at(crate::timezone::now_ms()).ok()?;
        let diff = c.field(crate::calendar::JULIAN_DAY) as i64 - now.field(crate::calendar::JULIAN_DAY) as i64;
        if !(-2..=2).contains(&diff) {
            return None;
        }
        let field = match self.date_style & !RELATIVE {
            0 | 1 => "day",
            2 => "day-short",
            _ => "day-narrow",
        };
        let word = sy
            .loc
            .get("fields", &format!("{field}/relative-type-{diff}"))
            .or_else(|| sy.loc.get("fields", &format!("day/relative-type-{diff}")))?;
        if self.time_style == NONE {
            return Some(word.to_string());
        }
        let loc = sy.loc;
        let sec = c.cal_type.data_section();
        let g = loc
            .get(sec, &format!("dateTimeFormats-relative/standard/{}", style_name(self.date_style)))
            .unwrap_or_else(|| glue(loc, sec, self.date_style));
        let time = format::format(sy, &format::tokenize(&time_pattern(loc, sec, self.time_style)), c);
        let g_toks = format::tokenize(g);
        let mut out = String::new();
        for t in g_toks {
            if let Tok::Lit(l) = t {
                out.push_str(&l.replace("{1}", word).replace("{0}", &time));
            }
        }
        Some(out)
    }

    /// Parses `text` from `*pos`; returns the calendar holding the parsed
    /// instant and whether a time zone was parsed. On failure `*pos` is
    /// left unchanged and the error index is returned.
    pub fn parse(&self, text: &str, pos: &mut usize) -> Result<(Calendar, bool), usize> {
        parse::parse(self, &self.toks, text, pos)
    }
}

// ---------------------------------------------------------------- MessageFormat API

/// Formats `udate_ms` with an ICU SimpleDateFormat `pattern` in `locale`
/// (Gregorian calendar) and zone `tz_id` (for MessageFormatter's date and
/// time arguments).
pub fn format_pattern(locale: &str, pattern: &str, udate_ms: f64, tz_id: &str) -> String {
    let l = LocaleId::parse(locale);
    let cal = Calendar::new(CalType::Gregorian, TimeZone::create(tz_id), &l.region_or_likely(), udate_ms);
    let f = DateFormatter::new(&l, NONE, NONE, Some(pattern), CalType::Gregorian, cal);
    f.format_ms(udate_ms).unwrap_or_default()
}

/// The pattern of IntlDateFormatter styles (FULL … SHORT, NONE) for a
/// locale (Gregorian calendar).
pub fn style_pattern(locale: &str, date_style: i64, time_style: i64) -> String {
    styles_pattern(&LocaleId::parse(locale).base, CalType::Gregorian, date_style, time_style)
}

/// The best pattern of a skeleton for a locale (UTS #35
/// availableFormats matching; MessageFormat's "::skeleton" style).
pub fn skeleton_pattern(locale: &str, skeleton: &str) -> String {
    crate::dtpg::best_pattern(&LocaleId::parse(locale).base, skeleton)
}

/// Parses `text` with a pattern in `locale` and zone `tz_id`: the instant
/// in milliseconds and the end position (in bytes), or None.
pub fn parse_pattern(locale: &str, pattern: &str, text: &str, tz_id: &str) -> Option<(f64, usize)> {
    let l = LocaleId::parse(locale);
    let cal = Calendar::new(CalType::Gregorian, TimeZone::create(tz_id), &l.region_or_likely(), 0.0);
    let f = DateFormatter::new(&l, NONE, NONE, Some(pattern), CalType::Gregorian, cal);
    let mut pos = 0;
    let (mut c, _) = f.parse(text, &mut pos).ok()?;
    Some((c.get_time().ok()?, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(locale: &str, d: i64, t: i64, tz: &str, ms: f64) -> String {
        let l = LocaleId::parse(locale);
        let cal = Calendar::new(l.calendar(), TimeZone::create(tz), &l.region_or_likely(), 0.0);
        DateFormatter::new(&l, d, t, None, l.calendar(), cal).format_ms(ms).unwrap()
    }

    #[test]
    fn styles() {
        assert_eq!(
            fmt("en_US", FULL, FULL, "GMT+05:00", 0.0),
            "Thursday, January 1, 1970 at 5:00:00\u{202f}AM GMT+05:00"
        );
        assert_eq!(fmt("en_US", LONG, LONG, "GMT+05:00", 0.0), "January 1, 1970 at 5:00:00\u{202f}AM GMT+5");
        assert_eq!(fmt("en_US", MEDIUM, MEDIUM, "GMT+05:00", 0.0), "Jan 1, 1970, 5:00:00\u{202f}AM");
        assert_eq!(fmt("en_US", SHORT, SHORT, "GMT-10:00", 0.0), "12/31/69, 2:00\u{202f}PM");
        assert_eq!(fmt("en_US", NONE, NONE, "GMT-10:00", 0.0), "19691231 02:00 PM");
        assert_eq!(
            fmt("es_ES", FULL, FULL, "Atlantic/Azores", 1_325_376_000_000.0),
            "sábado, 31 de diciembre de 2011, 23:00:00 (hora estándar de las Azores)"
        );
        assert_eq!(
            fmt("en_US@calendar=hebrew", FULL, FULL, "GMT+05:12", 1_325_376_000_000.0),
            "Sunday, 6 Tevet 5772 at 5:12:00\u{202f}AM GMT+05:12"
        );
    }

    #[test]
    fn patterns() {
        assert_eq!(format_pattern("en_US", "dd-MM-YY", 0.0, "America/New_York"), "31-12-70");
        assert_eq!(format_pattern("en_US", "yyyy-DDD.hh:mm:ss z", 0.0, "America/New_York"), "1969-365.07:00:00 EST");
        assert_eq!(format_pattern("vi_VN", "'tháng' MM, y", 1_428_133_423_941.0, "Asia/Ho_Chi_Minh"), "tháng 04, 2015");
        assert_eq!(format_pattern("ko_KR", "y년 MMM", 1_428_133_423_941.0, "Asia/Ho_Chi_Minh"), "2015년 4월");
    }

    #[test]
    fn parsing() {
        assert_eq!(
            parse_pattern("en_US", "EEEE, MMMM d, y 'at' h:mm:ss\u{202f}a zzzz", "Thursday, January 1, 1970 at 5:00:00\u{202f}AM GMT+05:00", "UTC").map(|x| x.0),
            Some(0.0)
        );
        assert_eq!(parse_pattern("en_GB", "yyyy-MM-dd", "2017-10-12", "UTC").map(|x| x.1), Some(10));
    }
}
