//! Port of vdbeapi.c: the public API of prepared statements, values and
//! function contexts.
//!
//! Prepared statements are designated by `StmtId` handles (slots in
//! `Sqlite3::aVdbe`).  A stale handle (finalized statement) is reported as
//! SQLITE_MISUSE where C would detect a finalized statement (and simply
//! yields NULL/0 values elsewhere, where C would have undefined behaviour).
//! Functions that need both the connection and the statement take the
//! statement out of its slot for the duration of the call
//! (`Sqlite3::with_stmt`).
//!
//! sqlite3_value_* functions that take only a Mem behave like C on a Mem
//! whose `db` is NULL: double->text conversions use 17 significant digits
//! and no UTF-8/UTF-16 translation is possible.  The sqlite3_column_*
//! functions use the connection (nFpDigit, encoding).

use crate::int::*;
use crate::util::sqlite3Error;
use crate::vdbemem::*;
use std::any::Any;
use std::rc::Rc;

/// sqlite3ApiExit() (malloc.c): this routine must be called before
/// exiting any API function that might have encountered an OOM.  There is
/// no OOM in this port, so it reduces to `rc & db->errMask`.
pub fn sqlite3ApiExit(db: &mut Sqlite3, rc: i32) -> i32 {
    if db.mallocFailed != 0 || rc != 0 {
        /* apiHandleError() */
        if db.mallocFailed != 0 || rc == SQLITE_IOERR_NOMEM {
            db.mallocFailed = 0;
            sqlite3Error(db, SQLITE_NOMEM);
            return SQLITE_NOMEM;
        }
        return rc & db.errMask;
    }
    0
}

impl<'a> Context<'a> {
    /// A function context with every optional member unset (the C
    /// `memset(&ctx, 0, sizeof(ctx))` followed by pOut/pFunc/enc).
    pub fn new(db: &'a mut Sqlite3, pOut: &'a mut Mem, pFunc: FuncDefRef) -> Context<'a> {
        let enc = db.ENC();
        Context {
            db,
            pOut,
            pFunc,
            pMem: None,
            pAuxData: None,
            iOp: 0,
            pOpInfo: None,
            isError: 0,
            enc,
            skipFlag: 0,
            argc: 0,
            zErr: None,
            pColl: None,
            piCurrentTime: None,
        }
    }
}

/// The current time as a Julian Day number times 86400000 (the unix VFS
/// xCurrentTimeInt64 method).
fn currentTimeInt64() -> i64 {
    const unixEpoch: i64 = 24405875i64 * 8640000i64;
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => unixEpoch + 1000 * d.as_secs() as i64 + (d.subsec_micros() / 1000) as i64,
        Err(_) => unixEpoch,
    }
}

/// C: `int sqlite3_expired(sqlite3_stmt *pStmt)`
///
/// Return TRUE (non-zero) of the statement supplied as an argument needs to
/// be recompiled.
pub fn sqlite3_expired(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    if !db.stmt_valid(pStmt) {
        return 1;
    }
    let s = &db.aVdbe[pStmt.idx as usize];
    match &s.v {
        Some(v) => v.expired.max(s.expired) as i32,
        None => s.expired as i32,
    }
}

/// vdbeSafetyNotNull(): log the misuse of a finalized statement.
fn vdbeSafetyMisuse() -> i32 {
    crate::printf::sqlite3_log(
        SQLITE_MISUSE,
        b"API called with finalized prepared statement",
        &[],
    );
    SQLITE_MISUSE
}

/// Invoke the profile callback.  This routine is only called if we already
/// know that the profile callback is defined and needs to be invoked.
/// Rust: the trace callback receives the SQL text (the elapsed time is not
/// passed).
fn invokeProfileCallback(db: &mut Sqlite3, p: &mut Vdbe) {
    let iNow = currentTimeInt64();
    let _iElapse = (iNow - p.startTime) * 1000000;
    if (db.mTrace as u32) & SQLITE_TRACE_PROFILE != 0 {
        if let Some(x) = db.xTrace.clone() {
            let z = p.zSql.clone().unwrap_or_default();
            x(SQLITE_TRACE_PROFILE, &z);
        }
    }
    p.startTime = 0;
}

/// The checkProfileCallback(DB,P) macro.
#[inline]
fn checkProfileCallback(db: &mut Sqlite3, p: &mut Vdbe) {
    if p.startTime > 0 {
        invokeProfileCallback(db, p);
    }
}

/// C: `int sqlite3_finalize(sqlite3_stmt *pStmt)`
///
/// Destroy a prepared statement.  The integer returned is an SQLITE_
/// success/failure code that describes the result of executing the
/// virtual machine.
pub fn sqlite3_finalize(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    let mut v = match db.stmt_take(pStmt) {
        Some(v) => v,
        None => return vdbeSafetyMisuse(),
    };
    checkProfileCallback(db, &mut v);
    debug_assert!(v.eVdbeState >= VDBE_READY_STATE);
    let mut rc = crate::vdbeaux::sqlite3VdbeReset(db, &mut v);
    db.stmt_remove(pStmt);
    crate::vdbeaux::sqlite3VdbeDelete(db, &mut v);
    drop(v);
    rc = sqlite3ApiExit(db, rc);
    crate::mainapi::sqlite3LeaveMutexAndCloseZombie(db);
    rc
}

/// sqlite3_reset() on a statement taken out of its slot.
pub fn vdbeResetStmt(db: &mut Sqlite3, v: &mut Vdbe) -> i32 {
    checkProfileCallback(db, v);
    let mut rc = crate::vdbeaux::sqlite3VdbeReset(db, v);
    crate::vdbeaux::sqlite3VdbeRewind(db, v);
    debug_assert!((rc & db.errMask) == rc);
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_reset(sqlite3_stmt *pStmt)`
///
/// Terminate the current execution of an SQL statement and reset it back
/// to its starting state so that it can be reused.  A success code from
/// the prior execution is returned.
pub fn sqlite3_reset(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    match db.with_stmt(pStmt, |db, v| vdbeResetStmt(db, v)) {
        Some(rc) => rc,
        None => vdbeSafetyMisuse(),
    }
}

/// C: `int sqlite3_clear_bindings(sqlite3_stmt *pStmt)`
///
/// Set all the parameters in the compiled SQL statement to NULL.
pub fn sqlite3_clear_bindings(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    match db.with_stmt(pStmt, |_db, p| {
        for m in p.aVar.iter_mut() {
            sqlite3VdbeMemRelease(m);
            m.flags = MEM_Null;
        }
        if p.expmask != 0 {
            p.expired = 1;
        }
        SQLITE_OK
    }) {
        Some(rc) => rc,
        None => SQLITE_MISUSE,
    }
}

/**************************** sqlite3_value_  *******************************
** The following routines extract information from a Mem or sqlite3_value
** structure.
*/

fn valueBlobDb<'a>(db: Option<&mut Sqlite3>, p: &'a mut Mem) -> Option<&'a [u8]> {
    if p.flags & (MEM_Blob | MEM_Str) != 0 {
        if ExpandBlob(p) != SQLITE_OK {
            return None;
        }
        p.flags |= MEM_Blob;
        if p.n != 0 { Some(p.bytes()) } else { None }
    } else {
        valueTextRef(db, p, SQLITE_UTF8)
    }
}

/// C: `const void * sqlite3_value_blob(sqlite3_value *pVal)`
pub fn sqlite3_value_blob(pVal: &mut Mem) -> Option<&[u8]> {
    valueBlobDb(None, pVal)
}

/// C: `int sqlite3_value_bytes(sqlite3_value *pVal)`
pub fn sqlite3_value_bytes(pVal: &mut Mem) -> i32 {
    sqlite3ValueBytes(pVal, SQLITE_UTF8)
}

/// UTF-16 rendering of a value (native = little-endian).  Minimal: the
/// UTF-8 text is converted with the standard library.
fn valueText16(db: Option<&mut Sqlite3>, pVal: &mut Mem, enc: u8) -> Option<Vec<u8>> {
    if (pVal.flags & MEM_Str) != 0 && pVal.enc == enc {
        return Some(pVal.bytes().to_vec());
    }
    let z = valueTextRef(db, pVal, SQLITE_UTF8)?;
    let s = String::from_utf8_lossy(z);
    let mut out = Vec::with_capacity(z.len() * 2);
    for u in s.encode_utf16() {
        if enc == SQLITE_UTF16BE {
            out.extend_from_slice(&u.to_be_bytes());
        } else {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }
    Some(out)
}

fn valueBytes16(db: Option<&mut Sqlite3>, pVal: &mut Mem) -> i32 {
    if (pVal.flags & MEM_Str) != 0 && pVal.enc != SQLITE_UTF8 {
        return pVal.n;
    }
    if (pVal.flags & MEM_Blob) != 0 {
        return sqlite3ValueBytes(pVal, SQLITE_UTF16NATIVE);
    }
    if pVal.flags & MEM_Null != 0 {
        return 0;
    }
    valueText16(db, pVal, SQLITE_UTF16NATIVE)
        .map(|v| v.len() as i32)
        .unwrap_or(0)
}

/// C: `int sqlite3_value_bytes16(sqlite3_value *pVal)`
pub fn sqlite3_value_bytes16(pVal: &mut Mem) -> i32 {
    valueBytes16(None, pVal)
}

/// C: `double sqlite3_value_double(sqlite3_value *pVal)`
pub fn sqlite3_value_double(pVal: &mut Mem) -> f64 {
    memRealValueAny(pVal)
}

/// C: `int sqlite3_value_int(sqlite3_value *pVal)`
pub fn sqlite3_value_int(pVal: &mut Mem) -> i32 {
    memIntValueAny(pVal) as i32
}

/// C: `sqlite_int64 sqlite3_value_int64(sqlite3_value *pVal)`
pub fn sqlite3_value_int64(pVal: &mut Mem) -> i64 {
    memIntValueAny(pVal)
}

/// C: `unsigned int sqlite3_value_subtype(sqlite3_value *pVal)`
pub fn sqlite3_value_subtype(pVal: &mut Mem) -> u32 {
    if pVal.flags & MEM_Subtype != 0 {
        pVal.eSubtype as u32
    } else {
        0
    }
}

/// C: `void * sqlite3_value_pointer(sqlite3_value *pVal, const char *zPType)`
pub fn sqlite3_value_pointer(pVal: &mut Mem, zPType: Option<&[u8]>) -> Option<Rc<dyn Any>> {
    let p = pVal;
    if (p.flags & (MEM_TypeMask | MEM_Term | MEM_Subtype)) == (MEM_Null | MEM_Term | MEM_Subtype)
        && zPType.is_some()
        && p.eSubtype == b'p'
        && p.u.zPType.as_deref().map(|s| s.as_bytes()) == zPType
    {
        p.pPtr.clone()
    } else {
        None
    }
}

/// C: `const unsigned char * sqlite3_value_text(sqlite3_value *pVal)`
pub fn sqlite3_value_text(pVal: &mut Mem) -> Option<&[u8]> {
    valueTextRef(None, pVal, SQLITE_UTF8)
}

/// C: `const void * sqlite3_value_text16(sqlite3_value* pVal)`
pub fn sqlite3_value_text16(pVal: &mut Mem) -> Option<Vec<u8>> {
    valueText16(None, pVal, SQLITE_UTF16NATIVE)
}

/// C: `const void * sqlite3_value_text16be(sqlite3_value *pVal)`
pub fn sqlite3_value_text16be(pVal: &mut Mem) -> Option<Vec<u8>> {
    valueText16(None, pVal, SQLITE_UTF16BE)
}

/// C: `const void * sqlite3_value_text16le(sqlite3_value *pVal)`
pub fn sqlite3_value_text16le(pVal: &mut Mem) -> Option<Vec<u8>> {
    valueText16(None, pVal, SQLITE_UTF16LE)
}

/// EVIDENCE-OF: R-12793-43283 Every value in SQLite has one of five
/// fundamental datatypes.
static aType: [u8; 64] = {
    const B: u8 = SQLITE_BLOB as u8;
    const N: u8 = SQLITE_NULL as u8;
    const T: u8 = SQLITE_TEXT as u8;
    const I: u8 = SQLITE_INTEGER as u8;
    const F: u8 = SQLITE_FLOAT as u8;
    [
        B, N, T, N, I, N, I, N, /* 0x00 - 0x07 */
        F, N, F, N, I, N, I, N, /* 0x08 - 0x0f */
        B, N, T, N, I, N, I, N, /* 0x10 - 0x17 */
        F, N, F, N, I, N, I, N, /* 0x18 - 0x1f */
        F, N, F, N, F, N, F, N, /* 0x20 - 0x27 */
        F, N, F, N, F, N, F, N, /* 0x28 - 0x2f */
        B, N, T, N, F, N, F, N, /* 0x30 - 0x37 */
        F, N, F, N, F, N, F, N, /* 0x38 - 0x3f */
    ]
};

/// C: `int sqlite3_value_type(sqlite3_value* pVal)`
pub fn sqlite3_value_type(pVal: &mut Mem) -> i32 {
    aType[(pVal.flags & MEM_AffMask) as usize] as i32
}

/// C: `int sqlite3_value_encoding(sqlite3_value *pVal)`
pub fn sqlite3_value_encoding(pVal: &mut Mem) -> i32 {
    pVal.enc as i32
}

/// C: `int sqlite3_value_nochange(sqlite3_value *pVal)`
///
/// Return true if a parameter to xUpdate represents an unchanged column.
pub fn sqlite3_value_nochange(pVal: &mut Mem) -> i32 {
    ((pVal.flags & (MEM_Null | MEM_Zero)) == (MEM_Null | MEM_Zero)) as i32
}

/// C: `int sqlite3_value_frombind(sqlite3_value *pVal)`
///
/// Return true if a parameter value originated from an sqlite3_bind().
pub fn sqlite3_value_frombind(pVal: &mut Mem) -> i32 {
    ((pVal.flags & MEM_FromBind) != 0) as i32
}

/// C: `sqlite3_value * sqlite3_value_dup(const sqlite3_value *pOrig)`
///
/// Make a copy of an sqlite3_value object.
pub fn sqlite3_value_dup(pOrig: &Mem) -> Option<Box<Mem>> {
    let mut pNew = Box::new(Mem::default());
    pNew.flags = MEM_Null;
    /* memcpy(pNew, pOrig, MEMCELLSIZE) */
    memShallowCopy(&mut pNew, pOrig, MEM_Ephem);
    pNew.flags = pOrig.flags & !MEM_Dyn;
    if pNew.flags & (MEM_Str | MEM_Blob) != 0 {
        pNew.flags &= !(MEM_Static | MEM_Dyn);
        pNew.flags |= MEM_Ephem;
        pNew.pPtr = None;
        let mut t = Mem::default();
        t.flags = MEM_Null;
        if sqlite3VdbeMemCopy(&mut t, &pNew) != SQLITE_OK {
            return None;
        }
        /* sqlite3VdbeMemMakeWriteable(pNew) */
        let mut pNew2 = Box::new(t);
        pNew2.flags &= !MEM_Ephem;
        return Some(pNew2);
    } else if pNew.flags & MEM_Null != 0 {
        /* Do not duplicate pointer values */
        pNew.flags &= !(MEM_Term | MEM_Subtype);
        pNew.pPtr = None;
    } else {
        pNew.pPtr = None;
    }
    Some(pNew)
}

/// C: `void sqlite3_value_free(sqlite3_value *pOld)`
///
/// Destroy an sqlite3_value object previously obtained from
/// sqlite3_value_dup().
pub fn sqlite3_value_free(pOld: &mut Mem) {
    sqlite3VdbeMemRelease(pOld);
}

/**************************** sqlite3_result_  *******************************
** The following routines are used by application-defined SQL functions to
** specify the function return value.
*/

/// Helper for the sqlite3_result_xxx() functions that store a string or
/// blob: sqlite3VdbeMemSetStr() with the appropriate errors.
fn setResultStrOrError(pCtx: &mut Context, z: Option<&[u8]>, n: i64, enc: u8) {
    let rc;
    if enc == SQLITE_UTF8 {
        rc = sqlite3VdbeMemSetText(pCtx.db, pCtx.pOut, z, n);
    } else if enc == SQLITE_UTF8_ZT {
        rc = sqlite3VdbeMemSetText(pCtx.db, pCtx.pOut, z, n);
        pCtx.pOut.flags |= MEM_Term;
    } else {
        rc = sqlite3VdbeMemSetStr(pCtx.db, pCtx.pOut, z, n, enc);
    }
    if rc != 0 {
        if rc == SQLITE_TOOBIG {
            sqlite3_result_error_toobig(pCtx);
        } else {
            /* The only errors possible from sqlite3VdbeMemSetStr are
             ** SQLITE_TOOBIG and SQLITE_NOMEM */
            sqlite3_result_error_nomem(pCtx);
        }
        return;
    }
    let e = pCtx.enc;
    sqlite3VdbeChangeEncoding(pCtx.db, pCtx.pOut, e as i32);
    if sqlite3VdbeMemTooBig(pCtx.db, pCtx.pOut) != 0 {
        sqlite3_result_error_toobig(pCtx);
    }
}

/// invokeValueDestructor(): the value is not used; raise SQLITE_TOOBIG.
fn invokeValueDestructor(pCtx: &mut Context) -> i32 {
    sqlite3_result_error_toobig(pCtx);
    SQLITE_TOOBIG
}

/// C: `void sqlite3_result_blob( sqlite3_context *pCtx, const void *z, int n, void (*xDel)(void *) )`
pub fn sqlite3_result_blob(pCtx: &mut Context, z: &[u8], n: i32) {
    debug_assert!(n >= 0);
    setResultStrOrError(pCtx, Some(z), n as i64, 0);
}

/// C: `void sqlite3_result_blob64( sqlite3_context *pCtx, const void *z, sqlite3_uint64 n, void (*xDel)(void *) )`
pub fn sqlite3_result_blob64(pCtx: &mut Context, z: &[u8], n: u64) {
    if n > 0x7fffffff {
        invokeValueDestructor(pCtx);
    } else {
        setResultStrOrError(pCtx, Some(z), n as i64, 0);
    }
}

/// C: `void sqlite3_result_double(sqlite3_context *pCtx, double rVal)`
pub fn sqlite3_result_double(pCtx: &mut Context, rVal: f64) {
    sqlite3VdbeMemSetDouble(pCtx.pOut, rVal);
}

/// C: `void sqlite3_result_error(sqlite3_context *pCtx, const char *z, int n)`
///
/// Raise an SQLITE_ERROR; the message is stored in pOut as text (and in
/// Context::zErr).
pub fn sqlite3_result_error(pCtx: &mut Context, z: Option<&[u8]>, n: i32) {
    pCtx.isError = SQLITE_ERROR;
    sqlite3VdbeMemSetStr(pCtx.db, pCtx.pOut, z, n as i64, SQLITE_UTF8);
    pCtx.zErr = z.map(|_| pCtx.pOut.bytes().to_vec());
}

/// C: `void sqlite3_result_error16(sqlite3_context *pCtx, const void *z, int n)`
pub fn sqlite3_result_error16(pCtx: &mut Context, z: &[u8], n: i32) {
    pCtx.isError = SQLITE_ERROR;
    sqlite3VdbeMemSetStr(pCtx.db, pCtx.pOut, Some(z), n as i64, SQLITE_UTF16NATIVE);
}

/// C: `void sqlite3_result_int(sqlite3_context *pCtx, int iVal)`
pub fn sqlite3_result_int(pCtx: &mut Context, iVal: i32) {
    sqlite3VdbeMemSetInt64(pCtx.pOut, iVal as i64);
}

/// C: `void sqlite3_result_int64(sqlite3_context *pCtx, i64 iVal)`
pub fn sqlite3_result_int64(pCtx: &mut Context, iVal: i64) {
    sqlite3VdbeMemSetInt64(pCtx.pOut, iVal);
}

/// C: `void sqlite3_result_null(sqlite3_context *pCtx)`
pub fn sqlite3_result_null(pCtx: &mut Context) {
    sqlite3VdbeMemSetNull(pCtx.pOut);
}

/// C: `void sqlite3_result_pointer( sqlite3_context *pCtx, void *pPtr, const char *zPType, void (*xDestructor)(void*) )`
///
/// Rust: the pointer is an `Rc<dyn Any>` (its drop is the destructor).
pub fn sqlite3_result_pointer(pCtx: &mut Context, pPtr: Rc<dyn Any>, zPType: Option<&[u8]>) {
    let pOut = &mut *pCtx.pOut;
    sqlite3VdbeMemRelease(pOut);
    pOut.flags = MEM_Null;
    sqlite3VdbeMemSetPointer(pOut, pPtr, zPType);
}

/// C: `void sqlite3_result_subtype(sqlite3_context *pCtx, unsigned int eSubtype)`
pub fn sqlite3_result_subtype(pCtx: &mut Context, eSubtype: u32) {
    let pOut = &mut *pCtx.pOut;
    pOut.eSubtype = (eSubtype & 0xff) as u8;
    pOut.flags |= MEM_Subtype;
}

/// C: `void sqlite3_result_text( sqlite3_context *pCtx, const char *z, int n, void (*xDel)(void *) )`
pub fn sqlite3_result_text(pCtx: &mut Context, z: Option<&[u8]>, n: i32) {
    setResultStrOrError(pCtx, z, n as i64, SQLITE_UTF8);
}

/// C: `void sqlite3_result_text64( sqlite3_context *pCtx, const char *z, sqlite3_uint64 n, void (*xDel)(void *), unsigned char enc )`
pub fn sqlite3_result_text64(pCtx: &mut Context, z: Option<&[u8]>, n: u64, enc: u8) {
    let mut enc = enc;
    let mut n = n;
    if enc != SQLITE_UTF8 && enc != SQLITE_UTF8_ZT {
        if enc == SQLITE_UTF16 {
            enc = SQLITE_UTF16NATIVE;
        }
        n &= !1u64;
    }
    if n > 0x7fffffff {
        invokeValueDestructor(pCtx);
    } else {
        /* (int)n: a value >= 2^31 was rejected above; n as i32 keeps the C
         ** behaviour for "negative" lengths (n = (u64)-1 => -1) */
        setResultStrOrError(pCtx, z, n as i32 as i64, enc);
        sqlite3VdbeMemZeroTerminateIfAble(pCtx.pOut);
    }
}

/// C: `void sqlite3_result_text16( sqlite3_context *pCtx, const void *z, int n, void (*xDel)(void *) )`
pub fn sqlite3_result_text16(pCtx: &mut Context, z: &[u8], n: i32) {
    setResultStrOrError(pCtx, Some(z), (n as i64) & !1, SQLITE_UTF16NATIVE);
}

/// C: `void sqlite3_result_text16be( sqlite3_context *pCtx, const void *z, int n, void (*xDel)(void *) )`
pub fn sqlite3_result_text16be(pCtx: &mut Context, z: &[u8], n: i32) {
    setResultStrOrError(pCtx, Some(z), (n as i64) & !1, SQLITE_UTF16BE);
}

/// C: `void sqlite3_result_text16le( sqlite3_context *pCtx, const void *z, int n, void (*xDel)(void *) )`
pub fn sqlite3_result_text16le(pCtx: &mut Context, z: &[u8], n: i32) {
    setResultStrOrError(pCtx, Some(z), (n as i64) & !1, SQLITE_UTF16LE);
}

/// C: `void sqlite3_result_value(sqlite3_context *pCtx, sqlite3_value *pValue)`
pub fn sqlite3_result_value(pCtx: &mut Context, pValue: &mut Mem) {
    sqlite3VdbeMemCopy(pCtx.pOut, pValue);
    let e = pCtx.enc;
    sqlite3VdbeChangeEncoding(pCtx.db, pCtx.pOut, e as i32);
    if sqlite3VdbeMemTooBig(pCtx.db, pCtx.pOut) != 0 {
        sqlite3_result_error_toobig(pCtx);
    }
}

/// C: `void sqlite3_result_zeroblob(sqlite3_context *pCtx, int n)`
pub fn sqlite3_result_zeroblob(pCtx: &mut Context, n: i32) {
    sqlite3_result_zeroblob64(pCtx, if n > 0 { n as u64 } else { 0 });
}

/// C: `int sqlite3_result_zeroblob64(sqlite3_context *pCtx, u64 n)`
pub fn sqlite3_result_zeroblob64(pCtx: &mut Context, n: u64) -> i32 {
    if n > pCtx.db.aLimit[SQLITE_LIMIT_LENGTH as usize] as u64 {
        sqlite3_result_error_toobig(pCtx);
        return SQLITE_TOOBIG;
    }
    sqlite3VdbeMemSetZeroBlob(pCtx.pOut, n as i32);
    SQLITE_OK
}

/// C: `void sqlite3_result_error_code(sqlite3_context *pCtx, int errCode)`
pub fn sqlite3_result_error_code(pCtx: &mut Context, errCode: i32) {
    pCtx.isError = if errCode != 0 { errCode } else { -1 };
    if pCtx.pOut.flags & MEM_Null != 0 {
        let z = crate::mainapi::sqlite3ErrStr(errCode);
        setResultStrOrError(pCtx, z.as_deref(), -1, SQLITE_UTF8);
        pCtx.zErr = z;
    }
}

/// C: `void sqlite3_result_error_toobig(sqlite3_context *pCtx)`
///
/// Cause the SQL function to raise an SQLITE_TOOBIG error.
pub fn sqlite3_result_error_toobig(pCtx: &mut Context) {
    pCtx.isError = SQLITE_TOOBIG;
    sqlite3VdbeMemSetStr(
        pCtx.db,
        pCtx.pOut,
        Some(b"string or blob too big"),
        -1,
        SQLITE_UTF8,
    );
    pCtx.zErr = Some(b"string or blob too big".to_vec());
}

/// C: `void sqlite3_result_error_nomem(sqlite3_context *pCtx)`
///
/// Cause the SQL function to raise an SQLITE_NOMEM error.  (There is no
/// OOM state in this port: db->mallocFailed is not set.)
pub fn sqlite3_result_error_nomem(pCtx: &mut Context) {
    sqlite3VdbeMemSetNull(pCtx.pOut);
    pCtx.isError = SQLITE_NOMEM;
}

/// C: `void sqlite3_result_str(sqlite3_context *pCtx, sqlite3_str *pStr, int eOwn)`
///
/// Make the return value of the SQL function be the content of the
/// sqlite3_str object pStr.  eOwn is SQLITE_COPY, SQLITE_XFER or
/// SQLITE_FINISH.
pub fn sqlite3_result_str(pCtx: &mut Context, pStr: &mut StrAccum, eOwn: i32) {
    if pStr.accError == 0 {
        if pStr.zText.is_empty() {
            setResultStrOrError(pCtx, Some(b""), 0, SQLITE_UTF8_ZT);
            crate::printf::sqlite3_str_reset(pStr);
        } else {
            let zText = std::mem::take(&mut pStr.zText);
            if eOwn == SQLITE_COPY {
                setResultStrOrError(pCtx, Some(&zText), zText.len() as i64, SQLITE_UTF8);
                pStr.zText = zText;
            } else {
                setResultStrOrError(pCtx, Some(&zText), zText.len() as i64, SQLITE_UTF8_ZT);
                let mx = pStr.mxAlloc as i32;
                crate::printf::sqlite3StrAccumInit(pStr, 0, mx);
            }
        }
    } else if pStr.accError as i32 == SQLITE_NOMEM {
        sqlite3_result_error_nomem(pCtx);
    } else {
        debug_assert!(pStr.accError as i32 == SQLITE_TOOBIG);
        sqlite3_result_error_toobig(pCtx);
    }
    if eOwn == SQLITE_FINISH {
        crate::printf::sqlite3_str_free(pStr);
    }
}

/// C: `void sqlite3ResultIntReal(sqlite3_context *pCtx)`
///
/// Force the INT64 value currently stored as the result to be a
/// MEM_IntReal value.  See the SQLITE_TESTCTRL_RESULT_INTREAL test-control.
pub fn sqlite3ResultIntReal(pCtx: &mut Context) {
    if pCtx.pOut.flags & MEM_Int != 0 {
        pCtx.pOut.flags &= !MEM_Int;
        pCtx.pOut.flags |= MEM_IntReal;
    }
}

/// This function is called after a transaction has been committed.  It
/// invokes callbacks registered with sqlite3_wal_hook() as required.
/// (No WAL hook in this port yet.)
fn doWalCallbacks(db: &mut Sqlite3) -> i32 {
    let _ = db;
    SQLITE_OK
}

/// Execute the statement p, either until a row of data is ready, the
/// statement is completely executed or an error occurs.
///
/// This routine implements the bulk of the logic behind the sqlite_step()
/// API.  The only thing omitted is the automatic recompile if a schema
/// change has occurred.  That detail is handled by the outer sqlite3_step()
/// wrapper procedure.
fn sqlite3Step(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut rc;

    if p.eVdbeState != VDBE_RUN_STATE {
        /* restart_step: */
        loop {
            if p.eVdbeState == VDBE_READY_STATE {
                if p.expired != 0 {
                    p.rc = SQLITE_SCHEMA;
                    rc = SQLITE_ERROR;
                    if (p.prepFlags as i32 & SQLITE_PREPARE_SAVESQL) != 0 {
                        /* If this statement was prepared using saved SQL and an
                         ** error has occurred, then return the error code in p->rc
                         ** to the caller. Set the error code in the database handle
                         ** to the same value.
                         */
                        rc = crate::vdbeaux::sqlite3VdbeTransferError(db, p);
                    }
                    /* goto end_of_step */
                    return rc & db.errMask;
                }

                /* If there are no other statements currently running, then
                 ** reset the interrupt flag.  This prevents a call to
                 ** sqlite3_interrupt from interrupting a statement that has not
                 ** yet started.
                 */
                if db.nVdbeActive == 0 {
                    db.isInterrupted.set(0);
                }

                if ((db.mTrace as u32) & (SQLITE_TRACE_PROFILE | SQLITE_TRACE_XPROFILE)) != 0
                    && db.init.busy == 0
                    && p.zSql.is_some()
                {
                    p.startTime = currentTimeInt64();
                } else {
                    debug_assert!(p.startTime == 0);
                }

                db.nVdbeActive += 1;
                if p.readOnly == 0 {
                    db.nVdbeWrite += 1;
                }
                if p.bIsReader != 0 {
                    db.nVdbeRead += 1;
                }
                p.pc = 0;
                p.eVdbeState = VDBE_RUN_STATE;
                break;
            } else if p.eVdbeState == VDBE_HALT_STATE {
                /* We used to require that sqlite3_reset() be called before
                 ** retrying sqlite3_step() after any error or after SQLITE_DONE.
                 ** Beginning with version 3.7.0, sqlite3_reset() is called
                 ** automatically instead ("automatic-reset").
                 */
                vdbeResetStmt(db, p);
                debug_assert!(p.eVdbeState == VDBE_READY_STATE);
                continue;
            } else {
                break;
            }
        }
    }

    if p.explain != 0 {
        rc = crate::vdbeaux::sqlite3VdbeList(db, p);
    } else {
        db.nVdbeExec += 1;
        rc = crate::vdbe::sqlite3VdbeExec(db, p);
        db.nVdbeExec -= 1;
    }

    if rc == SQLITE_ROW {
        debug_assert!(p.rc == SQLITE_OK);
        debug_assert!(db.mallocFailed == 0);
        db.errCode = SQLITE_ROW;
        return SQLITE_ROW;
    } else {
        /* If the statement completed successfully, invoke the profile
         ** callback */
        checkProfileCallback(db, p);
        p.iResultRow = -1;
        if rc == SQLITE_DONE && db.autoCommit != 0 {
            debug_assert!(p.rc == SQLITE_OK);
            p.rc = doWalCallbacks(db);
            if p.rc != SQLITE_OK {
                rc = SQLITE_ERROR;
            }
        } else if rc != SQLITE_DONE && (p.prepFlags as i32 & SQLITE_PREPARE_SAVESQL) != 0 {
            /* If this statement was prepared using saved SQL and an error
             ** has occurred, then return the error code in p->rc to the
             ** caller. Set the error code in the database handle to the same
             ** value.
             */
            rc = crate::vdbeaux::sqlite3VdbeTransferError(db, p);
        }
    }

    db.errCode = rc;
    let prc = p.rc;
    if SQLITE_NOMEM == sqlite3ApiExit(db, prc) {
        p.rc = SQLITE_NOMEM;
        if (p.prepFlags as i32 & SQLITE_PREPARE_SAVESQL) != 0 {
            rc = p.rc;
        }
    }
    /* end_of_step: */
    rc & db.errMask
}

/// sqlite3_step() on statement `pStmt`, already taken out of its slot into
/// `v`.  sqlite3Reprepare() needs the statement in its slot, so `v` is put
/// back for the duration of that call.
pub fn vdbeStepStmt(db: &mut Sqlite3, pStmt: StmtId, v: &mut Box<Vdbe>) -> i32 {
    let mut rc;
    let mut cnt = 0; /* Counter to prevent infinite loop of reprepares */
    loop {
        rc = sqlite3Step(db, v);
        if rc != SQLITE_SCHEMA {
            break;
        }
        let ok = cnt < SQLITE_MAX_SCHEMA_RETRY;
        cnt += 1;
        if !ok {
            break;
        }
        let savedPc = v.pc;
        db.stmt_put(pStmt, std::mem::take(v));
        rc = crate::prepare::sqlite3Reprepare(db, pStmt);
        *v = db.stmt_take(pStmt).expect("statement slot");
        if rc != SQLITE_OK {
            /* This case occurs after failing to recompile an sql statement.
             ** The error message from the SQL compiler has already been loaded
             ** into the database handle. This block copies the error message
             ** from the database handle into the statement and sets the
             ** statement program counter to 0 to ensure that when the
             ** statement is finalized or reset the parser error message is
             ** available via sqlite3_errmsg() and sqlite3_errcode().
             */
            let zErr: Option<ZStr> = match db.pErr.as_deref_mut() {
                Some(m) => sqlite3_value_text(m).map(|s| s.to_vec()),
                None => None,
            };
            v.zErrMsg = zErr;
            rc = sqlite3ApiExit(db, rc);
            v.rc = rc;
            break;
        }
        vdbeResetStmt(db, v);
        if savedPc >= 0 {
            /* Setting minWriteFileFormat to 254 is a signal to the OP_Init and
             ** OP_Trace opcodes to *not* perform SQLITE_TRACE_STMT because it
             ** has already been done once on a prior invocation that failed
             ** due to SQLITE_SCHEMA.   tag-20220401a  */
            v.minWriteFileFormat = 254;
        }
    }
    rc
}

/// C: `int sqlite3_step(sqlite3_stmt *pStmt)`
///
/// This is the top-level implementation of sqlite3_step().  Call
/// sqlite3Step() to do most of the work.  If a schema error occurs, call
/// sqlite3Reprepare() and try again.
pub fn sqlite3_step(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    let mut v = match db.stmt_take(pStmt) {
        Some(v) => v,
        None => return vdbeSafetyMisuse(),
    };
    let rc = vdbeStepStmt(db, pStmt, &mut v);
    db.stmt_put(pStmt, v);
    rc
}

/// C: `void * sqlite3_user_data(sqlite3_context *p)`
///
/// Extract the user data from a sqlite3_context structure.
pub fn sqlite3_user_data(p: &mut Context) -> FuncUserData {
    p.pFunc.pUserData.clone()
}

/// C: `sqlite3 * sqlite3_context_db_handle(sqlite3_context *p)`
///
/// The database connection of the function context.
pub fn sqlite3_context_db_handle<'a>(p: &'a mut Context) -> &'a mut Sqlite3 {
    &mut *p.db
}

/// C: `int sqlite3_vtab_nochange(sqlite3_context *p)`
///
/// If this routine is invoked from within an xColumn method of a virtual
/// table, then it returns true if and only if the the call is during an
/// UPDATE operation and the value of the column will not be modified by
/// the UPDATE.
pub fn sqlite3_vtab_nochange(p: &mut Context) -> i32 {
    sqlite3_value_nochange(p.pOut)
}

/// C: `void sqlite3VdbeValueListFree(void *pToDelete)`
///
/// The destructor function for a ValueList object (nothing to do: the
/// list is reference counted).
pub fn sqlite3VdbeValueListFree(pToDelete: &mut [u8]) {
    let _ = pToDelete;
}

/// Implementation of sqlite3_vtab_in_first() (if bNext==0) and
/// sqlite3_vtab_in_next() (if bNext!=0).  Virtual tables are not supported
/// yet: no value is ever a ValueList, so this returns SQLITE_ERROR as C
/// does for a value that is not a ValueList.
fn valueFromValueList(pVal: &mut Mem, ppOut: &mut [Mem], bNext: i32) -> i32 {
    let _ = (ppOut, bNext);
    if (pVal.flags & MEM_Dyn) == 0
        || pVal.u.zPType.as_deref() != Some("ValueList")
        || pVal.eSubtype != b'p'
    {
        return SQLITE_ERROR;
    }
    SQLITE_ERROR
}

/// C: `int sqlite3_vtab_in_first(sqlite3_value *pVal, sqlite3_value **ppOut)`
pub fn sqlite3_vtab_in_first(pVal: &mut Mem, ppOut: &mut [Mem]) -> i32 {
    valueFromValueList(pVal, ppOut, 0)
}

/// C: `int sqlite3_vtab_in_next(sqlite3_value *pVal, sqlite3_value **ppOut)`
pub fn sqlite3_vtab_in_next(pVal: &mut Mem, ppOut: &mut [Mem]) -> i32 {
    valueFromValueList(pVal, ppOut, 1)
}

/// C: `sqlite3_int64 sqlite3StmtCurrentTime(sqlite3_context *p)`
///
/// Return the current time for a statement.  If the current time is
/// requested more than once within the same run of a single prepared
/// statement, the exact same time is returned for each invocation.
pub fn sqlite3StmtCurrentTime(p: &mut Context) -> i64 {
    match p.piCurrentTime.as_deref_mut() {
        Some(piTime) => {
            if *piTime == 0 {
                *piTime = currentTimeInt64();
            }
            *piTime
        }
        None => currentTimeInt64(),
    }
}

/// C: `void * sqlite3_aggregate_context(sqlite3_context *p, int nByte)`
/// Rust: the aggregate context is a typed value; nByte<=0 means "do not
/// allocate if it does not exist yet" (returns None in that case).
///
/// The context is stored in `pMem.pAgg`; on creation the cell becomes
/// MEM_Agg with `u.pDef` set to the function, as in C.  None is also
/// returned when the existing context has a different type than T.
pub fn sqlite3_aggregate_context<'a, T: Default + 'static>(
    p: &'a mut Context<'_>,
    nByte: i32,
) -> Option<&'a mut T> {
    let pFunc = p.pFunc.clone();
    let pMem: &'a mut Mem = p.pMem.as_deref_mut()?;
    if (pMem.flags & MEM_Agg) == 0 || pMem.pAgg.is_none() {
        /* createAggContext() */
        if nByte <= 0 {
            sqlite3VdbeMemSetNull(pMem);
            pMem.z = Vec::new();
            return None;
        }
        sqlite3VdbeMemClearAndResize(pMem, 0);
        pMem.flags = MEM_Agg;
        pMem.u.pDef = Some(pFunc);
        pMem.pAgg = Some(Box::new(T::default()));
    }
    pMem.pAgg.as_mut()?.downcast_mut::<T>()
}

/// C: `void * sqlite3_get_auxdata(sqlite3_context *pCtx, int iArg)`
///
/// Return the auxiliary data pointer, if any, for the iArg'th argument to
/// the user-function defined by pCtx.  If iArg is negative then access a
/// cache of auxiliary data pointers that is available to all functions
/// within a single prepared statement.
pub fn sqlite3_get_auxdata(pCtx: &mut Context, iArg: i32) -> Option<Rc<dyn Any>> {
    let iOp = pCtx.iOp;
    let list = pCtx.pAuxData.as_deref()?;
    for pAuxData in list.iter() {
        if pAuxData.iAuxArg == iArg && (pAuxData.iAuxOp == iOp || iArg < 0) {
            return Some(pAuxData.pAux.clone());
        }
    }
    None
}

/// C: `void sqlite3_set_auxdata( sqlite3_context *pCtx, int iArg, void *pAux, void (*xDelete)(void*) )`
///
/// Set the auxiliary data pointer for the iArg'th argument to the
/// user-function defined by pCtx.  Any previous value is dropped (C: its
/// delete function is called).  The list keeps the C order (newest
/// first).
pub fn sqlite3_set_auxdata(pCtx: &mut Context, iArg: i32, pAux: Rc<dyn Any>) {
    let iOp = pCtx.iOp;
    let list = match pCtx.pAuxData.as_deref_mut() {
        Some(l) => l,
        None => return, /* failed: drop pAux */
    };
    for pAuxData in list.iter_mut() {
        if pAuxData.iAuxArg == iArg && (pAuxData.iAuxOp == iOp || iArg < 0) {
            pAuxData.pAux = pAux;
            return;
        }
    }
    list.insert(
        0,
        AuxData {
            iAuxOp: iOp,
            iAuxArg: iArg,
            pAux,
        },
    );
    if pCtx.isError == 0 {
        pCtx.isError = -1;
    }
}

/// C: `int sqlite3_aggregate_count(sqlite3_context *p)`
///
/// Return the number of times the Step function of an aggregate has been
/// called (deprecated).
pub fn sqlite3_aggregate_count(p: &mut Context) -> i32 {
    p.pMem.as_deref().map(|m| m.n).unwrap_or(0)
}

/// C: `int sqlite3_column_count(sqlite3_stmt *pStmt)`
///
/// Return the number of columns in the result set for the statement.
pub fn sqlite3_column_count(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    stmtRef(db, pStmt).map(|v| v.nResColumn as i32).unwrap_or(0)
}

/// The statement of a valid handle, in place (None if invalid or
/// currently taken out of its slot).
fn stmtRef(db: &Sqlite3, pStmt: StmtId) -> Option<&Vdbe> {
    if !db.stmt_valid(pStmt) {
        return None;
    }
    db.aVdbe[pStmt.idx as usize].v.as_deref()
}

fn stmtMut(db: &mut Sqlite3, pStmt: StmtId) -> Option<&mut Vdbe> {
    if !db.stmt_valid(pStmt) {
        return None;
    }
    db.aVdbe[pStmt.idx as usize].v.as_deref_mut()
}

/// C: `int sqlite3_data_count(sqlite3_stmt *pStmt)`
///
/// Return the number of values available from the current row of the
/// currently executing statement pStmt.
pub fn sqlite3_data_count(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    match stmtRef(db, pStmt) {
        Some(v) if v.iResultRow > 0 => v.nResColumn as i32,
        _ => 0,
    }
}

/// columnNullValue(): an SQL NULL value.
fn columnNullValue() -> Mem {
    let mut m = Mem::default();
    m.flags = MEM_Null;
    m
}

/// columnMem() + columnMallocFailure(): run `f` on the Mem of column i of
/// the current row (or on a NULL value with SQLITE_RANGE if i is out of
/// range).
fn columnApply<R>(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    f: impl FnOnce(&mut Sqlite3, &mut Mem) -> R,
) -> R {
    let mut v = match db.stmt_take(pStmt) {
        Some(v) => v,
        None => {
            let mut m = columnNullValue();
            return f(db, &mut m);
        }
    };
    let k = (v.iResultRow as i64) + i as i64;
    let r = if v.iResultRow > 0 && i < v.nResColumn as i32 && i >= 0 && (k as usize) < v.aMem.len()
    {
        f(db, &mut v.aMem[k as usize])
    } else {
        sqlite3Error(db, SQLITE_RANGE);
        let mut m = columnNullValue();
        f(db, &mut m)
    };
    /* columnMallocFailure() */
    let prc = v.rc;
    v.rc = sqlite3ApiExit(db, prc);
    db.stmt_put(pStmt, v);
    r
}

/**************************** sqlite3_column_  *******************************
** The following routines are used to access elements of the current row
** in the result set.
*/

/// C: `const void * sqlite3_column_blob(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_blob(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> Option<Vec<u8>> {
    columnApply(db, pStmt, i, |db, m| {
        valueBlobDb(Some(db), m).map(|s| s.to_vec())
    })
}

/// C: `int sqlite3_column_bytes(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_bytes(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i32 {
    columnApply(db, pStmt, i, |db, m| valueBytesDb(Some(db), m, SQLITE_UTF8))
}

/// C: `int sqlite3_column_bytes16(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_bytes16(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i32 {
    columnApply(db, pStmt, i, |db, m| valueBytes16(Some(db), m))
}

/// C: `double sqlite3_column_double(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_double(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> f64 {
    columnApply(db, pStmt, i, |_db, m| memRealValueAny(m))
}

/// C: `int sqlite3_column_int(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_int(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i32 {
    columnApply(db, pStmt, i, |_db, m| memIntValueAny(m) as i32)
}

/// C: `sqlite_int64 sqlite3_column_int64(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_int64(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i64 {
    columnApply(db, pStmt, i, |_db, m| memIntValueAny(m))
}

/// C: `const unsigned char * sqlite3_column_text(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_text(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> Option<Vec<u8>> {
    columnApply(db, pStmt, i, |db, m| {
        valueTextRef(Some(db), m, SQLITE_UTF8).map(|s| s.to_vec())
    })
}

/// C: `sqlite3_value * sqlite3_column_value(sqlite3_stmt *pStmt, int i)`
///
/// Rust: returns the Mem of the column in place; None stands for the C
/// static NULL value (statement without a current row, or column out of
/// range: SQLITE_RANGE is set on the connection in that case).
pub fn sqlite3_column_value(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> Option<&mut Mem> {
    let (ok, k, prc) = match stmtRef(db, pStmt) {
        None => return None,
        Some(v) => {
            let k = (v.iResultRow as i64) + i as i64;
            (
                v.iResultRow > 0
                    && i < v.nResColumn as i32
                    && i >= 0
                    && (k as usize) < v.aMem.len(),
                k as usize,
                v.rc,
            )
        }
    };
    if !ok {
        sqlite3Error(db, SQLITE_RANGE);
    }
    let nrc = sqlite3ApiExit(db, prc);
    let v = stmtMut(db, pStmt)?;
    v.rc = nrc;
    if !ok {
        return None;
    }
    let pOut = &mut v.aMem[k];
    if pOut.flags & MEM_Static != 0 {
        pOut.flags &= !MEM_Static;
        pOut.flags |= MEM_Ephem;
    }
    Some(pOut)
}

/// C: `const void * sqlite3_column_text16(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_text16(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> Option<Vec<u8>> {
    columnApply(db, pStmt, i, |db, m| {
        valueText16(Some(db), m, SQLITE_UTF16NATIVE)
    })
}

/// C: `int sqlite3_column_type(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_column_type(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i32 {
    columnApply(db, pStmt, i, |_db, m| sqlite3_value_type(m))
}

/// Column names appropriate for EXPLAIN or EXPLAIN QUERY PLAN.
static azExplainColNames8: [&[u8]; 12] = [
    b"addr", b"opcode", b"p1", b"p2", b"p3", b"p4", b"p5", b"comment", /* EXPLAIN */
    b"id", b"parent", b"notused", b"detail", /* EQP */
];

/// Convert the N-th element of pStmt->aColName[] into a string then return
/// that string.  If N is out of range, return None.
///
/// There are up to 5 names for each column.  useType determines which name
/// is returned (COLNAME_NAME, COLNAME_DECLTYPE, COLNAME_DATABASE,
/// COLNAME_TABLE, COLNAME_COLUMN).
fn columnName(
    db: &mut Sqlite3,
    pStmt: StmtId,
    N: i32,
    useUtf16: bool,
    useType: i32,
) -> Option<ZStr> {
    if N < 0 {
        return None;
    }
    db.with_stmt(pStmt, |db, p| {
        if p.explain != 0 {
            if useType > 0 {
                return None;
            }
            let n = if p.explain == 1 { 8 } else { 4 };
            if N >= n {
                return None;
            }
            let z = azExplainColNames8[(N + 8 * p.explain as i32 - 8) as usize];
            if useUtf16 {
                let mut out = Vec::new();
                for &c in z {
                    out.extend_from_slice(&(c as u16).to_le_bytes());
                }
                return Some(out);
            }
            return Some(z.to_vec());
        }
        let n = p.nResColumn as i32;
        if N < n {
            let k = (N + useType * n) as usize;
            let m = p.aColName.get_mut(k)?;
            if useUtf16 {
                valueText16(Some(db), m, SQLITE_UTF16NATIVE)
            } else {
                valueTextRef(Some(db), m, SQLITE_UTF8).map(|s| s.to_vec())
            }
        } else {
            None
        }
    })
    .flatten()
}

/// C: `const char * sqlite3_column_name(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_name(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<ZStr> {
    columnName(db, pStmt, N, false, COLNAME_NAME)
}

/// C: `const void * sqlite3_column_name16(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_name16(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<Vec<u8>> {
    columnName(db, pStmt, N, true, COLNAME_NAME)
}

/// C: `const char * sqlite3_column_decltype(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_decltype(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<ZStr> {
    columnName(db, pStmt, N, false, COLNAME_DECLTYPE)
}

/// C: `const void * sqlite3_column_decltype16(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_decltype16(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<Vec<u8>> {
    columnName(db, pStmt, N, true, COLNAME_DECLTYPE)
}

/// C: `const char * sqlite3_column_database_name(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_database_name(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<ZStr> {
    columnName(db, pStmt, N, false, COLNAME_DATABASE)
}

/// C: `const void * sqlite3_column_database_name16(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_database_name16(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<Vec<u8>> {
    columnName(db, pStmt, N, true, COLNAME_DATABASE)
}

/// C: `const char * sqlite3_column_table_name(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_table_name(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<ZStr> {
    columnName(db, pStmt, N, false, COLNAME_TABLE)
}

/// C: `const void * sqlite3_column_table_name16(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_table_name16(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<Vec<u8>> {
    columnName(db, pStmt, N, true, COLNAME_TABLE)
}

/// C: `const char * sqlite3_column_origin_name(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_origin_name(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<ZStr> {
    columnName(db, pStmt, N, false, COLNAME_COLUMN)
}

/// C: `const void * sqlite3_column_origin_name16(sqlite3_stmt *pStmt, int N)`
pub fn sqlite3_column_origin_name16(db: &mut Sqlite3, pStmt: StmtId, N: i32) -> Option<Vec<u8>> {
    columnName(db, pStmt, N, true, COLNAME_COLUMN)
}

/******************************* sqlite3_bind_  ***************************
**
** Routines used to attach values to wildcards in a compiled SQL statement.
*/

/// Unbind the value bound to variable i in virtual machine p.  This is the
/// the same as binding a NULL value to the column.  If the "i" parameter is
/// out of range, then SQLITE_RANGE is returned.  Otherwise SQLITE_OK.
///
/// The error code stored in database db is overwritten with the return
/// value in any case.
fn vdbeUnbind(db: &mut Sqlite3, p: &mut Vdbe, i: u32) -> i32 {
    if p.eVdbeState != VDBE_READY_STATE {
        sqlite3Error(db, SQLITE_MISUSE);
        let zSql = p.zSql.clone();
        crate::printf::sqlite3_log(
            SQLITE_MISUSE,
            b"bind on a busy prepared statement: [%s]",
            &[crate::printf::PfArg::Str(zSql.as_deref())],
        );
        return SQLITE_MISUSE;
    }
    if i >= p.nVar as u32 || i as usize >= p.aVar.len() {
        sqlite3Error(db, SQLITE_RANGE);
        return SQLITE_RANGE;
    }

    /* If the bit corresponding to this variable in Vdbe.expmask is set,
     ** then binding a new value to this variable invalidates the current
     ** query plan.
     **
     ** IMPLEMENTATION-OF: R-57496-20354 If the specific value bound to a
     ** host parameter in the WHERE clause might influence the choice of query
     ** plan for a statement, then the statement will be automatically
     ** recompiled, as if there had been a schema change, on the first
     ** sqlite3_step() call following any change to the bindings of that
     ** parameter.
     */
    debug_assert!((p.expmask & p.smimask) == p.smimask);
    let bit = if i >= 31 { 0x80000000u32 } else { 1u32 << i };
    {
        let pVar = &p.aVar[i as usize];
        if p.expmask != 0 && (p.expmask & bit) != 0 {
            /* If the p->smimask bit is also set, then only expire the
             ** statement if the value being clobbered was integer value 0 or
             ** 1. */
            if p.smimask == 0
                || (p.smimask & bit) == 0
                || ((pVar.flags & MEM_Int) != 0 && pVar.u.i >= 0 && pVar.u.i <= 1)
            {
                p.expired = 1;
            }
        }
    }

    let pVar = &mut p.aVar[i as usize];
    sqlite3VdbeMemRelease(pVar);
    pVar.flags = MEM_Null;
    db.errCode = SQLITE_OK;

    SQLITE_OK
}

/// Bind a text or BLOB value.
fn bindText(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    zData: Option<&[u8]>,
    nData: i64,
    encoding: u8,
) -> i32 {
    let r = db.with_stmt(pStmt, |db, p| {
        let mut rc = vdbeUnbind(db, p, (i as u32).wrapping_sub(1));
        if rc == SQLITE_OK {
            if zData.is_some() {
                let dbenc = db.ENC();
                let pVar = &mut p.aVar[(i - 1) as usize];
                if encoding == SQLITE_UTF8 {
                    rc = sqlite3VdbeMemSetText(db, pVar, zData, nData);
                } else if encoding == SQLITE_UTF8_ZT {
                    rc = sqlite3VdbeMemSetText(db, pVar, zData, nData);
                    pVar.flags |= MEM_Term;
                } else {
                    rc = sqlite3VdbeMemSetStr(db, pVar, zData, nData, encoding);
                    if encoding == 0 {
                        pVar.enc = dbenc;
                    }
                }
                if rc == SQLITE_OK && encoding != 0 {
                    rc = sqlite3VdbeChangeEncoding(db, pVar, dbenc as i32);
                }
                if rc != 0 {
                    sqlite3Error(db, rc);
                    rc = sqlite3ApiExit(db, rc);
                }
            }
        }
        rc
    });
    r.unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_blob( sqlite3_stmt *pStmt, int i, const void *zData, int nData, void (*xDel)(void*) )`
pub fn sqlite3_bind_blob(db: &mut Sqlite3, pStmt: StmtId, i: i32, zData: &[u8], nData: i32) -> i32 {
    bindText(db, pStmt, i, Some(zData), nData as i64, 0)
}

/// C: `int sqlite3_bind_blob64( sqlite3_stmt *pStmt, int i, const void *zData, sqlite3_uint64 nData, void (*xDel)(void*) )`
pub fn sqlite3_bind_blob64(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    zData: &[u8],
    nData: u64,
) -> i32 {
    bindText(db, pStmt, i, Some(zData), nData as i64, 0)
}

/// C: `int sqlite3_bind_double(sqlite3_stmt *pStmt, int i, double rValue)`
pub fn sqlite3_bind_double(db: &mut Sqlite3, pStmt: StmtId, i: i32, rValue: f64) -> i32 {
    db.with_stmt(pStmt, |db, p| {
        let rc = vdbeUnbind(db, p, (i as u32).wrapping_sub(1));
        if rc == SQLITE_OK {
            sqlite3VdbeMemSetDouble(&mut p.aVar[(i - 1) as usize], rValue);
        }
        rc
    })
    .unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_int(sqlite3_stmt *p, int i, int iValue)`
pub fn sqlite3_bind_int(db: &mut Sqlite3, p: StmtId, i: i32, iValue: i32) -> i32 {
    sqlite3_bind_int64(db, p, i, iValue as i64)
}

/// C: `int sqlite3_bind_int64(sqlite3_stmt *pStmt, int i, sqlite_int64 iValue)`
pub fn sqlite3_bind_int64(db: &mut Sqlite3, pStmt: StmtId, i: i32, iValue: i64) -> i32 {
    db.with_stmt(pStmt, |db, p| {
        if p.eVdbeState != VDBE_READY_STATE {
            sqlite3Error(db, SQLITE_MISUSE);
            let zSql = p.zSql.clone();
            crate::printf::sqlite3_log(
                SQLITE_MISUSE,
                b"bind on a busy prepared statement: [%s]",
                &[crate::printf::PfArg::Str(zSql.as_deref())],
            );
            return SQLITE_MISUSE;
        }
        let mut i = i;
        if i <= 0 || {
            i -= 1;
            i >= p.nVar as i32 || i as usize >= p.aVar.len()
        } {
            sqlite3Error(db, SQLITE_RANGE);
            return SQLITE_RANGE;
        }
        if p.expmask != 0 {
            let expireMask = if i >= 32 { 0x80000000u32 } else { 1u32 << i };
            debug_assert!((p.expmask & p.smimask) == p.smimask);
            if (p.expmask & expireMask) != 0 {
                let pVar = &p.aVar[i as usize];
                if (p.smimask & expireMask) != 0 {
                    /* If the smimask bit is set, only expire the prepared
                     ** statement if the value is changing to or from (0,1) and
                     ** something else */
                    let sm1 = (pVar.flags & MEM_Int) != 0 && pVar.u.i >= 0 && pVar.u.i <= 1;
                    let sm2 = iValue >= 0 && iValue <= 1;
                    if sm1 != sm2 {
                        p.expired = 1;
                    }
                } else if (pVar.flags & MEM_Int) == 0 || iValue != pVar.u.i {
                    /* Always expire if the smimask bit is clear */
                    p.expired = 1;
                }
            }
        }
        sqlite3VdbeMemSetInt64(&mut p.aVar[i as usize], iValue);
        SQLITE_OK
    })
    .unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_null(sqlite3_stmt *pStmt, int i)`
pub fn sqlite3_bind_null(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> i32 {
    db.with_stmt(pStmt, |db, p| vdbeUnbind(db, p, (i as u32).wrapping_sub(1)))
        .unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_pointer( sqlite3_stmt *pStmt, int i, void *pPtr, const char *zPTtype, void (*xDestructor)(void*) )`
///
/// Rust: the pointer is an `Rc<dyn Any>` (its drop is the destructor).
pub fn sqlite3_bind_pointer(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    pPtr: Rc<dyn Any>,
    zPTtype: Option<&[u8]>,
) -> i32 {
    db.with_stmt(pStmt, |db, p| {
        let rc = vdbeUnbind(db, p, (i as u32).wrapping_sub(1));
        if rc == SQLITE_OK {
            sqlite3VdbeMemSetPointer(&mut p.aVar[(i - 1) as usize], pPtr, zPTtype);
        }
        rc
    })
    .unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_text( sqlite3_stmt *pStmt, int i, const char *zData, int nData, void (*xDel)(void*) )`
pub fn sqlite3_bind_text(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    zData: Option<&[u8]>,
    nData: i32,
) -> i32 {
    bindText(db, pStmt, i, zData, nData as i64, SQLITE_UTF8)
}

/// C: `int sqlite3_bind_text64( sqlite3_stmt *pStmt, int i, const char *zData, sqlite3_uint64 nData, void (*xDel)(void*), unsigned char enc )`
pub fn sqlite3_bind_text64(
    db: &mut Sqlite3,
    pStmt: StmtId,
    i: i32,
    zData: Option<&[u8]>,
    nData: u64,
    enc: u8,
) -> i32 {
    let mut enc = enc;
    let mut nData = nData;
    if enc != SQLITE_UTF8 && enc != SQLITE_UTF8_ZT {
        if enc == SQLITE_UTF16 {
            enc = SQLITE_UTF16NATIVE;
        }
        nData &= !1u64;
    }
    bindText(db, pStmt, i, zData, nData as i64, enc)
}

/// C: `int sqlite3_bind_text16( sqlite3_stmt *pStmt, int i, const void *zData, int n, void (*xDel)(void*) )`
pub fn sqlite3_bind_text16(db: &mut Sqlite3, pStmt: StmtId, i: i32, zData: &[u8], n: i32) -> i32 {
    bindText(
        db,
        pStmt,
        i,
        Some(zData),
        (n as i64) & !1,
        SQLITE_UTF16NATIVE,
    )
}

/// C: `int sqlite3_bind_value(sqlite3_stmt *pStmt, int i, const sqlite3_value *pValue)`
pub fn sqlite3_bind_value(db: &mut Sqlite3, pStmt: StmtId, i: i32, pValue: &Mem) -> i32 {
    let ty = aType[(pValue.flags & MEM_AffMask) as usize] as i32;
    match ty {
        SQLITE_INTEGER => sqlite3_bind_int64(db, pStmt, i, pValue.u.i),
        SQLITE_FLOAT => {
            debug_assert!(pValue.flags & (MEM_Real | MEM_IntReal) != 0);
            let r = if pValue.flags & MEM_Real != 0 {
                pValue.u.r
            } else {
                pValue.u.i as f64
            };
            sqlite3_bind_double(db, pStmt, i, r)
        }
        SQLITE_BLOB => {
            if pValue.flags & MEM_Zero != 0 {
                sqlite3_bind_zeroblob(db, pStmt, i, pValue.u.nZero)
            } else {
                sqlite3_bind_blob(db, pStmt, i, pValue.bytes(), pValue.n)
            }
        }
        SQLITE_TEXT => bindText(
            db,
            pStmt,
            i,
            Some(pValue.bytes()),
            pValue.n as i64,
            pValue.enc,
        ),
        _ => sqlite3_bind_null(db, pStmt, i),
    }
}

/// C: `int sqlite3_bind_zeroblob(sqlite3_stmt *pStmt, int i, int n)`
pub fn sqlite3_bind_zeroblob(db: &mut Sqlite3, pStmt: StmtId, i: i32, n: i32) -> i32 {
    db.with_stmt(pStmt, |db, p| {
        let rc = vdbeUnbind(db, p, (i as u32).wrapping_sub(1));
        if rc == SQLITE_OK {
            sqlite3VdbeMemSetZeroBlob(&mut p.aVar[(i - 1) as usize], n);
        }
        rc
    })
    .unwrap_or_else(vdbeSafetyMisuse)
}

/// C: `int sqlite3_bind_zeroblob64(sqlite3_stmt *pStmt, int i, sqlite3_uint64 n)`
pub fn sqlite3_bind_zeroblob64(db: &mut Sqlite3, pStmt: StmtId, i: i32, n: u64) -> i32 {
    if !db.stmt_valid(pStmt) {
        return vdbeSafetyMisuse();
    }
    let mut rc;
    if n > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as u64 {
        rc = SQLITE_TOOBIG;
    } else {
        debug_assert!((n & 0x7FFFFFFF) == n);
        rc = sqlite3_bind_zeroblob(db, pStmt, i, n as i32);
    }
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_bind_parameter_count(sqlite3_stmt *pStmt)`
///
/// Return the number of wildcards that can be potentially bound to.
pub fn sqlite3_bind_parameter_count(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    stmtRef(db, pStmt).map(|p| p.nVar as i32).unwrap_or(0)
}

/// C: `const char * sqlite3_bind_parameter_name(sqlite3_stmt *pStmt, int i)`
///
/// Return the name of a wildcard parameter.  Return None if the index is
/// out of range or if the wildcard is unnamed.  The result is always UTF-8.
pub fn sqlite3_bind_parameter_name(db: &mut Sqlite3, pStmt: StmtId, i: i32) -> Option<ZStr> {
    let p = stmtMut(db, pStmt)?;
    crate::util::sqlite3VListNumToName(&mut p.pVList, i)
}

/// C: `int sqlite3VdbeParameterIndex(Vdbe *p, const char *zName, int nName)`
///
/// Given a wildcard parameter name, return the index of the variable with
/// that name.  If there is no variable with the given name, return 0.
pub fn sqlite3VdbeParameterIndex(p: &mut Vdbe, zName: Option<&[u8]>, nName: i32) -> i32 {
    if zName.is_none() {
        return 0;
    }
    crate::util::sqlite3VListNameToNum(&mut p.pVList, zName, nName)
}

/// C: `int sqlite3_bind_parameter_index(sqlite3_stmt *pStmt, const char *zName)`
pub fn sqlite3_bind_parameter_index(db: &mut Sqlite3, pStmt: StmtId, zName: Option<&[u8]>) -> i32 {
    let p = match stmtMut(db, pStmt) {
        Some(p) => p,
        None => return 0,
    };
    /* sqlite3Strlen30(zName) */
    let n = zName
        .map(|z| (z.iter().position(|&c| c == 0).unwrap_or(z.len()) & 0x3fffffff) as i32)
        .unwrap_or(0);
    sqlite3VdbeParameterIndex(p, zName, n)
}

/// C: `int sqlite3TransferBindings(sqlite3_stmt *pFromStmt, sqlite3_stmt *pToStmt)`
///
/// Transfer all bindings from the first statement over to the second.
pub fn sqlite3TransferBindings(db: &mut Sqlite3, pFromStmt: &mut Vdbe, pToStmt: &mut Vdbe) -> i32 {
    debug_assert!(pToStmt.nVar == pFromStmt.nVar);
    let n = (pFromStmt.nVar.max(0) as usize)
        .min(pFromStmt.aVar.len())
        .min(pToStmt.aVar.len());
    for i in 0..n {
        sqlite3VdbeMemMove(db, &mut pToStmt.aVar[i], &mut pFromStmt.aVar[i]);
    }
    SQLITE_OK
}

/// C: `int sqlite3_transfer_bindings(sqlite3_stmt *pFromStmt, sqlite3_stmt *pToStmt)`
///
/// Deprecated external interface.  If the two statements contain a
/// different number of bindings, then an SQLITE_ERROR is returned.
pub fn sqlite3_transfer_bindings(db: &mut Sqlite3, pFromStmt: StmtId, pToStmt: StmtId) -> i32 {
    if pFromStmt == pToStmt {
        return match stmtRef(db, pFromStmt) {
            Some(_) => SQLITE_OK,
            None => SQLITE_MISUSE,
        };
    }
    let mut pFrom = match db.stmt_take(pFromStmt) {
        Some(v) => v,
        None => return SQLITE_MISUSE,
    };
    let mut pTo = match db.stmt_take(pToStmt) {
        Some(v) => v,
        None => {
            db.stmt_put(pFromStmt, pFrom);
            return SQLITE_MISUSE;
        }
    };
    let rc = if pFrom.nVar != pTo.nVar {
        SQLITE_ERROR
    } else {
        if pTo.expmask != 0 {
            pTo.expired = 1;
        }
        if pFrom.expmask != 0 {
            pFrom.expired = 1;
        }
        sqlite3TransferBindings(db, &mut pFrom, &mut pTo)
    };
    db.stmt_put(pToStmt, pTo);
    db.stmt_put(pFromStmt, pFrom);
    rc
}

/// C: `sqlite3 * sqlite3_db_handle(sqlite3_stmt *pStmt)`
///
/// Rust: statements are always used together with their connection, so
/// there is nothing to return.
pub fn sqlite3_db_handle(db: &mut Sqlite3, pStmt: StmtId) {
    let _ = (db, pStmt);
}

/// C: `int sqlite3_stmt_readonly(sqlite3_stmt *pStmt)`
///
/// Return true if the prepared statement is guaranteed to not modify the
/// database.
pub fn sqlite3_stmt_readonly(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    stmtRef(db, pStmt).map(|p| p.readOnly as i32).unwrap_or(1)
}

/// C: `int sqlite3_stmt_isexplain(sqlite3_stmt *pStmt)`
///
/// Return 1 if the statement is an EXPLAIN and return 2 if the statement
/// is an EXPLAIN QUERY PLAN.
pub fn sqlite3_stmt_isexplain(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    stmtRef(db, pStmt).map(|p| p.explain as i32).unwrap_or(0)
}

/// C: `int sqlite3_stmt_explain(sqlite3_stmt *pStmt, int eMode)`
///
/// Set the explain mode for a statement.
pub fn sqlite3_stmt_explain(db: &mut Sqlite3, pStmt: StmtId, eMode: i32) -> i32 {
    let Some(mut v) = db.stmt_take(pStmt) else {
        return SQLITE_MISUSE;
    };
    let rc;
    if v.explain as i32 == eMode {
        rc = SQLITE_OK;
    } else if eMode < 0 || eMode > 2 {
        rc = SQLITE_ERROR;
    } else if (v.prepFlags as i32 & SQLITE_PREPARE_SAVESQL) == 0 {
        rc = SQLITE_ERROR;
    } else if v.eVdbeState != VDBE_READY_STATE {
        rc = SQLITE_BUSY;
    } else if v.nMem >= 10 && (eMode != 2 || v.haveEqpOps != 0) {
        /* No reprepare necessary */
        v.explain = eMode as u8;
        rc = SQLITE_OK;
    } else {
        v.explain = eMode as u8;
        /* sqlite3Reprepare() needs the statement in its slot */
        db.stmt_put(pStmt, v);
        rc = crate::prepare::sqlite3Reprepare(db, pStmt);
        v = db.stmt_take(pStmt).expect("statement slot");
        v.haveEqpOps = (eMode == 2) as u8;
    }
    if v.explain != 0 {
        v.nResColumn = (12 - 4 * v.explain as i32) as u16;
    } else {
        v.nResColumn = v.nResAlloc;
    }
    db.stmt_put(pStmt, v);
    rc
}

/// C: `int sqlite3_stmt_busy(sqlite3_stmt *pStmt)`
///
/// Return true if the prepared statement is in need of being reset.
pub fn sqlite3_stmt_busy(db: &mut Sqlite3, pStmt: StmtId) -> i32 {
    match stmtRef(db, pStmt) {
        Some(v) => (v.eVdbeState == VDBE_RUN_STATE) as i32,
        /* a statement taken out of its slot is running */
        None => (db.stmt_valid(pStmt)) as i32,
    }
}

/// C: `sqlite3_stmt * sqlite3_next_stmt(sqlite3 *pDb, sqlite3_stmt *pStmt)`
///
/// Return the next prepared statement after pStmt associated with the
/// database connection.  If pStmt is None, return the first prepared
/// statement for the database connection.  Return None if there are no
/// more.  Rust: the order is the reverse slot order (C: most recently
/// created first).
pub fn sqlite3_next_stmt(db: &mut Sqlite3, pStmt: Option<StmtId>) -> Option<StmtId> {
    let mut ids = db.stmt_ids();
    ids.reverse();
    match pStmt {
        None => ids.first().copied(),
        Some(s) => {
            let pos = ids.iter().position(|&x| x == s)?;
            ids.get(pos + 1).copied()
        }
    }
}

/// C: `int sqlite3_stmt_status(sqlite3_stmt *pStmt, int op, int resetFlag)`
///
/// Return the value of a status counter for a prepared statement.
/// SQLITE_STMTSTATUS_MEMUSED is an estimate of the memory used by the
/// statement.
pub fn sqlite3_stmt_status(db: &mut Sqlite3, pStmt: StmtId, op: i32, resetFlag: i32) -> i32 {
    let p = match stmtMut(db, pStmt) {
        Some(p) => p,
        None => return 0,
    };
    if op == SQLITE_STMTSTATUS_MEMUSED {
        let mut v = std::mem::size_of::<Vdbe>();
        v += p.aOp.len() * std::mem::size_of::<VdbeOp>();
        for m in p.aMem.iter().chain(p.aVar.iter()).chain(p.aColName.iter()) {
            v += std::mem::size_of::<Mem>() + m.z.capacity();
        }
        v += p.apCsr.len() * std::mem::size_of::<Option<Box<VdbeCursor>>>();
        v += p.zSql.as_ref().map(|z| z.len() + 1).unwrap_or(0);
        v as i32
    } else if op < 0 || op as usize >= p.aCounter.len() {
        0
    } else {
        let v = p.aCounter[op as usize];
        if resetFlag != 0 {
            p.aCounter[op as usize] = 0;
        }
        v as i32
    }
}

/// C: `const char * sqlite3_sql(sqlite3_stmt *pStmt)`
///
/// Return the SQL associated with a prepared statement.
pub fn sqlite3_sql(db: &mut Sqlite3, pStmt: StmtId) -> Option<ZStr> {
    stmtRef(db, pStmt).and_then(|p| p.zSql.clone())
}

/// C: `char * sqlite3_expanded_sql(sqlite3_stmt *pStmt)`
///
/// Return the SQL associated with a prepared statement with bound
/// parameters expanded.
pub fn sqlite3_expanded_sql(db: &mut Sqlite3, pStmt: StmtId) -> Option<ZStr> {
    let zSql = sqlite3_sql(db, pStmt)?;
    db.with_stmt(pStmt, |db, p| {
        crate::vdbetrace::sqlite3VdbeExpandSql(db, p, Some(&zSql))
    })
    .flatten()
}

/// C: `const char * sqlite3_normalized_sql(sqlite3_stmt *pStmt)`
/// (SQLITE_ENABLE_NORMALIZE is not part of the build: always None.)
pub fn sqlite3_normalized_sql(db: &mut Sqlite3, pStmt: StmtId) -> Option<ZStr> {
    let _ = (db, pStmt);
    None
}

/// C: `int sqlite3_preupdate_old(sqlite3 *db, int iIdx, sqlite3_value **ppValue)`
/// (no preupdate hook in this build: always SQLITE_MISUSE, as C does when
/// called outside a pre-update callback.)
pub fn sqlite3_preupdate_old(db: &mut Sqlite3, iIdx: i32, ppValue: &mut [Mem]) -> i32 {
    let _ = (iIdx, ppValue);
    sqlite3Error(db, SQLITE_MISUSE);
    sqlite3ApiExit(db, SQLITE_MISUSE)
}

/// C: `int sqlite3_preupdate_count(sqlite3 *db)`
pub fn sqlite3_preupdate_count(db: &mut Sqlite3) -> i32 {
    let _ = db;
    0
}

/// C: `int sqlite3_preupdate_depth(sqlite3 *db)`
pub fn sqlite3_preupdate_depth(db: &mut Sqlite3) -> i32 {
    let _ = db;
    0
}

/// C: `int sqlite3_preupdate_blobwrite(sqlite3 *db)`
pub fn sqlite3_preupdate_blobwrite(db: &mut Sqlite3) -> i32 {
    let _ = db;
    -1
}

/// C: `int sqlite3_preupdate_new(sqlite3 *db, int iIdx, sqlite3_value **ppValue)`
/// (no preupdate hook: always SQLITE_MISUSE.)
pub fn sqlite3_preupdate_new(db: &mut Sqlite3, iIdx: i32, ppValue: &mut [Mem]) -> i32 {
    let _ = (iIdx, ppValue);
    sqlite3Error(db, SQLITE_MISUSE);
    sqlite3ApiExit(db, SQLITE_MISUSE)
}

/// C: `int sqlite3_stmt_scanstatus_v2( sqlite3_stmt *pStmt, int iScan, int iScanStatusOp, int flags, void *pOut )`
/// (SQLITE_ENABLE_STMT_SCANSTATUS is not part of the build: returns 1, "no
/// data".)
pub fn sqlite3_stmt_scanstatus_v2(
    pStmt: &mut Vdbe,
    iScan: i32,
    iScanStatusOp: i32,
    flags: i32,
    pOut: &mut [u8],
) -> i32 {
    let _ = (pStmt, iScan, iScanStatusOp, flags, pOut);
    1
}

/// C: `int sqlite3_stmt_scanstatus( sqlite3_stmt *pStmt, int iScan, int iScanStatusOp, void *pOut )`
pub fn sqlite3_stmt_scanstatus(
    pStmt: &mut Vdbe,
    iScan: i32,
    iScanStatusOp: i32,
    pOut: &mut [u8],
) -> i32 {
    sqlite3_stmt_scanstatus_v2(pStmt, iScan, iScanStatusOp, 0, pOut)
}

/// C: `void sqlite3_stmt_scanstatus_reset(sqlite3_stmt *pStmt)`
/// (no scan status counters in this build.)
pub fn sqlite3_stmt_scanstatus_reset(db: &mut Sqlite3, pStmt: StmtId) {
    let _ = (db, pStmt);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_type_table() {
        let mut m = Mem::default();
        m.flags = MEM_Null;
        assert_eq!(sqlite3_value_type(&mut m), SQLITE_NULL);
        m.flags = MEM_Int | MEM_Str;
        assert_eq!(sqlite3_value_type(&mut m), SQLITE_INTEGER);
        m.flags = MEM_IntReal;
        assert_eq!(sqlite3_value_type(&mut m), SQLITE_FLOAT);
        m.flags = MEM_Blob;
        assert_eq!(sqlite3_value_type(&mut m), SQLITE_BLOB);
        m.flags = MEM_Str | MEM_Term;
        assert_eq!(sqlite3_value_type(&mut m), SQLITE_TEXT);
    }

    #[test]
    fn value_dup_and_text16() {
        let mut m = Mem::default();
        m.flags = MEM_Null;
        memSetStrNoDb(&mut m, Some(b"ab"), 2, SQLITE_UTF8);
        let mut d = sqlite3_value_dup(&m).unwrap();
        assert_eq!(sqlite3_value_text(&mut d), Some(&b"ab"[..]));
        assert_eq!(
            sqlite3_value_text16le(&mut d).as_deref(),
            Some(&[b'a', 0, b'b', 0][..])
        );
        assert_eq!(sqlite3_value_bytes16(&mut d), 4);
    }
}
