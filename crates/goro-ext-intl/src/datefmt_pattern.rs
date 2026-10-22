//! ICU SimpleDateFormat patterns: compilation, formatting of a calendar's
//! fields with a locale's symbols, and parsing (SimpleDateFormat::subParse).

use intl::datetime::{self as dt, Calendar, MonthStyle, NameStyle};

use crate::datecore::{self as dc, CalInfo};
use crate::dateformat::datefmt_data::{LocaleData, locale_data};

/// The ICU pattern letters (DateFormatSymbols::getPatternUChars()).
pub const PATTERN_CHARS: &str = "GyMdkHmsSEDFwWahKzYeugAZvcLQqVUOXxrbB:";

/// One element of a compiled pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Lit(String),
    Field(char, usize),
}

/// Split a pattern into literals and fields. Unknown letters are kept as
/// fields (they fail at format time, as in ICU).
pub fn compile(pattern: &str) -> Vec<Item> {
    let c: Vec<char> = pattern.chars().collect();
    let mut out: Vec<Item> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\'' {
            if i + 1 < c.len() && c[i + 1] == '\'' {
                lit.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < c.len() {
                if c[i] == '\'' {
                    if i + 1 < c.len() && c[i + 1] == '\'' {
                        lit.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                lit.push(c[i]);
                i += 1;
            }
            i += 1;
        } else if ch.is_ascii_alphabetic() {
            let start = i;
            while i < c.len() && c[i] == ch {
                i += 1;
            }
            if !lit.is_empty() {
                out.push(Item::Lit(std::mem::take(&mut lit)));
            }
            out.push(Item::Field(ch, i - start));
        } else {
            lit.push(ch);
            i += 1;
        }
    }
    if !lit.is_empty() {
        out.push(Item::Lit(lit));
    }
    out
}

/// The `intl` calendar of an ICU calendar type.
pub fn intl_calendar(cal_type: &str) -> Calendar {
    let key = match cal_type {
        "gregorian" => "gregory",
        "ethiopic-amete-alem" => "ethioaa",
        k => k,
    };
    Calendar::from_bcp47(key).unwrap_or(Calendar::Gregory)
}

fn pad(out: &mut String, v: i64, min: usize) {
    let s = v.unsigned_abs().to_string();
    if v < 0 {
        out.push('-');
    }
    for _ in s.len()..min {
        out.push('0');
    }
    out.push_str(&s);
}

fn pad_max(out: &mut String, v: i64, min: usize, max: usize) {
    let s = v.unsigned_abs().to_string();
    let s = if s.len() > max { &s[s.len() - max..] } else { &s[..] };
    if v < 0 {
        out.push('-');
    }
    for _ in s.len()..min {
        out.push('0');
    }
    out.push_str(s);
}

fn width_idx(count: usize) -> usize {
    match count {
        4 => 0,
        5 => 2,
        _ => 1,
    }
}

fn is_hebrew_leap(y: i64) -> bool {
    (7 * y + 1).rem_euclid(19) < 7
}

/// The context of one formatting operation.
pub struct Fmt<'a> {
    pub ld: &'a LocaleData,
    pub info: &'a CalInfo,
    pub f: [i32; dc::FIELD_COUNT],
    pub locale: &'a str,
}

impl<'a> Fmt<'a> {
    pub fn new(ld: &'a LocaleData, info: &'a CalInfo, locale: &'a str) -> Fmt<'a> {
        Fmt {
            ld,
            info,
            f: dc::compute_fields(info),
            locale,
        }
    }

    fn gregorian(&self) -> bool {
        matches!(self.info.cal_type.as_str(), "gregorian" | "iso8601" | "")
    }

    fn month_name(&self, count: usize) -> String {
        let m = self.f[dc::MONTH];
        let w = width_idx(count);
        if self.gregorian() {
            return self.ld.months[w][(m.clamp(0, 11)) as usize].clone();
        }
        let cal = intl_calendar(&self.info.cal_type);
        let st = [MonthStyle::Long, MonthStyle::Short, MonthStyle::Narrow][w];
        let (num, leap) = if self.info.cal_type == "hebrew" {
            let leap_year = is_hebrew_leap(self.f[dc::EXTENDED_YEAR] as i64);
            // ICU: ADAR_1 = 5 only in leap years; month 6 is Adar (II)
            ((m + 1) as u32, leap_year && m == 6)
        } else {
            ((m + 1) as u32, self.f[dc::IS_LEAP_MONTH] != 0)
        };
        dt::month_name(&self.ld.lang, cal, num, leap, st)
            .unwrap_or_else(|| self.ld.months[w][(m.clamp(0, 11)) as usize].clone())
    }

    fn era_name(&self, count: usize) -> String {
        let e = self.f[dc::ERA];
        let w = width_idx(count);
        if self.gregorian() {
            return self.ld.eras[w][(e.clamp(0, 1)) as usize].clone();
        }
        let cal = intl_calendar(&self.info.cal_type);
        let st = [NameStyle::Long, NameStyle::Short, NameStyle::Narrow][w];
        dt::era_name(&self.ld.lang, cal, e.max(0) as u32, st)
            .map(|s| s.to_string())
            .unwrap_or_else(|| e.to_string())
    }

    fn weekday(&self, count: usize) -> String {
        let d = (self.f[dc::DAY_OF_WEEK] - 1).clamp(0, 6) as usize;
        let w = match count {
            4 => 0,
            5 => 2,
            _ => 1,
        };
        self.ld.weekdays[w][d].clone()
    }

    /// The GMT offset (ms) at the instant.
    fn offset(&self) -> i32 {
        self.f[dc::ZONE_OFFSET] + self.f[dc::DST_OFFSET]
    }

    /// Localized GMT format, long ("GMT+05:00") or short ("GMT+5").
    pub fn gmt(&self, long: bool) -> String {
        gmt_format(self.ld, self.offset(), long)
    }

    fn zone_name(&self, style: i64) -> String {
        crate::dateformat::datefmt_ext::tz_display_name_at(
            &self.info.tz_id,
            self.info.millis,
            self.f[dc::DST_OFFSET] != 0,
            style,
            self.locale,
        )
    }

    fn day_period(&self, count: usize, flexible: bool) -> String {
        let h = self.f[dc::HOUR_OF_DAY];
        let mi = self.f[dc::MINUTE];
        let s = self.f[dc::SECOND];
        let ms = self.f[dc::MILLISECOND];
        let at_exact = mi == 0 && s == 0 && ms == 0 && (h == 0 || h == 12);
        if flexible || at_exact {
            let mut o = dt::DateTimeFormatOptions::default();
            o.hour = Some(dt::Numeric2Digit::Numeric);
            o.day_period = Some(match count {
                4 => NameStyle::Long,
                5 => NameStyle::Narrow,
                _ => NameStyle::Short,
            });
            let d = dt::DateTime {
                year: 2000,
                month: 1,
                day: 1,
                hour: h as u8,
                minute: mi as u8,
                second: s as u8,
                millisecond: ms as u16,
            };
            if let Ok(ps) = dt::format_to_parts(&self.ld.lang, &d, &o) {
                if let Some(p) = ps.iter().find(|p| p.kind == dt::DateTimePartType::DayPeriod) {
                    if flexible || p.value != self.ld.ampm[(h >= 12) as usize] {
                        return p.value.clone();
                    }
                }
            }
        }
        self.ld.ampm[(h >= 12) as usize].clone()
    }

    /// Format one field; Err(()) for a letter ICU does not know.
    pub fn field(&self, ch: char, count: usize, out: &mut String) -> Result<(), ()> {
        let f = &self.f;
        match ch {
            'G' => out.push_str(&self.era_name(count)),
            'y' => {
                let y = f[dc::YEAR] as i64;
                if count == 2 {
                    pad_max(out, y, 2, 2)
                } else {
                    pad(out, y, count)
                }
            }
            'Y' => {
                let y = f[dc::YEAR_WOY] as i64;
                if count == 2 {
                    pad_max(out, y, 2, 2)
                } else {
                    pad(out, y, count)
                }
            }
            'u' => pad(out, f[dc::EXTENDED_YEAR] as i64, count),
            'r' => {
                let y = if self.gregorian() {
                    f[dc::EXTENDED_YEAR] as i64
                } else {
                    let g = CalInfo::gregorian(self.info.millis, &self.info.tz_id, self.locale);
                    dc::compute_fields(&g)[dc::EXTENDED_YEAR] as i64
                };
                pad(out, y, count)
            }
            'U' => {
                let cal = intl_calendar(&self.info.cal_type);
                let y = f[dc::YEAR];
                match dt::cyclic_year_name(&self.ld.lang, cal, y.max(0) as u32) {
                    Some(n) if count < 5 => out.push_str(n),
                    _ => pad(out, y as i64, 1),
                }
            }
            'M' | 'L' => {
                if count <= 2 {
                    pad(out, f[dc::MONTH] as i64 + 1, count)
                } else {
                    out.push_str(&self.month_name(count))
                }
            }
            'Q' | 'q' => {
                let q = f[dc::MONTH] / 3 + 1;
                match count {
                    1 | 2 => pad(out, q as i64, count),
                    3 => out.push_str(&format!("Q{}", q)),
                    4 => out.push_str(
                        ["1st quarter", "2nd quarter", "3rd quarter", "4th quarter"]
                            [(q - 1).clamp(0, 3) as usize],
                    ),
                    _ => pad(out, q as i64, 1),
                }
            }
            'd' => pad(out, f[dc::DATE] as i64, count),
            'D' => pad(out, f[dc::DAY_OF_YEAR] as i64, count),
            'F' => pad(out, f[dc::DAY_OF_WEEK_IN_MONTH] as i64, count),
            'w' => pad(out, f[dc::WEEK_OF_YEAR] as i64, count),
            'W' => pad(out, f[dc::WEEK_OF_MONTH] as i64, count),
            'g' => pad(out, f[dc::JULIAN_DAY] as i64, count),
            'A' => pad(out, f[dc::MILLISECONDS_IN_DAY] as i64, count),
            'E' => out.push_str(&self.weekday(count)),
            'e' | 'c' => {
                if count <= 2 {
                    pad(out, f[dc::DOW_LOCAL] as i64, if ch == 'c' { 1 } else { count })
                } else {
                    out.push_str(&self.weekday(count))
                }
            }
            'a' => out.push_str(&self.ld.ampm[(f[dc::AM_PM] != 0) as usize]),
            'b' => out.push_str(&self.day_period(count, false)),
            'B' => out.push_str(&self.day_period(count, true)),
            'h' => {
                let h = if f[dc::HOUR] == 0 { 12 } else { f[dc::HOUR] };
                pad(out, h as i64, count)
            }
            'H' => pad(out, f[dc::HOUR_OF_DAY] as i64, count),
            'K' => pad(out, f[dc::HOUR] as i64, count),
            'k' => {
                let h = if f[dc::HOUR_OF_DAY] == 0 {
                    24
                } else {
                    f[dc::HOUR_OF_DAY]
                };
                pad(out, h as i64, count)
            }
            'm' => pad(out, f[dc::MINUTE] as i64, count),
            's' => pad(out, f[dc::SECOND] as i64, count),
            'S' => {
                let mut v = f[dc::MILLISECOND] as i64;
                if count == 1 {
                    v /= 100;
                } else if count == 2 {
                    v /= 10;
                }
                pad(out, v, count.min(3));
                for _ in 3..count {
                    out.push('0');
                }
            }
            'z' => {
                let style = if count >= 4 { 2 } else { 1 };
                out.push_str(&self.zone_name(style))
            }
            'v' => {
                let style = if count >= 4 { 4 } else { 3 };
                out.push_str(&self.zone_name(style))
            }
            'V' => match count {
                1 => out.push_str(&crate::dateformat::datefmt_ext::tz_short_id(&self.info.tz_id)),
                2 => out.push_str(
                    &dc::canonical_tz_id(&self.info.tz_id).unwrap_or(self.info.tz_id.clone()),
                ),
                3 => out.push_str(&crate::dateformat::datefmt_ext::tz_exemplar_city(
                    &self.info.tz_id,
                    self.locale,
                )),
                _ => out.push_str(&self.zone_name(8)),
            },
            'O' => out.push_str(&self.gmt(count >= 4)),
            'Z' => match count {
                1..=3 => out.push_str(&iso_offset(self.offset(), true, false, false, true)),
                4 => out.push_str(&self.gmt(true)),
                _ => out.push_str(&iso_offset(self.offset(), false, true, false, false)),
            },
            'X' | 'x' => {
                let z = ch == 'X';
                let off = self.offset();
                let s = match count {
                    1 => iso_offset(off, true, false, true, !z),
                    2 => iso_offset(off, true, false, false, !z),
                    3 => iso_offset(off, false, false, false, !z),
                    4 => iso_offset(off, true, true, false, !z),
                    _ => iso_offset(off, false, true, false, !z),
                };
                out.push_str(&s)
            }
            'l' => {}
            _ => return Err(()),
        }
        Ok(())
    }

    pub fn format(&self, items: &[Item]) -> Result<String, ()> {
        let mut out = String::new();
        for it in items {
            match it {
                Item::Lit(s) => out.push_str(s),
                Item::Field(ch, n) => self.field(*ch, *n, &mut out)?,
            }
        }
        Ok(out)
    }
}

/// ISO 8601 offsets: `basic` (no colon), `secs` (seconds when non-zero),
/// `short_hours` (+HH when minutes are zero), `no_z` ("+0000" for UTC
/// rather than "Z").
pub fn iso_offset(off_ms: i32, basic: bool, secs: bool, short_hours: bool, no_z: bool) -> String {
    if off_ms == 0 && !no_z {
        return "Z".to_string();
    }
    let sign = if off_ms < 0 { '-' } else { '+' };
    let a = (off_ms as i64).abs() / 1000;
    let (h, m, s) = (a / 3600, (a / 60) % 60, a % 60);
    let sep = if basic { "" } else { ":" };
    let mut r = format!("{}{:02}", sign, h);
    if short_hours && m == 0 && s == 0 {
        return r;
    }
    r.push_str(&format!("{}{:02}", sep, m));
    if secs && s != 0 {
        r.push_str(&format!("{}{:02}", sep, s));
    }
    r
}

/// The localized GMT format of an offset (TimeZoneFormat::formatOffsetLocalizedGMT).
pub fn gmt_format(ld: &LocaleData, off_ms: i32, long: bool) -> String {
    if off_ms == 0 {
        return ld.gmt_zero.clone();
    }
    let a = (off_ms as i64).abs() / 1000;
    let (h, m, s) = (a / 3600, (a / 60) % 60, a % 60);
    let mut r = ld.gmt_prefix.clone();
    r.push_str(if off_ms < 0 { &ld.gmt_minus } else { &ld.gmt_plus });
    if long {
        r.push_str(&format!("{:02}{}{:02}", h, ld.gmt_sep, m));
        if s != 0 {
            r.push_str(&format!("{}{:02}", ld.gmt_sep, s));
        }
    } else {
        r.push_str(&h.to_string());
        if m != 0 || s != 0 {
            r.push_str(&format!("{}{:02}", ld.gmt_sep, m));
        }
        if s != 0 {
            r.push_str(&format!("{}{:02}", ld.gmt_sep, s));
        }
    }
    r.push_str(&ld.gmt_suffix);
    r
}

/// Format `info` with `pattern` for `locale`; Err(()) for an invalid
/// pattern letter (U_INVALID_FORMAT_ERROR).
pub fn format_pattern(locale: &str, pattern: &str, info: &CalInfo) -> Result<String, ()> {
    let ld = locale_data(locale);
    let items = compile(pattern);
    Fmt::new(&ld, info, locale).format(&items)
}
