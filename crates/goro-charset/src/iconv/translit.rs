//! `//TRANSLIT` replacement lists.
//!
//! Each code point has an ordered list of replacement strings; the first one
//! the target charset can encode entirely is used, and "?" when none can
//! (PHP 8.5 with glibc 2.43 iconv in its default C.UTF-8 locale: observed).
//! The lists come from UnicodeData.txt decompositions and CLDR's Latin-ASCII
//! transform, corrected by the observed differences (see
//! `tools/charset/iconv_translit_base.py` and `tools/charset/iconv-data/translit.txt`).

use std::sync::OnceLock;

use super::tables::translit::TRANSLIT;

/// The parsed table: `keys[i]`'s replacements are `pool[offs[i]..offs[i + 1]]`,
/// each one terminated by `u32::MAX`.
struct Rules {
    keys: Vec<u32>,
    offs: Vec<u32>,
    pool: Vec<u32>,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut r = Rules {
            keys: Vec::with_capacity(20_000),
            offs: Vec::with_capacity(20_001),
            pool: Vec::with_capacity(80_000),
        };
        for line in TRANSLIT.split('\n').filter(|l| !l.is_empty()) {
            let (key, cands) = line.split_once(' ').unwrap_or((line, ""));
            let Ok(key) = u32::from_str_radix(key, 16) else {
                continue;
            };
            r.keys.push(key);
            r.offs.push(r.pool.len() as u32);
            for cand in cands.split('\t') {
                r.pool.extend(cand.chars().map(u32::from));
                r.pool.push(u32::MAX);
            }
        }
        r.offs.push(r.pool.len() as u32);
        r
    })
}

/// The replacement candidates for `cp`, in order.
pub(crate) fn candidates(cp: u32) -> impl Iterator<Item = &'static [u32]> {
    let r = rules();
    let pool: &'static [u32] = match r.keys.binary_search(&cp) {
        Ok(i) => &r.pool[r.offs[i] as usize..r.offs[i + 1] as usize],
        Err(_) => &[],
    };
    let mut rest = pool;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .iter()
            .position(|&c| c == u32::MAX)
            .unwrap_or(rest.len());
        let cand = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or(&[]);
        Some(cand)
    })
}
