//! Port of json.c: the SQLite JSON functions (text JSON, JSON5 input and
//! the JSONB binary encoding), the json_each / json_tree table-valued
//! functions and the per-statement JSON parse cache.
//!
//! Rust notes:
//!
//! * `JsonParse::aBlob` is a `Vec<u8>`.  When the blob is owned (C
//!   `nBlobAlloc>0`) the Vec length is exactly `nBlobAlloc` (the C
//!   allocation size, so that every capacity decision of the C code is
//!   reproduced); when it is "external" (C `nBlobAlloc==0`, a pointer to
//!   an SQL value or a static array) it holds a copy of those bytes.
//! * `JsonString::zBuf` is a `Vec<u8>` whose length is C `nUsed`;
//!   `nAlloc`/`bStatic` replicate the C allocation bookkeeping (which
//!   decides whether a generated text is put into the parse cache).
//! * C passes the `sqlite3_context` around inside JsonString::pCtx; here
//!   the routines that report errors take the context as a parameter.
//! * C strings (paths, JSON text) are byte slices read through `at()`,
//!   which returns 0 past the end (the C NUL terminator).
//! * Parse objects shared with the cache are `Rc<RefCell<JsonParse>>`
//!   (C reference counting, nJPRef).  The cache lives in the statement's
//!   auxiliary data (JSON_CACHE_ID), as in C.  C finds cache entries by
//!   pointer identity of the RCStr text first; Rust emulates that with the
//!   address of the result value's buffer (`JsonParse::zJsonPtr`).

use crate::global::{
    sqlite3CtypeMap, sqlite3Isalnum, sqlite3Isalpha, sqlite3Isdigit, sqlite3Isxdigit,
};
use crate::int::*;
use crate::printf::{PfArg, sqlite3_mprintf, sqlite3_snprintf};
use crate::utf::sqlite3Utf8ReadLimited;
use crate::util::{
    sqlite3AtoF, sqlite3Atoi64, sqlite3DecOrHexToI64, sqlite3HexToInt, sqlite3IsNaN,
    sqlite3StrICmp, sqlite3StrNICmp,
};
use crate::vdbeapi::*;
use crate::vtab::{IndexInfo, Vtab, VtabCursor, VtabModule};
use std::cell::RefCell;
use std::rc::Rc;

/* JSONB element types */
const JSONB_NULL: u8 = 0; /* "null" */
const JSONB_TRUE: u8 = 1; /* "true" */
const JSONB_FALSE: u8 = 2; /* "false" */
const JSONB_INT: u8 = 3; /* integer acceptable to JSON and SQL */
const JSONB_INT5: u8 = 4; /* integer in 0x000 notation */
const JSONB_FLOAT: u8 = 5; /* float acceptable to JSON and SQL */
const JSONB_FLOAT5: u8 = 6; /* float with JSON5 extensions */
const JSONB_TEXT: u8 = 7; /* Text compatible with both JSON and SQL */
const JSONB_TEXTJ: u8 = 8; /* Text with JSON escapes */
const JSONB_TEXT5: u8 = 9; /* Text with JSON-5 escape */
const JSONB_TEXTRAW: u8 = 10; /* SQL text that needs escaping for JSON */
const JSONB_ARRAY: u8 = 11; /* An array */
const JSONB_OBJECT: u8 = 12; /* An object */

/// Human-readable names for the JSONB values.
static jsonbType: [&[u8]; 17] = [
    b"null", b"true", b"false", b"integer", b"integer", b"real", b"real", b"text", b"text",
    b"text", b"text", b"array", b"object", b"", b"", b"", b"",
];

/// jsonIsspace(): the four RFC-8259 whitespace characters.
#[inline]
fn jsonIsspace(x: u8) -> bool {
    matches!(x, 0x09 | 0x0a | 0x0d | 0x20)
}

/// strspn(z, jsonSpaces)
#[inline]
fn jsonSpan(z: &[u8]) -> u32 {
    let mut n = 0usize;
    while n < z.len() && jsonIsspace(z[n]) {
        n += 1;
    }
    n as u32
}

/// Characters that are special to JSON: control characters, '"', '\\'
/// and '\''.
static jsonIsOk: [bool; 256] = {
    let mut a = [true; 256];
    let mut i = 0;
    while i < 0x20 {
        a[i] = false;
        i += 1;
    }
    a[b'"' as usize] = false;
    a[b'\'' as usize] = false;
    a[b'\\' as usize] = false;
    a
};

#[inline]
fn ok(c: u8) -> bool {
    jsonIsOk[c as usize]
}

/// Magic number used for the JSON parse cache in sqlite3_get_auxdata()
const JSON_CACHE_ID: i32 = -429938;
const JSON_CACHE_SIZE: usize = 4;

/// jsonUnescapeOneChar() returns this invalid code point if it encounters
/// a syntax error.
const JSON_INVALID_CHAR: u32 = 0x99999;

/* Allowed values for JsonString.eErr */
const JSTRING_OOM: u8 = 0x01; /* Out of memory */
const JSTRING_MALFORMED: u8 = 0x02; /* Malformed JSONB */
const JSTRING_TOODEEP: u8 = 0x04; /* JSON nested too deep */
const JSTRING_ERR: u8 = 0x08; /* Error already sent to sqlite3_result */

/// The "subtype" set for text JSON values.
const JSON_SUBTYPE: u32 = 74; /* Ascii for "J" */

/* Flags passed via sqlite3_user_data() */
const JSON_JSON: i32 = 0x01; /* Result is always JSON */
const JSON_SQL: i32 = 0x02; /* Result is always SQL */
const JSON_ABPATH: i32 = 0x03; /* Allow abbreviated JSON path specs */
const JSON_ISSET: i32 = 0x04; /* json_set(), not json_insert() */
const JSON_AINS: i32 = 0x08; /* json_array_insert(), not json_insert() */
const JSON_BLOB: i32 = 0x10; /* Use the BLOB output format */

#[inline]
fn JSON_INSERT_TYPE(x: i32) -> usize {
    ((x & 0xC) >> 2) as usize
}

/* Allowed values for JsonParse.eEdit */
const JEDIT_DEL: u8 = 1; /* Delete if exists */
const JEDIT_REPL: u8 = 2; /* Overwrite if exists */
const JEDIT_INS: u8 = 3; /* Insert if not exists */
const JEDIT_SET: u8 = 4; /* Insert or overwrite */
const JEDIT_AINS: u8 = 5; /* array_insert() */

/// Maximum nesting depth of JSON for this implementation.
const JSON_MAX_DEPTH: u16 = 1000;

/* Allowed values for the flgs argument to jsonParseFuncArg() */
const JSON_EDITABLE: u32 = 0x01; /* Generate a writable JsonParse object */
const JSON_KEEPERROR: u32 = 0x02; /* Return non-NULL even if there is an error */
const JSON_NOCACHE: u32 = 0x04; /* Do not use the JSON cache */

/// Byte `i` of a C string held in a slice (0 past the end).
#[inline]
fn at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// `&z[i..]`, empty past the end.
#[inline]
fn tail(z: &[u8], i: usize) -> &[u8] {
    if i < z.len() { &z[i..] } else { &[] }
}

/// `&z[i..i+n]` clamped to the slice.
#[inline]
fn sub(z: &[u8], i: usize, n: usize) -> &[u8] {
    let s = i.min(z.len());
    let e = i.saturating_add(n).min(z.len());
    &z[s..e]
}

/// A C string: the bytes before the first NUL.
#[inline]
fn cstr(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

#[inline]
fn sqlite3JsonId1(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x42) != 0
}
#[inline]
fn sqlite3JsonId2(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x46) != 0
}

#[inline]
fn user_flags(ctx: &mut Context) -> i32 {
    sqlite3_user_data(ctx).as_int() as i32
}

/// sqlite3_value_text() copied out of the value.
#[inline]
fn value_text(m: &mut Mem) -> Option<Vec<u8>> {
    sqlite3_value_text(m).map(|s| s.to_vec())
}

/**************************************************************************
** Objects
**************************************************************************/

/// A cache mapping JSON text into JSONB blobs (C JsonCache).  Entries are
/// stored in age order, oldest first.
#[derive(Default)]
struct JsonCache {
    a: Vec<JsonParseRef>,
}

/// A JSON string under construction (C JsonString).
#[derive(Default, Clone)]
pub struct JsonString {
    /// Content (length = C nUsed)
    zBuf: Vec<u8>,
    /// C nAlloc: bytes of storage the C buffer would have
    nAlloc: u64,
    /// True if C zBuf is the static zSpace[] buffer
    bStatic: bool,
    /// JSTRING_* error flags
    eErr: u8,
    /// C `zBuf!=0` (aggregate contexts start zeroed)
    init: bool,
}

/// A parsed JSON value (C JsonParse).
#[derive(Default, Clone)]
pub struct JsonParse {
    /// JSONB representation of JSON value
    aBlob: Vec<u8>,
    /// Bytes of aBlob[] actually used
    nBlob: u32,
    /// Bytes allocated to aBlob[].  0 if aBlob is external
    nBlobAlloc: u32,
    /// Json text used for parsing
    zJson: Option<Rc<[u8]>>,
    /// Length of the zJson string in bytes
    nJson: i32,
    /// Error location in zJson[]
    iErr: u32,
    /// Nesting depth
    iDepth: u16,
    /// Number of errors seen
    nErr: u8,
    /// Set to true if out of memory
    oom: u8,
    /// True if zJson is an RCStr
    bJsonIsRCStr: bool,
    /// True if input uses non-standard features like JSON5
    hasNonstd: u8,
    /// Do not modify.
    bReadOnly: bool,
    /// Edit operation to apply
    eEdit: u8,
    /// Size change due to the edit
    delta: i32,
    /// Number of bytes to insert
    nIns: u32,
    /// Location of label if search landed on an object value
    iLabel: u32,
    /// Content to be inserted
    aIns: Vec<u8>,
    /// Rust: address of the buffer of the SQL value holding zJson (the C
    /// RCStr pointer identity used by jsonCacheSearch); 0 if unknown.
    zJsonPtr: usize,
}

type JsonParseRef = Rc<RefCell<JsonParse>>;

impl JsonParse {
    /// aBlob[i] (0 past the end of the storage).
    #[inline]
    fn ab(&self, i: u32) -> u8 {
        at(&self.aBlob, i as usize)
    }
}

/**************************************************************************
** Utility routines for dealing with JsonCache objects
**************************************************************************/

fn jsonCacheGet(ctx: &mut Context) -> Option<Rc<RefCell<JsonCache>>> {
    let a = sqlite3_get_auxdata(ctx, JSON_CACHE_ID)?;
    a.downcast::<RefCell<JsonCache>>().ok()
}

/// Insert a new entry into the cache.  If the cache is full, expel the
/// least recently used entry.  Return SQLITE_OK on success or a result
/// code otherwise.
fn jsonCacheInsert(ctx: &mut Context, pParse: &JsonParseRef) -> i32 {
    let p = match jsonCacheGet(ctx) {
        Some(p) => p,
        None => {
            let c: Rc<RefCell<JsonCache>> = Rc::new(RefCell::new(JsonCache::default()));
            sqlite3_set_auxdata(ctx, JSON_CACHE_ID, c);
            match jsonCacheGet(ctx) {
                Some(p) => p,
                None => return SQLITE_NOMEM,
            }
        }
    };
    let mut p = p.borrow_mut();
    if p.a.len() >= JSON_CACHE_SIZE {
        p.a.remove(0);
    }
    {
        let mut x = pParse.borrow_mut();
        debug_assert!(x.zJson.is_some() && x.bJsonIsRCStr && x.delta == 0);
        debug_assert!(x.nBlobAlloc > 0);
        x.eEdit = 0;
        x.bReadOnly = true;
    }
    p.a.push(pParse.clone());
    SQLITE_OK
}

/// Search for a cached translation the json text supplied by pArg.
/// Return the JsonParse object if found.  The matching entry becomes the
/// most-recently used entry.
fn jsonCacheSearch(ctx: &mut Context, pArg: &mut Mem) -> Option<JsonParseRef> {
    if sqlite3_value_type(pArg) != SQLITE_TEXT {
        return None;
    }
    let ptr = pArg.z.as_ptr() as usize;
    let zJson = sqlite3_value_text(pArg)?.to_vec();
    let p = jsonCacheGet(ctx)?;
    let mut p = p.borrow_mut();
    let n = p.a.len();
    let same = |e: &JsonParseRef| {
        let e = e.borrow();
        e.zJson.as_deref() == Some(&zJson[..])
    };
    let mut i = 0;
    while i < n {
        if p.a[i].borrow().zJsonPtr == ptr && same(&p.a[i]) {
            break;
        }
        i += 1;
    }
    if i >= n {
        i = 0;
        while i < n {
            if same(&p.a[i]) {
                break;
            }
            i += 1;
        }
    }
    if i < n {
        if i < n - 1 {
            /* Make the matching entry the most recently used entry */
            let tmp = p.a.remove(i);
            p.a.push(tmp);
            i = n - 1;
        }
        debug_assert!(p.a[i].borrow().delta == 0);
        Some(p.a[i].clone())
    } else {
        None
    }
}

/**************************************************************************
** Utility routines for dealing with JsonString objects
**************************************************************************/

/// Turn uninitialized bulk memory into a valid JsonString object holding
/// a zero-length string.
fn jsonStringZero(p: &mut JsonString) {
    p.zBuf.clear();
    p.nAlloc = 100;
    p.bStatic = true;
    p.init = true;
}

/// Initialize the JsonString object
fn jsonStringInit(p: &mut JsonString) {
    p.eErr = 0;
    jsonStringZero(p);
}

fn jsonStringNew() -> JsonString {
    let mut s = JsonString::default();
    jsonStringInit(&mut s);
    s
}

/// Free all allocated memory and reset the JsonString object back to its
/// initial state.
fn jsonStringReset(p: &mut JsonString) {
    p.zBuf = Vec::new();
    jsonStringZero(p);
}

/// Report JSON nested too deep
fn jsonStringTooDeep(p: &mut JsonString, ctx: &mut Context) {
    p.eErr |= JSTRING_TOODEEP;
    sqlite3_result_error(ctx, Some(b"JSON nested too deep"), -1);
    jsonStringReset(p);
}

/// Enlarge pJson->zBuf so that it can hold at least N more bytes.
/// Return zero on success.  Return non-zero on an OOM error (or, as in C,
/// when the string is static and already in error).
fn jsonStringGrow(p: &mut JsonString, N: u32) -> i32 {
    let nTotal = if (N as u64) < p.nAlloc {
        p.nAlloc * 2
    } else {
        p.nAlloc + N as u64 + 10
    };
    if p.bStatic {
        if p.eErr != 0 {
            return 1;
        }
        p.bStatic = false;
    }
    p.nAlloc = nTotal;
    SQLITE_OK
}

/// Append N bytes from zIn onto the end of the JsonString string.
fn jsonAppendRaw(p: &mut JsonString, zIn: &[u8]) {
    let N = zIn.len() as u64;
    if N == 0 {
        return;
    }
    if N + p.zBuf.len() as u64 >= p.nAlloc && jsonStringGrow(p, N as u32) != 0 {
        return;
    }
    p.zBuf.extend_from_slice(zIn);
}

fn jsonAppendRawNZ(p: &mut JsonString, zIn: &[u8]) {
    debug_assert!(!zIn.is_empty());
    let N = zIn.len() as u64;
    if N + p.zBuf.len() as u64 >= p.nAlloc && jsonStringGrow(p, N as u32) != 0 {
        return;
    }
    p.zBuf.extend_from_slice(zIn);
}

/// Append formatted text (not to exceed N bytes) to the JsonString.
fn jsonPrintf(N: u32, p: &mut JsonString, zFormat: &[u8], args: &[PfArg]) {
    if p.zBuf.len() as u64 + N as u64 >= p.nAlloc && jsonStringGrow(p, N) != 0 {
        return;
    }
    let s = sqlite3_snprintf(N as i32, zFormat, args);
    p.zBuf.extend_from_slice(cstr(&s));
}

/// Append a single character
fn jsonAppendChar(p: &mut JsonString, c: u8) {
    if p.zBuf.len() as u64 >= p.nAlloc && jsonStringGrow(p, 1) != 0 {
        return;
    }
    p.zBuf.push(c);
}

/// Remove a single character from the end of the string
fn jsonStringTrimOneChar(p: &mut JsonString) {
    if p.eErr == 0 {
        debug_assert!(!p.zBuf.is_empty());
        p.zBuf.pop();
    }
}

/// Make sure there is a zero terminator on p->zBuf[].
fn jsonStringTerminate(p: &mut JsonString) -> bool {
    jsonAppendChar(p, 0);
    jsonStringTrimOneChar(p);
    p.eErr == 0
}

/// Append a comma separator to the output buffer, if the previous
/// character is not '[' or '{'.
fn jsonAppendSeparator(p: &mut JsonString) {
    let Some(&c) = p.zBuf.last() else {
        return;
    };
    if c == b'[' || c == b'{' {
        return;
    }
    jsonAppendChar(p, b',');
}

/// c is a control character.  Append the canonical JSON representation of
/// that control character to p.
fn jsonAppendControlChar(p: &mut JsonString, c: u8) {
    static aSpecial: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, b'b', b't', b'n', 0, b'f', b'r', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ];
    debug_assert!((c as usize) < aSpecial.len());
    let hex = b"0123456789abcdef";
    if aSpecial[c as usize] != 0 {
        p.zBuf.push(b'\\');
        p.zBuf.push(aSpecial[c as usize]);
    } else {
        p.zBuf.extend_from_slice(b"\\u00");
        p.zBuf.push(hex[(c >> 4) as usize]);
        p.zBuf.push(hex[(c & 0xf) as usize]);
    }
}

/// Append the N-byte string in zIn to the end of the JsonString string
/// under construction.  Enclose the string in double-quotes ("...") and
/// escape any double-quotes or backslash characters contained within the
/// string.
fn jsonAppendString(p: &mut JsonString, zIn: &[u8]) {
    let mut N = zIn.len() as u32;
    let mut z = zIn;
    if (N as u64 + p.zBuf.len() as u64 + 2 >= p.nAlloc) && jsonStringGrow(p, N + 2) != 0 {
        return;
    }
    p.zBuf.push(b'"');
    loop {
        let mut k = 0usize;
        while k < N as usize && ok(z[k]) {
            k += 1;
        }
        if k >= N as usize {
            if k > 0 {
                p.zBuf.extend_from_slice(&z[..k]);
            }
            break;
        }
        if k > 0 {
            p.zBuf.extend_from_slice(&z[..k]);
            z = &z[k..];
            N -= k as u32;
        }
        let c = z[0];
        if c == b'"' || c == b'\\' {
            if (p.zBuf.len() as u64 + N as u64 + 3 > p.nAlloc) && jsonStringGrow(p, N + 3) != 0 {
                return;
            }
            p.zBuf.push(b'\\');
            p.zBuf.push(c);
        } else if c == b'\'' {
            p.zBuf.push(c);
        } else {
            if (p.zBuf.len() as u64 + N as u64 + 7 > p.nAlloc) && jsonStringGrow(p, N + 7) != 0 {
                return;
            }
            jsonAppendControlChar(p, c);
        }
        z = &z[1..];
        N -= 1;
    }
    p.zBuf.push(b'"');
}

/// Append an sqlite3_value (such as a function parameter) to the JSON
/// string under construction in p.
fn jsonAppendSqlValue(p: &mut JsonString, pValue: &mut Mem, ctx: &mut Context) {
    match sqlite3_value_type(pValue) {
        SQLITE_NULL => {
            jsonAppendRawNZ(p, b"null");
        }
        SQLITE_FLOAT => {
            let r = sqlite3_value_double(pValue);
            jsonPrintf(100, p, b"%!0.17g", &[PfArg::from(r)]);
        }
        SQLITE_INTEGER => {
            let z = value_text(pValue).unwrap_or_default();
            jsonAppendRaw(p, &z);
        }
        SQLITE_TEXT => {
            let z = value_text(pValue).unwrap_or_default();
            if sqlite3_value_subtype(pValue) == JSON_SUBTYPE {
                jsonAppendRaw(p, &z);
            } else {
                jsonAppendString(p, &z);
            }
        }
        _ => {
            let mut px = JsonParse::default();
            if jsonArgIsJsonb(pValue, &mut px) {
                jsonTranslateBlobToText(&mut px, 0, p, ctx);
            } else if p.eErr == 0 {
                sqlite3_result_error(ctx, Some(b"JSON cannot hold BLOB values"), -1);
                p.eErr = JSTRING_ERR;
                jsonStringReset(p);
            }
        }
    }
}

/// Make the text in p (which is probably a generated JSON text string) the
/// result of the SQL function.
///
/// The JsonString is reset.
///
/// If pParse is not None, then the SQL string in p is loaded into the
/// zJson field of the pParse object as a RCStr and the pParse is added to
/// the cache.
fn jsonReturnString(p: &mut JsonString, pParse: Option<&JsonParseRef>, ctx: &mut Context) {
    jsonStringTerminate(p);
    if p.eErr == 0 {
        let flags = user_flags(ctx);
        if flags & JSON_BLOB != 0 {
            jsonReturnStringAsBlob(p, ctx);
        } else if p.bStatic {
            sqlite3_result_text64(ctx, Some(&p.zBuf), p.zBuf.len() as u64, SQLITE_UTF8);
        } else {
            let mut cached: Option<&JsonParseRef> = None;
            if let Some(pp) = pParse {
                let doit = {
                    let x = pp.borrow();
                    !x.bJsonIsRCStr && x.nBlobAlloc > 0
                };
                if doit {
                    {
                        let mut x = pp.borrow_mut();
                        x.zJson = Some(Rc::from(&p.zBuf[..]));
                        x.nJson = p.zBuf.len() as i32;
                        x.bJsonIsRCStr = true;
                    }
                    let rc = jsonCacheInsert(ctx, pp);
                    if rc == SQLITE_NOMEM {
                        sqlite3_result_error_nomem(ctx);
                        jsonStringReset(p);
                        return;
                    }
                    cached = Some(pp);
                }
            }
            sqlite3_result_text64(ctx, Some(&p.zBuf), p.zBuf.len() as u64, SQLITE_UTF8);
            if let Some(pp) = cached {
                pp.borrow_mut().zJsonPtr = ctx.pOut.z.as_ptr() as usize;
            }
        }
    } else if p.eErr & JSTRING_OOM != 0 {
        sqlite3_result_error_nomem(ctx);
    } else if p.eErr & JSTRING_TOODEEP != 0 {
        /* error already in ctx */
    } else if p.eErr & JSTRING_MALFORMED != 0 {
        sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
    }
    jsonStringReset(p);
}

/**************************************************************************
** Utility routines for dealing with JsonParse objects
**************************************************************************/

/// Reclaim all memory allocated by a JsonParse object.
fn jsonParseReset(pParse: &mut JsonParse) {
    if pParse.bJsonIsRCStr {
        pParse.zJson = None;
        pParse.nJson = 0;
        pParse.bJsonIsRCStr = false;
    }
    if pParse.nBlobAlloc != 0 {
        pParse.aBlob = Vec::new();
        pParse.nBlob = 0;
        pParse.nBlobAlloc = 0;
    }
}

/**************************************************************************
** Utility routines for the JSON text parser
**************************************************************************/

/// Translate a single byte of Hex into an integer.
#[inline]
fn jsonHexToInt(h: u8) -> u8 {
    let h = h as i32;
    let h = h + 9 * (1 & (h >> 6));
    (h & 0xf) as u8
}

/// Convert a 4-byte hex string into an integer
fn jsonHexToInt4(z: &[u8]) -> u32 {
    ((jsonHexToInt(at(z, 0)) as u32) << 12)
        + ((jsonHexToInt(at(z, 1)) as u32) << 8)
        + ((jsonHexToInt(at(z, 2)) as u32) << 4)
        + jsonHexToInt(at(z, 3)) as u32
}

/// Return true if z[] begins with 2 (or more) hexadecimal digits
fn jsonIs2Hex(z: &[u8]) -> bool {
    sqlite3Isxdigit(at(z, 0)) && sqlite3Isxdigit(at(z, 1))
}

/// Return true if z[] begins with 4 (or more) hexadecimal digits
fn jsonIs4Hex(z: &[u8]) -> bool {
    jsonIs2Hex(z) && jsonIs2Hex(tail(z, 2))
}

/// Return the number of bytes of JSON5 whitespace at the beginning of the
/// input string z[] (including comments).
fn json5Whitespace(z: &[u8]) -> u32 {
    let mut n = 0usize;
    loop {
        match at(z, n) {
            0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 => {
                n += 1;
            }
            b'/' => {
                if at(z, n + 1) == b'*' && at(z, n + 2) != 0 {
                    let mut j = n + 3;
                    while at(z, j) != b'/' || at(z, j - 1) != b'*' {
                        if at(z, j) == 0 {
                            return n as u32;
                        }
                        j += 1;
                    }
                    n = j + 1;
                } else if at(z, n + 1) == b'/' {
                    let mut j = n + 2;
                    loop {
                        let c = at(z, j);
                        if c == 0 {
                            break;
                        }
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        if 0xe2 == c
                            && 0x80 == at(z, j + 1)
                            && (0xa8 == at(z, j + 2) || 0xa9 == at(z, j + 2))
                        {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    n = j;
                    if at(z, n) != 0 {
                        n += 1;
                    }
                } else {
                    return n as u32;
                }
            }
            0xc2 => {
                if at(z, n + 1) == 0xa0 {
                    n += 2;
                } else {
                    return n as u32;
                }
            }
            0xe1 => {
                if at(z, n + 1) == 0x9a && at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    return n as u32;
                }
            }
            0xe2 => {
                if at(z, n + 1) == 0x80 {
                    let c = at(z, n + 2);
                    if c < 0x80 {
                        return n as u32;
                    }
                    if c <= 0x8a || c == 0xa8 || c == 0xa9 || c == 0xaf {
                        n += 3;
                    } else {
                        return n as u32;
                    }
                } else if at(z, n + 1) == 0x81 && at(z, n + 2) == 0x9f {
                    n += 3;
                } else {
                    return n as u32;
                }
            }
            0xe3 => {
                if at(z, n + 1) == 0x80 && at(z, n + 2) == 0x80 {
                    n += 3;
                } else {
                    return n as u32;
                }
            }
            0xef => {
                if at(z, n + 1) == 0xbb && at(z, n + 2) == 0xbf {
                    n += 3;
                } else {
                    return n as u32;
                }
            }
            _ => {
                return n as u32;
            }
        }
    }
}

/// Extra floating-point literals to allow in JSON.
struct NanInfName {
    c1: u8,
    c2: u8,
    n: u8,
    eType: u8,
    zMatch: &'static [u8],
}
static aNanInfName: [NanInfName; 5] = [
    NanInfName {
        c1: b'i',
        c2: b'I',
        n: 3,
        eType: JSONB_FLOAT,
        zMatch: b"inf",
    },
    NanInfName {
        c1: b'i',
        c2: b'I',
        n: 8,
        eType: JSONB_FLOAT,
        zMatch: b"infinity",
    },
    NanInfName {
        c1: b'n',
        c2: b'N',
        n: 3,
        eType: JSONB_NULL,
        zMatch: b"NaN",
    },
    NanInfName {
        c1: b'q',
        c2: b'Q',
        n: 4,
        eType: JSONB_NULL,
        zMatch: b"QNaN",
    },
    NanInfName {
        c1: b's',
        c2: b'S',
        n: 4,
        eType: JSONB_NULL,
        zMatch: b"SNaN",
    },
];

/// Report the wrong number of arguments for json_insert(), json_replace()
/// or json_set().
fn jsonWrongNumArgs(pCtx: &mut Context, zFuncName: &[u8]) {
    let zMsg = sqlite3_mprintf(
        b"json_%s() needs an odd number of arguments",
        &[PfArg::from(zFuncName)],
    );
    sqlite3_result_error(pCtx, Some(&zMsg), -1);
}

/****************************************************************************
** Utility routines for dealing with the binary BLOB representation of JSON
****************************************************************************/

/// Expand pParse->aBlob so that it holds at least N bytes.
/// Return the number of errors.
fn jsonBlobExpand(pParse: &mut JsonParse, N: u64) -> i32 {
    debug_assert!(N > pParse.nBlobAlloc as u64);
    let mut t: u64 = if pParse.nBlobAlloc == 0 {
        100
    } else {
        pParse.nBlobAlloc as u64 * 2
    };
    if t < N {
        t = N + 100;
    }
    debug_assert!(t < 0x7fffffff);
    pParse.aBlob.resize(t as usize, 0);
    pParse.nBlobAlloc = t as u32;
    0
}

/// If pParse->aBlob is not previously editable (because it is taken from
/// sqlite3_value_blob(), as indicated by the fact that
/// pParse->nBlobAlloc==0 and pParse->nBlob>0) then make it editable by
/// making a copy.  Return true on success.
fn jsonBlobMakeEditable(pParse: &mut JsonParse, nExtra: u32) -> bool {
    debug_assert!(!pParse.bReadOnly);
    if pParse.oom != 0 {
        return false;
    }
    if pParse.nBlobAlloc > 0 {
        return true;
    }
    let aOld = std::mem::take(&mut pParse.aBlob);
    let nSize = pParse.nBlob as u64 + nExtra as u64;
    if jsonBlobExpand(pParse, nSize) != 0 {
        return false;
    }
    let n = pParse.nBlob as usize;
    pParse.aBlob[..n].copy_from_slice(sub(&aOld, 0, n));
    true
}

/// Append a single character.
fn jsonBlobAppendOneByte(pParse: &mut JsonParse, c: u8) {
    if pParse.nBlob >= pParse.nBlobAlloc {
        jsonBlobExpand(pParse, pParse.nBlob as u64 + 1);
        if pParse.oom != 0 {
            return;
        }
    }
    let n = pParse.nBlob as usize;
    pParse.aBlob[n] = c;
    pParse.nBlob += 1;
}

/// Append a node type byte together with the payload size and possibly
/// also the payload.
///
/// If aPayload is not None, then it is the payload which is also appended.
/// If aPayload is None, the pParse->aBlob[] array is resized (if
/// necessary) so that it is big enough to hold the payload, but the
/// payload is not appended and pParse->nBlob is left pointing to where the
/// first byte of payload will eventually be.
fn jsonBlobAppendNode(pParse: &mut JsonParse, eType: u8, szPayload: u64, aPayload: Option<&[u8]>) {
    if pParse.nBlob as u64 + szPayload + 9 > pParse.nBlobAlloc as u64 {
        /* jsonBlobExpandAndAppendNode() */
        if jsonBlobExpand(pParse, pParse.nBlob as u64 + szPayload + 9) != 0 {
            return;
        }
    }
    let b = pParse.nBlob as usize;
    let a = &mut pParse.aBlob[b..];
    if szPayload <= 11 {
        a[0] = eType | ((szPayload as u8) << 4);
        pParse.nBlob += 1;
    } else if szPayload <= 0xff {
        a[0] = eType | 0xc0;
        a[1] = (szPayload & 0xff) as u8;
        pParse.nBlob += 2;
    } else if szPayload <= 0xffff {
        a[0] = eType | 0xd0;
        a[1] = ((szPayload >> 8) & 0xff) as u8;
        a[2] = (szPayload & 0xff) as u8;
        pParse.nBlob += 3;
    } else {
        a[0] = eType | 0xe0;
        a[1] = ((szPayload >> 24) & 0xff) as u8;
        a[2] = ((szPayload >> 16) & 0xff) as u8;
        a[3] = ((szPayload >> 8) & 0xff) as u8;
        a[4] = (szPayload & 0xff) as u8;
        pParse.nBlob += 5;
    }
    if let Some(pl) = aPayload {
        let s = pParse.nBlob as usize;
        let n = szPayload as usize;
        pParse.aBlob[s..s + n].copy_from_slice(&pl[..n]);
        pParse.nBlob += szPayload as u32;
    }
}

/// Change the payload size for the node at index i to be szPayload.
fn jsonBlobChangePayloadSize(pParse: &mut JsonParse, i: u32, szPayload: u32) -> i32 {
    if pParse.oom != 0 {
        return 0;
    }
    let iu = i as usize;
    let szType = pParse.aBlob[iu] >> 4;
    let nExtra: i32 = if szType <= 11 {
        0
    } else if szType == 12 {
        1
    } else if szType == 13 {
        2
    } else if szType == 14 {
        4
    } else {
        8
    };
    let nNeeded: i32 = if szPayload <= 11 {
        0
    } else if szPayload <= 0xff {
        1
    } else if szPayload <= 0xffff {
        2
    } else {
        4
    };
    let delta = nNeeded - nExtra;
    if delta != 0 {
        let newSize = pParse.nBlob as i64 + delta as i64;
        let nb = pParse.nBlob as usize;
        if delta > 0 {
            if newSize > pParse.nBlobAlloc as i64 && jsonBlobExpand(pParse, newSize as u64) != 0 {
                return 0;
            }
            let d = delta as usize;
            pParse.aBlob.copy_within(iu + 1..nb, iu + 1 + d);
        } else {
            let d = (-delta) as usize;
            pParse.aBlob.copy_within(iu + 1 + d..nb, iu + 1);
        }
        pParse.nBlob = newSize as u32;
    }
    let a = &mut pParse.aBlob[iu..];
    if nNeeded == 0 {
        a[0] = (a[0] & 0x0f) | ((szPayload as u8) << 4);
    } else if nNeeded == 1 {
        a[0] = (a[0] & 0x0f) | 0xc0;
        a[1] = (szPayload & 0xff) as u8;
    } else if nNeeded == 2 {
        a[0] = (a[0] & 0x0f) | 0xd0;
        a[1] = ((szPayload >> 8) & 0xff) as u8;
        a[2] = (szPayload & 0xff) as u8;
    } else {
        a[0] = (a[0] & 0x0f) | 0xe0;
        a[1] = ((szPayload >> 24) & 0xff) as u8;
        a[2] = ((szPayload >> 16) & 0xff) as u8;
        a[3] = ((szPayload >> 8) & 0xff) as u8;
        a[4] = (szPayload & 0xff) as u8;
    }
    delta
}

/// If z[0] is 'u' and is followed by exactly 4 hexadecimal character, then
/// set *pOp to JSONB_TEXTJ and return true.
fn jsonIs4HexB(z: &[u8], pOp: &mut u8) -> bool {
    if at(z, 0) != b'u' {
        return false;
    }
    if !jsonIs4Hex(tail(z, 1)) {
        return false;
    }
    *pOp = JSONB_TEXTJ;
    true
}

/// Check a single element of the JSONB in pParse for validity.
///
/// Return 0 if everything is correct.  Return the 1-based byte offset of
/// the error if a problem is detected.
fn jsonbValidityCheck(pParse: &JsonParse, i: u32, iEnd: u32, iDepth: u32) -> u32 {
    if iDepth > JSON_MAX_DEPTH as u32 {
        return i + 1;
    }
    let (n, sz) = jsonbPayloadSize(pParse, i);
    if n == 0 {
        return i + 1;
    }
    if i + n + sz != iEnd {
        return i + 1;
    }
    let z = |k: u32| pParse.ab(k);
    let x = z(i) & 0x0f;
    match x {
        JSONB_NULL | JSONB_TRUE | JSONB_FALSE => {
            if n + sz == 1 {
                0
            } else {
                i + 1
            }
        }
        JSONB_INT => {
            if sz < 1 {
                return i + 1;
            }
            let mut j = i + n;
            if z(j) == b'-' {
                j += 1;
                if sz < 2 {
                    return i + 1;
                }
            }
            let k = i + n + sz;
            while j < k {
                if sqlite3Isdigit(z(j)) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            0
        }
        JSONB_INT5 => {
            if sz < 3 {
                return i + 1;
            }
            let mut j = i + n;
            if z(j) == b'-' {
                if sz < 4 {
                    return i + 1;
                }
                j += 1;
            }
            if z(j) != b'0' {
                return i + 1;
            }
            if z(j + 1) != b'x' && z(j + 1) != b'X' {
                return j + 2;
            }
            j += 2;
            let k = i + n + sz;
            while j < k {
                if sqlite3Isxdigit(z(j)) {
                    j += 1;
                } else {
                    return j + 1;
                }
            }
            0
        }
        JSONB_FLOAT | JSONB_FLOAT5 => {
            let mut seen: u8 = 0; /* 0: initial.  1: '.' seen  2: 'e' seen */
            if sz < 2 {
                return i + 1;
            }
            let mut j = i + n;
            let k = j + sz;
            if z(j) == b'-' {
                j += 1;
                if sz < 3 {
                    return i + 1;
                }
            }
            if z(j) == b'.' {
                if x == JSONB_FLOAT {
                    return j + 1;
                }
                if !sqlite3Isdigit(z(j + 1)) {
                    return j + 1;
                }
                j += 2;
                seen = 1;
            } else if z(j) == b'0' && x == JSONB_FLOAT {
                if j + 3 > k {
                    return j + 1;
                }
                if z(j + 1) != b'.' && z(j + 1) != b'e' && z(j + 1) != b'E' {
                    return j + 1;
                }
                j += 1;
            }
            while j < k {
                if sqlite3Isdigit(z(j)) {
                    j += 1;
                    continue;
                }
                if z(j) == b'.' {
                    if seen > 0 {
                        return j + 1;
                    }
                    if x == JSONB_FLOAT && (j == k - 1 || !sqlite3Isdigit(z(j + 1))) {
                        return j + 1;
                    }
                    seen = 1;
                    j += 1;
                    continue;
                }
                if z(j) == b'e' || z(j) == b'E' {
                    if seen == 2 {
                        return j + 1;
                    }
                    if j == k - 1 {
                        return j + 1;
                    }
                    if z(j + 1) == b'+' || z(j + 1) == b'-' {
                        j += 1;
                        if j == k - 1 {
                            return j + 1;
                        }
                    }
                    seen = 2;
                    j += 1;
                    continue;
                }
                return j + 1;
            }
            if seen == 0 {
                return i + 1;
            }
            0
        }
        JSONB_TEXT => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                if !ok(z(j)) && z(j) != b'\'' {
                    return j + 1;
                }
                j += 1;
            }
            0
        }
        JSONB_TEXTJ | JSONB_TEXT5 => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                if !ok(z(j)) && z(j) != b'\'' {
                    if z(j) == b'"' {
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if z(j) <= 0x1f {
                        /* Control characters in JSON5 string literals are ok */
                        if x == JSONB_TEXTJ {
                            return j + 1;
                        }
                    } else if z(j) != b'\\' || j + 1 >= k {
                        return j + 1;
                    } else if b"\"\\/bfnrt\0".contains(&z(j + 1)) {
                        /* (strchr() also matches the NUL terminator) */
                        j += 1;
                    } else if z(j + 1) == b'u' {
                        if j + 5 >= k {
                            return j + 1;
                        }
                        if !jsonIs4Hex(tail(&pParse.aBlob, (j + 2) as usize)) {
                            return j + 1;
                        }
                        j += 1;
                    } else if x != JSONB_TEXT5 {
                        return j + 1;
                    } else {
                        let mut c = 0u32;
                        let szC =
                            jsonUnescapeOneChar(tail(&pParse.aBlob, j as usize), k - j, &mut c);
                        if c == JSON_INVALID_CHAR {
                            return j + 1;
                        }
                        j += szC - 1;
                    }
                }
                j += 1;
            }
            0
        }
        JSONB_TEXTRAW => 0,
        JSONB_ARRAY => {
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                let (n, sz) = jsonbPayloadSize(pParse, j);
                if n == 0 {
                    return j + 1;
                }
                if j + n + sz > k {
                    return j + 1;
                }
                let sub = jsonbValidityCheck(pParse, j, j + n + sz, iDepth + 1);
                if sub != 0 {
                    return sub;
                }
                j += n + sz;
            }
            debug_assert!(j == k);
            0
        }
        JSONB_OBJECT => {
            let mut cnt = 0u32;
            let mut j = i + n;
            let k = j + sz;
            while j < k {
                let (n, sz) = jsonbPayloadSize(pParse, j);
                if n == 0 {
                    return j + 1;
                }
                if j + n + sz > k {
                    return j + 1;
                }
                if (cnt & 1) == 0 {
                    let x = z(j) & 0x0f;
                    if !(JSONB_TEXT..=JSONB_TEXTRAW).contains(&x) {
                        return j + 1;
                    }
                }
                let sub = jsonbValidityCheck(pParse, j, j + n + sz, iDepth + 1);
                if sub != 0 {
                    return sub;
                }
                cnt += 1;
                j += n + sz;
            }
            debug_assert!(j == k);
            if (cnt & 1) != 0 {
                return j + 1;
            }
            0
        }
        _ => i + 1,
    }
}

/// Translate a single element of JSON text at z[i] into its equivalent
/// binary JSONB representation.  Append the translation into
/// pParse->aBlob[] beginning at pParse->nBlob.
///
/// Return the index of the first character past the end of the element
/// parsed, or one of the following special result codes:
///
///      0    End of input
///     -1    Syntax error or OOM
///     -2    '}' seen   \
///     -3    ']' seen    \___  For these returns, pParse->iErr is set to
///     -4    ',' seen    /     the index in zJson[] of the seen character
///     -5    ':' seen   /
fn jsonTranslateTextToBlob(pParse: &mut JsonParse, z: &[u8], i0: u32) -> i32 {
    let zc = |k: u32| at(z, k as usize);
    let mut i = i0;
    loop {
        /* json_parse_restart: */
        match zc(i) {
            b'{' => {
                /* Parse object */
                let iThis = pParse.nBlob;
                let szp = (pParse.nJson as u32).wrapping_sub(i) as u64;
                jsonBlobAppendNode(pParse, JSONB_OBJECT, szp, None);
                pParse.iDepth = pParse.iDepth.wrapping_add(1);
                if pParse.iDepth > JSON_MAX_DEPTH {
                    pParse.iErr = i;
                    return -1;
                }
                let iStart = pParse.nBlob;
                let mut j = i + 1;
                loop {
                    let iBlob = pParse.nBlob;
                    let mut x = jsonTranslateTextToBlob(pParse, z, j);
                    if x <= 0 {
                        if x == -2 {
                            j = pParse.iErr;
                            if pParse.nBlob != iStart {
                                pParse.hasNonstd = 1;
                            }
                            break;
                        }
                        j += json5Whitespace(tail(z, j as usize));
                        let mut op = JSONB_TEXT;
                        if sqlite3JsonId1(zc(j))
                            || (zc(j) == b'\\' && jsonIs4HexB(tail(z, j as usize + 1), &mut op))
                        {
                            let mut k = j + 1;
                            while (sqlite3JsonId2(zc(k))
                                && json5Whitespace(tail(z, k as usize)) == 0)
                                || (zc(k) == b'\\' && jsonIs4HexB(tail(z, k as usize + 1), &mut op))
                            {
                                k += 1;
                            }
                            debug_assert!(iBlob == pParse.nBlob);
                            jsonBlobAppendNode(
                                pParse,
                                op,
                                (k - j) as u64,
                                Some(&z[j as usize..k as usize]),
                            );
                            pParse.hasNonstd = 1;
                            x = k as i32;
                        } else {
                            if x != -1 {
                                pParse.iErr = j;
                            }
                            return -1;
                        }
                    }
                    if pParse.oom != 0 {
                        return -1;
                    }
                    let t = pParse.ab(iBlob) & 0x0f;
                    if !(JSONB_TEXT..=JSONB_TEXTRAW).contains(&t) {
                        pParse.iErr = j;
                        return -1;
                    }
                    j = x as u32;
                    let mut gotoValue = false;
                    if zc(j) == b':' {
                        j += 1;
                    } else {
                        if jsonIsspace(zc(j)) {
                            loop {
                                j += 1;
                                if !jsonIsspace(zc(j)) {
                                    break;
                                }
                            }
                            if zc(j) == b':' {
                                j += 1;
                                gotoValue = true;
                            }
                        }
                        if !gotoValue {
                            x = jsonTranslateTextToBlob(pParse, z, j);
                            if x != -5 {
                                if x != -1 {
                                    pParse.iErr = j;
                                }
                                return -1;
                            }
                            j = pParse.iErr + 1;
                        }
                    }
                    /* parse_object_value: */
                    x = jsonTranslateTextToBlob(pParse, z, j);
                    if x <= 0 {
                        if x != -1 {
                            pParse.iErr = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if zc(j) == b',' {
                        j += 1;
                        continue;
                    } else if zc(j) == b'}' {
                        break;
                    } else {
                        if jsonIsspace(zc(j)) {
                            j += 1 + jsonSpan(tail(z, j as usize + 1));
                            if zc(j) == b',' {
                                j += 1;
                                continue;
                            } else if zc(j) == b'}' {
                                break;
                            }
                        }
                        x = jsonTranslateTextToBlob(pParse, z, j);
                        if x == -4 {
                            j = pParse.iErr + 1;
                            continue;
                        }
                        if x == -2 {
                            j = pParse.iErr;
                            break;
                        }
                    }
                    pParse.iErr = j;
                    return -1;
                }
                let nb = pParse.nBlob;
                jsonBlobChangePayloadSize(pParse, iThis, nb - iStart);
                pParse.iDepth = pParse.iDepth.wrapping_sub(1);
                return (j + 1) as i32;
            }
            b'[' => {
                /* Parse array */
                let iThis = pParse.nBlob;
                let szp = (pParse.nJson as u32).wrapping_sub(i) as u64;
                jsonBlobAppendNode(pParse, JSONB_ARRAY, szp, None);
                let iStart = pParse.nBlob;
                if pParse.oom != 0 {
                    return -1;
                }
                pParse.iDepth = pParse.iDepth.wrapping_add(1);
                if pParse.iDepth > JSON_MAX_DEPTH {
                    pParse.iErr = i;
                    return -1;
                }
                let mut j = i + 1;
                loop {
                    let mut x = jsonTranslateTextToBlob(pParse, z, j);
                    if x <= 0 {
                        if x == -3 {
                            j = pParse.iErr;
                            if pParse.nBlob != iStart {
                                pParse.hasNonstd = 1;
                            }
                            break;
                        }
                        if x != -1 {
                            pParse.iErr = j;
                        }
                        return -1;
                    }
                    j = x as u32;
                    if zc(j) == b',' {
                        j += 1;
                        continue;
                    } else if zc(j) == b']' {
                        break;
                    } else {
                        if jsonIsspace(zc(j)) {
                            j += 1 + jsonSpan(tail(z, j as usize + 1));
                            if zc(j) == b',' {
                                j += 1;
                                continue;
                            } else if zc(j) == b']' {
                                break;
                            }
                        }
                        x = jsonTranslateTextToBlob(pParse, z, j);
                        if x == -4 {
                            j = pParse.iErr + 1;
                            continue;
                        }
                        if x == -3 {
                            j = pParse.iErr;
                            break;
                        }
                    }
                    pParse.iErr = j;
                    return -1;
                }
                let nb = pParse.nBlob;
                jsonBlobChangePayloadSize(pParse, iThis, nb - iStart);
                pParse.iDepth = pParse.iDepth.wrapping_sub(1);
                return (j + 1) as i32;
            }
            b'\'' | b'"' => {
                /* Parse string */
                if zc(i) == b'\'' {
                    pParse.hasNonstd = 1;
                }
                let mut opcode = JSONB_TEXT;
                let cDelim = zc(i);
                let mut j = i + 1;
                loop {
                    if ok(zc(j)) {
                        if !ok(zc(j + 1)) {
                            j += 1;
                        } else if !ok(zc(j + 2)) {
                            j += 2;
                        } else {
                            j += 3;
                            continue;
                        }
                    }
                    let c = zc(j);
                    if c == cDelim {
                        break;
                    } else if c == b'\\' {
                        j += 1;
                        let c = zc(j);
                        if c == b'"'
                            || c == b'\\'
                            || c == b'/'
                            || c == b'b'
                            || c == b'f'
                            || c == b'n'
                            || c == b'r'
                            || c == b't'
                            || (c == b'u' && jsonIs4Hex(tail(z, j as usize + 1)))
                        {
                            if opcode == JSONB_TEXT {
                                opcode = JSONB_TEXTJ;
                            }
                        } else if c == b'\''
                            || c == b'v'
                            || c == b'\n'
                            || (c == b'0' && !sqlite3Isdigit(zc(j + 1)))
                            || (0xe2 == c
                                && 0x80 == zc(j + 1)
                                && (0xa8 == zc(j + 2) || 0xa9 == zc(j + 2)))
                            || (c == b'x' && jsonIs2Hex(tail(z, j as usize + 1)))
                        {
                            opcode = JSONB_TEXT5;
                            pParse.hasNonstd = 1;
                        } else if c == b'\r' {
                            if zc(j + 1) == b'\n' {
                                j += 1;
                            }
                            opcode = JSONB_TEXT5;
                            pParse.hasNonstd = 1;
                        } else {
                            pParse.iErr = j;
                            return -1;
                        }
                    } else if c <= 0x1f {
                        if c == 0 {
                            pParse.iErr = j;
                            return -1;
                        }
                        /* Control characters are not allowed in canonical JSON
                         ** string literals, but are allowed in JSON5 string
                         ** literals. */
                        opcode = JSONB_TEXT5;
                        pParse.hasNonstd = 1;
                    } else if c == b'"' {
                        opcode = JSONB_TEXT5;
                    }
                    j += 1;
                }
                jsonBlobAppendNode(
                    pParse,
                    opcode,
                    (j - 1 - i) as u64,
                    Some(&z[i as usize + 1..j as usize]),
                );
                return (j + 1) as i32;
            }
            b't' => {
                if tail(z, i as usize).starts_with(b"true") && !sqlite3Isalnum(zc(i + 4)) {
                    jsonBlobAppendOneByte(pParse, JSONB_TRUE);
                    return (i + 4) as i32;
                }
                pParse.iErr = i;
                return -1;
            }
            b'f' => {
                if tail(z, i as usize).starts_with(b"false") && !sqlite3Isalnum(zc(i + 5)) {
                    jsonBlobAppendOneByte(pParse, JSONB_FALSE);
                    return (i + 5) as i32;
                }
                pParse.iErr = i;
                return -1;
            }
            b'+' | b'.' | b'-' | b'0'..=b'9' => {
                return jsonTranslateNumber(pParse, z, i);
            }
            b'}' => {
                pParse.iErr = i;
                return -2; /* End of {...} */
            }
            b']' => {
                pParse.iErr = i;
                return -3; /* End of [...] */
            }
            b',' => {
                pParse.iErr = i;
                return -4; /* List separator */
            }
            b':' => {
                pParse.iErr = i;
                return -5; /* Object label/value separator */
            }
            0 => {
                return 0; /* End of file */
            }
            0x09 | 0x0a | 0x0d | 0x20 => {
                i += 1 + jsonSpan(tail(z, i as usize + 1));
                continue;
            }
            0x0b | 0x0c | b'/' | 0xc2 | 0xe1 | 0xe2 | 0xe3 | 0xef => {
                let j = json5Whitespace(tail(z, i as usize));
                if j > 0 {
                    i += j;
                    pParse.hasNonstd = 1;
                    continue;
                }
                pParse.iErr = i;
                return -1;
            }
            c => {
                if c == b'n'
                    && tail(z, i as usize).starts_with(b"null")
                    && !sqlite3Isalnum(zc(i + 4))
                {
                    jsonBlobAppendOneByte(pParse, JSONB_NULL);
                    return (i + 4) as i32;
                }
                /* fall-through into the default case that checks for NaN */
                for nm in aNanInfName.iter() {
                    if c != nm.c1 && c != nm.c2 {
                        continue;
                    }
                    let nn = nm.n as u32;
                    if sqlite3StrNICmp(Some(tail(z, i as usize)), Some(nm.zMatch), nn as i32) != 0 {
                        continue;
                    }
                    if sqlite3Isalnum(zc(i + nn)) {
                        continue;
                    }
                    if nm.eType == JSONB_FLOAT {
                        jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, Some(b"9e999"));
                    } else {
                        jsonBlobAppendOneByte(pParse, JSONB_NULL);
                    }
                    pParse.hasNonstd = 1;
                    return (i + nn) as i32;
                }
                pParse.iErr = i;
                return -1; /* Syntax error */
            }
        }
    }
}

/// The number-parsing part of jsonTranslateTextToBlob() (C labels
/// parse_number, parse_number_2 and parse_number_finish).
fn jsonTranslateNumber(pParse: &mut JsonParse, z: &[u8], i0: u32) -> i32 {
    #[derive(PartialEq)]
    enum St {
        Number,
        Number2,
        Finish,
    }
    let zc = |k: u32| at(z, k as usize);
    let mut i = i0;
    let mut t: u8; /* Bit 0x01:  JSON5.   Bit 0x02:  FLOAT */
    let mut seenE = false;
    let mut j: u32 = 0;
    let mut st;
    match zc(i) {
        b'+' => {
            pParse.hasNonstd = 1;
            t = 0x00;
            st = St::Number;
        }
        b'.' => {
            if sqlite3Isdigit(zc(i + 1)) {
                pParse.hasNonstd = 1;
                t = 0x03;
                st = St::Number2;
            } else {
                pParse.iErr = i;
                return -1;
            }
        }
        _ => {
            t = 0x00;
            st = St::Number;
        }
    }
    if st == St::Number {
        /* parse_number: */
        let c = zc(i);
        if c <= b'0' {
            if c == b'0' {
                if (zc(i + 1) == b'x' || zc(i + 1) == b'X') && sqlite3Isxdigit(zc(i + 2)) {
                    debug_assert!(t == 0x00);
                    pParse.hasNonstd = 1;
                    t = 0x01;
                    j = i + 3;
                    while sqlite3Isxdigit(zc(j)) {
                        j += 1;
                    }
                    st = St::Finish;
                } else if sqlite3Isdigit(zc(i + 1)) {
                    pParse.iErr = i + 1;
                    return -1;
                }
            } else {
                if !sqlite3Isdigit(zc(i + 1)) {
                    /* JSON5 allows for "+Infinity" and "-Infinity" using exactly
                     ** that case.  SQLite also allows these in any case and it
                     ** allows "+inf" and "-inf". */
                    if (zc(i + 1) == b'I' || zc(i + 1) == b'i')
                        && sqlite3StrNICmp(Some(tail(z, i as usize + 1)), Some(b"inf"), 3) == 0
                    {
                        pParse.hasNonstd = 1;
                        if zc(i) == b'-' {
                            jsonBlobAppendNode(pParse, JSONB_FLOAT, 6, Some(b"-9e999"));
                        } else {
                            jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, Some(b"9e999"));
                        }
                        let k = if sqlite3StrNICmp(Some(tail(z, i as usize + 4)), Some(b"inity"), 5)
                            == 0
                        {
                            9
                        } else {
                            4
                        };
                        return (i + k) as i32;
                    }
                    if zc(i + 1) == b'.' {
                        pParse.hasNonstd = 1;
                        t |= 0x01;
                        st = St::Number2;
                    } else {
                        pParse.iErr = i;
                        return -1;
                    }
                }
                if st == St::Number && zc(i + 1) == b'0' {
                    if sqlite3Isdigit(zc(i + 2)) {
                        pParse.iErr = i + 1;
                        return -1;
                    } else if (zc(i + 2) == b'x' || zc(i + 2) == b'X') && sqlite3Isxdigit(zc(i + 3))
                    {
                        pParse.hasNonstd = 1;
                        t |= 0x01;
                        j = i + 4;
                        while sqlite3Isxdigit(zc(j)) {
                            j += 1;
                        }
                        st = St::Finish;
                    }
                }
            }
        }
        if st == St::Number {
            st = St::Number2;
        }
    }
    if st == St::Number2 {
        /* parse_number_2: */
        j = i + 1;
        loop {
            let mut c = zc(j);
            if sqlite3Isdigit(c) {
                j += 1;
                continue;
            }
            if c == b'.' {
                if (t & 0x02) != 0 {
                    pParse.iErr = j;
                    return -1;
                }
                t |= 0x02;
                j += 1;
                continue;
            }
            if c == b'e' || c == b'E' {
                if zc(j - 1) < b'0' {
                    if zc(j - 1) == b'.'
                        && j.wrapping_sub(2) >= i
                        && sqlite3Isdigit(zc(j.wrapping_sub(2)))
                    {
                        pParse.hasNonstd = 1;
                        t |= 0x01;
                    } else {
                        pParse.iErr = j;
                        return -1;
                    }
                }
                if seenE {
                    pParse.iErr = j;
                    return -1;
                }
                t |= 0x02;
                seenE = true;
                c = zc(j + 1);
                if c == b'+' || c == b'-' {
                    j += 1;
                    c = zc(j + 1);
                }
                if !c.is_ascii_digit() {
                    pParse.iErr = j;
                    return -1;
                }
                j += 1;
                continue;
            }
            break;
        }
        if zc(j - 1) < b'0' {
            if zc(j - 1) == b'.' && j.wrapping_sub(2) >= i && sqlite3Isdigit(zc(j.wrapping_sub(2)))
            {
                pParse.hasNonstd = 1;
                t |= 0x01;
            } else {
                pParse.iErr = j;
                return -1;
            }
        }
    }
    /* parse_number_finish: */
    if zc(i) == b'+' {
        i += 1;
    }
    jsonBlobAppendNode(
        pParse,
        JSONB_INT + t,
        (j - i) as u64,
        Some(&z[i as usize..j as usize]),
    );
    j as i32
}

/// Parse a complete JSON string.  Return 0 on success or non-zero if there
/// are any errors.  If an error occurs, free all memory held by pParse,
/// but not pParse itself.
fn jsonConvertTextToBlob(pParse: &mut JsonParse, pCtx: Option<&mut Context>) -> i32 {
    let zJson = pParse.zJson.clone().unwrap_or_else(|| Rc::from(&b""[..]));
    let z: &[u8] = &zJson;
    let mut i = jsonTranslateTextToBlob(pParse, z, 0);
    if pParse.oom != 0 {
        i = -1;
    }
    if i > 0 {
        let mut k = i as usize;
        while jsonIsspace(at(z, k)) {
            k += 1;
        }
        if at(z, k) != 0 {
            k += json5Whitespace(tail(z, k)) as usize;
            if at(z, k) != 0 {
                if let Some(ctx) = pCtx {
                    sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
                }
                jsonParseReset(pParse);
                return 1;
            }
            pParse.hasNonstd = 1;
        }
    }
    if i <= 0 {
        if let Some(ctx) = pCtx {
            if pParse.oom != 0 {
                sqlite3_result_error_nomem(ctx);
            } else {
                sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
            }
        }
        jsonParseReset(pParse);
        return 1;
    }
    0
}

/// The input string pStr is a well-formed JSON text string.  Convert this
/// into the JSONB format and make it the return value of the SQL
/// function.
fn jsonReturnStringAsBlob(pStr: &mut JsonString, ctx: &mut Context) {
    debug_assert!(pStr.eErr == 0);
    let mut px = JsonParse {
        nJson: pStr.zBuf.len() as i32,
        ..Default::default()
    };
    jsonTranslateTextToBlob(&mut px, &pStr.zBuf, 0);
    if px.oom != 0 {
        sqlite3_result_error_nomem(ctx);
    } else {
        debug_assert!(px.nBlobAlloc > 0);
        let n = px.nBlob as usize;
        sqlite3_result_blob(ctx, &px.aBlob[..n], n as i32);
    }
}

/// jsonbPayloadSizeWide(): the payload size is a 2, 4, or 8 byte integer.
fn jsonbPayloadSizeWide(pParse: &JsonParse, i: u32, x: u8) -> (u32, u32) {
    let a = |k: u32| pParse.ab(k) as u32;
    let sz: u32;
    let n: u32;
    if x == 13 {
        if i + 2 >= pParse.nBlob {
            return (0, 0);
        }
        sz = (a(i + 1) << 8) + a(i + 2);
        n = 3;
    } else if x == 14 {
        if i + 4 >= pParse.nBlob {
            return (0, 0);
        }
        sz = (a(i + 1) << 24) + (a(i + 2) << 16) + (a(i + 3) << 8) + a(i + 4);
        n = 5;
    } else {
        if i + 8 >= pParse.nBlob || a(i + 1) != 0 || a(i + 2) != 0 || a(i + 3) != 0 || a(i + 4) != 0
        {
            return (0, 0);
        }
        sz = (a(i + 5) << 24) + (a(i + 6) << 16) + (a(i + 7) << 8) + a(i + 8);
        n = 9;
    }
    let lhs = i as i64 + sz as i64 + n as i64;
    if lhs > pParse.nBlob as i64 && lhs > pParse.nBlob.wrapping_sub(pParse.delta as u32) as i64 {
        return (0, 0);
    }
    (n, sz)
}

/// The byte at index i is a node type-code.  This routine determines the
/// payload size for that node and returns (offset from i to the beginning
/// of the payload, payload size).  The offset is 0 on error.
fn jsonbPayloadSize(pParse: &JsonParse, i: u32) -> (u32, u32) {
    if i >= pParse.nBlob {
        return (0, 0);
    }
    let x = pParse.ab(i) >> 4;
    let sz: u32;
    let n: u32;
    if x <= 11 {
        sz = x as u32;
        n = 1;
    } else if x == 12 {
        if i + 1 >= pParse.nBlob {
            return (0, 0);
        }
        sz = pParse.ab(i + 1) as u32;
        n = 2;
    } else {
        return jsonbPayloadSizeWide(pParse, i, x);
    }
    let lhs = i as i64 + sz as i64 + n as i64;
    if lhs > pParse.nBlob as i64 && lhs > pParse.nBlob.wrapping_sub(pParse.delta as u32) as i64 {
        return (0, 0);
    }
    (n, sz)
}

/// Translate the binary JSONB representation of JSON beginning at
/// pParse->aBlob[i] into a JSON text string.  Append the JSON text onto
/// the end of pOut.  Return the index in pParse->aBlob[] of the first byte
/// past the end of the element that is translated.
fn jsonTranslateBlobToText(
    pParse: &mut JsonParse,
    i: u32,
    pOut: &mut JsonString,
    ctx: &mut Context,
) -> u32 {
    let (n, sz) = jsonbPayloadSize(pParse, i);
    if n == 0 {
        pOut.eErr |= JSTRING_MALFORMED;
        return pParse.nBlob + 1;
    }
    let ip = (i + n) as usize;
    let mut malformed = false;
    match pParse.ab(i) & 0x0f {
        JSONB_NULL => {
            jsonAppendRawNZ(pOut, b"null");
            return i + 1;
        }
        JSONB_TRUE => {
            jsonAppendRawNZ(pOut, b"true");
            return i + 1;
        }
        JSONB_FALSE => {
            jsonAppendRawNZ(pOut, b"false");
            return i + 1;
        }
        JSONB_INT | JSONB_FLOAT => {
            if sz == 0 {
                malformed = true;
            } else {
                jsonAppendRaw(pOut, sub(&pParse.aBlob, ip, sz as usize));
            }
        }
        JSONB_INT5 => {
            /* Integer literal in hexadecimal notation */
            if sz == 0 {
                malformed = true;
            } else {
                let zIn = sub(&pParse.aBlob, ip, sz as usize);
                let mut k = 2u32;
                let mut u: u64 = 0;
                let mut bOverflow = false;
                if at(zIn, 0) == b'-' {
                    jsonAppendChar(pOut, b'-');
                    k += 1;
                } else if at(zIn, 0) == b'+' {
                    k += 1;
                }
                while k < sz {
                    let c = at(zIn, k as usize);
                    if !sqlite3Isxdigit(c) {
                        pOut.eErr |= JSTRING_MALFORMED;
                        break;
                    } else if (u >> 60) != 0 {
                        bOverflow = true;
                    } else {
                        u = u * 16 + sqlite3HexToInt(c as i32) as u64;
                    }
                    k += 1;
                }
                if bOverflow {
                    jsonPrintf(100, pOut, b"9.0e999", &[]);
                } else {
                    jsonPrintf(100, pOut, b"%llu", &[PfArg::from(u)]);
                }
            }
        }
        JSONB_FLOAT5 => {
            /* Float literal missing digits beside "." */
            if sz == 0 {
                malformed = true;
            } else {
                let zIn: Vec<u8> = sub(&pParse.aBlob, ip, sz as usize).to_vec();
                let zi = |k: u32| at(&zIn, k as usize);
                let mut k = 0u32;
                let mut bad = false;
                if zi(0) == b'-' {
                    jsonAppendChar(pOut, b'-');
                    if sz <= 1 {
                        bad = true;
                    }
                    k = 1;
                }
                if bad {
                    malformed = true;
                } else {
                    if zi(k) == b'.' {
                        jsonAppendChar(pOut, b'0');
                    }
                    while k < sz {
                        jsonAppendChar(pOut, zi(k));
                        if zi(k) == b'.' && (k + 1 == sz || !sqlite3Isdigit(zi(k + 1))) {
                            jsonAppendChar(pOut, b'0');
                        }
                        k += 1;
                    }
                }
            }
        }
        JSONB_TEXT | JSONB_TEXTJ => {
            if pOut.zBuf.len() as u64 + sz as u64 + 2 <= pOut.nAlloc
                || jsonStringGrow(pOut, sz + 2) == 0
            {
                pOut.zBuf.push(b'"');
                pOut.zBuf
                    .extend_from_slice(sub(&pParse.aBlob, ip, sz as usize));
                pOut.zBuf.push(b'"');
            }
        }
        JSONB_TEXT5 => {
            let zAll: Vec<u8> = sub(&pParse.aBlob, ip, sz as usize).to_vec();
            let mut zo = 0usize; /* offset of zIn in zAll */
            let zi = |o: usize| at(&zAll, o);
            let mut sz2 = sz;
            jsonAppendChar(pOut, b'"');
            while sz2 > 0 {
                let mut k = 0u32;
                while k < sz2 && (ok(zi(zo + k as usize)) || zi(zo + k as usize) == b'\'') {
                    k += 1;
                }
                if k > 0 {
                    jsonAppendRawNZ(pOut, &zAll[zo..zo + k as usize]);
                    if k >= sz2 {
                        break;
                    }
                    zo += k as usize;
                    sz2 -= k;
                }
                if zi(zo) == b'"' {
                    jsonAppendRawNZ(pOut, b"\\\"");
                    zo += 1;
                    sz2 -= 1;
                    continue;
                }
                if zi(zo) <= 0x1f {
                    if pOut.zBuf.len() as u64 + 7 > pOut.nAlloc && jsonStringGrow(pOut, 7) != 0 {
                        break;
                    }
                    jsonAppendControlChar(pOut, zi(zo));
                    zo += 1;
                    sz2 -= 1;
                    continue;
                }
                debug_assert!(zi(zo) == b'\\');
                debug_assert!(sz2 >= 1);
                if sz2 < 2 {
                    pOut.eErr |= JSTRING_MALFORMED;
                    break;
                }
                match zi(zo + 1) {
                    b'\'' => {
                        jsonAppendChar(pOut, b'\'');
                    }
                    b'v' => {
                        jsonAppendRawNZ(pOut, b"\\u000b");
                    }
                    b'x' => {
                        if sz2 < 4 {
                            pOut.eErr |= JSTRING_MALFORMED;
                            sz2 = 2;
                        } else {
                            jsonAppendRawNZ(pOut, b"\\u00");
                            jsonAppendRawNZ(pOut, &zAll[zo + 2..zo + 4]);
                            zo += 2;
                            sz2 -= 2;
                        }
                    }
                    b'0' => {
                        jsonAppendRawNZ(pOut, b"\\u0000");
                    }
                    b'\r' => {
                        if sz2 > 2 && zi(zo + 2) == b'\n' {
                            zo += 1;
                            sz2 -= 1;
                        }
                    }
                    b'\n' => {}
                    0xe2 => {
                        /* '\' followed by either U+2028 or U+2029 is ignored as
                         ** whitespace. */
                        if sz2 < 4
                            || 0x80 != zi(zo + 2)
                            || (0xa8 != zi(zo + 3) && 0xa9 != zi(zo + 3))
                        {
                            pOut.eErr |= JSTRING_MALFORMED;
                            sz2 = 2;
                        } else {
                            zo += 2;
                            sz2 -= 2;
                        }
                    }
                    _ => {
                        jsonAppendRawNZ(pOut, &zAll[zo..zo + 2]);
                    }
                }
                debug_assert!(sz2 >= 2);
                zo += 2;
                sz2 -= 2;
            }
            jsonAppendChar(pOut, b'"');
        }
        JSONB_TEXTRAW => {
            let z = sub(&pParse.aBlob, ip, sz as usize).to_vec();
            jsonAppendString(pOut, &z);
        }
        JSONB_ARRAY => {
            jsonAppendChar(pOut, b'[');
            let mut j = i + n;
            let iEnd = j + sz;
            pParse.iDepth = pParse.iDepth.wrapping_add(1);
            if pParse.iDepth > JSON_MAX_DEPTH {
                jsonStringTooDeep(pOut, ctx);
            }
            while j < iEnd && pOut.eErr == 0 {
                j = jsonTranslateBlobToText(pParse, j, pOut, ctx);
                jsonAppendChar(pOut, b',');
            }
            pParse.iDepth = pParse.iDepth.wrapping_sub(1);
            if j > iEnd {
                pOut.eErr |= JSTRING_MALFORMED;
            }
            if sz > 0 {
                jsonStringTrimOneChar(pOut);
            }
            jsonAppendChar(pOut, b']');
        }
        JSONB_OBJECT => {
            let mut x = 0u32;
            jsonAppendChar(pOut, b'{');
            let mut j = i + n;
            let iEnd = j + sz;
            pParse.iDepth = pParse.iDepth.wrapping_add(1);
            if pParse.iDepth > JSON_MAX_DEPTH {
                jsonStringTooDeep(pOut, ctx);
            }
            while j < iEnd && pOut.eErr == 0 {
                j = jsonTranslateBlobToText(pParse, j, pOut, ctx);
                let c = if (x & 1) != 0 { b',' } else { b':' };
                x += 1;
                jsonAppendChar(pOut, c);
            }
            pParse.iDepth = pParse.iDepth.wrapping_sub(1);
            if (x & 1) != 0 || j > iEnd {
                pOut.eErr |= JSTRING_MALFORMED;
            }
            if sz > 0 {
                jsonStringTrimOneChar(pOut);
            }
            jsonAppendChar(pOut, b'}');
        }
        _ => {
            malformed = true;
        }
    }
    if malformed {
        pOut.eErr |= JSTRING_MALFORMED;
    }
    i + n + sz
}

/// Context for recursion of json_pretty()
struct JsonPretty<'a> {
    pParse: &'a mut JsonParse,
    pOut: &'a mut JsonString,
    zIndent: &'a [u8],
    nIndent: u32,
}

/// Append indentation to the pretty JSON under construction
fn jsonPrettyIndent(pPretty: &mut JsonPretty) {
    for _ in 0..pPretty.nIndent {
        jsonAppendRaw(pPretty.pOut, pPretty.zIndent);
    }
}

/// Variant of jsonTranslateBlobToText() that "pretty-prints" the output.
fn jsonTranslateBlobToPrettyText(pPretty: &mut JsonPretty, i0: u32, ctx: &mut Context) -> u32 {
    let mut i = i0;
    let (n, sz) = jsonbPayloadSize(pPretty.pParse, i);
    if n == 0 {
        pPretty.pOut.eErr |= JSTRING_MALFORMED;
        return pPretty.pParse.nBlob + 1;
    }
    match pPretty.pParse.ab(i) & 0x0f {
        JSONB_ARRAY => {
            let mut j = i + n;
            let iEnd = j + sz;
            jsonAppendChar(pPretty.pOut, b'[');
            if j < iEnd {
                jsonAppendChar(pPretty.pOut, b'\n');
                pPretty.nIndent += 1;
                if pPretty.nIndent >= JSON_MAX_DEPTH as u32 {
                    jsonStringTooDeep(pPretty.pOut, ctx);
                }
                while pPretty.pOut.eErr == 0 {
                    jsonPrettyIndent(pPretty);
                    j = jsonTranslateBlobToPrettyText(pPretty, j, ctx);
                    if j >= iEnd {
                        if j > iEnd {
                            pPretty.pOut.eErr |= JSTRING_MALFORMED;
                        }
                        break;
                    }
                    jsonAppendRawNZ(pPretty.pOut, b",\n");
                }
                jsonAppendChar(pPretty.pOut, b'\n');
                pPretty.nIndent -= 1;
                jsonPrettyIndent(pPretty);
            }
            jsonAppendChar(pPretty.pOut, b']');
            i = iEnd;
        }
        JSONB_OBJECT => {
            let mut j = i + n;
            let iEnd = j + sz;
            jsonAppendChar(pPretty.pOut, b'{');
            if j < iEnd {
                jsonAppendChar(pPretty.pOut, b'\n');
                pPretty.nIndent += 1;
                if pPretty.nIndent >= JSON_MAX_DEPTH as u32 {
                    jsonStringTooDeep(pPretty.pOut, ctx);
                }
                pPretty.pParse.iDepth = pPretty.nIndent as u16;
                while pPretty.pOut.eErr == 0 {
                    jsonPrettyIndent(pPretty);
                    j = jsonTranslateBlobToText(pPretty.pParse, j, pPretty.pOut, ctx);
                    if j > iEnd {
                        pPretty.pOut.eErr |= JSTRING_MALFORMED;
                        break;
                    }
                    jsonAppendRawNZ(pPretty.pOut, b": ");
                    j = jsonTranslateBlobToPrettyText(pPretty, j, ctx);
                    if j >= iEnd {
                        break;
                    }
                    jsonAppendRawNZ(pPretty.pOut, b",\n");
                }
                jsonAppendChar(pPretty.pOut, b'\n');
                pPretty.nIndent -= 1;
                jsonPrettyIndent(pPretty);
            }
            jsonAppendChar(pPretty.pOut, b'}');
            i = iEnd;
        }
        _ => {
            i = jsonTranslateBlobToText(pPretty.pParse, i, pPretty.pOut, ctx);
        }
    }
    i
}

/// Given that a JSONB_ARRAY object starts at offset i, return the number
/// of entries in that array.
fn jsonbArrayCount(pParse: &JsonParse, iRoot: u32) -> u32 {
    let mut k = 0u32;
    let (mut n, mut sz) = jsonbPayloadSize(pParse, iRoot);
    let iEnd = iRoot + n + sz;
    let mut i = iRoot + n;
    while n > 0 && i < iEnd {
        (n, sz) = jsonbPayloadSize(pParse, i);
        i += sz + n;
        k += 1;
    }
    k
}

/// Edit the payload size of the element at iRoot by the amount in
/// pParse->delta.
fn jsonAfterEditSizeAdjust(pParse: &mut JsonParse, iRoot: u32) {
    debug_assert!(pParse.delta != 0);
    debug_assert!(pParse.nBlobAlloc >= pParse.nBlob);
    let nBlob = pParse.nBlob;
    pParse.nBlob = pParse.nBlobAlloc;
    let (_, mut sz) = jsonbPayloadSize(pParse, iRoot);
    pParse.nBlob = nBlob;
    sz = sz.wrapping_add(pParse.delta as u32);
    let d = jsonBlobChangePayloadSize(pParse, iRoot, sz);
    pParse.delta += d;
}

/// If the JSONB at aIns[0..nIns-1] can be expanded (by denormalizing the
/// size field) by d bytes, then write the expansion into aOut[] and return
/// true.
fn jsonBlobOverwrite(aOut: &mut [u8], aIns: &[u8], nIns: u32, d: u32) -> bool {
    /* Lookup table for finding the upper 4 bits of the first byte of the
     ** expanded aIns[], based on the size of the expanded aIns[] header:
     **
     **                             2     3  4     5  6  7  8     9 */
    static aType: [u8; 8] = [0xc0, 0xd0, 0, 0xe0, 0, 0, 0, 0xf0];
    if (aIns[0] & 0x0f) <= 2 {
        return false; /* Cannot enlarge NULL, true, false */
    }
    let mut i: u32;
    let szHdr: u32;
    match aIns[0] >> 4 {
        12 => {
            /* aIns[] header size is 2 */
            if ((1u32 << d) & 0x8a) == 0 {
                return false; /* d must be 1, 3, or 7 */
            }
            i = d + 2;
            szHdr = 2;
        }
        13 => {
            /* aIns[] header size is 3 */
            if d != 2 && d != 6 {
                return false;
            }
            i = d + 3;
            szHdr = 3;
        }
        14 => {
            /* aIns[] header size is 5 */
            if d != 4 {
                return false;
            }
            i = 9;
            szHdr = 5;
        }
        15 => {
            /* aIns[] header size is 9 */
            return false;
        }
        _ => {
            /* aIns[] header size 1 */
            if ((1u32 << d) & 0x116) == 0 {
                return false; /* d must be 1, 2, 4, or 8 */
            }
            i = d + 1;
            szHdr = 1;
        }
    }
    debug_assert!((2..=9).contains(&i) && aType[(i - 2) as usize] != 0);
    aOut[0] = (aIns[0] & 0x0f) | aType[(i - 2) as usize];
    let np = (nIns - szHdr) as usize;
    aOut[i as usize..i as usize + np].copy_from_slice(&aIns[szHdr as usize..szHdr as usize + np]);
    let mut szPayload = nIns - szHdr;
    loop {
        i -= 1;
        aOut[i as usize] = (szPayload & 0xff) as u8;
        if i == 1 {
            break;
        }
        szPayload >>= 8;
    }
    debug_assert!((szPayload >> 8) == 0);
    true
}

/// Modify the JSONB blob at pParse->aBlob by removing nDel bytes of
/// content beginning at iDel, and replacing them with nIns bytes of
/// content given by aIns.
///
/// aIns may be None, in which case space is created to hold nIns bytes
/// beginning at iDel, but that space is uninitialized.
fn jsonBlobEdit(pParse: &mut JsonParse, iDel: u32, nDel: u32, aIns: Option<&[u8]>, nIns: u32) {
    let d: i64 = nIns as i64 - nDel as i64;
    debug_assert!(pParse.nBlob as u64 >= iDel as u64 + nDel as u64);
    if d < 0 && d >= -8 {
        if let Some(ins) = aIns {
            if jsonBlobOverwrite(&mut pParse.aBlob[iDel as usize..], ins, nIns, (-d) as u32) {
                return;
            }
        }
    }
    if d != 0 {
        if pParse.nBlob as i64 + d > pParse.nBlobAlloc as i64 {
            jsonBlobExpand(pParse, (pParse.nBlob as i64 + d) as u64);
            if pParse.oom != 0 {
                return;
            }
        }
        let nb = pParse.nBlob as usize;
        pParse
            .aBlob
            .copy_within((iDel + nDel) as usize..nb, (iDel + nIns) as usize);
        pParse.nBlob = (pParse.nBlob as i64 + d) as u32;
        pParse.delta += d as i32;
    }
    if nIns > 0 {
        if let Some(ins) = aIns {
            pParse.aBlob[iDel as usize..(iDel + nIns) as usize]
                .copy_from_slice(&ins[..nIns as usize]);
        }
    }
}

/// Return the number of escaped newlines to be ignored.
fn jsonBytesToBypass(z: &[u8], n: u32) -> u32 {
    let mut i = 0u32;
    let zc = |k: u32| at(z, k as usize);
    while i + 1 < n {
        if zc(i) != b'\\' {
            return i;
        }
        if zc(i + 1) == b'\n' {
            i += 2;
            continue;
        }
        if zc(i + 1) == b'\r' {
            if i + 2 < n && zc(i + 2) == b'\n' {
                i += 3;
            } else {
                i += 2;
            }
            continue;
        }
        if 0xe2 == zc(i + 1)
            && i + 3 < n
            && 0x80 == zc(i + 2)
            && (0xa8 == zc(i + 3) || 0xa9 == zc(i + 3))
        {
            i += 4;
            continue;
        }
        break;
    }
    i
}

/// Input z[0..n] defines JSON escape sequence including the leading '\\'.
/// Decode that escape sequence into a single character.  Write that
/// character into *piOut.  Return the number of bytes in the escape
/// sequence.
fn jsonUnescapeOneChar(z: &[u8], n: u32, piOut: &mut u32) -> u32 {
    debug_assert!(n > 0);
    debug_assert!(at(z, 0) == b'\\');
    if n < 2 {
        *piOut = JSON_INVALID_CHAR;
        return n;
    }
    match at(z, 1) {
        b'u' => {
            if n < 6 {
                *piOut = JSON_INVALID_CHAR;
                return n;
            }
            let v = jsonHexToInt4(tail(z, 2));
            if (v & 0xfc00) == 0xd800 && n >= 12 && at(z, 6) == b'\\' && at(z, 7) == b'u' {
                let vlo = jsonHexToInt4(tail(z, 8));
                if (vlo & 0xfc00) == 0xdc00 {
                    *piOut = ((v & 0x3ff) << 10) + (vlo & 0x3ff) + 0x10000;
                    return 12;
                }
            }
            *piOut = v;
            6
        }
        b'b' => {
            *piOut = 0x08;
            2
        }
        b'f' => {
            *piOut = 0x0c;
            2
        }
        b'n' => {
            *piOut = b'\n' as u32;
            2
        }
        b'r' => {
            *piOut = b'\r' as u32;
            2
        }
        b't' => {
            *piOut = b'\t' as u32;
            2
        }
        b'v' => {
            *piOut = 0x0b;
            2
        }
        b'0' => {
            /* JSON5 requires that the \0 escape not be followed by a digit. */
            *piOut = if n > 2 && sqlite3Isdigit(at(z, 2)) {
                JSON_INVALID_CHAR
            } else {
                0
            };
            2
        }
        b'\'' | b'"' | b'/' | b'\\' => {
            *piOut = at(z, 1) as u32;
            2
        }
        b'x' => {
            if n < 4 {
                *piOut = JSON_INVALID_CHAR;
                return n;
            }
            *piOut = ((jsonHexToInt(at(z, 2)) as u32) << 4) | jsonHexToInt(at(z, 3)) as u32;
            4
        }
        0xe2 | b'\r' | b'\n' => {
            let nSkip = jsonBytesToBypass(z, n);
            if nSkip == 0 {
                *piOut = JSON_INVALID_CHAR;
                n
            } else if nSkip == n {
                *piOut = 0;
                n
            } else if at(z, nSkip as usize) == b'\\' {
                nSkip + jsonUnescapeOneChar(tail(z, nSkip as usize), n - nSkip, piOut)
            } else {
                let sz = sqlite3Utf8ReadLimited(tail(z, nSkip as usize), (n - nSkip) as i32, piOut);
                nSkip + sz as u32
            }
        }
        _ => {
            *piOut = JSON_INVALID_CHAR;
            2
        }
    }
}

/// Read one character of a label for jsonLabelCompareEscaped().
fn jsonLabelChar(z: &[u8], off: &mut usize, n: &mut u32, raw: bool) -> u32 {
    let mut c: u32;
    if *n == 0 {
        c = 0;
    } else if raw || at(z, *off) != b'\\' {
        c = at(z, *off) as u32;
        if c >= 0xc0 {
            let sz = sqlite3Utf8ReadLimited(tail(z, *off), *n as i32, &mut c);
            *off += sz as usize;
            *n -= sz as u32;
        } else {
            *off += 1;
            *n -= 1;
        }
    } else {
        c = 0;
        let k = jsonUnescapeOneChar(tail(z, *off), *n, &mut c);
        *off += k as usize;
        debug_assert!(k <= *n);
        *n -= k;
    }
    c
}

/// Compare two object labels.  Return 1 if they are equal and 0 if they
/// differ.  In this version, one or the other or both of the two
/// comparands contains an escape sequence.
fn jsonLabelCompareEscaped(
    zLeft: &[u8],
    nLeft: u32,
    rawLeft: bool,
    zRight: &[u8],
    nRight: u32,
    rawRight: bool,
) -> bool {
    debug_assert!(!rawLeft || !rawRight);
    let (mut oL, mut nL, mut oR, mut nR) = (0usize, nLeft, 0usize, nRight);
    loop {
        let cLeft = jsonLabelChar(zLeft, &mut oL, &mut nL, rawLeft);
        let cRight = jsonLabelChar(zRight, &mut oR, &mut nR, rawRight);
        if cLeft != cRight {
            return false;
        }
        if cLeft == 0 {
            return true;
        }
    }
}

/// Compare two object labels.  Return true if they are equal.
fn jsonLabelCompare(
    zLeft: &[u8],
    nLeft: u32,
    rawLeft: bool,
    zRight: &[u8],
    nRight: u32,
    rawRight: bool,
) -> bool {
    if rawLeft && rawRight {
        /* Simpliest case:  Neither label contains escapes.  A simple
         ** memcmp() is sufficient. */
        if nLeft != nRight {
            return false;
        }
        sub(zLeft, 0, nLeft as usize) == sub(zRight, 0, nRight as usize)
    } else {
        jsonLabelCompareEscaped(zLeft, nLeft, rawLeft, zRight, nRight, rawRight)
    }
}

/* Error returns from jsonLookupStep() */
const JSON_LOOKUP_ERROR: u32 = 0xffffffff;
const JSON_LOOKUP_NOTFOUND: u32 = 0xfffffffe;
const JSON_LOOKUP_NOTARRAY: u32 = 0xfffffffd;
const JSON_LOOKUP_TOODEEP: u32 = 0xfffffffc;
const JSON_LOOKUP_PATHERROR: u32 = 0xfffffffb;
#[inline]
fn JSON_LOOKUP_ISERROR(x: u32) -> bool {
    x >= JSON_LOOKUP_PATHERROR
}

/// This helper routine for jsonLookupStep() populates pIns with binary
/// data that is to be inserted into pParse.  zPath[ip..] is the tail of
/// the path that determines substructure.
fn jsonCreateEditSubstructure(
    pParse: &mut JsonParse,
    pIns: &mut JsonParse,
    zPath: &[u8],
    ip: usize,
) -> u32 {
    *pIns = JsonParse::default();
    let rc;
    if at(zPath, ip) == 0 {
        /* No substructure.  Just insert what is given in pParse. */
        pIns.aBlob = pParse.aIns.clone();
        pIns.nBlob = pParse.nIns;
        rc = 0;
    } else {
        /* Construct the binary substructure */
        pIns.nBlob = 1;
        pIns.aBlob = vec![if at(zPath, ip) == b'.' {
            JSONB_OBJECT
        } else {
            JSONB_ARRAY
        }];
        pIns.eEdit = pParse.eEdit;
        pIns.nIns = pParse.nIns;
        pIns.aIns = pParse.aIns.clone();
        pIns.iDepth = pParse.iDepth.wrapping_add(1);
        if pIns.iDepth >= JSON_MAX_DEPTH {
            return JSON_LOOKUP_TOODEEP;
        }
        rc = jsonLookupStep(pIns, 0, zPath, ip, 0);
        pParse.iDepth = pParse.iDepth.wrapping_sub(1);
        pParse.oom |= pIns.oom;
    }
    rc /* Error code only */
}

/// Search along zPath (zPath[ip..], the part of the path that follows the
/// initial '$') to find the Json element specified.  Return an index into
/// pParse->aBlob[] for the start of that element's value.
///
/// If the value found by this routine is the value half of label/value
/// pair within an object, then set pPath->iLabel to the start of the
/// corresponding label, before returning.
///
/// Return one of the JSON_LOOKUP error codes if problems are seen.
///
/// This routine will also modify the blob.  If pParse->eEdit is one of
/// JEDIT_DEL, JEDIT_REPL, JEDIT_INS, JEDIT_SET, or JEDIT_AINS, then changes
/// might be made to the selected value.
fn jsonLookupStep(
    pParse: &mut JsonParse,
    iRoot0: u32,
    zPath: &[u8],
    ip: usize,
    iLabel: u32,
) -> u32 {
    let zp = |k: usize| at(zPath, ip + k);
    let mut iRoot = iRoot0;
    if zp(0) == 0 {
        if pParse.eEdit != 0 && jsonBlobMakeEditable(pParse, pParse.nIns) {
            let (n, mut sz) = jsonbPayloadSize(pParse, iRoot);
            sz += n;
            if pParse.eEdit == JEDIT_DEL {
                if iLabel > 0 {
                    sz += iRoot - iLabel;
                    iRoot = iLabel;
                }
                jsonBlobEdit(pParse, iRoot, sz, None, 0);
            } else if pParse.eEdit == JEDIT_INS {
                /* Already exists, so json_insert() is a no-op */
            } else if pParse.eEdit == JEDIT_AINS {
                /* json_array_insert() */
                if at(zPath, ip.wrapping_sub(1)) != b']' {
                    return JSON_LOOKUP_NOTARRAY;
                } else {
                    let ins = std::mem::take(&mut pParse.aIns);
                    let nIns = pParse.nIns;
                    jsonBlobEdit(pParse, iRoot, 0, Some(&ins), nIns);
                    pParse.aIns = ins;
                }
            } else {
                /* json_set() or json_replace() */
                let ins = std::mem::take(&mut pParse.aIns);
                let nIns = pParse.nIns;
                jsonBlobEdit(pParse, iRoot, sz, Some(&ins), nIns);
                pParse.aIns = ins;
            }
        }
        pParse.iLabel = iLabel;
        return iRoot;
    }
    if zp(0) == b'.' {
        let mut rawKey = true;
        let x = pParse.ab(iRoot);
        let ip = ip + 1;
        let zp = |k: usize| at(zPath, ip + k);
        let zKey: usize; /* offset of the key in zPath */
        let nKey: u32;
        let mut i: usize;
        if zp(0) == b'"' {
            zKey = ip + 1;
            i = 1;
            while zp(i) != 0 && zp(i) != b'"' {
                if zp(i) == b'\\' && zp(i + 1) != 0 {
                    i += 1;
                }
                i += 1;
            }
            nKey = (i - 1) as u32;
            if zp(i) != 0 {
                i += 1;
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
            rawKey = !sub(zPath, zKey, nKey as usize).contains(&b'\\');
        } else {
            zKey = ip;
            i = 0;
            while zp(i) != 0 && zp(i) != b'.' && zp(i) != b'[' {
                i += 1;
            }
            nKey = i as u32;
            if nKey == 0 {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        let key: Vec<u8> = sub(zPath, zKey, nKey as usize).to_vec();
        if (x & 0x0f) != JSONB_OBJECT {
            return JSON_LOOKUP_NOTFOUND;
        }
        let (n, sz) = jsonbPayloadSize(pParse, iRoot);
        let mut j = iRoot + n; /* j is the index of a label */
        let iEnd = j + sz;
        while j < iEnd {
            let x = pParse.ab(j) & 0x0f;
            if !(JSONB_TEXT..=JSONB_TEXTRAW).contains(&x) {
                return JSON_LOOKUP_ERROR;
            }
            let (n, sz) = jsonbPayloadSize(pParse, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            let k = j + n; /* k is the index of the label text */
            if k + sz >= iEnd {
                return JSON_LOOKUP_ERROR;
            }
            let rawLabel = x == JSONB_TEXT || x == JSONB_TEXTRAW;
            if jsonLabelCompare(
                &key,
                nKey,
                rawKey,
                tail(&pParse.aBlob, k as usize),
                sz,
                rawLabel,
            ) {
                let v = k + sz; /* v is the index of the value */
                if (pParse.ab(v) & 0x0f) > JSONB_OBJECT {
                    return JSON_LOOKUP_ERROR;
                }
                let (n, sz) = jsonbPayloadSize(pParse, v);
                if n == 0 || v + n + sz > iEnd {
                    return JSON_LOOKUP_ERROR;
                }
                debug_assert!(j > 0);
                pParse.iDepth = pParse.iDepth.wrapping_add(1);
                if pParse.iDepth >= JSON_MAX_DEPTH {
                    return JSON_LOOKUP_TOODEEP;
                }
                let rc = jsonLookupStep(pParse, v, zPath, ip + i, j);
                pParse.iDepth = pParse.iDepth.wrapping_sub(1);
                if pParse.delta != 0 {
                    jsonAfterEditSizeAdjust(pParse, iRoot);
                }
                return rc;
            }
            j = k + sz;
            if (pParse.ab(j) & 0x0f) > JSONB_OBJECT {
                return JSON_LOOKUP_ERROR;
            }
            let (n, sz) = jsonbPayloadSize(pParse, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j += n + sz;
        }
        if j > iEnd {
            return JSON_LOOKUP_ERROR;
        }
        if pParse.eEdit >= JEDIT_INS {
            if pParse.eEdit == JEDIT_AINS
                && crate::func::sqlite3_strglob(Some(b"*]"), Some(cstr(tail(zPath, ip + i)))) != 0
            {
                return JSON_LOOKUP_NOTARRAY;
            }
            let mut ix = JsonParse::default(); /* Header of the label to be inserted */
            jsonBlobAppendNode(
                &mut ix,
                if rawKey { JSONB_TEXTRAW } else { JSONB_TEXT5 },
                nKey as u64,
                None,
            );
            pParse.oom |= ix.oom;
            let mut v = JsonParse::default(); /* BLOB encoding of the value to be inserted */
            let rc = jsonCreateEditSubstructure(pParse, &mut v, zPath, ip + i);
            if !JSON_LOOKUP_ISERROR(rc) && jsonBlobMakeEditable(pParse, ix.nBlob + nKey + v.nBlob) {
                debug_assert!(pParse.oom == 0);
                let nIns = ix.nBlob + nKey + v.nBlob;
                jsonBlobEdit(pParse, j, 0, None, nIns);
                if pParse.oom == 0 {
                    let mut k = j as usize;
                    let nix = ix.nBlob as usize;
                    pParse.aBlob[k..k + nix].copy_from_slice(&ix.aBlob[..nix]);
                    k += nix;
                    pParse.aBlob[k..k + nKey as usize].copy_from_slice(&key);
                    k += nKey as usize;
                    let nv = v.nBlob as usize;
                    pParse.aBlob[k..k + nv].copy_from_slice(sub(&v.aBlob, 0, nv));
                    if pParse.delta != 0 {
                        jsonAfterEditSizeAdjust(pParse, iRoot);
                    }
                }
            }
            return rc;
        }
    } else if zp(0) == b'[' {
        let mut kk: u64 = 0;
        let x = pParse.ab(iRoot) & 0x0f;
        if x != JSONB_ARRAY {
            return JSON_LOOKUP_NOTFOUND;
        }
        let (n, sz) = jsonbPayloadSize(pParse, iRoot);
        let mut i = 1usize;
        while sqlite3Isdigit(zp(i)) {
            if kk < 0xffffffff {
                kk = kk * 10 + (zp(i) - b'0') as u64;
            }
            i += 1;
        }
        if i < 2 || zp(i) != b']' {
            if zp(1) == b'#' {
                kk = jsonbArrayCount(pParse, iRoot) as u64;
                i = 2;
                if zp(2) == b'-' && sqlite3Isdigit(zp(3)) {
                    let mut nn: u64 = 0;
                    i = 3;
                    loop {
                        if nn < 0xffffffff {
                            nn = nn * 10 + (zp(i) - b'0') as u64;
                        }
                        i += 1;
                        if !sqlite3Isdigit(zp(i)) {
                            break;
                        }
                    }
                    if nn > kk {
                        return JSON_LOOKUP_NOTFOUND;
                    }
                    kk -= nn;
                }
                if zp(i) != b']' {
                    return JSON_LOOKUP_PATHERROR;
                }
            } else {
                return JSON_LOOKUP_PATHERROR;
            }
        }
        let mut j = iRoot + n;
        let iEnd = j + sz;
        while j < iEnd {
            if kk == 0 {
                pParse.iDepth = pParse.iDepth.wrapping_add(1);
                if pParse.iDepth >= JSON_MAX_DEPTH {
                    return JSON_LOOKUP_TOODEEP;
                }
                let rc = jsonLookupStep(pParse, j, zPath, ip + i + 1, 0);
                pParse.iDepth = pParse.iDepth.wrapping_sub(1);
                if pParse.delta != 0 {
                    jsonAfterEditSizeAdjust(pParse, iRoot);
                }
                return rc;
            }
            kk -= 1;
            let (n, sz) = jsonbPayloadSize(pParse, j);
            if n == 0 {
                return JSON_LOOKUP_ERROR;
            }
            j += n + sz;
        }
        if j > iEnd {
            return JSON_LOOKUP_ERROR;
        }
        if kk > 0 {
            return JSON_LOOKUP_NOTFOUND;
        }
        if pParse.eEdit >= JEDIT_INS {
            let mut v = JsonParse::default();
            let rc = jsonCreateEditSubstructure(pParse, &mut v, zPath, ip + i + 1);
            if !JSON_LOOKUP_ISERROR(rc) && jsonBlobMakeEditable(pParse, v.nBlob) {
                debug_assert!(pParse.oom == 0);
                let nv = v.nBlob;
                let ins = sub(&v.aBlob, 0, nv as usize).to_vec();
                jsonBlobEdit(pParse, j, 0, Some(&ins), nv);
            }
            if pParse.delta != 0 {
                jsonAfterEditSizeAdjust(pParse, iRoot);
            }
            return rc;
        }
    } else {
        return JSON_LOOKUP_PATHERROR;
    }
    JSON_LOOKUP_NOTFOUND
}

/// Convert a JSON BLOB into text and make that text the return value of an
/// SQL function.
fn jsonReturnTextJsonFromBlob(ctx: &mut Context, aBlob: &[u8]) {
    let mut x = JsonParse {
        aBlob: aBlob.to_vec(),
        nBlob: aBlob.len() as u32,
        ..Default::default()
    };
    let mut s = jsonStringNew();
    jsonTranslateBlobToText(&mut x, 0, &mut s, ctx);
    jsonReturnString(&mut s, None, ctx);
}

/// Return the value of the BLOB node at index i.
///
/// If the value is a primitive, return it as an SQL value.  If the value
/// is an array or object, return it as either JSON text or the BLOB
/// encoding, depending on the eMode flag as follows:
///
///     eMode==0     JSONB if the JSON_B flag is set in userdata or
///                  text if the JSON_B flag is omitted from userdata.
///
///     eMode==1     Text
///
///     eMode==2     JSONB
fn jsonReturnFromBlob(pParse: &mut JsonParse, i: u32, pCtx: &mut Context, eMode0: i32) {
    let mut eMode = eMode0;
    debug_assert!((0..=2).contains(&eMode));
    let (mut n, mut sz) = jsonbPayloadSize(pParse, i);
    if n == 0 {
        sqlite3_result_error(pCtx, Some(b"malformed JSON"), -1);
        return;
    }
    let malformed = |pCtx: &mut Context| {
        sqlite3_result_error(pCtx, Some(b"malformed JSON"), -1);
    };
    let x = pParse.ab(i) & 0x0f;
    let mut to_double = false;
    match x {
        JSONB_NULL => {
            if sz != 0 {
                return malformed(pCtx);
            }
            sqlite3_result_null(pCtx);
        }
        JSONB_TRUE => {
            if sz != 0 {
                return malformed(pCtx);
            }
            sqlite3_result_int(pCtx, 1);
        }
        JSONB_FALSE => {
            if sz != 0 {
                return malformed(pCtx);
            }
            sqlite3_result_int(pCtx, 0);
        }
        JSONB_INT5 | JSONB_INT => {
            let mut iRes: i64 = 0;
            let mut bNeg = false;
            if sz == 0 {
                return malformed(pCtx);
            }
            let c = pParse.ab(i + n);
            if c == b'-' {
                if sz < 2 {
                    return malformed(pCtx);
                }
                n += 1;
                sz -= 1;
                bNeg = true;
            }
            let z = sub(&pParse.aBlob, (i + n) as usize, sz as usize).to_vec();
            let rc = sqlite3DecOrHexToI64(Some(cstr(&z)), &mut iRes);
            if rc == 0 {
                if iRes < 0 {
                    /* A hexadecimal literal with 16 significant digits and with
                     ** the high-order bit set is a negative integer in SQLite
                     ** (and hence iRes comes back as negative) but should be
                     ** interpreted as a positive value if it occurs within
                     ** JSON.  The value is too large to appear as an SQLite
                     ** integer so it must be converted into floating point. */
                    let r = (iRes as u64) as f64;
                    sqlite3_result_double(pCtx, if bNeg { -r } else { r });
                } else {
                    sqlite3_result_int64(pCtx, if bNeg { iRes.wrapping_neg() } else { iRes });
                }
            } else if rc == 3 && bNeg {
                sqlite3_result_int64(pCtx, SMALLEST_INT64);
            } else if rc == 1 {
                return malformed(pCtx);
            } else {
                if bNeg {
                    n -= 1;
                    sz += 1;
                }
                to_double = true;
            }
        }
        JSONB_FLOAT5 | JSONB_FLOAT => {
            if sz == 0 {
                return malformed(pCtx);
            }
            to_double = true;
        }
        JSONB_TEXTRAW | JSONB_TEXT => {
            let z = sub(&pParse.aBlob, (i + n) as usize, sz as usize).to_vec();
            sqlite3_result_text(pCtx, Some(&z), z.len() as i32);
        }
        JSONB_TEXT5 | JSONB_TEXTJ => {
            /* Translate JSON formatted string into raw text */
            let z = sub(&pParse.aBlob, (i + n) as usize, sz as usize).to_vec();
            let mut zOut: Vec<u8> = Vec::with_capacity(sz as usize + 1);
            let mut iIn = 0u32;
            while iIn < sz {
                let c = at(&z, iIn as usize);
                if c == b'\\' {
                    let mut v = 0u32;
                    let szEscape = jsonUnescapeOneChar(tail(&z, iIn as usize), sz - iIn, &mut v);
                    if v <= 0x7f {
                        zOut.push(v as u8);
                    } else if v <= 0x7ff {
                        zOut.push((0xc0 | (v >> 6)) as u8);
                        zOut.push((0x80 | (v & 0x3f)) as u8);
                    } else if v < 0x10000 {
                        zOut.push((0xe0 | (v >> 12)) as u8);
                        zOut.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        zOut.push((0x80 | (v & 0x3f)) as u8);
                    } else if v == JSON_INVALID_CHAR {
                        /* Silently ignore illegal unicode */
                    } else {
                        zOut.push((0xf0 | (v >> 18)) as u8);
                        zOut.push((0x80 | ((v >> 12) & 0x3f)) as u8);
                        zOut.push((0x80 | ((v >> 6) & 0x3f)) as u8);
                        zOut.push((0x80 | (v & 0x3f)) as u8);
                    }
                    iIn += szEscape - 1;
                } else {
                    zOut.push(c);
                }
                iIn += 1;
            }
            sqlite3_result_text(pCtx, Some(&zOut), zOut.len() as i32);
        }
        JSONB_ARRAY | JSONB_OBJECT => {
            if eMode == 0 {
                if (user_flags(pCtx) & JSON_BLOB) != 0 {
                    eMode = 2;
                } else {
                    eMode = 1;
                }
            }
            let z = sub(&pParse.aBlob, i as usize, (sz + n) as usize).to_vec();
            if eMode == 2 {
                sqlite3_result_blob(pCtx, &z, z.len() as i32);
            } else {
                jsonReturnTextJsonFromBlob(pCtx, &z);
            }
        }
        _ => {
            return malformed(pCtx);
        }
    }
    if to_double {
        let z = sub(&pParse.aBlob, (i + n) as usize, sz as usize).to_vec();
        let mut r = 0.0f64;
        let rc = sqlite3AtoF(Some(cstr(&z)), &mut r);
        if rc <= 0 {
            return malformed(pCtx);
        }
        sqlite3_result_double(pCtx, r);
    }
}

/// pArg is a function argument that might be an SQL value or a JSON value.
/// Figure out what it is and encode it as a JSONB blob.  Return the
/// results in pParse.
///
/// If the argument is a BLOB that is clearly not a JSONB, then this
/// function might set an error message in ctx and return non-zero.
fn jsonFunctionArgToBlob(ctx: &mut Context, pArg: &mut Mem, pParse: &mut JsonParse) -> i32 {
    let eType = sqlite3_value_type(pArg);
    *pParse = JsonParse::default();
    match eType {
        SQLITE_BLOB => {
            if !jsonArgIsJsonb(pArg, pParse) {
                sqlite3_result_error(ctx, Some(b"JSON cannot hold BLOB values"), -1);
                return 1;
            }
        }
        SQLITE_TEXT => {
            let Some(zJson) = value_text(pArg) else {
                return 1;
            };
            let nJson = zJson.len() as i32;
            if sqlite3_value_subtype(pArg) == JSON_SUBTYPE {
                pParse.zJson = Some(Rc::from(&zJson[..]));
                pParse.nJson = nJson;
                if jsonConvertTextToBlob(pParse, Some(&mut *ctx)) != 0 {
                    sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
                    *pParse = JsonParse::default();
                    return 1;
                }
            } else {
                jsonBlobAppendNode(pParse, JSONB_TEXTRAW, nJson as u64, Some(&zJson));
            }
        }
        SQLITE_FLOAT => {
            if sqlite3IsNaN(sqlite3_value_double(pArg)) != 0 {
                jsonBlobAppendNode(pParse, JSONB_NULL, 0, None);
            } else {
                let Some(z) = value_text(pArg) else {
                    return 1;
                };
                let n = z.len();
                if at(&z, 0) == b'I' {
                    jsonBlobAppendNode(pParse, JSONB_FLOAT, 5, Some(b"9e999"));
                } else if at(&z, 0) == b'-' && at(&z, 1) == b'I' {
                    jsonBlobAppendNode(pParse, JSONB_FLOAT, 6, Some(b"-9e999"));
                } else {
                    jsonBlobAppendNode(pParse, JSONB_FLOAT, n as u64, Some(&z));
                }
            }
        }
        SQLITE_INTEGER => {
            let Some(z) = value_text(pArg) else {
                return 1;
            };
            jsonBlobAppendNode(pParse, JSONB_INT, z.len() as u64, Some(&z));
        }
        _ => {
            pParse.aBlob = vec![0x00];
            pParse.nBlob = 1;
            return 0;
        }
    }
    if pParse.oom != 0 {
        sqlite3_result_error_nomem(ctx);
        1
    } else {
        0
    }
}

/// Generate a path error.
///
///          rc                        error
///  -----------------       ----------------------
///  JSON_LOOKUP_ARRAY       "not an array"
///  JSON_LOOKUP_TOODEEP     "JSON nested too deep"
///  JSON_LOOKUP_ERROR       "malformed JSON"
///  otherwise...            "bad JSON path"
///
/// If ctx is not None then push the error message into ctx and return
/// None.  If ctx is None, then return the text of the error message.
fn jsonBadPathError(ctx: Option<&mut Context>, zPath: &[u8], rc: u32) -> Option<ZStr> {
    let zPath = cstr(zPath);
    let zMsg = if rc == JSON_LOOKUP_NOTARRAY {
        sqlite3_mprintf(b"not an array element: %Q", &[PfArg::from(zPath)])
    } else if rc == JSON_LOOKUP_ERROR {
        sqlite3_mprintf(b"malformed JSON", &[])
    } else if rc == JSON_LOOKUP_TOODEEP {
        sqlite3_mprintf(b"JSON path too deep", &[])
    } else {
        sqlite3_mprintf(b"bad JSON path: %Q", &[PfArg::from(zPath)])
    };
    match ctx {
        None => Some(zMsg),
        Some(ctx) => {
            sqlite3_result_error(ctx, Some(&zMsg), -1);
            None
        }
    }
}

/// argv[0] is a BLOB that seems likely to be a JSONB.  Subsequent
/// arguments come in pairs where each pair contains a JSON path and
/// content to insert or set at that patch.  Do the updates and return the
/// result.
fn jsonInsertIntoBlob(ctx: &mut Context, argv: &mut [Mem], eEdit: u8) {
    let argc = argv.len();
    let mut rc: u32 = 0;
    let mut zPath: Vec<u8> = Vec::new();
    debug_assert!((argc & 1) == 1);
    let flgs = if argc == 1 { 0 } else { JSON_EDITABLE };
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], flgs) else {
        return;
    };
    let mut i = 1;
    let mut patherror = false;
    while i + 1 < argc {
        if sqlite3_value_type(&mut argv[i]) == SQLITE_NULL {
            i += 2;
            continue;
        }
        match value_text(&mut argv[i]) {
            Some(z) => zPath = cstr(&z).to_vec(),
            None => {
                sqlite3_result_error_nomem(ctx);
                return;
            }
        }
        if at(&zPath, 0) != b'$' {
            patherror = true;
            break;
        }
        let mut ax = JsonParse::default();
        if jsonFunctionArgToBlob(ctx, &mut argv[i + 1], &mut ax) != 0 {
            return;
        }
        {
            let mut pp = p.borrow_mut();
            if at(&zPath, 1) == 0 {
                if eEdit == JEDIT_REPL || eEdit == JEDIT_SET {
                    let nb = pp.nBlob;
                    let nx = ax.nBlob;
                    jsonBlobEdit(&mut pp, 0, nb, Some(sub(&ax.aBlob, 0, nx as usize)), nx);
                }
                rc = 0;
            } else {
                pp.eEdit = eEdit;
                pp.nIns = ax.nBlob;
                ax.aBlob.truncate(ax.nBlob as usize);
                pp.aIns = std::mem::take(&mut ax.aBlob);
                pp.delta = 0;
                pp.iDepth = 0;
                rc = jsonLookupStep(&mut pp, 0, &zPath, 1, 0);
            }
        }
        if rc == JSON_LOOKUP_NOTFOUND {
            i += 2;
            continue;
        }
        if JSON_LOOKUP_ISERROR(rc) {
            patherror = true;
            break;
        }
        i += 2;
    }
    if patherror {
        drop(p);
        jsonBadPathError(Some(&mut *ctx), &zPath, rc);
        return;
    }
    jsonReturnParse(ctx, &p);
}

/// If pArg is a blob that seems like a JSONB blob, then initialize p to
/// point to that JSONB and return TRUE.  If pArg does not seem like a
/// JSONB blob, then return FALSE.
///
/// For small BLOBs (having no more than 7 bytes of payload) a full
/// validity check is done.
fn jsonArgIsJsonb(pArg: &mut Mem, p: &mut JsonParse) -> bool {
    if sqlite3_value_type(pArg) != SQLITE_BLOB {
        return false;
    }
    p.aBlob = sqlite3_value_blob(pArg)
        .map(|b| b.to_vec())
        .unwrap_or_default();
    p.nBlob = sqlite3_value_bytes(pArg) as u32;
    p.aBlob.truncate(p.nBlob as usize);
    if p.nBlob > 0 {
        let c = p.aBlob[0];
        if (c & 0x0f) <= JSONB_OBJECT {
            let (n, sz) = jsonbPayloadSize(p, 0);
            if n > 0
                && sz + n == p.nBlob
                && ((c & 0x0f) > JSONB_FALSE || sz == 0)
                && (sz > 7
                    || (c != 0x7b && c != 0x5b && !sqlite3Isdigit(c))
                    || jsonbValidityCheck(p, 0, p.nBlob, 1) == 0)
            {
                return true;
            }
        }
    }
    p.aBlob = Vec::new();
    p.nBlob = 0;
    false
}

/// Generate a JsonParse object, containing valid JSONB in aBlob and nBlob,
/// from the SQL function argument pArg.
///
/// If any errors are detected, an appropriate error messages is set using
/// sqlite3_result_error() or the equivalent and this routine returns
/// None.  This routine also returns None if the pArg argument is an SQL
/// NULL value, but no error message is set in that case.
fn jsonParseFuncArg(ctx: &mut Context, pArg: &mut Mem, flgs: u32) -> Option<JsonParseRef> {
    let eType = sqlite3_value_type(pArg);
    if eType == SQLITE_NULL {
        return None;
    }
    let mut pFromCache: Option<JsonParseRef> = None;
    if (flgs & JSON_NOCACHE) == 0 {
        pFromCache = jsonCacheSearch(ctx, pArg);
    }
    if let Some(c) = &pFromCache {
        if (flgs & JSON_EDITABLE) == 0 {
            return Some(c.clone());
        }
    }
    loop {
        /* rebuild_from_cache: */
        let mut p = JsonParse::default();
        if let Some(c) = pFromCache.take() {
            let c = c.borrow();
            let nBlob = c.nBlob;
            p.aBlob = sub(&c.aBlob, 0, nBlob as usize).to_vec();
            p.nBlobAlloc = nBlob;
            p.nBlob = nBlob;
            p.hasNonstd = c.hasNonstd;
            return Some(Rc::new(RefCell::new(p)));
        }
        if eType == SQLITE_BLOB {
            if jsonArgIsJsonb(pArg, &mut p) {
                if (flgs & JSON_EDITABLE) != 0 && !jsonBlobMakeEditable(&mut p, 0) {
                    sqlite3_result_error_nomem(ctx);
                    return None;
                }
                return Some(Rc::new(RefCell::new(p)));
            }
            /* If the blob is not valid JSONB, fall through into trying to
             ** cast the blob into text which is then interpreted as JSON.
             ** (tag-20240123-a) */
        }
        let bSubtype = sqlite3_value_subtype(pArg) == JSON_SUBTYPE;
        let ptr = pArg.z.as_ptr() as usize;
        let zJson = value_text(pArg).unwrap_or_default();
        p.nJson = sqlite3_value_bytes(pArg);
        p.zJson = Some(Rc::from(&zJson[..]));
        if p.nJson == 0 {
            /* json_pfa_malformed: */
            if (flgs & JSON_KEEPERROR) != 0 {
                p.nErr = 1;
                return Some(Rc::new(RefCell::new(p)));
            } else {
                sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
                return None;
            }
        }
        let e = if (flgs & JSON_KEEPERROR) != 0 {
            jsonConvertTextToBlob(&mut p, None)
        } else {
            jsonConvertTextToBlob(&mut p, Some(&mut *ctx))
        };
        if e != 0 {
            if (flgs & JSON_KEEPERROR) != 0 {
                p.nErr = 1;
                return Some(Rc::new(RefCell::new(p)));
            } else {
                return None;
            }
        } else {
            /* Rust: a text value carrying the JSON subtype is (almost always)
             ** the RCStr result of another JSON function, whose pointer C
             ** keeps in the cache entry. */
            p.zJsonPtr = if bSubtype { ptr } else { 0 };
            p.bJsonIsRCStr = true;
            let pr = Rc::new(RefCell::new(p));
            let rc = jsonCacheInsert(ctx, &pr);
            if rc == SQLITE_NOMEM {
                sqlite3_result_error_nomem(ctx);
                return None;
            }
            if (flgs & JSON_EDITABLE) != 0 {
                pFromCache = Some(pr);
                continue;
            }
            return Some(pr);
        }
    }
}

/// Make the return value of a JSON function either the raw JSONB blob or
/// make it JSON text, depending on whether the JSON_BLOB flag is set on
/// the function.
fn jsonReturnParse(ctx: &mut Context, p: &JsonParseRef) {
    if p.borrow().oom != 0 {
        sqlite3_result_error_nomem(ctx);
        return;
    }
    let flgs = user_flags(ctx);
    if flgs & JSON_BLOB != 0 {
        let mut x = p.borrow_mut();
        let nb = x.nBlob as usize;
        sqlite3_result_blob(ctx, sub(&x.aBlob, 0, nb), nb as i32);
        if x.nBlobAlloc > 0 && !x.bReadOnly {
            /* (ownership of the blob passes to the result) */
            x.nBlobAlloc = 0;
            x.aBlob.truncate(nb);
        }
    } else {
        let mut s = jsonStringNew();
        {
            let mut x = p.borrow_mut();
            x.delta = 0;
            jsonTranslateBlobToText(&mut x, 0, &mut s, ctx);
        }
        jsonReturnString(&mut s, Some(p), ctx);
        sqlite3_result_subtype(ctx, JSON_SUBTYPE);
    }
}

/****************************************************************************
** Scalar SQL function implementations
****************************************************************************/

/// Implementation of the json_quote(VALUE) function.
fn jsonQuoteFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let mut jx = jsonStringNew();
    jsonAppendSqlValue(&mut jx, &mut argv[0], ctx);
    jsonReturnString(&mut jx, None, ctx);
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}

/// Implementation of the json_array(VALUE,...) function.
fn jsonArrayFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let mut jx = jsonStringNew();
    jsonAppendChar(&mut jx, b'[');
    for a in argv.iter_mut() {
        jsonAppendSeparator(&mut jx);
        jsonAppendSqlValue(&mut jx, a, ctx);
    }
    jsonAppendChar(&mut jx, b']');
    jsonReturnString(&mut jx, None, ctx);
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}

/// json_array_length(JSON)
/// json_array_length(JSON, PATH)
///
/// Return the number of elements in the top-level JSON array.  Return 0
/// if the input is not a well-formed JSON array.
fn jsonArrayLengthFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let mut cnt: i64 = 0;
    let mut eErr = false;
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], 0) else {
        return;
    };
    let mut pp = p.borrow_mut();
    let mut i: u32;
    if argv.len() == 2 {
        let Some(zPath) = value_text(&mut argv[1]) else {
            return;
        };
        let zPath = cstr(&zPath).to_vec();
        i = if at(&zPath, 0) == b'$' {
            jsonLookupStep(&mut pp, 0, &zPath, 1, 0)
        } else {
            jsonLookupStep(&mut pp, 0, b"@", 0, 0)
        };
        if JSON_LOOKUP_ISERROR(i) {
            if i == JSON_LOOKUP_NOTFOUND {
                /* no-op */
            } else {
                jsonBadPathError(Some(&mut *ctx), &zPath, i);
            }
            eErr = true;
            i = 0;
        }
    } else {
        i = 0;
    }
    if (pp.ab(i) & 0x0f) == JSONB_ARRAY {
        cnt = jsonbArrayCount(&pp, i) as i64;
    }
    if !eErr {
        sqlite3_result_int64(ctx, cnt);
    }
}

/// True if the string is all alphanumerics and underscores
fn jsonAllAlphanum(z: &[u8]) -> bool {
    z.iter().all(|&c| sqlite3Isalnum(c) || c == b'_')
}

/// json_extract(JSON, PATH, ...)
/// "->"(JSON,PATH)
/// "->>"(JSON,PATH)
///
/// Return the element described by PATH.  Return NULL if that PATH element
/// is not found.
fn jsonExtractFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if argc < 2 {
        return;
    }
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], 0) else {
        return;
    };
    let mut p = p.borrow_mut();
    let flags = user_flags(ctx);
    let mut jx = jsonStringNew();
    if argc > 2 {
        jsonAppendChar(&mut jx, b'[');
    }
    for i in 1..argc {
        /* With a single PATH argument */
        let Some(zPath) = value_text(&mut argv[i]) else {
            return;
        };
        let zPath = cstr(&zPath).to_vec();
        let nPath = zPath.len();
        let j: u32;
        if at(&zPath, 0) == b'$' {
            j = jsonLookupStep(&mut p, 0, &zPath, 1, 0);
        } else if (flags & JSON_ABPATH) != 0 {
            /* The -> and ->> operators accept abbreviated PATH arguments.
             **
             **     NUMBER   ==>  $[NUMBER]     // PG compatible
             **     LABEL    ==>  $.LABEL       // PG compatible
             **     [NUMBER] ==>  $[NUMBER]     // Not PG.  Purely for convenience
             **
             ** If the NUMBER is negative, then PG counts from the right of the
             ** array:
             **
             **     NUMBER   ==>  $[#NUMBER]    // PG compatible
             */
            jsonStringInit(&mut jx);
            if sqlite3_value_type(&mut argv[i]) == SQLITE_INTEGER {
                jsonAppendRawNZ(&mut jx, b"[");
                if at(&zPath, 0) == b'-' {
                    jsonAppendRawNZ(&mut jx, b"#");
                }
                jsonAppendRaw(&mut jx, &zPath);
                jsonAppendRawNZ(&mut jx, b"]\0");
            } else if jsonAllAlphanum(&zPath) {
                jsonAppendRawNZ(&mut jx, b".");
                jsonAppendRaw(&mut jx, &zPath);
            } else if at(&zPath, 0) == b'[' && nPath >= 3 && zPath[nPath - 1] == b']' {
                jsonAppendRaw(&mut jx, &zPath);
            } else {
                jsonAppendRawNZ(&mut jx, b".\"");
                jsonAppendRaw(&mut jx, &zPath);
                jsonAppendRawNZ(&mut jx, b"\"");
            }
            jsonStringTerminate(&mut jx);
            let path = std::mem::take(&mut jx.zBuf);
            j = jsonLookupStep(&mut p, 0, &path, 0, 0);
            jsonStringReset(&mut jx);
        } else {
            jsonBadPathError(Some(&mut *ctx), &zPath, 0);
            return;
        }
        if j < p.nBlob {
            if argc == 2 {
                if flags & JSON_JSON != 0 {
                    jsonStringInit(&mut jx);
                    jsonTranslateBlobToText(&mut p, j, &mut jx, ctx);
                    jsonReturnString(&mut jx, None, ctx);
                    jsonStringReset(&mut jx);
                    debug_assert!((flags & JSON_BLOB) == 0);
                    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
                } else {
                    jsonReturnFromBlob(&mut p, j, ctx, 0);
                    if (flags & (JSON_SQL | JSON_BLOB)) == 0 && (p.ab(j) & 0x0f) >= JSONB_ARRAY {
                        sqlite3_result_subtype(ctx, JSON_SUBTYPE);
                    }
                }
            } else {
                jsonAppendSeparator(&mut jx);
                jsonTranslateBlobToText(&mut p, j, &mut jx, ctx);
            }
        } else if j == JSON_LOOKUP_NOTFOUND {
            if argc == 2 {
                return; /* Return NULL if not found */
            } else {
                jsonAppendSeparator(&mut jx);
                jsonAppendRawNZ(&mut jx, b"null");
            }
        } else {
            jsonBadPathError(Some(&mut *ctx), &zPath, j);
            return;
        }
    }
    if argc > 2 {
        jsonAppendChar(&mut jx, b']');
        jsonReturnString(&mut jx, None, ctx);
        if (flags & JSON_BLOB) == 0 {
            sqlite3_result_subtype(ctx, JSON_SUBTYPE);
        }
    }
}

/* Return codes for jsonMergePatch() */
const JSON_MERGE_OK: i32 = 0; /* Success */
const JSON_MERGE_BADTARGET: i32 = 1; /* Malformed TARGET blob */
const JSON_MERGE_BADPATCH: i32 = 2; /* Malformed PATCH blob */
const JSON_MERGE_OOM: i32 = 3; /* Out-of-memory condition */
const JSON_MERGE_TOODEEP: i32 = 4; /* Nested too deep */

/// RFC-7396 MergePatch for two JSONB blobs.
///
/// pTarget is the target. pPatch is the patch.  The target is updated in
/// place.  The patch is read-only.
fn jsonMergePatch(
    pTarget: &mut JsonParse,
    iTarget: u32,
    pPatch: &JsonParse,
    iPatch: u32,
    iDepth: u32,
) -> i32 {
    debug_assert!(iTarget < pTarget.nBlob);
    debug_assert!(iPatch < pPatch.nBlob);
    let mut x = pPatch.ab(iPatch) & 0x0f;
    if x != JSONB_OBJECT {
        /* Algorithm line 02 */
        let (n, sz) = jsonbPayloadSize(pPatch, iPatch);
        let szPatch = n + sz;
        let (n, sz) = jsonbPayloadSize(pTarget, iTarget);
        let szTarget = n + sz;
        jsonBlobEdit(
            pTarget,
            iTarget,
            szTarget,
            Some(sub(&pPatch.aBlob, iPatch as usize, szPatch as usize)),
            szPatch,
        );
        return if pTarget.oom != 0 {
            JSON_MERGE_OOM
        } else {
            JSON_MERGE_OK
        }; /* Line 03 */
    }
    x = pTarget.ab(iTarget) & 0x0f;
    if x != JSONB_OBJECT {
        /* Algorithm line 05 */
        let (n, sz) = jsonbPayloadSize(pTarget, iTarget);
        jsonBlobEdit(pTarget, iTarget + n, sz, None, 0);
        x = pTarget.ab(iTarget);
        pTarget.aBlob[iTarget as usize] = (x & 0xf0) | JSONB_OBJECT;
    }
    let (n, sz) = jsonbPayloadSize(pPatch, iPatch);
    if n == 0 {
        return JSON_MERGE_BADPATCH;
    }
    let mut iPCursor = iPatch + n;
    let iPEnd = iPCursor + sz;
    let (n, sz) = jsonbPayloadSize(pTarget, iTarget);
    if n == 0 {
        return JSON_MERGE_BADTARGET;
    }
    let iTStart = iTarget + n;
    let iTEndBE = iTStart + sz;

    while iPCursor < iPEnd {
        /* Algorithm line 07 */
        let iPLabel = iPCursor;
        let ePLabel = pPatch.ab(iPCursor) & 0x0f;
        if !(JSONB_TEXT..=JSONB_TEXTRAW).contains(&ePLabel) {
            return JSON_MERGE_BADPATCH;
        }
        let (nPLabel, szPLabel) = jsonbPayloadSize(pPatch, iPCursor);
        if nPLabel == 0 {
            return JSON_MERGE_BADPATCH;
        }
        let iPValue = iPCursor + nPLabel + szPLabel;
        if iPValue >= iPEnd {
            return JSON_MERGE_BADPATCH;
        }
        let (nPValue, szPValue) = jsonbPayloadSize(pPatch, iPValue);
        if nPValue == 0 {
            return JSON_MERGE_BADPATCH;
        }
        iPCursor = iPValue + nPValue + szPValue;
        if iPCursor > iPEnd {
            return JSON_MERGE_BADPATCH;
        }

        let mut iTCursor = iTStart;
        let iTEnd = iTEndBE.wrapping_add(pTarget.delta as u32);
        let mut iTLabel = 0u32;
        let mut nTLabel = 0u32;
        let mut szTLabel = 0u32;
        let mut iTValue = 0u32;
        let mut nTValue = 0u32;
        let mut szTValue = 0u32;
        while iTCursor < iTEnd {
            iTLabel = iTCursor;
            let eTLabel = pTarget.ab(iTCursor) & 0x0f;
            if !(JSONB_TEXT..=JSONB_TEXTRAW).contains(&eTLabel) {
                return JSON_MERGE_BADTARGET;
            }
            (nTLabel, szTLabel) = jsonbPayloadSize(pTarget, iTCursor);
            if nTLabel == 0 {
                return JSON_MERGE_BADTARGET;
            }
            iTValue = iTLabel + nTLabel + szTLabel;
            if iTValue >= iTEnd {
                return JSON_MERGE_BADTARGET;
            }
            (nTValue, szTValue) = jsonbPayloadSize(pTarget, iTValue);
            if nTValue == 0 {
                return JSON_MERGE_BADTARGET;
            }
            if iTValue + nTValue + szTValue > iTEnd {
                return JSON_MERGE_BADTARGET;
            }
            let isEqual = jsonLabelCompare(
                tail(&pPatch.aBlob, (iPLabel + nPLabel) as usize),
                szPLabel,
                ePLabel == JSONB_TEXT || ePLabel == JSONB_TEXTRAW,
                tail(&pTarget.aBlob, (iTLabel + nTLabel) as usize),
                szTLabel,
                eTLabel == JSONB_TEXT || eTLabel == JSONB_TEXTRAW,
            );
            if isEqual {
                break;
            }
            iTCursor = iTValue + nTValue + szTValue;
        }
        x = pPatch.ab(iPValue) & 0x0f;
        if iTCursor < iTEnd {
            /* A match was found.  Algorithm line 08 */
            if x == 0 {
                /* Patch value is NULL.  Algorithm line 09 */
                jsonBlobEdit(
                    pTarget,
                    iTLabel,
                    nTLabel + szTLabel + nTValue + szTValue,
                    None,
                    0,
                );
                if pTarget.oom != 0 {
                    return JSON_MERGE_OOM;
                }
            } else {
                /* Algorithm line 12 */
                let savedDelta = pTarget.delta;
                pTarget.delta = 0;
                if iDepth >= JSON_MAX_DEPTH as u32 {
                    return JSON_MERGE_TOODEEP;
                }
                let rc = jsonMergePatch(pTarget, iTValue, pPatch, iPValue, iDepth + 1);
                if rc != 0 {
                    return rc;
                }
                pTarget.delta += savedDelta;
            }
        } else if x > 0 {
            /* Algorithm line 13 */
            /* No match and patch value is not NULL */
            let szNew = szPLabel + nPLabel;
            if (pPatch.ab(iPValue) & 0x0f) != JSONB_OBJECT {
                /* Line 14 */
                jsonBlobEdit(pTarget, iTEnd, 0, None, szPValue + nPValue + szNew);
                if pTarget.oom != 0 {
                    return JSON_MERGE_OOM;
                }
                let e = iTEnd as usize;
                pTarget.aBlob[e..e + szNew as usize].copy_from_slice(sub(
                    &pPatch.aBlob,
                    iPLabel as usize,
                    szNew as usize,
                ));
                let nv = (szPValue + nPValue) as usize;
                pTarget.aBlob[e + szNew as usize..e + szNew as usize + nv].copy_from_slice(sub(
                    &pPatch.aBlob,
                    iPValue as usize,
                    nv,
                ));
            } else {
                jsonBlobEdit(pTarget, iTEnd, 0, None, szNew + 1);
                if pTarget.oom != 0 {
                    return JSON_MERGE_OOM;
                }
                let e = iTEnd as usize;
                pTarget.aBlob[e..e + szNew as usize].copy_from_slice(sub(
                    &pPatch.aBlob,
                    iPLabel as usize,
                    szNew as usize,
                ));
                pTarget.aBlob[e + szNew as usize] = 0x00;
                let savedDelta = pTarget.delta;
                pTarget.delta = 0;
                if iDepth >= JSON_MAX_DEPTH as u32 {
                    return JSON_MERGE_TOODEEP;
                }
                let rc = jsonMergePatch(pTarget, iTEnd + szNew, pPatch, iPValue, iDepth + 1);
                if rc != 0 {
                    return rc;
                }
                pTarget.delta += savedDelta;
            }
        }
    }
    if pTarget.delta != 0 {
        jsonAfterEditSizeAdjust(pTarget, iTarget);
    }
    if pTarget.oom != 0 {
        JSON_MERGE_OOM
    } else {
        JSON_MERGE_OK
    }
}

/// Implementation of the json_mergepatch(JSON1,JSON2) function.  Return a
/// JSON object that is the result of running the RFC 7396 MergePatch()
/// algorithm on the two arguments.
fn jsonPatchFunc(ctx: &mut Context, argv: &mut [Mem]) {
    debug_assert!(argv.len() == 2);
    let Some(pTarget) = jsonParseFuncArg(ctx, &mut argv[0], JSON_EDITABLE) else {
        return;
    };
    let pPatch = jsonParseFuncArg(ctx, &mut argv[1], 0);
    if let Some(pPatch) = pPatch {
        let rc = {
            let mut t = pTarget.borrow_mut();
            let pp = pPatch.borrow();
            jsonMergePatch(&mut t, 0, &pp, 0, 0)
        };
        if rc == JSON_MERGE_OK {
            jsonReturnParse(ctx, &pTarget);
        } else if rc == JSON_MERGE_OOM {
            sqlite3_result_error_nomem(ctx);
        } else if rc == JSON_MERGE_TOODEEP {
            sqlite3_result_error(ctx, Some(b"JSON nested too deep"), -1);
        } else {
            sqlite3_result_error(ctx, Some(b"malformed JSON"), -1);
        }
    }
}

/// Implementation of the json_object(NAME,VALUE,...) function.  Return a
/// JSON object that contains all name/value given in arguments.  Or if any
/// name is not a string or if any value is a BLOB, throw an error.
fn jsonObjectFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if argc & 1 != 0 {
        sqlite3_result_error(
            ctx,
            Some(b"json_object() requires an even number of arguments"),
            -1,
        );
        return;
    }
    let mut jx = jsonStringNew();
    jsonAppendChar(&mut jx, b'{');
    let mut i = 0;
    while i < argc {
        if sqlite3_value_type(&mut argv[i]) != SQLITE_TEXT {
            sqlite3_result_error(ctx, Some(b"json_object() labels must be TEXT"), -1);
            return;
        }
        jsonAppendSeparator(&mut jx);
        let z = value_text(&mut argv[i]).unwrap_or_default();
        jsonAppendString(&mut jx, &z);
        jsonAppendChar(&mut jx, b':');
        jsonAppendSqlValue(&mut jx, &mut argv[i + 1], ctx);
        i += 2;
    }
    jsonAppendChar(&mut jx, b'}');
    jsonReturnString(&mut jx, None, ctx);
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}

/// json_remove(JSON, PATH, ...)
///
/// Remove the named elements from JSON and return the result.  malformed
/// JSON or PATH arguments result in an error.
fn jsonRemoveFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if argc < 1 {
        return;
    }
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], if argc > 1 { JSON_EDITABLE } else { 0 })
    else {
        return;
    };
    for i in 1..argc {
        let Some(zPath) = value_text(&mut argv[i]) else {
            return; /* json_remove_done */
        };
        let zPath = cstr(&zPath).to_vec();
        if at(&zPath, 0) != b'$' {
            /* json_remove_patherror: */
            jsonBadPathError(Some(&mut *ctx), &zPath, 0);
            return;
        }
        if at(&zPath, 1) == 0 {
            /* json_remove(j,'$') returns NULL */
            return;
        }
        let rc = {
            let mut pp = p.borrow_mut();
            pp.eEdit = JEDIT_DEL;
            pp.delta = 0;
            jsonLookupStep(&mut pp, 0, &zPath, 1, 0)
        };
        if JSON_LOOKUP_ISERROR(rc) {
            if rc == JSON_LOOKUP_NOTFOUND {
                continue; /* No-op */
            } else {
                jsonBadPathError(Some(&mut *ctx), &zPath, rc);
            }
            return;
        }
    }
    jsonReturnParse(ctx, &p);
}

/// json_replace(JSON, PATH, VALUE, ...)
///
/// Replace the value at PATH with VALUE.  If PATH does not already exist,
/// this routine is a no-op.  If JSON or PATH is malformed, throw an error.
fn jsonReplaceFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let argc = argv.len();
    if argc < 1 {
        return;
    }
    if (argc & 1) == 0 {
        jsonWrongNumArgs(ctx, b"replace");
        return;
    }
    jsonInsertIntoBlob(ctx, argv, JEDIT_REPL);
}

/// json_set(JSON, PATH, VALUE, ...)
///
/// Set the value at PATH to VALUE.  Create the PATH if it does not already
/// exist.  Overwrite existing values that do exist.  If JSON or PATH is
/// malformed, throw an error.
///
/// json_insert(JSON, PATH, VALUE, ...)
///
/// Create PATH and initialize it to VALUE.  If PATH already exists, this
/// routine is a no-op.  If JSON or PATH is malformed, throw an error.
fn jsonSetFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let flags = user_flags(ctx);
    let eInsType = JSON_INSERT_TYPE(flags);
    static azInsType: [&[u8]; 3] = [b"insert", b"set", b"array_insert"];
    static aEditType: [u8; 3] = [JEDIT_INS, JEDIT_SET, JEDIT_AINS];
    let argc = argv.len();
    if argc < 1 {
        return;
    }
    debug_assert!(eInsType <= 2);
    if (argc & 1) == 0 {
        jsonWrongNumArgs(ctx, azInsType[eInsType]);
        return;
    }
    jsonInsertIntoBlob(ctx, argv, aEditType[eInsType]);
}

/// json_type(JSON)
/// json_type(JSON, PATH)
///
/// Return the top-level "type" of a JSON string.  json_type() raises an
/// error if either the JSON or PATH inputs are not well-formed.
fn jsonTypeFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], 0) else {
        return;
    };
    let mut pp = p.borrow_mut();
    let i: u32;
    if argv.len() == 2 {
        let Some(zPath) = value_text(&mut argv[1]) else {
            return;
        };
        let zPath = cstr(&zPath).to_vec();
        if at(&zPath, 0) != b'$' {
            jsonBadPathError(Some(&mut *ctx), &zPath, 0);
            return;
        }
        i = jsonLookupStep(&mut pp, 0, &zPath, 1, 0);
        if JSON_LOOKUP_ISERROR(i) {
            if i == JSON_LOOKUP_NOTFOUND {
                /* no-op */
            } else {
                jsonBadPathError(Some(&mut *ctx), &zPath, i);
            }
            return;
        }
    } else {
        i = 0;
    }
    let t = jsonbType[(pp.ab(i) & 0x0f) as usize];
    sqlite3_result_text(ctx, Some(t), t.len() as i32);
}

/// json_pretty(JSON)
/// json_pretty(JSON, INDENT)
///
/// Return text that is a pretty-printed rendering of the input JSON.  If
/// the argument is not valid JSON, return NULL.
fn jsonPrettyFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let Some(p) = jsonParseFuncArg(ctx, &mut argv[0], 0) else {
        return;
    };
    let mut s = jsonStringNew();
    let zIndent: Vec<u8> = if argv.len() == 1 {
        b"    ".to_vec()
    } else {
        match value_text(&mut argv[1]) {
            None => b"    ".to_vec(),
            Some(z) => cstr(&z).to_vec(),
        }
    };
    {
        let mut pp = p.borrow_mut();
        let mut x = JsonPretty {
            pParse: &mut *pp,
            pOut: &mut s,
            zIndent: &zIndent,
            nIndent: 0,
        };
        jsonTranslateBlobToPrettyText(&mut x, 0, ctx);
    }
    jsonReturnString(&mut s, None, ctx);
}

/// json_valid(JSON)
/// json_valid(JSON, FLAGS)
///
/// Check the JSON argument to see if it is well-formed.  The FLAGS
/// argument encodes the various constraints on what is meant by
/// "well-formed":
///
///     0x01      Canonical RFC-8259 JSON text
///     0x02      JSON text with optional JSON-5 extensions
///     0x04      Superficially appears to be JSONB
///     0x08      Strictly well-formed JSONB
///
/// If the FLAGS argument is omitted, it defaults to 1.
fn jsonValidFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let mut flags: u8 = 1;
    let mut res: u8 = 0;
    if argv.len() == 2 {
        let f = sqlite3_value_int64(&mut argv[1]);
        if !(1..=15).contains(&f) {
            sqlite3_result_error(
                ctx,
                Some(b"FLAGS parameter to json_valid() must be between 1 and 15"),
                -1,
            );
            return;
        }
        flags = (f & 0x0f) as u8;
    }
    let eType = sqlite3_value_type(&mut argv[0]);
    if eType == SQLITE_NULL {
        return;
    }
    let mut asText = true;
    if eType == SQLITE_BLOB {
        let mut py = JsonParse::default();
        if jsonArgIsJsonb(&mut argv[0], &mut py) {
            if flags & 0x04 != 0 {
                /* Superficial checking only - accomplished by the
                 ** jsonArgIsJsonb() call above. */
                res = 1;
            } else if flags & 0x08 != 0 {
                /* Strict checking. */
                res = (0 == jsonbValidityCheck(&py, 0, py.nBlob, 1)) as u8;
            }
            asText = false;
        }
        /* Fall through into interpreting the input as text.  See note above
         ** at tag-20240123-a. */
    }
    if asText && (flags & 0x3) != 0 {
        let p = jsonParseFuncArg(ctx, &mut argv[0], JSON_KEEPERROR | JSON_NOCACHE);
        if let Some(p) = p {
            let p = p.borrow();
            if p.oom != 0 {
                sqlite3_result_error_nomem(ctx);
            } else if p.nErr != 0 {
                /* no-op */
            } else if (flags & 0x02) != 0 || p.hasNonstd == 0 {
                res = 1;
            }
        } else {
            sqlite3_result_error_nomem(ctx);
        }
    }
    sqlite3_result_int(ctx, res as i32);
}

/// json_error_position(JSON)
///
/// If the argument is NULL, return NULL
///
/// If the argument is BLOB, do a full validity check and return non-zero
/// if the check fails.  The return value is the approximate 1-based offset
/// to the byte of the element that contains the first error.
///
/// Otherwise interpret the argument is TEXT (even if it is numeric) and
/// return the 1-based character position for where the parser first
/// recognized that the input was not valid JSON, or return 0 if the input
/// text looks ok.  JSON-5 extensions are accepted.
fn jsonErrorFunc(ctx: &mut Context, argv: &mut [Mem]) {
    let mut iErrPos: i64 = 0;
    let mut s = JsonParse::default();
    debug_assert!(argv.len() == 1);
    if jsonArgIsJsonb(&mut argv[0], &mut s) {
        iErrPos = jsonbValidityCheck(&s, 0, s.nBlob, 1) as i64;
    } else {
        let Some(z) = value_text(&mut argv[0]) else {
            return; /* NULL input or OOM */
        };
        s.nJson = sqlite3_value_bytes(&mut argv[0]);
        s.zJson = Some(Rc::from(&z[..]));
        if jsonConvertTextToBlob(&mut s, None) != 0 {
            if s.oom != 0 {
                iErrPos = -1;
            } else {
                /* Convert byte-offset s.iErr into a character offset */
                let mut k = 0usize;
                while k < s.iErr as usize && at(&z, k) != 0 {
                    if (z[k] & 0xc0) != 0x80 {
                        iErrPos += 1;
                    }
                    k += 1;
                }
                iErrPos += 1;
            }
        }
    }
    jsonParseReset(&mut s);
    if iErrPos < 0 {
        sqlite3_result_error_nomem(ctx);
    } else {
        sqlite3_result_int64(ctx, iErrPos);
    }
}

/****************************************************************************
** Aggregate SQL function implementations
****************************************************************************/

/// Take the JsonString aggregate context out of the context (so that the
/// context can be used while the string is being modified).
fn aggTake(ctx: &mut Context, nByte: i32) -> Option<JsonString> {
    sqlite3_aggregate_context::<JsonString>(ctx, nByte).map(std::mem::take)
}

fn aggPut(ctx: &mut Context, s: JsonString) {
    if let Some(p) = sqlite3_aggregate_context::<JsonString>(ctx, 1) {
        *p = s;
    }
}

/// json_group_array(VALUE)
///
/// Return a JSON array composed of all values in the aggregate.
fn jsonArrayStep(ctx: &mut Context, argv: &mut [Mem]) {
    let Some(mut s) = aggTake(ctx, 1) else {
        return;
    };
    if !s.init {
        jsonStringInit(&mut s);
        jsonAppendChar(&mut s, b'[');
    } else if s.zBuf.len() > 1 {
        jsonAppendChar(&mut s, b',');
    }
    jsonAppendSqlValue(&mut s, &mut argv[0], ctx);
    aggPut(ctx, s);
}

fn jsonArrayCompute(ctx: &mut Context, isFinal: bool) {
    let flags = user_flags(ctx);
    if let Some(mut s) = aggTake(ctx, 0) {
        jsonAppendRawNZ(&mut s, b"]\0");
        jsonStringTrimOneChar(&mut s);
        if s.eErr != 0 {
            jsonReturnString(&mut s, None, ctx);
            aggPut(ctx, s);
            return;
        } else if flags & JSON_BLOB != 0 {
            jsonReturnStringAsBlob(&mut s, ctx);
            if !isFinal {
                jsonStringTrimOneChar(&mut s);
            }
            aggPut(ctx, s);
            return;
        } else if isFinal {
            sqlite3_result_text(ctx, Some(&s.zBuf), s.zBuf.len() as i32);
            s.bStatic = true;
        } else {
            sqlite3_result_text(ctx, Some(&s.zBuf), s.zBuf.len() as i32);
            jsonStringTrimOneChar(&mut s);
        }
        aggPut(ctx, s);
    } else if flags & JSON_BLOB != 0 {
        sqlite3_result_blob(ctx, &[0x0b], 1);
    } else {
        sqlite3_result_text(ctx, Some(b"[]"), 2);
    }
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}
fn jsonArrayValue(ctx: &mut Context) {
    jsonArrayCompute(ctx, false);
}
fn jsonArrayFinal(ctx: &mut Context) {
    jsonArrayCompute(ctx, true);
}

/// This method works for both json_group_array() and json_group_object().
/// It works by removing the first element of the group by searching
/// forward to the first comma (",") that is not within a string and
/// deleting all text through that comma.
fn jsonGroupInverse(ctx: &mut Context, _argv: &mut [Mem]) {
    let Some(pStr) = sqlite3_aggregate_context::<JsonString>(ctx, 0) else {
        return;
    };
    let z = &mut pStr.zBuf;
    let nUsed = z.len();
    let mut inStr = false;
    let mut nNest = 0i32;
    let mut i = 1usize;
    while i < nUsed {
        let c = z[i];
        if !(c != b',' || inStr || nNest != 0) {
            break;
        }
        if c == b'"' {
            inStr = !inStr;
        } else if c == b'\\' {
            i += 1;
        } else if !inStr {
            if c == b'{' || c == b'[' {
                nNest += 1;
            }
            if c == b'}' || c == b']' {
                nNest -= 1;
            }
        }
        i += 1;
    }
    if i < nUsed {
        z.drain(1..=i);
    } else {
        z.truncate(1);
    }
}

/// json_group_obj(NAME,VALUE)
///
/// Return a JSON object composed of all names and values in the
/// aggregate.  Rows for which NAME is NULL do not result in a new entry,
/// but an "@" marker is inserted (and the first character of the string is
/// changed to "@") so that xInverse() requests can be processed.
fn jsonObjectStep(ctx: &mut Context, argv: &mut [Mem]) {
    let Some(mut s) = aggTake(ctx, 1) else {
        return;
    };
    let z = value_text(&mut argv[0]);
    if !s.init {
        jsonStringInit(&mut s);
        jsonAppendChar(&mut s, b'{');
    } else if s.zBuf.len() > 1 {
        jsonAppendChar(&mut s, b',');
    }
    if let Some(z) = z {
        jsonAppendString(&mut s, cstr(&z));
        jsonAppendChar(&mut s, b':');
        jsonAppendSqlValue(&mut s, &mut argv[1], ctx);
    } else {
        if let Some(c) = s.zBuf.first_mut() {
            *c = b'@';
        }
        jsonAppendRawNZ(&mut s, b"@");
    }
    aggPut(ctx, s);
}

fn jsonObjectCompute(ctx: &mut Context, isFinal: bool) {
    let flags = user_flags(ctx);
    if let Some(mut og) = aggTake(ctx, 0) {
        jsonAppendRawNZ(&mut og, b"}\0"); /* Ensure it is zero-terminated */
        jsonStringTrimOneChar(&mut og); /* Remove the zero terminator */
        if og.eErr != 0 {
            jsonReturnString(&mut og, None, ctx);
            aggPut(ctx, og);
            return;
        }
        let mut tmp: Option<JsonString> = None;
        if at(&og.zBuf, 0) != b'{' {
            /* The string contains null entries that need to be removed */
            if !isFinal {
                /* Work with a temporary copy of the string if this is not the
                 ** final result */
                let mut t = jsonStringNew();
                let mut src = og.zBuf.clone();
                src.push(0);
                jsonAppendRawNZ(&mut t, &src);
                if t.eErr != 0 {
                    jsonReturnString(&mut t, None, ctx);
                    aggPut(ctx, og);
                    return;
                }
                jsonStringTrimOneChar(&mut t); /* Remove zero terminator */
                tmp = Some(t);
            }
            let ps: &mut JsonString = match tmp.as_mut() {
                Some(t) => t,
                None => &mut og,
            };
            /* Fix up the string by changing the initial "@" flag back to
             ** to "{" and removing all subsequence "@" entries, with their
             ** associated comma delimeters. */
            let z = &mut ps.zBuf;
            let nUsed = z.len();
            z[0] = b'{';
            let mut inStr = false;
            let mut i = 1usize;
            let mut j = 1usize;
            while i < nUsed {
                let c = z[i];
                if c == b'"' {
                    inStr = !inStr;
                    z[j] = b'"';
                    j += 1;
                } else if c == b'\\' {
                    z[j] = b'\\';
                    j += 1;
                    i += 1;
                    let c2 = at(z, i);
                    if j < z.len() {
                        z[j] = c2;
                    }
                    j += 1;
                } else if c == b'@' && !inStr {
                    debug_assert!(i + 1 < nUsed);
                    if at(z, i + 1) == b',' {
                        i += 1;
                    } else if z[j - 1] == b',' {
                        j -= 1;
                    }
                } else {
                    z[j] = c;
                    j += 1;
                }
                i += 1;
            }
            z.truncate(j); /* Truncate the string */
        }
        let ps: &mut JsonString = match tmp.as_mut() {
            Some(t) => t,
            None => &mut og,
        };
        if flags & JSON_BLOB != 0 {
            jsonReturnStringAsBlob(ps, ctx);
            if !isFinal {
                jsonStringTrimOneChar(&mut og);
            }
        } else if isFinal {
            sqlite3_result_text(ctx, Some(&ps.zBuf), ps.zBuf.len() as i32);
            ps.bStatic = true;
        } else {
            sqlite3_result_text(ctx, Some(&ps.zBuf), ps.zBuf.len() as i32);
            jsonStringTrimOneChar(&mut og);
        }
        aggPut(ctx, og);
    } else if flags & JSON_BLOB != 0 {
        sqlite3_result_blob(ctx, &[0x0c], 1);
    } else {
        sqlite3_result_text(ctx, Some(b"{}"), 2);
    }
    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
}
fn jsonObjectValue(ctx: &mut Context) {
    jsonObjectCompute(ctx, false);
}
fn jsonObjectFinal(ctx: &mut Context) {
    jsonObjectCompute(ctx, true);
}

/****************************************************************************
** The json_each virtual table
****************************************************************************/

#[derive(Clone, Copy, Default)]
struct JsonParent {
    /// Start of object or array
    iHead: u32,
    /// Start of the value
    iValue: u32,
    /// First byte past the end
    iEnd: u32,
    /// Length of path
    nPath: u32,
    /// Key for JSONB_ARRAY
    iKey: i64,
}

/* Column numbers */
const JEACH_KEY: i32 = 0;
const JEACH_VALUE: i32 = 1;
const JEACH_TYPE: i32 = 2;
const JEACH_ATOM: i32 = 3;
const JEACH_ID: i32 = 4;
const JEACH_PARENT: i32 = 5;
const JEACH_FULLKEY: i32 = 6;
const JEACH_PATH: i32 = 7;
/* The xBestIndex method assumes that the JSON and ROOT columns are the
** last two columns in the table.  Should this ever changes, be sure to
** update the xBestIndex method. */
const JEACH_JSON: i32 = 8;
const JEACH_ROOT: i32 = 9;

/// Shared slot through which a cursor reports an error message to its
/// virtual table (C cur->pVtab->zErrMsg).
type ErrSlot = Rc<RefCell<Option<ZStr>>>;

/// A json_each / json_tree cursor (C JsonEachCursor).
struct JsonEachCursor {
    /// The rowid
    iRowid: u32,
    /// Index in sParse.aBlob[] of current row
    i: u32,
    /// EOF when i equals or exceeds this value
    iEnd: u32,
    /// Size of the root path in bytes
    nRoot: u32,
    /// Type of the container for element i
    eType: u8,
    /// True for json_tree().  False for json_each()
    bRecursive: bool,
    /// 1 for json_each().  2 for jsonb_each()
    eMode: u8,
    /// Current nesting depth
    nParent: u32,
    /// Parent elements of i (length = C nParentAlloc)
    aParent: Vec<JsonParent>,
    /// Current path
    path: JsonString,
    /// Parse of the input JSON
    sParse: JsonParse,
    /// cur->pVtab->zErrMsg
    zErrMsg: ErrSlot,
}

/// The json_each virtual table (C JsonEachConnection).
struct JsonEachConnection {
    /// 1 for json_each().  2 for jsonb_each()
    eMode: u8,
    /// True for json_tree().  False for json_each()
    bRecursive: bool,
    zErrMsg: Option<ZStr>,
    /// Error messages set by the cursors
    pending: ErrSlot,
}

/// The json_each / json_tree / jsonb_each / jsonb_tree module.
pub struct JsonEachModule;

impl VtabModule for JsonEachModule {
    fn iVersion(&self) -> i32 {
        0
    }
    /// Constructor for the json_each virtual table
    fn xConnect(
        &self,
        db: &mut Sqlite3,
        argv: &[&[u8]],
        _pzErr: &mut Option<ZStr>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let rc = crate::vtab::sqlite3_declare_vtab(
            db,
            Some(b"CREATE TABLE x(key,value,type,atom,id,parent,fullkey,path,json HIDDEN,root HIDDEN)"),
        );
        if rc != SQLITE_OK {
            return Err(rc);
        }
        crate::vtab::sqlite3_vtab_config(db, SQLITE_VTAB_INNOCUOUS, &[]);
        let z0 = argv.first().copied().unwrap_or(b"");
        let eMode: u8 = if at(z0, 4) == b'b' { 2 } else { 1 };
        let bRecursive = at(z0, 4 + eMode as usize) == b't';
        Ok(Box::new(JsonEachConnection {
            eMode,
            bRecursive,
            zErrMsg: None,
            pending: Rc::new(RefCell::new(None)),
        }))
    }
    fn has_xCreate(&self) -> bool {
        false
    }
    fn create_is_connect(&self) -> bool {
        false
    }
}

impl Vtab for JsonEachConnection {
    /// The query strategy is to look for an equality constraint on the
    /// json column.  Without such a constraint, the table cannot operate.
    /// idxNum is 1 if the constraint is found, 3 if the constraint and
    /// zRoot are found, and 0 otherwise.
    fn xBestIndex(&mut self, _db: &mut Sqlite3, pIdxInfo: &mut IndexInfo) -> i32 {
        const _: () = assert!(JEACH_ROOT == JEACH_JSON + 1);
        let mut aIdx: [i32; 2] = [-1, -1];
        let mut unusableMask = 0i32;
        let mut idxMask = 0i32;
        for (i, pConstraint) in pIdxInfo.aConstraint.iter().enumerate() {
            if pConstraint.iColumn < JEACH_JSON {
                continue;
            }
            let iCol = pConstraint.iColumn - JEACH_JSON;
            debug_assert!(iCol == 0 || iCol == 1);
            let iMask = 1 << iCol;
            if pConstraint.usable == 0 {
                unusableMask |= iMask;
            } else if pConstraint.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                aIdx[iCol as usize] = i as i32;
                idxMask |= iMask;
            }
        }
        if !pIdxInfo.aOrderBy.is_empty()
            && pIdxInfo.aOrderBy[0].iColumn < 0
            && pIdxInfo.aOrderBy[0].desc == 0
        {
            pIdxInfo.orderByConsumed = 1;
        }
        if (unusableMask & !idxMask) != 0 {
            /* If there are any unusable constraints on JSON or ROOT, then
             ** reject this entire plan */
            return SQLITE_CONSTRAINT;
        }
        if aIdx[0] < 0 {
            /* No JSON input.  Leave estimatedCost at the huge value that it
             ** was initialized to to discourage the query planner from
             ** selecting this plan. */
            pIdxInfo.idxNum = 0;
        } else {
            pIdxInfo.estimatedCost = 1.0;
            let i = aIdx[0] as usize;
            pIdxInfo.aConstraintUsage[i].argvIndex = 1;
            pIdxInfo.aConstraintUsage[i].omit = 1;
            if aIdx[1] < 0 {
                pIdxInfo.idxNum = 1; /* Only JSON supplied.  Plan 1 */
            } else {
                let i = aIdx[1] as usize;
                pIdxInfo.aConstraintUsage[i].argvIndex = 2;
                pIdxInfo.aConstraintUsage[i].omit = 1;
                pIdxInfo.idxNum = 3; /* Both JSON and ROOT are supplied.  Plan 3 */
            }
        }
        SQLITE_OK
    }

    /// constructor for a JsonEachCursor object for json_each()/json_tree().
    fn xOpen(&mut self, _db: &mut Sqlite3) -> Result<Box<dyn VtabCursor>, i32> {
        let mut path = JsonString::default();
        jsonStringZero(&mut path);
        Ok(Box::new(JsonEachCursor {
            iRowid: 0,
            i: 0,
            iEnd: 0,
            nRoot: 0,
            eType: 0,
            bRecursive: self.bRecursive,
            eMode: self.eMode,
            nParent: 0,
            aParent: Vec::new(),
            path,
            sParse: JsonParse::default(),
            zErrMsg: self.pending.clone(),
        }))
    }

    fn zErrMsg(&mut self) -> &mut Option<ZStr> {
        if let Some(m) = self.pending.borrow_mut().take() {
            self.zErrMsg = Some(m);
        }
        &mut self.zErrMsg
    }
}

impl JsonEachCursor {
    /// Reset a JsonEachCursor back to its original state.
    fn reset(&mut self) {
        jsonParseReset(&mut self.sParse);
        self.sParse = JsonParse::default();
        jsonStringReset(&mut self.path);
        self.aParent = Vec::new();
        self.iRowid = 0;
        self.i = 0;
        self.nParent = 0;
        self.iEnd = 0;
        self.eType = 0;
    }

    /// If the cursor is currently pointing at the label of a object entry,
    /// then return the index of the value.  For all other cases, return
    /// the current pointer position, which is the value.
    fn jsonSkipLabel(&self) -> u32 {
        if self.eType == JSONB_OBJECT {
            let (n, sz) = jsonbPayloadSize(&self.sParse, self.i);
            let mut sz = sz + self.i + n;
            if sz >= self.sParse.nBlob {
                sz = self.i;
            }
            sz
        } else {
            self.i
        }
    }

    /// Append the path name for the current element.
    fn jsonAppendPathName(&mut self) {
        debug_assert!(self.nParent > 0);
        debug_assert!(self.eType == JSONB_ARRAY || self.eType == JSONB_OBJECT);
        if self.eType == JSONB_ARRAY {
            let k = self.aParent[(self.nParent - 1) as usize].iKey;
            jsonPrintf(30, &mut self.path, b"[%lld]", &[PfArg::from(k)]);
        } else {
            let (n, sz) = jsonbPayloadSize(&self.sParse, self.i);
            let k = self.i + n;
            let z: Vec<u8> = sub(&self.sParse.aBlob, k as usize, sz as usize).to_vec();
            let mut needQuote = false;
            if sz == 0 || !sqlite3Isalpha(at(&z, 0)) {
                needQuote = true;
            } else {
                for i in 0..sz as usize {
                    if !sqlite3Isalnum(at(&z, i)) {
                        needQuote = true;
                        break;
                    }
                }
            }
            if needQuote {
                jsonPrintf(
                    sz + 4,
                    &mut self.path,
                    b".\"%.*s\"",
                    &[PfArg::from(sz as i32), PfArg::from(&z[..])],
                );
            } else {
                jsonPrintf(
                    sz + 2,
                    &mut self.path,
                    b".%.*s",
                    &[PfArg::from(sz as i32), PfArg::from(&z[..])],
                );
            }
        }
    }

    /// Report a "malformed JSON" or OOM error against the cursor.
    fn jsonEachMalformedInput(&mut self) -> i32 {
        *self.zErrMsg.borrow_mut() = Some(b"malformed JSON".to_vec());
        self.reset();
        SQLITE_ERROR
    }

    /// Length of the path for rowid==0 in bRecursive mode.
    fn jsonEachPathLength(&mut self) -> u32 {
        let mut n = self.path.zBuf.len() as u32;
        if self.iRowid == 0 && self.bRecursive && n >= 2 {
            while n > 1 {
                n -= 1;
                let c = self.path.zBuf[n as usize];
                if c == b'[' || c == b'.' {
                    debug_assert!(self.sParse.eEdit == 0);
                    let z = &self.path.zBuf[..n as usize];
                    let x = jsonLookupStep(&mut self.sParse, 0, z, 1, 0);
                    if JSON_LOOKUP_ISERROR(x) {
                        continue;
                    }
                    if x.wrapping_add(jsonbPayloadSize(&self.sParse, x).0) == self.i {
                        break;
                    }
                }
            }
        }
        n
    }
}

impl VtabCursor for JsonEachCursor {
    /// Start a search on a new JSON string
    fn xFilter(
        &mut self,
        _db: &mut Sqlite3,
        idxNum: i32,
        _idxStr: Option<&[u8]>,
        argv: &mut [Mem],
    ) -> i32 {
        self.reset();
        if idxNum == 0 {
            return SQLITE_OK;
        }
        self.sParse = JsonParse::default();
        if jsonArgIsJsonb(&mut argv[0], &mut self.sParse) {
            /* We have JSONB */
        } else {
            let Some(z) = value_text(&mut argv[0]) else {
                self.i = 0;
                self.iEnd = 0;
                return SQLITE_OK;
            };
            self.sParse.nJson = sqlite3_value_bytes(&mut argv[0]);
            self.sParse.zJson = Some(Rc::from(&z[..]));
            if jsonConvertTextToBlob(&mut self.sParse, None) != 0 {
                if self.sParse.oom != 0 {
                    return SQLITE_NOMEM;
                }
                return self.jsonEachMalformedInput();
            }
        }
        let i: u32;
        if idxNum == 3 {
            let Some(zRoot) = value_text(&mut argv[1]) else {
                return SQLITE_OK;
            };
            let zRoot = cstr(&zRoot).to_vec();
            if at(&zRoot, 0) != b'$' {
                *self.zErrMsg.borrow_mut() = jsonBadPathError(None, &zRoot, 0);
                self.reset();
                return SQLITE_ERROR;
            }
            self.nRoot = zRoot.len() as u32;
            if at(&zRoot, 1) == 0 {
                i = 0;
                self.i = 0;
                self.eType = 0;
            } else {
                i = jsonLookupStep(&mut self.sParse, 0, &zRoot, 1, 0);
                if JSON_LOOKUP_ISERROR(i) {
                    if i == JSON_LOOKUP_NOTFOUND {
                        self.i = 0;
                        self.eType = 0;
                        self.iEnd = 0;
                        return SQLITE_OK;
                    }
                    *self.zErrMsg.borrow_mut() = jsonBadPathError(None, &zRoot, 0);
                    self.reset();
                    return SQLITE_ERROR;
                }
                if self.sParse.iLabel != 0 {
                    self.i = self.sParse.iLabel;
                    self.eType = JSONB_OBJECT;
                } else {
                    self.i = i;
                    self.eType = JSONB_ARRAY;
                }
            }
            jsonAppendRaw(&mut self.path, &zRoot);
        } else {
            i = 0;
            self.i = 0;
            self.eType = 0;
            self.nRoot = 1;
            jsonAppendRaw(&mut self.path, b"$");
        }
        self.nParent = 0;
        let (n, sz) = jsonbPayloadSize(&self.sParse, i);
        self.iEnd = i + n + sz;
        if (self.sParse.ab(i) & 0x0f) >= JSONB_ARRAY && !self.bRecursive {
            self.i = i + n;
            self.eType = self.sParse.ab(i) & 0x0f;
            self.aParent = vec![JsonParent {
                iKey: 0,
                iEnd: self.iEnd,
                iHead: self.i,
                iValue: i,
                nPath: 0,
            }];
            self.nParent = 1;
        }
        SQLITE_OK
    }

    /// Advance the cursor to the next element for json_tree()
    fn xNext(&mut self, _db: &mut Sqlite3) -> i32 {
        let mut rc = SQLITE_OK;
        if self.bRecursive {
            let mut levelChange = false;
            let i = self.jsonSkipLabel();
            let x = self.sParse.ab(i) & 0x0f;
            let (n, sz) = jsonbPayloadSize(&self.sParse, i);
            if n == 0 {
                return self.jsonEachMalformedInput();
            }
            if x == JSONB_OBJECT || x == JSONB_ARRAY {
                if self.nParent as usize >= self.aParent.len() {
                    let nNew = self.aParent.len() * 2 + 3;
                    self.aParent.resize(nNew, JsonParent::default());
                }
                levelChange = true;
                let np = self.path.zBuf.len() as u32;
                let iHead = self.i;
                {
                    let pParent = &mut self.aParent[self.nParent as usize];
                    pParent.iHead = iHead;
                    pParent.iValue = i;
                    pParent.iEnd = i + n + sz;
                    pParent.iKey = -1;
                    pParent.nPath = np;
                }
                if self.eType != 0 && self.nParent != 0 {
                    self.jsonAppendPathName();
                    if self.path.eErr != 0 {
                        rc = SQLITE_NOMEM;
                    }
                }
                self.nParent += 1;
                self.i = i + n;
            } else {
                self.i = i + n + sz;
            }
            while self.nParent > 0 && self.i >= self.aParent[(self.nParent - 1) as usize].iEnd {
                self.nParent -= 1;
                let np = self.aParent[self.nParent as usize].nPath as usize;
                self.path.zBuf.truncate(np);
                levelChange = true;
            }
            if levelChange {
                if self.nParent > 0 {
                    let iVal = self.aParent[(self.nParent - 1) as usize].iValue;
                    self.eType = self.sParse.ab(iVal) & 0x0f;
                } else {
                    self.eType = 0;
                }
            }
        } else {
            let i = self.jsonSkipLabel();
            let (n, sz) = jsonbPayloadSize(&self.sParse, i);
            if n == 0 {
                return self.jsonEachMalformedInput();
            }
            self.i = i + n + sz;
        }
        if self.eType == JSONB_ARRAY && self.nParent != 0 {
            self.aParent[(self.nParent - 1) as usize].iKey += 1;
        }
        self.iRowid = self.iRowid.wrapping_add(1);
        rc
    }

    /// Return TRUE if the jsonEachCursor object has been advanced off the
    /// end of the JSON object
    fn xEof(&mut self) -> bool {
        self.i >= self.iEnd
    }

    /// Return the value of a column
    fn xColumn(&mut self, ctx: &mut Context, iColumn: i32) -> i32 {
        match iColumn {
            JEACH_KEY => {
                if self.nParent == 0 {
                    if self.nRoot == 1 {
                        return SQLITE_OK;
                    }
                    let j = self.jsonEachPathLength();
                    let n = self.nRoot.wrapping_sub(j);
                    let z = &self.path.zBuf;
                    if n == 0 {
                        return SQLITE_OK;
                    } else if at(z, j as usize) == b'[' {
                        let mut x: i64 = 0;
                        sqlite3Atoi64(
                            Some(tail(z, j as usize + 1)),
                            &mut x,
                            n as i32 - 1,
                            SQLITE_UTF8,
                        );
                        sqlite3_result_int64(ctx, x);
                    } else if at(z, j as usize + 1) == b'"' {
                        let t = sub(z, j as usize + 2, (n as i32 - 3).max(0) as usize).to_vec();
                        sqlite3_result_text(ctx, Some(&t), t.len() as i32);
                    } else {
                        let t = sub(z, j as usize + 1, (n as i32 - 1).max(0) as usize).to_vec();
                        sqlite3_result_text(ctx, Some(&t), t.len() as i32);
                    }
                    return SQLITE_OK;
                }
                if self.eType == JSONB_OBJECT {
                    let i = self.i;
                    jsonReturnFromBlob(&mut self.sParse, i, ctx, 1);
                } else {
                    debug_assert!(self.eType == JSONB_ARRAY);
                    sqlite3_result_int64(ctx, self.aParent[(self.nParent - 1) as usize].iKey);
                }
            }
            JEACH_VALUE => {
                let i = self.jsonSkipLabel();
                let eMode = self.eMode as i32;
                jsonReturnFromBlob(&mut self.sParse, i, ctx, eMode);
                if (self.sParse.ab(i) & 0x0f) >= JSONB_ARRAY {
                    sqlite3_result_subtype(ctx, JSON_SUBTYPE);
                }
            }
            JEACH_TYPE => {
                let i = self.jsonSkipLabel();
                let eType = self.sParse.ab(i) & 0x0f;
                let t = jsonbType[eType as usize];
                sqlite3_result_text(ctx, Some(t), t.len() as i32);
            }
            JEACH_ATOM => {
                let i = self.jsonSkipLabel();
                if (self.sParse.ab(i) & 0x0f) < JSONB_ARRAY {
                    jsonReturnFromBlob(&mut self.sParse, i, ctx, 1);
                }
            }
            JEACH_ID => {
                sqlite3_result_int64(ctx, self.i as i64);
            }
            JEACH_PARENT => {
                if self.nParent > 0 && self.bRecursive {
                    sqlite3_result_int64(
                        ctx,
                        self.aParent[(self.nParent - 1) as usize].iHead as i64,
                    );
                }
            }
            JEACH_FULLKEY => {
                let nBase = self.path.zBuf.len();
                if self.nParent != 0 {
                    self.jsonAppendPathName();
                }
                let z = self.path.zBuf.clone();
                sqlite3_result_text64(ctx, Some(&z), z.len() as u64, SQLITE_UTF8);
                self.path.zBuf.truncate(nBase);
            }
            JEACH_PATH => {
                let n = self.jsonEachPathLength();
                let z = sub(&self.path.zBuf, 0, n as usize).to_vec();
                sqlite3_result_text64(ctx, Some(&z), n as u64, SQLITE_UTF8);
            }
            JEACH_JSON => match &self.sParse.zJson {
                None => {
                    let nb = self.sParse.nBlob as usize;
                    let b = sub(&self.sParse.aBlob, 0, nb).to_vec();
                    sqlite3_result_blob(ctx, &b, nb as i32);
                }
                Some(z) => {
                    let t = cstr(z).to_vec();
                    sqlite3_result_text(ctx, Some(&t), t.len() as i32);
                }
            },
            _ => {
                /* JEACH_ROOT */
                let t = sub(&self.path.zBuf, 0, self.nRoot as usize).to_vec();
                sqlite3_result_text(ctx, Some(&t), self.nRoot as i32);
            }
        }
        SQLITE_OK
    }

    /// Return the current rowid value
    fn xRowid(&mut self, pRowid: &mut i64) -> i32 {
        *pRowid = self.iRowid as i64;
        SQLITE_OK
    }

    /// Destructor for a jsonEachCursor object
    fn xClose(&mut self) -> i32 {
        self.reset();
        SQLITE_OK
    }
}

/****************************************************************************
** Registration
****************************************************************************/

/// JFUNCTION(zName, nArg, bUseCache, bWS, bRS, bJsonB, iArg, xFunc)
#[allow(clippy::too_many_arguments)]
fn JFUNCTION(
    zName: &str,
    nArg: i32,
    bUseCache: u32,
    bWS: u32,
    bRS: u32,
    bJsonB: i64,
    iArg: i32,
    xFunc: fn(&mut Context, &mut [Mem]),
) -> FuncDef {
    let ud = iArg as i64 | (bJsonB * JSON_BLOB as i64);
    FuncDef {
        nArg: nArg as i16,
        funcFlags: SQLITE_FUNC_BUILTIN
            | SQLITE_DETERMINISTIC
            | SQLITE_FUNC_CONSTANT
            | SQLITE_UTF8 as u32
            | (bUseCache * SQLITE_FUNC_RUNONLY)
            | (bRS * SQLITE_SUBTYPE)
            | (bWS * SQLITE_RESULT_SUBTYPE),
        pUserData: if ud == 0 {
            FuncUserData::None
        } else {
            FuncUserData::Int(ud)
        },
        xSFunc: Some(Rc::new(xFunc) as XSFunc),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: zName.as_bytes().to_vec(),
        pDestructor: None,
    }
}

/// The FuncDef objects of the C aJsonFunc[] array, in array order.
pub fn sqlite3JsonFunctionDefs() -> Vec<FuncDef> {
    use crate::func::WAGGREGATE;
    let aggFlags =
        SQLITE_SUBTYPE | SQLITE_RESULT_SUBTYPE | SQLITE_UTF8 as u32 | SQLITE_DETERMINISTIC;
    vec![
        /*   sqlite3_result_subtype() ----,  ,--- sqlite3_value_subtype()       */
        /*                                |  |                                  */
        /*             Uses cache ------, |  | ,---- Returns JSONB              */
        /*                              | |  | |                                */
        /*     Number of arguments ---, | |  | | ,--- Flags                     */
        JFUNCTION("json", 1, 1, 1, 0, 0, 0, jsonRemoveFunc),
        JFUNCTION("jsonb", 1, 1, 0, 0, 1, 0, jsonRemoveFunc),
        JFUNCTION("json_array", -1, 0, 1, 1, 0, 0, jsonArrayFunc),
        JFUNCTION("jsonb_array", -1, 0, 1, 1, 1, 0, jsonArrayFunc),
        JFUNCTION("json_array_insert", -1, 1, 1, 1, 0, JSON_AINS, jsonSetFunc),
        JFUNCTION("jsonb_array_insert", -1, 1, 0, 1, 1, JSON_AINS, jsonSetFunc),
        JFUNCTION("json_array_length", 1, 1, 0, 0, 0, 0, jsonArrayLengthFunc),
        JFUNCTION("json_array_length", 2, 1, 0, 0, 0, 0, jsonArrayLengthFunc),
        JFUNCTION("json_error_position", 1, 1, 0, 0, 0, 0, jsonErrorFunc),
        JFUNCTION("json_extract", -1, 1, 1, 0, 0, 0, jsonExtractFunc),
        JFUNCTION("jsonb_extract", -1, 1, 0, 0, 1, 0, jsonExtractFunc),
        JFUNCTION("->", 2, 1, 1, 0, 0, JSON_JSON, jsonExtractFunc),
        JFUNCTION("->>", 2, 1, 0, 0, 0, JSON_SQL, jsonExtractFunc),
        JFUNCTION("json_insert", -1, 1, 1, 1, 0, 0, jsonSetFunc),
        JFUNCTION("jsonb_insert", -1, 1, 0, 1, 1, 0, jsonSetFunc),
        JFUNCTION("json_object", -1, 0, 1, 1, 0, 0, jsonObjectFunc),
        JFUNCTION("jsonb_object", -1, 0, 1, 1, 1, 0, jsonObjectFunc),
        JFUNCTION("json_patch", 2, 1, 1, 0, 0, 0, jsonPatchFunc),
        JFUNCTION("jsonb_patch", 2, 1, 0, 0, 1, 0, jsonPatchFunc),
        JFUNCTION("json_pretty", 1, 1, 0, 0, 0, 0, jsonPrettyFunc),
        JFUNCTION("json_pretty", 2, 1, 0, 0, 0, 0, jsonPrettyFunc),
        JFUNCTION("json_quote", 1, 0, 1, 1, 0, 0, jsonQuoteFunc),
        JFUNCTION("json_remove", -1, 1, 1, 0, 0, 0, jsonRemoveFunc),
        JFUNCTION("jsonb_remove", -1, 1, 0, 0, 1, 0, jsonRemoveFunc),
        JFUNCTION("json_replace", -1, 1, 1, 1, 0, 0, jsonReplaceFunc),
        JFUNCTION("jsonb_replace", -1, 1, 0, 1, 1, 0, jsonReplaceFunc),
        JFUNCTION("json_set", -1, 1, 1, 1, 0, JSON_ISSET, jsonSetFunc),
        JFUNCTION("jsonb_set", -1, 1, 0, 1, 1, JSON_ISSET, jsonSetFunc),
        JFUNCTION("json_type", 1, 1, 0, 0, 0, 0, jsonTypeFunc),
        JFUNCTION("json_type", 2, 1, 0, 0, 0, 0, jsonTypeFunc),
        JFUNCTION("json_valid", 1, 1, 0, 0, 0, 0, jsonValidFunc),
        JFUNCTION("json_valid", 2, 1, 0, 0, 0, 0, jsonValidFunc),
        WAGGREGATE(
            "json_group_array",
            1,
            0,
            0,
            jsonArrayStep,
            jsonArrayFinal,
            Some(jsonArrayValue),
            Some(jsonGroupInverse),
            aggFlags,
        ),
        WAGGREGATE(
            "jsonb_group_array",
            1,
            JSON_BLOB as i64,
            0,
            jsonArrayStep,
            jsonArrayFinal,
            Some(jsonArrayValue),
            Some(jsonGroupInverse),
            aggFlags,
        ),
        WAGGREGATE(
            "json_group_object",
            2,
            0,
            0,
            jsonObjectStep,
            jsonObjectFinal,
            Some(jsonObjectValue),
            Some(jsonGroupInverse),
            aggFlags,
        ),
        WAGGREGATE(
            "jsonb_group_object",
            2,
            JSON_BLOB as i64,
            0,
            jsonObjectStep,
            jsonObjectFinal,
            Some(jsonObjectValue),
            Some(jsonGroupInverse),
            aggFlags,
        ),
    ]
}

/// C: `void sqlite3RegisterJsonFunctions(void)`
///
/// Rust: inserts into the global hash table passed explicitly (like
/// sqlite3RegisterDateTimeFunctions).
pub fn sqlite3RegisterJsonFunctions(pHash: &mut crate::callback::FuncDefHash) {
    crate::callback::sqlite3InsertBuiltinFuncs(pHash, &sqlite3JsonFunctionDefs());
}

/// The JSON table-valued functions.
static azModule: [&[u8]; 4] = [b"json_each", b"json_tree", b"jsonb_each", b"jsonb_tree"];

/// C: `Module * sqlite3JsonVtabRegister(sqlite3 *db, const char *zName)`
///
/// Register the JSON table-valued function named zName and return its
/// module.  Rust: returns the canonical (lower case) module name and the
/// module object; the caller (sqlite3LocateTable, build.c) registers it
/// with sqlite3VtabCreateModule(db, name, module) and uses the resulting
/// Module.  None if zName is not one of the JSON table-valued functions.
pub fn sqlite3JsonVtabRegister(
    db: &mut Sqlite3,
    zName: &[u8],
) -> Option<(&'static [u8], Rc<dyn VtabModule>)> {
    let _ = db;
    for m in azModule.iter() {
        if sqlite3StrICmp(Some(m), Some(zName)) == 0 {
            return Some((m, Rc::new(JsonEachModule)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(z: &str) -> Option<JsonParse> {
        let mut p = JsonParse {
            zJson: Some(Rc::from(z.as_bytes())),
            nJson: z.len() as i32,
            ..Default::default()
        };
        if jsonConvertTextToBlob(&mut p, None) != 0 {
            return None;
        }
        Some(p)
    }

    #[test]
    fn text_to_blob() {
        let p = parse("[1,2]").unwrap();
        assert_eq!(
            &p.aBlob[..p.nBlob as usize],
            &[0x4b, 0x13, b'1', 0x13, b'2']
        );
        let p = parse("{\"a\":null}").unwrap();
        assert_eq!(&p.aBlob[..p.nBlob as usize], &[0x3c, 0x17, b'a', 0x00]);
        assert!(parse("[1,").is_none());
        let p = parse("{a:0x1F, b:.5, c:'x'}").unwrap();
        assert_eq!(p.hasNonstd, 1);
        assert_eq!(jsonbValidityCheck(&p, 0, p.nBlob, 1), 0);
    }

    #[test]
    fn whitespace() {
        assert_eq!(json5Whitespace(b"  /* c */ x"), 10);
        assert_eq!(json5Whitespace(b"// c\nx"), 5);
        assert_eq!(json5Whitespace("\u{a0}x".as_bytes()), 2);
    }

    #[test]
    fn unescape() {
        let mut c = 0;
        assert_eq!(jsonUnescapeOneChar(b"\\u0041", 6, &mut c), 6);
        assert_eq!(c, 0x41);
        assert_eq!(jsonUnescapeOneChar(b"\\ud83d\\ude00", 12, &mut c), 12);
        assert_eq!(c, 0x1f600);
        assert_eq!(jsonUnescapeOneChar(b"\\x4a", 4, &mut c), 4);
        assert_eq!(c, 0x4a);
    }
}
