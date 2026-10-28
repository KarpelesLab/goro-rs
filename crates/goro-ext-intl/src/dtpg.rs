//! The date time pattern generator (PHP manual:
//! IntlDatePatternGenerator::getBestPattern; UTS #35 Part 4 "Matching
//! Skeletons"): the locale's availableFormats entry closest to a
//! skeleton, with field widths adjusted to the skeleton (date fields only,
//! as ICU does by default), date and time parts joined with the
//! dateTimeFormats glue chosen by the month width; "j"/"J"/"C" are the
//! region's preferred hour cycle (CLDR timeData).

use crate::datefmt::data::Loc;
use crate::datefmt::format::{self, Tok};
use crate::datefmt::supplemental as sup;

/// Field type of a pattern letter.
fn ftype(c: char) -> u8 {
    match c {
        'G' => 0,
        'y' | 'Y' | 'u' | 'U' | 'r' => 1,
        'Q' | 'q' => 2,
        'M' | 'L' => 3,
        'w' | 'W' => 4,
        'd' | 'D' | 'F' | 'g' => 5,
        'E' | 'e' | 'c' => 6,
        'a' | 'b' | 'B' => 7,
        'h' | 'H' | 'k' | 'K' => 8,
        'm' => 9,
        's' | 'S' | 'A' => 10,
        _ => 11,
    }
}

fn is_time_type(t: u8) -> bool {
    t >= 7
}

/// A parsed skeleton: (letter, count) per field type.
type Skel = Vec<(char, usize)>;

fn parse_skel(s: &str) -> Skel {
    let mut v: Skel = Vec::new();
    for c in s.chars() {
        // "a" goes with the 12-hour cycle and is not part of skeletons
        if !c.is_ascii_alphabetic() || c == 'a' {
            continue;
        }
        match v.iter_mut().find(|(l, _)| ftype(*l) == ftype(c)) {
            Some(e) if e.0 == c => e.1 += 1,
            Some(_) => {}
            None => v.push((c, 1)),
        }
    }
    v
}

fn types(s: &Skel) -> Vec<u8> {
    let mut t: Vec<u8> = s.iter().map(|(c, _)| ftype(*c)).collect();
    t.sort();
    t
}

fn is_text(c: char, n: usize) -> bool {
    match c {
        'M' | 'L' | 'Q' | 'q' | 'e' | 'c' => n >= 3,
        'E' | 'G' | 'a' | 'b' | 'B' | 'z' | 'v' => true,
        _ => false,
    }
}

/// The preferred hour letters of a locale: (preferred, allowed first).
fn hour_letters(locale: &str) -> (char, char) {
    let region = crate::timezone::names::locale_region(locale);
    let lang = locale.split('_').next().unwrap_or("");
    let find = |k: &str| sup::TIME_DATA.iter().find(|(r, _, _)| *r == k);
    let e = find(&format!("{lang}_{region}"))
        .or_else(|| find(&region))
        .or_else(|| find("001"));
    match e {
        Some((_, allowed, pref)) => {
            let p = pref.chars().next().unwrap_or('H');
            let a = allowed.chars().next().unwrap_or(p);
            (p, a)
        }
        None => ('H', 'H'),
    }
}

/// Resolves j/J/C in a skeleton.
fn normalize(locale: &str, skel: &str) -> String {
    let (pref, allowed) = hour_letters(locale);
    let mut out = String::new();
    let mut need_ampm = false;
    for c in skel.chars() {
        match c {
            'j' | 'C' => {
                let l = if c == 'j' { pref } else { allowed };
                out.push(l);
                if l == 'h' || l == 'K' {
                    need_ampm = true;
                }
            }
            'J' => out.push(if pref == 'h' || pref == 'K' {
                'h'
            } else {
                pref
            }),
            _ => out.push(c),
        }
    }
    if need_ampm && !out.contains(['a', 'b', 'B']) {
        out.push('a');
    }
    out
}

fn distance(req: &Skel, cand: &Skel) -> u32 {
    let mut d = 0;
    for (rc, rn) in req {
        let Some((cc, cn)) = cand.iter().find(|(c, _)| ftype(*c) == ftype(*rc)) else {
            return u32::MAX;
        };
        if is_text(*rc, *rn) != is_text(*cc, *cn) {
            d += 0x1000;
        }
        if ftype(*rc) == 8 && rc != cc {
            // a different hour cycle
            d += 0x100;
        } else if rc != cc && !(ftype(*rc) == 1 || ftype(*rc) == 3 || ftype(*rc) == 6) {
            d += 0x100;
        }
        d += (*rn as i32 - *cn as i32).unsigned_abs();
    }
    d
}

/// The availableFormats pattern closest to a skeleton with the same field
/// types.
fn best_exact(loc: Loc, req: &Skel) -> Option<(Skel, &'static str)> {
    let want = types(req);
    let mut best: Option<(u32, Skel, &'static str)> = None;
    for (k, v) in loc.entries("gregorian", "dateTimeFormats/availableFormats/") {
        let key = &k["dateTimeFormats/availableFormats/".len()..];
        if key.contains('-') {
            continue;
        }
        let cand = parse_skel(key);
        if types(&cand) != want {
            continue;
        }
        let d = distance(req, &cand);
        if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
            best = Some((d, cand, v));
        }
    }
    best.map(|(_, s, p)| (s, p))
}

/// Adjusts the field widths of a pattern to the requested skeleton.
fn adjust(pattern: &str, req: &Skel) -> String {
    let toks = format::tokenize(pattern);
    let mut out = Vec::new();
    for t in toks {
        match t {
            Tok::Field(c, n) => {
                let ty = ftype(c);
                match req.iter().find(|(rc, _)| ftype(*rc) == ty) {
                    Some((rc, rn)) if !(8..=10).contains(&ty) && ty != 7 => {
                        let letter = if ty == 1 { *rc } else { c };
                        let count = if is_text(c, n) == is_text(*rc, *rn) {
                            if ty == 3 && !is_text(c, n) && *rn > 2 {
                                n
                            } else {
                                *rn
                            }
                        } else {
                            n
                        };
                        out.push(Tok::Field(letter, count));
                    }
                    Some((rc, rn)) if ty == 10 && *rc == 'S' => out.push(Tok::Field('S', *rn)),
                    _ => out.push(Tok::Field(c, n)),
                }
            }
            lit => out.push(lit),
        }
    }
    format::untokenize(&out)
}

fn find_pattern(loc: Loc, req: &Skel) -> Option<String> {
    if req.is_empty() {
        return Some(String::new());
    }
    if let Some((_, p)) = best_exact(loc, req) {
        return Some(adjust(p, req));
    }
    // fields alone: their own pattern
    if req.len() == 1 {
        let (c, n) = req[0];
        return Some(format::untokenize(&[Tok::Field(c, n)]));
    }
    // drop fields until a match exists, then append the rest
    let mut best: Option<(usize, String, Skel)> = None;
    for mask in 1u32..(1 << req.len()) - 1 {
        let sub: Skel = req
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, f)| *f)
            .collect();
        if let Some((_, p)) = best_exact(loc, &sub)
            && best.as_ref().is_none_or(|(n, _, _)| sub.len() > *n)
        {
            best = Some((sub.len(), adjust(p, &sub), sub));
        }
    }
    let (_, mut pat, sub) = best?;
    for f in req {
        if !sub.contains(f) {
            let fp = format::untokenize(&[Tok::Field(f.0, f.1)]);
            pat = format!("{pat} {fp}");
        }
    }
    Some(pat)
}

/// `getBestPattern()`.
pub fn best_pattern(locale: &str, skeleton: &str) -> String {
    let loc = Loc::resolve(locale);
    let req = parse_skel(&normalize(locale, skeleton));
    if let Some((_, p)) = best_exact(loc, &req) {
        return adjust(p, &req);
    }
    let date: Skel = req
        .iter()
        .copied()
        .filter(|(c, _)| !is_time_type(ftype(*c)))
        .collect();
    let time: Skel = req
        .iter()
        .copied()
        .filter(|(c, _)| is_time_type(ftype(*c)))
        .collect();
    if !date.is_empty() && !time.is_empty() {
        let dp = find_pattern(loc, &date).unwrap_or_default();
        let tp = find_pattern(loc, &time).unwrap_or_default();
        let month = date.iter().find(|(c, _)| ftype(*c) == 3).map_or(0, |f| f.1);
        let has_weekday = date.iter().any(|(c, _)| ftype(*c) == 6);
        let style = if month >= 4 {
            if has_weekday { "full" } else { "long" }
        } else if month == 3 {
            "medium"
        } else {
            "short"
        };
        let glue = loc
            .get("gregorian", &format!("dateTimeFormats/{style}"))
            .unwrap_or("{1} {0}");
        return glue
            .replace("{1}", "\u{0}")
            .replace("{0}", &tp)
            .replace('\u{0}', &dp);
    }
    find_pattern(loc, &req).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best() {
        assert_eq!(best_pattern("en_US", "jjmm"), "h:mm\u{202f}a");
        assert_eq!(best_pattern("de_DE", "jjmm"), "HH:mm");
        assert_eq!(best_pattern("en_US", "YYYYMMMdd"), "MMM dd, YYYY");
        assert_eq!(best_pattern("de_DE", "YYYYMMMdd"), "dd. MMM YYYY");
        assert_eq!(
            best_pattern("de_DE", "YYYYMMMddjjmm"),
            "dd. MMM YYYY, HH:mm"
        );
        assert_eq!(best_pattern("en_US", ""), "");
    }
}
