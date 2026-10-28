//! The embedded CLDR 48 locale data of the date/time cluster: calendar
//! symbols and patterns, time zone names, relative day names and territory
//! names for every CLDR locale (`data/dates.bin.z`, compiled from cldr-json
//! by tools/gen_intl_dates.py, whose docstring documents the layout).
//!
//! Locale ids are ICU's spelling (`pt_PT`, `zh_Hant_TW`, root = `und`). A
//! lookup takes a resolved [`Loc`] (the CLDR locale the data comes from),
//! a section (a calendar type, "tz", "fields" or "territories") and a key
//! (the cldr-json path below the section, e.g. "months/format/wide/1").

use std::sync::OnceLock;

use super::supplemental as sup;

static PACKED: &[u8] = include_bytes!("../../data/dates.bin.z");

struct Blob {
    b: &'static [u8],
    pool_off: usize,
    keys_off: usize,
    n_keys: usize,
    recs_off: usize,
    locs_off: usize,
    n_locs: usize,
    n_sections: usize,
}

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn blob() -> &'static Blob {
    static B: OnceLock<Blob> = OnceLock::new();
    B.get_or_init(|| {
        let raw = goro_ext_zlib::gzinflate(PACKED, 0).unwrap_or_default();
        let b: &'static [u8] = Box::leak(raw.into_boxed_slice());
        if b.len() < 40 || &b[..4] != b"GID1" {
            return Blob {
                b: &[],
                pool_off: 0,
                keys_off: 0,
                n_keys: 0,
                recs_off: 0,
                locs_off: 0,
                n_locs: 0,
                n_sections: 0,
            };
        }
        Blob {
            b,
            pool_off: u32_at(b, 4) as usize,
            keys_off: u32_at(b, 12) as usize,
            n_keys: u32_at(b, 16) as usize,
            recs_off: u32_at(b, 20) as usize,
            locs_off: u32_at(b, 28) as usize,
            n_locs: u32_at(b, 32) as usize,
            n_sections: u32_at(b, 36) as usize,
        }
    })
}

impl Blob {
    fn str(&self, r: u32) -> &'static str {
        let o = self.pool_off + r as usize;
        let len = u16_at(self.b, o);
        std::str::from_utf8(&self.b[o + 2..o + 2 + len]).unwrap_or("")
    }

    fn key(&self, i: usize) -> &'static str {
        self.str(u32_at(self.b, self.keys_off + 4 * i))
    }

    fn key_index(&self, key: &str) -> Option<usize> {
        let (mut lo, mut hi) = (0, self.n_keys);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.key(mid).as_bytes().cmp(key.as_bytes()) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn loc_row(&self, i: usize) -> usize {
        self.locs_off + i * 4 * (1 + self.n_sections)
    }

    fn loc_name(&self, i: usize) -> &'static str {
        self.str(u32_at(self.b, self.loc_row(i)))
    }

    fn find_loc(&self, id: &str) -> Option<usize> {
        let (mut lo, mut hi) = (0, self.n_locs);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.loc_name(mid).as_bytes().cmp(id.as_bytes()) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn section_rec(&self, loc: usize, sec: usize) -> Option<u32> {
        let r = u32_at(self.b, self.loc_row(loc) + 4 + 4 * sec);
        (r != u32::MAX).then_some(r)
    }

    /// Looks `key` (an index) up in record `rec` and its bases.
    fn lookup(&self, mut rec: u32, key: usize) -> Option<&'static str> {
        let key = key as u16;
        loop {
            let off = u32_at(self.b, self.recs_off + 4 * rec as usize) as usize;
            let base = u32_at(self.b, off);
            let n = u32_at(self.b, off + 4) as usize;
            let keys = off + 8;
            let vals = (keys + 2 * n + 3) & !3;
            let (mut lo, mut hi) = (0, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let k = u16_at(self.b, keys + 2 * mid) as u16;
                match k.cmp(&key) {
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                    std::cmp::Ordering::Equal => {
                        let v = u32_at(self.b, vals + 4 * mid);
                        return (v != u32::MAX).then(|| self.str(v));
                    }
                }
            }
            if base == u32::MAX {
                return None;
            }
            rec = base;
        }
    }

    /// Every key of a record chain starting with `prefix`, with its value.
    fn entries_with_prefix(&self, rec: u32, prefix: &str) -> Vec<(&'static str, &'static str)> {
        // keys are sorted: find the range of keys with the prefix
        let (mut lo, mut hi) = (0, self.n_keys);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.key(mid).as_bytes() < prefix.as_bytes() {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let mut out = Vec::new();
        let mut i = lo;
        while i < self.n_keys && self.key(i).starts_with(prefix) {
            if let Some(v) = self.lookup(rec, i) {
                out.push((self.key(i), v));
            }
            i += 1;
        }
        out
    }
}

/// Section indexes.
pub fn section(name: &str) -> Option<usize> {
    sup::SECTIONS.iter().position(|s| *s == name)
}

/// A CLDR locale of the table (resolved from a requested locale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc(usize);

impl Loc {
    /// The root locale.
    pub fn root() -> Loc {
        Loc(blob().find_loc("und").unwrap_or(0))
    }

    /// The table locale with exactly this ICU id.
    pub fn exact(id: &str) -> Option<Loc> {
        blob().find_loc(id).map(Loc)
    }

    /// The most specific table locale on the CLDR fallback chain of `id`
    /// (an ICU-style id without keywords), root if none.
    pub fn resolve(id: &str) -> Loc {
        let mut cur = id.to_string();
        loop {
            if cur.is_empty() || cur == "root" {
                return Loc::root();
            }
            if let Some(l) = Loc::exact(&cur) {
                return l;
            }
            match parent_locale(&cur) {
                Some(p) => cur = p,
                None => return Loc::root(),
            }
        }
    }

    pub fn id(self) -> &'static str {
        blob().loc_name(self.0)
    }

    pub fn is_root(self) -> bool {
        self.id() == "und"
    }

    /// The parent locale in the table (None at root).
    pub fn parent(self) -> Option<Loc> {
        if self.is_root() {
            return None;
        }
        Some(Loc::resolve(&parent_locale(self.id())?))
    }

    /// The value of `key` in `sec`, through the locale's fallback chain.
    pub fn get(self, sec: &str, key: &str) -> Option<&'static str> {
        let b = blob();
        let s = section(sec)?;
        let k = b.key_index(key)?;
        let mut loc = Some(self);
        while let Some(l) = loc {
            if let Some(r) = b.section_rec(l.0, s) {
                return b.lookup(r, k);
            }
            loc = l.parent();
        }
        None
    }

    /// The entries of `sec` whose key starts with `prefix`.
    pub fn entries(self, sec: &str, prefix: &str) -> Vec<(&'static str, &'static str)> {
        let b = blob();
        let Some(s) = section(sec) else {
            return Vec::new();
        };
        let mut loc = Some(self);
        while let Some(l) = loc {
            if let Some(r) = b.section_rec(l.0, s) {
                return b.entries_with_prefix(r, prefix);
            }
            loc = l.parent();
        }
        Vec::new()
    }

    /// Whether the locale has data for the calendar section `sec`.
    pub fn has_section(self, sec: &str) -> bool {
        section(sec).is_some_and(|s| blob().section_rec(self.0, s).is_some())
    }
}

/// The CLDR parent of a locale id (UTS #35 "Parent Locales": the explicit
/// parentLocales entries, else truncation; root is "und").
pub fn parent_locale(id: &str) -> Option<String> {
    if id == "und" || id.is_empty() {
        return None;
    }
    if let Ok(i) = sup::PARENT_LOCALES.binary_search_by(|(k, _)| k.as_bytes().cmp(id.as_bytes())) {
        return Some(sup::PARENT_LOCALES[i].1.to_string());
    }
    match id.rfind('_') {
        Some(i) => Some(id[..i].to_string()),
        None => Some("und".to_string()),
    }
}

/// Whether `id` is a CLDR locale (with data or default content).
pub fn is_known_locale(id: &str) -> bool {
    sup::LOCALES.binary_search(&id).is_ok() || is_default_content(id)
}

/// Whether `id` is a default-content locale (UTS #35: its data is its
/// parent's).
pub fn is_default_content(id: &str) -> bool {
    sup::DEFAULT_CONTENT.binary_search(&id).is_ok()
}
