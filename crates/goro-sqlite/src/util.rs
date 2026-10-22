//! Port of util.c: utility functions used throughout sqlite.
//!
//! Strings are byte slices.  Where the C code relies on a zero terminator
//! the Rust code treats the end of the slice (or an embedded NUL byte) as
//! the terminator.

use crate::global::{
    sqlite3Isdigit, sqlite3Isquote, sqlite3Isspace, sqlite3Isxdigit, sqlite3StdType,
    sqlite3UpperToLower as UpperToLower,
};
use crate::int::*;
use crate::printf::PfArg;

/// Decoded floating point value (sqlite3FpDecode()).
#[derive(Clone, Default, Debug)]
pub struct FpDecode {
    /// Number of significant digits in z
    pub n: i32,
    /// Location of the decimal point
    pub iDP: i32,
    /// The digits (ASCII), z.len() >= n
    pub z: Vec<u8>,
    /// b'+' or b'-'
    pub sign: u8,
    /// 1: Infinity, 2: NaN
    pub isSpecial: u8,
}

/// Byte `i` of a C string held in a slice: 0 past the end (the C zero
/// terminator).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// Signed-index variant of at().
#[inline]
fn ati(z: &[u8], i: isize) -> u8 {
    if i >= 0 && (i as usize) < z.len() {
        z[i as usize]
    } else {
        0
    }
}

/// C strlen() of a byte string held in a slice.
#[inline]
fn c_strlen(z: &[u8]) -> usize {
    z.iter().position(|&c| c == 0).unwrap_or(z.len())
}

/// C: `int sqlite3FaultSim(int iTest)`
///
/// There is no fault-simulation callback in this port: always SQLITE_OK.
pub fn sqlite3FaultSim(_iTest: i32) -> i32 {
    SQLITE_OK
}

/// C: `int sqlite3IsNaN(double x)`
pub fn sqlite3IsNaN(x: f64) -> i32 {
    x.is_nan() as i32
}

/// C: `int sqlite3IsOverflow(double x)`
///
/// Return true if the floating point value is NaN or +Inf or -Inf.
pub fn sqlite3IsOverflow(x: f64) -> i32 {
    let y = x.to_bits();
    ((y & 0x7ff0000000000000) == 0x7ff0000000000000) as i32
}

/// C: `int sqlite3Strlen30(const char *z)`
///
/// Compute a string length that is limited to what can be stored in lower
/// 30 bits of a 32-bit signed integer.  The length stops at the first NUL
/// byte (if any) or at the end of the slice.
pub fn sqlite3Strlen30(z: Option<&[u8]>) -> i32 {
    match z {
        None => 0,
        Some(z) => 0x3fffffff & (c_strlen(z) as i32),
    }
}

/// C: `char * sqlite3ColumnType(Column *pCol, char *zDflt)`
///
/// Return the declared type of a column.  Or return zDflt if the column
/// has no declared type.
pub fn sqlite3ColumnType(pCol: &Column, zDflt: Option<&[u8]>) -> Option<ZStr> {
    if (pCol.colFlags & COLFLAG_HASTYPE) != 0 {
        Some(pCol.zCnType.clone().unwrap_or_default())
    } else if pCol.eCType != 0 {
        debug_assert!(pCol.eCType as usize <= SQLITE_N_STDTYPE);
        Some(sqlite3StdType[pCol.eCType as usize - 1].to_vec())
    } else {
        zDflt.map(|z| z.to_vec())
    }
}

/// Helper function for sqlite3Error() - called rarely.
fn sqlite3ErrorFinish(db: &mut Sqlite3, err_code: i32) {
    if let Some(p) = db.pErr.as_deref_mut() {
        crate::vdbemem::sqlite3ValueSetNull(p);
    }
    sqlite3SystemError(db, err_code);
}

/// C: `void sqlite3Error(sqlite3 *db, int err_code)`
///
/// Set the current error code to err_code and clear any prior error
/// message.
pub fn sqlite3Error(db: &mut Sqlite3, err_code: i32) {
    db.errCode = err_code;
    if err_code != 0 || db.pErr.is_some() {
        sqlite3ErrorFinish(db, err_code);
    } else {
        db.errByteOffset = -1;
    }
}

/// C: `void sqlite3ErrorClear(sqlite3 *db)`
pub fn sqlite3ErrorClear(db: &mut Sqlite3) {
    db.errCode = SQLITE_OK;
    db.errByteOffset = -1;
    if let Some(p) = db.pErr.as_deref_mut() {
        crate::vdbemem::sqlite3ValueSetNull(p);
    }
}

/// C: `void sqlite3SystemError(sqlite3 *db, int rc)`
///
/// Load the sqlite3.iSysErrno field if that is an appropriate thing to do
/// based on the SQLite error code in rc.
///
/// Rust: the unix VFS xGetLastError() method returns `errno`; the VFS is
/// held in an `Rc` (no `&mut` access), so errno is read directly here.
pub fn sqlite3SystemError(db: &mut Sqlite3, rc: i32) {
    if rc == SQLITE_IOERR_NOMEM {
        return;
    }
    let rc = rc & 0xff;
    if rc == SQLITE_CANTOPEN || rc == SQLITE_IOERR {
        db.iSysErrno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    }
}

/// C: `void sqlite3ErrorWithMsg(sqlite3 *db, int err_code, const char *zFormat, ...)`
///
/// Set the most recent error code and error string for the sqlite handle
/// "db".
pub fn sqlite3ErrorWithMsg(
    db: &mut Sqlite3,
    err_code: i32,
    zFormat: Option<&[u8]>,
    args: &[PfArg],
) {
    db.errCode = err_code;
    sqlite3SystemError(db, err_code);
    match zFormat {
        None => sqlite3Error(db, err_code),
        Some(fmt) => {
            if db.pErr.is_none() {
                db.pErr = Some(Box::new(Mem::default()));
            }
            let z = crate::printf::sqlite3VMPrintf(db, fmt, args);
            if let Some(p) = db.pErr.as_deref_mut() {
                crate::vdbemem::sqlite3ValueSetStr(p, -1, &z, SQLITE_UTF8);
            }
        }
    }
}

/// C: `void sqlite3ProgressCheck(Parse *p)`
///
/// Check for interrupts and invoke progress callback.
pub fn sqlite3ProgressCheck(db: &mut Sqlite3) {
    if db.isInterrupted.get() != 0 {
        let p = db.pp();
        p.nErr += 1;
        p.rc = SQLITE_INTERRUPT;
    }
    if let Some(xProgress) = db.xProgress.clone() {
        let nOps = db.nProgressOps;
        if db.ppr().rc == SQLITE_INTERRUPT {
            db.pp().nProgressSteps = 0;
        } else {
            db.pp().nProgressSteps += 1;
            if db.ppr().nProgressSteps >= nOps {
                if xProgress() != 0 {
                    let p = db.pp();
                    p.nErr += 1;
                    p.rc = SQLITE_INTERRUPT;
                }
                db.pp().nProgressSteps = 0;
            }
        }
    }
}

/// C: `void sqlite3ErrorMsg(Parse *pParse, const char *zFormat, ...)`
///
/// Add an error message to pParse->zErrMsg and increment pParse->nErr.
/// The message is recorded in the current parse (db.pp()).
pub fn sqlite3ErrorMsg(db: &mut Sqlite3, zFormat: &[u8], args: &[PfArg]) {
    db.errByteOffset = -2;
    let zMsg = crate::printf::sqlite3VMPrintf(db, zFormat, args);
    if db.errByteOffset < -1 {
        db.errByteOffset = -1;
    }
    if db.suppressErr != 0 {
        if db.mallocFailed != 0 {
            let p = db.pp();
            p.nErr += 1;
            p.rc = SQLITE_NOMEM;
        }
    } else {
        let p = db.pp();
        p.nErr += 1;
        p.zErrMsg = Some(zMsg);
        p.rc = SQLITE_ERROR;
        p.pWith = WithId::NULL;
    }
}

/// C: `int sqlite3ErrorToParser(sqlite3 *db, int errCode)`
///
/// If database connection db is currently parsing SQL, then transfer error
/// code errCode to that parser if the parser has not already encountered
/// some other kind of error.
pub fn sqlite3ErrorToParser(db: &mut Sqlite3, errCode: i32) -> i32 {
    let Some(pParse) = db.parses.last_mut() else {
        return errCode;
    };
    pParse.rc = errCode;
    pParse.nErr += 1;
    errCode
}

/// C: `void sqlite3Dequote(char *z)`
///
/// Convert an SQL-style quoted string into a normal string by removing the
/// quote characters.  The conversion is done in-place.  If the input does
/// not begin with a quote character, then this routine is a no-op.
///
/// Rust: takes the string by `&mut Vec<u8>` (C works in place).
pub fn sqlite3Dequote(z: &mut Vec<u8>) {
    if z.is_empty() {
        return;
    }
    let mut quote = z[0];
    if !sqlite3Isquote(quote) {
        return;
    }
    if quote == b'[' {
        quote = b']';
    }
    let mut i = 1usize;
    let mut j = 0usize;
    loop {
        // C asserts z[i]!=0: the string is always terminated by the quote.
        if i >= z.len() || z[i] == 0 {
            break;
        }
        if z[i] == quote {
            if at(z, i + 1) == quote {
                z[j] = quote;
                j += 1;
                i += 1;
            } else {
                break;
            }
        } else {
            z[j] = z[i];
            j += 1;
        }
        i += 1;
    }
    z.truncate(j);
}

/// C: `void sqlite3DequoteExpr(Expr *p)`
pub fn sqlite3DequoteExpr(db: &mut Sqlite3, p: ExprId) {
    let e = db.exm(p);
    debug_assert!(!e.HasProperty(EP_IntValue));
    let first = e.u.zToken.as_deref().map(|z| at(z, 0)).unwrap_or(0);
    debug_assert!(sqlite3Isquote(first));
    e.flags |= if first == b'"' {
        EP_Quoted | EP_DblQuoted
    } else {
        EP_Quoted
    };
    if let Some(z) = e.u.zToken.as_mut() {
        sqlite3Dequote(z);
    }
}

/// C: `void sqlite3DequoteNumber(Parse *pParse, Expr *p)`
///
/// Expression p is a QNUMBER (quoted number).  Dequote the value in
/// p->u.zToken and set the type to INTEGER or FLOAT.
pub fn sqlite3DequoteNumber(db: &mut Sqlite3, p: ExprId) {
    if p.is_null() {
        return;
    }
    // Work on a zero-terminated copy, exactly as C does in place (the error
    // message shows the partially rewritten buffer, as in C).
    let mut buf: Vec<u8> = db.ex(p).u.zToken.clone().unwrap_or_default();
    buf.push(0);
    let bHex = buf[0] == b'0' && (at(&buf, 1) == b'x' || at(&buf, 1) == b'X');
    debug_assert!(db.ex(p).op == TK_QNUMBER);
    let mut op = TK_INTEGER;
    let mut pIn: usize = 0;
    let mut pOut: usize = 0;
    loop {
        let c = buf[pIn];
        if c != b'_' {
            buf[pOut] = c;
            pOut += 1;
            if c == b'e' || c == b'E' || c == b'.' {
                op = TK_FLOAT;
            }
        } else {
            let prev = if pIn > 0 { buf[pIn - 1] } else { 0 };
            let next = at(&buf, pIn + 1);
            if (!bHex && (!sqlite3Isdigit(prev) || !sqlite3Isdigit(next)))
                || (bHex && (!sqlite3Isxdigit(prev) || !sqlite3Isxdigit(next)))
            {
                let shown = buf[..c_strlen(&buf)].to_vec();
                errmsg!(db, "unrecognized token: \"%s\"", &shown);
            }
        }
        pIn += 1;
        if c == 0 {
            break;
        }
    }
    if bHex {
        op = TK_INTEGER;
    }
    let len = c_strlen(&buf);
    buf.truncate(len);
    let mut iValue = 0i32;
    let isInt = op == TK_INTEGER && sqlite3GetInt32(Some(&buf), &mut iValue) != 0;
    let e = db.exm(p);
    e.op = op;
    e.u.zToken = Some(buf);
    // tag-20240227-a: If after dequoting, the number is an integer that
    // fits in 32 bits, then it must be converted into EP_IntValue.
    if isInt {
        e.u.iValue = iValue;
        e.flags |= EP_IntValue;
    }
}

/// C: `void sqlite3DequoteToken(Token *p)`
///
/// If the input token p is quoted, try to adjust the token to remove the
/// quotes.  This is not always possible (interior quotes).
pub fn sqlite3DequoteToken(p: &mut Token) {
    if p.n < 2 {
        return;
    }
    if !sqlite3Isquote(p.at(0)) {
        return;
    }
    for i in 1..(p.n - 1) {
        if sqlite3Isquote(p.at(i as isize)) {
            return;
        }
    }
    p.n -= 2;
    p.off += 1;
}

/// C: `void sqlite3TokenInit(Token *p, char *z)`
///
/// Generate a Token object from a string.
pub fn sqlite3TokenInit(p: &mut Token, z: Option<&[u8]>) {
    match z {
        None => {
            p.z = None;
            p.off = 0;
            p.n = 0;
        }
        Some(s) => {
            let n = sqlite3Strlen30(Some(s)) as usize;
            *p = Token::from_bytes(&s[..n]);
        }
    }
}

/// C: `int sqlite3_stricmp(const char *zLeft, const char *zRight)`
pub fn sqlite3_stricmp(zLeft: Option<&[u8]>, zRight: Option<&[u8]>) -> i32 {
    match (zLeft, zRight) {
        (None, r) => {
            if r.is_some() {
                -1
            } else {
                0
            }
        }
        (Some(_), None) => 1,
        (l, r) => sqlite3StrICmp(l, r),
    }
}

/// C: `int sqlite3StrICmp(const char *zLeft, const char *zRight)`
pub fn sqlite3StrICmp(zLeft: Option<&[u8]>, zRight: Option<&[u8]>) -> i32 {
    let a = zLeft.unwrap_or(b"");
    let b = zRight.unwrap_or(b"");
    let mut i = 0usize;
    loop {
        let c = at(a, i) as i32;
        let x = at(b, i) as i32;
        if c == x {
            if c == 0 {
                return 0;
            }
        } else {
            let d = UpperToLower[c as usize] as i32 - UpperToLower[x as usize] as i32;
            if d != 0 {
                return d;
            }
        }
        i += 1;
    }
}

/// C: `int sqlite3_strnicmp(const char *zLeft, const char *zRight, int N)`
pub fn sqlite3_strnicmp(zLeft: Option<&[u8]>, zRight: Option<&[u8]>, N: i32) -> i32 {
    let (a, b) = match (zLeft, zRight) {
        (None, r) => return if r.is_some() { -1 } else { 0 },
        (Some(_), None) => return 1,
        (Some(a), Some(b)) => (a, b),
    };
    let mut N = N;
    let mut i = 0usize;
    loop {
        let go = N > 0;
        N -= 1;
        if !go
            || at(a, i) == 0
            || UpperToLower[at(a, i) as usize] != UpperToLower[at(b, i) as usize]
        {
            break;
        }
        i += 1;
    }
    if N < 0 {
        0
    } else {
        UpperToLower[at(a, i) as usize] as i32 - UpperToLower[at(b, i) as usize] as i32
    }
}

/// sqlite3StrNICmp (macro for sqlite3_strnicmp).
#[inline]
pub fn sqlite3StrNICmp(zLeft: Option<&[u8]>, zRight: Option<&[u8]>, N: i32) -> i32 {
    sqlite3_strnicmp(zLeft, zRight, N)
}

/// C: `u8 sqlite3StrIHash(const char *z)`
///
/// Compute an 8-bit hash on a string that is insensitive to case
/// differences.
pub fn sqlite3StrIHash(z: Option<&[u8]>) -> u8 {
    let Some(z) = z else { return 0 };
    let mut h: u8 = 0;
    for &c in z {
        if c == 0 {
            break;
        }
        h = h.wrapping_add(UpperToLower[c as usize]);
    }
    h
}

// ---------------------------------------------------------------------------
// Floating point <-> decimal conversion
// ---------------------------------------------------------------------------

/// Two inputs are multiplied to get a 128-bit result.  Write the lower
/// 64-bits of the result into *pLo, and return the high-order 64 bits.
#[inline]
fn sqlite3Multiply128(a: u64, b: u64, pLo: &mut u64) -> u64 {
    let r = (a as u128) * (b as u128);
    *pLo = r as u64;
    (r >> 64) as u64
}

/// A is an unsigned 96-bit integer formed by (a<<32)+aLo.  B is an
/// unsigned 64-bit integer.  Compute the upper 96 bits of 160-bit result
/// of A*B: write the middle 32 bits into *pLo and return the upper 64
/// bits.
#[inline]
fn sqlite3Multiply160(a: u64, aLo: u32, b: u64, pLo: &mut u32) -> u64 {
    let mut r = (a as u128) * (b as u128);
    r += ((aLo as u128) * (b as u128)) >> 32;
    *pLo = ((r >> 32) & 0xffffffff) as u32;
    (r >> 64) as u64
}

#[inline]
fn U64_BIT(n: i32) -> u64 {
    1u64 << n
}

const POWERSOF10_FIRST: i32 = -348;
const POWERSOF10_LAST: i32 = 347;

static aBase: [u64; 27] = [
    0x8000000000000000, // 0: 1.0e+0 << 63
    0xa000000000000000, // 1: 1.0e+1 << 60
    0xc800000000000000, // 2: 1.0e+2 << 57
    0xfa00000000000000, // 3: 1.0e+3 << 54
    0x9c40000000000000, // 4: 1.0e+4 << 50
    0xc350000000000000, // 5: 1.0e+5 << 47
    0xf424000000000000, // 6: 1.0e+6 << 44
    0x9896800000000000, // 7: 1.0e+7 << 40
    0xbebc200000000000, // 8: 1.0e+8 << 37
    0xee6b280000000000, // 9: 1.0e+9 << 34
    0x9502f90000000000, // 10: 1.0e+10 << 30
    0xba43b74000000000, // 11: 1.0e+11 << 27
    0xe8d4a51000000000, // 12: 1.0e+12 << 24
    0x9184e72a00000000, // 13: 1.0e+13 << 20
    0xb5e620f480000000, // 14: 1.0e+14 << 17
    0xe35fa931a0000000, // 15: 1.0e+15 << 14
    0x8e1bc9bf04000000, // 16: 1.0e+16 << 10
    0xb1a2bc2ec5000000, // 17: 1.0e+17 << 7
    0xde0b6b3a76400000, // 18: 1.0e+18 << 4
    0x8ac7230489e80000, // 19: 1.0e+19 >> 0
    0xad78ebc5ac620000, // 20: 1.0e+20 >> 3
    0xd8d726b7177a8000, // 21: 1.0e+21 >> 6
    0x878678326eac9000, // 22: 1.0e+22 >> 10
    0xa968163f0a57b400, // 23: 1.0e+23 >> 13
    0xd3c21bcecceda100, // 24: 1.0e+24 >> 16
    0x84595161401484a0, // 25: 1.0e+25 >> 20
    0xa56fa5b99019a5c8, // 26: 1.0e+26 >> 23
];
static aScale: [u64; 26] = [
    0x8049a4ac0c5811ae, // 0: 1.0e-351 << 1229
    0xcf42894a5dce35ea, // 1: 1.0e-324 << 1140
    0xa76c582338ed2621, // 2: 1.0e-297 << 1050
    0x873e4f75e2224e68, // 3: 1.0e-270 << 960
    0xda7f5bf590966848, // 4: 1.0e-243 << 871
    0xb080392cc4349dec, // 5: 1.0e-216 << 781
    0x8e938662882af53e, // 6: 1.0e-189 << 691
    0xe65829b3046b0afa, // 7: 1.0e-162 << 602
    0xba121a4650e4ddeb, // 8: 1.0e-135 << 512
    0x964e858c91ba2655, // 9: 1.0e-108 << 422
    0xf2d56790ab41c2a2, // 10: 1.0e-81 << 333
    0xc428d05aa4751e4c, // 11: 1.0e-54 << 243
    0x9e74d1b791e07e48, // 12: 1.0e-27 << 153
    0xcccccccccccccccc, // 13: 1.0e-1 << 67 (special case)
    0xcecb8f27f4200f3a, // 14: 1.0e+27 >> 26
    0xa70c3c40a64e6c51, // 15: 1.0e+54 >> 116
    0x86f0ac99b4e8dafd, // 16: 1.0e+81 >> 206
    0xda01ee641a708de9, // 17: 1.0e+108 >> 295
    0xb01ae745b101e9e4, // 18: 1.0e+135 >> 385
    0x8e41ade9fbebc27d, // 19: 1.0e+162 >> 475
    0xe5d3ef282a242e81, // 20: 1.0e+189 >> 564
    0xb9a74a0637ce2ee1, // 21: 1.0e+216 >> 654
    0x95f83d0a1fb69cd9, // 22: 1.0e+243 >> 744
    0xf24a01a73cf2dccf, // 23: 1.0e+270 >> 833
    0xc3b8358109e84f07, // 24: 1.0e+297 >> 923
    0x9e19db92b4e31ba9, // 25: 1.0e+324 >> 1013
];
static aScaleLo: [u32; 26] = [
    0x205b896d, // 0: 1.0e-351 << 1229
    0x52064cad, // 1: 1.0e-324 << 1140
    0xaf2af2b8, // 2: 1.0e-297 << 1050
    0x5a7744a7, // 3: 1.0e-270 << 960
    0xaf39a475, // 4: 1.0e-243 << 871
    0xbd8d794e, // 5: 1.0e-216 << 781
    0x547eb47b, // 6: 1.0e-189 << 691
    0x0cb4a5a3, // 7: 1.0e-162 << 602
    0x92f34d62, // 8: 1.0e-135 << 512
    0x3a6a07f9, // 9: 1.0e-108 << 422
    0xfae27299, // 10: 1.0e-81 << 333
    0xaa97e14c, // 11: 1.0e-54 << 243
    0x775ea265, // 12: 1.0e-27 << 153
    0xcccccccc, // 13: 1.0e-1 << 67 (special case)
    0x00000000, // 14: 1.0e+27 >> 26
    0x999090b6, // 15: 1.0e+54 >> 116
    0x69a028bb, // 16: 1.0e+81 >> 206
    0xe80e6f48, // 17: 1.0e+108 >> 295
    0x5ec05dd0, // 18: 1.0e+135 >> 385
    0x14588f14, // 19: 1.0e+162 >> 475
    0x8f1668c9, // 20: 1.0e+189 >> 564
    0x6d953e2c, // 21: 1.0e+216 >> 654
    0x4abdaf10, // 22: 1.0e+243 >> 744
    0xbc633b39, // 23: 1.0e+270 >> 833
    0x0a862f81, // 24: 1.0e+297 >> 923
    0x6c07a2c2, // 25: 1.0e+324 >> 1013
];

/// For any p between -348 and +347, return the most significant 64 bits
/// of pow(10,p) and write the next 32 significant bits into *pLo.
fn powerOfTen(p: i32, pLo: &mut u32) -> u64 {
    debug_assert!(p >= POWERSOF10_FIRST && p <= POWERSOF10_LAST);
    let mut g: i32;
    let mut n: i32;
    if p < 0 {
        if p == -1 {
            *pLo = aScaleLo[13];
            return aScale[13];
        }
        g = p / 27;
        n = p % 27;
        if n != 0 {
            g -= 1;
            n += 27;
        }
    } else if p < 27 {
        *pLo = 0;
        return aBase[p as usize];
    } else {
        g = p / 27;
        n = p % 27;
    }
    let s = aScale[(g + 13) as usize];
    if n == 0 {
        *pLo = aScaleLo[(g + 13) as usize];
        return s;
    }
    let mut lo: u32 = 0;
    let mut x = sqlite3Multiply160(s, aScaleLo[(g + 13) as usize], aBase[n as usize], &mut lo);
    if (U64_BIT(63) & x) == 0 {
        x = x << 1 | ((lo >> 31) & 1) as u64;
        lo = (lo << 1) | 1;
    }
    *pLo = lo;
    x
}

/// pwr10to2(x) computes floor(log2(pow(10,x))).
#[inline]
fn pwr10to2(p: i32) -> i32 {
    (p * 108853) >> 15
}
/// pwr2to10(y) computes floor(log10(pow(2,y))).
#[inline]
fn pwr2to10(p: i32) -> i32 {
    (p * 78913) >> 18
}

#[inline]
fn countLeadingZeros(m: u64) -> i32 {
    m.leading_zeros() as i32
}

/// Given m and e, which represent a quantity r == m*pow(2,e), return
/// values (D, P) such that r == D*pow(10,P), approximately.  D should
/// contain at least n significant digits.  m must have its highest bit set.
fn sqlite3Fp2Convert10(m: u64, e: i32, n: i32, pD: &mut u64, pP: &mut i32) {
    debug_assert!(n >= 1 && n <= 18);
    let p = n - 1 - pwr2to10(e + 63);
    let mut d2: u32 = 0;
    let mut d1: u64 = 0;
    let pw = powerOfTen(p, &mut d2);
    let mut h = sqlite3Multiply128(m, pw, &mut d1);
    debug_assert!(-(e + pwr10to2(p) + 2) >= 0);
    debug_assert!(-(e + pwr10to2(p) + 1) <= 63);
    if n == 18 {
        h >>= -(e + pwr10to2(p) + 2);
        *pD = (h + ((h << 1) & 2)) >> 1;
    } else {
        *pD = h >> -(e + pwr10to2(p) + 1);
    }
    *pP = -p;
}

/// Return an IEEE754 floating point value that approximates d*pow(10,p).
fn sqlite3Fp10Convert2(d: u64, p: i32) -> f64 {
    if p < POWERSOF10_FIRST {
        return 0.0;
    }
    if p > POWERSOF10_LAST {
        return f64::INFINITY;
    }
    let b = 64 - countLeadingZeros(d);
    let lp = pwr10to2(p);
    let mut e = 53 - b - lp;
    if e > 1074 {
        if e >= 1130 {
            return 0.0;
        }
        e = 1074;
    }
    let s = -(e - (64 - b) + lp + 3);
    let mut pwr10l: u32 = 0;
    let mut pwr10h = powerOfTen(p, &mut pwr10l);
    if pwr10l != 0 {
        pwr10h = pwr10h.wrapping_add(1);
        pwr10l = !pwr10l;
    }
    let x = if b == 64 { d } else { d << (64 - b) };
    let mut lo: u64 = 0;
    let mut hi = sqlite3Multiply128(x, pwr10h, &mut lo);
    let mid1 = (lo >> 32) as u32;
    let mut sticky: u64 = 1;
    if (hi & (U64_BIT(s).wrapping_sub(1))) == 0 {
        let mid2 = (sqlite3Multiply128(x, (pwr10l as u64) << 32, &mut lo) >> 32) as u32;
        sticky = (mid1.wrapping_sub(mid2) > 1) as u64;
        hi = hi.wrapping_sub((mid1 < mid2) as u64);
    }
    let mut u = (hi >> s) | sticky;
    let adj = (u >= U64_BIT(55) - 2) as i32;
    if adj != 0 {
        u = (u >> adj) | (u & 1);
        e -= adj;
    }
    let mut m = (u + 1 + ((u >> 2) & 1)) >> 2;
    if e <= -972 {
        return f64::INFINITY;
    }
    if (m & U64_BIT(52)) != 0 {
        m = (m & !U64_BIT(52)) | (((1075 - e) as u64) << 52);
    }
    f64::from_bits(m)
}

/// C: `int sqlite3AtoF(const char *zIn, double *pResult)`
///
/// The string z[] is an text representation of a real number.  Convert
/// this string to a double and write it into *pResult.
///
/// Return positive if the result is a valid real number (or integer) and
/// zero or negative if the string is empty or contains extraneous text.
/// Lower bits of the return value: bit 0 = some prefix is valid, bit 1 =
/// decimal point or exponent seen, bit 2 = exactly 0.0, bit 3 = more than
/// about 19 significant digits.
pub fn sqlite3AtoF(zIn: Option<&[u8]>, pResult: &mut f64) -> i32 {
    let z = zIn.unwrap_or(b"");
    let mut i = 0usize;
    let mut neg = false;
    let mut s: u64 = 0;
    let mut d: i32 = 0;
    let mut mState: i32 = 0;
    const LIM: u64 = (LARGEST_UINT64 - 9) / 10;

    // start_of_text:
    loop {
        let c0 = at(z, i);
        let mut v = c0.wrapping_sub(b'0') as u32;
        let mut parse_int = false;
        if v < 10 {
            parse_int = true;
        } else if c0 == b'-' {
            neg = true;
            i += 1;
            v = at(z, i).wrapping_sub(b'0') as u32;
            parse_int = v < 10;
        } else if c0 == b'+' {
            i += 1;
            v = at(z, i).wrapping_sub(b'0') as u32;
            parse_int = v < 10;
        } else if sqlite3Isspace(c0) {
            loop {
                i += 1;
                if !sqlite3Isspace(at(z, i)) {
                    break;
                }
            }
            continue;
        } else {
            s = 0;
        }
        if parse_int {
            // parse_integer_part:
            mState = 1;
            s = v as u64;
            i += 1;
            loop {
                let v = at(z, i).wrapping_sub(b'0') as u32;
                if v >= 10 {
                    break;
                }
                s = s * 10 + v as u64;
                i += 1;
                if s >= LIM {
                    mState = 9;
                    while sqlite3Isdigit(at(z, i)) {
                        i += 1;
                        d += 1;
                    }
                    break;
                }
            }
        }
        break;
    }

    // if decimal point is present
    if at(z, i) == b'.' {
        i += 1;
        if sqlite3Isdigit(at(z, i)) {
            mState |= 1;
            loop {
                if s < LIM {
                    s = s * 10 + (at(z, i) - b'0') as u64;
                    d -= 1;
                } else {
                    mState = 11;
                }
                i += 1;
                if !sqlite3Isdigit(at(z, i)) {
                    break;
                }
            }
        } else if mState == 0 {
            *pResult = 0.0;
            return 0;
        }
        mState |= 2;
    } else if mState == 0 {
        *pResult = 0.0;
        return 0;
    }

    // if exponent is present
    if at(z, i) == b'e' || at(z, i) == b'E' {
        let esign: i32;
        i += 1;
        // get sign of exponent
        if at(z, i) == b'-' {
            esign = -1;
            i += 1;
        } else {
            esign = 1;
            if at(z, i) == b'+' {
                i += 1;
            }
        }
        // copy digits to exponent
        let v = at(z, i).wrapping_sub(b'0') as u32;
        if v < 10 {
            let mut exp = v as i32;
            i += 1;
            mState |= 2;
            loop {
                let v = at(z, i).wrapping_sub(b'0') as u32;
                if v >= 10 {
                    break;
                }
                exp = if exp < 10000 {
                    exp * 10 + v as i32
                } else {
                    10000
                };
                i += 1;
            }
            d += esign * exp;
        } else {
            // Leave z[0] at 'e' or '+' or '-', so that the return is 0 or -1
            i -= 1;
        }
    }

    // Convert s*pow(10,d) into real
    if s == 0 {
        *pResult = 0.0;
        mState |= 4;
    } else {
        *pResult = sqlite3Fp10Convert2(s, d);
    }
    if neg {
        *pResult = -*pResult;
    }
    debug_assert!(sqlite3IsNaN(*pResult) == 0);

    // return true if number and no extra non-whitespace characters after
    if at(z, i) == 0 {
        return mState;
    }
    if sqlite3Isspace(at(z, i)) {
        loop {
            i += 1;
            if !sqlite3Isspace(at(z, i)) {
                break;
            }
        }
        if at(z, i) == 0 {
            return mState;
        }
    }
    (0xfffffff0u32 as i32) | mState
}

/// Digit pairs used to convert a U64 or I64 into text, two digits at a
/// time.
static sqlite3DigitPairs: &[u8; 200] = b"00010203040506070809\
10111213141516171819\
20212223242526272829\
30313233343536373839\
40414243444546474849\
50515253545556575859\
60616263646566676869\
70717273747576777879\
80818283848586878889\
90919293949596979899";

/// C: `int sqlite3Int64ToText(i64 v, char *zOut)`
///
/// Render a signed 64-bit integer as text.  Store the result in zOut[]
/// (followed by a zero terminator if there is room) and return the length
/// of the string, in bytes.  zOut[] should be at least 21 bytes.
///
/// Rust: zOut is `&mut [u8]` (was `Option<&[u8]>`).
pub fn sqlite3Int64ToText(v: i64, zOut: &mut [u8]) -> i32 {
    let t = sqlite3Int64ToTextVec(v);
    let n = t.len();
    zOut[..n].copy_from_slice(&t);
    if zOut.len() > n {
        zOut[n] = 0;
    }
    n as i32
}

/// sqlite3Int64ToText() returning the text as a new vector.
pub fn sqlite3Int64ToTextVec(v: i64) -> Vec<u8> {
    let mut x: u64;
    if v > 0 {
        x = v as u64;
    } else if v == 0 {
        return b"0".to_vec();
    } else {
        x = if v == SMALLEST_INT64 {
            1u64 << 63
        } else {
            (-v) as u64
        };
    }
    let mut a = [0u8; SQLITE_U64_DIGITS as usize + 1];
    let mut i = a.len() - 1;
    a[i] = 0;
    while x >= 10 {
        let kk = ((x % 100) * 2) as usize;
        a[i - 2] = sqlite3DigitPairs[kk];
        a[i - 1] = sqlite3DigitPairs[kk + 1];
        i -= 2;
        x /= 100;
    }
    if x != 0 {
        i -= 1;
        a[i] = x as u8 + b'0';
    }
    if v < 0 {
        i -= 1;
        a[i] = b'-';
    }
    a[i..a.len() - 1].to_vec()
}

/// Compare the 19-character string zNum against the text representation
/// value 2^63:  9223372036854775808.  Return negative, zero, or positive if
/// zNum is less than, equal to, or greater than the string.
fn compare2pow63(z: &[u8], start: isize, incr: isize) -> i32 {
    let mut c: i32 = 0;
    let pow63 = b"922337203685477580";
    let mut i = 0isize;
    while c == 0 && i < 18 {
        c = (ati(z, start + i * incr) as i32 - pow63[i as usize] as i32) * 10;
        i += 1;
    }
    if c == 0 {
        c = ati(z, start + 18 * incr) as i32 - b'8' as i32;
    }
    c
}

/// C: `int sqlite3Atoi64(const char *zNum, i64 *pNum, int length, u8 enc)`
///
/// Convert zNum to a 64-bit signed integer.  zNum must be decimal.
///
/// Returns: -1 not even a prefix looks like an integer; 0 success; 1
/// excess non-space text after the integer value; 2 integer too large or
/// malformed; 3 special case of 9223372036854775808.
///
/// length is the number of bytes in the string (the string is not
/// necessarily zero-terminated).
pub fn sqlite3Atoi64(zNum: Option<&[u8]>, pNum: &mut i64, length: i32, enc: u8) -> i32 {
    let z = zNum.unwrap_or(b"");
    let mut length = length as isize;
    let incr: isize;
    let mut u: u64 = 0;
    let mut neg = false;
    let mut c: u32 = 0;
    let mut nonNum = false;
    let mut p: isize = 0;
    let mut zEnd: isize = length;
    debug_assert!(enc == SQLITE_UTF8 || enc == SQLITE_UTF16LE || enc == SQLITE_UTF16BE);
    if enc == SQLITE_UTF8 {
        incr = 1;
    } else {
        incr = 2;
        length &= !1;
        let mut i = 3 - enc as isize;
        while i < length && ati(z, i) == 0 {
            i += 2;
        }
        nonNum = i < length;
        zEnd = i ^ 1;
        p += (enc & 1) as isize;
    }
    while p < zEnd && sqlite3Isspace(ati(z, p)) {
        p += incr;
    }
    if p < zEnd {
        if ati(z, p) == b'-' {
            neg = true;
            p += incr;
        } else if ati(z, p) == b'+' {
            p += incr;
        }
    }
    let zStart = p;
    while p < zEnd && ati(z, p) == b'0' {
        p += incr; // Skip leading zeros.
    }
    let mut i: isize = 0;
    while p + i < zEnd {
        c = (ati(z, p + i) as u32).wrapping_sub(b'0' as u32);
        if c > 9 {
            break;
        }
        u = u.wrapping_mul(10).wrapping_add(c as u64);
        i += incr;
    }
    if u > LARGEST_INT64 as u64 {
        *pNum = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
    } else if neg {
        *pNum = -(u as i64);
    } else {
        *pNum = u as i64;
    }
    let mut rc = 0;
    if i == 0 && zStart == p {
        // No digits
        rc = -1;
    } else if nonNum {
        // UTF16 with high-order bytes non-zero
        rc = 1;
    } else if p + i < zEnd {
        // Extra bytes at the end
        let mut jj = i;
        loop {
            if !sqlite3Isspace(ati(z, p + jj)) {
                rc = 1; // Extra non-space text after the integer
                break;
            }
            jj += incr;
            if p + jj >= zEnd {
                break;
            }
        }
    }
    let _ = c;
    if i < 19 * incr {
        // Less than 19 digits, so we know that it fits in 64 bits
        debug_assert!(u <= LARGEST_INT64 as u64);
        rc
    } else {
        // zNum is a 19-digit numbers.  Compare it against 9223372036854775808.
        let j = if i > 19 * incr {
            1
        } else {
            compare2pow63(z, p, incr)
        };
        if j < 0 {
            // zNum is less than 9223372036854775808 so it fits
            debug_assert!(u <= LARGEST_INT64 as u64);
            rc
        } else {
            *pNum = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
            if j > 0 {
                // zNum is greater than 9223372036854775808 so it overflows
                2
            } else {
                // zNum is exactly 9223372036854775808.  Fits if negative.
                // The special case 2 overflow if positive
                debug_assert!(u - 1 == LARGEST_INT64 as u64);
                if neg { rc } else { 3 }
            }
        }
    }
}

/// C: `int sqlite3DecOrHexToI64(const char *z, i64 *pOut)`
///
/// Transform a UTF-8 integer literal, in either decimal or hexadecimal,
/// into a 64-bit signed integer.  Returns 0 success, 1 excess text after
/// the integer value, 2 too large or malformed, 3 special case of
/// 9223372036854775808.
pub fn sqlite3DecOrHexToI64(z: Option<&[u8]>, pOut: &mut i64) -> i32 {
    let z = z.unwrap_or(b"");
    if at(z, 0) == b'0' && (at(z, 1) == b'x' || at(z, 1) == b'X') {
        let mut u: u64 = 0;
        let mut i = 2usize;
        while at(z, i) == b'0' {
            i += 1;
        }
        let mut k = i;
        while sqlite3Isxdigit(at(z, k)) {
            u = u
                .wrapping_mul(16)
                .wrapping_add(sqlite3HexToInt(at(z, k) as i32) as u64);
            k += 1;
        }
        *pOut = u as i64;
        if k - i > 16 {
            return 2;
        }
        if at(z, k) != 0 {
            return 1;
        }
        0
    } else {
        let accept = b"+- \n\t0123456789";
        let mut n = 0usize;
        while n < z.len() && z[n] != 0 && accept.contains(&z[n]) {
            n += 1;
        }
        let mut n = (n & 0x3fffffff) as i32;
        if at(z, n as usize) != 0 {
            n += 1;
        }
        sqlite3Atoi64(Some(z), pOut, n, SQLITE_UTF8)
    }
}

/// C: `int sqlite3GetInt32(const char *zNum, int *pValue)`
///
/// If zNum represents an integer that will fit in 32-bits, then set
/// *pValue to that integer and return true.  Otherwise return false.
/// Accepts decimal and hexadecimal.  Any non-numeric characters that
/// follow zNum are ignored.
pub fn sqlite3GetInt32(zNum: Option<&[u8]>, pValue: &mut i32) -> i32 {
    let z = zNum.unwrap_or(b"");
    let mut p = 0usize;
    let mut v: i64 = 0;
    let mut neg = false;
    if at(z, 0) == b'-' {
        neg = true;
        p += 1;
    } else if at(z, 0) == b'+' {
        p += 1;
    } else if at(z, 0) == b'0'
        && (at(z, 1) == b'x' || at(z, 1) == b'X')
        && sqlite3Isxdigit(at(z, 2))
    {
        let mut u: u32 = 0;
        p += 2;
        while at(z, p) == b'0' {
            p += 1;
        }
        let mut i = 0usize;
        while i < 8 && sqlite3Isxdigit(at(z, p + i)) {
            u = u * 16 + sqlite3HexToInt(at(z, p + i) as i32) as u32;
            i += 1;
        }
        if (u & 0x80000000) == 0 && !sqlite3Isxdigit(at(z, p + i)) {
            *pValue = u as i32;
            return 1;
        } else {
            return 0;
        }
    }
    if !sqlite3Isdigit(at(z, p)) {
        return 0;
    }
    while at(z, p) == b'0' {
        p += 1;
    }
    let mut i = 0usize;
    while i < 11 {
        let c = at(z, p + i) as i32 - b'0' as i32;
        if !(0..=9).contains(&c) {
            break;
        }
        v = v * 10 + c as i64;
        i += 1;
    }
    // The longest decimal representation of a 32 bit integer is 10 digits
    if i > 10 {
        return 0;
    }
    if v - neg as i64 > 2147483647 {
        return 0;
    }
    if neg {
        v = -v;
    }
    *pValue = v as i32;
    1
}

/// C: `int sqlite3Atoi(const char *z)`
///
/// Return a 32-bit integer value extracted from a string.  If the string
/// is not an integer, just return 0.
pub fn sqlite3Atoi(z: Option<&[u8]>) -> i32 {
    let mut x = 0;
    sqlite3GetInt32(z, &mut x);
    x
}

/// C: `void sqlite3FpDecode(FpDecode *p, double r, int iRound, int mxRound)`
///
/// Decode a floating-point value into an approximate decimal
/// representation.  If iRound<=0 then round to -iRound significant digits
/// to the right of the decimal point, or to a maximum of mxRound total
/// significant digits.  If iRound>0 round to min(iRound,mxRound)
/// significant digits total.  mxRound must be positive.
pub fn sqlite3FpDecode(p: &mut FpDecode, r: f64, iRound: i32, mxRound: i32) {
    let mut r = r;
    let mut iRound = iRound;
    p.isSpecial = 0;
    debug_assert!(mxRound > 0);

    // Convert negative numbers to positive.  Deal with Infinity, 0.0, and
    // NaN.
    if r < 0.0 {
        p.sign = b'-';
        r = -r;
    } else if r == 0.0 {
        p.sign = b'+';
        p.n = 1;
        p.iDP = 1;
        p.z = b"0".to_vec();
        return;
    } else {
        p.sign = b'+';
    }
    let mut v = r.to_bits();
    let mut e = ((v >> 52) & 0x7ff) as i32;
    if e == 0x7ff {
        p.isSpecial = 1 + (v != 0x7ff0000000000000) as u8;
        p.n = 0;
        p.iDP = 0;
        p.z = Vec::new();
        return;
    }
    v &= 0x000fffffffffffff;
    if e == 0 {
        let nn = countLeadingZeros(v);
        v <<= nn;
        e = -1074 - nn;
    } else {
        v = (v << 11) | U64_BIT(63);
        e -= 1086;
    }
    let mut exp: i32 = 0;
    let nd = if iRound <= 0 || iRound >= 18 {
        18
    } else {
        iRound + 1
    };
    let v0 = v;
    sqlite3Fp2Convert10(v0, e, nd, &mut v, &mut exp);

    // Extract significant digits, start at the right-most slot in zBuf and
    // working back to the right.
    let mut zBuf = [0u8; SQLITE_U64_DIGITS as usize + 1];
    debug_assert!(v > 0);
    let mut i = SQLITE_U64_DIGITS as usize;
    while v >= 10 {
        let kk = ((v % 100) * 2) as usize;
        zBuf[i - 2] = sqlite3DigitPairs[kk];
        zBuf[i - 1] = sqlite3DigitPairs[kk + 1];
        i -= 2;
        v /= 100;
    }
    if v != 0 {
        i -= 1;
        zBuf[i] = v as u8 + b'0';
    }
    let mut n = SQLITE_U64_DIGITS - i as i32; // Total number of digits extracted
    debug_assert!(n > 0);
    p.iDP = n + exp;
    if iRound <= 0 {
        iRound = p.iDP - iRound;
        if iRound == 0 && zBuf[i] >= b'5' {
            iRound = 1;
            i -= 1;
            zBuf[i] = b'0';
            n += 1;
            p.iDP += 1;
        }
    }
    let mut z = i; // z points to the first digit
    if iRound > 0 && (iRound < n || n > mxRound) {
        if iRound > mxRound {
            iRound = mxRound;
        }
        if iRound == 17 {
            // If the precision is exactly 17, which only happens with the
            // "!" flag (ex: "%!.17g") then try to reduce the precision if
            // that yields text that will round-trip to the original
            // floating-point value.
            let zz = |k: usize| zBuf[z + k];
            if zz(15) == b'9' && zz(14) == b'9' {
                let mut jj = 14usize;
                while jj > 0 && zz(jj - 1) == b'9' {
                    jj -= 1;
                }
                let v2: u64 = if jj == 0 {
                    1
                } else {
                    let mut v2 = (zz(0) - b'0') as u64;
                    for kk in 1..jj {
                        v2 = (v2 * 10) + (zz(kk) - b'0') as u64;
                    }
                    v2 + 1
                };
                if r == sqlite3Fp10Convert2(v2, exp + n - jj as i32) {
                    iRound = jj as i32 + 1;
                }
            } else if p.iDP >= n || (zz(15) == b'0' && zz(14) == b'0' && zz(13) == b'0') {
                debug_assert!(zz(0) != b'0');
                let mut jj = 13usize;
                while jj > 0 && zz(jj - 1) == b'0' {
                    jj -= 1;
                }
                let mut v2 = (zz(0) - b'0') as u64;
                for kk in 1..jj {
                    v2 = (v2 * 10) + (zz(kk) - b'0') as u64;
                }
                if r == sqlite3Fp10Convert2(v2, exp + n - jj as i32) {
                    iRound = jj as i32 + 1;
                }
            }
        }
        n = iRound;
        if zBuf[z + iRound as usize] >= b'5' {
            let mut j = iRound as usize - 1;
            loop {
                zBuf[z + j] += 1;
                if zBuf[z + j] <= b'9' {
                    break;
                }
                zBuf[z + j] = b'0';
                if j == 0 {
                    z -= 1;
                    zBuf[z] = b'1';
                    n += 1;
                    p.iDP += 1;
                    break;
                } else {
                    j -= 1;
                }
            }
        }
    }
    debug_assert!(n > 0);
    while zBuf[z + n as usize - 1] == b'0' {
        n -= 1;
        debug_assert!(n > 0);
    }
    p.n = n;
    p.z = zBuf[z..z + n as usize].to_vec();
}

/// C: `int sqlite3GetUInt32(const char *z, u32 *pI)`
///
/// Try to convert z into an unsigned 32-bit integer.  Return true on
/// success and false if there is an error.  Only decimal notation is
/// accepted.
pub fn sqlite3GetUInt32(z: Option<&[u8]>, pI: &mut u32) -> i32 {
    let z = z.unwrap_or(b"");
    let mut v: u64 = 0;
    let mut i = 0usize;
    while sqlite3Isdigit(at(z, i)) {
        v = v * 10 + (z[i] - b'0') as u64;
        if v > 4294967295 {
            *pI = 0;
            return 0;
        }
        i += 1;
    }
    if i == 0 || at(z, i) != 0 {
        *pI = 0;
        return 0;
    }
    *pI = v as u32;
    1
}

// ---------------------------------------------------------------------------
// Varints
// ---------------------------------------------------------------------------

fn putVarint64(p: &mut [u8], mut v: u64) -> i32 {
    if (v & ((0xff000000u64) << 32)) != 0 {
        p[8] = v as u8;
        v >>= 8;
        for i in (0..=7).rev() {
            p[i] = ((v & 0x7f) | 0x80) as u8;
            v >>= 7;
        }
        return 9;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    loop {
        buf[n] = ((v & 0x7f) | 0x80) as u8;
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    buf[0] &= 0x7f;
    debug_assert!(n <= 9);
    for (i, j) in (0..n).rev().enumerate() {
        p[i] = buf[j];
    }
    n as i32
}

/// C: `int sqlite3PutVarint(unsigned char *p, u64 v)`
///
/// Write a 64-bit variable-length integer to memory starting at p[0].
/// The length of data write will be between 1 and 9 bytes.  The number of
/// bytes written is returned.
pub fn sqlite3PutVarint(p: &mut [u8], v: u64) -> i32 {
    if v <= 0x7f {
        p[0] = (v & 0x7f) as u8;
        return 1;
    }
    if v <= 0x3fff {
        p[0] = (((v >> 7) & 0x7f) | 0x80) as u8;
        p[1] = (v & 0x7f) as u8;
        return 2;
    }
    putVarint64(p, v)
}

/// Shared body of sqlite3GetVarint() and sqlite3VarintValue(): returns
/// (value, number of bytes).  Bytes past the end of the slice read as 0.
#[inline]
fn getVarint(p: &[u8]) -> (u64, u8) {
    let b = |k: usize| -> u64 { at(p, k) as u64 };
    let mut iKey: u64 = b(0);
    let mut k = 0usize;
    if iKey >= 0x80 {
        k += 1;
        let mut x = b(k);
        iKey = (iKey << 7) ^ x;
        if x >= 0x80 {
            k += 1;
            x = b(k);
            iKey = (iKey << 7) ^ x;
            if x >= 0x80 {
                k += 1;
                x = b(k);
                iKey = (iKey << 7) ^ 0x10204000 ^ x;
                if x >= 0x80 {
                    k += 1;
                    x = b(k);
                    iKey = (iKey << 7) ^ 0x4000 ^ x;
                    if x >= 0x80 {
                        k += 1;
                        x = b(k);
                        iKey = (iKey << 7) ^ 0x4000 ^ x;
                        if x >= 0x80 {
                            k += 1;
                            x = b(k);
                            iKey = (iKey << 7) ^ 0x4000 ^ x;
                            if x >= 0x80 {
                                k += 1;
                                x = b(k);
                                iKey = (iKey << 7) ^ 0x4000 ^ x;
                                if x >= 0x80 {
                                    k += 1;
                                    iKey = (iKey << 8) ^ 0x8000 ^ b(k);
                                }
                            }
                        }
                    }
                }
            } else {
                iKey ^= 0x204000;
            }
        } else {
            iKey ^= 0x4000;
        }
    }
    (iKey, (k + 1) as u8)
}

/// C: `u8 sqlite3GetVarint(const unsigned char *p, u64 *v)`
///
/// Read a 64-bit variable-length integer from memory starting at p[0].
/// Return the number of bytes read.  The value is stored in *v.
pub fn sqlite3GetVarint(p: &[u8], v: &mut u64) -> u8 {
    let (x, n) = getVarint(p);
    *v = x;
    n
}

/// C: `i64 sqlite3VarintValue(const unsigned char *p)`
pub fn sqlite3VarintValue(p: &[u8]) -> i64 {
    getVarint(p).0 as i64
}

/// C: `u8 sqlite3GetVarint32(const unsigned char *p, u32 *v)`
///
/// Read a 32-bit variable-length integer from memory starting at p[0].
/// Return the number of bytes read.  The value is stored in *v.  If the
/// varint is larger than can fit in a 32-bit unsigned integer, set *v to
/// 0xffffffff.
///
/// Rust: also handles the single-byte case (the C getVarint32() macro), so
/// it can be called directly.
pub fn sqlite3GetVarint32(p: &[u8], v: &mut u32) -> u8 {
    let p0 = at(p, 0) as u32;
    if p0 < 0x80 {
        *v = p0;
        return 1;
    }
    let p1 = at(p, 1) as u32;
    if (p1 & 0x80) == 0 {
        // This is the two-byte case
        *v = ((p0 & 0x7f) << 7) | p1;
        return 2;
    }
    let p2 = at(p, 2) as u32;
    if (p2 & 0x80) == 0 {
        // This is the three-byte case
        *v = ((p0 & 0x7f) << 14) | ((p1 & 0x7f) << 7) | p2;
        return 3;
    }
    // four or more bytes
    let (v64, n) = getVarint(p);
    debug_assert!(n > 3 && n <= 9);
    if (v64 & SQLITE_MAX_U32 as u64) != v64 {
        *v = 0xffffffff;
    } else {
        *v = v64 as u32;
    }
    n
}

/// getVarint32(A,B) macro: returns the number of bytes read.
#[inline]
pub fn getVarint32(p: &[u8], v: &mut u32) -> u8 {
    sqlite3GetVarint32(p, v)
}

/// C: `int sqlite3VarintLen(u64 v)`
///
/// Return the number of bytes that will be needed to store the given
/// 64-bit integer.
pub fn sqlite3VarintLen(mut v: u64) -> i32 {
    let mut i = 1;
    loop {
        v >>= 7;
        if v == 0 {
            break;
        }
        debug_assert!(i < 10);
        i += 1;
    }
    i
}

/// C: `u32 sqlite3Get4byte(const u8 *p)`
pub fn sqlite3Get4byte(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

/// C: `void sqlite3Put4byte(unsigned char *p, u32 v)`
pub fn sqlite3Put4byte(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_be_bytes());
}

/// C: `SQLITE_OPT_INLINE u64 sqlite3Get8byte(const u8 *p)`
pub fn sqlite3Get8byte(p: &[u8]) -> u64 {
    u64::from_be_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

/// C: `SQLITE_OPT_INLINE u64 sqlite3BSwap64(u64 x)`
pub fn sqlite3BSwap64(x: u64) -> u64 {
    x.swap_bytes()
}

/// C: `u8 sqlite3HexToInt(int h)`
///
/// Translate a single byte of Hex into an integer.  This routine only
/// works if h really is a valid hexadecimal character:  0..9a..fA..F
pub fn sqlite3HexToInt(h: i32) -> u8 {
    let mut h = h;
    h += 9 * (1 & (h >> 6));
    (h & 0xf) as u8
}

/// C: `void * sqlite3HexToBlob(sqlite3 *db, const char *z, int n)`
///
/// Convert a BLOB literal of the form "x'hhhhhh'" into its binary value.
/// z[0..n] holds the hex digits (n is even in practice).
///
/// Rust: the returned vector holds exactly the decoded bytes (C appends a
/// zero terminator).
pub fn sqlite3HexToBlob(_db: &mut Sqlite3, z: Option<&[u8]>, n: i32) -> Option<Vec<u8>> {
    let z = z.unwrap_or(b"");
    let mut zBlob: Vec<u8> = Vec::with_capacity((n / 2 + 1).max(0) as usize);
    let n = n - 1;
    let mut i = 0i32;
    while i < n {
        zBlob.push(
            (sqlite3HexToInt(at(z, i as usize) as i32) << 4)
                | sqlite3HexToInt(at(z, i as usize + 1) as i32),
        );
        i += 2;
    }
    Some(zBlob)
}

/// Log an error that is an API call on a connection pointer that should
/// not have been used.
fn logBadConnection(zType: &[u8]) {
    crate::printf::sqlite3_log(
        SQLITE_MISUSE,
        b"API call with %s database connection pointer",
        &[PfArg::Str(Some(zType))],
    );
}

/// C: `int sqlite3SafetyCheckOk(sqlite3 *db)`
///
/// Check to make sure we have a valid db pointer.  Returns 1 if the db
/// pointer is valid and 0 if it should not be used.
pub fn sqlite3SafetyCheckOk(db: &mut Sqlite3) -> i32 {
    let eOpenState = db.eOpenState;
    if eOpenState != SQLITE_STATE_OPEN {
        if sqlite3SafetyCheckSickOrOk(db) != 0 {
            logBadConnection(b"unopened");
        }
        0
    } else {
        1
    }
}

/// C: `int sqlite3SafetyCheckSickOrOk(sqlite3 *db)`
pub fn sqlite3SafetyCheckSickOrOk(db: &mut Sqlite3) -> i32 {
    let eOpenState = db.eOpenState;
    if eOpenState != SQLITE_STATE_SICK
        && eOpenState != SQLITE_STATE_OPEN
        && eOpenState != SQLITE_STATE_BUSY
    {
        logBadConnection(b"invalid");
        0
    } else {
        1
    }
}

/// C: `int sqlite3AddInt64(i64 *pA, i64 iB)`
///
/// Attempt to add the 64-bit signed value iB to *pA.  Return 0 on success.
/// On overflow return 1 (and, as with __builtin_add_overflow, *pA holds the
/// wrapped result).
pub fn sqlite3AddInt64(pA: &mut i64, iB: i64) -> i32 {
    let (r, o) = pA.overflowing_add(iB);
    *pA = r;
    o as i32
}

/// C: `int sqlite3SubInt64(i64 *pA, i64 iB)`
pub fn sqlite3SubInt64(pA: &mut i64, iB: i64) -> i32 {
    let (r, o) = pA.overflowing_sub(iB);
    *pA = r;
    o as i32
}

/// C: `int sqlite3MulInt64(i64 *pA, i64 iB)`
pub fn sqlite3MulInt64(pA: &mut i64, iB: i64) -> i32 {
    let (r, o) = pA.overflowing_mul(iB);
    *pA = r;
    o as i32
}

/// C: `int sqlite3AbsInt32(int x)`
///
/// Compute the absolute value of a 32-bit signed integer, if possible.  Or
/// if the integer has a value of -2147483648, return +2147483647
pub fn sqlite3AbsInt32(x: i32) -> i32 {
    if x >= 0 {
        return x;
    }
    if x == i32::MIN {
        return 0x7fffffff;
    }
    -x
}

/// C: `void sqlite3FileSuffix3(const char *zBaseFilename, char *z)`
///
/// SQLITE_ENABLE_8_3_NAMES is not enabled: this is a no-op (as the C macro).
pub fn sqlite3FileSuffix3(_zBaseFilename: Option<&[u8]>, _z: Option<&[u8]>) {}

/// C: `LogEst sqlite3LogEstAdd(LogEst a, LogEst b)`
///
/// Find (an approximate) sum of two LogEst values.
pub fn sqlite3LogEstAdd(a: LogEst, b: LogEst) -> LogEst {
    static x: [u8; 32] = [
        10, 10, // 0,1
        9, 9, // 2,3
        8, 8, // 4,5
        7, 7, 7, // 6,7,8
        6, 6, 6, // 9,10,11
        5, 5, 5, // 12-14
        4, 4, 4, 4, // 15-18
        3, 3, 3, 3, 3, 3, // 19-24
        2, 2, 2, 2, 2, 2, 2, // 25-31
    ];
    let (a, b) = (a as i32, b as i32);
    let r = if a >= b {
        if a > b + 49 {
            a
        } else if a > b + 31 {
            a + 1
        } else {
            a + x[(a - b) as usize] as i32
        }
    } else if b > a + 49 {
        b
    } else if b > a + 31 {
        b + 1
    } else {
        b + x[(b - a) as usize] as i32
    };
    r as LogEst
}

/// C: `LogEst sqlite3LogEst(u64 x)`
///
/// Convert an integer into a LogEst.  In other words, compute an
/// approximation for 10*log2(x).
pub fn sqlite3LogEst(x: u64) -> LogEst {
    static a: [LogEst; 8] = [0, 2, 3, 5, 6, 7, 8, 9];
    let mut x = x;
    let mut y: LogEst = 40;
    if x < 8 {
        if x < 2 {
            return 0;
        }
        while x < 8 {
            y -= 10;
            x <<= 1;
        }
    } else {
        let i = 60 - x.leading_zeros() as i32;
        y += (i * 10) as LogEst;
        x >>= i;
    }
    a[(x & 7) as usize] + y - 10
}

/// C: `LogEst sqlite3LogEstFromDouble(double x)`
///
/// Convert a double into a LogEst.
pub fn sqlite3LogEstFromDouble(x: f64) -> LogEst {
    if x <= 1.0 {
        return 0;
    }
    if x <= 2000000000.0 {
        return sqlite3LogEst(x as u64);
    }
    let a = x.to_bits();
    let e = ((a >> 52) as i64 - 1022) as LogEst;
    e.wrapping_mul(10)
}

/// C: `u64 sqlite3LogEstToInt(LogEst x)`
///
/// Convert a LogEst into an integer.
pub fn sqlite3LogEstToInt(x: LogEst) -> u64 {
    let mut x = x;
    let mut n: u64 = (x % 10) as i64 as u64;
    x /= 10;
    if n >= 5 {
        n = n.wrapping_sub(2);
    } else if n >= 1 {
        n -= 1;
    }
    if x > 60 {
        return LARGEST_INT64 as u64;
    }
    if x >= 3 {
        n.wrapping_add(8) << (x - 3)
    } else {
        n.wrapping_add(8) >> (3 - x)
    }
}

/// C: `VList * sqlite3VListAdd( sqlite3 *db, VList *pIn, const char *zName, int nName, int iVal )`
///
/// Add a new name/number pair to a VList.
///
/// Rust: `pIn` is modified in place; a copy of the updated list is also
/// returned (C returns the possibly reallocated list).
pub fn sqlite3VListAdd(
    _db: &mut Sqlite3,
    pIn: &mut VList,
    zName: Option<&[u8]>,
    nName: i32,
    iVal: i32,
) -> Option<Box<VList>> {
    let z = zName.unwrap_or(b"");
    let n = (nName.max(0) as usize).min(z.len());
    pIn.a.push((iVal, z[..n].to_vec()));
    Some(Box::new(pIn.clone()))
}

/// C: `const char * sqlite3VListNumToName(VList *pIn, int iVal)`
///
/// Return the name of a variable in the given VList that has the value
/// iVal.  Or return None if there is no such variable in the list.
pub fn sqlite3VListNumToName(pIn: &mut VList, iVal: i32) -> Option<ZStr> {
    pIn.a
        .iter()
        .find(|(v, _)| *v == iVal)
        .map(|(_, n)| n.clone())
}

/// C: `int sqlite3VListNameToNum(VList *pIn, const char *zName, int nName)`
///
/// Return the number of the variable named zName, if it is in VList, or
/// return 0 if there is no such variable.
pub fn sqlite3VListNameToNum(pIn: &mut VList, zName: Option<&[u8]>, nName: i32) -> i32 {
    let z = zName.unwrap_or(b"");
    let n = (nName.max(0) as usize).min(z.len());
    let key = &z[..n];
    for (v, name) in pIn.a.iter() {
        // strncmp(z,zName,nName)==0 && z[nName]==0
        if name.as_slice() == key {
            return *v;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atof(s: &str) -> (i32, f64) {
        let mut r = 0.0;
        let rc = sqlite3AtoF(Some(s.as_bytes()), &mut r);
        (rc, r)
    }

    #[test]
    fn test_atof_values() {
        // Correct rounding for ordinary inputs (compare with Rust's parser,
        // which is correctly rounded).
        for s in [
            "0.1",
            "1e300",
            "123456789012345678.0",
            "3.14159",
            "2.2250738585072014e-308",
            "4.9e-324",
            "1.7976931348623157e308",
            "0.3",
            "1e23",
            "8.98846567431158e307",
            "5e-324",
            "123.456e-10",
            "9007199254740993",
            "0.000001",
            "1.5",
            "100",
        ] {
            let (rc, r) = atof(s);
            assert!(rc > 0, "{s}");
            assert_eq!(r, s.parse::<f64>().unwrap(), "{s}");
        }
        assert_eq!(atof("-2.5"), (3, -2.5));
        assert_eq!(atof("  12  "), (1, 12.0));
        assert_eq!(atof("0"), (5, 0.0));
        assert_eq!(atof("0.0"), (7, 0.0));
        assert_eq!(atof(""), (0, 0.0));
        assert_eq!(atof("abc"), (0, 0.0));
        assert_eq!(atof("."), (0, 0.0));
        assert_eq!(atof("12abc").0, (0xfffffff0u32 as i32) | 1);
        assert_eq!(atof("1e").0, (0xfffffff0u32 as i32) | 1);
        assert_eq!(atof("1e+").0, (0xfffffff0u32 as i32) | 1);
        assert_eq!(atof("1e999").1, f64::INFINITY);
        assert_eq!(atof("1e-999").1, 0.0);
        assert_eq!(atof("12345678901234567890123").0, 9);
        assert_eq!(atof("1.").0, 3);
        assert_eq!(atof(".5").0, 3);
    }

    fn fp(r: f64, iRound: i32, mx: i32) -> (String, i32, u8) {
        let mut p = FpDecode::default();
        sqlite3FpDecode(&mut p, r, iRound, mx);
        (
            String::from_utf8(p.z[..p.n as usize].to_vec()).unwrap(),
            p.iDP,
            p.sign,
        )
    }

    #[test]
    fn test_fpdecode() {
        assert_eq!(fp(0.1, 15, 16), ("1".into(), 0, b'+'));
        assert_eq!(fp(1.0 / 3.0, 15, 16), ("333333333333333".into(), 0, b'+'));
        assert_eq!(fp(1e300, 15, 16), ("1".into(), 301, b'+'));
        assert_eq!(fp(-2.5, 15, 16), ("25".into(), 1, b'-'));
        assert_eq!(
            fp(123456789012345678.0, 15, 16),
            ("123456789012346".into(), 18, b'+')
        );
        assert_eq!(fp(0.0, 15, 16), ("0".into(), 1, b'+'));
        // %!.17g round-trip reduction
        assert_eq!(fp(49.47, 17, 20), ("4947".into(), 2, b'+'));
        assert_eq!(fp(0.1, 17, 20), ("1".into(), 0, b'+'));
    }

    include!("util_testdata.rs");

    #[test]
    fn test_printf_fp_against_reference() {
        // Expected values produced by the reference CLI (sqlite3ref).
        let mut bad = Vec::new();
        for (txt, exp) in TABLE.iter() {
            let mut r = 0.0;
            let rc = sqlite3AtoF(Some(txt.as_bytes()), &mut r);
            assert!(rc > 0, "{txt}");
            assert_eq!(
                r.to_bits(),
                txt.parse::<f64>().unwrap().to_bits(),
                "atof {txt}"
            );
            for (k, f) in FMTS.iter().enumerate() {
                let got = String::from_utf8(crate::printf::sqlite3_mprintf(
                    f.as_bytes(),
                    &[PfArg::Dbl(r)],
                ))
                .unwrap();
                if got != exp[k] {
                    bad.push(format!("{txt} {f}: got {got} want {}", exp[k]));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn test_atoi64() {
        let t = |s: &str| {
            let mut v = 0i64;
            let rc = sqlite3Atoi64(Some(s.as_bytes()), &mut v, s.len() as i32, SQLITE_UTF8);
            (rc, v)
        };
        assert_eq!(t("123"), (0, 123));
        assert_eq!(t("-123"), (0, -123));
        assert_eq!(t(" 12 "), (0, 12));
        assert_eq!(t("12x"), (1, 12));
        assert_eq!(t(""), (-1, 0));
        assert_eq!(t("9223372036854775807"), (0, i64::MAX));
        assert_eq!(t("9223372036854775808"), (3, i64::MAX));
        assert_eq!(t("-9223372036854775808"), (0, i64::MIN));
        assert_eq!(t("9223372036854775809"), (2, i64::MAX));
        assert_eq!(t("99999999999999999999"), (2, i64::MAX));
        let mut v = 0i64;
        assert_eq!(sqlite3DecOrHexToI64(Some(b"0x7fffffffffffffff"), &mut v), 0);
        assert_eq!(v, i64::MAX);
        assert_eq!(sqlite3DecOrHexToI64(Some(b"0xffffffffffffffff"), &mut v), 0);
        assert_eq!(v, -1);
        assert_eq!(
            sqlite3DecOrHexToI64(Some(b"0x1ffffffffffffffff"), &mut v),
            2
        );
        assert_eq!(sqlite3DecOrHexToI64(Some(b"12"), &mut v), 0);
        assert_eq!(v, 12);
    }

    #[test]
    fn test_getint32() {
        let g = |s: &str| {
            let mut v = 0;
            let r = sqlite3GetInt32(Some(s.as_bytes()), &mut v);
            (r, v)
        };
        assert_eq!(g("2147483647"), (1, 2147483647));
        assert_eq!(g("-2147483648"), (1, -2147483648));
        assert_eq!(g("2147483648").0, 0);
        assert_eq!(g("12 rest"), (1, 12));
        assert_eq!(g("0x10"), (1, 16));
        assert_eq!(g("0x80000000").0, 0);
        assert_eq!(g("abc").0, 0);
    }

    #[test]
    fn test_varint() {
        for v in [
            0u64,
            1,
            127,
            128,
            0x3fff,
            0x4000,
            0x1fffff,
            1 << 40,
            u64::MAX,
            0xff00000000000000,
        ] {
            let mut b = [0u8; 10];
            let n = sqlite3PutVarint(&mut b, v);
            if v < (1 << 56) {
                assert_eq!(n, sqlite3VarintLen(v));
            }
            let mut x = 0u64;
            assert_eq!(sqlite3GetVarint(&b, &mut x) as i32, n);
            assert_eq!(x, v);
            let mut y = 0u32;
            sqlite3GetVarint32(&b, &mut y);
            assert_eq!(y as u64, if v > u32::MAX as u64 { 0xffffffff } else { v });
        }
    }

    #[test]
    fn test_misc() {
        assert_eq!(sqlite3StrICmp(Some(b"abc"), Some(b"ABC")), 0);
        assert!(sqlite3StrICmp(Some(b"abc"), Some(b"abd")) < 0);
        assert!(sqlite3StrICmp(Some(b"abc"), Some(b"ab")) > 0);
        assert_eq!(sqlite3_strnicmp(Some(b"abcx"), Some(b"ABCy"), 3), 0);
        assert!(sqlite3_strnicmp(Some(b"ab"), Some(b"abc"), 3) < 0);
        let mut z = b"\"a\"\"b\"".to_vec();
        sqlite3Dequote(&mut z);
        assert_eq!(z, b"a\"b");
        let mut z = b"[x y]".to_vec();
        sqlite3Dequote(&mut z);
        assert_eq!(z, b"x y");
        let mut t = Token::from_bytes(b"'abc'");
        sqlite3DequoteToken(&mut t);
        assert_eq!(t.text(), b"abc");
        assert_eq!(sqlite3Int64ToTextVec(i64::MIN), b"-9223372036854775808");
        assert_eq!(sqlite3Int64ToTextVec(0), b"0");
        assert_eq!(sqlite3Int64ToTextVec(1234567), b"1234567");
        assert_eq!(sqlite3LogEst(1), 0);
        assert_eq!(sqlite3LogEst(10), 33);
        assert_eq!(sqlite3LogEst(100), 66);
        assert_eq!(sqlite3LogEst(1000000), 199);
        assert_eq!(sqlite3LogEstToInt(33), 10);
        assert_eq!(sqlite3LogEstAdd(10, 10), 20);
        assert_eq!(sqlite3LogEstFromDouble(1e10), 340);
    }
}
