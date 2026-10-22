//! Port of date.c: the SQL date and time functions (julianday(),
//! unixepoch(), date(), time(), datetime(), strftime(), timediff(),
//! current_time(), current_date(), current_timestamp()).
//!
//! The internal representation is a julian day number times 86400000
//! (milliseconds), as in C.  "localtime" and "utc" use libc localtime_r().

use crate::func::DFUNCTION;
use crate::global::{sqlite3Isdigit, sqlite3Isspace, sqlite3UpperToLower};
use crate::int::*;
use crate::printf::{
    PfArg, sqlite3_str_append, sqlite3_str_appendchar, sqlite3_str_appendf, sqlite3_str_new,
    sqlite3StrAccumInit,
};
use crate::util::{
    sqlite3_stricmp, sqlite3_strnicmp, sqlite3AtoF, sqlite3StrICmp, sqlite3Strlen30,
};
use crate::vdbeapi::*;
use std::rc::Rc;

/// Byte `i` of a zero-terminated string held in a slice (0 past the end).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// The string starting at byte `i` (empty past the end).
#[inline]
fn from(z: &[u8], i: usize) -> &[u8] {
    if i <= z.len() { &z[i..] } else { b"" }
}

/// A structure for holding a single date and time.
#[derive(Clone, Copy, Default, Debug)]
struct DateTime {
    /// The julian day number times 86400000
    iJD: i64,
    /// Year, month, and day
    Y: i32,
    M: i32,
    D: i32,
    /// Hour and minutes
    h: i32,
    m: i32,
    /// Timezone offset in minutes
    tz: i32,
    /// Seconds
    s: f64,
    /// True (1) if iJD is valid
    validJD: u8,
    /// True (1) if Y,M,D are valid
    validYMD: u8,
    /// True (1) if h,m,s are valid
    validHMS: u8,
    /// Days to implement "floor"
    nFloor: i8,
    /// Raw numeric value stored in s
    rawS: u8,
    /// An overflow has occurred
    isError: u8,
    /// Display subsecond precision
    useSubsec: u8,
    /// Time is known to be UTC
    isUtc: u8,
    /// Time is known to be localtime
    isLocal: u8,
}

/// Convert zDate into one or more integers according to the conversion
/// specifier zFormat (4 characters per integer: number of digits, minimum
/// value, maximum value code a..f, separator or 0 for the last one).
/// Returns the number of successful conversions; the values are written
/// into `vals`.
fn getDigits(zDate: &[u8], zFormat: &[u8], vals: &mut [i32]) -> i32 {
    // The aMx[] array translates the 3rd character of each format spec into
    // a max size:    a   b   c   d   e      f
    static aMx: [u16; 6] = [12, 14, 24, 31, 59, 14712];
    let mut cnt = 0i32;
    let mut zi = 0usize;
    let mut fi = 0usize;
    loop {
        let mut N = at(zFormat, fi) as i32 - b'0' as i32;
        let min = at(zFormat, fi + 1) as i32 - b'0' as i32;
        debug_assert!(at(zFormat, fi + 2) >= b'a' && at(zFormat, fi + 2) <= b'f');
        let max = aMx[(at(zFormat, fi + 2) - b'a') as usize] as i32;
        let nextC = at(zFormat, fi + 3);
        let mut val = 0i32;
        while N > 0 {
            N -= 1;
            if !sqlite3Isdigit(at(zDate, zi)) {
                return cnt;
            }
            val = val * 10 + (zDate[zi] - b'0') as i32;
            zi += 1;
        }
        if val < min || val > max || (nextC != 0 && nextC != at(zDate, zi)) {
            return cnt;
        }
        vals[cnt as usize] = val;
        zi += 1;
        cnt += 1;
        fi += 4;
        if nextC == 0 {
            break;
        }
    }
    cnt
}

/// Parse a timezone extension on the end of a date-time: (+/-)HH:MM or
/// "Z".  If the parse is successful, write the number of minutes of change
/// in p->tz and return 0.  If a parser error occurs, return non-zero.  A
/// missing specifier is not considered an error.
fn parseTimezone(zDate: &[u8], p: &mut DateTime) -> i32 {
    let sgn: i32;
    let mut zi = 0usize;
    while sqlite3Isspace(at(zDate, zi)) {
        zi += 1;
    }
    p.tz = 0;
    let c = at(zDate, zi);
    if c == b'-' {
        sgn = -1;
    } else if c == b'+' {
        sgn = 1;
    } else if c == b'Z' || c == b'z' {
        zi += 1;
        p.isLocal = 0;
        p.isUtc = 1;
        // zulu_time:
        while sqlite3Isspace(at(zDate, zi)) {
            zi += 1;
        }
        return (at(zDate, zi) != 0) as i32;
    } else {
        return (c != 0) as i32;
    }
    zi += 1;
    let mut v = [0i32; 2];
    if getDigits(from(zDate, zi), b"20b:20e", &mut v) != 2 {
        return 1;
    }
    let (nHr, nMn) = (v[0], v[1]);
    zi += 5;
    p.tz = sgn * (nMn + nHr * 60);
    if p.tz == 0 {
        // Forum post 2025-09-17T10:12:14z
        p.isLocal = 0;
        p.isUtc = 1;
    }
    // zulu_time:
    while sqlite3Isspace(at(zDate, zi)) {
        zi += 1;
    }
    (at(zDate, zi) != 0) as i32
}

/// Parse times of the form HH:MM or HH:MM:SS or HH:MM:SS.FFFF.  The HH, MM,
/// and SS must each be exactly 2 digits.  The fractional seconds FFFF can
/// be one or more digits.  Return 1 if there is a parsing error and 0 on
/// success.
fn parseHhMmSs(zDate: &[u8], p: &mut DateTime) -> i32 {
    let mut v = [0i32; 2];
    let mut ms = 0.0f64;
    if getDigits(zDate, b"20c:20e", &mut v) != 2 {
        return 1;
    }
    let (h, m) = (v[0], v[1]);
    let s: i32;
    let mut zi = 5usize;
    if at(zDate, zi) == b':' {
        zi += 1;
        let mut sv = [0i32; 1];
        if getDigits(from(zDate, zi), b"20e", &mut sv) != 1 {
            return 1;
        }
        s = sv[0];
        zi += 2;
        if at(zDate, zi) == b'.' && sqlite3Isdigit(at(zDate, zi + 1)) {
            let mut rScale = 1.0f64;
            zi += 1;
            while sqlite3Isdigit(at(zDate, zi)) {
                ms = ms * 10.0 + (zDate[zi] - b'0') as f64;
                rScale *= 10.0;
                zi += 1;
            }
            ms /= rScale;
            // Truncate to avoid problems with sub-milliseconds rounding.
            // https://sqlite.org/forum/forumpost/766a2c9231
            if ms > 0.999 {
                ms = 0.999;
            }
        }
    } else {
        s = 0;
    }
    if h == 24 && (m != 0 || s != 0 || ms != 0.0) {
        return 1;
    }
    p.validJD = 0;
    p.rawS = 0;
    p.validHMS = 1;
    p.h = h;
    p.m = m;
    p.s = s as f64 + ms;
    if parseTimezone(from(zDate, zi), p) != 0 {
        return 1;
    }
    0
}

/// Put the DateTime object into its error state.
fn datetimeError(p: &mut DateTime) {
    *p = DateTime::default();
    p.isError = 1;
}

/// Convert from YYYY-MM-DD HH:MM:SS to julian day.  We always assume that
/// the YYYY-MM-DD is according to the Gregorian calendar.
///
/// Reference:  Meeus page 61
fn computeJD(p: &mut DateTime) {
    let mut Y: i32;
    let mut M: i32;
    let D: i32;
    if p.validJD != 0 {
        return;
    }
    if p.validYMD != 0 {
        Y = p.Y;
        M = p.M;
        D = p.D;
    } else {
        Y = 2000; // If no YMD specified, assume 2000-Jan-01
        M = 1;
        if p.validHMS != 0 && p.h == 24 {
            p.h = 0;
            D = 2;
        } else {
            D = 1;
        }
    }
    if Y < -4713 || Y > 9999 || p.rawS != 0 {
        datetimeError(p);
        return;
    }
    if M <= 2 {
        Y -= 1;
        M += 12;
    }
    let A = (Y + 4800) / 100;
    let B = 38 - A + (A / 4);
    let X1 = 36525 * (Y + 4716) / 100;
    let X2 = 306001 * (M + 1) / 10000;
    p.iJD = (((X1 + X2 + D + B) as f64 - 1524.5) * 86400000.0) as i64;
    p.validJD = 1;
    if p.validHMS != 0 {
        p.iJD += (p.h * 3600000 + p.m * 60000) as i64 + (p.s * 1000.0 + 0.5) as i64;
        if p.tz != 0 {
            p.iJD -= p.tz as i64 * 60000;
            p.validYMD = 0;
            p.validHMS = 0;
            p.tz = 0;
            p.isUtc = 1;
            p.isLocal = 0;
        }
    }
}

/// Return the number of dates in a specific month.
fn daysOfMonth(Y: i32, M: i32) -> i32 {
    if ((1i32 << M) & 0x15aa) != 0 {
        31
    } else if M != 2 {
        30
    } else if Y % 4 == 0 && (Y % 100 != 0 || Y % 400 == 0) {
        29
    } else {
        28
    }
}

/// Given the YYYY-MM-DD information current in p, determine if there is
/// day-of-month overflow and set nFloor to the number of days that would
/// need to be subtracted from the date in order to bring the date back to
/// the end of the month.
fn computeFloor(p: &mut DateTime) {
    debug_assert!(p.validYMD != 0 || p.isError != 0);
    debug_assert!(p.D >= 0 && p.D <= 32);
    debug_assert!(p.M >= 0 && p.M <= 12);
    let mxD = daysOfMonth(p.Y, p.M);
    p.nFloor = if p.D > mxD { (p.D - mxD) as i8 } else { 0 };
}

/// Parse dates of the form `YYYY-MM-DD HH:MM:SS.FFF`, `YYYY-MM-DD HH:MM:SS`,
/// `YYYY-MM-DD HH:MM` or `YYYY-MM-DD`.
/// Write the result into the DateTime structure and return 0 on success
/// and 1 if the input string is not a well-formed date.
fn parseYyyyMmDd(zDate: &[u8], p: &mut DateTime) -> i32 {
    let mut zi = 0usize;
    let neg;
    if at(zDate, 0) == b'-' {
        zi += 1;
        neg = true;
    } else {
        neg = false;
    }
    let mut v = [0i32; 3];
    if getDigits(from(zDate, zi), b"40f-21a-21d", &mut v) != 3 {
        return 1;
    }
    let (mut Y, mut M, mut D) = (v[0], v[1], v[2]);
    zi += 10;
    while sqlite3Isspace(at(zDate, zi)) || b'T' == at(zDate, zi) {
        zi += 1;
    }
    if parseHhMmSs(from(zDate, zi), p) == 0 {
        if p.h == 24 {
            let mxD = daysOfMonth(Y, M);
            p.h = 0;
            D += 1;
            if D > mxD {
                D -= mxD;
                M += 1;
                if M > 12 {
                    M = 1;
                    Y += 1;
                }
            }
        }
    } else if at(zDate, zi) == 0 {
        p.validHMS = 0;
    } else {
        return 1;
    }
    p.validJD = 0;
    p.validYMD = 1;
    p.Y = if neg { -Y } else { Y };
    p.M = M;
    p.D = D;
    computeFloor(p);
    if p.tz != 0 {
        computeJD(p);
    }
    0
}

/// Set the time to the current time reported by the VFS.  Return the
/// number of errors.
fn setDateTimeToCurrent(context: &mut Context, p: &mut DateTime) -> i32 {
    p.iJD = sqlite3StmtCurrentTime(context);
    if p.iJD > 0 {
        p.validJD = 1;
        p.isUtc = 1;
        p.isLocal = 0;
        clearYMD_HMS_TZ(p);
        0
    } else {
        1
    }
}

/// Input "r" is a numeric quantity which might be a julian day number, or
/// the number of seconds since 1970.  If the value if r is within range of
/// a julian day number, install it as such and set validJD.  If the value
/// is a valid unix timestamp, put it in p->s and set p->rawS.
fn setRawDateNumber(p: &mut DateTime, r: f64) {
    p.s = r;
    p.rawS = 1;
    if r >= 0.0 && r < 5373484.5 {
        p.iJD = (r * 86400000.0 + 0.5) as i64;
        p.validJD = 1;
    }
}

/// Attempt to parse the given string into a julian day number.  Return the
/// number of errors.
///
/// Acceptable forms: YYYY-MM-DD HH:MM:SS.FFF +/-HH:MM, DDDD.DD, now.
fn parseDateOrTime(context: &mut Context, zDate: &[u8], p: &mut DateTime) -> i32 {
    let mut r = 0.0f64;
    if parseYyyyMmDd(zDate, p) == 0 {
        0
    } else if parseHhMmSs(zDate, p) == 0 {
        0
    } else if sqlite3StrICmp(Some(zDate), Some(b"now")) == 0
        && crate::vdbeaux::sqlite3NotPureFunc(context) != 0
    {
        setDateTimeToCurrent(context, p)
    } else if sqlite3AtoF(Some(zDate), &mut r) > 0 {
        setRawDateNumber(p, r);
        0
    } else if (sqlite3StrICmp(Some(zDate), Some(b"subsec")) == 0
        || sqlite3StrICmp(Some(zDate), Some(b"subsecond")) == 0)
        && crate::vdbeaux::sqlite3NotPureFunc(context) != 0
    {
        p.useSubsec = 1;
        setDateTimeToCurrent(context, p)
    } else {
        1
    }
}

/// The julian day number for 9999-12-31 23:59:59.999 is 5373484.4999999.
/// Multiplying this by 86400000 gives 464269060799999 as the maximum value
/// for DateTime.iJD.
const INT_464269060799999: i64 = (0x1a640i64 << 32) | 0x1072fdff;

/// Return TRUE if the given julian day number is within range.
fn validJulianDay(iJD: i64) -> bool {
    iJD >= 0 && iJD <= INT_464269060799999
}

/// Compute the Year, Month, and Day from the julian day number.
fn computeYMD(p: &mut DateTime) {
    if p.validYMD != 0 {
        return;
    }
    if p.validJD == 0 {
        p.Y = 2000;
        p.M = 1;
        p.D = 1;
    } else if !validJulianDay(p.iJD) {
        datetimeError(p);
        return;
    } else {
        let Z = ((p.iJD + 43200000) / 86400000) as i32;
        let alpha = ((Z as f64 + 32044.75) / 36524.25) as i32 - 52;
        let A = Z + 1 + alpha - ((alpha + 100) / 4) + 25;
        let B = A + 1524;
        let C = ((B as f64 - 122.1) / 365.25) as i32;
        let D = (36525 * (C & 32767)) / 100;
        let E = ((B - D) as f64 / 30.6001) as i32;
        let X1 = (30.6001 * E as f64) as i32;
        p.D = B - D - X1;
        p.M = if E < 14 { E - 1 } else { E - 13 };
        p.Y = if p.M > 2 { C - 4716 } else { C - 4715 };
    }
    p.validYMD = 1;
}

/// Compute the Hour, Minute, and Seconds from the julian day number.
fn computeHMS(p: &mut DateTime) {
    if p.validHMS != 0 {
        return;
    }
    computeJD(p);
    let day_ms = ((p.iJD + 43200000) % 86400000) as i32;
    p.s = (day_ms % 60000) as f64 / 1000.0;
    let day_min = day_ms / 60000;
    p.m = day_min % 60;
    p.h = day_min / 60;
    p.rawS = 0;
    p.validHMS = 1;
}

/// Compute both YMD and HMS
fn computeYMD_HMS(p: &mut DateTime) {
    computeYMD(p);
    computeHMS(p);
}

/// Clear the YMD and HMS and the TZ
fn clearYMD_HMS_TZ(p: &mut DateTime) {
    p.validYMD = 0;
    p.validHMS = 0;
    p.tz = 0;
}

/// The rough equivalent of localtime_r().  Returns 0 on success and
/// non-zero on any kind of error.
fn osLocaltime(t: i64, pTm: &mut libc::tm) -> i32 {
    if crate::global::sqlite3GlobalConfig().bLocaltimeFault != 0 {
        return 1;
    }
    let tt: libc::time_t = t as libc::time_t;
    // SAFETY: localtime_r() only reads `tt` and writes the caller-provided
    // `tm` structure; both are valid for the duration of the call.
    let r = unsafe { libc::localtime_r(&tt, pTm) };
    r.is_null() as i32
}

/// Assuming the input DateTime is UTC, move it to its localtime equivalent.
fn toLocaltime(p: &mut DateTime, pCtx: &mut Context) -> i32 {
    // SAFETY: libc::tm is a plain C struct for which all-zero bytes is a
    // valid value (as the C code's memset()).
    let mut sLocal: libc::tm = unsafe { std::mem::zeroed() };
    let iYearDiff: i32;
    let t: i64;

    computeJD(p);
    if p.iJD < 2108667600i64 * 100000 // 1970-01-01
        || p.iJD > 2130141456i64 * 100000
    // 2038-01-18
    {
        // EVIDENCE-OF: R-55269-29598 The localtime_r() C function normally
        // only works for years between 1970 and 2037.  For dates outside
        // this range, SQLite attempts to map the year into an equivalent
        // year within this range, do the calculation, then map the year
        // back.
        let mut x = *p;
        computeYMD_HMS(&mut x);
        iYearDiff = (2000 + x.Y % 4) - x.Y;
        x.Y += iYearDiff;
        x.validJD = 0;
        computeJD(&mut x);
        t = x.iJD / 1000 - 21086676i64 * 10000;
    } else {
        iYearDiff = 0;
        t = p.iJD / 1000 - 21086676i64 * 10000;
    }
    if osLocaltime(t, &mut sLocal) != 0 {
        sqlite3_result_error(pCtx, Some(b"local time unavailable"), -1);
        return SQLITE_ERROR;
    }
    p.Y = sLocal.tm_year + 1900 - iYearDiff;
    p.M = sLocal.tm_mon + 1;
    p.D = sLocal.tm_mday;
    p.h = sLocal.tm_hour;
    p.m = sLocal.tm_min;
    p.s = sLocal.tm_sec as f64 + (p.iJD % 1000) as f64 * 0.001;
    p.validYMD = 1;
    p.validHMS = 1;
    p.validJD = 0;
    p.rawS = 0;
    p.tz = 0;
    p.isError = 0;
    SQLITE_OK
}

/// The following table defines various date transformations of the form
/// 'NNN days'.
struct XformType {
    nName: u8,
    zName: &'static [u8],
    /// Maximum NNN value for this transform (a C float)
    rLimit: f32,
    /// Constant used for this transform (a C float)
    rXform: f32,
}

static aXformType: [XformType; 6] = [
    XformType {
        nName: 6,
        zName: b"second",
        rLimit: 4.6427e+14,
        rXform: 1.0,
    },
    XformType {
        nName: 6,
        zName: b"minute",
        rLimit: 7.7379e+12,
        rXform: 60.0,
    },
    XformType {
        nName: 4,
        zName: b"hour",
        rLimit: 1.2897e+11,
        rXform: 3600.0,
    },
    XformType {
        nName: 3,
        zName: b"day",
        rLimit: 5373485.0,
        rXform: 86400.0,
    },
    XformType {
        nName: 5,
        zName: b"month",
        rLimit: 176546.0,
        rXform: 2592000.0,
    },
    XformType {
        nName: 4,
        zName: b"year",
        rLimit: 14713.0,
        rXform: 31536000.0,
    },
];

/// If the DateTime p is raw number, try to figure out if it is a julian day
/// number of a unix timestamp.  Set the p value appropriately.
fn autoAdjustDate(p: &mut DateTime) {
    if p.rawS == 0 || p.validJD != 0 {
        p.rawS = 0;
    } else if p.s >= (-21086676i64 * 10000) as f64 // -4713-11-24 12:00:00
        && p.s <= ((25340230i64 * 10000) + 799) as f64
    // 9999-12-31 23:59:59
    {
        let r = p.s * 1000.0 + 210866760000000.0;
        clearYMD_HMS_TZ(p);
        p.iJD = (r + 0.5) as i64;
        p.validJD = 1;
        p.rawS = 0;
    }
}

/// Process a modifier to a date-time stamp.  Return 0 on success and 1 if
/// there is any kind of error.  If the error is in a system call (i.e.
/// localtime()), then an error message is written to context pCtx.  If the
/// error is an unrecognized modifier, no error is written to pCtx.
///
/// `z` is the (zero-terminated) text of the modifier; `idx` is the
/// parameter index of the modifier.
fn parseModifier(pCtx: &mut Context, z: &[u8], n: i32, p: &mut DateTime, idx: i32) -> i32 {
    let _ = n;
    let mut rc = 1;
    let mut r = 0.0f64;
    match sqlite3UpperToLower[at(z, 0) as usize] {
        b'a' => {
            // auto: If rawS is available, then interpret as a julian day
            // number, or a unix timestamp, depending on its magnitude.
            if sqlite3_stricmp(Some(z), Some(b"auto")) == 0 {
                if idx > 1 {
                    return 1; // IMP: R-33611-57934
                }
                autoAdjustDate(p);
                rc = 0;
            }
        }
        b'c' => {
            // ceiling: Resolve day-of-month overflow by rolling forward into
            // the next month.
            if sqlite3_stricmp(Some(z), Some(b"ceiling")) == 0 {
                computeJD(p);
                clearYMD_HMS_TZ(p);
                rc = 0;
                p.nFloor = 0;
            }
        }
        b'e' => {
            // end of day / end of month / end of year
            if sqlite3_strnicmp(Some(z), Some(b"end of "), 7) != 0 {
                return rc;
            }
            if p.validJD == 0 && p.validYMD == 0 && p.validHMS == 0 {
                return rc;
            }
            let z = from(z, 7);
            computeYMD(p);
            p.validHMS = 1;
            p.h = 23;
            p.m = 59;
            p.s = 59.999;
            p.rawS = 0;
            p.tz = 0;
            p.validJD = 0;
            if sqlite3_stricmp(Some(z), Some(b"month")) == 0 {
                p.D = 1;
                p.M += 1;
                if p.M > 12 {
                    p.Y += 1;
                    p.M = 1;
                }
                computeFloor(p);
                computeJD(p);
                p.iJD -= (p.nFloor as i64 + 1) * 86400000;
                clearYMD_HMS_TZ(p);
                rc = 0;
            } else if sqlite3_stricmp(Some(z), Some(b"year")) == 0 {
                p.M = 12;
                p.D = 31;
                rc = 0;
            } else if sqlite3_stricmp(Some(z), Some(b"day")) == 0 {
                rc = 0;
            }
        }
        b'f' => {
            // floor: Resolve day-of-month overflow by rolling back to the
            // end of the previous month.
            if sqlite3_stricmp(Some(z), Some(b"floor")) == 0 {
                computeJD(p);
                p.iJD -= p.nFloor as i64 * 86400000;
                clearYMD_HMS_TZ(p);
                rc = 0;
            }
        }
        b'j' => {
            // julianday: Always interpret the prior number as a julian-day
            // value.
            if sqlite3_stricmp(Some(z), Some(b"julianday")) == 0 {
                if idx > 1 {
                    return 1; // IMP: R-31176-64601
                }
                if p.validJD != 0 && p.rawS != 0 {
                    rc = 0;
                    p.rawS = 0;
                }
            }
        }
        b'l' => {
            // localtime: Assuming the current time value is UTC (a.k.a.
            // GMT), shift it to show local time.
            if sqlite3_stricmp(Some(z), Some(b"localtime")) == 0
                && crate::vdbeaux::sqlite3NotPureFunc(pCtx) != 0
            {
                rc = if p.isLocal != 0 {
                    SQLITE_OK
                } else {
                    toLocaltime(p, pCtx)
                };
                p.isUtc = 0;
                p.isLocal = 1;
            }
        }
        b'u' => {
            // unixepoch: Treat the current value of p->s as the number of
            // seconds since 1970.  Convert to a real julian day number.
            if sqlite3_stricmp(Some(z), Some(b"unixepoch")) == 0 && p.rawS != 0 {
                if idx > 1 {
                    return 1; // IMP: R-49255-55373
                }
                r = p.s * 1000.0 + 210866760000000.0;
                if r >= 0.0 && r < 464269060800000.0 {
                    clearYMD_HMS_TZ(p);
                    p.iJD = (r + 0.5) as i64;
                    p.validJD = 1;
                    p.rawS = 0;
                    rc = 0;
                }
            } else if sqlite3_stricmp(Some(z), Some(b"utc")) == 0
                && crate::vdbeaux::sqlite3NotPureFunc(pCtx) != 0
            {
                if p.isUtc == 0 {
                    let mut cnt = 0; // Safety to prevent infinite loop
                    computeJD(p);
                    let iOrigJD = p.iJD; // Original localtime
                    let mut iGuess = iOrigJD; // Guess at the corresponding utc time
                    let mut iErr: i64 = 0; // Guess is off by this much
                    loop {
                        let mut new = DateTime::default();
                        iGuess -= iErr;
                        new.iJD = iGuess;
                        new.validJD = 1;
                        rc = toLocaltime(&mut new, pCtx);
                        if rc != 0 {
                            return rc;
                        }
                        computeJD(&mut new);
                        iErr = new.iJD - iOrigJD;
                        let go = iErr != 0 && cnt < 3;
                        cnt += 1;
                        if !go {
                            break;
                        }
                    }
                    *p = DateTime::default();
                    p.iJD = iGuess;
                    p.validJD = 1;
                    p.isUtc = 1;
                    p.isLocal = 0;
                }
                rc = SQLITE_OK;
            }
        }
        b'w' => {
            // weekday N: Move the date forward (for N>=0) or backward (for
            // N<=-0) to the same time on the next/previous occurrence of
            // weekday abs(N) where 0==Sunday, 1==Monday, and so forth.
            let mut nn: i32;
            if sqlite3_strnicmp(Some(z), Some(b"weekday "), 8) == 0
                && sqlite3AtoF(Some(from(z, 8)), &mut r) > 0
                && r >= -6.0
                && r <= 6.0
                && {
                    nn = r as i32;
                    crate::vdbemem::sqlite3RealSameAsInt(r, nn as i64) != 0
                }
            {
                computeYMD_HMS(p);
                p.tz = 0;
                p.validJD = 0;
                computeJD(p);
                let mut Z = ((p.iJD + 129600000) / 86400000) % 7;
                if nn < 0 {
                    nn = -nn;
                }
                if Z != nn as i64 {
                    if Z > nn as i64 {
                        Z -= 7;
                    }
                    p.iJD += (nn as i64 - Z) * 86400000;
                    let rest = from(z, 8);
                    let rest = &rest[..rest.iter().position(|&c| c == 0).unwrap_or(rest.len())];
                    if rest.contains(&b'-') {
                        p.iJD -= 7 * 86400000;
                    }
                }
                clearYMD_HMS_TZ(p);
                rc = 0;
            }
        }
        b's' => {
            // start of day / start of month / start of year
            // subsecond / subsec
            if sqlite3_strnicmp(Some(z), Some(b"start of "), 9) != 0 {
                if sqlite3_stricmp(Some(z), Some(b"subsec")) == 0
                    || sqlite3_stricmp(Some(z), Some(b"subsecond")) == 0
                {
                    p.useSubsec = 1;
                    rc = 0;
                }
                return rc;
            }
            if p.validJD == 0 && p.validYMD == 0 && p.validHMS == 0 {
                return rc;
            }
            let z = from(z, 9);
            computeYMD(p);
            p.validHMS = 1;
            p.h = 0;
            p.m = 0;
            p.s = 0.0;
            p.rawS = 0;
            p.tz = 0;
            p.validJD = 0;
            if sqlite3_stricmp(Some(z), Some(b"month")) == 0 {
                p.D = 1;
                rc = 0;
            } else if sqlite3_stricmp(Some(z), Some(b"year")) == 0 {
                p.M = 1;
                p.D = 1;
                rc = 0;
            } else if sqlite3_stricmp(Some(z), Some(b"day")) == 0 {
                rc = 0;
            }
        }
        b'+' | b'-' | b'0'..=b'9' => {
            return parseModifierNumeric(z, p);
        }
        _ => {}
    }
    rc
}

/// The '+', '-' and digit cases of parseModifier().
fn parseModifierNumeric(z: &[u8], p: &mut DateTime) -> i32 {
    let mut rc = 1;
    let mut r = 0.0f64;
    let mut zo = 0usize; // C `z` (offset into the modifier)
    let mut z2 = 0usize; // C `z2`
    let z0 = at(z, 0);
    let mut n: usize = 1;
    let mut Y = [0i32; 3];
    while at(z, n) != 0 {
        if z[n] == b':' {
            break;
        }
        if sqlite3Isspace(z[n]) {
            break;
        }
        if z[n] == b'-' {
            if n == 5 && getDigits(from(z, 1), b"40f", &mut Y) == 1 {
                break;
            }
            if n == 6 && getDigits(from(z, 1), b"50f", &mut Y) == 1 {
                break;
            }
        }
        n += 1;
    }
    let zCopy = z[..n.min(z.len())].to_vec();
    let rx = sqlite3AtoF(Some(&zCopy), &mut r) <= 0;
    if rx {
        debug_assert!(rc == 1);
        return rc;
    }
    if at(z, n) == b'-' {
        // A modifier of the form (+|-)YYYY-MM-DD adds or subtracts the
        // specified number of years, months, and days.  MM is limited to the
        // range 0-11 and DD is limited to 0-30.
        if z0 != b'+' && z0 != b'-' {
            return rc; // Must start with +/-
        }
        let mut v = [0i32; 3];
        if n == 5 {
            if getDigits(from(z, 1), b"40f-20a-20d", &mut v) != 3 {
                return rc;
            }
        } else {
            debug_assert!(n == 6);
            if getDigits(from(z, 1), b"50f-20a-20d", &mut v) != 3 {
                return rc;
            }
            zo += 1;
        }
        let (y, M, mut D) = (v[0], v[1], v[2]);
        if M >= 12 {
            return rc; // M range 0..11
        }
        if D >= 31 {
            return rc; // D range 0..30
        }
        computeYMD_HMS(p);
        p.validJD = 0;
        if z0 == b'-' {
            p.Y -= y;
            p.M -= M;
            D = -D;
        } else {
            p.Y += y;
            p.M += M;
        }
        let x = if p.M > 0 {
            (p.M - 1) / 12
        } else {
            (p.M - 12) / 12
        };
        p.Y += x;
        p.M -= x * 12;
        computeFloor(p);
        computeJD(p);
        p.validHMS = 0;
        p.validYMD = 0;
        p.iJD += D as i64 * 86400000;
        if at(z, zo + 11) == 0 {
            return 0;
        }
        let mut hm = [0i32; 2];
        if sqlite3Isspace(at(z, zo + 11)) && getDigits(from(z, zo + 12), b"20c:20e", &mut hm) == 2 {
            z2 = zo + 12;
            n = 2;
        } else {
            return rc;
        }
    }
    if at(z, z2 + n) == b':' {
        // A modifier of the form (+|-)HH:MM:SS.FFF adds (or subtracts) the
        // specified number of hours, minutes, seconds, and fractional
        // seconds to the time.  The ".FFF" may be omitted.  The ":SS.FFF"
        // may be omitted.
        if !sqlite3Isdigit(at(z, z2)) {
            z2 += 1;
        }
        let mut tx = DateTime::default();
        if parseHhMmSs(from(z, z2), &mut tx) != 0 {
            return rc;
        }
        computeJD(&mut tx);
        tx.iJD -= 43200000;
        let day = tx.iJD / 86400000;
        tx.iJD -= day * 86400000;
        if z0 == b'-' {
            tx.iJD = -tx.iJD;
        }
        computeJD(p);
        clearYMD_HMS_TZ(p);
        p.iJD += tx.iJD;
        return 0;
    }

    // If control reaches this point, it means the transformation is one of
    // the forms like "+NNN days".
    zo += n;
    while sqlite3Isspace(at(z, zo)) {
        zo += 1;
    }
    let zr = from(z, zo);
    let mut n = sqlite3Strlen30(Some(zr)) as usize;
    if !(3..=10).contains(&n) {
        return rc;
    }
    if sqlite3UpperToLower[zr[n - 1] as usize] == b's' {
        n -= 1;
    }
    computeJD(p);
    debug_assert!(rc == 1);
    let rRounder = if r < 0.0 { -0.5 } else { 0.5 };
    p.nFloor = 0;
    for (i, xf) in aXformType.iter().enumerate() {
        if xf.nName as usize == n
            && sqlite3_strnicmp(Some(xf.zName), Some(zr), n as i32) == 0
            && r > -(xf.rLimit as f64)
            && r < xf.rLimit as f64
        {
            match i {
                4 => {
                    // Special processing to add months
                    computeYMD_HMS(p);
                    p.M += r as i32;
                    let x = if p.M > 0 {
                        (p.M - 1) / 12
                    } else {
                        (p.M - 12) / 12
                    };
                    p.Y += x;
                    p.M -= x * 12;
                    computeFloor(p);
                    p.validJD = 0;
                    r -= (r as i32) as f64;
                }
                5 => {
                    // Special processing to add years
                    let y = r as i32;
                    computeYMD_HMS(p);
                    debug_assert!(p.M >= 0 && p.M <= 12);
                    p.Y += y;
                    computeFloor(p);
                    p.validJD = 0;
                    r -= (r as i32) as f64;
                }
                _ => {}
            }
            computeJD(p);
            p.iJD += (r * 1000.0 * xf.rXform as f64 + rRounder) as i64;
            rc = 0;
            break;
        }
    }
    clearYMD_HMS_TZ(p);
    rc
}

/// Process time function arguments.  argv[0] is a date-time stamp.
/// argv[1] and following are modifiers.  Parse them all and write the
/// resulting time into the DateTime structure p.  Return 0 on success and
/// 1 if there are any errors.  If there are zero parameters then assume a
/// default value of "now" for argv[0].
fn isDate(context: &mut Context, argv: &mut [Mem], p: &mut DateTime) -> i32 {
    let argc = argv.len();
    *p = DateTime::default();
    if argc == 0 {
        if crate::vdbeaux::sqlite3NotPureFunc(context) == 0 {
            return 1;
        }
        return setDateTimeToCurrent(context, p);
    }
    let eType = sqlite3_value_type(&mut argv[0]);
    if eType == SQLITE_FLOAT || eType == SQLITE_INTEGER {
        let r = sqlite3_value_double(&mut argv[0]);
        setRawDateNumber(p, r);
    } else {
        let z = sqlite3_value_text(&mut argv[0]).map(|s| s.to_vec());
        match z {
            None => return 1,
            Some(z) => {
                if parseDateOrTime(context, &z, p) != 0 {
                    return 1;
                }
            }
        }
    }
    for i in 1..argc {
        let z = sqlite3_value_text(&mut argv[i]).map(|s| s.to_vec());
        let n = sqlite3_value_bytes(&mut argv[i]);
        match z {
            None => return 1,
            Some(z) => {
                if parseModifier(context, &z, n, p, i as i32) != 0 {
                    return 1;
                }
            }
        }
    }
    computeJD(p);
    if p.isError != 0 || !validJulianDay(p.iJD) {
        return 1;
    }
    if argc == 1 && p.validYMD != 0 && p.D > 28 {
        // Make sure a YYYY-MM-DD is normalized.
        // Example: 2023-02-31 -> 2023-03-03
        debug_assert!(p.validJD != 0);
        p.validYMD = 0;
    }
    0
}

/// julianday( TIMESTRING, MOD, MOD, ...)
///
/// Return the julian day number of the date specified in the arguments
fn juliandayFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut x = DateTime::default();
    if isDate(context, argv, &mut x) == 0 {
        computeJD(&mut x);
        sqlite3_result_double(context, x.iJD as f64 / 86400000.0);
    }
}

/// unixepoch( TIMESTRING, MOD, MOD, ...)
///
/// Return the number of seconds (including fractional seconds) since the
/// unix epoch of 1970-01-01 00:00:00 GMT.
fn unixepochFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut x = DateTime::default();
    if isDate(context, argv, &mut x) == 0 {
        computeJD(&mut x);
        if x.useSubsec != 0 {
            sqlite3_result_double(context, (x.iJD - 21086676i64 * 10000000) as f64 / 1000.0);
        } else {
            sqlite3_result_int64(context, x.iJD / 1000 - 21086676i64 * 10000);
        }
    }
}

#[inline]
fn dg(v: i32) -> u8 {
    b'0' + (v % 10) as u8
}

/// datetime( TIMESTRING, MOD, MOD, ...)
///
/// Return YYYY-MM-DD HH:MM:SS
fn datetimeFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut x = DateTime::default();
    if isDate(context, argv, &mut x) == 0 {
        let mut zBuf = [0u8; 32];
        computeYMD_HMS(&mut x);
        let mut Y = x.Y;
        if Y < 0 {
            Y = -Y;
        }
        zBuf[1] = dg(Y / 1000);
        zBuf[2] = dg(Y / 100);
        zBuf[3] = dg(Y / 10);
        zBuf[4] = dg(Y);
        zBuf[5] = b'-';
        zBuf[6] = dg(x.M / 10);
        zBuf[7] = dg(x.M);
        zBuf[8] = b'-';
        zBuf[9] = dg(x.D / 10);
        zBuf[10] = dg(x.D);
        zBuf[11] = b' ';
        zBuf[12] = dg(x.h / 10);
        zBuf[13] = dg(x.h);
        zBuf[14] = b':';
        zBuf[15] = dg(x.m / 10);
        zBuf[16] = dg(x.m);
        zBuf[17] = b':';
        let n: usize;
        if x.useSubsec != 0 {
            let s = (1000.0 * x.s + 0.5) as i32;
            zBuf[18] = dg(s / 10000);
            zBuf[19] = dg(s / 1000);
            zBuf[20] = b'.';
            zBuf[21] = dg(s / 100);
            zBuf[22] = dg(s / 10);
            zBuf[23] = dg(s);
            zBuf[24] = 0;
            n = 24;
        } else {
            let s = x.s as i32;
            zBuf[18] = dg(s / 10);
            zBuf[19] = dg(s);
            zBuf[20] = 0;
            n = 20;
        }
        if x.Y < 0 {
            zBuf[0] = b'-';
            sqlite3_result_text(context, Some(&zBuf[..n]), n as i32);
        } else {
            sqlite3_result_text(context, Some(&zBuf[1..n]), n as i32 - 1);
        }
    }
}

/// time( TIMESTRING, MOD, MOD, ...)
///
/// Return HH:MM:SS
fn timeFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut x = DateTime::default();
    if isDate(context, argv, &mut x) == 0 {
        let mut zBuf = [0u8; 16];
        computeHMS(&mut x);
        zBuf[0] = dg(x.h / 10);
        zBuf[1] = dg(x.h);
        zBuf[2] = b':';
        zBuf[3] = dg(x.m / 10);
        zBuf[4] = dg(x.m);
        zBuf[5] = b':';
        let n: usize;
        if x.useSubsec != 0 {
            let s = (1000.0 * x.s + 0.5) as i32;
            zBuf[6] = dg(s / 10000);
            zBuf[7] = dg(s / 1000);
            zBuf[8] = b'.';
            zBuf[9] = dg(s / 100);
            zBuf[10] = dg(s / 10);
            zBuf[11] = dg(s);
            zBuf[12] = 0;
            n = 12;
        } else {
            let s = x.s as i32;
            zBuf[6] = dg(s / 10);
            zBuf[7] = dg(s);
            zBuf[8] = 0;
            n = 8;
        }
        sqlite3_result_text(context, Some(&zBuf[..n]), n as i32);
    }
}

/// date( TIMESTRING, MOD, MOD, ...)
///
/// Return YYYY-MM-DD
fn dateFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut x = DateTime::default();
    if isDate(context, argv, &mut x) == 0 {
        let mut zBuf = [0u8; 16];
        computeYMD(&mut x);
        let mut Y = x.Y;
        if Y < 0 {
            Y = -Y;
        }
        zBuf[1] = dg(Y / 1000);
        zBuf[2] = dg(Y / 100);
        zBuf[3] = dg(Y / 10);
        zBuf[4] = dg(Y);
        zBuf[5] = b'-';
        zBuf[6] = dg(x.M / 10);
        zBuf[7] = dg(x.M);
        zBuf[8] = b'-';
        zBuf[9] = dg(x.D / 10);
        zBuf[10] = dg(x.D);
        zBuf[11] = 0;
        if x.Y < 0 {
            zBuf[0] = b'-';
            sqlite3_result_text(context, Some(&zBuf[..11]), 11);
        } else {
            sqlite3_result_text(context, Some(&zBuf[1..11]), 10);
        }
    }
}

/// Compute the number of days after the most recent January 1 (Jan01 = 0,
/// ..., Dec31 = 364 or 365).
fn daysAfterJan01(pDate: &DateTime) -> i32 {
    let mut jan01 = *pDate;
    debug_assert!(jan01.validYMD != 0);
    debug_assert!(jan01.validHMS != 0);
    debug_assert!(pDate.validJD != 0);
    jan01.validJD = 0;
    jan01.M = 1;
    jan01.D = 1;
    computeJD(&mut jan01);
    ((pDate.iJD - jan01.iJD + 43200000) / 86400000) as i32
}

/// Return the number of days after the most recent Monday (0=Monday, ...,
/// 6=Sunday).
fn daysAfterMonday(pDate: &DateTime) -> i32 {
    debug_assert!(pDate.validJD != 0);
    (((pDate.iJD + 43200000) / 86400000) % 7) as i32
}

/// Return the number of days after the most recent Sunday (0=Sunday, ...,
/// 6=Saturday).
fn daysAfterSunday(pDate: &DateTime) -> i32 {
    debug_assert!(pDate.validJD != 0);
    (((pDate.iJD + 129600000) / 86400000) % 7) as i32
}

/// strftime( FORMAT, TIMESTRING, MOD, MOD, ...)
///
/// Return a string described by FORMAT (see the C documentation of the
/// conversions %d %e %f %F %G %g %H %k %I %j %J %l %m %M %p %P %R %s %S %T
/// %u %w %U %V %W %Y %%).
fn strftimeFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let mut x = DateTime::default();
    if argc == 0 {
        return;
    }
    let Some(zFmt) = sqlite3_value_text(&mut argv[0]).map(|s| s.to_vec()) else {
        return;
    };
    if isDate(context, &mut argv[1..], &mut x) != 0 {
        return;
    }
    let mut pRes = sqlite3_str_new(Some(&*context.db));

    computeJD(&mut x);
    computeYMD_HMS(&mut x);
    let I = |v: i32| PfArg::Int(v as i64);
    let mut i = 0usize;
    let mut j = 0usize;
    while at(&zFmt, i) != 0 {
        if zFmt[i] != b'%' {
            i += 1;
            continue;
        }
        if j < i {
            sqlite3_str_append(&mut pRes, &zFmt[j..i], (i - j) as i32);
        }
        i += 1;
        j = i + 1;
        let cf = at(&zFmt, i);
        match cf {
            b'd' | b'e' => {
                let f: &[u8] = if cf == b'd' { b"%02d" } else { b"%2d" };
                sqlite3_str_appendf(&mut pRes, f, &[I(x.D)]);
            }
            b'f' => {
                // Fractional seconds.  (Non-standard)
                let mut s = x.s;
                if s > 59.999 {
                    s = 59.999;
                }
                sqlite3_str_appendf(&mut pRes, b"%06.3f", &[PfArg::Dbl(s)]);
            }
            b'F' => {
                sqlite3_str_appendf(&mut pRes, b"%04d-%02d-%02d", &[I(x.Y), I(x.M), I(x.D)]);
            }
            b'G' | b'g' => {
                let mut y = x;
                debug_assert!(y.validJD != 0);
                // Move y so that it is the Thursday in the same week as x
                y.iJD += (3 - daysAfterMonday(&x)) as i64 * 86400000;
                y.validYMD = 0;
                computeYMD(&mut y);
                if cf == b'g' {
                    sqlite3_str_appendf(&mut pRes, b"%02d", &[I(y.Y % 100)]);
                } else {
                    sqlite3_str_appendf(&mut pRes, b"%04d", &[I(y.Y)]);
                }
            }
            b'H' | b'k' => {
                let f: &[u8] = if cf == b'H' { b"%02d" } else { b"%2d" };
                sqlite3_str_appendf(&mut pRes, f, &[I(x.h)]);
            }
            b'I' | b'l' => {
                let mut h = x.h;
                if h > 12 {
                    h -= 12;
                }
                if h == 0 {
                    h = 12;
                }
                let f: &[u8] = if cf == b'I' { b"%02d" } else { b"%2d" };
                sqlite3_str_appendf(&mut pRes, f, &[I(h)]);
            }
            b'j' => {
                // Day of year.  Jan01==1, Jan02==2, and so forth
                sqlite3_str_appendf(&mut pRes, b"%03d", &[I(daysAfterJan01(&x) + 1)]);
            }
            b'J' => {
                // Julian day number.  (Non-standard)
                sqlite3_str_appendf(
                    &mut pRes,
                    b"%.16g",
                    &[PfArg::Dbl(x.iJD as f64 / 86400000.0)],
                );
            }
            b'm' => {
                sqlite3_str_appendf(&mut pRes, b"%02d", &[I(x.M)]);
            }
            b'M' => {
                sqlite3_str_appendf(&mut pRes, b"%02d", &[I(x.m)]);
            }
            b'p' | b'P' => {
                if x.h >= 12 {
                    sqlite3_str_append(&mut pRes, if cf == b'p' { b"PM" } else { b"pm" }, 2);
                } else {
                    sqlite3_str_append(&mut pRes, if cf == b'p' { b"AM" } else { b"am" }, 2);
                }
            }
            b'R' => {
                sqlite3_str_appendf(&mut pRes, b"%02d:%02d", &[I(x.h), I(x.m)]);
            }
            b's' => {
                if x.useSubsec != 0 {
                    sqlite3_str_appendf(
                        &mut pRes,
                        b"%.3f",
                        &[PfArg::Dbl((x.iJD - 21086676i64 * 10000000) as f64 / 1000.0)],
                    );
                } else {
                    let iS = x.iJD / 1000 - 21086676i64 * 10000;
                    sqlite3_str_appendf(&mut pRes, b"%lld", &[PfArg::Int(iS)]);
                }
            }
            b'S' => {
                sqlite3_str_appendf(&mut pRes, b"%02d", &[I(x.s as i32)]);
            }
            b'T' => {
                sqlite3_str_appendf(
                    &mut pRes,
                    b"%02d:%02d:%02d",
                    &[I(x.h), I(x.m), I(x.s as i32)],
                );
            }
            b'u' | b'w' => {
                // Day of week.  %u: 1 to 7, Monday==1, Sunday==7.  %w: 0 to
                // 6, Sunday==0, Monday==1
                let mut c = daysAfterSunday(&x) as u8 + b'0';
                if c == b'0' && cf == b'u' {
                    c = b'7';
                }
                sqlite3_str_appendchar(&mut pRes, 1, c);
            }
            b'U' => {
                // Week num. 00-53. First Sun of the year is week 01
                sqlite3_str_appendf(
                    &mut pRes,
                    b"%02d",
                    &[I((daysAfterJan01(&x) - daysAfterSunday(&x) + 7) / 7)],
                );
            }
            b'V' => {
                // Week num. 01-53. First week with a Thur is week 01
                let mut y = x;
                // Adjust y so that is the Thursday in the same week as x
                debug_assert!(y.validJD != 0);
                y.iJD += (3 - daysAfterMonday(&x)) as i64 * 86400000;
                y.validYMD = 0;
                computeYMD(&mut y);
                sqlite3_str_appendf(&mut pRes, b"%02d", &[I(daysAfterJan01(&y) / 7 + 1)]);
            }
            b'W' => {
                // Week num. 00-53. First Mon of the year is week 01
                sqlite3_str_appendf(
                    &mut pRes,
                    b"%02d",
                    &[I((daysAfterJan01(&x) - daysAfterMonday(&x) + 7) / 7)],
                );
            }
            b'Y' => {
                sqlite3_str_appendf(&mut pRes, b"%04d", &[I(x.Y)]);
            }
            b'%' => {
                sqlite3_str_appendchar(&mut pRes, 1, b'%');
            }
            _ => {
                crate::printf::sqlite3_str_free(&mut pRes);
                return;
            }
        }
        i += 1;
    }
    if j < i {
        sqlite3_str_append(&mut pRes, &zFmt[j..i], (i - j) as i32);
    }
    sqlite3_result_str(context, &mut pRes, SQLITE_FINISH);
}

/// current_time(): This function returns the same value as time('now').
fn ctimeFunc(context: &mut Context, _argv: &mut [Mem]) {
    timeFunc(context, &mut []);
}

/// current_date(): This function returns the same value as date('now').
fn cdateFunc(context: &mut Context, _argv: &mut [Mem]) {
    dateFunc(context, &mut []);
}

/// timediff(DATE1, DATE2)
///
/// Return the amount of time that must be added to DATE2 in order to
/// convert it into DATE2.  The time difference format is
/// +YYYY-MM-DD HH:MM:SS.SSS (the initial "+" becomes "-" if DATE1 occurs
/// before DATE2).
fn timediffFunc(context: &mut Context, argv: &mut [Mem]) {
    let sign: u8;
    let mut Y: i32;
    let mut M: i32;
    let mut d1 = DateTime::default();
    let mut d2 = DateTime::default();
    if isDate(context, &mut argv[0..1], &mut d1) != 0 {
        return;
    }
    if isDate(context, &mut argv[1..2], &mut d2) != 0 {
        return;
    }
    computeYMD_HMS(&mut d1);
    computeYMD_HMS(&mut d2);
    if d1.iJD >= d2.iJD {
        sign = b'+';
        Y = d1.Y - d2.Y;
        if Y != 0 {
            d2.Y = d1.Y;
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        M = d1.M - d2.M;
        if M < 0 {
            Y -= 1;
            M += 12;
        }
        if M != 0 {
            d2.M = d1.M;
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        while d1.iJD < d2.iJD {
            M -= 1;
            if M < 0 {
                M = 11;
                Y -= 1;
            }
            d2.M -= 1;
            if d2.M < 1 {
                d2.M = 12;
                d2.Y -= 1;
            }
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        d1.iJD -= d2.iJD;
        d1.iJD += 1486995408i64 * 100000;
    } else {
        // d1<d2
        sign = b'-';
        Y = d2.Y - d1.Y;
        if Y != 0 {
            d2.Y = d1.Y;
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        M = d2.M - d1.M;
        if M < 0 {
            Y -= 1;
            M += 12;
        }
        if M != 0 {
            d2.M = d1.M;
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        while d1.iJD > d2.iJD {
            M -= 1;
            if M < 0 {
                M = 11;
                Y -= 1;
            }
            d2.M += 1;
            if d2.M > 12 {
                d2.M = 1;
                d2.Y += 1;
            }
            d2.validJD = 0;
            computeJD(&mut d2);
        }
        d1.iJD = d2.iJD - d1.iJD;
        d1.iJD += 1486995408i64 * 100000;
    }
    clearYMD_HMS_TZ(&mut d1);
    computeYMD_HMS(&mut d1);
    let mut sRes = StrAccum::default();
    sqlite3StrAccumInit(&mut sRes, 0, 100);
    sqlite3_str_appendf(
        &mut sRes,
        b"%c%04d-%02d-%02d %02d:%02d:%06.3f",
        &[
            PfArg::Int(sign as i64),
            PfArg::Int(Y as i64),
            PfArg::Int(M as i64),
            PfArg::Int(d1.D as i64 - 1),
            PfArg::Int(d1.h as i64),
            PfArg::Int(d1.m as i64),
            PfArg::Dbl(d1.s),
        ],
    );
    sqlite3_result_str(context, &mut sRes, SQLITE_XFER);
}

/// current_timestamp(): This function returns the same value as
/// datetime('now').
fn ctimestampFunc(context: &mut Context, _argv: &mut [Mem]) {
    datetimeFunc(context, &mut []);
}

/// PURE_DATE(zName, nArg, iArg, bNC, xFunc)
///
/// Rust: the C user data (&sqlite3Config) is never read; it is
/// FuncUserData::None here.
fn PURE_DATE(zName: &str, nArg: i32, xFunc: fn(&mut Context, &mut [Mem])) -> FuncDef {
    FuncDef {
        nArg: nArg as i16,
        funcFlags: SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_SLOCHNG
            | SQLITE_UTF8 as u32
            | SQLITE_FUNC_CONSTANT,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(xFunc) as XSFunc),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: zName.as_bytes().to_vec(),
        pDestructor: None,
    }
}

/// The FuncDef objects of the C aDateTimeFuncs[] array, in array order.
pub fn sqlite3DateTimeFunctionDefs() -> Vec<FuncDef> {
    vec![
        PURE_DATE("julianday", -1, juliandayFunc),
        PURE_DATE("unixepoch", -1, unixepochFunc),
        PURE_DATE("date", -1, dateFunc),
        PURE_DATE("time", -1, timeFunc),
        PURE_DATE("datetime", -1, datetimeFunc),
        PURE_DATE("strftime", -1, strftimeFunc),
        PURE_DATE("timediff", 2, timediffFunc),
        DFUNCTION("current_time", 0, 0, 0, ctimeFunc),
        DFUNCTION("current_timestamp", 0, 0, 0, ctimestampFunc),
        DFUNCTION("current_date", 0, 0, 0, cdateFunc),
    ]
}

/// C: `void sqlite3RegisterDateTimeFunctions(void)`
///
/// This function registered all of the above C functions as SQL functions.
///
/// Rust: the skeleton's sqlite3InsertBuiltinFuncs() takes one FuncDef at a
/// time, so each definition is inserted with nDef==1, in array order.
pub fn sqlite3RegisterDateTimeFunctions() {
    for def in sqlite3DateTimeFunctionDefs() {
        crate::callback::sqlite3InsertBuiltinFuncs(&Rc::new(def), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(s: &str) -> Option<DateTime> {
        let mut p = DateTime::default();
        if parseYyyyMmDd(s.as_bytes(), &mut p) == 0 {
            computeJD(&mut p);
            Some(p)
        } else {
            None
        }
    }

    #[test]
    fn parse_and_compute() {
        let p = ymd("2000-01-01 12:00:00").unwrap();
        assert_eq!(p.iJD, 2451545 * 86400000);
        let p = ymd("1970-01-01").unwrap();
        assert_eq!(p.iJD / 1000 - 21086676i64 * 10000, 0);
        let mut p = ymd("2024-02-29 23:59:59.999").unwrap();
        p.validYMD = 0;
        p.validHMS = 0;
        computeYMD_HMS(&mut p);
        assert_eq!((p.Y, p.M, p.D, p.h, p.m), (2024, 2, 29, 23, 59));
        assert!((p.s - 59.999).abs() < 1e-9);
        assert!(ymd("2024-13-01").is_none());
        assert!(ymd("2024-01-01 25:00").is_none());
        let p = ymd("2024-01-01T10:00:00+02:00").unwrap();
        let q = ymd("2024-01-01 08:00:00").unwrap();
        assert_eq!(p.iJD, q.iJD);
        let mut v = [0i32; 3];
        assert_eq!(getDigits(b"2024-05-06", b"40f-21a-21d", &mut v), 3);
        assert_eq!(v, [2024, 5, 6]);
    }

    fn fmt(p: &mut DateTime) -> String {
        computeYMD_HMS(p);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            p.Y, p.M, p.D, p.h, p.m, p.s as i32
        )
    }

    #[test]
    fn numeric_modifiers() {
        // Expected values from the reference CLI.
        for (d, m, want) in [
            ("2024-01-31", "+1 month", "2024-03-02 00:00:00"),
            ("2024-01-31 10:00", "-1 day", "2024-01-30 10:00:00"),
            ("2024-01-31 10:00", "+01:30", "2024-01-31 11:30:00"),
            ("2024-01-31 10:00", "+0001-02-03", "2025-04-03 10:00:00"),
            (
                "2024-01-31 10:00",
                "+0001-02-03 04:05",
                "2025-04-03 14:05:00",
            ),
            ("2024-03-31", "-1 month", "2024-03-02 00:00:00"),
            ("2023-01-01", "+1.5 years", "2024-07-01 12:00:00"),
            ("2024-01-31 10:00", "-00:00:30.5", "2024-01-31 09:59:29"),
            ("2024-02-29", "+1 year", "2025-03-01 00:00:00"),
            ("2024-01-01 12:34:56", "+90 minutes", "2024-01-01 14:04:56"),
            ("2024-01-01", "+1e3 seconds", "2024-01-01 00:16:40"),
        ] {
            let mut p = ymd(d).unwrap();
            assert_eq!(parseModifierNumeric(m.as_bytes(), &mut p), 0, "{d} {m}");
            computeJD(&mut p);
            assert_eq!(fmt(&mut p), want, "{d} {m}");
        }
    }

    #[test]
    fn weekdays() {
        let p = ymd("2024-10-22").unwrap(); // a Tuesday
        assert_eq!(daysAfterSunday(&p), 2);
        assert_eq!(daysAfterMonday(&p), 1);
        let mut p = p;
        computeYMD_HMS(&mut p);
        assert_eq!(daysAfterJan01(&p), 295);
    }
}
