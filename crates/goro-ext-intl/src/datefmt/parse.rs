//! Date parsing with a pattern (ICU SimpleDateFormat::parse semantics as
//! the ext/intl tests show them: numeric fields take as many digits as
//! present unless the next field is numeric too (then exactly their
//! count), names match any width case-insensitively, lenient parsing skips
//! whitespace and tolerates missing literal text, zones may be written
//! with a localized GMT format, an ISO offset, an id or a zone name).

use super::DateFormatter;
use super::format::{self, Symbols, Tok};
use crate::calendar::{self as cal, Calendar};
use crate::timezone::{self, TimeZone};

struct P<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> P<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.rest().chars().next() {
            if c.is_whitespace() || c == '\u{202f}' || c == '\u{a0}' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    /// Consumes `t` case-insensitively.
    fn eat_ci(&mut self, t: &str) -> bool {
        let r = self.rest();
        let mut ri = r.char_indices();
        for tc in t.chars() {
            match ri.next() {
                Some((_, rc)) if eq_ci(rc, tc) => {}
                _ => return false,
            }
        }
        let n = ri.next().map_or(r.len(), |(i, _)| i);
        self.pos += n;
        true
    }

    fn digits(&mut self, max: usize) -> Option<(i64, usize)> {
        let r = self.rest();
        let mut v: i64 = 0;
        let mut n = 0;
        let mut bytes = 0;
        for c in r.chars() {
            if n >= max {
                break;
            }
            let Some(d) = c.to_digit(10).or_else(|| unicode_digit(c)) else {
                break;
            };
            v = v.saturating_mul(10).saturating_add(d as i64);
            n += 1;
            bytes += c.len_utf8();
        }
        if n == 0 {
            return None;
        }
        self.pos += bytes;
        Some((v, n))
    }
}

fn unicode_digit(c: char) -> Option<u32> {
    // decimal digits of other scripts (Nd blocks of ten)
    let cp = c as u32;
    for base in [
        0x660u32, 0x6F0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66, 0xE50,
        0xED0, 0xF20, 0x1040, 0xFF10,
    ] {
        if (base..base + 10).contains(&cp) {
            return Some(cp - base);
        }
    }
    None
}

fn eq_ci(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    let sp = |c: char| c.is_whitespace() || c == '\u{202f}' || c == '\u{a0}';
    if sp(a) && sp(b) {
        return true;
    }
    a.to_lowercase().eq(b.to_lowercase())
}

fn is_numeric_field(c: char, n: usize) -> bool {
    match c {
        'y' | 'Y' | 'u' | 'd' | 'D' | 'F' | 'g' | 'w' | 'W' | 'h' | 'H' | 'k' | 'K' | 'm' | 's'
        | 'S' | 'A' | 'r' => true,
        'M' | 'L' | 'Q' | 'q' | 'e' | 'c' => n <= 2,
        _ => false,
    }
}

/// The longest name among candidates matching at the position.
fn match_names(p: &mut P, cands: &[(String, i32)]) -> Option<i32> {
    let mut best: Option<(usize, i32)> = None;
    for (name, v) in cands {
        if name.is_empty() {
            continue;
        }
        let save = p.pos;
        if p.eat_ci(name) {
            let len = p.pos - save;
            if best.is_none_or(|(l, _)| len > l) {
                best = Some((len, *v));
            }
        }
        p.pos = save;
    }
    let (len, v) = best?;
    p.pos += len;
    Some(v)
}

/// Parses with tokens.
pub fn parse(
    f: &DateFormatter,
    toks: &[Tok],
    text: &str,
    pos: &mut usize,
) -> Result<(Calendar, bool), usize> {
    let sy = f.symbols();
    let mut p = P { s: text, pos: *pos };
    if p.pos > text.len() || !text.is_char_boundary(p.pos) {
        return Err(*pos);
    }
    let mut c = f.calendar.clone();
    c.clear();
    let mut zone: Option<TimeZone> = None;
    let mut pm: Option<bool> = None;
    let mut hour12: Option<i32> = None;
    let lenient = f.lenient;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(l) => {
                let lt = l.trim_matches(|c: char| c.is_whitespace() || c == '\u{202f}');
                p.skip_ws();
                if lt.is_empty() {
                    continue;
                }
                let save = p.pos;
                if p.eat_ci(lt) {
                    continue;
                }
                p.pos = save;
                if !lenient {
                    return Err(p.pos);
                }
                // tolerate punctuation the text writes differently or omits
                if lt.chars().all(|c| !c.is_alphanumeric())
                    && let Some(ch) = p.rest().chars().next()
                    && !ch.is_alphanumeric()
                    && lt.contains(ch)
                {
                    p.pos += ch.len_utf8();
                }
            }
            Tok::Field(ch, n) => {
                let (ch, n) = (*ch, *n);
                p.skip_ws();
                let abut = matches!(toks.get(i + 1), Some(Tok::Field(c2, n2)) if is_numeric_field(*c2, *n2));
                let start = p.pos;
                if !field(
                    &sy,
                    &mut p,
                    &mut c,
                    ch,
                    n,
                    abut,
                    &mut zone,
                    &mut pm,
                    &mut hour12,
                ) {
                    return Err(start);
                }
            }
        }
    }
    if let Some(h) = hour12 {
        c.set(cal::HOUR, h);
        if pm.is_none() {
            c.set(cal::AM_PM, 0);
        }
    }
    if let Some(v) = pm {
        c.set(cal::AM_PM, v as i32);
    }
    let parsed_zone = zone.is_some();
    if let Some(z) = zone {
        c.set_time_zone_keep_fields(z);
    }
    if c.get_time().is_err() {
        return Err(*pos);
    }
    if c.complete().is_err() {
        return Err(*pos);
    }
    *pos = p.pos;
    Ok((c, parsed_zone))
}

#[allow(clippy::too_many_arguments)]
fn field(
    sy: &Symbols,
    p: &mut P,
    c: &mut Calendar,
    ch: char,
    n: usize,
    abut: bool,
    zone: &mut Option<TimeZone>,
    pm: &mut Option<bool>,
    hour12: &mut Option<i32>,
) -> bool {
    let num = |p: &mut P| -> Option<(i64, usize)> {
        let max = if abut { n.max(1) } else { 10 };
        let neg = p.rest().starts_with('-') && matches!(ch, 'y' | 'u' | 'Y');
        if neg {
            p.pos += 1;
        }
        let r = p.digits(max)?;
        Some(if neg { (-r.0, r.1) } else { r })
    };
    let set = |c: &mut Calendar, f: usize, v: i64| {
        c.set(f, v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    };
    match ch {
        'y' | 'Y' | 'u' => {
            let Some((v, digits)) = num(p) else {
                return false;
            };
            let v = if n == 2 && digits == 2 && ch != 'u' && sy.cal_type.is_gregorian_like() {
                // two-digit years: the century window starting 80 years ago
                let now_year = crate::calendar::systems::jd_to_greg(
                    crate::calendar::systems::EPOCH_JD + (timezone::now_ms() / 86_400_000.0) as i64,
                )
                .0;
                let start = now_year - 80;
                let mut y = start - start.rem_euclid(100) + v;
                if y < start {
                    y += 100;
                }
                y
            } else {
                v
            };
            let f = match ch {
                'y' => cal::YEAR,
                'Y' => cal::YEAR_WOY,
                _ => cal::EXTENDED_YEAR,
            };
            set(c, f, v);
            true
        }
        'M' | 'L' => {
            if n >= 3 {
                let mut cands = Vec::new();
                for w in [3, 4, 5] {
                    for sa in [false, true] {
                        for m in 0..=sy.cal_type.max_month() {
                            if let Some(s) = format::month_name(sy, sa, w, m, 0) {
                                cands.push((s.to_string(), m));
                            }
                        }
                    }
                }
                if let Some(m) = match_names(p, &cands) {
                    set(c, cal::MONTH, m as i64);
                    return true;
                }
            }
            let Some((v, _)) = num(p) else { return false };
            set(c, cal::MONTH, v - 1);
            true
        }
        'd' => num(p).map(|(v, _)| set(c, cal::DATE, v)).is_some(),
        'D' => num(p).map(|(v, _)| set(c, cal::DAY_OF_YEAR, v)).is_some(),
        'F' => num(p)
            .map(|(v, _)| set(c, cal::DAY_OF_WEEK_IN_MONTH, v))
            .is_some(),
        'w' => num(p).map(|(v, _)| set(c, cal::WEEK_OF_YEAR, v)).is_some(),
        'W' => num(p).map(|(v, _)| set(c, cal::WEEK_OF_MONTH, v)).is_some(),
        'g' => num(p).map(|(v, _)| set(c, cal::JULIAN_DAY, v)).is_some(),
        'A' => num(p)
            .map(|(v, _)| set(c, cal::MILLISECONDS_IN_DAY, v))
            .is_some(),
        'H' => num(p).map(|(v, _)| set(c, cal::HOUR_OF_DAY, v)).is_some(),
        'k' => num(p)
            .map(|(v, _)| set(c, cal::HOUR_OF_DAY, if v == 24 { 0 } else { v }))
            .is_some(),
        'h' | 'K' => match num(p) {
            Some((v, _)) => {
                *hour12 = Some(if ch == 'h' && v == 12 { 0 } else { v as i32 });
                true
            }
            None => false,
        },
        'm' => num(p).map(|(v, _)| set(c, cal::MINUTE, v)).is_some(),
        's' => num(p).map(|(v, _)| set(c, cal::SECOND, v)).is_some(),
        'S' => match num(p) {
            Some((v, digits)) => {
                let ms = match digits {
                    1 => v * 100,
                    2 => v * 10,
                    3 => v,
                    d => v / 10i64.pow(d as u32 - 3),
                };
                set(c, cal::MILLISECOND, ms);
                true
            }
            None => false,
        },
        'E' | 'e' | 'c' => {
            if (ch == 'e' || ch == 'c') && n <= 2 {
                return num(p).map(|(v, _)| set(c, cal::DOW_LOCAL, v)).is_some();
            }
            let mut cands = Vec::new();
            for w in [3, 4, 5, 6] {
                for sa in [false, true] {
                    for d in 1..=7 {
                        if let Some(s) = format::day_name(sy, sa, w, d) {
                            cands.push((s.to_string(), d));
                        }
                    }
                }
            }
            match match_names(p, &cands) {
                Some(d) => {
                    set(c, cal::DAY_OF_WEEK, d as i64);
                    true
                }
                None => false,
            }
        }
        'a' | 'b' | 'B' => {
            let mut cands = Vec::new();
            for w in [3, 4, 5] {
                cands.push((format::ampm_name(sy, w, false).to_string(), 0));
                cands.push((format::ampm_name(sy, w, true).to_string(), 1));
            }
            cands.push(("am".into(), 0));
            cands.push(("pm".into(), 1));
            match match_names(p, &cands) {
                Some(v) => {
                    *pm = Some(v == 1);
                    true
                }
                None => false,
            }
        }
        'G' => {
            let mut cands = Vec::new();
            for w in [3, 4, 5] {
                let max_era = match sy.cal_type {
                    crate::calendar::CalType::Japanese => {
                        crate::datefmt::supplemental::JAPANESE_ERAS.len() as i32 - 1
                    }
                    _ => 1,
                };
                for e in 0..=max_era {
                    if let Some(s) = format::era_name(sy, w, e) {
                        cands.push((s.to_string(), e));
                    }
                }
            }
            match match_names(p, &cands) {
                Some(e) => {
                    set(c, cal::ERA, e as i64);
                    true
                }
                None => false,
            }
        }
        'Q' | 'q' => {
            let Some((v, _)) = num(p) else { return false };
            set(c, cal::MONTH, (v - 1) * 3);
            true
        }
        'z' | 'Z' | 'O' | 'v' | 'V' | 'X' | 'x' => match parse_zone(sy, p, ch) {
            Some(z) => {
                *zone = Some(z);
                true
            }
            None => false,
        },
        'r' => num(p).map(|(v, _)| set(c, cal::EXTENDED_YEAR, v)).is_some(),
        _ => false,
    }
}

/// An offset "+h", "+hh", "+hhmm", "+hh:mm" (sign required): ms.
fn parse_offset(p: &mut P, colon_ok: bool) -> Option<i32> {
    let save = p.pos;
    let sign = match p.rest().chars().next()? {
        '+' => 1,
        '-' | '\u{2212}' => -1,
        _ => return None,
    };
    p.pos += p.rest().chars().next()?.len_utf8();
    let Some((a, na)) = p.digits(4) else {
        p.pos = save;
        return None;
    };
    let (h, m) = match na {
        1 | 2 => {
            let mut m = 0;
            if colon_ok && p.rest().starts_with(':') {
                p.pos += 1;
                match p.digits(2) {
                    Some((mm, 2)) => m = mm,
                    _ => {
                        p.pos = save;
                        return None;
                    }
                }
            }
            (a, m)
        }
        3 => (a / 100, a % 100),
        _ => (a / 100, a % 100),
    };
    if h > 23 || m > 59 {
        p.pos = save;
        return None;
    }
    Some(sign * ((h * 3600 + m * 60) * 1000) as i32)
}

fn parse_zone(sy: &Symbols, p: &mut P, ch: char) -> Option<TimeZone> {
    // ISO "Z"
    if matches!(ch, 'X' | 'Z') && p.rest().starts_with('Z') {
        p.pos += 1;
        return Some(TimeZone::create("UTC"));
    }
    // localized GMT ("GMT", "GMT+5", "UTC-03:00")
    let gmt_zero = sy.loc.get("tz", "gmtZeroFormat").unwrap_or("GMT");
    let gmt_fmt = sy.loc.get("tz", "gmtFormat").unwrap_or("GMT{0}");
    let prefix = gmt_fmt.split("{0}").next().unwrap_or("GMT");
    for pre in [prefix, "GMT", "UTC", "UT"] {
        if pre.is_empty() {
            continue;
        }
        let save = p.pos;
        if p.eat_ci(pre) {
            if let Some(o) = parse_offset(p, true) {
                return Some(TimeZone::fixed(o));
            }
            return Some(TimeZone::create(if pre == "UTC" { "UTC" } else { "GMT" }));
        }
        p.pos = save;
    }
    let save = p.pos;
    if p.eat_ci(gmt_zero) {
        return Some(TimeZone::create("GMT"));
    }
    p.pos = save;
    if let Some(o) = parse_offset(p, true) {
        return Some(TimeZone::fixed(o));
    }
    // a zone id (longest match)
    let r = p.rest();
    let mut best: Option<&str> = None;
    for id in timezone::all_ids() {
        if r.starts_with(id.as_str()) && best.is_none_or(|b| id.len() > b.len()) {
            best = Some(id);
        }
    }
    if let Some(id) = best {
        p.pos += id.len();
        return Some(TimeZone::create(id));
    }
    // zone and metazone names
    let mut bestn: Option<(usize, String)> = None;
    for (name, zone, _) in timezone::names::parse_candidates(sy.loc) {
        let save = p.pos;
        if p.eat_ci(name) {
            let len = p.pos - save;
            if bestn.as_ref().is_none_or(|(l, _)| len > *l) {
                bestn = Some((len, zone));
            }
        }
        p.pos = save;
    }
    if let Some((len, zone)) = bestn {
        p.pos += len;
        return Some(TimeZone::create(&zone));
    }
    None
}
