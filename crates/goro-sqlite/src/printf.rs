//! Port of printf.c: SQLite's own printf() implementation.
//!
//! The C varargs become a slice of `PfArg` values.  Formats that take
//! internal objects (%T for a Token, %#T for an Expr, %S for a SrcItem)
//! take the matching `PfArg` variant and need the connection (only
//! available through sqlite3MPrintf()/sqlite3ErrorMsg()).
//!
//! Use the `mprintf!` and `errmsg!` macros (defined in lib.rs):
//! `mprintf!(db, "no such table: %s", name)`.

use crate::int::*;

/// Arguments of the SQL printf()/format() function.
pub struct PrintfArguments<'a> {
    pub nArg: i32,
    pub nUsed: i32,
    pub apArg: &'a mut [Mem],
}

/// An argument of a printf-style function (the C varargs).
#[derive(Clone, Copy)]
pub enum PfArg<'a> {
    /// Any integer argument (%d %i %u %x %X %o %c %lld %r %p ...)
    Int(i64),
    /// %f %e %E %g %G
    Dbl(f64),
    /// %s %z %q %Q %w %j %J (None is the C NULL pointer)
    Str(Option<&'a [u8]>),
    /// %T
    Tok(&'a Token),
    /// %#T: the token of an expression
    Expr(ExprId),
    /// %S
    Src(SrcItemId),
}

impl<'a> From<i64> for PfArg<'a> {
    fn from(v: i64) -> Self {
        PfArg::Int(v)
    }
}
impl<'a> From<i32> for PfArg<'a> {
    fn from(v: i32) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<u32> for PfArg<'a> {
    fn from(v: u32) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<u64> for PfArg<'a> {
    fn from(v: u64) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<u16> for PfArg<'a> {
    fn from(v: u16) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<i16> for PfArg<'a> {
    fn from(v: i16) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<u8> for PfArg<'a> {
    fn from(v: u8) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<usize> for PfArg<'a> {
    fn from(v: usize) -> Self {
        PfArg::Int(v as i64)
    }
}
impl<'a> From<f64> for PfArg<'a> {
    fn from(v: f64) -> Self {
        PfArg::Dbl(v)
    }
}
impl<'a> From<&'a [u8]> for PfArg<'a> {
    fn from(v: &'a [u8]) -> Self {
        PfArg::Str(Some(v))
    }
}
impl<'a, const N: usize> From<&'a [u8; N]> for PfArg<'a> {
    fn from(v: &'a [u8; N]) -> Self {
        PfArg::Str(Some(&v[..]))
    }
}
impl<'a> From<&'a Vec<u8>> for PfArg<'a> {
    fn from(v: &'a Vec<u8>) -> Self {
        PfArg::Str(Some(v.as_slice()))
    }
}
impl<'a> From<&'a str> for PfArg<'a> {
    fn from(v: &'a str) -> Self {
        PfArg::Str(Some(v.as_bytes()))
    }
}
impl<'a> From<Option<&'a [u8]>> for PfArg<'a> {
    fn from(v: Option<&'a [u8]>) -> Self {
        PfArg::Str(v)
    }
}
impl<'a> From<&'a Option<Vec<u8>>> for PfArg<'a> {
    fn from(v: &'a Option<Vec<u8>>) -> Self {
        PfArg::Str(v.as_deref())
    }
}
impl<'a> From<&'a Token> for PfArg<'a> {
    fn from(v: &'a Token) -> Self {
        PfArg::Tok(v)
    }
}
impl<'a> From<ExprId> for PfArg<'a> {
    fn from(v: ExprId) -> Self {
        PfArg::Expr(v)
    }
}
impl<'a> From<SrcItemId> for PfArg<'a> {
    fn from(v: SrcItemId) -> Self {
        PfArg::Src(v)
    }
}

const etRADIX: u8 = 0;
const etFLOAT: u8 = 1;
const etEXP: u8 = 2;
const etGENERIC: u8 = 3;
const etSIZE: u8 = 4;
const etSTRING: u8 = 5;
const etDYNSTRING: u8 = 6;
const etPERCENT: u8 = 7;
const etCHARX: u8 = 8;
const etESCAPE_q: u8 = 9;
const etESCAPE_Q: u8 = 10;
const etTOKEN: u8 = 11;
const etSRCITEM: u8 = 12;
const etPOINTER: u8 = 13;
const etESCAPE_w: u8 = 14;
const etORDINAL: u8 = 15;
const etDECIMAL: u8 = 16;
const etESCAPE_j: u8 = 17;
const etESCAPE_J: u8 = 18;
const etINVALID: u8 = 19;

const FLAG_SIGNED: u8 = 1;

struct EtInfo {
    fmttype: u8,
    base: u8,
    flags: u8,
    type_: u8,
    charset: u8,
    prefix: u8,
    iNxt: u8,
}

const aDigits: &[u8] = b"0123456789ABCDEF0123456789abcdef";
const aHex: &[u8] = b"0123456789abcdef";
const aPrefix: &[u8] = b"-x0\0X0\0";

macro_rules! et {
    ($c:expr, $b:expr, $f:expr, $t:expr, $cs:expr, $p:expr, $n:expr) => {
        EtInfo {
            fmttype: $c,
            base: $b,
            flags: $f,
            type_: $t,
            charset: $cs,
            prefix: $p,
            iNxt: $n,
        }
    };
}

static fmtinfo: [EtInfo; 25] = [
    et!(b'd', 10, 1, etDECIMAL, 0, 0, 0),
    et!(b'e', 0, 1, etEXP, 30, 0, 0),
    et!(b'f', 0, 1, etFLOAT, 0, 0, 0),
    et!(b'g', 0, 1, etGENERIC, 30, 0, 0),
    et!(b'j', 0, 0, etESCAPE_j, 0, 0, 0),
    et!(b'i', 10, 1, etDECIMAL, 0, 0, 0),
    et!(b'Q', 0, 4, etESCAPE_Q, 0, 0, 4),
    et!(b'p', 16, 0, etPOINTER, 0, 1, 0),
    et!(b'S', 0, 0, etSRCITEM, 0, 0, 0),
    et!(b'T', 0, 0, etTOKEN, 0, 0, 0),
    et!(b'n', 0, 0, etSIZE, 0, 0, 0),
    et!(b'o', 8, 0, etRADIX, 0, 2, 0),
    et!(b'%', 0, 0, etPERCENT, 0, 0, 7),
    et!(b'q', 0, 4, etESCAPE_q, 0, 0, 16),
    et!(b'r', 10, 1, etORDINAL, 0, 0, 0),
    et!(b's', 0, 4, etSTRING, 0, 0, 0),
    et!(b'X', 16, 0, etRADIX, 0, 4, 0),
    et!(b'u', 10, 0, etDECIMAL, 0, 0, 0),
    et!(b'w', 0, 4, etESCAPE_w, 0, 0, 0),
    et!(b'E', 0, 1, etEXP, 14, 0, 18),
    et!(b'x', 16, 0, etRADIX, 16, 1, 0),
    et!(b'G', 0, 1, etGENERIC, 14, 0, 0),
    et!(b'z', 0, 4, etDYNSTRING, 0, 0, 0),
    et!(b'J', 0, 0, etESCAPE_J, 0, 0, 0),
    et!(b'c', 0, 0, etCHARX, 0, 0, 23),
];

const SQLITE_FP_PRECISION_LIMIT: i64 = 100000000;

/// Where the arguments of a format come from.
pub enum PfArgs<'a, 'b> {
    /// C varargs
    Va(&'b [PfArg<'a>]),
    /// SQL function arguments (SQLITE_PRINTF_SQLFUNC)
    Sql(&'b mut PrintfArguments<'a>),
}

struct ArgCursor<'a, 'b> {
    src: PfArgs<'a, 'b>,
    i: usize,
}

impl<'a, 'b> ArgCursor<'a, 'b> {
    fn is_sql(&self) -> bool {
        matches!(self.src, PfArgs::Sql(_))
    }
    fn next_va(&mut self) -> Option<PfArg<'a>> {
        match &self.src {
            PfArgs::Va(a) => {
                let r = a.get(self.i).copied();
                self.i += 1;
                r
            }
            PfArgs::Sql(_) => None,
        }
    }
    fn int(&mut self) -> i64 {
        match &mut self.src {
            PfArgs::Sql(p) => {
                if p.nArg <= p.nUsed {
                    return 0;
                }
                let k = p.nUsed as usize;
                p.nUsed += 1;
                crate::vdbeapi::sqlite3_value_int64(&mut p.apArg[k])
            }
            PfArgs::Va(_) => match self.next_va() {
                Some(PfArg::Int(v)) => v,
                Some(PfArg::Dbl(v)) => v as i64,
                _ => 0,
            },
        }
    }
    fn dbl(&mut self) -> f64 {
        match &mut self.src {
            PfArgs::Sql(p) => {
                if p.nArg <= p.nUsed {
                    return 0.0;
                }
                let k = p.nUsed as usize;
                p.nUsed += 1;
                crate::vdbeapi::sqlite3_value_double(&mut p.apArg[k])
            }
            PfArgs::Va(_) => match self.next_va() {
                Some(PfArg::Dbl(v)) => v,
                Some(PfArg::Int(v)) => v as f64,
                _ => 0.0,
            },
        }
    }
    fn text(&mut self) -> Option<Vec<u8>> {
        match &mut self.src {
            PfArgs::Sql(p) => {
                if p.nArg <= p.nUsed {
                    return None;
                }
                let k = p.nUsed as usize;
                p.nUsed += 1;
                crate::vdbeapi::sqlite3_value_text(&mut p.apArg[k]).map(|s| s.to_vec())
            }
            PfArgs::Va(_) => match self.next_va() {
                Some(PfArg::Str(s)) => s.map(|x| x.to_vec()),
                Some(PfArg::Tok(t)) => Some(t.text().to_vec()),
                _ => None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// StrAccum
// ---------------------------------------------------------------------------

/// sqlite3StrAccumSetError()
pub fn sqlite3StrAccumSetError(p: &mut StrAccum, eError: u8) {
    p.accError = eError;
    if p.mxAlloc != 0 {
        sqlite3_str_reset(p);
    }
}

/// Make room for N more bytes.  Returns the number of bytes that can be
/// appended (N, or less when the buffer is fixed or the limit is hit).
pub fn sqlite3StrAccumEnlarge(p: &mut StrAccum, N: i64) -> i32 {
    if p.accError != 0 {
        return 0;
    }
    let nChar = p.zText.len() as i64;
    if p.mxAlloc == 0 {
        sqlite3StrAccumSetError(p, SQLITE_TOOBIG as u8);
        return (p.nAlloc as i64 - nChar - 1).max(0) as i32;
    }
    let szNew = nChar + N + 1;
    if szNew > p.mxAlloc as i64 {
        sqlite3_str_reset(p);
        sqlite3StrAccumSetError(p, SQLITE_TOOBIG as u8);
        return 0;
    }
    N as i32
}

/// Returns accError after ensuring space for N more bytes.
pub fn sqlite3StrAccumEnlargeIfNeeded(p: &mut StrAccum, N: i64) -> i32 {
    if needs_enlarge(p, N) {
        sqlite3StrAccumEnlarge(p, N);
    }
    p.accError as i32
}

#[inline]
fn needs_enlarge(p: &StrAccum, N: i64) -> bool {
    let nChar = p.zText.len() as i64;
    if p.mxAlloc == 0 {
        nChar + N >= p.nAlloc as i64
    } else {
        nChar + N + 1 > p.mxAlloc as i64
    }
}

/// Append N copies of character c.
pub fn sqlite3_str_appendchar(p: &mut StrAccum, N: i32, c: u8) {
    StrAppendchar64(p, N as i64, c);
}

fn StrAppendchar64(p: &mut StrAccum, mut N: i64, c: u8) {
    if needs_enlarge(p, N) {
        N = sqlite3StrAccumEnlarge(p, N) as i64;
        if N <= 0 {
            return;
        }
    }
    if p.accError != 0 && p.mxAlloc != 0 {
        return;
    }
    for _ in 0..N {
        p.zText.push(c);
    }
}

/// Append the first N bytes of z.
pub fn sqlite3_str_append(p: &mut StrAccum, z: &[u8], N: i32) {
    StrAppend64(p, &z[..(N.max(0) as usize).min(z.len())]);
}

fn StrAppend64(p: &mut StrAccum, z: &[u8]) {
    let mut n = z.len() as i64;
    if needs_enlarge(p, n) {
        n = sqlite3StrAccumEnlarge(p, n) as i64;
        if n <= 0 {
            return;
        }
    }
    if p.accError != 0 && p.mxAlloc != 0 {
        return;
    }
    p.zText.extend_from_slice(&z[..n as usize]);
}

/// Append the whole of z.
pub fn sqlite3_str_appendall(p: &mut StrAccum, z: &[u8]) {
    StrAppend64(p, z);
}

/// Finish the string and return its value.  None if there was an error
/// (as C, which returns NULL on OOM/TOOBIG).
pub fn sqlite3StrAccumFinish(p: &mut StrAccum) -> Option<ZStr> {
    if p.accError != 0 && p.mxAlloc != 0 {
        return None;
    }
    Some(std::mem::take(&mut p.zText))
}

pub fn sqlite3_str_finish(p: &mut StrAccum) -> Option<ZStr> {
    sqlite3StrAccumFinish(p)
}

pub fn sqlite3_str_errcode(p: &StrAccum) -> i32 {
    p.accError as i32
}

pub fn sqlite3_str_length(p: &StrAccum) -> i32 {
    p.zText.len() as i32
}

pub fn sqlite3_str_truncate(p: &mut StrAccum, N: i32) {
    if N >= 0 && (N as usize) < p.zText.len() {
        p.zText.truncate(N as usize);
    }
}

pub fn sqlite3_str_value(p: &StrAccum) -> Option<&[u8]> {
    if p.zText.is_empty() {
        None
    } else {
        Some(&p.zText)
    }
}

pub fn sqlite3_str_reset(p: &mut StrAccum) {
    p.zText.clear();
}

pub fn sqlite3_str_free(p: &mut StrAccum) {
    sqlite3_str_reset(p);
}

/// Initialize a string accumulator.  `n` is the size of the fixed buffer
/// (only meaningful when mx==0) and `mx` the maximum length.
pub fn sqlite3StrAccumInit(p: &mut StrAccum, n: i32, mx: i32) {
    p.zText = Vec::new();
    p.nAlloc = n.max(0) as u32;
    p.mxAlloc = mx.max(0) as u32;
    p.accError = 0;
    p.printfFlags = 0;
}

/// sqlite3_str_new(): an accumulator limited by the connection's length
/// limit.
pub fn sqlite3_str_new(db: Option<&Sqlite3>) -> StrAccum {
    let mut p = StrAccum::default();
    let mx = match db {
        Some(d) => d.aLimit[SQLITE_LIMIT_LENGTH as usize],
        None => SQLITE_MAX_LENGTH,
    };
    sqlite3StrAccumInit(&mut p, 0, mx);
    p
}

// ---------------------------------------------------------------------------
// The formatter
// ---------------------------------------------------------------------------

/// Render a string into `acc` from `fmt` and `args`.  `db` is needed for
/// the internal conversions (%T, %S); it is None for the public
/// sqlite3_mprintf().
pub fn sqlite3_str_vappendf(
    acc: &mut StrAccum,
    db: Option<&mut Sqlite3>,
    fmt: &[u8],
    args: PfArgs,
) {
    let mut db = db;
    let mut ap = ArgCursor { src: args, i: 0 };
    let bArgList = ap.is_sql();
    let n = fmt.len();
    let mut i = 0usize;
    let at = |k: usize| -> u8 { if k < n { fmt[k] } else { 0 } };
    while i < n {
        let mut c = fmt[i];
        if c == 0 {
            break;
        }
        if c != b'%' {
            let start = i;
            while i < n && fmt[i] != b'%' && fmt[i] != 0 {
                i += 1;
            }
            StrAppend64(acc, &fmt[start..i]);
            if i >= n || fmt[i] == 0 {
                break;
            }
        }
        // fmt[i] == '%'
        i += 1;
        c = at(i);
        if c == 0 {
            sqlite3_str_append(acc, b"%", 1);
            break;
        }
        let mut flag_leftjustify = false;
        let mut flag_prefix: u8 = 0;
        let mut cThousand: u8 = 0;
        let mut flag_alternateform = false;
        let mut flag_altform2 = false;
        let mut flag_zeropad = false;
        let mut width: i64 = 0;
        let mut flag_long = 0;
        let mut precision: i64 = -1;
        let mut done = false;
        loop {
            match c {
                b'-' => flag_leftjustify = true,
                b'+' => flag_prefix = b'+',
                b' ' => flag_prefix = b' ',
                b'#' => flag_alternateform = true,
                b'!' => flag_altform2 = true,
                b'0' => flag_zeropad = true,
                b',' => cThousand = b',',
                b'l' => {
                    flag_long = 1;
                    i += 1;
                    c = at(i);
                    if c == b'l' {
                        i += 1;
                        c = at(i);
                        flag_long = 2;
                    }
                    done = true;
                }
                b'1'..=b'9' => {
                    let mut wx: u64 = (c - b'0') as u64;
                    loop {
                        i += 1;
                        c = at(i);
                        if !c.is_ascii_digit() {
                            break;
                        }
                        wx = wx.wrapping_mul(10).wrapping_add((c - b'0') as u64) & 0xffffffff;
                    }
                    width = (wx & 0x7fffffff) as i64;
                    if c != b'.' && c != b'l' {
                        done = true;
                    } else {
                        i -= 1;
                    }
                }
                b'*' => {
                    width = ap.int() as i32 as i64;
                    if width < 0 {
                        flag_leftjustify = true;
                        width = if width >= -2147483647 { -width } else { 0 };
                    }
                    let c2 = at(i + 1);
                    if c2 != b'.' && c2 != b'l' {
                        i += 1;
                        c = at(i);
                        done = true;
                    }
                }
                b'.' => {
                    i += 1;
                    c = at(i);
                    if c == b'*' {
                        precision = ap.int() as i32 as i64;
                        if precision < 0 {
                            precision = if precision >= -2147483647 {
                                -precision
                            } else {
                                -1
                            };
                        }
                        i += 1;
                        c = at(i);
                    } else {
                        let mut px: u64 = 0;
                        while c.is_ascii_digit() {
                            px = px.wrapping_mul(10).wrapping_add((c - b'0') as u64) & 0xffffffff;
                            i += 1;
                            c = at(i);
                        }
                        precision = (px & 0x7fffffff) as i64;
                    }
                    if c == b'l' {
                        i -= 1;
                    } else {
                        done = true;
                    }
                }
                _ => done = true,
            }
            if done {
                break;
            }
            i += 1;
            c = at(i);
            if c == 0 {
                break;
            }
        }
        // c is the conversion character, fmt[i] == c
        let mut idx = (c as usize) % 25;
        let xtype;
        if fmtinfo[idx].fmttype == c || {
            idx = fmtinfo[idx].iNxt as usize;
            fmtinfo[idx].fmttype == c
        } {
            xtype = fmtinfo[idx].type_;
        } else {
            idx = 0;
            xtype = etINVALID;
        }
        let infop = &fmtinfo[idx];
        let mut xtype = xtype;
        // The rendered text of this conversion (C bufpt/length).
        let mut buf: Vec<u8> = Vec::new();
        let mut length: i64;
        match xtype {
            etPOINTER | etORDINAL | etRADIX | etDECIMAL => {
                if xtype == etPOINTER {
                    flag_long = 2;
                }
                if xtype != etDECIMAL {
                    cThousand = 0;
                }
                let mut longvalue: u64;
                let prefix: u8;
                if infop.flags & FLAG_SIGNED != 0 {
                    let v: i64 = if bArgList {
                        ap.int()
                    } else if flag_long != 0 {
                        ap.int()
                    } else {
                        ap.int() as i32 as i64
                    };
                    if v < 0 {
                        longvalue = (!(v as u64)).wrapping_add(1);
                        prefix = b'-';
                    } else {
                        longvalue = v as u64;
                        prefix = flag_prefix;
                    }
                } else {
                    longvalue = if bArgList {
                        ap.int() as u64
                    } else if flag_long != 0 {
                        ap.int() as u64
                    } else {
                        ap.int() as u32 as u64
                    };
                    prefix = 0;
                }
                if longvalue == 0 {
                    flag_alternateform = false;
                }
                if flag_zeropad && precision < width - (prefix != 0) as i64 {
                    precision = width - (prefix != 0) as i64;
                }
                // Build the digits in reverse.
                let mut rev: Vec<u8> = Vec::new();
                if xtype == etORDINAL {
                    const zOrd: &[u8] = b"thstndrd";
                    let mut x = (longvalue % 10) as usize;
                    if x >= 4 || (longvalue / 10) % 10 == 1 {
                        x = 0;
                    }
                    rev.push(zOrd[x * 2 + 1]);
                    rev.push(zOrd[x * 2]);
                }
                let cset = &aDigits[infop.charset as usize..];
                let base = infop.base as u64;
                let ndigit_start = rev.len();
                loop {
                    rev.push(cset[(longvalue % base) as usize]);
                    longvalue /= base;
                    if longvalue == 0 {
                        break;
                    }
                }
                let mut nd = (rev.len() - ndigit_start) as i64 + (ndigit_start as i64);
                // length counts digits plus ordinal suffix (as C)
                if precision > nd {
                    let nn = precision - nd;
                    for _ in 0..nn {
                        rev.push(b'0');
                    }
                    nd = precision;
                }
                let mut digits: Vec<u8> = rev.into_iter().rev().collect();
                if cThousand != 0 {
                    // insert separators every 3 digits from the right
                    let len = nd as usize;
                    let mut out = Vec::with_capacity(len + len / 3);
                    for (k, &d) in digits.iter().enumerate() {
                        if k > 0 && (len - k) % 3 == 0 {
                            out.push(cThousand);
                        }
                        out.push(d);
                    }
                    digits = out;
                }
                if flag_alternateform && infop.prefix != 0 {
                    let mut pre = Vec::new();
                    let mut k = infop.prefix as usize;
                    while aPrefix[k] != 0 {
                        pre.push(aPrefix[k]);
                        k += 1;
                    }
                    pre.reverse();
                    let mut v = pre;
                    v.extend_from_slice(&digits);
                    digits = v;
                }
                if prefix != 0 {
                    let mut v = vec![prefix];
                    v.extend_from_slice(&digits);
                    digits = v;
                }
                length = digits.len() as i64;
                buf = digits;
            }
            etFLOAT | etEXP | etGENERIC => {
                let realvalue = ap.dbl();
                if precision < 0 {
                    precision = 6;
                }
                if precision > SQLITE_FP_PRECISION_LIMIT {
                    precision = SQLITE_FP_PRECISION_LIMIT;
                }
                let iRound: i64 = if xtype == etFLOAT {
                    -precision
                } else if xtype == etGENERIC {
                    if precision == 0 {
                        precision = 1;
                    }
                    precision
                } else {
                    precision + 1
                };
                let mut s = crate::util::FpDecode::default();
                crate::util::sqlite3FpDecode(
                    &mut s,
                    realvalue,
                    iRound as i32,
                    if flag_altform2 { 20 } else { 16 },
                );
                let mut special_done = false;
                if s.isSpecial != 0 {
                    if s.isSpecial == 2 {
                        buf = if flag_zeropad {
                            b"null".to_vec()
                        } else {
                            b"NaN".to_vec()
                        };
                        special_done = true;
                    } else if flag_zeropad {
                        s.z = vec![b'9'];
                        s.iDP = 1000;
                        s.n = 1;
                    } else {
                        let mut b = b"-Inf".to_vec();
                        if s.sign == b'-' {
                        } else if flag_prefix != 0 {
                            b[0] = flag_prefix;
                        } else {
                            b.remove(0);
                        }
                        buf = b;
                        special_done = true;
                    }
                }
                if special_done {
                    length = buf.len() as i64;
                } else {
                    let prefix: u8 = if s.sign == b'-' {
                        if flag_alternateform
                            && flag_prefix == 0
                            && xtype == etFLOAT
                            && s.iDP as i64 <= iRound
                        {
                            0
                        } else {
                            b'-'
                        }
                    } else {
                        flag_prefix
                    };
                    let mut exp = s.iDP as i64 - 1;
                    let flag_rtz;
                    if xtype == etGENERIC {
                        precision -= 1;
                        flag_rtz = !flag_alternateform;
                        if exp < -4 || exp > precision {
                            xtype = etEXP;
                        } else {
                            precision -= exp;
                            xtype = etFLOAT;
                        }
                    } else {
                        flag_rtz = flag_altform2;
                    }
                    let mut e2: i64 = if xtype == etEXP { 0 } else { s.iDP as i64 - 1 };
                    let mut out: Vec<u8> = Vec::new();
                    let flag_dp = precision > 0 || flag_alternateform || flag_altform2;
                    if prefix != 0 {
                        out.push(prefix);
                    }
                    let sn = s.n as usize;
                    let sz = |k: usize| -> u8 { if k < s.z.len() { s.z[k] } else { b'0' } };
                    let mut j: usize = 0;
                    if e2 < 0 {
                        out.push(b'0');
                    } else if cThousand != 0 {
                        while e2 >= 0 {
                            out.push(if j < sn {
                                j += 1;
                                sz(j - 1)
                            } else {
                                b'0'
                            });
                            if e2 % 3 == 0 && e2 > 1 {
                                out.push(b',');
                            }
                            e2 -= 1;
                        }
                    } else {
                        j = (e2 + 1) as usize;
                        if j > sn {
                            j = sn;
                        }
                        for k in 0..j {
                            out.push(sz(k));
                        }
                        e2 -= j as i64;
                        if e2 >= 0 {
                            for _ in 0..(e2 + 1) {
                                out.push(b'0');
                            }
                            e2 = -1;
                        }
                    }
                    if flag_dp {
                        out.push(b'.');
                    }
                    if e2 < -1 && precision > 0 {
                        let mut nn = -1 - e2;
                        if nn > precision {
                            nn = precision;
                        }
                        for _ in 0..nn {
                            out.push(b'0');
                        }
                        precision -= nn;
                    }
                    if precision > 0 {
                        let mut nn = sn as i64 - j as i64;
                        if nn > precision {
                            nn = precision;
                        }
                        if nn > 0 {
                            for k in 0..nn as usize {
                                out.push(sz(j + k));
                            }
                            precision -= nn;
                        }
                        if precision > 0 && !flag_rtz {
                            for _ in 0..precision {
                                out.push(b'0');
                            }
                        }
                    }
                    if flag_rtz && flag_dp {
                        while out.last() == Some(&b'0') {
                            out.pop();
                        }
                        if out.last() == Some(&b'.') {
                            if flag_altform2 {
                                out.push(b'0');
                            } else {
                                out.pop();
                            }
                        }
                    }
                    if xtype == etEXP {
                        exp = s.iDP as i64 - 1;
                        out.push(aDigits[infop.charset as usize]);
                        if exp < 0 {
                            out.push(b'-');
                            exp = -exp;
                        } else {
                            out.push(b'+');
                        }
                        if exp >= 100 {
                            out.push((exp / 100) as u8 + b'0');
                            exp %= 100;
                        }
                        out.push((exp / 10) as u8 + b'0');
                        out.push((exp % 10) as u8 + b'0');
                    }
                    length = out.len() as i64;
                    if length < width {
                        let nPad = (width - length) as usize;
                        if flag_leftjustify {
                            out.extend(std::iter::repeat_n(b' ', nPad));
                        } else if !flag_zeropad {
                            let mut v = vec![b' '; nPad];
                            v.extend_from_slice(&out);
                            out = v;
                        } else {
                            let adj = (prefix != 0) as usize;
                            let mut v = out[..adj].to_vec();
                            v.extend(std::iter::repeat_n(b'0', nPad));
                            v.extend_from_slice(&out[adj..]);
                            out = v;
                        }
                        length = width;
                    }
                    // C appends directly and continues (no further width
                    // processing).
                    StrAppend64(acc, &out);
                    let _ = length;
                    i += 1;
                    continue;
                }
            }
            etSIZE => {
                length = 0;
                width = 0;
            }
            etPERCENT => {
                buf = vec![b'%'];
                length = 1;
            }
            etCHARX => {
                let mut b: Vec<u8> = Vec::new();
                if bArgList {
                    let t = ap.text();
                    match t {
                        Some(t) if !t.is_empty() => {
                            let c0 = t[0];
                            b.push(c0);
                            if (c0 & 0xc0) == 0xc0 {
                                let mut k = 1;
                                while b.len() < 4 && k < t.len() && (t[k] & 0xc0) == 0x80 {
                                    b.push(t[k]);
                                    k += 1;
                                }
                            }
                        }
                        _ => b.push(0),
                    }
                } else {
                    let ch = ap.int() as u32;
                    let mut tmp = [0u8; 8];
                    let len = append_one_utf8(&mut tmp, ch);
                    b.extend_from_slice(&tmp[..len]);
                }
                length = b.len() as i64;
                if precision > 1 {
                    width -= precision - 1;
                    if width > 1 && !flag_leftjustify {
                        StrAppendchar64(acc, width - 1, b' ');
                        width = 0;
                    }
                    for _ in 0..(precision - 1) {
                        StrAppend64(acc, &b);
                    }
                }
                buf = b;
                flag_altform2 = true;
                // adjust_width_for_utf8
                if flag_altform2 && width > 0 {
                    for &x in buf.iter() {
                        if (x & 0xc0) == 0x80 {
                            width += 1;
                        }
                    }
                }
            }
            etSTRING | etDYNSTRING => {
                let s = ap.text();
                let s = s.unwrap_or_default();
                let s = match s.iter().position(|&x| x == 0) {
                    Some(p) => s[..p].to_vec(),
                    None => s,
                };
                if precision >= 0 {
                    if flag_altform2 {
                        let mut k = 0usize;
                        let mut pr = precision;
                        while pr > 0 && k < s.len() {
                            pr -= 1;
                            // SQLITE_SKIP_UTF8
                            let c0 = s[k];
                            k += 1;
                            if c0 >= 0xc0 {
                                while k < s.len() && (s[k] & 0xc0) == 0x80 {
                                    k += 1;
                                }
                            }
                        }
                        length = k as i64;
                    } else {
                        length = (s.len() as i64).min(precision);
                    }
                } else {
                    length = s.len() as i64;
                }
                buf = s[..length as usize].to_vec();
                if flag_altform2 && width > 0 {
                    for &x in buf.iter() {
                        if (x & 0xc0) == 0x80 {
                            width += 1;
                        }
                    }
                }
            }
            etESCAPE_j | etESCAPE_J => {
                let escarg = ap.text();
                let iStart = acc.zText.len();
                match escarg {
                    None => {
                        if xtype == etESCAPE_J {
                            sqlite3_str_append(acc, b"null", 4);
                        }
                    }
                    Some(e) => {
                        let ec = |k: usize| -> u8 { if k < e.len() { e[k] } else { 0 } };
                        if xtype == etESCAPE_J {
                            sqlite3_str_append(acc, b"\"", 1);
                        }
                        let mut px = precision;
                        if px < 0 {
                            px = 0x7fffffff;
                        } else if flag_altform2 {
                            let mut k: i64 = 0;
                            while k < px && ec(k as usize) != 0 {
                                if (ec(k as usize) & 0xc0) == 0x80 {
                                    px += 1;
                                }
                                k += 1;
                            }
                            if k == px {
                                while (ec(px as usize) & 0xc0) == 0x80 {
                                    px += 1;
                                }
                            }
                        }
                        let mut j: usize = 0;
                        let mut k: usize = 0;
                        while (k as i64) < px {
                            let ch = ec(k);
                            if ch <= 0x1f || ch == b'"' || ch == b'\\' {
                                if j < k {
                                    StrAppend64(acc, &e[j..k]);
                                }
                                j = k + 1;
                                if ch == 0 {
                                    break;
                                }
                                sqlite3_str_appendchar(acc, 1, b'\\');
                                if ch > 0x1f {
                                    sqlite3_str_appendchar(acc, 1, ch);
                                } else if ((1u32 << ch) & 0x3700) != 0 {
                                    let m = b"btn?fr"[(ch - 8) as usize];
                                    sqlite3_str_appendchar(acc, 1, m);
                                } else {
                                    sqlite3_str_append(acc, b"u00", 3);
                                    sqlite3_str_appendchar(acc, 1, aHex[(ch >> 4) as usize]);
                                    sqlite3_str_appendchar(acc, 1, aHex[(ch & 0xf) as usize]);
                                }
                            }
                            k += 1;
                        }
                        let k2 = k.min(e.len());
                        if j < k2 {
                            StrAppend64(acc, &e[j..k2]);
                        }
                        if xtype == etESCAPE_J {
                            sqlite3_str_append(acc, b"\"", 1);
                        }
                    }
                }
                if width > 0 && acc.accError == 0 {
                    let n = (acc.zText.len() - iStart) as i64;
                    let mut len = n;
                    if flag_altform2 && n > 0 {
                        for &x in &acc.zText[iStart..] {
                            if (x & 0xc0) == 0x80 {
                                len -= 1;
                            }
                        }
                    }
                    if width > len {
                        let sp = (width - len) as usize;
                        if !flag_leftjustify && n > 0 {
                            let tail = acc.zText.split_off(iStart);
                            acc.zText.extend(std::iter::repeat_n(b' ', sp));
                            acc.zText.extend_from_slice(&tail);
                        } else {
                            StrAppendchar64(acc, sp as i64, b' ');
                        }
                    }
                }
                i += 1;
                continue;
            }
            etESCAPE_q | etESCAPE_Q | etESCAPE_w => {
                let mut needQuote = 0;
                let arg = ap.text();
                let escarg: Vec<u8> = match arg {
                    None => {
                        if xtype == etESCAPE_Q {
                            b"NULL".to_vec()
                        } else {
                            b"(NULL)".to_vec()
                        }
                    }
                    Some(a) => {
                        if xtype == etESCAPE_Q {
                            needQuote = 1;
                        }
                        a
                    }
                };
                let ec = |k: usize| -> u8 { if k < escarg.len() { escarg[k] } else { 0 } };
                let q = if xtype == etESCAPE_w {
                    flag_alternateform = false;
                    b'"'
                } else {
                    b'\''
                };
                let mut k = precision;
                let mut ii: usize = 0;
                while k != 0 && ec(ii) != 0 {
                    let ch = ec(ii);
                    if flag_altform2 && (ch & 0xc0) == 0xc0 {
                        while (ec(ii + 1) & 0xc0) == 0x80 {
                            ii += 1;
                        }
                    }
                    ii += 1;
                    k -= 1;
                }
                if flag_alternateform {
                    let mut nCtrl = 0;
                    for kk in 0..ii {
                        if escarg[kk] != b'\\' && escarg[kk] <= 0x1f {
                            nCtrl += 1;
                        }
                    }
                    if nCtrl > 0 || xtype == etESCAPE_q {
                        if xtype == etESCAPE_Q {
                            needQuote = 2;
                        }
                    } else {
                        flag_alternateform = false;
                    }
                }
                let mut out: Vec<u8> = Vec::new();
                if needQuote != 0 {
                    if needQuote == 2 {
                        out.extend_from_slice(b"unistr('");
                    } else {
                        out.push(b'\'');
                    }
                }
                if flag_alternateform {
                    for kk in 0..ii {
                        let ch = escarg[kk];
                        out.push(ch);
                        if ch == q {
                            out.push(ch);
                        } else if ch == b'\\' {
                            out.push(b'\\');
                        } else if ch <= 0x1f {
                            out.pop();
                            out.push(b'\\');
                            out.push(b'u');
                            out.push(b'0');
                            out.push(b'0');
                            out.push(if ch >= 0x10 { b'1' } else { b'0' });
                            out.push(aHex[(ch & 0xf) as usize]);
                        }
                    }
                } else {
                    for kk in 0..ii {
                        let ch = escarg[kk];
                        out.push(ch);
                        if ch == q {
                            out.push(ch);
                        }
                    }
                }
                if needQuote != 0 {
                    out.push(b'\'');
                    if needQuote == 2 {
                        out.push(b')');
                    }
                }
                length = out.len() as i64;
                buf = out;
                if flag_altform2 && width > 0 {
                    for &x in buf.iter() {
                        if (x & 0xc0) == 0x80 {
                            width += 1;
                        }
                    }
                }
            }
            etTOKEN => {
                if (acc.printfFlags & SQLITE_PRINTF_INTERNAL) == 0 {
                    return;
                }
                let a = ap.next_va();
                if let Some(d) = db.as_deref_mut() {
                    match a {
                        Some(PfArg::Expr(e)) if flag_alternateform => {
                            if e.nn() && !d.ex(e).HasProperty(EP_IntValue) {
                                let t = d.ex(e).token().to_vec();
                                sqlite3_str_appendall(acc, &t);
                                sqlite3RecordErrorOffsetOfExpr(d, e);
                            }
                        }
                        Some(PfArg::Tok(t)) => {
                            if t.n > 0 {
                                sqlite3_str_appendall(acc, t.text());
                                sqlite3RecordErrorByteOffset(d, t);
                            }
                        }
                        Some(PfArg::Str(Some(s))) => sqlite3_str_appendall(acc, s),
                        _ => {}
                    }
                } else if let Some(PfArg::Tok(t)) = a {
                    sqlite3_str_appendall(acc, t.text());
                }
                length = 0;
                width = 0;
            }
            etSRCITEM => {
                if (acc.printfFlags & SQLITE_PRINTF_INTERNAL) == 0 {
                    return;
                }
                let a = ap.next_va();
                if let (Some(PfArg::Src(it)), Some(d)) = (a, db.as_deref_mut()) {
                    let s = srcitem_text(d, it, flag_altform2);
                    sqlite3_str_appendall(acc, &s);
                } else if let Some(PfArg::Str(Some(s))) = a {
                    sqlite3_str_appendall(acc, s);
                }
                length = 0;
                width = 0;
            }
            _ => {
                return;
            }
        }
        width -= length;
        if width > 0 {
            if !flag_leftjustify {
                StrAppendchar64(acc, width, b' ');
            }
            StrAppend64(acc, &buf[..length as usize]);
            if flag_leftjustify {
                StrAppendchar64(acc, width, b' ');
            }
        } else {
            StrAppend64(acc, &buf[..(length.max(0) as usize).min(buf.len())]);
        }
        i += 1;
    }
}

fn append_one_utf8(z: &mut [u8], v: u32) -> usize {
    if v < 0x80 {
        z[0] = v as u8;
        1
    } else if v < 0x800 {
        z[0] = 0xc0 + ((v >> 6) & 0x1f) as u8;
        z[1] = 0x80 + (v & 0x3f) as u8;
        2
    } else if v < 0x10000 {
        z[0] = 0xe0 + ((v >> 12) & 0x0f) as u8;
        z[1] = 0x80 + ((v >> 6) & 0x3f) as u8;
        z[2] = 0x80 + (v & 0x3f) as u8;
        3
    } else {
        z[0] = 0xf0 + ((v >> 18) & 0x07) as u8;
        z[1] = 0x80 + ((v >> 12) & 0x3f) as u8;
        z[2] = 0x80 + ((v >> 6) & 0x3f) as u8;
        z[3] = 0x80 + (v & 0x3f) as u8;
        4
    }
}

/// The text %S renders for a SrcItem.
pub fn srcitem_text(db: &Sqlite3, it: SrcItemId, altform2: bool) -> ZStr {
    let p = db.item(it);
    let mut out = Vec::new();
    if p.zAlias.is_some() && !altform2 {
        out.extend_from_slice(p.zAlias.as_deref().unwrap());
    } else if let Some(name) = &p.zName {
        if p.fg.fixedSchema == 0 && p.fg.isSubquery == 0 {
            if let Some(d) = &p.u4.zDatabase {
                out.extend_from_slice(d);
                out.push(b'.');
            }
        }
        out.extend_from_slice(name);
    } else if let Some(a) = &p.zAlias {
        out.extend_from_slice(a);
    } else if p.fg.isSubquery != 0 {
        if let Some(sq) = &p.u4.pSubq {
            let sel = db.sel(sq.pSelect);
            if sel.selFlags & SF_NestedFrom != 0 {
                out.extend_from_slice(format!("(join-{})", sel.selId).as_bytes());
            } else if sel.selFlags & SF_MultiValue != 0 {
                out.extend_from_slice(format!("{}-ROW VALUES CLAUSE", p.u1.nRow).as_bytes());
            } else {
                out.extend_from_slice(format!("(subquery-{})", sel.selId).as_bytes());
            }
        }
    }
    out
}

/// If the token `t` is part of the SQL text being parsed, record its offset
/// as the error offset (sqlite3_error_offset()).
pub fn sqlite3RecordErrorByteOffset(db: &mut Sqlite3, t: &Token) {
    if db.errByteOffset != -2 {
        return;
    }
    let Some(pParse) = db.parses.last() else {
        return;
    };
    let (Some(sql), Some(tz)) = (&pParse.zSql, &t.z) else {
        return;
    };
    if !std::rc::Rc::ptr_eq(sql, tz) {
        return;
    }
    let zTail = pParse.zTail;
    let off = t.off as usize;
    if off >= zTail && off <= sql.len() {
        db.errByteOffset = (off - zTail) as i32;
    }
}

/// Record the error offset of an expression.
pub fn sqlite3RecordErrorOffsetOfExpr(db: &mut Sqlite3, pExpr: ExprId) {
    let mut e = pExpr;
    while e.nn() {
        let x = db.ex(e);
        if x.HasProperty(EP_Reduced | EP_TokenOnly) {
            return;
        }
        if x.HasProperty(EP_OuterON | EP_InnerON) || x.w.iOfst <= 0 {
            e = x.pLeft;
        } else {
            break;
        }
    }
    if e.is_null() {
        return;
    }
    if db.ex(e).HasProperty(EP_FromDDL) {
        return;
    }
    db.errByteOffset = db.ex(e).w.iOfst;
}

/// Print into memory obtained from the connection (sqlite3MPrintf()).
/// Internal conversions (%T, %S) are allowed.
pub fn sqlite3MPrintf(db: &mut Sqlite3, zFormat: &[u8], args: &[PfArg]) -> ZStr {
    let mut acc = StrAccum::default();
    sqlite3StrAccumInit(&mut acc, 0, db.aLimit[SQLITE_LIMIT_LENGTH as usize]);
    acc.printfFlags = SQLITE_PRINTF_INTERNAL;
    sqlite3_str_vappendf(&mut acc, Some(db), zFormat, PfArgs::Va(args));
    if acc.accError == SQLITE_TOOBIG as u8 {
        crate::util::sqlite3ErrorToParser(db, SQLITE_TOOBIG);
    }
    sqlite3StrAccumFinish(&mut acc).unwrap_or_default()
}

/// sqlite3VMPrintf() (same as sqlite3MPrintf in Rust).
pub fn sqlite3VMPrintf(db: &mut Sqlite3, zFormat: &[u8], args: &[PfArg]) -> ZStr {
    sqlite3MPrintf(db, zFormat, args)
}

/// The public sqlite3_mprintf(): no internal conversions.
pub fn sqlite3_mprintf(zFormat: &[u8], args: &[PfArg]) -> ZStr {
    let mut acc = StrAccum::default();
    sqlite3StrAccumInit(&mut acc, 0, SQLITE_MAX_LENGTH);
    sqlite3_str_vappendf(&mut acc, None, zFormat, PfArgs::Va(args));
    sqlite3StrAccumFinish(&mut acc).unwrap_or_default()
}

/// sqlite3_snprintf(): output limited to n-1 bytes.
pub fn sqlite3_snprintf(n: i32, zFormat: &[u8], args: &[PfArg]) -> ZStr {
    if n <= 0 {
        return Vec::new();
    }
    let mut acc = StrAccum::default();
    sqlite3StrAccumInit(&mut acc, n, 0);
    sqlite3_str_vappendf(&mut acc, None, zFormat, PfArgs::Va(args));
    std::mem::take(&mut acc.zText)
}

/// sqlite3_str_appendf()
pub fn sqlite3_str_appendf(p: &mut StrAccum, zFormat: &[u8], args: &[PfArg]) {
    sqlite3_str_vappendf(p, None, zFormat, PfArgs::Va(args));
}

/// sqlite3_str_appendf() with internal conversions allowed.
pub fn sqlite3_str_appendf_db(p: &mut StrAccum, db: &mut Sqlite3, zFormat: &[u8], args: &[PfArg]) {
    let f = p.printfFlags;
    p.printfFlags |= SQLITE_PRINTF_INTERNAL;
    sqlite3_str_vappendf(p, Some(db), zFormat, PfArgs::Va(args));
    p.printfFlags = f;
}

/// sqlite3_log(): there is no log callback in this port.
pub fn sqlite3_log(_iErrCode: i32, _zFormat: &[u8], _args: &[PfArg]) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[PfArg]) -> String {
        String::from_utf8(sqlite3_mprintf(fmt.as_bytes(), args)).unwrap()
    }

    #[test]
    fn ints() {
        assert_eq!(f("%d", &[PfArg::Int(42)]), "42");
        assert_eq!(
            f(
                "%5d|%-5d|%05d",
                &[PfArg::Int(42), PfArg::Int(42), PfArg::Int(-42)]
            ),
            "   42|42   |-0042"
        );
        assert_eq!(f("%lld", &[PfArg::Int(i64::MIN)]), "-9223372036854775808");
        assert_eq!(
            f(
                "%x %X %#x %o",
                &[
                    PfArg::Int(255),
                    PfArg::Int(255),
                    PfArg::Int(255),
                    PfArg::Int(8)
                ]
            ),
            "ff FF 0xff 10"
        );
        assert_eq!(f("%,d", &[PfArg::Int(1234567)]), "1,234,567");
        assert_eq!(
            f(
                "%r %r %r %r",
                &[PfArg::Int(1), PfArg::Int(2), PfArg::Int(3), PfArg::Int(11)]
            ),
            "1st 2nd 3rd 11th"
        );
        assert_eq!(f("%u", &[PfArg::Int(-1)]), "4294967295");
        assert_eq!(f("100%%", &[]), "100%");
    }

    #[test]
    fn strings() {
        assert_eq!(f("%s", &[PfArg::Str(Some(b"abc"))]), "abc");
        assert_eq!(
            f(
                "[%5s][%-5s][%.2s]",
                &[
                    PfArg::Str(Some(b"ab")),
                    PfArg::Str(Some(b"ab")),
                    PfArg::Str(Some(b"abc"))
                ]
            ),
            "[   ab][ab   ][ab]"
        );
        assert_eq!(f("%q", &[PfArg::Str(Some(b"it's"))]), "it''s");
        assert_eq!(f("%Q", &[PfArg::Str(Some(b"it's"))]), "'it''s'");
        assert_eq!(f("%Q", &[PfArg::Str(None)]), "NULL");
        assert_eq!(f("%w", &[PfArg::Str(Some(b"a\"b"))]), "a\"\"b");
        assert_eq!(f("%s", &[PfArg::Str(None)]), "");
        assert_eq!(f("%c", &[PfArg::Int(65)]), "A");
    }
}
