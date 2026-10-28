//! Date pattern tokens and formatting (UTS #35 Part 4 "Date Field Symbol
//! Table"; quoting as in ICU SimpleDateFormat: text in single quotes is
//! literal, "''" is a quote).

use super::data::Loc;
use crate::calendar::{self as cal, CalType, Calendar};
use crate::timezone::TimeZone;
use crate::timezone::names::{self, Opt, Style};

/// A pattern token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    /// A field: letter and count.
    Field(char, usize),
    /// Literal text.
    Lit(String),
}

/// Whether `c` is a pattern letter.
pub fn is_field_letter(c: char) -> bool {
    "GyYuUrQqMLlwWdDFgEecabBhHKkjJCmsSAzZOvVXx".contains(c)
}

/// Splits a pattern into tokens.
pub fn tokenize(p: &str) -> Vec<Tok> {
    let chars: Vec<char> = p.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    let push_lit = |out: &mut Vec<Tok>, lit: &mut String| {
        if !lit.is_empty() {
            out.push(Tok::Lit(std::mem::take(lit)));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if i + 1 < chars.len() && chars[i + 1] == '\'' {
                lit.push('\'');
                i += 2;
                continue;
            }
            // quoted text up to the closing quote
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        lit.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                lit.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() && is_field_letter(c) {
            push_lit(&mut out, &mut lit);
            let mut n = 1;
            while i + n < chars.len() && chars[i + n] == c {
                n += 1;
            }
            out.push(Tok::Field(c, n));
            i += n;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    push_lit(&mut out, &mut lit);
    out
}

/// Re-serializes tokens as a pattern.
pub fn untokenize(toks: &[Tok]) -> String {
    let mut s = String::new();
    for t in toks {
        match t {
            Tok::Field(c, n) => {
                for _ in 0..*n {
                    s.push(*c);
                }
            }
            Tok::Lit(l) => s.push_str(&quote(l)),
        }
    }
    s
}

/// Quotes literal text for a pattern (letters and quotes).
pub fn quote(l: &str) -> String {
    if l == "'" {
        return "''".into();
    }
    if l.chars().any(|c| c.is_ascii_alphabetic() || c == '\'') {
        let mut s = String::from("'");
        for c in l.chars() {
            if c == '\'' {
                s.push_str("''");
            } else {
                s.push(c);
            }
        }
        s.push('\'');
        s
    } else {
        l.to_string()
    }
}

/// What formatting needs from the locale.
#[derive(Clone, Debug)]
pub struct Symbols {
    pub loc: Loc,
    /// The requested locale's base id (region for zone names).
    pub locale: String,
    /// The calendar whose names are used.
    pub cal_type: CalType,
    /// The numbering system ("latn", "arab", ...).
    pub numbers: &'static str,
}

impl Symbols {
    pub fn new(locale: &str, cal_type: CalType) -> Symbols {
        let loc = Loc::resolve(locale);
        let lang = locale.split('_').next().unwrap_or("en");
        let numbers = intl::number::default_numbering_system(if lang.is_empty() { "en" } else { lang });
        Symbols {
            loc,
            locale: locale.to_string(),
            cal_type,
            numbers,
        }
    }

    fn sec(&self) -> &'static str {
        self.cal_type.data_section()
    }

    /// A calendar string with fallbacks to the Gregorian data.
    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.loc
            .get(self.sec(), key)
            .or_else(|| self.loc.get("gregorian", key))
            .or_else(|| self.loc.get("generic", key))
    }

    /// A name trying several widths/contexts in order.
    fn name(&self, keys: &[String]) -> Option<&'static str> {
        keys.iter().find_map(|k| self.get(k))
    }

    pub fn digits(&self, s: &str) -> String {
        if self.numbers == "latn" {
            s.to_string()
        } else {
            intl::number::to_numbering_system(s, self.numbers)
        }
    }

    pub fn num(&self, v: i64, min: usize) -> String {
        let s = if v < 0 {
            format!("-{:0width$}", -v, width = min)
        } else {
            format!("{v:0min$}")
        };
        self.digits(&s)
    }
}

const DAY_KEYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn width_name(n: usize) -> &'static str {
    match n {
        4 => "wide",
        5 => "narrow",
        6 => "short",
        _ => "abbreviated",
    }
}

/// The CLDR key of the month (ICU MONTH) in a year.
pub fn month_key(ct: CalType, month: i32, eyear: i32) -> String {
    if ct == CalType::Hebrew && month == 6 && crate::calendar::systems::is_hebrew_leap(eyear as i64) {
        return "7-yeartype-leap".into();
    }
    (month + 1).to_string()
}

/// A month name: `standalone` context, width by count (3 abbreviated, 4
/// wide, 5 narrow).
pub fn month_name(sy: &Symbols, standalone: bool, n: usize, month: i32, eyear: i32) -> Option<&'static str> {
    let k = month_key(sy.cal_type, month, eyear);
    let w = width_name(n);
    let (a, b) = if standalone { ("stand-alone", "format") } else { ("format", "stand-alone") };
    sy.name(&[format!("months/{a}/{w}/{k}"), format!("months/{b}/{w}/{k}"), format!("months/{a}/wide/{k}")])
}

/// A weekday name (dow 1 = Sunday).
pub fn day_name(sy: &Symbols, standalone: bool, n: usize, dow: i32) -> Option<&'static str> {
    let k = DAY_KEYS[((dow - 1).rem_euclid(7)) as usize];
    let w = width_name(n);
    let (a, b) = if standalone { ("stand-alone", "format") } else { ("format", "stand-alone") };
    sy.name(&[format!("days/{a}/{w}/{k}"), format!("days/{b}/{w}/{k}"), format!("days/{a}/abbreviated/{k}")])
}

/// An era name: count 1-3 abbreviated, 4 wide, 5 narrow.
pub fn era_name(sy: &Symbols, n: usize, era: i32) -> Option<&'static str> {
    let kind = match n {
        4 => "eraNames",
        5 => "eraNarrow",
        _ => "eraAbbr",
    };
    sy.name(&[format!("eras/{kind}/{era}"), format!("eras/eraAbbr/{era}")])
}

/// The AM/PM marker.
pub fn ampm_name(sy: &Symbols, n: usize, pm: bool) -> &'static str {
    let w = match n {
        4 => "wide",
        5 => "narrow",
        _ => "abbreviated",
    };
    let k = if pm { "pm" } else { "am" };
    sy.name(&[format!("dayPeriods/format/{w}/{k}"), format!("dayPeriods/format/abbreviated/{k}")])
        .unwrap_or(if pm { "PM" } else { "AM" })
}

fn quarter_name(sy: &Symbols, standalone: bool, n: usize, q: i32) -> Option<&'static str> {
    let w = width_name(n.min(5));
    let (a, b) = if standalone { ("stand-alone", "format") } else { ("format", "stand-alone") };
    sy.name(&[format!("quarters/{a}/{w}/{q}"), format!("quarters/{b}/{w}/{q}")])
}

/// Formats the instant `c` holds (fields complete) with tokens.
pub fn format(sy: &Symbols, toks: &[Tok], c: &Calendar) -> String {
    let mut out = String::new();
    for t in toks {
        match t {
            Tok::Lit(l) => out.push_str(l),
            Tok::Field(ch, n) => out.push_str(&format_field(sy, *ch, *n, c)),
        }
    }
    out
}

fn year_str(sy: &Symbols, y: i32, n: usize) -> String {
    if n == 2 {
        sy.num((y as i64).rem_euclid(100), 2)
    } else {
        sy.num(y as i64, n)
    }
}

/// One field.
pub fn format_field(sy: &Symbols, ch: char, n: usize, c: &Calendar) -> String {
    let f = |x: usize| c.field(x);
    let ct = sy.cal_type;
    match ch {
        'G' => era_name(sy, n, f(cal::ERA)).map(str::to_string).unwrap_or_else(|| sy.num(f(cal::ERA) as i64, 1)),
        'y' => {
            let y = f(cal::YEAR);
            year_str(sy, y, n)
        }
        'Y' => year_str(sy, f(cal::YEAR_WOY), n),
        'u' => sy.num(f(cal::EXTENDED_YEAR) as i64, n),
        'U' => {
            let y = f(cal::YEAR);
            sy.get(&format!("cyclicNameSets/years/format/abbreviated/{y}"))
                .map(str::to_string)
                .unwrap_or_else(|| sy.num(y as i64, 1))
        }
        'r' => {
            // the related Gregorian year
            let gy = crate::calendar::systems::jd_to_greg(f(cal::JULIAN_DAY) as i64).0;
            sy.num(gy, n)
        }
        'Q' | 'q' => {
            let q = f(cal::MONTH) / 3 + 1;
            if n <= 2 {
                sy.num(q as i64, n)
            } else {
                quarter_name(sy, ch == 'q', n, q).map(str::to_string).unwrap_or_else(|| q.to_string())
            }
        }
        'M' | 'L' => {
            let m = f(cal::MONTH);
            if n <= 2 {
                let num = if ct == CalType::Hebrew { f(cal::ORDINAL_MONTH) + 1 } else { m + 1 };
                sy.num(num as i64, n)
            } else {
                month_name(sy, ch == 'L', n, m, f(cal::EXTENDED_YEAR))
                    .map(str::to_string)
                    .unwrap_or_else(|| sy.num(m as i64 + 1, 1))
            }
        }
        'l' => String::new(),
        'w' => sy.num(f(cal::WEEK_OF_YEAR) as i64, n.min(2)),
        'W' => sy.num(f(cal::WEEK_OF_MONTH) as i64, 1),
        'd' => sy.num(f(cal::DATE) as i64, n.min(2)),
        'D' => sy.num(f(cal::DAY_OF_YEAR) as i64, n.min(3)),
        'F' => sy.num(f(cal::DAY_OF_WEEK_IN_MONTH) as i64, 1),
        'g' => sy.num(f(cal::JULIAN_DAY) as i64, n),
        'E' => day_name(sy, false, if n <= 3 { 3 } else { n }, f(cal::DAY_OF_WEEK))
            .unwrap_or("")
            .to_string(),
        'e' | 'c' => {
            if n <= 2 {
                sy.num(f(cal::DOW_LOCAL) as i64, if ch == 'c' { 1 } else { n })
            } else {
                day_name(sy, ch == 'c', n, f(cal::DAY_OF_WEEK)).unwrap_or("").to_string()
            }
        }
        'a' | 'b' | 'B' => {
            let ms = f(cal::MILLISECONDS_IN_DAY);
            if ch == 'b' {
                let key = if ms == 0 {
                    Some("midnight")
                } else if ms == 43_200_000 {
                    Some("noon")
                } else {
                    None
                };
                if let Some(k) = key {
                    let w = match n {
                        4 => "wide",
                        5 => "narrow",
                        _ => "abbreviated",
                    };
                    if let Some(s) = sy.get(&format!("dayPeriods/format/{w}/{k}")) {
                        return s.to_string();
                    }
                }
            }
            ampm_name(sy, n, f(cal::AM_PM) == 1).to_string()
        }
        'h' => {
            let h = f(cal::HOUR);
            sy.num(if h == 0 { 12 } else { h } as i64, n.min(2))
        }
        'H' => sy.num(f(cal::HOUR_OF_DAY) as i64, n.min(2)),
        'K' => sy.num(f(cal::HOUR) as i64, n.min(2)),
        'k' => {
            let h = f(cal::HOUR_OF_DAY);
            sy.num(if h == 0 { 24 } else { h } as i64, n.min(2))
        }
        'm' => sy.num(f(cal::MINUTE) as i64, n.min(2)),
        's' => sy.num(f(cal::SECOND) as i64, n.min(2)),
        'S' => {
            let ms = format!("{:03}", f(cal::MILLISECOND));
            let mut s: String = ms.chars().take(n).collect();
            while s.len() < n {
                s.push('0');
            }
            sy.digits(&s)
        }
        'A' => sy.num(f(cal::MILLISECONDS_IN_DAY) as i64, n),
        'z' | 'Z' | 'O' | 'v' | 'V' | 'X' | 'x' => zone_field(sy, ch, n, c),
        _ => String::new(),
    }
}

fn zone_field(sy: &Symbols, ch: char, n: usize, c: &Calendar) -> String {
    let tz: &TimeZone = c.time_zone();
    let offset = c.field(cal::ZONE_OFFSET) + c.field(cal::DST_OFFSET);
    let ms = {
        let mut cc = c.clone();
        cc.get_time().unwrap_or(0.0)
    };
    let name = |style: Style| names::display_name(sy.loc, &sy.locale, tz, ms, style, None);
    match (ch, n) {
        ('z', 1..=3) => name(Style::ShortSpecific),
        ('z', _) => name(Style::LongSpecific),
        ('Z', 1..=3) => names::iso_offset(offset, false, Opt::Required, false, false),
        ('Z', 4) => name(Style::LongGmt),
        ('Z', _) => names::iso_offset(offset, true, Opt::Required, true, true),
        ('O', 1..=3) => name(Style::ShortGmt),
        ('O', _) => name(Style::LongGmt),
        ('v', 1..=3) => name(Style::ShortGeneric),
        ('v', _) => name(Style::LongGeneric),
        ('V', 1) => crate::timezone::short_id(tz.id()).unwrap_or("unk").to_string(),
        ('V', 2) => {
            if tz.is_custom() {
                tz.id().to_string()
            } else {
                tz.canonical_id()
            }
        }
        ('V', 3) => name(Style::ExemplarCity),
        ('V', _) => name(Style::GenericLocation),
        ('X', 1) => names::iso_offset(offset, false, Opt::Optional, false, true),
        ('X', 2) => names::iso_offset(offset, false, Opt::Required, false, true),
        ('X', 3) => names::iso_offset(offset, true, Opt::Required, false, true),
        ('X', 4) => names::iso_offset(offset, false, Opt::Required, true, true),
        ('X', _) => names::iso_offset(offset, true, Opt::Required, true, true),
        ('x', 1) => names::iso_offset(offset, false, Opt::Optional, false, false),
        ('x', 2) => names::iso_offset(offset, false, Opt::Required, false, false),
        ('x', 3) => names::iso_offset(offset, true, Opt::Required, false, false),
        ('x', 4) => names::iso_offset(offset, false, Opt::Required, true, false),
        _ => names::iso_offset(offset, true, Opt::Required, true, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert_eq!(
            tokenize("EEEE, MMMM d, y 'at' h:mm"),
            vec![
                Tok::Field('E', 4),
                Tok::Lit(", ".into()),
                Tok::Field('M', 4),
                Tok::Lit(" ".into()),
                Tok::Field('d', 1),
                Tok::Lit(", ".into()),
                Tok::Field('y', 1),
                Tok::Lit(" at ".into()),
                Tok::Field('h', 1),
                Tok::Lit(":".into()),
                Tok::Field('m', 2),
            ]
        );
        assert_eq!(tokenize("'tháng' MM, y")[0], Tok::Lit("tháng ".into()));
        assert_eq!(tokenize("h 'o''clock'")[1], Tok::Lit(" o'clock".into()));
    }
}
