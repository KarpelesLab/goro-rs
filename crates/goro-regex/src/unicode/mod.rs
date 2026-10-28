//! Unicode character properties for `\p{..}`, case-insensitive matching and `\X`.
//!
//! The tables are generated from the Unicode Character Database by
//! `tools/gen_regex_unicode.py` (Unicode 16.0.0, the version PHP 8.5's PCRE2 uses:
//! observed on PHP 8.5, U+1C89 is assigned and U+088F is not).

#[rustfmt::skip]
mod tables;

pub use tables::*;

/// Properties reported for values above U+10FFFF: those of an unassigned code point.
static OUT_OF_RANGE: Record = Record {
    gc: GC_CN,
    script: SCRIPT_UNKNOWN,
    gbp: GBP_OTHER,
    bidi: BIDI_UNASSIGNED_DEFAULT,
    caseset: 0,
    scx: 0,
    bprops: 0,
};

/// The property record of code point `c`.
#[inline]
pub fn record(c: u32) -> &'static Record {
    if c > 0x10FFFF {
        return &OUT_OF_RANGE;
    }
    let block = STAGE1[(c >> BLOCK_SHIFT) as usize] as usize;
    let idx = STAGE2[(block << BLOCK_SHIFT) | (c & ((1 << BLOCK_SHIFT) - 1)) as usize];
    &RECORDS[idx as usize]
}

/// General category (index into [`GC_NAMES`]).
#[inline]
pub fn gc(c: u32) -> u8 {
    record(c).gc
}

/// Script (index into [`SCRIPTS`]).
#[inline]
pub fn script(c: u32) -> u8 {
    record(c).script
}

/// Whether `script_idx` is in the Script_Extensions of `c`.
#[inline]
pub fn scx_contains(c: u32, script_idx: u8) -> bool {
    let r = record(c);
    if r.scx == 0 {
        r.script == script_idx
    } else {
        SCX_SETS[r.scx as usize].contains(&script_idx)
    }
}

/// Whether `c` has binary property `idx` (index into [`BPROPS`]).
#[inline]
pub fn has_bprop(c: u32, idx: u8) -> bool {
    BPROP_SETS[record(c).bprops as usize] >> idx & 1 != 0
}

/// The case-equivalence class of `c`, sorted ascending (includes `c`); empty if none.
#[inline]
pub fn case_set(c: u32) -> &'static [u32] {
    CASE_SETS[record(c).caseset as usize]
}

/// Bidi_Class (index into [`BIDI_NAMES`]).
#[inline]
pub fn bidi(c: u32) -> u8 {
    record(c).bidi
}

/// Grapheme_Cluster_Break (index into [`GBP_NAMES`]); `GBP_EXTENDED_PICTOGRAPHIC`
/// marks Extended_Pictographic characters whose break property is Other.
#[inline]
pub fn gbp(c: u32) -> u8 {
    record(c).gbp
}

/// Longest name accepted by the lookups (longer names can't match any table entry).
const MAX_NAME: usize = 64;

/// Looks `name` up in a normalised, sorted table using Unicode loose matching:
/// ASCII case is ignored, as are spaces, '-' and '_'.
fn loose_lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    let mut buf = [0u8; MAX_NAME];
    let mut n = 0;
    for &b in name.as_bytes() {
        if matches!(b, b' ' | b'-' | b'_') {
            continue;
        }
        if n == MAX_NAME {
            return None;
        }
        buf[n] = b.to_ascii_lowercase();
        n += 1;
    }
    let key = &buf[..n];
    table
        .binary_search_by(|(k, _)| k.as_bytes().cmp(key))
        .ok()
        .map(|i| table[i].1)
}

/// A general category by short or long name or alias (e.g. "Lu", "uppercase letter").
pub fn lookup_gc(name: &str) -> Option<u8> {
    loose_lookup(&GC_LOOKUP, name)
}

/// A general category group ("L", "Letter", "LC", "L&", "P", "punct", ...) as a bit mask
/// over [`GC_NAMES`] indices.
pub fn lookup_gc_group(name: &str) -> Option<u32> {
    loose_lookup(&GC_GROUP_LOOKUP, name)
}

/// A script by short or long name or alias (e.g. "Latn", "Latin", "Qaac").
pub fn lookup_script(name: &str) -> Option<u8> {
    loose_lookup(&SCRIPT_LOOKUP, name)
}

/// A binary property by short or long name or alias (e.g. "WSpace", "White_Space", "space").
pub fn lookup_bprop(name: &str) -> Option<u8> {
    loose_lookup(&BPROP_LOOKUP, name)
}

/// A Bidi_Class by short or long name (e.g. "AL", "Arabic_Letter").
pub fn lookup_bidi(name: &str) -> Option<u8> {
    loose_lookup(&BIDI_LOOKUP, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(name: &str) -> u8 {
        lookup_script(name).unwrap()
    }

    #[test]
    fn categories_and_scripts() {
        assert_eq!(gc('A' as u32), GC_LU);
        assert_eq!(script('A' as u32), sc("Latin"));
        assert_eq!(gc(0x300), GC_MN);
        assert_eq!(script(0x300), SCRIPT_INHERITED);
        assert_eq!(gc(0x1C89), GC_LU);
        assert_eq!(script(0x1C89), sc("Cyrillic"));
        assert_eq!(gc(0x88F), GC_CN);
        assert_eq!(script(0x88F), SCRIPT_UNKNOWN);
        assert_eq!(gc(0xD800), GC_CS);
        assert_eq!(gc(0xE000), GC_CO);
        assert_eq!(gc(0x10FFFD), GC_CO);
        assert_eq!(gc(0x10FFFF), GC_CN);
        assert_eq!(gc(0x4E00), GC_LO);
        assert_eq!(script(0x4E00), sc("Han"));
        assert_eq!(gc(0xD7A3), GC_LO);
        assert_eq!(script(' ' as u32), SCRIPT_COMMON);
        assert_eq!(SCRIPTS[0], ("Zzzz", "Unknown"));
        assert!(lookup_script("Hrkt").is_none());
        assert!(lookup_script("Katakana_Or_Hiragana").is_none());
        assert_eq!(lookup_script("Qaac"), lookup_script("Coptic"));
        assert_eq!(lookup_script("qaai"), Some(SCRIPT_INHERITED));
    }

    #[test]
    fn script_extensions() {
        // ScriptExtensions.txt: 0660..0669 ; Arab Thaa Yezi
        assert_eq!(script(0x660), sc("Arabic"));
        assert!(scx_contains(0x660, sc("Arabic")));
        assert!(scx_contains(0x660, sc("Thaana")));
        assert!(scx_contains(0x660, sc("Yezidi")));
        assert!(!scx_contains(0x660, SCRIPT_COMMON));
        assert!(!scx_contains(0x660, sc("Latin")));
        // No entry: Script_Extensions = {Script}.
        assert!(scx_contains('A' as u32, sc("Latin")));
        assert!(!scx_contains('A' as u32, SCRIPT_COMMON));
    }

    #[test]
    fn case_sets() {
        assert_eq!(case_set('k' as u32), &[0x4B, 0x6B, 0x212A]);
        assert_eq!(case_set('K' as u32), &[0x4B, 0x6B, 0x212A]);
        assert!(case_set('s' as u32).contains(&0x17F));
        let sigma = case_set(0x3C3);
        for c in [0x3A3, 0x3C2, 0x3C3] {
            assert!(sigma.contains(&c));
        }
        assert!(case_set(0x130).is_empty());
        assert!(case_set(0x131).is_empty());
        assert!(case_set('1' as u32).is_empty());
        assert_eq!(case_set(0xDF), &[0xDF, 0x1E9E]);
    }

    #[test]
    fn grapheme_and_bidi() {
        assert_eq!(gbp(0x1F600), GBP_EXTENDED_PICTOGRAPHIC);
        assert!(has_bprop(0x1F600, lookup_bprop("Emoji").unwrap()));
        assert!(has_bprop(0x1F600, BPROP_EXTPICT));
        assert_eq!(gbp(0x0D), GBP_CR);
        assert_eq!(gbp(0x0A), GBP_LF);
        assert_eq!(gbp(0x1100), GBP_L);
        assert_eq!(gbp(0xAC00), GBP_LV);
        assert_eq!(gbp(0xAC01), GBP_LVT);
        assert_eq!(gbp(0x200D), GBP_ZWJ);
        assert_eq!(gbp('a' as u32), GBP_OTHER);
        assert_eq!(bidi(0x627), BIDI_AL);
        assert_eq!(bidi('a' as u32), BIDI_L);
        assert_eq!(bidi('1' as u32), BIDI_EN);
        // Unassigned code points in the Hebrew block default to R.
        assert_eq!(gc(0x5FF), GC_CN);
        assert_eq!(bidi(0x5FF), BIDI_R);
    }

    #[test]
    fn binary_properties() {
        let ws = lookup_bprop("white space").unwrap();
        assert_eq!(lookup_bprop("WSpace"), Some(ws));
        assert_eq!(lookup_bprop("space"), Some(ws));
        assert_eq!(BPROPS[ws as usize], ("WSpace", "White_Space"));
        assert!(has_bprop(' ' as u32, ws));
        assert!(has_bprop(0x3000, ws));
        assert!(!has_bprop('a' as u32, ws));
        assert!(has_bprop('a' as u32, BPROP_ALPHA));
        assert!(has_bprop('F' as u32, BPROP_AHEX));
        assert!(has_bprop('(' as u32, BPROP_BIDI_M));
        assert!(!has_bprop('a' as u32, BPROP_BIDI_M));
        assert!(has_bprop(0xFFFE, BPROP_NCHAR));
        assert!(lookup_bprop("Other_Alphabetic").is_none());
        assert_eq!(BPROPS.len(), 55);
    }

    #[test]
    fn name_lookups() {
        assert_eq!(lookup_script("latn"), lookup_script("Latin"));
        assert_eq!(lookup_script("LATIN"), lookup_script("l-a_t i n"));
        assert!(lookup_script("Klingon").is_none());
        assert_eq!(lookup_gc("Lu"), Some(GC_LU));
        assert_eq!(lookup_gc("uppercase letter"), Some(GC_LU));
        assert_eq!(lookup_gc("digit"), Some(GC_ND));
        assert!(lookup_gc("L").is_none());
        let l = lookup_gc_group("L").unwrap();
        assert_eq!(lookup_gc_group("letter"), Some(l));
        assert_ne!(l & 1 << GC_LM, 0);
        let lc = lookup_gc_group("L&").unwrap();
        assert_eq!(lookup_gc_group("LC"), Some(lc));
        assert_eq!(lc & 1 << GC_LM, 0);
        assert_eq!(lookup_bidi("al"), Some(BIDI_AL));
        assert_eq!(lookup_bidi("Arabic Letter"), Some(BIDI_AL));
        assert!(lookup_bidi(&"x".repeat(100)).is_none());
    }

    #[test]
    fn out_of_range() {
        let r = record(0x110000);
        assert_eq!(r.gc, GC_CN);
        assert_eq!(r.script, SCRIPT_UNKNOWN);
        assert_eq!(r.gbp, GBP_OTHER);
        assert_eq!(r.bidi, BIDI_L);
        assert!(case_set(u32::MAX).is_empty());
        assert!(!has_bprop(0x110000, BPROP_NCHAR));
        assert_eq!(UNICODE_VERSION, (16, 0, 0));
    }
}
