//! Port of func.c: the C-language implementations for many of the SQL
//! functions of SQLite.  (The date and time functions are in date.rs.)
//!
//! Built-in functions are `fn(&mut Context, &mut [Mem])` (xSFunc, xStep,
//! xInverse) and `fn(&mut Context)` (xFinal, xValue); argc is
//! `argv.len()`.  Function user data (C `sqlite3_user_data()`) is a
//! `FuncUserData`: `Int(n)` for SQLITE_INT_TO_PTR(n); the LIKE/GLOB
//! compareInfo is encoded as an integer (see `CompareInfo::to_int`) and
//! the libm function pointers of the math functions are `Any` values.
//!
//! Percentile/median (SQLITE_ENABLE_PERCENTILE) and the SQLITE_DEBUG-only
//! functions are not ported; soundex() is ported but not registered
//! (SQLITE_SOUNDEX is off, as in the default build).

use crate::global::{sqlite3Isalpha, sqlite3Isxdigit, sqlite3Tolower, sqlite3Toupper};
use crate::int::*;
use crate::printf::{
    PfArg, PfArgs, PrintfArguments, sqlite3_str_append, sqlite3_str_appendchar,
    sqlite3_str_appendf, sqlite3_str_vappendf, sqlite3StrAccumEnlarge, sqlite3StrAccumInit,
};
use crate::utf::{Utf8Read, sqlite3AppendOneUtf8Character, sqlite3Utf8CharLen, sqlite3Utf8Read};
use crate::util::{
    sqlite3AddInt64, sqlite3AtoF, sqlite3HexToInt, sqlite3IsOverflow, sqlite3SubInt64,
};
use crate::vdbe::sqlite3_value_numeric_type;
use crate::vdbeapi::*;
use std::rc::Rc;

/// Byte `i` of a zero-terminated string held in a slice (0 past the end).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// sqlite3_value_text() copied out of the value (so that several argument
/// values can be used at the same time).
#[inline]
fn value_text(m: &mut Mem) -> Option<Vec<u8>> {
    sqlite3_value_text(m).map(|s| s.to_vec())
}

#[inline]
fn value_blob(m: &mut Mem) -> Option<Vec<u8>> {
    sqlite3_value_blob(m).map(|s| s.to_vec())
}

/// Return the collating function associated with a function.
fn sqlite3GetFuncCollSeq(context: &mut Context) -> CollSeqRef {
    context.pColl.clone().expect("function without OP_CollSeq")
}

/// Indicate that the accumulator load should be skipped on this iteration
/// of the aggregate loop.
fn sqlite3SkipAccumulatorLoad(context: &mut Context) {
    debug_assert!(context.isError <= 0);
    context.isError = -1;
    context.skipFlag = 1;
}

/// Implementation of the non-aggregate min() and max() functions
fn minmaxFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    debug_assert!(argc > 1);
    let mask: i32 = if sqlite3_user_data(context).as_int() == 0 {
        0
    } else {
        -1
    };
    let pColl = sqlite3GetFuncCollSeq(context);
    let mut iBest = 0usize;
    if sqlite3_value_type(&mut argv[0]) == SQLITE_NULL {
        return;
    }
    for i in 1..argc {
        if sqlite3_value_type(&mut argv[i]) == SQLITE_NULL {
            return;
        }
        if (crate::vdbeaux::sqlite3MemCompare(context.db, &argv[iBest], &argv[i], Some(&pColl))
            ^ mask)
            >= 0
        {
            iBest = i;
        }
    }
    sqlite3_result_value(context, &mut argv[iBest]);
}

/// Return the type of the argument.
fn typeofFunc(context: &mut Context, argv: &mut [Mem]) {
    static azType: [&[u8]; 5] = [b"integer", b"real", b"text", b"blob", b"null"];
    let i = (sqlite3_value_type(&mut argv[0]) - 1) as usize;
    debug_assert!(i < azType.len());
    sqlite3_result_text(context, Some(azType[i]), -1);
}

/// subtype(X): Return the subtype of X
fn subtypeFunc(context: &mut Context, argv: &mut [Mem]) {
    let t = sqlite3_value_subtype(&mut argv[0]);
    sqlite3_result_int(context, t as i32);
}

/// Implementation of the length() function
fn lengthFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    match sqlite3_value_type(&mut argv[0]) {
        SQLITE_BLOB | SQLITE_INTEGER | SQLITE_FLOAT => {
            let n = sqlite3_value_bytes(&mut argv[0]);
            sqlite3_result_int(context, n);
        }
        SQLITE_TEXT => {
            let Some(z) = sqlite3_value_text(&mut argv[0]) else {
                return;
            };
            let mut i = 0usize;
            let mut z0 = 0usize;
            loop {
                let c = at(z, i);
                if c.wrapping_sub(1) < (0x80 - 1) {
                    i += 1;
                } else if c == 0 {
                    break;
                } else {
                    i += 1;
                    while (at(z, i) & 0xc0) == 0x80 {
                        i += 1;
                        z0 += 1;
                    }
                }
            }
            sqlite3_result_int(context, (i - z0) as i32);
        }
        _ => {
            sqlite3_result_null(context);
        }
    }
}

/// Implementation of the octet_length() function
fn bytelengthFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    match sqlite3_value_type(&mut argv[0]) {
        SQLITE_BLOB => {
            let n = sqlite3_value_bytes(&mut argv[0]);
            sqlite3_result_int(context, n);
        }
        SQLITE_INTEGER | SQLITE_FLOAT => {
            let m: i64 = if context.db.enc <= SQLITE_UTF8 { 1 } else { 2 };
            let n = sqlite3_value_bytes(&mut argv[0]) as i64;
            sqlite3_result_int64(context, n * m);
        }
        SQLITE_TEXT => {
            if sqlite3_value_encoding(&mut argv[0]) <= SQLITE_UTF8 as i32 {
                let n = sqlite3_value_bytes(&mut argv[0]);
                sqlite3_result_int(context, n);
            } else {
                let n = sqlite3_value_bytes16(&mut argv[0]);
                sqlite3_result_int(context, n);
            }
        }
        _ => {
            sqlite3_result_null(context);
        }
    }
}

/// Implementation of the abs() function.
fn absFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    match sqlite3_value_type(&mut argv[0]) {
        SQLITE_INTEGER => {
            let mut iVal = sqlite3_value_int64(&mut argv[0]);
            if iVal < 0 {
                if iVal == SMALLEST_INT64 {
                    // IMP: R-31676-45509 If X is the integer
                    // -9223372036854775808 then abs(X) throws an integer
                    // overflow error.
                    sqlite3_result_error(context, Some(b"integer overflow"), -1);
                    return;
                }
                iVal = -iVal;
            }
            sqlite3_result_int64(context, iVal);
        }
        SQLITE_NULL => {
            // IMP: R-37434-19929 Abs(X) returns NULL if X is NULL.
            sqlite3_result_null(context);
        }
        _ => {
            // Because sqlite3_value_double() returns 0.0 if the argument is
            // not something that can be converted into a number, we have:
            // IMP: R-01992-00519 Abs(X) returns 0.0 if X is a string or blob
            // that cannot be converted to a numeric value.
            let mut rVal = sqlite3_value_double(&mut argv[0]);
            if rVal < 0.0 {
                rVal = -rVal;
            }
            sqlite3_result_double(context, rVal);
        }
    }
}

/// Implementation of the instr() function.
///
/// instr(haystack,needle) finds the first occurrence of needle in haystack
/// and returns the number of previous characters plus 1, or 0 if needle
/// does not occur within haystack.  If both haystack and needle are BLOBs,
/// then the result is one more than the number of bytes in haystack prior
/// to the first occurrence of needle, or 0 if needle never occurs in
/// haystack.
fn instrFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut N: i32 = 1;
    let typeHaystack = sqlite3_value_type(&mut argv[0]);
    let typeNeedle = sqlite3_value_type(&mut argv[1]);
    if typeHaystack == SQLITE_NULL || typeNeedle == SQLITE_NULL {
        return;
    }
    let mut nHaystack = sqlite3_value_bytes(&mut argv[0]);
    let mut nNeedle = sqlite3_value_bytes(&mut argv[1]);
    if nNeedle > 0 {
        let zHaystack: Option<Vec<u8>>;
        let zNeedle: Option<Vec<u8>>;
        let isText: bool;
        if typeHaystack == SQLITE_BLOB && typeNeedle == SQLITE_BLOB {
            zHaystack = value_blob(&mut argv[0]);
            zNeedle = value_blob(&mut argv[1]);
            isText = false;
        } else if typeHaystack != SQLITE_BLOB && typeNeedle != SQLITE_BLOB {
            zHaystack = value_text(&mut argv[0]);
            zNeedle = value_text(&mut argv[1]);
            isText = true;
        } else {
            let Some(mut pC1) = sqlite3_value_dup(&argv[0]) else {
                sqlite3_result_error_nomem(context);
                return;
            };
            zHaystack = value_text(&mut pC1);
            if zHaystack.is_none() {
                sqlite3_result_error_nomem(context);
                return;
            }
            nHaystack = sqlite3_value_bytes(&mut pC1);
            let Some(mut pC2) = sqlite3_value_dup(&argv[1]) else {
                sqlite3_result_error_nomem(context);
                return;
            };
            zNeedle = value_text(&mut pC2);
            if zNeedle.is_none() {
                sqlite3_result_error_nomem(context);
                return;
            }
            nNeedle = sqlite3_value_bytes(&mut pC2);
            isText = true;
        }
        let (Some(zNeedle), zHaystack) = (zNeedle, zHaystack) else {
            sqlite3_result_error_nomem(context);
            return;
        };
        if nHaystack != 0 && zHaystack.is_none() {
            sqlite3_result_error_nomem(context);
            return;
        }
        let zHaystack = zHaystack.unwrap_or_default();
        let nN = nNeedle as usize;
        let firstChar = at(&zNeedle, 0);
        let mut h = 0usize;
        while nNeedle <= nHaystack
            && (at(&zHaystack, h) != firstChar || zHaystack[h..h + nN] != zNeedle[..nN])
        {
            N += 1;
            loop {
                nHaystack -= 1;
                h += 1;
                if !(isText && (at(&zHaystack, h) & 0xc0) == 0x80) {
                    break;
                }
            }
        }
        if nNeedle > nHaystack {
            N = 0;
        }
    }
    sqlite3_result_int(context, N);
}

/// Implementation of the printf() (a.k.a. format()) SQL function.
fn printfFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if argc >= 1 {
        let Some(zFormat) = value_text(&mut argv[0]) else {
            return;
        };
        let mut str = StrAccum::default();
        sqlite3StrAccumInit(&mut str, 0, context.db.aLimit[SQLITE_LIMIT_LENGTH as usize]);
        str.printfFlags = SQLITE_PRINTF_SQLFUNC;
        {
            let mut x = PrintfArguments {
                nArg: argc as i32 - 1,
                nUsed: 0,
                apArg: &mut argv[1..],
            };
            sqlite3_str_vappendf(&mut str, None, &zFormat, PfArgs::Sql(&mut x));
        }
        sqlite3_result_str(context, &mut str, SQLITE_XFER);
    }
}

/// Implementation of the substr() function.
///
/// substr(x,p1,p2)  returns p2 characters of x[] beginning with p1.  p1 is
/// 1-indexed.  If x is text, then we actually count UTF-8 characters.  If x
/// is a blob, then we count bytes.  If p1 is negative, then we begin
/// abs(p1) from the end of x[].  If p2 is negative, return the p2
/// characters preceding p1.
fn substrFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 3 || argc == 2);
    let p0type = sqlite3_value_type(&mut argv[0]);
    let mut p1 = sqlite3_value_int64(&mut argv[1]);
    let z: Vec<u8>;
    let mut len: i32;
    if p0type == SQLITE_BLOB {
        len = sqlite3_value_bytes(&mut argv[0]);
        match value_blob(&mut argv[0]) {
            Some(b) => z = b,
            None => return,
        }
    } else {
        match value_text(&mut argv[0]) {
            Some(t) => z = t,
            None => return,
        }
        len = 0;
        if p1 < 0 {
            let mut z2 = 0usize;
            while at(&z, z2) != 0 {
                crate::utf::SQLITE_SKIP_UTF8(&z, &mut z2);
                len += 1;
            }
        }
    }
    let mut p2: i64;
    if argc == 3 {
        p2 = sqlite3_value_int64(&mut argv[2]);
        if p2 == 0 && sqlite3_value_type(&mut argv[2]) == SQLITE_NULL {
            return;
        }
    } else {
        p2 = LARGEST_INT64;
    }
    if p1 == 0 && sqlite3_value_type(&mut argv[1]) == SQLITE_NULL {
        return;
    }
    if p1 < 0 {
        p1 += len as i64;
        if p1 < 0 {
            if p2 < 0 {
                p2 = 0;
            } else {
                p2 += p1;
            }
            p1 = 0;
        }
    } else if p1 > 0 {
        p1 -= 1;
    } else if p2 > 0 {
        p2 -= 1;
    }
    if p2 < 0 {
        if p2 < -p1 {
            p2 = p1;
        } else {
            p2 = -p2;
        }
        p1 -= p2;
    }
    debug_assert!(p1 >= 0 && p2 >= 0);
    if p0type != SQLITE_BLOB {
        let mut zi = 0usize;
        while p1 > 0 {
            let c = at(&z, zi);
            if c.wrapping_sub(1) < (0x80 - 1) {
                zi += 1;
            } else if c == 0 {
                break;
            } else {
                loop {
                    zi += 1;
                    if (at(&z, zi) & 0xc0) != 0x80 {
                        break;
                    }
                }
            }
            p1 -= 1;
        }
        let mut z2 = zi;
        while p2 > 0 {
            let c = at(&z, z2);
            if c.wrapping_sub(1) < (0x80 - 1) {
                z2 += 1;
            } else if c == 0 {
                break;
            } else {
                loop {
                    z2 += 1;
                    if (at(&z, z2) & 0xc0) != 0x80 {
                        break;
                    }
                }
            }
            p2 -= 1;
        }
        let z2 = z2.min(z.len());
        let zi = zi.min(z2);
        sqlite3_result_text64(context, Some(&z[zi..z2]), (z2 - zi) as u64, SQLITE_UTF8);
    } else {
        let len = len as i64;
        if p1 >= len {
            p1 = 0;
            p2 = 0;
        } else if p2 > len - p1 {
            p2 = len - p1;
            debug_assert!(p2 > 0);
        }
        let s = p1 as usize;
        let e = (p1 + p2) as usize;
        sqlite3_result_blob64(context, &z[s..e], p2 as u64);
    }
}

/// Implementation of the round() function
fn roundFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let mut n: i64 = 0;
    debug_assert!(argc == 1 || argc == 2);
    if argc == 2 {
        if SQLITE_NULL == sqlite3_value_type(&mut argv[1]) {
            return;
        }
        n = sqlite3_value_int64(&mut argv[1]);
        if n > 30 {
            n = 30;
        }
        if n < 0 {
            n = 0;
        }
    }
    if sqlite3_value_type(&mut argv[0]) == SQLITE_NULL {
        return;
    }
    let mut r = sqlite3_value_double(&mut argv[0]);
    // If Y==0 and X will fit in a 64-bit int, handle the rounding directly,
    // otherwise use printf.
    if r < -4503599627370496.0 || r > 4503599627370496.0 {
        // The value has no fractional part so there is nothing to round
    } else if n == 0 {
        r = ((r + if r < 0.0 { -0.5 } else { 0.5 }) as i64) as f64;
    } else {
        let zBuf = crate::printf::sqlite3_mprintf(b"%!.*f", &[PfArg::Int(n), PfArg::Dbl(r)]);
        sqlite3AtoF(Some(&zBuf), &mut r);
    }
    sqlite3_result_double(context, r);
}

/// Check that nByte bytes may be allocated (contextMalloc()).  If nByte is
/// larger than the maximum string or blob length, then raise an
/// SQLITE_TOOBIG exception and return false.
fn contextMalloc(context: &mut Context, nByte: i64) -> bool {
    debug_assert!(nByte > 0);
    if nByte > context.db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        sqlite3_result_error_toobig(context);
        false
    } else {
        true
    }
}

/// Implementation of the upper() SQL function.
fn upperFunc(context: &mut Context, argv: &mut [Mem]) {
    let z2 = value_text(&mut argv[0]);
    let n = sqlite3_value_bytes(&mut argv[0]);
    if let Some(z2) = z2 {
        if contextMalloc(context, n as i64 + 1) {
            let z1: Vec<u8> = z2[..n as usize]
                .iter()
                .map(|&c| sqlite3Toupper(c))
                .collect();
            sqlite3_result_text(context, Some(&z1), n);
        }
    }
}

/// Implementation of the lower() SQL function.
fn lowerFunc(context: &mut Context, argv: &mut [Mem]) {
    let z2 = value_text(&mut argv[0]);
    let n = sqlite3_value_bytes(&mut argv[0]);
    if let Some(z2) = z2 {
        if contextMalloc(context, n as i64 + 1) {
            let z1: Vec<u8> = z2[..n as usize]
                .iter()
                .map(|&c| sqlite3Tolower(c))
                .collect();
            sqlite3_result_text(context, Some(&z1), n);
        }
    }
}

/// Implementation of random().  Return a random integer.
fn randomFunc(context: &mut Context, _argv: &mut [Mem]) {
    let mut b = [0u8; 8];
    crate::random::sqlite3_randomness(8, &mut b);
    let mut r = i64::from_ne_bytes(b);
    if r < 0 {
        // We need to prevent a random number of 0x8000000000000000 (or
        // -9223372036854775808) since when you do abs() of that number you
        // get the same value back again.
        r = -(r & LARGEST_INT64);
    }
    sqlite3_result_int64(context, r);
}

/// Implementation of randomblob(N).  Return a random blob that is N bytes
/// long.
fn randomBlob(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let mut n = sqlite3_value_int64(&mut argv[0]);
    if n < 1 {
        n = 1;
    }
    if contextMalloc(context, n) {
        let mut p = vec![0u8; n as usize];
        crate::random::sqlite3_randomness(n as i32, &mut p);
        sqlite3_result_blob(context, &p, n as i32);
    }
}

/// Implementation of the last_insert_rowid() SQL function.
fn last_insert_rowid(context: &mut Context, _argv: &mut [Mem]) {
    // IMP: R-51513-12026 The last_insert_rowid() SQL function is a wrapper
    // around the sqlite3_last_insert_rowid() C/C++ interface function.
    let v = crate::mainapi::sqlite3_last_insert_rowid(context.db);
    sqlite3_result_int64(context, v);
}

/// Implementation of the changes() SQL function.
fn changes(context: &mut Context, _argv: &mut [Mem]) {
    let v = crate::mainapi::sqlite3_changes64(context.db);
    sqlite3_result_int64(context, v);
}

/// Implementation of the total_changes() SQL function.
fn total_changes(context: &mut Context, _argv: &mut [Mem]) {
    // IMP: R-11217-42568 This function is a wrapper around the
    // sqlite3_total_changes64() C/C++ interface.
    let v = crate::mainapi::sqlite3_total_changes64(context.db);
    sqlite3_result_int64(context, v);
}

/// A structure defining how to do GLOB-style comparisons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareInfo {
    /// "*" or "%"
    pub matchAll: u8,
    /// "?" or "_"
    pub matchOne: u8,
    /// "[" or 0
    pub matchSet: u8,
    /// true to ignore case differences
    pub noCase: u8,
}

impl CompareInfo {
    /// Encoding of a compareInfo as the FuncUserData of like()/glob().
    pub fn to_int(&self) -> i64 {
        (self.matchAll as i64)
            | ((self.matchOne as i64) << 8)
            | ((self.matchSet as i64) << 16)
            | ((self.noCase as i64) << 24)
    }
    pub fn from_int(v: i64) -> CompareInfo {
        CompareInfo {
            matchAll: (v & 0xff) as u8,
            matchOne: ((v >> 8) & 0xff) as u8,
            matchSet: ((v >> 16) & 0xff) as u8,
            noCase: ((v >> 24) & 0xff) as u8,
        }
    }
}

static globInfo: CompareInfo = CompareInfo {
    matchAll: b'*',
    matchOne: b'?',
    matchSet: b'[',
    noCase: 0,
};
// The correct SQL-92 behavior is for the LIKE operator to ignore case.
// Thus  'a' LIKE 'A' would be true.
static likeInfoNorm: CompareInfo = CompareInfo {
    matchAll: b'%',
    matchOne: b'_',
    matchSet: 0,
    noCase: 1,
};
// If SQLITE_CASE_SENSITIVE_LIKE is defined, then the LIKE operator is case
// sensitive causing 'a' LIKE 'A' to be false
static likeInfoAlt: CompareInfo = CompareInfo {
    matchAll: b'%',
    matchOne: b'_',
    matchSet: 0,
    noCase: 0,
};

// Possible error returns from patternMatch()
const SQLITE_MATCH: i32 = 0;
const SQLITE_NOMATCH: i32 = 1;
const SQLITE_NOWILDCARDMATCH: i32 = 2;

/// Compare two UTF-8 strings for equality where the first string is a GLOB
/// or LIKE expression.  Return values:
///
///    SQLITE_MATCH:            Match
///    SQLITE_NOMATCH:          No match
///    SQLITE_NOWILDCARDMATCH:  No match in spite of having * or % wildcards.
///
/// Both strings are zero-terminated (the end of the slice counts as the
/// terminator).
fn patternCompare(zPattern: &[u8], zString: &[u8], pInfo: &CompareInfo, matchOther: u32) -> i32 {
    let matchOne = pInfo.matchOne as u32;
    let matchAll = pInfo.matchAll as u32;
    let noCase = pInfo.noCase;
    let mut zEscaped: Option<usize> = None; // One past the last escaped input char
    let mut ip = 0usize; // index in zPattern
    let mut is = 0usize; // index in zString
    loop {
        let mut c = Utf8Read(zPattern, &mut ip);
        if c == 0 {
            break;
        }
        if c == matchAll {
            // Match "*"
            // Skip over multiple "*" characters in the pattern.  If there are
            // also "?" characters, skip those as well, but consume a single
            // character of the input string for each "?" skipped
            loop {
                c = Utf8Read(zPattern, &mut ip);
                if !(c == matchAll || (c == matchOne && matchOne != 0)) {
                    break;
                }
                if c == matchOne && sqlite3Utf8Read(zString, &mut is) == 0 {
                    return SQLITE_NOWILDCARDMATCH;
                }
            }
            if c == 0 {
                return SQLITE_MATCH; // "*" at the end of the pattern matches
            } else if c == matchOther {
                if pInfo.matchSet == 0 {
                    c = sqlite3Utf8Read(zPattern, &mut ip);
                    if c == 0 {
                        return SQLITE_NOWILDCARDMATCH;
                    }
                } else {
                    // "[...]" immediately follows the "*".  We have to do a
                    // slow recursive search in this case, but it is an
                    // unusual case.
                    debug_assert!(matchOther < 0x80); // '[' is a single-byte character
                    while at(zString, is) != 0 {
                        let bMatch =
                            patternCompare(&zPattern[ip - 1..], &zString[is..], pInfo, matchOther);
                        if bMatch != SQLITE_NOMATCH {
                            return bMatch;
                        }
                        crate::utf::SQLITE_SKIP_UTF8(zString, &mut is);
                    }
                    return SQLITE_NOWILDCARDMATCH;
                }
            }

            // At this point variable c contains the first character of the
            // pattern string past the "*".  Search in the input string for
            // the first matching character and recursively continue the
            // match from that point.
            let zp: &[u8] = if ip <= zPattern.len() {
                &zPattern[ip..]
            } else {
                b""
            };
            if c < 0x80 {
                let (s0, s1) = if noCase != 0 {
                    (sqlite3Toupper(c as u8), sqlite3Tolower(c as u8))
                } else {
                    (c as u8, c as u8)
                };
                loop {
                    // zString += strcspn(zString, zStop)
                    while is < zString.len() {
                        let ch = zString[is];
                        if ch == 0 || ch == s0 || ch == s1 {
                            break;
                        }
                        is += 1;
                    }
                    if at(zString, is) == 0 {
                        break;
                    }
                    is += 1;
                    let bMatch = patternCompare(zp, &zString[is..], pInfo, matchOther);
                    if bMatch != SQLITE_NOMATCH {
                        return bMatch;
                    }
                }
            } else {
                loop {
                    let c2 = Utf8Read(zString, &mut is);
                    if c2 == 0 {
                        break;
                    }
                    if c2 != c {
                        continue;
                    }
                    let zs: &[u8] = if is <= zString.len() {
                        &zString[is..]
                    } else {
                        b""
                    };
                    let bMatch = patternCompare(zp, zs, pInfo, matchOther);
                    if bMatch != SQLITE_NOMATCH {
                        return bMatch;
                    }
                }
            }
            return SQLITE_NOWILDCARDMATCH;
        }
        if c == matchOther {
            if pInfo.matchSet == 0 {
                c = sqlite3Utf8Read(zPattern, &mut ip);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                zEscaped = Some(ip);
            } else {
                let mut prior_c: u32 = 0;
                let mut seen = 0;
                let mut invert = 0;
                c = sqlite3Utf8Read(zString, &mut is);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                let mut c2 = sqlite3Utf8Read(zPattern, &mut ip);
                if c2 == b'^' as u32 {
                    invert = 1;
                    c2 = sqlite3Utf8Read(zPattern, &mut ip);
                }
                if c2 == b']' as u32 {
                    if c == b']' as u32 {
                        seen = 1;
                    }
                    c2 = sqlite3Utf8Read(zPattern, &mut ip);
                }
                while c2 != 0 && c2 != b']' as u32 {
                    if c2 == b'-' as u32
                        && at(zPattern, ip) != b']'
                        && at(zPattern, ip) != 0
                        && prior_c > 0
                    {
                        c2 = sqlite3Utf8Read(zPattern, &mut ip);
                        if c >= prior_c && c <= c2 {
                            seen = 1;
                        }
                        prior_c = 0;
                    } else {
                        if c == c2 {
                            seen = 1;
                        }
                        prior_c = c2;
                    }
                    c2 = sqlite3Utf8Read(zPattern, &mut ip);
                }
                if c2 == 0 || (seen ^ invert) == 0 {
                    return SQLITE_NOMATCH;
                }
                continue;
            }
        }
        let c2 = Utf8Read(zString, &mut is);
        if c == c2 {
            continue;
        }
        if noCase != 0
            && sqlite3Tolower(c as u8) == sqlite3Tolower(c2 as u8)
            && c < 0x80
            && c2 < 0x80
        {
            continue;
        }
        if c == matchOne && Some(ip) != zEscaped && c2 != 0 {
            continue;
        }
        return SQLITE_NOMATCH;
    }
    if at(zString, is) == 0 {
        SQLITE_MATCH
    } else {
        SQLITE_NOMATCH
    }
}

/// C: `int sqlite3_strglob(const char *zGlobPattern, const char *zString)`
///
/// The sqlite3_strglob() interface.  Return 0 on a match (like strcmp())
/// and non-zero if there is no match.
pub fn sqlite3_strglob(zGlobPattern: Option<&[u8]>, zString: Option<&[u8]>) -> i32 {
    match (zString, zGlobPattern) {
        (None, p) => p.is_some() as i32,
        (Some(_), None) => 1,
        (Some(s), Some(p)) => patternCompare(p, s, &globInfo, b'[' as u32),
    }
}

/// C: `int sqlite3_strlike(const char *zPattern, const char *zStr, unsigned int esc)`
///
/// The sqlite3_strlike() interface.  Return 0 on a match and non-zero for a
/// miss - like strcmp().
pub fn sqlite3_strlike(zPattern: Option<&[u8]>, zStr: Option<&[u8]>, esc: u32) -> i32 {
    match (zStr, zPattern) {
        (None, p) => p.is_some() as i32,
        (Some(_), None) => 1,
        (Some(s), Some(p)) => patternCompare(p, s, &likeInfoNorm, esc),
    }
}

/// Implementation of the like() SQL function.  This function implements
/// the built-in LIKE operator.  The first argument to the function is the
/// pattern and the second argument is the string.  So "A LIKE B" is
/// implemented as like(B,A).  This same function (with a different
/// compareInfo structure) computes the GLOB operator.
fn likeFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let mut info = CompareInfo::from_int(sqlite3_user_data(context).as_int());
    let escape: u32;

    // Limit the length of the LIKE or GLOB pattern to avoid problems of
    // deep recursion and N*N behavior in patternCompare().
    let nPat = sqlite3_value_bytes(&mut argv[0]);
    if nPat > context.db.aLimit[SQLITE_LIMIT_LIKE_PATTERN_LENGTH as usize] {
        sqlite3_result_error(context, Some(b"LIKE or GLOB pattern too complex"), -1);
        return;
    }
    if argc == 3 {
        // The escape character string must consist of a single UTF-8
        // character.  Otherwise, return an error.
        let Some(zEsc) = value_text(&mut argv[2]) else {
            return;
        };
        if sqlite3Utf8CharLen(Some(&zEsc), -1) != 1 {
            sqlite3_result_error(
                context,
                Some(b"ESCAPE expression must be a single character"),
                -1,
            );
            return;
        }
        let mut k = 0usize;
        escape = sqlite3Utf8Read(&zEsc, &mut k);
        if escape == info.matchAll as u32 || escape == info.matchOne as u32 {
            if escape == info.matchAll as u32 {
                info.matchAll = 0;
            }
            if escape == info.matchOne as u32 {
                info.matchOne = 0;
            }
        }
    } else {
        escape = info.matchSet as u32;
    }
    let zB = value_text(&mut argv[0]);
    let zA = value_text(&mut argv[1]);
    if let (Some(zA), Some(zB)) = (zA, zB) {
        let r = (patternCompare(&zB, &zA, &info, escape) == SQLITE_MATCH) as i32;
        sqlite3_result_int(context, r);
    }
}

/// Implementation of the NULLIF(x,y) function.  The result is the first
/// argument if the arguments are different.  The result is NULL if the
/// arguments are equal to each other.
fn nullifFunc(context: &mut Context, argv: &mut [Mem]) {
    let pColl = sqlite3GetFuncCollSeq(context);
    if crate::vdbeaux::sqlite3MemCompare(context.db, &argv[0], &argv[1], Some(&pColl)) != 0 {
        sqlite3_result_value(context, &mut argv[0]);
    }
}

/// Implementation of the sqlite_version() function.  The result is the
/// version of the SQLite library that is running.
fn versionFunc(context: &mut Context, _argv: &mut [Mem]) {
    // IMP: R-48699-48617 This function is an SQL wrapper around the
    // sqlite3_libversion() C-interface.
    let v = crate::mainapi::sqlite3_libversion();
    sqlite3_result_text(context, v.as_deref(), -1);
}

/// Some functions like COALESCE() and IFNULL() and UNLIKELY() are
/// implemented as VDBE code; noopFunc is their (never called)
/// implementation in the function table.
fn noopFunc(context: &mut Context, argv: &mut [Mem]) {
    versionFunc(context, argv)
}

/// Implementation of the sqlite_source_id() function.
fn sourceidFunc(context: &mut Context, _argv: &mut [Mem]) {
    // IMP: R-24470-31136 This function is an SQL wrapper around the
    // sqlite3_sourceid() C interface.
    let v = crate::mainapi::sqlite3_sourceid();
    sqlite3_result_text(context, v.as_deref(), -1);
}

/// Implementation of the sqlite_log() function.  This is a wrapper around
/// sqlite3_log().  The return value is NULL.
fn errlogFunc(_context: &mut Context, argv: &mut [Mem]) {
    let code = sqlite3_value_int(&mut argv[0]);
    let msg = value_text(&mut argv[1]);
    crate::printf::sqlite3_log(code, b"%s", &[PfArg::Str(msg.as_deref())]);
}

/// Implementation of the sqlite_compileoption_used() function.
fn compileoptionusedFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    // IMP: R-39564-36305 The sqlite_compileoption_used() SQL function is a
    // wrapper around the sqlite3_compileoption_used() C/C++ function.
    if let Some(zOptName) = value_text(&mut argv[0]) {
        let r = crate::mainapi::sqlite3_compileoption_used(Some(&zOptName));
        sqlite3_result_int(context, r);
    }
}

/// Implementation of the sqlite_compileoption_get() function.
fn compileoptiongetFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    // IMP: R-04922-24076 The sqlite_compileoption_get() SQL function is a
    // wrapper around the sqlite3_compileoption_get() C/C++ function.
    let n = sqlite3_value_int(&mut argv[0]);
    let v = crate::mainapi::sqlite3_compileoption_get(n);
    sqlite3_result_text(context, v.as_deref(), -1);
}

/// Array for converting from half-bytes (nybbles) into ASCII hex digits.
static hexdigits: [u8; 16] = *b"0123456789ABCDEF";

/// C: `void sqlite3QuoteValue(StrAccum *pStr, sqlite3_value *pValue, int bEscape)`
///
/// Append to pStr text that is the SQL literal representation of the value
/// contained in pValue.
pub fn sqlite3QuoteValue(pStr: &mut StrAccum, pValue: &mut Mem, bEscape: i32) {
    // As currently implemented, the string must be initially empty.
    debug_assert!(pStr.zText.is_empty());
    match sqlite3_value_type(pValue) {
        SQLITE_FLOAT => {
            //    ,---  Show infinity as 9.0e+999
            //    | ,--- 17 precision guarantees round-trip
            //    v v
            let r = sqlite3_value_double(pValue);
            sqlite3_str_appendf(pStr, b"%!0.17g", &[PfArg::Dbl(r)]);
        }
        SQLITE_INTEGER => {
            let i = sqlite3_value_int64(pValue);
            sqlite3_str_appendf(pStr, b"%lld", &[PfArg::Int(i)]);
        }
        SQLITE_BLOB => {
            let zBlob = value_blob(pValue).unwrap_or_default();
            let nBlob = sqlite3_value_bytes(pValue) as i64;
            sqlite3StrAccumEnlarge(pStr, nBlob * 2 + 4);
            if pStr.accError == 0 {
                let mut t: Vec<u8> = Vec::with_capacity(nBlob as usize * 2 + 3);
                t.push(b'X');
                t.push(b'\'');
                for &b in zBlob[..nBlob as usize].iter() {
                    t.push(hexdigits[((b >> 4) & 0x0F) as usize]);
                    t.push(hexdigits[(b & 0x0F) as usize]);
                }
                t.push(b'\'');
                pStr.zText = t;
            }
        }
        SQLITE_TEXT => {
            let zArg = value_text(pValue);
            let fmt: &[u8] = if bEscape != 0 { b"%#Q" } else { b"%Q" };
            sqlite3_str_appendf(pStr, fmt, &[PfArg::Str(zArg.as_deref())]);
        }
        _ => {
            debug_assert!(sqlite3_value_type(pValue) == SQLITE_NULL);
            sqlite3_str_append(pStr, b"NULL", 4);
        }
    }
}

/// Return true if z[] begins with N hexadecimal digits, and write a
/// decoding of those digits into *pVal.
fn isNHex(z: &[u8], N: usize, pVal: &mut u32) -> bool {
    let mut v: u32 = 0;
    for i in 0..N {
        if !sqlite3Isxdigit(at(z, i)) {
            return false;
        }
        v = (v << 4) + sqlite3HexToInt(z[i] as i32) as u32;
    }
    *pVal = v;
    true
}

/// Implementation of the UNISTR() function.
///
/// Evaluate escaped Unicode characters in the argument: \XXXX, \+XXXXXX,
/// \uXXXX, \UXXXXXXXX; two backslashes are a backslash.
fn unistrFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let Some(zIn) = value_text(&mut argv[0]) else {
        return;
    };
    let nIn = sqlite3_value_bytes(&mut argv[0]) as usize;
    let mut zOut: Vec<u8> = Vec::with_capacity(nIn + 1);
    let mut i = 0usize;
    let mut v: u32 = 0;
    let mut buf = [0u8; 4];
    while i < nIn {
        // strchr(&zIn[i],'\\') (stops at a NUL byte)
        let mut z = None;
        let mut k = i;
        while k < zIn.len() && zIn[k] != 0 {
            if zIn[k] == b'\\' {
                z = Some(k);
                break;
            }
            k += 1;
        }
        let Some(z) = z else {
            zOut.extend_from_slice(&zIn[i..nIn]);
            break;
        };
        let n = z - i;
        if n > 0 {
            zOut.extend_from_slice(&zIn[i..i + n]);
            i += n;
        }
        let c1 = at(&zIn, i + 1);
        let ok = if c1 == b'\\' {
            i += 2;
            zOut.push(b'\\');
            true
        } else if sqlite3Isxdigit(c1) {
            if !isNHex(&zIn[i + 1..], 4, &mut v) {
                false
            } else {
                i += 5;
                let n = sqlite3AppendOneUtf8Character(&mut buf, v) as usize;
                zOut.extend_from_slice(&buf[..n]);
                true
            }
        } else if c1 == b'+' {
            if !isNHex(&zIn[(i + 2).min(zIn.len())..], 6, &mut v) {
                false
            } else {
                i += 8;
                let n = sqlite3AppendOneUtf8Character(&mut buf, v) as usize;
                zOut.extend_from_slice(&buf[..n]);
                true
            }
        } else if c1 == b'u' {
            if !isNHex(&zIn[(i + 2).min(zIn.len())..], 4, &mut v) {
                false
            } else {
                i += 6;
                let n = sqlite3AppendOneUtf8Character(&mut buf, v) as usize;
                zOut.extend_from_slice(&buf[..n]);
                true
            }
        } else if c1 == b'U' {
            if !isNHex(&zIn[(i + 2).min(zIn.len())..], 8, &mut v) {
                false
            } else {
                i += 10;
                let n = sqlite3AppendOneUtf8Character(&mut buf, v) as usize;
                zOut.extend_from_slice(&buf[..n]);
                true
            }
        } else {
            false
        };
        if !ok {
            sqlite3_result_error(context, Some(b"invalid Unicode escape"), -1);
            return;
        }
    }
    let j = zOut.len();
    sqlite3_result_text64(context, Some(&zOut), j as u64, SQLITE_UTF8_ZT);
}

/// Implementation of the QUOTE() function.
///
/// The quote(X) function returns the text of an SQL literal which is the
/// value of its argument suitable for inclusion into an SQL statement.  If
/// sqlite3_user_data() is non-zero, then the UNISTR_QUOTE() function is
/// implemented instead.
fn quoteFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let mut str = StrAccum::default();
    sqlite3StrAccumInit(&mut str, 0, context.db.aLimit[SQLITE_LIMIT_LENGTH as usize]);
    let bEscape = sqlite3_user_data(context).as_int() as i32;
    sqlite3QuoteValue(&mut str, &mut argv[0], bEscape);
    sqlite3_result_str(context, &mut str, SQLITE_XFER);
}

/// The unicode() function.  Return the integer unicode code-point value
/// for the first character of the input string.
fn unicodeFunc(context: &mut Context, argv: &mut [Mem]) {
    if let Some(z) = value_text(&mut argv[0]) {
        if at(&z, 0) != 0 {
            let mut k = 0usize;
            let c = sqlite3Utf8Read(&z, &mut k);
            sqlite3_result_int(context, c as i32);
        }
    }
}

/// The char() function takes zero or more arguments, each of which is an
/// integer.  It constructs a string where each character of the string is
/// the unicode character for the corresponding integer argument.
fn charFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let mut z: Vec<u8> = Vec::with_capacity(argc * 4 + 1);
    for i in 0..argc {
        let mut x = sqlite3_value_int64(&mut argv[i]);
        if x < 0 || x > 0x10ffff {
            x = 0xfffd;
        }
        let c = (x & 0x1fffff) as u32;
        if c < 0x00080 {
            z.push((c & 0xFF) as u8);
        } else if c < 0x00800 {
            z.push(0xC0 + ((c >> 6) & 0x1F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        } else if c < 0x10000 {
            z.push(0xE0 + ((c >> 12) & 0x0F) as u8);
            z.push(0x80 + ((c >> 6) & 0x3F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        } else {
            z.push(0xF0 + ((c >> 18) & 0x07) as u8);
            z.push(0x80 + ((c >> 12) & 0x3F) as u8);
            z.push(0x80 + ((c >> 6) & 0x3F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        }
    }
    let n = z.len();
    sqlite3_result_text64(context, Some(&z), n as u64, SQLITE_UTF8_ZT);
}

/// The hex() function.  Interpret the argument as a blob.  Return a
/// hexadecimal rendering as text.
fn hexFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let pBlob = value_blob(&mut argv[0]).unwrap_or_default();
    let n = sqlite3_value_bytes(&mut argv[0]) as usize;
    if contextMalloc(context, n as i64 * 2 + 1) {
        let mut zHex: Vec<u8> = Vec::with_capacity(n * 2 + 1);
        for &c in pBlob[..n].iter() {
            zHex.push(hexdigits[((c >> 4) & 0xf) as usize]);
            zHex.push(hexdigits[(c & 0xf) as usize]);
        }
        let len = zHex.len();
        sqlite3_result_text64(context, Some(&zHex), len as u64, SQLITE_UTF8_ZT);
    }
}

/// Buffer zStr contains nStr bytes of utf-8 encoded text.  Return true if
/// zStr contains character ch.
fn strContainsChar(zStr: &[u8], nStr: usize, ch: u32) -> bool {
    let z = &zStr[..nStr.min(zStr.len())];
    let mut i = 0usize;
    while i < nStr {
        let tst = Utf8Read(z, &mut i);
        if tst == ch {
            return true;
        }
    }
    false
}

/// The unhex() function.  The first argument is text containing pairs of
/// hexadecimal digits which are decoded and returned as a blob.  Characters
/// of the optional second argument may appear between pairs of digits.
/// NULL is returned on any other character.
fn unhexFunc(pCtx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    let zHex = value_text(&mut argv[0]);
    let nHex = sqlite3_value_bytes(&mut argv[0]);
    let mut zPass: Option<Vec<u8>> = Some(Vec::new());
    let mut nPass = 0;
    if argc == 2 {
        zPass = value_text(&mut argv[1]);
        nPass = sqlite3_value_bytes(&mut argv[1]);
    }
    let (Some(zHex), Some(zPass)) = (zHex, zPass) else {
        return;
    };
    if !contextMalloc(pCtx, (nHex / 2) as i64 + 1) {
        return;
    }
    let mut pBlob: Vec<u8> = Vec::with_capacity((nHex / 2) as usize + 1);
    let mut i = 0usize;
    'outer: loop {
        let mut c = at(&zHex, i);
        if c == 0x00 {
            break;
        }
        while !sqlite3Isxdigit(c) {
            let ch = Utf8Read(&zHex, &mut i);
            if !strContainsChar(&zPass, nPass as usize, ch) {
                // unhex_null
                return;
            }
            c = at(&zHex, i);
            if c == 0x00 {
                break 'outer; // unhex_done
            }
        }
        i += 1;
        let d = at(&zHex, i);
        i += 1;
        if !sqlite3Isxdigit(d) {
            return; // unhex_null
        }
        pBlob.push((sqlite3HexToInt(c as i32) << 4) | sqlite3HexToInt(d as i32));
    }
    // unhex_done:
    let n = pBlob.len();
    sqlite3_result_blob(pCtx, &pBlob, n as i32);
}

/// The zeroblob(N) function returns a zero-filled blob of size N bytes.
fn zeroblobFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let mut n = sqlite3_value_int64(&mut argv[0]);
    if n < 0 {
        n = 0;
    }
    let rc = sqlite3_result_zeroblob64(context, n as u64); // IMP: R-00293-64994
    if rc != 0 {
        sqlite3_result_error_code(context, rc);
    }
}

/// The replace() function.  Three arguments are all strings: call them A,
/// B, and C.  The result is also a string which is derived from A by
/// replacing every occurrence of B with C.  The match must be exact.
/// Collating sequences are not used.
fn replaceFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 3);
    let Some(zStr) = value_text(&mut argv[0]) else {
        return;
    };
    let nStr = sqlite3_value_bytes(&mut argv[0]) as i64;
    let Some(zPattern) = value_text(&mut argv[1]) else {
        return;
    };
    if at(&zPattern, 0) == 0 {
        debug_assert!(sqlite3_value_type(&mut argv[1]) != SQLITE_NULL);
        sqlite3_result_text(context, Some(&zStr), nStr as i32);
        return;
    }
    let nPattern = sqlite3_value_bytes(&mut argv[1]) as i64;
    let Some(zRep) = value_text(&mut argv[2]) else {
        return;
    };
    let nRep = sqlite3_value_bytes(&mut argv[2]) as i64;
    let mut nOut: i64 = nStr + 1;
    debug_assert!(nOut < SQLITE_MAX_LENGTH as i64);
    if !contextMalloc(context, nOut) {
        return;
    }
    let limit = context.db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64;
    let mut zOut: Vec<u8> = Vec::with_capacity(nOut as usize);
    let loopLimit = nStr - nPattern;
    let pat = &zPattern[..nPattern as usize];
    let rep = &zRep[..nRep as usize];
    let mut i: i64 = 0;
    while i <= loopLimit {
        let iu = i as usize;
        if zStr[iu] != pat[0] || &zStr[iu..iu + nPattern as usize] != pat {
            zOut.push(zStr[iu]);
        } else {
            if nRep > nPattern {
                nOut += nRep - nPattern;
                if nOut - 1 > limit {
                    sqlite3_result_error_toobig(context);
                    return;
                }
            }
            zOut.extend_from_slice(rep);
            i += nPattern - 1;
        }
        i += 1;
    }
    zOut.extend_from_slice(&zStr[i as usize..nStr as usize]);
    let j = zOut.len();
    sqlite3_result_text(context, Some(&zOut), j as i32);
}

/// Implementation of the TRIM(), LTRIM(), and RTRIM() functions.  The
/// userdata is 0x1 for left trim, 0x2 for right trim, 0x3 for both.
fn trimFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if sqlite3_value_type(&mut argv[0]) == SQLITE_NULL {
        return;
    }
    let Some(zIn) = value_text(&mut argv[0]) else {
        return;
    };
    let mut nIn = sqlite3_value_bytes(&mut argv[0]) as usize;
    let mut zi = 0usize; // start of zIn
    let azChar: Vec<Vec<u8>>;
    if argc == 1 {
        azChar = vec![b" ".to_vec()];
    } else {
        let Some(zCharSet) = value_text(&mut argv[1]) else {
            return;
        };
        let mut v = Vec::new();
        let mut z = 0usize;
        while at(&zCharSet, z) != 0 {
            let s = z;
            crate::utf::SQLITE_SKIP_UTF8(&zCharSet, &mut z);
            let e = z.min(zCharSet.len());
            v.push(zCharSet[s..e].to_vec());
        }
        if !v.is_empty()
            && !contextMalloc(
                context,
                (v.len() * (std::mem::size_of::<usize>() + std::mem::size_of::<u32>())) as i64,
            )
        {
            return;
        }
        azChar = v;
    }
    let nChar = azChar.len();
    if nChar > 0 {
        let flags = sqlite3_user_data(context).as_int() as i32;
        if (flags & 1) != 0 {
            while nIn > 0 {
                let mut len = 0usize;
                let mut i = 0usize;
                while i < nChar {
                    len = azChar[i].len();
                    if len <= nIn && zIn[zi..zi + len] == azChar[i][..] {
                        break;
                    }
                    i += 1;
                }
                if i >= nChar {
                    break;
                }
                zi += len;
                nIn -= len;
            }
        }
        if (flags & 2) != 0 {
            while nIn > 0 {
                let mut len = 0usize;
                let mut i = 0usize;
                while i < nChar {
                    len = azChar[i].len();
                    if len <= nIn && zIn[zi + nIn - len..zi + nIn] == azChar[i][..] {
                        break;
                    }
                    i += 1;
                }
                if i >= nChar {
                    break;
                }
                nIn -= len;
            }
        }
    }
    sqlite3_result_text(context, Some(&zIn[zi..zi + nIn]), nIn as i32);
}

/// The core implementation of the CONCAT(...) and CONCAT_WS(SEP,...)
/// functions.  Return a string value that is the concatenation of all
/// non-null entries in argv[].  Use zSep as the separator.
fn concatFuncCore(context: &mut Context, argv: &mut [Mem], zSep: &[u8]) {
    let argc = argv.len();
    let nSep = zSep.len();
    let mut n: i64 = 0;
    for i in 0..argc {
        n += sqlite3_value_bytes(&mut argv[i]) as i64;
    }
    n += (argc as i64 - 1) * nSep as i64;
    let mut z: Vec<u8> = Vec::with_capacity((n + 1).max(1) as usize);
    let mut bNotNull = false;
    for i in 0..argc {
        if sqlite3_value_type(&mut argv[i]) != SQLITE_NULL {
            let k = sqlite3_value_bytes(&mut argv[i]) as usize;
            if let Some(v) = value_text(&mut argv[i]) {
                if bNotNull && nSep > 0 {
                    z.extend_from_slice(zSep);
                }
                z.extend_from_slice(&v[..k.min(v.len())]);
                bNotNull = true;
            }
        }
    }
    let j = z.len();
    sqlite3_result_text64(context, Some(&z), j as u64, SQLITE_UTF8_ZT);
}

/// The CONCAT(...) function.  Generate a string result that is the
/// concatentation of all non-null arguments.
fn concatFunc(context: &mut Context, argv: &mut [Mem]) {
    concatFuncCore(context, argv, b"");
}

/// The CONCAT_WS(separator, ...) function.
fn concatwsFunc(context: &mut Context, argv: &mut [Mem]) {
    let nSep = sqlite3_value_bytes(&mut argv[0]) as usize;
    let Some(zSep) = value_text(&mut argv[0]) else {
        return;
    };
    concatFuncCore(context, &mut argv[1..], &zSep[..nSep.min(zSep.len())]);
}

/// Compute the soundex encoding of a word (SQLITE_SOUNDEX builds only; not
/// registered in the default build).
#[allow(dead_code)]
fn soundexFunc(context: &mut Context, argv: &mut [Mem]) {
    static iCode: [u8; 128] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 1, 2, 3, 0, 1, 2, 0, 0, 2, 2, 4, 5, 5, 0, 1, 2, 6, 2, 3, 0, 1, 0, 2, 0,
        2, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 0, 1, 2, 0, 0, 2, 2, 4, 5, 5, 0, 1, 2, 6, 2, 3, 0, 1, 0,
        2, 0, 2, 0, 0, 0, 0, 0,
    ];
    debug_assert!(argv.len() == 1);
    let zIn = value_text(&mut argv[0]).unwrap_or_default();
    let mut i = 0usize;
    while at(&zIn, i) != 0 && !sqlite3Isalpha(zIn[i]) {
        i += 1;
    }
    if at(&zIn, i) != 0 {
        let mut zResult = [0u8; 8];
        let mut prevcode = iCode[(zIn[i] & 0x7f) as usize];
        zResult[0] = sqlite3Toupper(zIn[i]);
        let mut j = 1usize;
        while j < 4 && at(&zIn, i) != 0 {
            let code = iCode[(zIn[i] & 0x7f) as usize];
            if code > 0 {
                if code != prevcode {
                    prevcode = code;
                    zResult[j] = code + b'0';
                    j += 1;
                }
            } else {
                prevcode = 0;
            }
            i += 1;
        }
        while j < 4 {
            zResult[j] = b'0';
            j += 1;
        }
        sqlite3_result_text(context, Some(&zResult[..4]), 4);
    } else {
        // IMP: R-64894-50321 The string "?000" is returned if the argument
        // is NULL or contains no ASCII alphabetic characters.
        sqlite3_result_text(context, Some(b"?000"), 4);
    }
}

/// A function that loads a shared-library extension then returns NULL.
fn loadExt(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let zFile = value_text(&mut argv[0]);

    // Disallow the load_extension() SQL function unless the
    // SQLITE_LoadExtFunc flag is set.  See the
    // sqlite3_enable_load_extension() API.
    if (context.db.flags & SQLITE_LoadExtFunc) == 0 {
        sqlite3_result_error(context, Some(b"not authorized"), -1);
        return;
    }
    let zProc = if argc == 2 {
        value_text(&mut argv[1])
    } else {
        None
    };
    if let Some(zFile) = zFile {
        let mut zErrMsg: Option<ZStr> = None;
        if crate::loadext::sqlite3_load_extension(
            context.db,
            Some(&zFile),
            zProc.as_deref(),
            &mut zErrMsg,
        ) != 0
        {
            sqlite3_result_error(context, zErrMsg.as_deref(), -1);
        }
    }
}

/// An instance of the following structure holds the context of a sum() or
/// avg() aggregate computation.
#[derive(Default, Clone, Copy, Debug)]
pub struct SumCtx {
    /// Running sum as as a double
    pub rSum: f64,
    /// Error term for Kahan-Babushka-Neumaier summation
    pub rErr: f64,
    /// Running sum as a signed integer
    pub iSum: i64,
    /// Number of elements summed
    pub cnt: i64,
    /// True if any non-integer value was input to the sum
    pub approx: u8,
    /// Integer overflow seen
    pub ovrfl: u8,
}

/// Do one step of the Kahan-Babushka-Neumaier summation.
fn kahanBabuskaNeumaierStep(pSum: &mut SumCtx, r: f64) {
    let s = pSum.rSum;
    let t = s + r;
    if s.abs() > r.abs() {
        pSum.rErr += (s - t) + r;
    } else {
        pSum.rErr += (r - t) + s;
    }
    pSum.rSum = t;
}

/// Add a (possibly large) integer to the running sum.
fn kahanBabuskaNeumaierStepInt64(pSum: &mut SumCtx, iVal: i64) {
    if iVal <= -4503599627370496 || iVal >= 4503599627370496 {
        let iSm = iVal % 16384;
        let iBig = iVal - iSm;
        kahanBabuskaNeumaierStep(pSum, iBig as f64);
        kahanBabuskaNeumaierStep(pSum, iSm as f64);
    } else {
        kahanBabuskaNeumaierStep(pSum, iVal as f64);
    }
}

/// Initialize the Kahan-Babaska-Neumaier sum from a 64-bit integer
fn kahanBabuskaNeumaierInit(p: &mut SumCtx, iVal: i64) {
    if iVal <= -4503599627370496 || iVal >= 4503599627370496 {
        let iSm = iVal % 16384;
        p.rSum = (iVal - iSm) as f64;
        p.rErr = iSm as f64;
    } else {
        p.rSum = iVal as f64;
        p.rErr = 0.0;
    }
}

/// Routines used to compute the sum, average, and total.
///
/// The SUM() function follows the (broken) SQL standard which means that
/// it returns NULL if it sums over no inputs.  TOTAL returns 0.0 in that
/// case.  In addition, TOTAL always returns a float where SUM might return
/// an integer if it never encounters a floating point value.  TOTAL never
/// fails, but SUM might throw an exception if it overflows an integer.
fn sumStep(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    if sqlite3_aggregate_context::<SumCtx>(context, std::mem::size_of::<SumCtx>() as i32).is_none()
    {
        return;
    }
    let ty = sqlite3_value_numeric_type(context.db, &mut argv[0]);
    if ty == SQLITE_NULL {
        return;
    }
    let (iv, rv) = if ty == SQLITE_INTEGER {
        (sqlite3_value_int64(&mut argv[0]), 0.0)
    } else {
        (0, sqlite3_value_double(&mut argv[0]))
    };
    let Some(p) =
        sqlite3_aggregate_context::<SumCtx>(context, std::mem::size_of::<SumCtx>() as i32)
    else {
        return;
    };
    p.cnt += 1;
    if p.approx == 0 {
        if ty != SQLITE_INTEGER {
            let s = p.iSum;
            kahanBabuskaNeumaierInit(p, s);
            p.approx = 1;
            kahanBabuskaNeumaierStep(p, rv);
        } else {
            let mut x = p.iSum;
            if sqlite3AddInt64(&mut x, iv) == 0 {
                p.iSum = x;
            } else {
                p.ovrfl = 1;
                let s = p.iSum;
                kahanBabuskaNeumaierInit(p, s);
                p.approx = 1;
                kahanBabuskaNeumaierStepInt64(p, iv);
            }
        }
    } else if ty == SQLITE_INTEGER {
        kahanBabuskaNeumaierStepInt64(p, iv);
    } else {
        p.ovrfl = 0;
        kahanBabuskaNeumaierStep(p, rv);
    }
}

fn sumInverse(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    if sqlite3_aggregate_context::<SumCtx>(context, std::mem::size_of::<SumCtx>() as i32).is_none()
    {
        return;
    }
    let ty = sqlite3_value_numeric_type(context.db, &mut argv[0]);
    // p is always non-NULL because sumStep() will have been called first to
    // initialize it
    if ty == SQLITE_NULL {
        return;
    }
    let (iv, rv) = if ty == SQLITE_INTEGER {
        let i = sqlite3_value_int64(&mut argv[0]);
        (i, 0.0)
    } else {
        (
            sqlite3_value_int64(&mut argv[0]),
            sqlite3_value_double(&mut argv[0]),
        )
    };
    let Some(p) =
        sqlite3_aggregate_context::<SumCtx>(context, std::mem::size_of::<SumCtx>() as i32)
    else {
        return;
    };
    debug_assert!(p.cnt > 0);
    p.cnt -= 1;
    if p.approx == 0 {
        let mut x = p.iSum;
        if sqlite3SubInt64(&mut x, iv) == 0 {
            p.iSum = x;
            return;
        }
        p.ovrfl = 1;
        p.approx = 1;
        let s = p.iSum;
        kahanBabuskaNeumaierInit(p, s);
    }
    if ty == SQLITE_INTEGER {
        if iv != SMALLEST_INT64 {
            kahanBabuskaNeumaierStepInt64(p, -iv);
        } else {
            kahanBabuskaNeumaierStepInt64(p, LARGEST_INT64);
            kahanBabuskaNeumaierStepInt64(p, 1);
        }
    } else {
        kahanBabuskaNeumaierStep(p, -rv);
    }
}

fn sumFinalize(context: &mut Context) {
    let p = sqlite3_aggregate_context::<SumCtx>(context, 0).copied();
    if let Some(p) = p {
        if p.cnt > 0 {
            if p.approx != 0 {
                if p.ovrfl != 0 {
                    sqlite3_result_error(context, Some(b"integer overflow"), -1);
                } else if sqlite3IsOverflow(p.rErr) == 0 {
                    sqlite3_result_double(context, p.rSum + p.rErr);
                } else {
                    sqlite3_result_double(context, p.rSum);
                }
            } else {
                sqlite3_result_int64(context, p.iSum);
            }
        }
    }
}

fn avgFinalize(context: &mut Context) {
    let p = sqlite3_aggregate_context::<SumCtx>(context, 0).copied();
    if let Some(p) = p {
        if p.cnt > 0 {
            let mut r;
            if p.approx != 0 {
                r = p.rSum;
                if sqlite3IsOverflow(p.rErr) == 0 {
                    r += p.rErr;
                }
            } else {
                r = p.iSum as f64;
            }
            sqlite3_result_double(context, r / p.cnt as f64);
        }
    }
}

fn totalFinalize(context: &mut Context) {
    let mut r = 0.0;
    let p = sqlite3_aggregate_context::<SumCtx>(context, 0).copied();
    if let Some(p) = p {
        if p.approx != 0 {
            r = p.rSum;
            if sqlite3IsOverflow(p.rErr) == 0 {
                r += p.rErr;
            }
        } else {
            r = p.iSum as f64;
        }
    }
    sqlite3_result_double(context, r);
}

/// The following structure keeps track of state information for the
/// count() aggregate function.
#[derive(Default, Clone, Copy, Debug)]
pub struct CountCtx {
    pub n: i64,
}

/// Routines to implement the count() aggregate function.
fn countStep(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let notNull = argc == 0 || SQLITE_NULL != sqlite3_value_type(&mut argv[0]);
    let p = sqlite3_aggregate_context::<CountCtx>(context, std::mem::size_of::<CountCtx>() as i32);
    if let Some(p) = p {
        if notNull {
            p.n += 1;
        }
    }
}

fn countFinalize(context: &mut Context) {
    let n = sqlite3_aggregate_context::<CountCtx>(context, 0)
        .map(|p| p.n)
        .unwrap_or(0);
    sqlite3_result_int64(context, n);
}

fn countInverse(ctx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    let notNull = argc == 0 || SQLITE_NULL != sqlite3_value_type(&mut argv[0]);
    // p is always non-NULL since countStep() will have been called first
    let p = sqlite3_aggregate_context::<CountCtx>(ctx, std::mem::size_of::<CountCtx>() as i32);
    if let Some(p) = p {
        if notNull {
            p.n -= 1;
        }
    }
}

/// Routines to implement min() and max() aggregate functions.
fn minmaxStep(context: &mut Context, argv: &mut [Mem]) {
    let szMem = std::mem::size_of::<Mem>() as i32;
    let argIsNull = sqlite3_value_type(&mut argv[0]) == SQLITE_NULL;
    let bestFlags = match sqlite3_aggregate_context::<Mem>(context, szMem) {
        None => return,
        Some(b) => b.flags,
    };
    if argIsNull {
        if bestFlags != 0 {
            sqlite3SkipAccumulatorLoad(context);
        }
    } else if bestFlags != 0 {
        let pColl = sqlite3GetFuncCollSeq(context);
        // This step function is used for both the min() and max()
        // aggregates, the only difference between the two being that the
        // sense of the comparison is inverted.
        let max = sqlite3_user_data(context).as_int() != 0;
        let mut best =
            std::mem::take(sqlite3_aggregate_context::<Mem>(context, szMem).expect("agg"));
        let cmp = crate::vdbeaux::sqlite3MemCompare(context.db, &best, &argv[0], Some(&pColl));
        let replace = (max && cmp < 0) || (!max && cmp > 0);
        if replace {
            crate::vdbemem::sqlite3VdbeMemCopy(&mut best, &argv[0]);
        }
        *sqlite3_aggregate_context::<Mem>(context, szMem).expect("agg") = best;
        if !replace {
            sqlite3SkipAccumulatorLoad(context);
        }
    } else {
        let best = sqlite3_aggregate_context::<Mem>(context, szMem).expect("agg");
        crate::vdbemem::sqlite3VdbeMemCopy(best, &argv[0]);
    }
}

fn minMaxValueFinalize(context: &mut Context, bValue: bool) {
    let Some(pRes) = sqlite3_aggregate_context::<Mem>(context, 0) else {
        return;
    };
    let mut res = std::mem::take(pRes);
    if res.flags != 0 {
        sqlite3_result_value(context, &mut res);
    }
    if bValue {
        if let Some(p) = sqlite3_aggregate_context::<Mem>(context, 0) {
            *p = res;
        }
    } else {
        crate::vdbemem::sqlite3VdbeMemRelease(&mut res);
    }
}

fn minMaxValue(context: &mut Context) {
    minMaxValueFinalize(context, true);
}

fn minMaxFinalize(context: &mut Context) {
    minMaxValueFinalize(context, false);
}

/// group_concat(EXPR, ?SEPARATOR?) / string_agg(EXPR, SEPARATOR)
///
/// Content is accumulated in GroupConcatCtx.str with the SEPARATOR coming
/// before the EXPR value, except for the first entry which omits the
/// SEPARATOR.
#[derive(Default, Clone)]
pub struct GroupConcatCtx {
    /// The accumulated concatenation
    pub str: StrAccum,
    /// Number of strings presently concatenated
    pub nAccum: i32,
    /// Used to detect separator length change
    pub nFirstSepLength: i32,
    /// If Some, the inter-string separator lengths, as actually
    /// incorporated into presently accumulated result (nAccum-1 slots in
    /// use between method calls).  If None, nFirstSepLength is the length
    /// used throughout.
    pub pnSepLengths: Option<Vec<i32>>,
}

fn groupConcatStep(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    if sqlite3_value_type(&mut argv[0]) == SQLITE_NULL {
        return;
    }
    let szCtx = std::mem::size_of::<GroupConcatCtx>() as i32;
    if sqlite3_aggregate_context::<GroupConcatCtx>(context, szCtx).is_none() {
        return;
    }
    let limit = context.db.aLimit[SQLITE_LIMIT_LENGTH as usize];
    // Values of the arguments (computed before borrowing the context).
    let (zSep, nSep1) = if argc == 2 {
        let z = value_text(&mut argv[1]);
        let n = sqlite3_value_bytes(&mut argv[1]);
        (z, n)
    } else {
        (None, 0)
    };
    let zVal = value_text(&mut argv[0]);
    let nVal = sqlite3_value_bytes(&mut argv[0]);
    let pGCC = sqlite3_aggregate_context::<GroupConcatCtx>(context, szCtx).expect("agg");
    let firstTerm = pGCC.str.mxAlloc == 0;
    pGCC.str.mxAlloc = limit as u32;
    if argc == 1 {
        if !firstTerm {
            sqlite3_str_appendchar(&mut pGCC.str, 1, b',');
        } else {
            pGCC.nFirstSepLength = 1;
        }
    } else if !firstTerm {
        let mut nSep = nSep1;
        if let Some(zSep) = &zSep {
            sqlite3_str_append(&mut pGCC.str, zSep, nSep);
        } else {
            nSep = 0;
        }
        if nSep != pGCC.nFirstSepLength || pGCC.pnSepLengths.is_some() {
            let nAccum = pGCC.nAccum;
            let mut pnsl = match pGCC.pnSepLengths.take() {
                None => {
                    // First separator length variation seen, start tracking
                    // them.
                    let nA = (nAccum - 1).max(0) as usize;
                    vec![pGCC.nFirstSepLength; nA]
                }
                Some(v) => v,
            };
            if nAccum > 0 {
                pnsl.resize(nAccum as usize, 0);
                pnsl[nAccum as usize - 1] = nSep;
            }
            pGCC.pnSepLengths = Some(pnsl);
        }
    } else {
        pGCC.nFirstSepLength = nSep1;
    }
    pGCC.nAccum += 1;
    if let Some(zVal) = zVal {
        sqlite3_str_append(&mut pGCC.str, &zVal, nVal);
    }
}

fn groupConcatInverse(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1 || argv.len() == 2);
    if sqlite3_value_type(&mut argv[0]) == SQLITE_NULL {
        return;
    }
    let szCtx = std::mem::size_of::<GroupConcatCtx>() as i32;
    if sqlite3_aggregate_context::<GroupConcatCtx>(context, szCtx).is_none() {
        return;
    }
    // Must call sqlite3_value_text() to convert the argument into text prior
    // to invoking sqlite3_value_bytes(), in case the text encoding is UTF16
    let _ = sqlite3_value_text(&mut argv[0]);
    let mut nVS = sqlite3_value_bytes(&mut argv[0]); // Number of characters to remove
    let pGCC = sqlite3_aggregate_context::<GroupConcatCtx>(context, szCtx).expect("agg");
    pGCC.nAccum -= 1;
    if let Some(pnsl) = pGCC.pnSepLengths.as_mut() {
        debug_assert!(pGCC.nAccum >= 0);
        if pGCC.nAccum > 0 {
            nVS += pnsl[0];
            pnsl.remove(0);
            pnsl.truncate((pGCC.nAccum - 1).max(0) as usize);
        }
    } else {
        // If removing single accumulated string, harmlessly over-do.
        nVS += pGCC.nFirstSepLength;
    }
    let nChar = pGCC.str.zText.len() as i32;
    if nVS >= nChar {
        pGCC.str.zText.clear();
    } else {
        pGCC.str.zText.drain(0..nVS as usize);
    }
    if pGCC.str.zText.is_empty() {
        pGCC.str.mxAlloc = 0;
        pGCC.pnSepLengths = None;
    }
}

fn groupConcatFinalize(context: &mut Context) {
    let s = sqlite3_aggregate_context::<GroupConcatCtx>(context, 0).map(|g| {
        g.pnSepLengths = None;
        std::mem::take(&mut g.str)
    });
    if let Some(mut s) = s {
        sqlite3_result_str(context, &mut s, SQLITE_XFER);
    }
}

fn groupConcatValue(context: &mut Context) {
    let s = sqlite3_aggregate_context::<GroupConcatCtx>(context, 0).and_then(|g| {
        if g.nAccum > 0 {
            Some(g.str.clone())
        } else {
            None
        }
    });
    if let Some(mut s) = s {
        sqlite3_result_str(context, &mut s, SQLITE_COPY);
    }
}

/// C: `void sqlite3RegisterPerConnectionBuiltinFunctions(sqlite3 *db)`
///
/// This routine does per-connection function registration.  Most of the
/// built-in functions above are part of the global function set.  This
/// routine only deals with those that are not global.
pub fn sqlite3RegisterPerConnectionBuiltinFunctions(db: &mut Sqlite3) {
    let rc = crate::mainapi::sqlite3_overload_function(db, Some(b"MATCH"), 2);
    debug_assert!(rc == SQLITE_NOMEM || rc == SQLITE_OK);
    if rc == SQLITE_NOMEM {
        db.mallocFailed = 1;
    }
}

/// C: `void sqlite3RegisterLikeFunctions(sqlite3 *db, int caseSensitive)`
///
/// Re-register the built-in LIKE functions.  The caseSensitive parameter
/// determines whether or not the LIKE operator is case sensitive.
pub fn sqlite3RegisterLikeFunctions(db: &mut Sqlite3, caseSensitive: i32) {
    let (pInfo, flags) = if caseSensitive != 0 {
        (&likeInfoAlt, SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE)
    } else {
        (&likeInfoNorm, SQLITE_FUNC_LIKE)
    };
    for nArg in 2..=3 {
        let xSFunc: XSFunc = Rc::new(likeFunc);
        crate::mainapi::sqlite3CreateFunc(
            db,
            Some(b"like"),
            nArg,
            SQLITE_UTF8 as i32,
            FuncUserData::Int(pInfo.to_int()),
            Some(xSFunc),
            None,
            None,
            None,
            None,
            None,
        );
        let pDef = crate::callback::sqlite3FindFunction(db, Some(b"like"), nArg, SQLITE_UTF8, 0);
        // The sqlite3CreateFunc() call above cannot fail because the "like"
        // SQL-function already exists
        let pDef = pDef.expect("like() must exist");
        let mut d = (*pDef).clone();
        d.funcFlags |= flags;
        d.funcFlags &= !SQLITE_FUNC_UNSAFE;
        crate::callback::sqlite3FuncDefReplace(db, &pDef, d);
    }
}

/// C: `int sqlite3IsLikeFunction(sqlite3 *db, Expr *pExpr, int *pIsNocase, char *aWc)`
///
/// pExpr points to an expression which implements a function.  If it is
/// appropriate to apply the LIKE optimization to that function then set
/// aWc[0] through aWc[2] to the wildcard characters and the escape
/// character and then return TRUE.  aWc[3] is the ESCAPE character (0 if
/// none).
///
/// Rust: aWc is `&mut [u8]` of at least 4 bytes (was `Option<&[u8]>`).
pub fn sqlite3IsLikeFunction(
    db: &mut Sqlite3,
    pExpr: ExprId,
    pIsNocase: &mut i32,
    aWc: &mut [u8],
) -> i32 {
    let (pList, zToken) = {
        let e = db.ex(pExpr);
        debug_assert!(e.op == TK_FUNCTION);
        debug_assert!(e.UseXList());
        (e.x.pList, e.u.zToken.clone())
    };
    if pList.is_null() {
        return 0;
    }
    let nExpr = db.el(pList).nExpr();
    let pDef = crate::callback::sqlite3FindFunction(db, zToken.as_deref(), nExpr, SQLITE_UTF8, 0);
    let Some(pDef) = pDef else {
        return 0;
    };
    if (pDef.funcFlags & SQLITE_FUNC_LIKE) == 0 {
        return 0;
    }

    // The memcpy() statement assumes that the wildcard characters are the
    // first three statements in the compareInfo structure.
    let info = CompareInfo::from_int(pDef.pUserData.as_int());
    aWc[0] = info.matchAll;
    aWc[1] = info.matchOne;
    aWc[2] = info.matchSet;

    if nExpr < 3 {
        aWc[3] = 0;
    } else {
        let pEscape = db.el(pList).a[2].pExpr;
        let e = db.ex(pEscape);
        if e.op != TK_STRING {
            return 0;
        }
        debug_assert!(!e.HasProperty(EP_IntValue));
        let zEscape = e.token();
        if at(zEscape, 0) == 0 || at(zEscape, 1) != 0 {
            return 0;
        }
        if zEscape[0] == aWc[0] {
            return 0;
        }
        if zEscape[0] == aWc[1] {
            return 0;
        }
        aWc[3] = zEscape[0];
    }

    *pIsNocase = ((pDef.funcFlags & SQLITE_FUNC_CASE) == 0) as i32;
    1
}

// Mathematical Constants
const M_PI: f64 = std::f64::consts::PI;

/// The libm function of a 1-argument math function (its sqlite3_user_data).
#[derive(Clone, Copy)]
struct MathFunc1(fn(f64) -> f64);
/// The libm function of a 2-argument math function.
#[derive(Clone, Copy)]
struct MathFunc2(fn(f64, f64) -> f64);

fn user_math1(context: &mut Context) -> fn(f64) -> f64 {
    match sqlite3_user_data(context) {
        FuncUserData::Any(a) => a.downcast_ref::<MathFunc1>().expect("math1").0,
        _ => panic!("math function without libm pointer"),
    }
}

fn user_math2(context: &mut Context) -> fn(f64, f64) -> f64 {
    match sqlite3_user_data(context) {
        FuncUserData::Any(a) => a.downcast_ref::<MathFunc2>().expect("math2").0,
        _ => panic!("math function without libm pointer"),
    }
}

/// Implementation SQL functions: ceil(X), ceiling(X), floor(X), trunc(X).
/// The sqlite3_user_data() pointer is a pointer to the libm implementation
/// of the underlying C function.
fn ceilingFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    match sqlite3_value_numeric_type(context.db, &mut argv[0]) {
        SQLITE_INTEGER => {
            let v = sqlite3_value_int64(&mut argv[0]);
            sqlite3_result_int64(context, v);
        }
        SQLITE_FLOAT => {
            let x = user_math1(context);
            let v = sqlite3_value_double(&mut argv[0]);
            sqlite3_result_double(context, x(v));
        }
        _ => {}
    }
}

fn xCeil(x: f64) -> f64 {
    x.ceil()
}
fn xFloor(x: f64) -> f64 {
    x.floor()
}
fn xTrunc(x: f64) -> f64 {
    x.trunc()
}

/// Implementation of SQL functions: ln(X), log(X), log10(X), log2(X),
/// log(B,X).
fn logFunc(context: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    let mut x: f64;
    match sqlite3_value_numeric_type(context.db, &mut argv[0]) {
        SQLITE_INTEGER | SQLITE_FLOAT => {
            x = sqlite3_value_double(&mut argv[0]);
            if x <= 0.0 {
                return;
            }
        }
        _ => return,
    }
    let ans;
    if argc == 2 {
        match sqlite3_value_numeric_type(context.db, &mut argv[0]) {
            SQLITE_INTEGER | SQLITE_FLOAT => {
                let b = x.ln();
                if b <= 0.0 {
                    return;
                }
                x = sqlite3_value_double(&mut argv[1]);
                if x <= 0.0 {
                    return;
                }
                ans = x.ln() / b;
            }
            _ => return,
        }
    } else {
        ans = match sqlite3_user_data(context).as_int() {
            1 => x.log10(),
            2 => x.log2(),
            _ => x.ln(),
        };
    }
    sqlite3_result_double(context, ans);
}

/// Functions to converts degrees to radians and radians to degrees.
fn degToRad(x: f64) -> f64 {
    x * (M_PI / 180.0)
}
fn radToDeg(x: f64) -> f64 {
    x * (180.0 / M_PI)
}

/// Implementation of 1-argument SQL math functions: exp(X) ...
fn math1Func(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let type0 = sqlite3_value_numeric_type(context.db, &mut argv[0]);
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let v0 = sqlite3_value_double(&mut argv[0]);
    let x = user_math1(context);
    let ans = x(v0);
    sqlite3_result_double(context, ans);
}

/// Implementation of 2-argument SQL math functions: power(X,Y) ...
fn math2Func(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 2);
    let type0 = sqlite3_value_numeric_type(context.db, &mut argv[0]);
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let type1 = sqlite3_value_numeric_type(context.db, &mut argv[1]);
    if type1 != SQLITE_INTEGER && type1 != SQLITE_FLOAT {
        return;
    }
    let v0 = sqlite3_value_double(&mut argv[0]);
    let v1 = sqlite3_value_double(&mut argv[1]);
    let x = user_math2(context);
    let ans = x(v0, v1);
    sqlite3_result_double(context, ans);
}

/// Implementation of 0-argument pi() function.
fn piFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.is_empty());
    sqlite3_result_double(context, M_PI);
}

/// Implementation of sign(X) function.
fn signFunc(context: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 1);
    let type0 = sqlite3_value_numeric_type(context.db, &mut argv[0]);
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let x = sqlite3_value_double(&mut argv[0]);
    sqlite3_result_int(
        context,
        if x < 0.0 {
            -1
        } else if x > 0.0 {
            1
        } else {
            0
        },
    );
}

// ---------------------------------------------------------------------------
// The table of built-in functions (aBuiltinFunc[])
// ---------------------------------------------------------------------------

type SFn = fn(&mut Context, &mut [Mem]);
type FFn = fn(&mut Context);

fn mkdef(
    zName: &str,
    nArg: i32,
    funcFlags: u32,
    pUserData: FuncUserData,
    xSFunc: Option<SFn>,
    xFinal: Option<FFn>,
    xValue: Option<FFn>,
    xInverse: Option<SFn>,
) -> FuncDef {
    FuncDef {
        nArg: nArg as i16,
        funcFlags,
        pUserData,
        xSFunc: xSFunc.map(|f| Rc::new(f) as XSFunc),
        xFinalize: xFinal.map(|f| Rc::new(f) as XFinal),
        xValue: xValue.map(|f| Rc::new(f) as XFinal),
        xInverse: xInverse.map(|f| Rc::new(f) as XSFunc),
        zName: zName.as_bytes().to_vec(),
        pDestructor: None,
    }
}

#[inline]
fn ud(i: i64) -> FuncUserData {
    if i == 0 {
        FuncUserData::None
    } else {
        FuncUserData::Int(i)
    }
}

/// FUNCTION(zName, nArg, iArg, bNC, xFunc)
pub fn FUNCTION(zName: &str, nArg: i32, iArg: i64, bNC: u32, xFunc: SFn) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_CONSTANT
            | SQLITE_UTF8 as u32
            | (bNC * SQLITE_FUNC_NEEDCOLL),
        ud(iArg),
        Some(xFunc),
        None,
        None,
        None,
    )
}

/// VFUNCTION(zName, nArg, iArg, bNC, xFunc)
pub fn VFUNCTION(zName: &str, nArg: i32, iArg: i64, bNC: u32, xFunc: SFn) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32 | (bNC * SQLITE_FUNC_NEEDCOLL),
        ud(iArg),
        Some(xFunc),
        None,
        None,
        None,
    )
}

/// SFUNCTION(zName, nArg, iArg, bNC, xFunc)
pub fn SFUNCTION(zName: &str, nArg: i32, iArg: i64, _bNC: u32, xFunc: SFn) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32 | SQLITE_DIRECTONLY | SQLITE_FUNC_UNSAFE,
        ud(iArg),
        Some(xFunc),
        None,
        None,
        None,
    )
}

/// MFUNCTION(zName, nArg, xPtr, xFunc)
fn MFUNCTION(zName: &str, nArg: i32, xPtr: FuncUserData, xFunc: SFn) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_CONSTANT | SQLITE_UTF8 as u32,
        xPtr,
        Some(xFunc),
        None,
        None,
        None,
    )
}

fn m1(f: fn(f64) -> f64) -> FuncUserData {
    FuncUserData::Any(Rc::new(MathFunc1(f)))
}
fn m2(f: fn(f64, f64) -> f64) -> FuncUserData {
    FuncUserData::Any(Rc::new(MathFunc2(f)))
}

/// INLINE_FUNC(zName, nArg, iArg, mFlags)
pub fn INLINE_FUNC(zName: &str, nArg: i32, iArg: i32, mFlags: u32) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_UTF8 as u32
            | SQLITE_FUNC_INLINE
            | SQLITE_FUNC_CONSTANT
            | mFlags,
        ud(iArg as i64),
        Some(noopFunc),
        None,
        None,
        None,
    )
}

/// TEST_FUNC(zName, nArg, iArg, mFlags)
pub fn TEST_FUNC(zName: &str, nArg: i32, iArg: i32, mFlags: u32) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_UTF8 as u32
            | SQLITE_FUNC_INTERNAL
            | SQLITE_FUNC_TEST
            | SQLITE_FUNC_INLINE
            | SQLITE_FUNC_CONSTANT
            | mFlags,
        ud(iArg as i64),
        Some(noopFunc),
        None,
        None,
        None,
    )
}

/// DFUNCTION(zName, nArg, iArg, bNC, xFunc)
pub fn DFUNCTION(zName: &str, nArg: i32, _iArg: i64, _bNC: u32, xFunc: SFn) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_SLOCHNG | SQLITE_UTF8 as u32,
        FuncUserData::None,
        Some(xFunc),
        None,
        None,
        None,
    )
}

/// FUNCTION2(zName, nArg, iArg, bNC, xFunc, extraFlags)
pub fn FUNCTION2(zName: &str, nArg: i32, iArg: i64, bNC: u32, xFunc: SFn, extra: u32) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_CONSTANT
            | SQLITE_UTF8 as u32
            | (bNC * SQLITE_FUNC_NEEDCOLL)
            | extra,
        ud(iArg),
        Some(xFunc),
        None,
        None,
        None,
    )
}

/// LIKEFUNC(zName, nArg, arg, flags)
fn LIKEFUNC(zName: &str, nArg: i32, arg: &CompareInfo, flags: u32) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_FUNC_CONSTANT | SQLITE_UTF8 as u32 | flags,
        FuncUserData::Int(arg.to_int()),
        Some(likeFunc),
        None,
        None,
        None,
    )
}

/// WAGGREGATE(zName, nArg, arg, nc, xStep, xFinal, xValue, xInverse, f)
#[allow(clippy::too_many_arguments)]
pub fn WAGGREGATE(
    zName: &str,
    nArg: i32,
    arg: i64,
    nc: u32,
    xStep: SFn,
    xFinal: FFn,
    xValue: Option<FFn>,
    xInverse: Option<SFn>,
    f: u32,
) -> FuncDef {
    mkdef(
        zName,
        nArg,
        SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32 | (nc * SQLITE_FUNC_NEEDCOLL) | f,
        ud(arg),
        Some(xStep),
        Some(xFinal),
        xValue,
        xInverse,
    )
}

/// The FuncDef objects of the C aBuiltinFunc[] array of
/// sqlite3RegisterBuiltinFunctions(), in array order.
pub fn sqlite3BuiltinFunctionDefs() -> Vec<FuncDef> {
    vec![
        // ***** Functions only available with SQLITE_TESTCTRL_INTERNAL_FUNCTIONS *****
        TEST_FUNC("implies_nonnull_row", 2, INLINEFUNC_implies_nonnull_row, 0),
        TEST_FUNC("expr_compare", 2, INLINEFUNC_expr_compare, 0),
        TEST_FUNC("expr_implies_expr", 2, INLINEFUNC_expr_implies_expr, 0),
        TEST_FUNC("affinity", 1, INLINEFUNC_affinity, 0),
        // ***** Regular functions *****
        SFUNCTION("load_extension", 1, 0, 0, loadExt),
        SFUNCTION("load_extension", 2, 0, 0, loadExt),
        DFUNCTION("sqlite_compileoption_used", 1, 0, 0, compileoptionusedFunc),
        DFUNCTION("sqlite_compileoption_get", 1, 0, 0, compileoptiongetFunc),
        INLINE_FUNC("unlikely", 1, INLINEFUNC_unlikely, SQLITE_FUNC_UNLIKELY),
        INLINE_FUNC("likelihood", 2, INLINEFUNC_unlikely, SQLITE_FUNC_UNLIKELY),
        INLINE_FUNC("likely", 1, INLINEFUNC_unlikely, SQLITE_FUNC_UNLIKELY),
        FUNCTION("ltrim", 1, 1, 0, trimFunc),
        FUNCTION("ltrim", 2, 1, 0, trimFunc),
        FUNCTION("rtrim", 1, 2, 0, trimFunc),
        FUNCTION("rtrim", 2, 2, 0, trimFunc),
        FUNCTION("trim", 1, 3, 0, trimFunc),
        FUNCTION("trim", 2, 3, 0, trimFunc),
        FUNCTION("min", -3, 0, 1, minmaxFunc),
        WAGGREGATE(
            "min",
            1,
            0,
            1,
            minmaxStep,
            minMaxFinalize,
            Some(minMaxValue),
            None,
            SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
        ),
        FUNCTION("max", -3, 1, 1, minmaxFunc),
        WAGGREGATE(
            "max",
            1,
            1,
            1,
            minmaxStep,
            minMaxFinalize,
            Some(minMaxValue),
            None,
            SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
        ),
        FUNCTION2("typeof", 1, 0, 0, typeofFunc, SQLITE_FUNC_TYPEOF),
        FUNCTION2(
            "subtype",
            1,
            0,
            0,
            subtypeFunc,
            SQLITE_FUNC_TYPEOF | SQLITE_SUBTYPE,
        ),
        FUNCTION2("length", 1, 0, 0, lengthFunc, SQLITE_FUNC_LENGTH),
        FUNCTION2("octet_length", 1, 0, 0, bytelengthFunc, SQLITE_FUNC_BYTELEN),
        FUNCTION("instr", 2, 0, 0, instrFunc),
        FUNCTION("printf", -1, 0, 0, printfFunc),
        FUNCTION("format", -1, 0, 0, printfFunc),
        FUNCTION("unicode", 1, 0, 0, unicodeFunc),
        FUNCTION("char", -1, 0, 0, charFunc),
        FUNCTION("abs", 1, 0, 0, absFunc),
        FUNCTION("round", 1, 0, 0, roundFunc),
        FUNCTION("round", 2, 0, 0, roundFunc),
        FUNCTION("upper", 1, 0, 0, upperFunc),
        FUNCTION("lower", 1, 0, 0, lowerFunc),
        FUNCTION("hex", 1, 0, 0, hexFunc),
        FUNCTION("unhex", 1, 0, 0, unhexFunc),
        FUNCTION("unhex", 2, 0, 0, unhexFunc),
        FUNCTION("concat", -3, 0, 0, concatFunc),
        FUNCTION("concat_ws", -4, 0, 0, concatwsFunc),
        INLINE_FUNC("ifnull", 2, INLINEFUNC_coalesce, 0),
        VFUNCTION("random", 0, 0, 0, randomFunc),
        VFUNCTION("randomblob", 1, 0, 0, randomBlob),
        FUNCTION("nullif", 2, 0, 1, nullifFunc),
        DFUNCTION("sqlite_version", 0, 0, 0, versionFunc),
        DFUNCTION("sqlite_source_id", 0, 0, 0, sourceidFunc),
        SFUNCTION("sqlite_log", 2, 0, 0, errlogFunc),
        FUNCTION("unistr", 1, 0, 0, unistrFunc),
        FUNCTION("quote", 1, 0, 0, quoteFunc),
        FUNCTION("unistr_quote", 1, 1, 0, quoteFunc),
        VFUNCTION("last_insert_rowid", 0, 0, 0, last_insert_rowid),
        VFUNCTION("changes", 0, 0, 0, changes),
        VFUNCTION("total_changes", 0, 0, 0, total_changes),
        FUNCTION("replace", 3, 0, 0, replaceFunc),
        FUNCTION("zeroblob", 1, 0, 0, zeroblobFunc),
        FUNCTION("substr", 2, 0, 0, substrFunc),
        FUNCTION("substr", 3, 0, 0, substrFunc),
        FUNCTION("substring", 2, 0, 0, substrFunc),
        FUNCTION("substring", 3, 0, 0, substrFunc),
        WAGGREGATE(
            "sum",
            1,
            0,
            0,
            sumStep,
            sumFinalize,
            Some(sumFinalize),
            Some(sumInverse),
            0,
        ),
        WAGGREGATE(
            "total",
            1,
            0,
            0,
            sumStep,
            totalFinalize,
            Some(totalFinalize),
            Some(sumInverse),
            0,
        ),
        WAGGREGATE(
            "avg",
            1,
            0,
            0,
            sumStep,
            avgFinalize,
            Some(avgFinalize),
            Some(sumInverse),
            0,
        ),
        WAGGREGATE(
            "count",
            0,
            0,
            0,
            countStep,
            countFinalize,
            Some(countFinalize),
            Some(countInverse),
            SQLITE_FUNC_COUNT | SQLITE_FUNC_ANYORDER,
        ),
        WAGGREGATE(
            "count",
            1,
            0,
            0,
            countStep,
            countFinalize,
            Some(countFinalize),
            Some(countInverse),
            SQLITE_FUNC_ANYORDER,
        ),
        WAGGREGATE(
            "group_concat",
            1,
            0,
            0,
            groupConcatStep,
            groupConcatFinalize,
            Some(groupConcatValue),
            Some(groupConcatInverse),
            0,
        ),
        WAGGREGATE(
            "group_concat",
            2,
            0,
            0,
            groupConcatStep,
            groupConcatFinalize,
            Some(groupConcatValue),
            Some(groupConcatInverse),
            0,
        ),
        WAGGREGATE(
            "string_agg",
            2,
            0,
            0,
            groupConcatStep,
            groupConcatFinalize,
            Some(groupConcatValue),
            Some(groupConcatInverse),
            0,
        ),
        LIKEFUNC("glob", 2, &globInfo, SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE),
        LIKEFUNC("like", 2, &likeInfoNorm, SQLITE_FUNC_LIKE),
        LIKEFUNC("like", 3, &likeInfoNorm, SQLITE_FUNC_LIKE),
        // SQLITE_ENABLE_MATH_FUNCTIONS
        MFUNCTION("ceil", 1, m1(xCeil), ceilingFunc),
        MFUNCTION("ceiling", 1, m1(xCeil), ceilingFunc),
        MFUNCTION("floor", 1, m1(xFloor), ceilingFunc),
        MFUNCTION("trunc", 1, m1(xTrunc), ceilingFunc),
        FUNCTION("ln", 1, 0, 0, logFunc),
        FUNCTION("log", 1, 1, 0, logFunc),
        FUNCTION("log10", 1, 1, 0, logFunc),
        FUNCTION("log2", 1, 2, 0, logFunc),
        FUNCTION("log", 2, 0, 0, logFunc),
        MFUNCTION("exp", 1, m1(f64::exp), math1Func),
        MFUNCTION("pow", 2, m2(f64::powf), math2Func),
        MFUNCTION("power", 2, m2(f64::powf), math2Func),
        MFUNCTION("mod", 2, m2(fmod), math2Func),
        MFUNCTION("acos", 1, m1(f64::acos), math1Func),
        MFUNCTION("asin", 1, m1(f64::asin), math1Func),
        MFUNCTION("atan", 1, m1(f64::atan), math1Func),
        MFUNCTION("atan2", 2, m2(f64::atan2), math2Func),
        MFUNCTION("cos", 1, m1(f64::cos), math1Func),
        MFUNCTION("sin", 1, m1(f64::sin), math1Func),
        MFUNCTION("tan", 1, m1(f64::tan), math1Func),
        MFUNCTION("cosh", 1, m1(f64::cosh), math1Func),
        MFUNCTION("sinh", 1, m1(f64::sinh), math1Func),
        MFUNCTION("tanh", 1, m1(f64::tanh), math1Func),
        MFUNCTION("acosh", 1, m1(f64::acosh), math1Func),
        MFUNCTION("asinh", 1, m1(f64::asinh), math1Func),
        MFUNCTION("atanh", 1, m1(f64::atanh), math1Func),
        MFUNCTION("sqrt", 1, m1(f64::sqrt), math1Func),
        MFUNCTION("radians", 1, m1(degToRad), math1Func),
        MFUNCTION("degrees", 1, m1(radToDeg), math1Func),
        MFUNCTION("pi", 0, FuncUserData::None, piFunc),
        FUNCTION("sign", 1, 0, 0, signFunc),
        INLINE_FUNC("coalesce", -4, INLINEFUNC_coalesce, 0),
        INLINE_FUNC("iif", -4, INLINEFUNC_iif, 0),
        INLINE_FUNC("if", -4, INLINEFUNC_iif, 0),
    ]
}

/// C fmod()
fn fmod(x: f64, y: f64) -> f64 {
    x % y
}

/// C: `void sqlite3RegisterBuiltinFunctions(void)`
///
/// All of the FuncDef structures in the aBuiltinFunc[] array above to the
/// global function hash table.  This occurs at start-time (as a
/// consequence of a call to sqlite3_initialize()).
///
/// Rust: the global hash table (C sqlite3BuiltinFunctions) is passed
/// explicitly; it is built by sqlite3_initialize().  The registration
/// order is the C one: ALTER TABLE helpers, window functions, date/time
/// functions, JSON functions, then the aBuiltinFunc[] array.
pub fn sqlite3RegisterBuiltinFunctions(pHash: &mut crate::callback::FuncDefHash) {
    crate::callback::sqlite3InsertBuiltinFuncs(pHash, &crate::alter::sqlite3AlterFunctions());
    crate::window::sqlite3WindowFunctions(pHash);
    crate::date::sqlite3RegisterDateTimeFunctions(pHash);
    crate::json::sqlite3RegisterJsonFunctions(pHash);
    crate::callback::sqlite3InsertBuiltinFuncs(pHash, &sqlite3BuiltinFunctionDefs());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn like(p: &str, s: &str) -> bool {
        patternCompare(p.as_bytes(), s.as_bytes(), &likeInfoNorm, 0) == SQLITE_MATCH
    }
    fn likee(p: &str, s: &str, e: char) -> bool {
        patternCompare(p.as_bytes(), s.as_bytes(), &likeInfoNorm, e as u32) == SQLITE_MATCH
    }
    fn glob(p: &str, s: &str) -> bool {
        sqlite3_strglob(Some(p.as_bytes()), Some(s.as_bytes())) == 0
    }

    #[test]
    fn like_glob() {
        assert!(like("abc", "ABC"));
        assert!(like("a%", "abc"));
        assert!(like("%c", "abc"));
        assert!(like("a_c", "abc"));
        assert!(!like("a_c", "abbc"));
        assert!(like("%b%", "abc"));
        assert!(like("%", ""));
        assert!(!like("_", ""));
        assert!(like("é%", "éa"));
        assert!(!like("É", "é")); // case folding is ASCII-only
        assert!(likee("a\\%", "a%", '\\'));
        assert!(!likee("a\\%", "ab", '\\'));
        assert!(glob("a*", "abc"));
        assert!(!glob("A*", "abc"));
        assert!(glob("a?c", "abc"));
        assert!(glob("[a-c]x", "bx"));
        assert!(!glob("[^a-c]x", "bx"));
        assert!(glob("*[0-9]", "abc5"));
        assert!(glob("[]]", "]"));
        assert!(!glob("[a-c", "b"));
        assert_eq!(
            patternCompare(b"a%x", b"abc", &likeInfoNorm, 0),
            SQLITE_NOWILDCARDMATCH
        );
        assert_eq!(sqlite3_strlike(Some(b"A%"), Some(b"abc"), 0), 0);
    }

    #[test]
    fn compare_info_roundtrip() {
        for ci in [globInfo, likeInfoNorm, likeInfoAlt] {
            assert_eq!(CompareInfo::from_int(ci.to_int()), ci);
        }
    }

    #[test]
    fn builtin_table() {
        let defs = sqlite3BuiltinFunctionDefs();
        assert_eq!(defs.len(), 104);
        let n = defs.iter().filter(|d| d.zName == b"like").count();
        assert_eq!(n, 2);
        for d in defs.iter() {
            assert!(d.funcFlags & SQLITE_FUNC_BUILTIN != 0);
            assert!(d.xSFunc.is_some());
        }
    }
}
