//! ICU SimpleDateFormat parsing (SimpleDateFormat::parse / subParse /
//! matchLiterals / matchString), over the same compiled patterns and
//! locale data as the formatter.

use intl::datetime::{self as dt, MonthStyle, NameStyle};

use crate::datecore::{self as dc, CalInfo};
use crate::dateformat::datefmt_data::LocaleData;
use crate::dateformat::datefmt_pattern::{Item, intl_calendar};

/// The fields a parse set.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub f: [Option<i32>; dc::FIELD_COUNT],
    /// A parsed zone: an ICU zone ID ("GMT+05:00", "America/Los_Angeles").
    pub tz: Option<String>,
    /// The fixed offset (ms) of a parsed GMT/ISO zone.
    pub offset: Option<i32>,
}

fn is_numeric_field(ch: char, count: usize) -> bool {
    "ydkHmsSDFwWhKYugAr".contains(ch) || ("MLeQqc".contains(ch) && count < 3)
}

fn is_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{200E}' || c == '\u{200F}'
}

fn fold(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

fn width_idx(count: usize) -> usize {
    match count {
        4 => 0,
        5 => 2,
        _ => 1,
    }
}

/// The longest case-insensitive match of `names` at `text[start..]`
/// (a trailing '.' of a name is optional): (index, chars consumed).
fn match_names<S: AsRef<str>>(text: &[char], start: usize, names: &[S]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (i, n) in names.iter().enumerate() {
        let n = n.as_ref();
        if n.is_empty() {
            continue;
        }
        let nf: Vec<char> = fold(n).chars().collect();
        let try_len = |nf: &[char]| -> Option<usize> {
            if nf.is_empty() || start + nf.len() > text.len() {
                return None;
            }
            let t: String = text[start..start + nf.len()].iter().collect();
            if fold(&t).chars().eq(nf.iter().copied()) {
                Some(nf.len())
            } else {
                None
            }
        };
        let mut m = try_len(&nf);
        if m.is_none() && nf.last() == Some(&'.') {
            m = try_len(&nf[..nf.len() - 1]);
        }
        if let Some(l) = m {
            if best.is_none_or(|(_, bl)| l > bl) {
                best = Some((i, l));
            }
        }
    }
    best
}

/// Parse digits at `pos` (at most `max` chars when obeying the count):
/// (value, chars consumed).
fn parse_int(text: &[char], pos: usize, max: Option<usize>, allow_neg: bool) -> Option<(i64, usize)> {
    let mut i = pos;
    let mut neg = false;
    let end = max.map(|m| (pos + m).min(text.len())).unwrap_or(text.len());
    if allow_neg && i < end && text[i] == '-' {
        neg = true;
        i += 1;
    }
    let ds = i;
    let mut v: i64 = 0;
    while i < end {
        let Some(d) = text[i].to_digit(10) else {
            break;
        };
        v = v.saturating_mul(10).saturating_add(d as i64);
        i += 1;
    }
    if i == ds {
        return None;
    }
    Some((if neg { -v } else { v }, i - pos))
}

fn date_ignorable(c: char) -> bool {
    matches!(c, '-' | ',' | '.' | '/') || is_ws(c)
}

fn time_ignorable(c: char) -> bool {
    matches!(c, '-' | '.' | ':') || is_ws(c)
}

/// The ignorables of the field after a literal (getIgnorables()).
fn ignorable_for(ch: Option<char>) -> fn(char) -> bool {
    match ch {
        Some('y' | 'M' | 'd' | 'c' | 'L') => date_ignorable,
        Some('k' | 'H' | 'm' | 's' | 'h' | 'K') => time_ignorable,
        _ => is_ws,
    }
}

/// matchLiterals(): `lit` at `t0`; the new text position, None on failure.
fn match_literal(
    lit: &str,
    text: &[char],
    t0: usize,
    lenient: bool,
    after_non_numeric: bool,
    next_field: Option<char>,
) -> Option<usize> {
    let lit: Vec<char> = if lenient {
        lit.trim_matches(is_ws).chars().collect()
    } else {
        lit.chars().collect()
    };
    let mut t = t0;
    if lenient {
        while t < text.len() && is_ws(text[t]) {
            t += 1;
        }
    }
    let mut p = 0;
    while p < lit.len() && t < text.len() {
        let mut need_ws = false;
        while p < lit.len() && is_ws(lit[p]) {
            need_ws = true;
            p += 1;
        }
        if need_ws {
            let ts = t;
            while t < text.len() && is_ws(text[t]) {
                t += 1;
            }
            if !lenient && t == ts {
                return None;
            }
            if p >= lit.len() {
                break;
            }
        }
        if t >= text.len() || lit[p] != text[t] {
            if lenient {
                if t == t0 && t < text.len() && text[t] == '.' && after_non_numeric {
                    t += 1;
                    continue;
                }
                break;
            }
            return None;
        }
        p += 1;
        t += 1;
    }
    if !lenient && p < lit.len() {
        return None;
    }
    if p == 0 {
        let ign = ignorable_for(next_field);
        t = t0;
        while t < text.len() && ign(text[t]) {
            t += 1;
        }
    }
    Some(t)
}

/// The ICU custom zone ID of a fixed offset ("GMT+05:00", "GMT" for 0).
pub fn custom_id(off_ms: i32) -> String {
    if off_ms == 0 {
        return "GMT".to_string();
    }
    let a = (off_ms as i64).abs() / 1000;
    let (h, m, s) = (a / 3600, (a / 60) % 60, a % 60);
    let mut r = format!("GMT{}{:02}:{:02}", if off_ms < 0 { '-' } else { '+' }, h, m);
    if s != 0 {
        r.push_str(&format!(":{:02}", s));
    }
    r
}

/// An ISO 8601 offset at `pos` ("Z", "+05", "+0530", "+05:30"): (ms, length).
fn iso_at(text: &[char], pos: usize, allow_z: bool) -> Option<(i32, usize)> {
    let c = *text.get(pos)?;
    if allow_z && (c == 'Z' || c == 'z') {
        return Some((0, 1));
    }
    let sign = match c {
        '+' => 1,
        '-' | '\u{2212}' => -1,
        _ => return None,
    };
    let mut i = pos + 1;
    let (h, n) = parse_int(text, i, Some(2), false)?;
    if n != 2 {
        return None;
    }
    i += 2;
    let mut m = 0;
    let mut s = 0;
    let colon = text.get(i) == Some(&':');
    let j = if colon { i + 1 } else { i };
    if let Some((mm, 2)) = parse_int(text, j, Some(2), false) {
        m = mm;
        i = j + 2;
        let colon2 = text.get(i) == Some(&':');
        let k = if colon2 { i + 1 } else { i };
        if colon2 == colon {
            if let Some((ss, 2)) = parse_int(text, k, Some(2), false) {
                s = ss;
                i = k + 2;
            }
        }
    }
    if h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some((sign * ((h * 3600 + m * 60 + s) * 1000) as i32, i - pos))
}

/// A localized GMT offset at `pos` ("GMT", "GMT+5", "GMT+05:30", "UTC-8").
fn gmt_at(ld: &LocaleData, text: &[char], pos: usize) -> Option<(i32, usize)> {
    for prefix in [ld.gmt_zero.as_str(), ld.gmt_prefix.as_str(), "GMT", "UTC", "UT"] {
        if prefix.is_empty() {
            continue;
        }
        let Some((_, l)) = match_names(text, pos, &[prefix]) else {
            continue;
        };
        let i = pos + l;
        let sign = match text.get(i).copied() {
            Some('+') => 1,
            Some('-' | '\u{2212}') => -1,
            _ => return Some((0, l)),
        };
        let Some((h, n)) = parse_int(text, i + 1, Some(2), false) else {
            return Some((0, l));
        };
        let mut j = i + 1 + n;
        let mut m = 0;
        let mut s = 0;
        if text.get(j) == Some(&':') {
            if let Some((mm, 2)) = parse_int(text, j + 1, Some(2), false) {
                m = mm;
                j += 3;
                if text.get(j) == Some(&':') {
                    if let Some((ss, 2)) = parse_int(text, j + 1, Some(2), false) {
                        s = ss;
                        j += 3;
                    }
                }
            }
        } else if n == 2 {
            if let Some((mm, 2)) = parse_int(text, j, Some(2), false) {
                m = mm;
                j += 2;
            }
        }
        if h > 23 || m > 59 {
            return Some((0, l));
        }
        return Some((sign * ((h * 3600 + m * 60 + s) * 1000) as i32, j - pos));
    }
    None
}

/// The parsing context.
pub struct ParseCtx<'a> {
    pub ld: &'a LocaleData,
    pub info: &'a CalInfo,
    pub locale: &'a str,
    pub lenient: bool,
}

impl ParseCtx<'_> {
    fn greg(&self) -> bool {
        matches!(self.info.cal_type.as_str(), "gregorian" | "iso8601" | "")
    }

    fn month_names(&self, w: usize) -> Vec<String> {
        if self.greg() {
            return self.ld.months[w].to_vec();
        }
        let cal = intl_calendar(&self.info.cal_type);
        let st = [MonthStyle::Long, MonthStyle::Short, MonthStyle::Narrow][w];
        let n = if matches!(
            self.info.cal_type.as_str(),
            "hebrew" | "coptic" | "ethiopic" | "ethiopic-amete-alem"
        ) {
            13
        } else {
            12
        };
        (1..=n)
            .map(|m| dt::month_name(&self.ld.lang, cal, m, false, st).unwrap_or_default())
            .collect()
    }

    fn era_names(&self, w: usize) -> Vec<String> {
        if self.greg() {
            return self.ld.eras[w].to_vec();
        }
        let cal = intl_calendar(&self.info.cal_type);
        let st = [NameStyle::Long, NameStyle::Short, NameStyle::Narrow][w];
        (0..240)
            .map(|e| dt::era_name(&self.ld.lang, cal, e, st).unwrap_or("").to_string())
            .collect()
    }

    /// Parse a zone at `pos` for a zone field: (zone ID, fixed offset, end).
    fn parse_zone(&self, ch: char, count: usize, text: &[char], pos: usize) -> Option<(String, Option<i32>, usize)> {
        let fixed = |off: i32, len: usize| (custom_id(off), Some(off), pos + len);
        match ch {
            'X' | 'x' => {
                let (off, l) = iso_at(text, pos, ch == 'X')?;
                return Some(fixed(off, l));
            }
            'Z' if count < 4 || count == 5 => {
                if let Some((off, l)) = iso_at(text, pos, true) {
                    return Some(fixed(off, l));
                }
                let (off, l) = gmt_at(self.ld, text, pos)?;
                return Some(fixed(off, l));
            }
            'V' if count <= 2 => {
                let mut end = pos;
                while end < text.len()
                    && (text[end].is_ascii_alphanumeric() || "/_+-".contains(text[end]))
                {
                    end += 1;
                }
                while end > pos {
                    let id: String = text[pos..end].iter().collect();
                    if id.contains('/') || count == 2 {
                        if let Some(c) = dc::canonical_tz_id(&id) {
                            let _ = c;
                            return Some((id, None, end));
                        }
                    }
                    end -= 1;
                }
                return None;
            }
            _ => {}
        }
        // Names of the formatter's zone and of UTC, then the GMT and ISO forms
        let mut cands: Vec<(String, String)> = Vec::new();
        for z in [self.info.tz_id.as_str(), "UTC"] {
            for style in [1i64, 2, 3, 4, 7, 8] {
                for dst in [false, true] {
                    let n = dc::tz_display_name(z, dst, style, self.locale);
                    if !n.is_empty() && !n.starts_with(&self.ld.gmt_prefix) {
                        cands.push((n, z.to_string()));
                    }
                }
            }
        }
        let names: Vec<&str> = cands.iter().map(|(n, _)| n.as_str()).collect();
        let named = match_names(text, pos, &names);
        let gmt = gmt_at(self.ld, text, pos);
        match (named, gmt) {
            (Some((i, l)), g) if g.is_none_or(|(_, gl)| l > gl) => {
                return Some((cands[i].1.clone(), None, pos + l));
            }
            (_, Some((off, l))) => return Some(fixed(off, l)),
            _ => {}
        }
        if let Some((off, l)) = iso_at(text, pos, true) {
            return Some(fixed(off, l));
        }
        None
    }

    /// subParse(): Ok(new position) or Err(position of the failure);
    /// Ok(usize::MAX) for an era that is not present (parsing continues).
    #[allow(clippy::too_many_arguments)]
    fn sub_parse(
        &self,
        text: &[char],
        mut start: usize,
        ch: char,
        count: usize,
        obey: bool,
        p: &mut Parsed,
    ) -> Result<usize, usize> {
        loop {
            if start >= text.len() {
                return Err(start);
            }
            if !is_ws(text[start]) {
                break;
            }
            start += 1;
        }
        let max = if obey { Some(count) } else { None };
        if obey && start + count > text.len() {
            return Err(start);
        }
        let numeric_first = matches!(ch, 'k' | 'H' | 'h' | 'K' | 'y' | 'Y' | 'U' | 'S')
            || (matches!(ch, 'e' | 'c' | 'M' | 'L' | 'Q' | 'q') && count <= 2);
        let num = if numeric_first {
            parse_int(text, start, max, true)
        } else {
            None
        };
        let set = |p: &mut Parsed, field: usize, v: i64| p.f[field] = Some(v as i32);
        match ch {
            'k' | 'H' | 'h' | 'K' => {
                let Some((v, l)) = num else {
                    return Err(start);
                };
                if !(0..=24).contains(&v) {
                    return Err(start);
                }
                match ch {
                    'k' => set(p, dc::HOUR_OF_DAY, if v == 24 { 0 } else { v }),
                    'H' => set(p, dc::HOUR_OF_DAY, v),
                    'h' => {
                        p.f[dc::HOUR_OF_DAY] = None;
                        set(p, dc::HOUR, if v == 12 { 0 } else { v })
                    }
                    _ => {
                        p.f[dc::HOUR_OF_DAY] = None;
                        set(p, dc::HOUR, v)
                    }
                }
                return Ok(start + l);
            }
            'y' | 'Y' | 'U' => {
                let Some((mut v, l)) = num else {
                    return Err(start);
                };
                if l == 2
                    && count < 3
                    && ch != 'U'
                    && text[start].is_ascii_digit()
                    && text[start + 1].is_ascii_digit()
                {
                    // the default century: from 80 years ago
                    let now = crate::dateformat::datefmt_ext::now_millis();
                    let g = CalInfo::gregorian(now, "UTC", "");
                    let start_year = dc::compute_fields(&g)[dc::EXTENDED_YEAR] as i64 - 80;
                    let amb = start_year % 100;
                    v += (start_year / 100) * 100 + if v < amb { 100 } else { 0 };
                }
                set(p, if ch == 'Y' { dc::YEAR_WOY } else { dc::YEAR }, v);
                return Ok(start + l);
            }
            'S' => {
                let Some((mut v, l)) = num else {
                    return Err(start);
                };
                let mut i = l;
                while i < 3 {
                    v *= 10;
                    i += 1;
                }
                while i > 3 {
                    v /= 10;
                    i -= 1;
                }
                set(p, dc::MILLISECOND, v);
                return Ok(start + l);
            }
            'G' => {
                let names = self.era_names(width_idx(count));
                return match match_names(text, start, &names) {
                    Some((i, l)) => {
                        set(p, dc::ERA, i as i64);
                        Ok(start + l)
                    }
                    None => Ok(usize::MAX),
                };
            }
            'M' | 'L' => {
                if let Some((v, l)) = num {
                    set(p, dc::MONTH, v - 1);
                    return Ok(start + l);
                }
                for w in [0, 1] {
                    if let Some((i, l)) = match_names(text, start, &self.month_names(w)) {
                        set(p, dc::MONTH, i as i64);
                        return Ok(start + l);
                    }
                }
                if !self.lenient {
                    return Err(start);
                }
            }
            'E' | 'e' | 'c' => {
                if let Some((v, l)) = num {
                    set(p, dc::DOW_LOCAL, v);
                    return Ok(start + l);
                }
                for w in [0, 1, 2] {
                    if let Some((i, l)) = match_names(text, start, &self.ld.weekdays[w]) {
                        set(p, dc::DAY_OF_WEEK, i as i64 + 1);
                        return Ok(start + l);
                    }
                }
                if ch == 'E' || !self.lenient {
                    return Err(start);
                }
            }
            'a' | 'b' | 'B' => {
                return match match_names(text, start, &self.ld.ampm) {
                    Some((i, l)) => {
                        set(p, dc::AM_PM, i as i64);
                        Ok(start + l)
                    }
                    None => Err(start),
                };
            }
            'Q' | 'q' => {
                if let Some((v, l)) = num {
                    set(p, dc::MONTH, (v - 1) * 3);
                    return Ok(start + l);
                }
                let long = ["1st quarter", "2nd quarter", "3rd quarter", "4th quarter"];
                let short = ["Q1", "Q2", "Q3", "Q4"];
                for names in [&long, &short] {
                    if let Some((i, l)) = match_names(text, start, names) {
                        set(p, dc::MONTH, i as i64 * 3);
                        return Ok(start + l);
                    }
                }
                if !self.lenient {
                    return Err(start);
                }
            }
            'z' | 'Z' | 'O' | 'v' | 'V' | 'X' | 'x' => {
                return match self.parse_zone(ch, count, text, start) {
                    Some((id, off, end)) => {
                        p.tz = Some(id);
                        p.offset = off;
                        Ok(end)
                    }
                    None => Err(start),
                };
            }
            _ => {}
        }
        // the generic numeric fields
        let field = match ch {
            'd' => dc::DATE,
            'D' => dc::DAY_OF_YEAR,
            'F' => dc::DAY_OF_WEEK_IN_MONTH,
            'w' => dc::WEEK_OF_YEAR,
            'W' => dc::WEEK_OF_MONTH,
            'm' => dc::MINUTE,
            's' => dc::SECOND,
            'u' | 'r' => dc::EXTENDED_YEAR,
            'g' => dc::JULIAN_DAY,
            'A' => dc::MILLISECONDS_IN_DAY,
            'M' | 'L' | 'Q' | 'q' => dc::MONTH,
            'e' | 'c' => dc::DOW_LOCAL,
            _ => return Err(start),
        };
        let Some((v, l)) = parse_int(text, start, max, true) else {
            return Err(start);
        };
        let v = match ch {
            'M' | 'L' => v - 1,
            'Q' | 'q' => (v - 1) * 3,
            _ => v,
        };
        set(p, field, v);
        Ok(start + l)
    }

    /// SimpleDateFormat::parse(): Ok((fields, end)) or Err(error index).
    pub fn parse(&self, items: &[Item], text: &[char], pos0: usize) -> Result<(Parsed, usize), usize> {
        let mut p = Parsed::default();
        let mut pos = pos0;
        let mut abut_pat: Option<usize> = None;
        let mut abut_start = 0;
        let mut abut_pass = 0;
        let mut i = 0;
        while i < items.len() {
            match &items[i] {
                Item::Field(ch, count) => {
                    let (ch, mut count) = (*ch, *count);
                    if is_numeric_field(ch, count) {
                        if abut_pat.is_none()
                            && matches!(items.get(i + 1), Some(Item::Field(c2, n2)) if is_numeric_field(*c2, *n2))
                        {
                            abut_pat = Some(i);
                            abut_start = pos;
                            abut_pass = 0;
                        }
                    } else {
                        abut_pat = None;
                    }
                    if let Some(ap) = abut_pat {
                        if i == ap {
                            if abut_pass >= count {
                                return Err(abut_start);
                            }
                            count -= abut_pass;
                            abut_pass += 1;
                        }
                        match self.sub_parse(text, pos, ch, count, true, &mut p) {
                            Ok(np) => pos = np,
                            Err(_) => {
                                i = ap;
                                pos = abut_start;
                                continue;
                            }
                        }
                    } else if ch != 'l' {
                        match self.sub_parse(text, pos, ch, count, false, &mut p) {
                            Ok(usize::MAX) => {
                                // era not present: skip the whitespace after it
                                if let Some(Item::Lit(l)) = items.get(i + 1) {
                                    if l.chars().all(is_ws) {
                                        i += 1;
                                    }
                                }
                            }
                            Ok(np) => pos = np,
                            Err(e) => return Err(e),
                        }
                    }
                }
                Item::Lit(l) => {
                    abut_pat = None;
                    let after_non_numeric = match i.checked_sub(1).and_then(|j| items.get(j)) {
                        Some(Item::Field(c, n)) => !is_numeric_field(*c, *n),
                        _ => false,
                    };
                    let next_field = match items.get(i + 1) {
                        Some(Item::Field(c, _)) => Some(*c),
                        _ => None,
                    };
                    match match_literal(l, text, pos, self.lenient, after_non_numeric, next_field) {
                        Some(np) => pos = np,
                        None => return Err(pos),
                    }
                }
            }
            i += 1;
        }
        // trailing "." after a non-numeric field
        if self.lenient && text.get(pos) == Some(&'.') {
            if let Some(Item::Field(c, n)) = items.last() {
                if !is_numeric_field(*c, *n) {
                    pos += 1;
                }
            }
        }
        Ok((p, pos))
    }

    /// The instant of parsed fields (the calendar cleared first: missing
    /// fields from 1970-01-01 00:00:00.000), and the zone to use.
    /// With `keep` (udat_parseCalendar()), the calendar is not cleared: the
    /// fields not parsed keep the calendar's.
    pub fn resolve(&self, p: &Parsed, keep: bool) -> (f64, String) {
        let tz = p.tz.clone().unwrap_or_else(|| self.info.tz_id.clone());
        let base = if keep {
            Some(dc::compute_fields(self.info))
        } else {
            None
        };
        let mut f = [0i32; dc::FIELD_COUNT];
        let greg = self.greg();
        let g = |i: usize| p.f[i].or(base.map(|b| b[i]).filter(|_| i != dc::DAY_OF_YEAR));
        let year = p.f[dc::YEAR]
            .or(p.f[dc::YEAR_WOY])
            .or(p.f[dc::EXTENDED_YEAR])
            .map(|v| v as i64);
        let year = match (year, base) {
            (None, Some(b)) => Some(if greg && b[dc::ERA] == 0 {
                1 - b[dc::YEAR] as i64
            } else if greg {
                b[dc::YEAR] as i64
            } else {
                b[dc::EXTENDED_YEAR] as i64
            }),
            (y, _) => y,
        };
        let mut ext = match year {
            Some(y) => y,
            None if greg => 1970,
            None => {
                // the epoch in this calendar
                let e = CalInfo {
                    millis: 0.0,
                    tz_id: "UTC".to_string(),
                    ..self.info.clone()
                };
                dc::compute_fields(&e)[dc::EXTENDED_YEAR] as i64
            }
        };
        if greg && p.f[dc::ERA] == Some(0) && p.f[dc::YEAR].is_some() {
            ext = 1 - ext;
        }
        f[dc::ERA] = g(dc::ERA).unwrap_or(1);
        f[dc::YEAR] = ext as i32;
        f[dc::EXTENDED_YEAR] = ext as i32;
        let mut month = g(dc::MONTH).unwrap_or(0);
        let mut date = g(dc::DATE).unwrap_or(1);
        if p.f[dc::MONTH].is_none() && p.f[dc::DATE].is_none() && greg {
            if let Some(doy) = p.f[dc::DAY_OF_YEAR] {
                let d = dc::days_from_civil(ext, 1, 1) + doy as i64 - 1;
                let (_, m, dd) = dc::civil_from_days(d);
                month = m as i32 - 1;
                date = dd as i32;
            }
        }
        f[dc::MONTH] = month;
        f[dc::DATE] = date;
        let ampm = p.f[dc::AM_PM].or(base.map(|b| b[dc::AM_PM])).unwrap_or(0);
        f[dc::HOUR_OF_DAY] = match (p.f[dc::HOUR], p.f[dc::HOUR_OF_DAY]) {
            (Some(h), _) => h + 12 * ampm,
            (None, Some(h)) => h,
            (None, None) => match base {
                Some(b) if p.f[dc::AM_PM].is_some() => b[dc::HOUR] + 12 * ampm,
                Some(b) => b[dc::HOUR_OF_DAY],
                None => 12 * ampm,
            },
        };
        f[dc::MINUTE] = g(dc::MINUTE).unwrap_or(0);
        f[dc::SECOND] = g(dc::SECOND).unwrap_or(0);
        f[dc::MILLISECOND] = g(dc::MILLISECOND).unwrap_or(0);
        if let Some(ms) = p.f[dc::MILLISECONDS_IN_DAY] {
            if p.f[dc::HOUR].is_none() && p.f[dc::HOUR_OF_DAY].is_none() {
                f[dc::HOUR_OF_DAY] = ms / 3_600_000;
                f[dc::MINUTE] = (ms / 60_000) % 60;
                f[dc::SECOND] = (ms / 1000) % 60;
                f[dc::MILLISECOND] = ms % 1000;
            }
        }
        let millis = match p.offset {
            Some(off) => {
                let u = CalInfo {
                    tz_id: "UTC".to_string(),
                    ..self.info.clone()
                };
                dc::fields_to_millis(&u, &f) - off as f64
            }
            None => {
                let c = CalInfo {
                    tz_id: tz.clone(),
                    ..self.info.clone()
                };
                dc::fields_to_millis(&c, &f)
            }
        };
        (millis, tz)
    }
}
