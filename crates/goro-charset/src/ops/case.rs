//! Case conversion (`mb_strtoupper`, `mb_strtolower`, `mb_convert_case`).
//!
//! Full mappings and full case folding come from the intl crate (Unicode
//! 17, as PHP 8.5's mbstring; test ext/mbstring/tests/unicode_versions.phpt);
//! the simple mappings, simple folding and the Cased/Case_Ignorable
//! properties are generated from the UCD by tools/charset/gen_tables.py.
//! Language-specific mappings are never applied; Greek final sigma is
//! (observed on PHP 8.5).

use crate::codec::ERR;
use crate::tables::case as t;
use crate::{Encoding, Subst, convert};

/// The `mb_convert_case()` modes, numbered as PHP's `MB_CASE_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// `MB_CASE_UPPER`
    Upper = 0,
    /// `MB_CASE_LOWER`
    Lower = 1,
    /// `MB_CASE_TITLE`
    Title = 2,
    /// `MB_CASE_FOLD`
    Fold = 3,
    /// `MB_CASE_UPPER_SIMPLE`
    UpperSimple = 4,
    /// `MB_CASE_LOWER_SIMPLE`
    LowerSimple = 5,
    /// `MB_CASE_TITLE_SIMPLE`
    TitleSimple = 6,
    /// `MB_CASE_FOLD_SIMPLE`
    FoldSimple = 7,
}

impl CaseMode {
    /// The mode of an `MB_CASE_*` value, if valid.
    pub fn from_php(v: i64) -> Option<CaseMode> {
        use CaseMode::*;
        Some(match v {
            0 => Upper,
            1 => Lower,
            2 => Title,
            3 => Fold,
            4 => UpperSimple,
            5 => LowerSimple,
            6 => TitleSimple,
            7 => FoldSimple,
            _ => return None,
        })
    }
}

fn lookup_pair(tab: &[(u32, u32)], cp: u32) -> u32 {
    match tab.binary_search_by_key(&cp, |e| e.0) {
        Ok(i) => tab[i].1,
        Err(_) => cp,
    }
}

fn in_ranges(tab: &[(u32, u32)], cp: u32) -> bool {
    let i = tab.partition_point(|e| e.1 < cp);
    i < tab.len() && tab[i].0 <= cp
}

/// The simple uppercase mapping.
pub fn simple_upper(cp: u32) -> u32 {
    if cp < 0x80 {
        return (cp as u8).to_ascii_uppercase() as u32;
    }
    lookup_pair(&t::SIMPLE_UPPER, cp)
}

/// The simple lowercase mapping.
pub fn simple_lower(cp: u32) -> u32 {
    if cp < 0x80 {
        return (cp as u8).to_ascii_lowercase() as u32;
    }
    lookup_pair(&t::SIMPLE_LOWER, cp)
}

/// The simple titlecase mapping.
pub fn simple_title(cp: u32) -> u32 {
    if cp < 0x80 {
        return (cp as u8).to_ascii_uppercase() as u32;
    }
    lookup_pair(&t::SIMPLE_TITLE, cp)
}

/// The simple case folding (statuses C and S).
pub fn simple_fold(cp: u32) -> u32 {
    if cp < 0x80 {
        return (cp as u8).to_ascii_lowercase() as u32;
    }
    lookup_pair(&t::SIMPLE_FOLD, cp)
}

/// Whether `cp` has the Cased property.
pub fn is_cased(cp: u32) -> bool {
    in_ranges(&t::CASED, cp)
}

/// Whether `cp` has the Case_Ignorable property.
pub fn is_case_ignorable(cp: u32) -> bool {
    in_ranges(&t::CASE_IGNORABLE, cp)
}

fn push_full(cp: u32, out: &mut Vec<u32>, f: fn(char) -> intl::unicode::CaseMapIter) {
    match char::from_u32(cp) {
        Some(c) => out.extend(f(c).map(|c| c as u32)),
        None => out.push(cp),
    }
}

/// Whether the Σ at `i` is word-final (Unicode's Final_Sigma condition): a
/// cased letter before it and none after it, skipping case-ignorables.
fn final_sigma(cps: &[u32], i: usize) -> bool {
    let before = cps[..i]
        .iter()
        .rev()
        .find(|&&c| !is_case_ignorable(c))
        .is_some_and(|&c| is_cased(c));
    let after = cps[i + 1..]
        .iter()
        .find(|&&c| !is_case_ignorable(c))
        .is_some_and(|&c| is_cased(c));
    before && !after
}

/// Maps code points (invalid-input markers pass through).
pub fn convert_case_cps(cps: &[u32], mode: CaseMode, out: &mut Vec<u32>) {
    use CaseMode::*;
    use intl::unicode::{case_fold, to_lowercase, to_titlecase, to_uppercase};
    out.reserve(cps.len());
    let mut in_word = false;
    for (i, &cp) in cps.iter().enumerate() {
        if cp == ERR || cp > 0x10FFFF {
            // Invalid input does not end a word (observed on PHP 8.5).
            out.push(cp);
            continue;
        }
        match mode {
            Upper => push_full(cp, out, to_uppercase),
            UpperSimple => out.push(simple_upper(cp)),
            Fold => push_full(cp, out, case_fold),
            FoldSimple => out.push(simple_fold(cp)),
            // Final sigma applies to the full mappings only (observed on
            // PHP 8.5).
            Lower | LowerSimple => {
                if cp == 0x3A3 && mode == Lower && final_sigma(cps, i) {
                    out.push(0x3C2);
                } else if mode == Lower {
                    push_full(cp, out, to_lowercase);
                } else {
                    out.push(simple_lower(cp));
                }
            }
            Title | TitleSimple => {
                if !in_word {
                    if mode == Title {
                        push_full(cp, out, to_titlecase);
                    } else {
                        out.push(simple_title(cp));
                    }
                } else if cp == 0x3A3 && mode == Title && final_sigma(cps, i) {
                    out.push(0x3C2);
                } else if mode == Title {
                    push_full(cp, out, to_lowercase);
                } else {
                    out.push(simple_lower(cp));
                }
                if is_cased(cp) {
                    in_word = true;
                } else if !is_case_ignorable(cp) {
                    in_word = false;
                }
            }
        }
    }
}

/// `mb_convert_case()`.
pub fn convert_case(s: &[u8], mode: CaseMode, enc: Encoding, sub: &Subst) -> Vec<u8> {
    if s.is_ascii() && enc.is_ascii_compatible() {
        let mut v = s.to_vec();
        match mode {
            CaseMode::Upper | CaseMode::UpperSimple => v.make_ascii_uppercase(),
            CaseMode::Lower | CaseMode::LowerSimple | CaseMode::Fold | CaseMode::FoldSimple => {
                v.make_ascii_lowercase()
            }
            CaseMode::Title | CaseMode::TitleSimple => {
                let mut in_word = false;
                for b in v.iter_mut() {
                    if in_word {
                        b.make_ascii_lowercase();
                    } else {
                        b.make_ascii_uppercase();
                    }
                    let cp = *b as u32;
                    if b.is_ascii_alphabetic() {
                        in_word = true;
                    } else if !is_case_ignorable(cp) {
                        in_word = false;
                    }
                }
            }
        }
        return v;
    }
    let cps = convert::decode(s, enc);
    let mut out = Vec::with_capacity(cps.len());
    convert_case_cps(&cps, mode, &mut out);
    convert::encode(&out, enc, sub).bytes
}

/// `mb_strtoupper()`.
pub fn strtoupper(s: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    convert_case(s, CaseMode::Upper, enc, sub)
}

/// `mb_strtolower()`.
pub fn strtolower(s: &[u8], enc: Encoding, sub: &Subst) -> Vec<u8> {
    convert_case(s, CaseMode::Lower, enc, sub)
}
