//! ICU's TimeZone service: the system zones (OlsonTimeZone, over the
//! timezonedb of ext/date), custom "GMT+hh:mm" zones (SimpleTimeZone),
//! zone metadata (canonical IDs, regions, Windows IDs, equivalence groups:
//! data.rs, extracted from ICU) and the raw / DST offset split ICU makes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_ext_date::timelib::tz::{TzInfo, parse_tzfile};

use super::data;

/// One period of a zone's history: from `start` (UTC seconds; i64::MIN for
/// the initial period) the zone has `raw` standard and `dst` daylight
/// offsets (ms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub start: i64,
    pub raw: i32,
    pub dst: i32,
}

/// The rules of a system zone.
#[derive(Debug, PartialEq, Eq)]
pub struct Rules {
    pub periods: Vec<Period>,
    /// The DST savings of the final (ongoing) rule, when the zone still
    /// observes DST.
    pub final_save: Option<i32>,
}

/// A time zone: a system zone with its rules, or a fixed-offset one (GMT,
/// custom IDs, Etc/Unknown).
#[derive(Debug)]
pub struct Zone {
    pub id: String,
    pub rules: Option<Rc<Rules>>,
    /// The raw offset (ms) of a fixed zone.
    pub fixed: i32,
}

pub const UNKNOWN_ID: &str = "Etc/Unknown";

/// The last year the transitions of an ongoing DST rule are generated for.
const MAX_RULE_YEAR: i32 = 3000;

/// The largest absolute UDate ICU's TimeZone::getOffset() accepts (days
/// must fit an int32).
const MAX_DAYS: f64 = 2_147_483_647.0;

impl Zone {
    pub fn fixed(id: &str, raw: i32) -> Zone {
        Zone {
            id: id.to_string(),
            rules: None,
            fixed: raw,
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.id == UNKNOWN_ID && self.rules.is_none()
    }

    fn period_at(r: &Rules, sec: i64) -> usize {
        // last period with start <= sec
        match r.periods.binary_search_by(|p| p.start.cmp(&sec)) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }

    /// TimeZone::getOffset(date, local = false)
    pub fn offsets(&self, ms: f64) -> Result<(i32, i32), ()> {
        if !ms.is_finite() || (ms / 86_400_000.0).abs() > MAX_DAYS {
            return Err(());
        }
        Ok(self.offsets_utc(ms))
    }

    /// The offsets at UTC instant `ms` (assumed in range).
    pub fn offsets_utc(&self, ms: f64) -> (i32, i32) {
        match &self.rules {
            None => (self.fixed, 0),
            Some(r) => {
                let sec = (ms / 1000.0).floor() as i64;
                let p = r.periods[Self::period_at(r, sec)];
                (p.raw, p.dst)
            }
        }
    }

    /// OlsonTimeZone::getHistoricalOffset() with local = true: the offsets
    /// of local wall time `wall` (ms). `nonexist_latter`: a skipped wall
    /// time is interpreted with the rule after the transition (kLatter);
    /// `dup_former`: a repeated wall time takes the earlier occurrence.
    pub fn offsets_local(&self, wall: f64, nonexist_latter: bool, dup_former: bool) -> (i32, i32) {
        let Some(r) = &self.rules else {
            return (self.fixed, 0);
        };
        if !wall.is_finite() {
            return (r.periods[0].raw, r.periods[0].dst);
        }
        let sec = (wall / 1000.0).floor() as i64;
        let n = r.periods.len();
        let mut idx: isize = n as isize - 1;
        while idx >= 1 {
            let i = idx as usize;
            let mut transition = r.periods[i].start;
            let before = r.periods[i - 1];
            let after = r.periods[i];
            let ob = ((before.raw + before.dst) / 1000) as i64;
            let oa = ((after.raw + after.dst) / 1000) as i64;
            if sec >= transition.saturating_sub(86_400) {
                if oa - ob >= 0 {
                    // positive transition: a non-existing local range
                    transition += if nonexist_latter { ob } else { oa };
                } else {
                    // negative: a duplicated range
                    transition += if dup_former { ob } else { oa };
                }
            }
            if sec >= transition {
                break;
            }
            idx -= 1;
        }
        let p = r.periods[idx.max(0) as usize];
        (p.raw, p.dst)
    }

    /// TimeZone::getOffset(date, local = true): former for skipped, latter
    /// for repeated wall times.
    pub fn offsets_wall(&self, wall: f64) -> (i32, i32) {
        self.offsets_local(wall, false, false)
    }

    /// getRawOffset(): the raw offset now.
    pub fn raw_offset(&self) -> i32 {
        self.offsets_utc(now_millis()).0
    }

    /// useDaylightTime(): whether DST is observed during the current year.
    pub fn use_daylight_time(&self) -> bool {
        let Some(r) = &self.rules else {
            return false;
        };
        let now = now_millis();
        let days = (now / 86_400_000.0).floor() as i64;
        let (y, _, _) = super::civil_from_days(days);
        let start = super::days_from_civil(y, 1, 1) * 86_400;
        let limit = super::days_from_civil(y + 1, 1, 1) * 86_400;
        let first = Self::period_at(r, start);
        r.periods[first..]
            .iter()
            .take_while(|p| p.start < limit)
            .any(|p| p.dst != 0)
    }

    /// getDSTSavings()
    pub fn dst_savings(&self) -> i32 {
        if let Some(r) = &self.rules {
            if let Some(s) = r.final_save {
                return s;
            }
        }
        if self.use_daylight_time() {
            3_600_000
        } else {
            0
        }
    }

    /// hasSameRules()
    pub fn has_same_rules(&self, other: &Zone) -> bool {
        match (&self.rules, &other.rules) {
            (None, None) => self.fixed == other.fixed,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b) || **a == **b,
            _ => false,
        }
    }

    /// operator==: same class, same ID, same rules.
    pub fn equals(&self, other: &Zone) -> bool {
        self.id == other.id && self.has_same_rules(other)
    }

    /// BasicTimeZone::getPreviousTransition(base, inclusive): the start
    /// (ms) of the latest transition at or before `base`.
    pub fn prev_transition(&self, base: f64, inclusive: bool) -> Option<f64> {
        let r = self.rules.as_ref()?;
        // the number of periods (after the initial one) starting at or
        // before `base`
        let n = r.periods[1..].partition_point(|p| {
            let t = p.start as f64 * 1000.0;
            t < base || (inclusive && t == base)
        });
        if n == 0 {
            None
        } else {
            Some(r.periods[n].start as f64 * 1000.0)
        }
    }

    /// The transitions (start ms, previous offsets, new offsets) in
    /// [from, to).
    pub fn transitions_in(&self, from: f64, to: f64) -> Vec<(f64, Period, Period)> {
        let mut v = Vec::new();
        if let Some(r) = &self.rules {
            for w in r.periods.windows(2) {
                let t = w[1].start as f64 * 1000.0;
                if t >= from && t < to {
                    v.push((t, w[0], w[1]));
                }
            }
        }
        v
    }
}

/// Calendar::getNow()
pub fn now_millis() -> f64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as f64,
        Err(e) => -(e.duration().as_millis() as f64),
    }
}

// ---------------------------------------------------------------------------
// Rules from the timezonedb

/// The rules of a zone of the IANA database (the default build, links
/// instead of backzone data, as ICU's zoneinfo64 is built).
fn rules_from_tzdata(z: &timezone_data::Zone) -> Rules {
    let types = z.types();
    let mut raw: Vec<(i64, i32, bool)> = Vec::new();
    let t0 = types
        .first()
        .filter(|t| !t.is_dst)
        .or_else(|| types.iter().find(|t| !t.is_dst))
        .or(types.first());
    match t0 {
        Some(t) => raw.push((i64::MIN, t.offset, t.is_dst)),
        None => raw.push((i64::MIN, 0, false)),
    }
    for t in z.transitions() {
        let ty = types[t.type_idx];
        raw.push((t.when, ty.offset, ty.is_dst));
    }
    let mut final_save = None;
    if let Some(px) = z.extend() {
        if px.has_dst() {
            final_save = Some((px.dst_offset - px.std_offset).abs() * 1000);
            let last = raw.last().map(|r| r.0).unwrap_or(i64::MIN);
            let y0 = if last == i64::MIN {
                1970
            } else {
                super::civil_from_days(last.div_euclid(86_400)).0 as i32
            };
            for y in y0..=MAX_RULE_YEAR {
                if let Some((a, b)) = px.transitions_for_year(y) {
                    let mut v = [(a, px.dst_offset, true), (b, px.std_offset, false)];
                    if b < a {
                        v.swap(0, 1);
                    }
                    for e in v {
                        if e.0 > raw.last().map(|r| r.0).unwrap_or(i64::MIN) {
                            raw.push(e);
                        }
                    }
                }
            }
        } else if raw.len() == 1 {
            raw[0] = (i64::MIN, px.std_offset, false);
        }
    }
    rules_from_list(raw, final_save)
}

fn rules_from_tzinfo(tz: &TzInfo) -> Rules {
    // (start, total offset s, isdst)
    let mut raw: Vec<(i64, i32, bool)> = Vec::new();
    let t0 = tz.types.first().copied();
    match t0 {
        Some(t) => raw.push((i64::MIN, t.offset, t.isdst != 0)),
        None => raw.push((i64::MIN, 0, false)),
    }
    for k in 0..tz.timecnt.min(tz.trans.len()) {
        let ty = tz.types[tz.trans_idx[k] as usize];
        raw.push((tz.trans[k], ty.offset, ty.isdst != 0));
    }
    let mut final_save = None;
    if let Some(pi) = &tz.posix_info {
        if pi.dst_end.is_some() && pi.dst_begin.is_some() {
            final_save = Some(((pi.dst_offset - pi.std_offset).abs() * 1000) as i32);
            let last = raw.last().map(|r| r.0).unwrap_or(i64::MIN);
            let y0 = if last == i64::MIN {
                1970
            } else {
                super::civil_from_days(last.div_euclid(86_400)).0
            };
            let mut out = Vec::new();
            for y in y0..=MAX_RULE_YEAR as i64 {
                out.clear();
                tz.get_transitions_for_year(y, &mut out);
                for &(t, ti) in &out {
                    if t > raw.last().map(|r| r.0).unwrap_or(i64::MIN) {
                        let ty = tz.types[ti];
                        raw.push((t, ty.offset, ty.isdst != 0));
                    }
                }
            }
        } else if raw.len() == 1 {
            let ty = tz.types[pi.type_index_std_type];
            raw[0] = (i64::MIN, ty.offset, false);
        }
    }
    rules_from_list(raw, final_save)
}

/// Periods from (start, total offset s, is DST) entries.
fn rules_from_list(raw: Vec<(i64, i32, bool)>, final_save: Option<i32>) -> Rules {
    // drop no-op transitions
    let mut ded: Vec<(i64, i32, bool)> = Vec::with_capacity(raw.len());
    for r in raw {
        match ded.last() {
            Some(l) if l.1 == r.1 && l.2 == r.2 => {}
            _ => ded.push(r),
        }
    }
    // raw / dst split: a DST period's raw offset is the offset of a
    // neighbouring standard period, the one giving a positive (preferably
    // one hour) saving.
    let n = ded.len();
    let mut periods: Vec<Period> = Vec::with_capacity(n);
    let mut negative = vec![false; n];
    let mut prev_std: Option<i32> = None;
    for i in 0..n {
        let (start, off, isdst) = ded[i];
        let offms = off * 1000;
        if !isdst {
            prev_std = Some(offms);
            periods.push(Period {
                start,
                raw: offms,
                dst: 0,
            });
            continue;
        }
        let next_std = ded[i + 1..].iter().find(|d| !d.2).map(|d| d.1 * 1000);
        let sp = prev_std.map(|r| offms - r);
        let sn = next_std.map(|r| offms - r);
        let raw_off = if sp == Some(3_600_000) {
            prev_std
        } else if sn == Some(3_600_000) {
            next_std
        } else if sp.is_some_and(|v| v > 0) {
            prev_std
        } else if sn.is_some_and(|v| v > 0) {
            next_std
        } else {
            None
        };
        match raw_off {
            Some(r) => periods.push(Period {
                start,
                raw: r,
                dst: offms - r,
            }),
            None => {
                // Negative DST (Europe/Dublin, Africa/Windhoek, ... in the
                // main data): ICU's data has the opposite view, this period
                // being standard time.
                let neg = sp.is_some_and(|v| v < 0) || sn.is_some_and(|v| v < 0);
                negative[i] = neg;
                periods.push(Period {
                    start,
                    raw: offms,
                    dst: 0,
                });
            }
        }
    }
    // ... and the standard periods between two of them daylight time
    for j in 0..n {
        if ded[j].2 {
            continue;
        }
        let before = (0..j).rev().find(|&k| ded[k].2);
        let after = (j + 1..n).find(|&k| ded[k].2);
        if let (Some(x), Some(y)) = (before, after) {
            if negative[x] && negative[y] {
                let base = periods[x].raw.max(periods[y].raw);
                let t = ded[j].1 * 1000;
                if t > base {
                    periods[j].raw = base;
                    periods[j].dst = t - base;
                }
            }
        }
    }
    let mut out: Vec<Period> = Vec::with_capacity(n);
    for p in periods {
        match out.last() {
            Some(l) if l.raw == p.raw && l.dst == p.dst => {}
            _ => out.push(p),
        }
    }
    Rules {
        periods: out,
        final_save,
    }
}

thread_local! {
    static RULES: RefCell<HashMap<String, Option<Rc<Rules>>>> = RefCell::new(HashMap::new());
}

/// The rules of the timezonedb zone `name`.
fn load_rules(name: &str) -> Option<Rc<Rules>> {
    if let Some(r) = RULES.with(|c| c.borrow().get(name).cloned()) {
        return r;
    }
    let r = match timezone_data::load(name) {
        Ok(z) => Some(Rc::new(rules_from_tzdata(&z))),
        Err(_) => parse_tzfile(name).map(|tz| Rc::new(rules_from_tzinfo(&tz))),
    };
    RULES.with(|c| c.borrow_mut().insert(name.to_string(), r.clone()));
    r
}

// ---------------------------------------------------------------------------
// System zone metadata

/// The index of system zone `id` (exact, case-sensitive).
pub fn system_index(id: &str) -> Option<usize> {
    data::ZONES
        .binary_search_by(|z| z.0.as_bytes().cmp(id.as_bytes()))
        .ok()
        .or_else(|| data::ZONES.iter().position(|z| z.0 == id))
}

/// The ICU canonical ID of the system zone at `i`.
pub fn canonical_of(i: usize) -> &'static str {
    data::ZONES[data::ZONES[i].1 as usize].0
}

/// The IANA ID (TimeZone::getIanaID()) of an ICU canonical ID.
pub fn iana_of(canonical: &str) -> &str {
    data::IANA_MAP
        .iter()
        .find(|(c, _)| *c == canonical)
        .map(|(_, i)| *i)
        .unwrap_or(canonical)
}

/// TimeZone::parseCustomID(): the offset (ms) of "GMT[+-]hh[[:]mm[[:]ss]]".
pub fn parse_custom_id(id: &str) -> Option<i32> {
    let b = id.as_bytes();
    if b.len() <= 3 || !b[..3].eq_ignore_ascii_case(b"GMT") {
        return None;
    }
    let neg = match b[3] {
        b'+' => false,
        b'-' => true,
        _ => return None,
    };
    let mut pos = 4;
    let digits = |pos: &mut usize| -> (i64, usize) {
        let st = *pos;
        let mut n: i64 = 0;
        while *pos < b.len() && b[*pos].is_ascii_digit() {
            n = (n * 10 + (b[*pos] - b'0') as i64).min(1_000_000_000);
            *pos += 1;
        }
        (n, *pos - st)
    };
    let (mut hour, hl) = digits(&mut pos);
    if hl == 0 {
        return None;
    }
    let mut min = 0;
    let mut sec = 0;
    if pos < b.len() {
        if hl > 2 || b[pos] != b':' {
            return None;
        }
        pos += 1;
        let (m, ml) = digits(&mut pos);
        if ml != 2 {
            return None;
        }
        min = m;
        if pos < b.len() {
            if b[pos] != b':' {
                return None;
            }
            pos += 1;
            let (s, sl) = digits(&mut pos);
            if sl != 2 || pos != b.len() {
                return None;
            }
            sec = s;
        }
    } else {
        match hl {
            1 | 2 => {}
            3 | 4 => {
                min = hour % 100;
                hour /= 100;
            }
            5 | 6 => {
                sec = hour % 100;
                min = (hour / 100) % 100;
                hour /= 10000;
            }
            _ => return None,
        }
    }
    if hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    let ms = ((hour * 60 + min) * 60 + sec) * 1000;
    Some(if neg { -ms as i32 } else { ms as i32 })
}

/// TimeZone::formatCustomID()
pub fn format_custom_id(offset: i32) -> String {
    let neg = offset < 0;
    let a = (offset as i64).abs() / 1000;
    let (h, m, s) = (a / 3600, a / 60 % 60, a % 60);
    let mut id = format!("GMT{}{:02}:{:02}", if neg { '-' } else { '+' }, h, m);
    if s != 0 {
        id.push_str(&format!(":{:02}", s));
    }
    id
}

/// TimeZone::createTimeZone(): a system zone, a custom one, or None
/// (getUnknown()).
pub fn create(id: &str) -> Option<Rc<Zone>> {
    if id == UNKNOWN_ID {
        // a loadable zone of its own (not getUnknown())
        return Some(unknown());
    }
    if let Some(i) = system_index(id) {
        return Some(Rc::new(system_zone(id, i)));
    }
    let off = parse_custom_id(id)?;
    Some(Rc::new(Zone::fixed(&format_custom_id(off), off)))
}

/// TimeZone::createTimeZone() with the unknown zone for unknown IDs.
pub fn create_or_unknown(id: &str) -> Rc<Zone> {
    create(id).unwrap_or_else(unknown)
}

pub fn unknown() -> Rc<Zone> {
    Rc::new(Zone::fixed(UNKNOWN_ID, 0))
}

pub fn gmt() -> Rc<Zone> {
    Rc::new(Zone::fixed("GMT", 0))
}

fn system_zone(id: &str, i: usize) -> Zone {
    let canon = canonical_of(i);
    // ICU's zones of an equivalence group are links to one IANA Zone (the
    // one zone1970.tab lists); the database's own entries of the others
    // carry backzone history ICU does not have.
    let group = data::EQUIV_GROUPS[data::ZONES[i].5 as usize];
    let source = group
        .iter()
        .map(|&g| data::ZONES[g as usize].0)
        .find(|n| timezone_data::meta(n).is_some());
    let rules = source
        .and_then(load_rules)
        .or_else(|| load_rules(canon))
        .or_else(|| load_rules(id))
        .or_else(|| load_rules(iana_of(canon)));
    // ICU's SystemV/ zones: fixed offsets, or the US-rule POSIX zones
    let rules = rules.or_else(|| {
        let sv = id.strip_prefix("SystemV/")?;
        if sv.len() > 4 {
            load_rules(
                &sv.replace("AST4ADT", "America/Halifax")
                    .replace("YST9YDT", "America/Anchorage"),
            )
        } else {
            None
        }
    });
    let (rules, fixed) = match rules {
        Some(r) if r.periods.len() > 1 || r.final_save.is_some() => (Some(r), 0),
        Some(r) => (None, r.periods[0].raw + r.periods[0].dst),
        None => {
            let digits: String = id
                .trim_start_matches("SystemV/")
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let h: i32 = digits.parse().unwrap_or(0);
            (
                None,
                if id.starts_with("SystemV/") {
                    -h * 3_600_000
                } else {
                    0
                },
            )
        }
    };
    Zone {
        id: id.to_string(),
        rules,
        fixed,
    }
}

/// TimeZone::getCanonicalID(): (canonical ID, is a system ID).
pub fn canonical_id(id: &str) -> Option<(String, bool)> {
    if id == UNKNOWN_ID {
        return Some((UNKNOWN_ID.to_string(), false));
    }
    if let Some(i) = system_index(id) {
        if data::ZONES[i].4 & 4 == 0 {
            return Some((canonical_of(i).to_string(), true));
        }
    }
    let off = parse_custom_id(id)?;
    Some((format_custom_id(off), false))
}
