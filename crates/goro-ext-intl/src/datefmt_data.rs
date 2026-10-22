//! CLDR date/time data for IntlDateFormatter, read from the `intl` crate:
//! the symbols (month, weekday, era, day period names) and the style
//! patterns (FULL..SHORT dates and times, and their date-time combiners).
//!
//! `intl` exposes its CLDR patterns only through its formatters, so the
//! style patterns are recovered from tagged formatting output (two probe
//! instants whose fields render unambiguously, see [`pattern_of`]); the
//! name tables come from its per-field accessors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use intl::datetime::{
    self as dt, Calendar, DateStyle, DateTime, DateTimeFormatOptions, DateTimePart,
    DateTimePartType as P, MonthStyle, NameStyle, Numeric2Digit,
};

/// The CLDR data of one locale (gregorian names, style patterns).
#[derive(Debug)]
pub struct LocaleData {
    /// The `intl` language tag ("en-US").
    pub lang: String,
    /// Month names: [wide, abbreviated, narrow][month 0..12].
    pub months: [[String; 12]; 3],
    /// Weekday names: [wide, abbreviated, narrow][Sunday = 0 .. 6].
    pub weekdays: [[String; 7]; 3],
    /// Era names: [wide, abbreviated, narrow][BC, AD].
    pub eras: [[String; 2]; 3],
    /// AM, PM.
    pub ampm: [String; 2],
    /// Date patterns FULL..SHORT.
    pub date: [String; 4],
    /// Time patterns FULL..SHORT.
    pub time: [String; 4],
    /// Date-time combiners FULL..SHORT (by the date style): `{1}` is the
    /// date, `{0}` the time.
    pub glue: [String; 4],
    /// Localized GMT format: prefix ("GMT", "UTC"), the zero format, the
    /// hour/minute separator and the plus/minus signs.
    pub gmt_prefix: String,
    pub gmt_zero: String,
    pub gmt_sep: String,
    pub gmt_plus: String,
    pub gmt_minus: String,
    /// The GMT format places the offset after the prefix ("GMT{0}"); the
    /// suffix after it, if any.
    pub gmt_suffix: String,
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<LocaleData>>> = RefCell::new(HashMap::new());
}

/// The `intl` language tag of an ICU locale ID ("en_US@calendar=x" -> "en-US").
pub fn intl_lang(locale: &str) -> String {
    let base = locale.split(['@', '.']).next().unwrap_or("");
    let t = base.replace('_', "-");
    if t.is_empty() { "en".to_string() } else { t }
}

/// The CLDR data of `locale` (an ICU locale ID).
pub fn locale_data(locale: &str) -> Rc<LocaleData> {
    let lang = intl_lang(locale);
    if let Some(d) = CACHE.with(|c| c.borrow().get(&lang).cloned()) {
        return d;
    }
    let d = Rc::new(load(&lang));
    CACHE.with(|c| c.borrow_mut().insert(lang, d.clone()));
    d
}

fn mk(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond: 0,
    }
}

fn parts(lang: &str, d: &DateTime, o: &DateTimeFormatOptions) -> Vec<DateTimePart> {
    dt::format_to_parts(lang, d, o).unwrap_or_default()
}

fn part_of(ps: &[DateTimePart], k: P) -> Option<String> {
    ps.iter().find(|p| p.kind == k).map(|p| p.value.clone())
}

#[allow(clippy::needless_range_loop)]
fn load(lang: &str) -> LocaleData {
    let mut months: [[String; 12]; 3] = Default::default();
    for (w, st) in [MonthStyle::Long, MonthStyle::Short, MonthStyle::Narrow]
        .into_iter()
        .enumerate()
    {
        for m in 0..12 {
            months[w][m] = dt::month_name(lang, Calendar::Gregory, m as u32 + 1, false, st)
                .unwrap_or_else(|| (m + 1).to_string());
        }
    }
    let mut weekdays: [[String; 7]; 3] = Default::default();
    for (w, st) in [NameStyle::Long, NameStyle::Short, NameStyle::Narrow]
        .into_iter()
        .enumerate()
    {
        for d in 0..7 {
            // 2012-01-01 was a Sunday
            let mut o = DateTimeFormatOptions::default();
            o.weekday = Some(st);
            let ps = parts(lang, &mk(2012, 1, 1 + d as u8, 12, 0, 0), &o);
            weekdays[w][d] = part_of(&ps, P::Weekday).unwrap_or_default();
        }
    }
    let mut eras: [[String; 2]; 3] = Default::default();
    for (w, st) in [NameStyle::Long, NameStyle::Short, NameStyle::Narrow]
        .into_iter()
        .enumerate()
    {
        for e in 0..2 {
            eras[w][e] = dt::era_name(lang, Calendar::Gregory, e as u32, st)
                .unwrap_or(if e == 0 { "BC" } else { "AD" })
                .to_string();
        }
    }
    let mut ampm: [String; 2] = ["AM".to_string(), "PM".to_string()];
    for (i, h) in [3u8, 15].into_iter().enumerate() {
        let mut o = DateTimeFormatOptions::default();
        o.hour = Some(Numeric2Digit::Numeric);
        o.hour12 = Some(true);
        let ps = parts(lang, &mk(2012, 1, 1, h, 0, 0), &o);
        if let Some(v) = part_of(&ps, P::DayPeriod) {
            ampm[i] = v;
        }
    }
    let mut ld = LocaleData {
        lang: lang.to_string(),
        months,
        weekdays,
        eras,
        ampm,
        date: Default::default(),
        time: Default::default(),
        glue: Default::default(),
        gmt_prefix: "GMT".to_string(),
        gmt_zero: "GMT".to_string(),
        gmt_sep: ":".to_string(),
        gmt_plus: "+".to_string(),
        gmt_minus: "-".to_string(),
        gmt_suffix: String::new(),
    };
    let styles = [
        DateStyle::Full,
        DateStyle::Long,
        DateStyle::Medium,
        DateStyle::Short,
    ];
    for (i, st) in styles.into_iter().enumerate() {
        let zone = match i {
            0 => "zzzz",
            1 => "z",
            _ => "",
        };
        let mut o = DateTimeFormatOptions::default();
        o.date_style = Some(st);
        ld.date[i] = pattern_of(&ld, &o, "");
        let mut o = DateTimeFormatOptions::default();
        o.time_style = Some(st);
        o.tz_offset_minutes = Some(0);
        o.time_zone = Some("UTC");
        ld.time[i] = pattern_of(&ld, &o, zone);
        ld.glue[i] = glue_of(lang, st);
    }
    // Localized GMT format
    let zero = dt::format_gmt_offset(lang, 0);
    let pos = dt::format_gmt_offset(lang, 61);
    let neg = dt::format_gmt_offset(lang, -61);
    if let Some(i) = pos.find("01") {
        let head = &pos[..i];
        let sign_len = head.chars().last().map(|c| c.len_utf8()).unwrap_or(0);
        ld.gmt_prefix = head[..head.len() - sign_len].to_string();
        ld.gmt_plus = head[head.len() - sign_len..].to_string();
        let rest = &pos[i + 2..];
        if let Some(j) = rest.find("01") {
            ld.gmt_sep = rest[..j].to_string();
            ld.gmt_suffix = rest[j + 2..].to_string();
        }
        if let Some(k) = neg.find("01") {
            let nh = &neg[..k];
            if let Some(s) = nh.strip_prefix(ld.gmt_prefix.as_str()) {
                ld.gmt_minus = s.to_string();
            }
        }
    }
    ld.gmt_zero = zero;
    ld
}

/// Quote the literal text of a pattern (ICU style: runs of ASCII letters
/// in quotes, a quote doubled).
pub fn quote_literal(s: &str, out: &mut String) {
    let mut in_q = false;
    for ch in s.chars() {
        if ch == '\'' {
            if in_q {
                out.push('\'');
                in_q = false;
            }
            out.push_str("''");
        } else if ch.is_ascii_alphabetic() {
            if !in_q {
                out.push('\'');
                in_q = true;
            }
            out.push(ch);
        } else {
            if in_q {
                out.push('\'');
                in_q = false;
            }
            out.push(ch);
        }
    }
    if in_q {
        out.push('\'');
    }
}

fn idx_of(names: &[String], v: &str) -> bool {
    names.iter().any(|n| n == v)
}

/// Recover the CLDR pattern behind a style format: two probe instants
/// (2345-11-23 15:47:58 and 2005-01-02 03:04:05) tell the fields apart
/// (2-digit years, hour cycles, zero padding, name widths).
fn pattern_of(ld: &LocaleData, o: &DateTimeFormatOptions, zone: &str) -> String {
    let p1 = parts(&ld.lang, &mk(2345, 11, 23, 15, 47, 58), o);
    let p2 = parts(&ld.lang, &mk(2005, 1, 2, 3, 4, 5), o);
    let aligned = p1.len() == p2.len() && p1.iter().zip(&p2).all(|(a, b)| a.kind == b.kind);
    // 2345-11-23 is a Friday
    let wd1 = 5;
    let mut out = String::new();
    for (i, p) in p1.iter().enumerate() {
        let q = if aligned { Some(&p2[i]) } else { None };
        let padded = q.is_some_and(|q| q.value.starts_with('0') && q.value.len() >= 2);
        let v = p.value.as_str();
        match p.kind {
            P::Literal => quote_literal(v, &mut out),
            P::Year => out.push_str(if v.chars().count() == 2 { "yy" } else { "y" }),
            P::Month => {
                if v.chars().all(|c| c.is_ascii_digit()) {
                    out.push_str(if padded { "MM" } else { "M" });
                } else if v == ld.months[0][10] {
                    out.push_str("MMMM");
                } else if v == ld.months[1][10] {
                    out.push_str("MMM");
                } else if v == ld.months[2][10] {
                    out.push_str("MMMMM");
                } else {
                    out.push_str("MMMM");
                }
            }
            P::Day => out.push_str(if padded { "dd" } else { "d" }),
            P::Weekday => {
                if v == ld.weekdays[0][wd1] {
                    out.push_str("EEEE");
                } else if v == ld.weekdays[2][wd1] && !idx_of(&ld.weekdays[1], v) {
                    out.push_str("EEEEE");
                } else {
                    out.push_str("EEE");
                }
            }
            P::Era => {
                if idx_of(&ld.eras[0], v) && !idx_of(&ld.eras[1], v) {
                    out.push_str("GGGG");
                } else if idx_of(&ld.eras[2], v) && !idx_of(&ld.eras[1], v) {
                    out.push_str("GGGGG");
                } else {
                    out.push('G');
                }
            }
            P::Hour => {
                let c = if v == "15" { 'H' } else { 'h' };
                out.push(c);
                if padded {
                    out.push(c);
                }
            }
            P::Minute => out.push_str(if padded { "mm" } else { "m" }),
            P::Second => out.push_str(if padded { "ss" } else { "s" }),
            P::FractionalSecond => {
                for _ in 0..v.chars().count() {
                    out.push('S');
                }
            }
            P::DayPeriod => out.push('a'),
            P::TimeZoneName => out.push_str(if zone.is_empty() { "z" } else { zone }),
            _ => quote_literal(v, &mut out),
        }
    }
    out
}

/// The date-time combiner of a date style: `{1}` the date, `{0}` the time.
fn glue_of(lang: &str, st: DateStyle) -> String {
    let d = mk(2345, 11, 23, 15, 47, 58);
    let mut od = DateTimeFormatOptions::default();
    od.date_style = Some(st);
    let mut ot = DateTimeFormatOptions::default();
    ot.time_style = Some(DateStyle::Short);
    let mut ob = DateTimeFormatOptions::default();
    ob.date_style = Some(st);
    ob.time_style = Some(DateStyle::Short);
    let date = dt::format_options(lang, &d, &od).unwrap_or_default();
    let time = dt::format_options(lang, &d, &ot).unwrap_or_default();
    let both = dt::format_options(lang, &d, &ob).unwrap_or_default();
    let mut out = String::new();
    if let (Some(di), Some(ti)) = (both.find(&date), both.find(&time)) {
        if di < ti && di + date.len() <= ti {
            quote_literal(&both[..di], &mut out);
            out.push_str("{1}");
            quote_literal(&both[di + date.len()..ti], &mut out);
            out.push_str("{0}");
            quote_literal(&both[ti + time.len()..], &mut out);
            return out;
        }
        if ti < di && ti + time.len() <= di {
            quote_literal(&both[..ti], &mut out);
            out.push_str("{0}");
            quote_literal(&both[ti + time.len()..di], &mut out);
            out.push_str("{1}");
            quote_literal(&both[di + date.len()..], &mut out);
            return out;
        }
    }
    "{1}, {0}".to_string()
}

/// ICU's available locales (ResourceBundle::getLocales("")) and, when it
/// differs, the locale the date format data actually comes from
/// (`valid=actual`), generated from ICU 77.
const ICU_LOCALES: &str = include_str!("datefmt_locales.txt");

thread_local! {
    static LOCALES: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        for e in ICU_LOCALES.split_whitespace() {
            match e.split_once('=') {
                Some((v, a)) => m.insert(v, a),
                None => m.insert(e, e),
            };
        }
        m
    };
}

/// The valid and actual locales (ULOC_VALID_LOCALE, ULOC_ACTUAL_LOCALE) of
/// a date format created for `locale`.
pub fn valid_actual(locale: &str) -> (String, String) {
    let base = locale.split('@').next().unwrap_or("");
    let mut cur = base.to_string();
    loop {
        if let Some(a) = LOCALES.with(|m| m.get(cur.as_str()).copied()) {
            return (cur, a.to_string());
        }
        match cur.rfind('_') {
            Some(i) => {
                cur.truncate(i);
                while cur.ends_with('_') {
                    cur.pop();
                }
            }
            None => return ("root".to_string(), "root".to_string()),
        }
    }
}
