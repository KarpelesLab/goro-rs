//! The embedded IANA time zone database.
//!
//! `data/tzdata.bin` is compiled from the official tzdata release by
//! `tools/gen_tzdata.py` (layout documented there). Everything here reads
//! the blob in place: lookups never allocate.

use crate::civil;

static BLOB: &[u8] = include_bytes!("../../data/tzdata.bin");

#[inline]
fn u16_at(off: usize) -> u16 {
    u16::from_le_bytes([BLOB[off], BLOB[off + 1]])
}

#[inline]
fn u32_at(off: usize) -> u32 {
    u32::from_le_bytes(BLOB[off..off + 4].try_into().unwrap())
}

#[inline]
fn i32_at(off: usize) -> i32 {
    i32::from_le_bytes(BLOB[off..off + 4].try_into().unwrap())
}

#[inline]
fn i64_at(off: usize) -> i64 {
    i64::from_le_bytes(BLOB[off..off + 8].try_into().unwrap())
}

struct Header {
    version_off: usize,
    version_len: usize,
    n_entries: usize,
    entries_off: usize,
    zones_off: usize,
    strings_off: usize,
}

#[inline]
fn header() -> Header {
    Header {
        version_off: u32_at(4) as usize,
        version_len: u32_at(8) as usize,
        n_entries: u32_at(12) as usize,
        entries_off: u32_at(16) as usize,
        zones_off: u32_at(24) as usize,
        strings_off: u32_at(28) as usize,
    }
}

fn str_at(off: usize, len: usize) -> &'static str {
    // The generator only writes UTF-8 (ASCII) strings.
    std::str::from_utf8(&BLOB[off..off + len]).unwrap_or("")
}

/// The tzdata release the table was built from, e.g. `"2026e"`.
pub fn release() -> &'static str {
    let h = header();
    str_at(h.version_off, h.version_len)
}

/// The version as `timezone_version_get()` reports it: `"2026.5"` for
/// release 2026e (PHP manual: timezone_version_get, which returns the
/// year and the release letter as a number).
pub fn php_version() -> String {
    let r = release();
    let (year, letter) = r.split_at(r.len().saturating_sub(1));
    match letter.as_bytes().first() {
        Some(c @ b'a'..=b'z') => format!("{}.{}", year, c - b'a' + 1),
        _ => r.to_string(),
    }
}

/// A name in the database: a zone or a backward-compatibility link.
/// Cheap to copy; identifies both the name and its zone data.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZoneId(u16);

impl std::fmt::Debug for ZoneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ZoneId({})", self.name())
    }
}

/// One local time type of a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalType {
    /// Offset from UTC in seconds (east positive).
    pub offset: i32,
    /// Whether this is daylight saving time.
    pub is_dst: bool,
    /// The abbreviation ("CEST", "+0530", "LMT").
    pub abbr: &'static str,
}

/// A change of local time type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// The instant of the change (Unix time).
    pub ts: i64,
    /// The type in effect from `ts` on.
    pub ty: LocalType,
}

/// Location data from zone.tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// ISO 3166 country code.
    pub country_code: &'static str,
    /// Latitude in degrees.
    pub latitude: f64,
    /// Longitude in degrees.
    pub longitude: f64,
    /// zone.tab's comment column (may be empty).
    pub comments: &'static str,
}

const FLAG_ZONE_TAB: u16 = 1;
const FLAG_LINK: u16 = 2;

struct ZoneRec {
    trans_off: usize,
    n_trans: usize,
    idx_off: usize,
    types_off: usize,
    tail_off: usize,
}

impl ZoneId {
    /// Every name, in the database order (ASCII case-insensitive).
    pub fn all() -> impl Iterator<Item = ZoneId> {
        (0..header().n_entries as u16).map(ZoneId)
    }

    /// Looks a name up, ignoring ASCII case.
    pub fn find(name: &str) -> Option<ZoneId> {
        let h = header();
        let needle = name.as_bytes();
        let (mut lo, mut hi) = (0usize, h.n_entries);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let cand = ZoneId(mid as u16).name().as_bytes();
            match cmp_ci(cand, needle) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(ZoneId(mid as u16)),
            }
        }
        None
    }

    #[inline]
    fn entry_off(self) -> usize {
        header().entries_off + 16 * self.0 as usize
    }

    /// The name as the database spells it ("Europe/Paris").
    pub fn name(self) -> &'static str {
        let h = header();
        let e = self.entry_off();
        let off = u32_at(e) as usize;
        let len = u16_at(e + 4) as usize;
        str_at(h.strings_off + off, len)
    }

    fn flags(self) -> u16 {
        u16_at(self.entry_off() + 8)
    }

    /// Whether zone.tab lists this name (the names
    /// `DateTimeZone::listIdentifiers()` returns, besides "UTC").
    pub fn in_zone_tab(self) -> bool {
        self.flags() & FLAG_ZONE_TAB != 0
    }

    /// Whether this is a backward-compatibility link.
    pub fn is_link(self) -> bool {
        self.flags() & FLAG_LINK != 0
    }

    fn rec(self) -> ZoneRec {
        let h = header();
        let z = u16_at(self.entry_off() + 6) as usize;
        let o = h.zones_off + 32 * z;
        ZoneRec {
            trans_off: u32_at(o) as usize,
            n_trans: u32_at(o + 4) as usize,
            idx_off: u32_at(o + 8) as usize,
            types_off: u32_at(o + 12) as usize,
            tail_off: u32_at(o + 20) as usize,
        }
    }

    /// zone.tab's location of this name, if listed.
    pub fn location(self) -> Option<Location> {
        let off = u32_at(self.entry_off() + 12) as usize;
        if off == 0 {
            return None;
        }
        let h = header();
        let clen = u16_at(off + 2) as usize;
        let lat = i32_at(off + 4);
        let lon = i32_at(off + 8);
        let coff = u32_at(off + 12) as usize;
        Some(Location {
            country_code: str_at(off, 2),
            latitude: lat as f64 / 100_000.0,
            longitude: lon as f64 / 100_000.0,
            comments: str_at(h.strings_off + coff, clen),
        })
    }

    /// The local time type in effect at Unix time `ts`.
    pub fn type_at(self, ts: i64) -> LocalType {
        let r = self.rec();
        // last transition <= ts
        let (mut lo, mut hi) = (0usize, r.n_trans);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if i64_at(r.trans_off + 8 * mid) <= ts {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == r.n_trans
            && r.tail_off != 0
            && let Some(t) = tail_type_at(&r, ts)
        {
            return t;
        }
        if lo == 0 {
            return type_n(&r, 0);
        }
        type_n(&r, BLOB[r.idx_off + lo - 1] as usize)
    }

    /// The first local time type (in effect before any transition).
    pub fn initial_type(self) -> LocalType {
        type_n(&self.rec(), 0)
    }

    /// Transitions with `begin < ts < end`, in order: the explicit ones,
    /// then those generated from the zone's ongoing rules.
    pub fn transitions(self, begin: i64, end: i64) -> TransitionIter {
        let r = self.rec();
        // first explicit transition > begin
        let (mut lo, mut hi) = (0usize, r.n_trans);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if i64_at(r.trans_off + 8 * mid) <= begin {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let (tail_year, n_rules) = if r.tail_off != 0 {
            (i32_at(r.tail_off) as i64, u32_at(r.tail_off + 4) as usize)
        } else {
            (0, 0)
        };
        let mut year = tail_year;
        if n_rules > 0 && begin > i64::MIN {
            // skip whole years before begin
            let (y, _, _) = civil::civil_from_days(begin.div_euclid(86_400));
            year = year.max(y - 1);
        }
        TransitionIter {
            rec: r,
            next: lo,
            begin,
            end,
            year,
            n_rules,
            pending: [None, None, None, None],
            done: false,
        }
    }
}

fn cmp_ci(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let n = a.len().min(b.len());
    for i in 0..n {
        let (x, y) = (a[i].to_ascii_lowercase(), b[i].to_ascii_lowercase());
        if x != y {
            return x.cmp(&y);
        }
    }
    a.len().cmp(&b.len())
}

fn type_n(r: &ZoneRec, n: usize) -> LocalType {
    let o = r.types_off + 8 * n;
    let h = header();
    LocalType {
        offset: i32_at(o),
        is_dst: BLOB[o + 4] != 0,
        abbr: str_at(h.strings_off + u16_at(o + 6) as usize, BLOB[o + 5] as usize),
    }
}

/// One generated transition of a tail rule in `year`: (ts, type index).
fn tail_rule_ts(r: &ZoneRec, rule: usize, year: i64) -> (i64, usize) {
    let o = r.tail_off + 8 + 12 * rule;
    let month = BLOB[o] as u32;
    let kind = BLOB[o + 1];
    let dow = BLOB[o + 2] as u32;
    let day = BLOB[o + 3] as u32;
    let secs = i32_at(o + 4) as i64;
    let ty = BLOB[o + 8] as usize;
    let days = match kind {
        0 => civil::days_from_civil(year, month as i64, day as i64),
        1 => {
            let last = civil::days_from_civil(
                year,
                month as i64,
                civil::days_in_month(year, month) as i64,
            );
            last - ((civil::weekday(last) + 7 - dow) % 7) as i64
        }
        2 => {
            let d = civil::days_from_civil(year, month as i64, day as i64);
            d + ((dow + 7 - civil::weekday(d)) % 7) as i64
        }
        _ => {
            let d = civil::days_from_civil(year, month as i64, day as i64);
            d - ((civil::weekday(d) + 7 - dow) % 7) as i64
        }
    };
    (days.saturating_mul(86_400).saturating_add(secs), ty)
}

fn tail_type_at(r: &ZoneRec, ts: i64) -> Option<LocalType> {
    let first_year = i32_at(r.tail_off) as i64;
    let n = u32_at(r.tail_off + 4) as usize;
    if n == 0 {
        return None;
    }
    let (y, _, _) = civil::civil_from_days(ts.div_euclid(86_400));
    if y > 292_277_026_596 || y < first_year {
        // beyond the representable range: keep the last explicit type
        return None;
    }
    // the latest rule occurrence <= ts, looking at this year and the last
    let mut best: Option<(i64, usize)> = None;
    for year in [y - 1, y] {
        if year < first_year {
            continue;
        }
        for rule in 0..n {
            let (t, ty) = tail_rule_ts(r, rule, year);
            if t <= ts && best.is_none_or(|(bt, _)| t > bt) {
                best = Some((t, ty));
            }
        }
    }
    best.map(|(_, ty)| type_n(r, ty))
}

/// Iterator over a zone's transitions (see [`ZoneId::transitions`]).
pub struct TransitionIter {
    rec: ZoneRec,
    next: usize,
    begin: i64,
    end: i64,
    year: i64,
    n_rules: usize,
    pending: [Option<(i64, usize)>; 4],
    done: bool,
}

impl Iterator for TransitionIter {
    type Item = Transition;

    fn next(&mut self) -> Option<Transition> {
        if self.done {
            return None;
        }
        if self.next < self.rec.n_trans {
            let ts = i64_at(self.rec.trans_off + 8 * self.next);
            if ts >= self.end {
                self.done = true;
                return None;
            }
            let ty = BLOB[self.rec.idx_off + self.next] as usize;
            self.next += 1;
            return Some(Transition {
                ts,
                ty: type_n(&self.rec, ty),
            });
        }
        if self.n_rules == 0 {
            self.done = true;
            return None;
        }
        loop {
            // take the earliest pending occurrence
            let mut best: Option<usize> = None;
            for (i, p) in self.pending.iter().enumerate() {
                if let Some((t, _)) = p
                    && best.is_none_or(|b| self.pending[b].unwrap().0 > *t)
                {
                    best = Some(i);
                }
            }
            if let Some(b) = best {
                let (t, ty) = self.pending[b].take().unwrap();
                if t >= self.end {
                    self.done = true;
                    return None;
                }
                if t <= self.begin {
                    continue;
                }
                return Some(Transition {
                    ts: t,
                    ty: type_n(&self.rec, ty),
                });
            }
            if self.year > 292_277_026_596 {
                self.done = true;
                return None;
            }
            for rule in 0..self.n_rules.min(4) {
                self.pending[rule] = Some(tail_rule_ts(&self.rec, rule, self.year));
            }
            self.year += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_names() {
        let z = ZoneId::find("europe/paris").unwrap();
        assert_eq!(z.name(), "Europe/Paris");
        assert!(z.in_zone_tab());
        assert!(ZoneId::find("US/Eastern").unwrap().is_link());
        assert!(ZoneId::find("Mars/Olympus").is_none());
        assert_eq!(release(), "2026e");
        assert_eq!(php_version(), "2026.5");
        // every name finds itself
        for z in ZoneId::all() {
            assert_eq!(ZoneId::find(z.name()), Some(z));
        }
    }

    #[test]
    fn offsets() {
        let ny = ZoneId::find("America/New_York").unwrap();
        let t = ny.type_at(1_625_140_800); // 2021-07-01 12:00 UTC
        assert_eq!((t.offset, t.is_dst, t.abbr), (-14400, true, "EDT"));
        let t = ny.type_at(1_609_502_400);
        assert_eq!((t.offset, t.is_dst, t.abbr), (-18000, false, "EST"));
        // far future from the tail rules
        let t = ny.type_at(4_000_000_000); // 2096-10-02
        assert_eq!(t.abbr, "EDT");
        let t = ny.type_at(i64::MIN);
        assert_eq!((t.offset, t.abbr), (-17762, "LMT"));
        let paris = ZoneId::find("Europe/Paris").unwrap();
        let v: Vec<_> = paris.transitions(4_000_000_000, 4_100_000_000).collect();
        assert_eq!(v.len(), 7);
        assert_eq!(v[0].ts, 4_002_224_400);
        assert_eq!(v[0].ty.abbr, "CET");
    }

    #[test]
    fn location() {
        let l = ZoneId::find("America/New_York")
            .unwrap()
            .location()
            .unwrap();
        assert_eq!(l.country_code, "US");
        assert_eq!(l.latitude, 40.71416);
        assert_eq!(l.longitude, -74.00638);
        assert_eq!(l.comments, "Eastern (most areas)");
    }
}
