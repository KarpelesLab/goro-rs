//! Time zones as ICU's TimeZone exposes them to PHP's IntlTimeZone (PHP
//! manual: IntlTimeZone; ICU API reference: TimeZone, BasicTimeZone).
//!
//! Zone data is the IANA database embedded in goro-ext-date. Zone ids and
//! their canonical forms are CLDR's (bcp47/timezone.xml: the first alias of
//! each zone is CLDR's canonical id, e.g. "Asia/Calcutta"), Windows ids
//! come from CLDR's windowsZones, regions from zone.tab. Offsets are split
//! into a raw (standard) part and a DST part as ICU reports them: the IANA
//! data gives the total offset and a DST flag, the standard offset of a DST
//! period is the neighbouring standard period's, and negative DST (Europe/
//! Dublin's winter in the main data form) is reported as ICU does, with the
//! summer period as the DST one (expected output of the ext/intl tests).

pub mod names;

use std::collections::HashMap;
use std::sync::OnceLock;

use goro_ext_date::tz::db::ZoneId;

use crate::datefmt::supplemental as sup;

/// ICU's custom zone id prefix.
const GMT: &str = "GMT";

/// The unknown zone's id (ICU TimeZone::getUnknown()).
pub const UNKNOWN_ID: &str = "Etc/Unknown";

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Zone(ZoneId),
    /// A fixed offset in milliseconds (custom "GMT+hh:mm" zones).
    Fixed(i32),
    Unknown,
}

/// A time zone: an id and its rules.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeZone {
    id: String,
    kind: Kind,
}

/// Wall time resolution options (IntlCalendar::WALLTIME_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallTime {
    /// The later of a repeated time; for a skipped time, the offset in
    /// effect before the transition (WALLTIME_LAST, 0).
    Last,
    /// The earlier of a repeated time; for a skipped time, the offset in
    /// effect after the transition (WALLTIME_FIRST, 1).
    First,
    /// A skipped time becomes the transition instant (WALLTIME_NEXT_VALID, 2).
    NextValid,
}

const DAY_MS: f64 = 86_400_000.0;

impl TimeZone {
    /// `TimeZone::createTimeZone`: a system zone or a custom "GMT±hh:mm"
    /// zone; anything else gives the unknown zone.
    pub fn create(id: &str) -> TimeZone {
        TimeZone::try_create(id).unwrap_or_else(TimeZone::unknown)
    }

    /// [`TimeZone::create`] that reports unknown ids as None.
    pub fn try_create(id: &str) -> Option<TimeZone> {
        if id == UNKNOWN_ID {
            return Some(TimeZone::unknown());
        }
        if let Some(z) = system_zone(id) {
            return Some(TimeZone {
                id: id.to_string(),
                kind: Kind::Zone(z),
            });
        }
        let ms = parse_custom_id(id)?;
        Some(TimeZone::fixed(ms))
    }

    /// A custom zone with a fixed offset ("GMT+05:30").
    pub fn fixed(offset_ms: i32) -> TimeZone {
        TimeZone {
            id: custom_id(offset_ms),
            kind: Kind::Fixed(offset_ms),
        }
    }

    /// `TimeZone::getGMT()`.
    pub fn gmt() -> TimeZone {
        TimeZone::try_create("GMT").unwrap_or(TimeZone {
            id: "GMT".into(),
            kind: Kind::Fixed(0),
        })
    }

    /// `TimeZone::getUnknown()`.
    pub fn unknown() -> TimeZone {
        TimeZone {
            id: UNKNOWN_ID.into(),
            kind: Kind::Unknown,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether this is the unknown zone.
    pub fn is_unknown(&self) -> bool {
        self.kind == Kind::Unknown
    }

    /// Whether this is a custom (fixed offset) zone.
    pub fn is_custom(&self) -> bool {
        matches!(self.kind, Kind::Fixed(_))
    }

    /// The CLDR canonical id of this zone's id (the id itself for custom
    /// zones).
    pub fn canonical_id(&self) -> String {
        match &self.kind {
            Kind::Zone(_) => canonical_id(&self.id)
                .map(|(c, _)| c)
                .unwrap_or_else(|| self.id.clone()),
            _ => self.id.clone(),
        }
    }

    /// (raw offset, DST offset) in milliseconds at UTC time `ms`.
    pub fn offsets(&self, ms: f64) -> (i32, i32) {
        match &self.kind {
            Kind::Fixed(o) => (*o, 0),
            Kind::Unknown => (0, 0),
            Kind::Zone(z) => {
                let s = (ms / 1000.0).floor();
                let ts = if s.is_finite() { s as i64 } else { 0 };
                let (raw, dst) = split(*z, ts);
                (raw * 1000, dst * 1000)
            }
        }
    }

    /// Raw + DST offset at `ms`.
    pub fn total_offset(&self, ms: f64) -> i32 {
        let (r, d) = self.offsets(ms);
        r + d
    }

    /// (raw, DST) offsets for local (wall) time `local_ms`, resolving
    /// skipped and repeated wall times with the options.
    pub fn offsets_local(
        &self,
        local_ms: f64,
        skipped: WallTime,
        repeated: WallTime,
    ) -> (i32, i32) {
        let z = match &self.kind {
            Kind::Fixed(o) => return (*o, 0),
            Kind::Unknown => return (0, 0),
            Kind::Zone(z) => *z,
        };
        let utc = self.local_to_utc(z, local_ms, skipped, repeated);
        self.offsets(utc)
    }

    /// The UTC time of local time `local_ms`.
    pub fn local_to_utc_ms(&self, local_ms: f64, skipped: WallTime, repeated: WallTime) -> f64 {
        match &self.kind {
            Kind::Fixed(o) => local_ms - *o as f64,
            Kind::Unknown => local_ms,
            Kind::Zone(z) => self.local_to_utc(*z, local_ms, skipped, repeated),
        }
    }

    fn local_to_utc(&self, _z: ZoneId, local: f64, skipped: WallTime, repeated: WallTime) -> f64 {
        // offsets well before and after the wall time
        let before = self.total_offset(local - DAY_MS) as f64;
        let after = self.total_offset(local + DAY_MS) as f64;
        let c1 = local - before;
        let c2 = local - after;
        let ok1 = self.total_offset(c1) as f64 == before;
        let ok2 = self.total_offset(c2) as f64 == after;
        match (ok1, ok2) {
            (true, true) if c1 != c2 => {
                // a repeated wall time: c1 (earlier offset, e.g. DST) is the
                // earlier instant when the offset decreases
                let (early, late) = if c1 < c2 { (c1, c2) } else { (c2, c1) };
                match repeated {
                    WallTime::First => early,
                    _ => late,
                }
            }
            (true, _) => c1,
            (false, true) => c2,
            (false, false) => {
                // a skipped wall time (the offset increases from `before`
                // to `after`)
                match skipped {
                    WallTime::Last => c1,
                    WallTime::First => c2,
                    WallTime::NextValid => {
                        // the transition instant lies between c2 and c1
                        let (mut lo, mut hi) = (c2.min(c1), c2.max(c1));
                        while hi - lo > 1.0 {
                            let mid = ((lo + hi) / 2.0).floor();
                            if self.total_offset(mid) as f64 == before {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        hi
                    }
                }
            }
        }
    }

    /// `getRawOffset()`: the raw offset now.
    pub fn raw_offset(&self) -> i32 {
        self.offsets(now_ms()).0
    }

    /// Whether DST is in effect at `ms`.
    pub fn in_daylight_time(&self, ms: f64) -> bool {
        self.offsets(ms).1 != 0
    }

    /// `useDaylightTime()`: whether the zone observes DST in the current
    /// year.
    pub fn use_daylight_time(&self) -> bool {
        self.dst_in_range(now_ms() - 365.0 * DAY_MS, now_ms() + 365.0 * DAY_MS)
            .is_some()
    }

    /// `getDSTSavings()`: the DST amount when the zone observes DST.
    pub fn dst_savings(&self) -> i32 {
        self.dst_in_range(now_ms() - 365.0 * DAY_MS, now_ms() + 2.0 * 365.0 * DAY_MS)
            .unwrap_or(0)
    }

    /// The DST amount of the first DST period in [from, to).
    fn dst_in_range(&self, from: f64, to: f64) -> Option<i32> {
        let Kind::Zone(z) = &self.kind else {
            return None;
        };
        let (b, e) = ((from / 1000.0) as i64, (to / 1000.0) as i64);
        let d = split(*z, b).1;
        if d != 0 {
            return Some(d * 1000);
        }
        for t in z.transitions(b, e) {
            let d = split(*z, t.ts).1;
            if d != 0 {
                return Some(d * 1000);
            }
        }
        None
    }

    /// `hasSameRules()`.
    pub fn has_same_rules(&self, other: &TimeZone) -> bool {
        match (&self.kind, &other.kind) {
            (Kind::Unknown, Kind::Unknown) => true,
            (Kind::Fixed(a), Kind::Fixed(b)) => a == b,
            (Kind::Zone(a), Kind::Zone(b)) => a == b || zone_signature(*a) == zone_signature(*b),
            (Kind::Fixed(a), Kind::Zone(z)) | (Kind::Zone(z), Kind::Fixed(a)) => {
                z.transitions(i64::MIN / 2, i64::MAX / 2).next().is_none()
                    && z.initial_type().offset * 1000 == *a
            }
            _ => false,
        }
    }

    /// ICU's `operator==`: same id and rules.
    pub fn equals(&self, other: &TimeZone) -> bool {
        self.id == other.id && self.has_same_rules(other)
    }

    /// The IANA zone, for DateTimeZone conversion.
    pub fn zone_id(&self) -> Option<ZoneId> {
        match &self.kind {
            Kind::Zone(z) => Some(*z),
            _ => None,
        }
    }

    /// The fixed offset of a custom zone.
    pub fn fixed_offset(&self) -> Option<i32> {
        match &self.kind {
            Kind::Fixed(o) => Some(*o),
            _ => None,
        }
    }
}

/// The current time in milliseconds.
pub fn now_ms() -> f64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_millis() as f64).floor()
}

/// (raw, dst) seconds at Unix time `ts`.
fn split(z: ZoneId, ts: i64) -> (i32, i32) {
    let ty = z.type_at(ts);
    // the neighbouring periods' types
    let neighbour = |want_dst: bool| -> Option<i32> {
        // the period start: the last transition <= ts
        let mut prev_start = None;
        for t in z.transitions(ts - 400 * 86_400, ts) {
            prev_start = Some(t.ts);
        }
        if let Some(t) = z.transitions(ts, ts + 400 * 86_400).next()
            && t.ty.is_dst == want_dst
        {
            return Some(t.ty.offset);
        }
        if let Some(s) = prev_start {
            let p = z.type_at(s - 1);
            if p.is_dst == want_dst {
                return Some(p.offset);
            }
        }
        None
    };
    if ty.is_dst {
        match neighbour(false) {
            Some(std) if std <= ty.offset => (std, ty.offset - std),
            Some(_) => (ty.offset, 0),
            None => (ty.offset - 3600, 3600),
        }
    } else {
        match neighbour(true) {
            Some(d) if d < ty.offset => (d, ty.offset - d),
            _ => (ty.offset, 0),
        }
    }
}

/// A fingerprint of a zone's rules for equivalence tests.
fn zone_signature(z: ZoneId) -> Vec<(i64, i32, bool)> {
    let begin = -5_364_662_400; // 1800
    let end = 4_102_444_800; // 2100
    let t0 = z.type_at(begin);
    let mut v = vec![(begin, t0.offset, t0.is_dst)];
    for t in z.transitions(begin, end) {
        v.push((t.ts, t.ty.offset, t.ty.is_dst));
    }
    v
}

// ---------------------------------------------------------------- ids

/// The three-letter ids of the Java TimeZone class that ICU keeps as
/// aliases (Java SE TimeZone API documentation, "three-letter time zone
/// IDs"), where the IANA database lacks them.
const LEGACY_IDS: &[(&str, &str)] = &[
    ("ACT", "Australia/Darwin"),
    ("AET", "Australia/Sydney"),
    ("AGT", "America/Argentina/Buenos_Aires"),
    ("ART", "Africa/Cairo"),
    ("AST", "America/Anchorage"),
    ("BET", "America/Sao_Paulo"),
    ("BST", "Asia/Dhaka"),
    ("CAT", "Africa/Harare"),
    ("CNT", "America/St_Johns"),
    ("CST", "America/Chicago"),
    ("CTT", "Asia/Shanghai"),
    ("EAT", "Africa/Addis_Ababa"),
    ("ECT", "Europe/Paris"),
    ("IET", "America/Indiana/Indianapolis"),
    ("IST", "Asia/Kolkata"),
    ("JST", "Asia/Tokyo"),
    ("MIT", "Pacific/Apia"),
    ("NET", "Asia/Yerevan"),
    ("NST", "Pacific/Auckland"),
    ("PLT", "Asia/Karachi"),
    ("PNT", "America/Phoenix"),
    ("PRT", "America/Puerto_Rico"),
    ("PST", "America/Los_Angeles"),
    ("SST", "Pacific/Guadalcanal"),
    ("VST", "Asia/Ho_Chi_Minh"),
];

fn legacy_target(id: &str) -> Option<&'static str> {
    LEGACY_IDS.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// The IANA zone of a system id (exact spelling).
fn system_zone(id: &str) -> Option<ZoneId> {
    if let Some(t) = legacy_target(id)
        && ZoneId::find(id).is_none_or(|z| z.name() != id)
    {
        return ZoneId::find(t);
    }
    if let Some(z) = ZoneId::find(id)
        && z.name() == id
    {
        return Some(z);
    }
    // CLDR aliases the IANA database dropped
    let canon = cldr_alias_map().get(id)?;
    let z = ZoneId::find(canon).filter(|z| z.name() == *canon);
    z.or_else(|| {
        let b = bcp47_for(canon)?;
        let iana = if b.2.is_empty() { canon } else { b.2 };
        ZoneId::find(iana)
    })
}

/// alias -> CLDR canonical id.
fn cldr_alias_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for (_, aliases, _, deprecated, _) in sup::BCP47_ZONES {
            if *deprecated || aliases.is_empty() {
                continue;
            }
            let mut it = aliases.split(' ');
            let canon = it.next().unwrap_or("");
            m.insert(canon, canon);
            for a in it {
                m.insert(a, canon);
            }
        }
        m
    })
}

/// The BCP 47 record of a CLDR canonical id.
fn bcp47_for(
    canon: &str,
) -> Option<&'static (&'static str, &'static str, &'static str, bool, &'static str)> {
    static M: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let m = M.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, (_, aliases, _, _, _)) in sup::BCP47_ZONES.iter().enumerate() {
            if let Some(c) = aliases.split(' ').next()
                && !c.is_empty()
            {
                m.insert(c, i);
            }
        }
        m
    });
    m.get(canon).map(|&i| &sup::BCP47_ZONES[i])
}

/// The BCP 47 short id of a zone ("ptlis"), for the `V` pattern.
pub fn short_id(id: &str) -> Option<&'static str> {
    let (c, _) = canonical_id(id)?;
    bcp47_for(&c).map(|b| b.0)
}

/// Every system id, sorted (TYPE_ANY).
pub fn all_ids() -> &'static [String] {
    static V: OnceLock<Vec<String>> = OnceLock::new();
    V.get_or_init(|| {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for z in ZoneId::all() {
            let n = z.name();
            if n == "posixrules" || n == "Factory" || n == "localtime" {
                continue;
            }
            set.insert(n.to_string());
        }
        for a in cldr_alias_map().keys() {
            if system_zone(a).is_some() {
                set.insert(a.to_string());
            }
        }
        for (a, _) in LEGACY_IDS {
            set.insert(a.to_string());
        }
        set.into_iter().collect()
    })
}

/// `getCanonicalID()`: (canonical id, is a system id). Custom ids give
/// their normalized form and false.
pub fn canonical_id(id: &str) -> Option<(String, bool)> {
    if let Some(t) = legacy_target(id)
        && ZoneId::find(id).is_none_or(|z| z.name() != id)
    {
        return canonical_id(t);
    }
    if let Some(c) = cldr_alias_map().get(id) {
        return Some((c.to_string(), true));
    }
    if id == UNKNOWN_ID {
        return Some((UNKNOWN_ID.to_string(), true));
    }
    if let Some(z) = system_zone(id) {
        // an IANA name CLDR does not list: follow the link to its zone
        if z.is_link() {
            for o in ZoneId::all() {
                if !o.is_link()
                    && o != z
                    && zone_signature(o) == zone_signature(z)
                    && let Some(c) = cldr_alias_map().get(o.name())
                {
                    return Some((c.to_string(), true));
                }
            }
        }
        return Some((z.name().to_string(), true));
    }
    let ms = parse_custom_id(id)?;
    Some((custom_id(ms), false))
}

/// `getIanaID()`: the IANA name of a system id.
pub fn iana_id(id: &str) -> Option<String> {
    let (c, sys) = canonical_id(id)?;
    if !sys {
        return None;
    }
    match bcp47_for(&c) {
        Some(b) if !b.2.is_empty() => Some(b.2.to_string()),
        _ => Some(c),
    }
}

/// The equivalence group of a system id (ids with the same rules, ICU's
/// links), sorted.
pub fn equivalent_ids(id: &str) -> Vec<String> {
    let Some(z) = system_zone(id) else {
        return Vec::new();
    };
    type Groups = HashMap<Vec<(i64, i32, bool)>, Vec<String>>;
    static G: OnceLock<Groups> = OnceLock::new();
    let g = G.get_or_init(|| {
        let mut g: Groups = HashMap::new();
        for n in all_ids() {
            if let Some(z) = system_zone(n) {
                g.entry(zone_signature(z)).or_default().push(n.clone());
            }
        }
        g
    });
    let mut v = g.get(&zone_signature(z)).cloned().unwrap_or_default();
    v.sort();
    v
}

/// `getRegion()`: the ISO country of a system id ("001" for zones with
/// none).
pub fn region(id: &str) -> Option<String> {
    let (c, sys) = canonical_id(id)?;
    if !sys {
        return None;
    }
    Some(canonical_region(&c).to_string())
}

fn canonical_region(canon: &str) -> &'static str {
    static M: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let m = M.get_or_init(|| {
        let mut m = HashMap::new();
        for z in ZoneId::all() {
            if let Some(loc) = z.location() {
                m.insert(z.name().to_string(), loc.country_code);
            }
        }
        m
    });
    if let Some(r) = m.get(canon) {
        return r;
    }
    // the IANA name of the CLDR id, or its BCP 47 code's region
    if let Some(b) = bcp47_for(canon) {
        for a in b.1.split(' ').chain(std::iter::once(b.2)) {
            if let Some(r) = m.get(a) {
                return r;
            }
        }
        if b.0.len() == 5 && !canon.starts_with("Etc/") {
            let r = b.0[..2].to_ascii_uppercase();
            return Box::leak(r.into_boxed_str());
        }
    }
    "001"
}

/// The canonical ids (TYPE_CANONICAL), sorted.
pub fn canonical_ids() -> Vec<String> {
    let mut v: Vec<String> = all_ids()
        .iter()
        .filter(|n| cldr_alias_map().get(n.as_str()) == Some(&n.as_str()))
        .cloned()
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Zone ids of an enumeration: `kind` 0 any, 1 canonical, 2 canonical
/// location (IntlTimeZone::TYPE_*), filtered by region and raw offset.
pub fn enumerate(kind: i64, region_f: Option<&str>, raw_offset: Option<i32>) -> Vec<String> {
    let base: Vec<String> = match kind {
        0 => all_ids().to_vec(),
        _ => canonical_ids(),
    };
    let now = now_ms();
    base.into_iter()
        .filter(|id| {
            if kind == 2 && region(id).as_deref() == Some("001") {
                return false;
            }
            if let Some(r) = region_f
                && region(id).as_deref() != Some(r)
            {
                return false;
            }
            if let Some(o) = raw_offset {
                let tz = TimeZone::create(id);
                if tz.offsets(now).0 != o {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// `getWindowsID()`: the Windows zone of a system id ("" when CLDR maps
/// none).
pub fn windows_id(id: &str) -> Option<String> {
    let (c, sys) = canonical_id(id)?;
    if !sys {
        return None;
    }
    for (win, _, zones) in sup::WINDOWS_ZONES {
        if zones.split(' ').any(|z| z == c) {
            return Some(win.to_string());
        }
    }
    Some(String::new())
}

/// `getIDForWindowsID()`: the zone of a Windows id for a region (the
/// "001" mapping when the region has none). None for unknown Windows ids.
pub fn id_for_windows_id(win: &str, region: Option<&str>) -> Option<String> {
    let mut known = false;
    let mut world = None;
    for (w, t, zones) in sup::WINDOWS_ZONES {
        if *w != win {
            continue;
        }
        known = true;
        let first = zones.split(' ').next().unwrap_or("");
        if Some(*t) == region {
            return Some(first.to_string());
        }
        if *t == "001" {
            world = Some(first.to_string());
        }
    }
    if !known {
        return None;
    }
    world
}

/// The tz database release ("2026e").
pub fn tz_data_version() -> &'static str {
    goro_ext_date::tz::db::release()
}

// ---------------------------------------------------------------- custom ids

/// Parses a custom zone id "GMT[+-]hh[[:]mm[[:]ss]]" (ICU TimeZone API
/// reference: custom time zone ids) into an offset in milliseconds.
pub fn parse_custom_id(id: &str) -> Option<i32> {
    let b = id.as_bytes();
    if b.len() < 4 || !b[..3].eq_ignore_ascii_case(GMT.as_bytes()) {
        return None;
    }
    let sign = match b[3] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &id[4..];
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    let (h, m, s) = if rest.contains(':') {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() > 3 || !parts.iter().all(|p| digits(p)) {
            return None;
        }
        if parts[0].len() > 2 || parts.iter().skip(1).any(|p| p.len() != 2) {
            return None;
        }
        let n = |i: usize| parts.get(i).map_or(Some(0), |p| p.parse::<i32>().ok());
        (n(0)?, n(1)?, n(2)?)
    } else {
        if !digits(rest) {
            return None;
        }
        match rest.len() {
            1 | 2 => (rest.parse().ok()?, 0, 0),
            3 => (rest[..1].parse().ok()?, rest[1..].parse().ok()?, 0),
            4 => (rest[..2].parse().ok()?, rest[2..].parse().ok()?, 0),
            5 => (
                rest[..1].parse().ok()?,
                rest[1..3].parse().ok()?,
                rest[3..].parse().ok()?,
            ),
            6 => (
                rest[..2].parse().ok()?,
                rest[2..4].parse().ok()?,
                rest[4..].parse().ok()?,
            ),
            _ => return None,
        }
    };
    if h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some(sign * ((h * 3600 + m * 60 + s) * 1000))
}

/// The normalized custom id of an offset ("GMT+05:30", "GMT-01:00:30").
pub fn custom_id(offset_ms: i32) -> String {
    let sign = if offset_ms < 0 { '-' } else { '+' };
    let t = (offset_ms.unsigned_abs() / 1000) as i32;
    let (h, m, s) = (t / 3600, t / 60 % 60, t % 60);
    if s != 0 {
        format!("GMT{sign}{h:02}:{m:02}:{s:02}")
    } else {
        format!("GMT{sign}{h:02}:{m:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_ids() {
        assert_eq!(parse_custom_id("GMT+01:00"), Some(3_600_000));
        assert_eq!(parse_custom_id("GMT+0405"), Some(14_700_000));
        assert_eq!(parse_custom_id("GMT-2"), Some(-7_200_000));
        assert_eq!(parse_custom_id("GMT +01:25"), None);
        assert_eq!(custom_id(14_700_000), "GMT+04:05");
        assert_eq!(TimeZone::create("GMT+00:01").id(), "GMT+00:01");
    }

    #[test]
    fn canonical() {
        assert_eq!(
            canonical_id("Portugal"),
            Some(("Europe/Lisbon".into(), true))
        );
        assert_eq!(canonical_id("Asia/Kolkata").unwrap().0, "Asia/Calcutta");
        assert_eq!(iana_id("Asia/Calcutta").as_deref(), Some("Asia/Kolkata"));
        assert_eq!(region("Europe/Amsterdam").as_deref(), Some("NL"));
        assert_eq!(region("Portugal").as_deref(), Some("PT"));
        assert_eq!(equivalent_ids("Europe/Lisbon")[1], "Portugal");
    }

    #[test]
    fn offsets() {
        let lsb = TimeZone::create("Europe/Lisbon");
        // 1 July 2012
        assert_eq!(lsb.offsets(1_341_100_800_000.0), (0, 3_600_000));
        let ams = TimeZone::create("Europe/Amsterdam");
        assert_eq!(ams.offsets(1_341_100_800_000.0), (3_600_000, 3_600_000));
        assert_eq!(ams.raw_offset(), 3_600_000);
        assert!(lsb.use_daylight_time());
        assert!(!TimeZone::gmt().use_daylight_time());
        assert_eq!(lsb.dst_savings(), 3_600_000);
        let dub = TimeZone::create("Europe/Dublin");
        assert_eq!(dub.offsets(1_341_100_800_000.0), (0, 3_600_000));
        assert_eq!(dub.offsets(1_325_376_000_000.0), (0, 0));
    }

    #[test]
    fn windows() {
        assert_eq!(
            windows_id("America/Havana").as_deref(),
            Some("Cuba Standard Time")
        );
        assert_eq!(
            id_for_windows_id("Pacific Standard Time", Some("ZZ")).as_deref(),
            Some("America/Los_Angeles")
        );
        assert_eq!(
            id_for_windows_id("India Standard Time", None).as_deref(),
            Some("Asia/Calcutta")
        );
    }

    #[test]
    fn enumerations() {
        let v = enumerate(0, Some("PT"), Some(-3_600_000));
        assert_eq!(v, vec!["Atlantic/Azores".to_string()]);
    }
}
