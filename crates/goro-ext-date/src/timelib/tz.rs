//! Port of timelib parse_tz.c / parse_posix.c, reading PHP's bundled
//! timezonedb (extracted verbatim from ext/date/lib/timezonedb.h).

use super::{TimeOffset, SECS_PER_DAY};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

static TZDB_DATA: &[u8] = include_bytes!("../data/tzdb.bin");

#[path = "../data/tzdb_index.rs"]
mod index;
pub use index::{TZDB_INDEX, TZDB_VERSION};

#[derive(Debug, Clone, Copy, Default)]
pub struct TtInfo {
    pub offset: i32,
    pub isdst: i32,
    pub abbr_idx: usize,
    pub isstdcnt: u32,
    pub isgmtcnt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PosixTransType {
    JulianNoFeb29,
    JulianFeb29,
    Mwd,
}

#[derive(Debug, Clone, Copy)]
pub struct PosixTransInfo {
    pub ttype: PosixTransType,
    pub days: i64,
    pub month: i64,
    pub week: i64,
    pub dow: i64,
    pub hour: i64,
}

#[derive(Debug, Clone)]
pub struct PosixInfo {
    pub std: String,
    pub std_offset: i64,
    pub dst: Option<String>,
    pub dst_offset: i64,
    pub dst_begin: Option<PosixTransInfo>,
    pub dst_end: Option<PosixTransInfo>,
    pub type_index_std_type: usize,
    pub type_index_dst_type: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TzInfo {
    pub name: String,
    pub bit32_timecnt: u32,
    pub timecnt: usize,
    pub typecnt: usize,
    pub charcnt: usize,
    pub leapcnt: usize,
    pub trans: Vec<i64>,
    pub trans_idx: Vec<u8>,
    pub types: Vec<TtInfo>,
    pub timezone_abbr: Vec<u8>,
    pub leap_times: Vec<(i64, i32)>,
    pub bc: bool,
    pub country_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub comments: String,
    pub posix_string: Option<String>,
    pub posix_info: Option<PosixInfo>,
}

fn be32(b: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}
fn be64(b: &[u8], p: usize) -> i64 {
    i64::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3], b[p + 4], b[p + 5], b[p + 6], b[p + 7]])
}

/// Binary search for a timezone id (case-insensitive), returns the canonical
/// db name and its data offset.
pub fn seek_to_tz_position(name: &str) -> Option<(&'static str, usize)> {
    let lname = name.to_ascii_lowercase();
    let mut left: isize = 0;
    let mut right: isize = TZDB_INDEX.len() as isize - 1;
    while left <= right {
        let mid = ((left + right) as usize) >> 1;
        let (id, pos) = TZDB_INDEX[mid];
        let cmp = lname.as_bytes().cmp(id.to_ascii_lowercase().as_bytes());
        match cmp {
            std::cmp::Ordering::Less => right = mid as isize - 1,
            std::cmp::Ordering::Greater => left = mid as isize + 1,
            std::cmp::Ordering::Equal => return Some((id, pos as usize)),
        }
    }
    None
}

pub fn timezone_id_is_valid(name: &str) -> bool {
    seek_to_tz_position(name).is_some()
}

thread_local! {
    static TZ_CACHE: RefCell<HashMap<String, Rc<TzInfo>>> = RefCell::new(HashMap::new());
}

/// Equivalent of php_date_parse_tzfile: looks up (with cache) a timezone by
/// identifier. The returned TzInfo carries the name as given by the caller
/// (PHP keeps the user's casing of the identifier).
pub fn parse_tzfile(name: &str) -> Option<Rc<TzInfo>> {
    if let Some(t) = TZ_CACHE.with(|c| c.borrow().get(name).cloned()) {
        return Some(t);
    }
    let (_id, pos) = seek_to_tz_position(name)?;
    let mut tz = parse_at(pos)?;
    tz.name = name.to_string();
    let rc = Rc::new(tz);
    TZ_CACHE.with(|c| c.borrow_mut().insert(name.to_string(), rc.clone()));
    Some(rc)
}

fn parse_at(start: usize) -> Option<TzInfo> {
    let b = TZDB_DATA;
    let mut p = start;
    let mut tz = TzInfo::default();
    // PHP preamble
    let is_php = &b[p..p + 3] == b"PHP";
    if is_php {
        let version = b[p + 3].wrapping_sub(b'0');
        if !(2..=4).contains(&version) {
            return None;
        }
        p += 4;
        tz.bc = b[p] == 1;
        p += 1;
        tz.country_code = String::from_utf8_lossy(&b[p..p + 2]).into_owned();
        p += 2;
        p += 13;
    } else if &b[p..p + 4] == b"TZif" {
        p += 5 + 15;
        tz.country_code = "??".to_string();
    } else {
        return None;
    }
    // 32-bit header
    let ttisgmtcnt = be32(b, p) as usize;
    let ttisstdcnt = be32(b, p + 4) as usize;
    let leapcnt = be32(b, p + 8) as usize;
    let timecnt = be32(b, p + 12) as usize;
    let typecnt = be32(b, p + 16) as usize;
    let charcnt = be32(b, p + 20) as usize;
    tz.bit32_timecnt = timecnt as u32;
    p += 24;
    // skip 32 bit transitions
    p += timecnt * 4 + timecnt;
    // skip 32 bit types
    p += 6 * typecnt + charcnt + leapcnt * 8 + ttisstdcnt + ttisgmtcnt;
    // 64-bit preamble
    if &b[p..p + 4] != b"TZif" {
        return None;
    }
    p += 20;
    let ttisgmtcnt = be32(b, p) as usize;
    let ttisstdcnt = be32(b, p + 4) as usize;
    let leapcnt = be32(b, p + 8) as usize;
    let timecnt = be32(b, p + 12) as usize;
    let typecnt = be32(b, p + 16) as usize;
    let charcnt = be32(b, p + 20) as usize;
    p += 24;
    tz.timecnt = timecnt;
    tz.typecnt = typecnt;
    tz.charcnt = charcnt;
    tz.leapcnt = leapcnt;
    for i in 0..timecnt {
        tz.trans.push(be64(b, p + i * 8));
    }
    p += timecnt * 8;
    tz.trans_idx = b[p..p + timecnt].to_vec();
    p += timecnt;
    for i in 0..typecnt {
        let j = p + i * 6;
        let offset = ((b[j] as u32) << 24 | (b[j + 1] as u32) << 16 | (b[j + 2] as u32) << 8 | b[j + 3] as u32) as i32;
        tz.types.push(TtInfo { offset, isdst: b[j + 4] as i32, abbr_idx: b[j + 5] as usize, isstdcnt: 0, isgmtcnt: 0 });
    }
    p += typecnt * 6;
    tz.timezone_abbr = b[p..p + charcnt].to_vec();
    p += charcnt;
    for i in 0..leapcnt {
        let t = be64(b, p + i * 12);
        let o = be32(b, p + i * 12 + 8) as i32;
        tz.leap_times.push((t, o));
    }
    p += leapcnt * 12;
    for i in 0..ttisstdcnt {
        if i < tz.types.len() {
            tz.types[i].isstdcnt = b[p + i] as u32;
        }
    }
    p += ttisstdcnt;
    for i in 0..ttisgmtcnt {
        if i < tz.types.len() {
            tz.types[i].isgmtcnt = b[p + i] as u32;
        }
    }
    p += ttisgmtcnt;
    // POSIX string, delimited by \n
    p += 1;
    let begin = p;
    while b[p] != b'\n' {
        p += 1;
    }
    let posix = String::from_utf8_lossy(&b[begin..p]).into_owned();
    p += 1;
    if !posix.is_empty() {
        tz.posix_string = Some(posix.clone());
        if !integrate_posix_string(&mut tz, &posix) {
            return None;
        }
    } else {
        tz.posix_string = Some(String::new());
    }
    if is_php {
        let lat = be32(b, p) as f64;
        let lon = be32(b, p + 4) as f64;
        let clen = be32(b, p + 8) as usize;
        p += 12;
        tz.latitude = (lat / 100000.0) - 90.0;
        tz.longitude = (lon / 100000.0) - 180.0;
        tz.comments = String::from_utf8_lossy(&b[p..p + clen]).into_owned();
    } else {
        tz.comments = "?".to_string();
    }
    Some(tz)
}

impl TzInfo {
    pub fn abbr_at(&self, idx: usize) -> String {
        let s = &self.timezone_abbr[idx.min(self.timezone_abbr.len())..];
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        String::from_utf8_lossy(&s[..end]).into_owned()
    }

    fn find_ttinfo_index(&self, offset: i64, isdst: i32, abbr: &str) -> Option<usize> {
        (0..self.typecnt).find(|&i| {
            self.types[i].offset as i64 == offset && self.types[i].isdst == isdst && self.abbr_at(self.types[i].abbr_idx) == abbr
        })
    }

    fn add_new_ttinfo_index(&mut self, offset: i64, isdst: i32, abbr: &str) -> usize {
        let idx = self.timezone_abbr.len();
        self.timezone_abbr.extend_from_slice(abbr.as_bytes());
        self.timezone_abbr.push(0);
        self.charcnt = self.timezone_abbr.len();
        let t = TtInfo { offset: offset as i32, isdst, abbr_idx: idx, isstdcnt: 0, isgmtcnt: 0 };
        if self.types.len() > self.typecnt {
            self.types[self.typecnt] = t;
        } else {
            self.types.push(t);
        }
        self.typecnt += 1;
        self.typecnt - 1
    }

    /// timelib_fetch_timezone_offset: returns (ttinfo, transition_time)
    pub fn fetch_timezone_offset(&self, ts: i64) -> Option<(TtInfo, i64)> {
        if self.timecnt == 0 || self.trans.is_empty() {
            if self.posix_info.is_some() {
                return self.fetch_posix_timezone_offset(ts).map(|(t, _)| (t, i64::MIN));
            }
            if self.typecnt == 1 {
                return Some((self.types[0], i64::MIN));
            }
            return None;
        }
        if ts < self.trans[0] {
            return Some((self.types[0], i64::MIN));
        }
        if ts >= self.trans[self.timecnt - 1] {
            if self.posix_info.is_some() {
                return self.fetch_posix_timezone_offset(ts);
            }
            return Some((self.types[self.trans_idx[self.timecnt - 1] as usize], self.trans[self.timecnt - 1]));
        }
        let mut left = 0usize;
        let mut right = self.timecnt - 1;
        while right - left > 1 {
            let mid = (left + right) >> 1;
            if ts < self.trans[mid] {
                right = mid;
            } else {
                left = mid;
            }
        }
        Some((self.types[self.trans_idx[left] as usize], self.trans[left]))
    }

    fn fetch_leaptime_offset(&self, ts: i64) -> Option<(i64, i32)> {
        if self.leap_times.is_empty() {
            return None;
        }
        let mut i = self.leap_times.len() - 1;
        while i > 0 {
            if ts > self.leap_times[i].0 {
                return Some(self.leap_times[i]);
            }
            i -= 1;
        }
        None
    }

    /// timelib_get_time_zone_info
    pub fn get_time_zone_info(&self, ts: i64) -> TimeOffset {
        let mut tmp = TimeOffset::default();
        let (offset, abbr) = if let Some((to, tt)) = self.fetch_timezone_offset(ts) {
            tmp.is_dst = to.isdst as u32;
            tmp.transition_time = tt;
            (to.offset, self.abbr_at(to.abbr_idx))
        } else {
            tmp.is_dst = 0;
            tmp.transition_time = 0;
            (0, self.abbr_at(0))
        };
        if let Some((_, o)) = self.fetch_leaptime_offset(ts) {
            tmp.leap_secs = (-o) as u32;
        }
        tmp.offset = offset;
        tmp.abbr = abbr;
        tmp
    }

    /// timelib_get_time_zone_offset_info: (offset, transition_time, is_dst)
    pub fn get_offset_info(&self, ts: i64) -> Option<(i32, i64, u32)> {
        self.fetch_timezone_offset(ts).map(|(to, tt)| (to.offset, tt, to.isdst as u32))
    }

    pub fn timestamp_is_in_dst(&self, ts: i64) -> i32 {
        match self.fetch_timezone_offset(ts) {
            Some((to, _)) => to.isdst,
            None => -1,
        }
    }

    /// timelib_get_transitions_for_year: pushes two (time, type) pairs
    pub fn get_transitions_for_year(&self, year: i64, out: &mut Vec<(i64, usize)>) {
        let pi = self.posix_info.as_ref().unwrap();
        let year_begin_ts = ts_at_start_of_year(year);
        let db = pi.dst_begin.as_ref().unwrap();
        let de = pi.dst_end.as_ref().unwrap();
        let trans_begin = year_begin_ts + calc_transition(db, year) + db.hour - pi.std_offset;
        let trans_end = year_begin_ts + calc_transition(de, year) + de.hour - pi.dst_offset;
        if trans_begin < trans_end {
            out.push((trans_begin, pi.type_index_dst_type));
            out.push((trans_end, pi.type_index_std_type));
        } else {
            out.push((trans_end, pi.type_index_std_type));
            out.push((trans_begin, pi.type_index_dst_type));
        }
    }

    /// timelib_fetch_posix_timezone_offset
    pub fn fetch_posix_timezone_offset(&self, ts: i64) -> Option<(TtInfo, i64)> {
        let pi = self.posix_info.as_ref()?;
        if pi.dst_end.is_none() {
            let tt = if self.timecnt > 0 { self.trans[self.timecnt - 1] } else { i64::MIN };
            return Some((self.types[pi.type_index_std_type], tt));
        }
        let (year, _, _) = super::unixtime2tm::unixtime2date(ts);
        let mut transitions = Vec::with_capacity(6);
        self.get_transitions_for_year(year - 1, &mut transitions);
        self.get_transitions_for_year(year, &mut transitions);
        self.get_transitions_for_year(year + 1, &mut transitions);
        for i in 1..transitions.len() {
            if ts < transitions[i].0 {
                return Some((self.types[transitions[i - 1].1], transitions[i - 1].0));
            }
        }
        None
    }
}

fn integrate_posix_string(tz: &mut TzInfo, posix: &str) -> bool {
    let mut pi = match parse_posix_str(posix) {
        Some(p) => p,
        None => return false,
    };
    match tz.find_ttinfo_index(pi.std_offset, 0, &pi.std) {
        Some(i) => pi.type_index_std_type = i,
        None => {
            pi.type_index_std_type = tz.add_new_ttinfo_index(pi.std_offset, 0, &pi.std.clone());
            tz.posix_info = Some(pi);
            return true;
        }
    }
    if pi.dst.is_none() {
        tz.posix_info = Some(pi);
        return true;
    }
    let dst = pi.dst.clone().unwrap();
    match tz.find_ttinfo_index(pi.dst_offset, 1, &dst) {
        Some(i) => pi.type_index_dst_type = i,
        None => {
            pi.type_index_dst_type = tz.add_new_ttinfo_index(pi.dst_offset, 1, &dst);
        }
    }
    tz.posix_info = Some(pi);
    true
}

const POSIX_UNSET: i64 = super::UNSET;

struct PPtr<'a> {
    s: &'a [u8],
    p: usize,
}
impl PPtr<'_> {
    fn c(&self) -> u8 {
        *self.s.get(self.p).unwrap_or(&0)
    }
}

fn read_description(ptr: &mut PPtr) -> Option<String> {
    if ptr.c() == b'<' {
        ptr.p += 1;
        let begin = ptr.p;
        while ptr.c() != 0 && ptr.c() != b'>' {
            ptr.p += 1;
        }
        if ptr.c() == 0 {
            return None;
        }
        let end = ptr.p;
        ptr.p += 1;
        if end - begin < 1 {
            return None;
        }
        Some(String::from_utf8_lossy(&ptr.s[begin..end]).into_owned())
    } else {
        let begin = ptr.p;
        while ptr.c().is_ascii_alphabetic() {
            ptr.p += 1;
        }
        if ptr.p - begin < 1 {
            return None;
        }
        Some(String::from_utf8_lossy(&ptr.s[begin..ptr.p]).into_owned())
    }
}

fn read_sign(ptr: &mut PPtr) -> i64 {
    match ptr.c() {
        b'+' => {
            ptr.p += 1;
            1
        }
        b'-' => {
            ptr.p += 1;
            -1
        }
        _ => 1,
    }
}

fn read_number(ptr: &mut PPtr) -> i64 {
    let begin = ptr.p;
    while ptr.c() == b'0' {
        ptr.p += 1;
    }
    let mut acc: i64 = 0;
    while ptr.c().is_ascii_digit() {
        acc = acc * 10 + (ptr.c() - b'0') as i64;
        ptr.p += 1;
    }
    if begin == ptr.p {
        return POSIX_UNSET;
    }
    acc
}

fn read_offset(ptr: &mut PPtr) -> i64 {
    let bias = read_sign(ptr);
    let begin = ptr.p;
    let hours = read_number(ptr);
    if hours == POSIX_UNSET {
        return hours;
    }
    let mut minutes = 0;
    let mut seconds = 0;
    if ptr.c() == b':' {
        ptr.p += 1;
        minutes = read_number(ptr);
        if minutes == POSIX_UNSET {
            return minutes;
        }
    }
    if ptr.c() == b':' {
        ptr.p += 1;
        seconds = read_number(ptr);
        if seconds == POSIX_UNSET {
            return seconds;
        }
    }
    if begin == ptr.p {
        return POSIX_UNSET;
    }
    -bias * (hours * 3600 + minutes * 60 + seconds)
}

fn read_transition_spec(ptr: &mut PPtr) -> Option<PosixTransInfo> {
    let mut tmp = PosixTransInfo { ttype: PosixTransType::JulianFeb29, days: 0, month: 0, week: 0, dow: 0, hour: 2 * 3600 };
    if ptr.c() == b'M' {
        tmp.ttype = PosixTransType::Mwd;
        ptr.p += 1;
        tmp.month = read_number(ptr);
        if tmp.month == POSIX_UNSET || ptr.c() != b'.' {
            return None;
        }
        ptr.p += 1;
        tmp.week = read_number(ptr);
        if tmp.week == POSIX_UNSET || ptr.c() != b'.' {
            return None;
        }
        ptr.p += 1;
        tmp.dow = read_number(ptr);
        if tmp.dow == POSIX_UNSET {
            return None;
        }
    } else {
        if ptr.c() == b'J' {
            tmp.ttype = PosixTransType::JulianNoFeb29;
            ptr.p += 1;
        }
        tmp.days = read_number(ptr);
        if tmp.days == POSIX_UNSET {
            return None;
        }
    }
    if ptr.c() == b'/' {
        ptr.p += 1;
        tmp.hour = read_offset(ptr);
        if tmp.hour == POSIX_UNSET {
            return None;
        }
        tmp.hour = -tmp.hour;
    }
    Some(tmp)
}

pub fn parse_posix_str(posix: &str) -> Option<PosixInfo> {
    let mut ptr = PPtr { s: posix.as_bytes(), p: 0 };
    let std = read_description(&mut ptr)?;
    let std_offset = read_offset(&mut ptr);
    if std_offset == POSIX_UNSET {
        return None;
    }
    let mut pi = PosixInfo {
        std,
        std_offset,
        dst: None,
        dst_offset: 0,
        dst_begin: None,
        dst_end: None,
        type_index_std_type: 0,
        type_index_dst_type: 0,
    };
    if ptr.c() == 0 {
        return Some(pi);
    }
    pi.dst_offset = pi.std_offset + 3600;
    pi.dst = Some(read_description(&mut ptr)?);
    if ptr.c() != b',' && ptr.c() != 0 {
        pi.dst_offset = read_offset(&mut ptr);
        if pi.dst_offset == POSIX_UNSET {
            return None;
        }
    }
    if ptr.c() != b',' {
        return None;
    }
    ptr.p += 1;
    pi.dst_begin = Some(read_transition_spec(&mut ptr)?);
    if ptr.c() != b',' {
        return None;
    }
    ptr.p += 1;
    pi.dst_end = Some(read_transition_spec(&mut ptr)?);
    if ptr.c() != 0 {
        return None;
    }
    Some(pi)
}

const MONTH_LENGTHS: [[i64; 12]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

fn calc_transition(psi: &PosixTransInfo, year: i64) -> i64 {
    let leap_year = super::is_leap(year) as usize;
    match psi.ttype {
        PosixTransType::JulianNoFeb29 => {
            let mut value = psi.days - 1;
            if leap_year == 1 && psi.days >= 60 {
                value += 1;
            }
            value * SECS_PER_DAY
        }
        PosixTransType::JulianFeb29 => psi.days * SECS_PER_DAY,
        PosixTransType::Mwd => {
            let m1 = (psi.month + 9) % 12 + 1;
            let yy0 = if psi.month <= 2 { year - 1 } else { year };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow = ((26 * m1 - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
            if dow < 0 {
                dow += 7;
            }
            let mut d = psi.dow - dow;
            if d < 0 {
                d += 7;
            }
            let mut i = 1;
            while i < psi.week {
                if d + 7 >= MONTH_LENGTHS[leap_year][(psi.month - 1) as usize] {
                    break;
                }
                d += 7;
                i += 1;
            }
            let mut value = d * SECS_PER_DAY;
            for i in 0..(psi.month - 1) as usize {
                value += MONTH_LENGTHS[leap_year][i] * SECS_PER_DAY;
            }
            value
        }
    }
}

fn count_leap_years(y: i64) -> i64 {
    let y = y - 1;
    (y / 4) - (y / 100) + (y / 400)
}

pub fn ts_at_start_of_year(year: i64) -> i64 {
    let epoch_leap_years = count_leap_years(1970);
    let current_leap_years = count_leap_years(year);
    SECS_PER_DAY * (((year - 1970) * 365) + current_leap_years - epoch_leap_years)
}
