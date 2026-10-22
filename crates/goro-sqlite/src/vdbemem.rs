//! Port of vdbemem.c: manipulation of "Mem" structures.  A "Mem" stores a
//! single value in the VDBE.  Interface routines refer to a Mem using the
//! name sqlite3_value.
//!
//! Rust representation (see PORTING.md and int.rs):
//!
//! * the content of strings and blobs is owned by `Mem::z`; `z.len()` plays
//!   the role of the C `szMalloc` (size of the buffer) and `z[..n]` is the
//!   value.  MEM_Ephem / MEM_Static are kept as flags for fidelity (they
//!   mean "z would not point into zMalloc in C").
//! * MEM_Dyn is only used for pointer values (sqlite3_bind_pointer /
//!   sqlite3_result_pointer: `Mem::pPtr`) and RowSets (`Mem::pPtr` holds an
//!   `Rc<RefCell<RowSet>>`).  The C destructor call is the drop of the Rc.
//! * the aggregate context of a MEM_Agg cell is `Mem::pAgg`.  Releasing a
//!   MEM_Agg cell calls the xFinalize method only when a connection is
//!   available: use sqlite3VdbeMemReleaseDb() / sqlite3VdbeMemSetNullDb()
//!   for that (the db-less functions just drop the context).
//! * Mem has no `db` back pointer.  Where the C code reads `pMem->db`
//!   (length limit, nFpDigit, encoding translation) the Rust function takes
//!   `db`; db-less variants use the C defaults for a Mem with `db==0`
//!   (SQLITE_MAX_LENGTH, 17 significant digits, no UTF-16 translation).

use crate::int::*;
use crate::printf::{PfArg, sqlite3_str_appendf, sqlite3StrAccumInit};
use std::cell::RefCell;
use std::rc::Rc;

/// C: `int sqlite3VdbeCheckMemInvariants(Mem *p)` (SQLITE_DEBUG only).
pub fn sqlite3VdbeCheckMemInvariants(db: &mut Sqlite3, p: &mut Mem) -> i32 {
    let _ = db;
    checkMemInvariants(p)
}

fn checkMemInvariants(p: &Mem) -> i32 {
    /* Cannot have more than one of MEM_Int, MEM_Real, or MEM_IntReal */
    let x = p.flags & (MEM_Int | MEM_Real | MEM_IntReal);
    debug_assert!((x & x.wrapping_sub(1)) == 0);
    if p.flags & MEM_Null != 0 {
        /* Cannot be both MEM_Null and some other type */
        debug_assert!((p.flags & (MEM_Int | MEM_Real | MEM_Str | MEM_Blob | MEM_Agg)) == 0);
    } else {
        /* The MEM_Cleared bit is only allowed on NULLs */
        debug_assert!((p.flags & MEM_Cleared) == 0);
    }
    1
}

/// Default number of significant digits for a Mem without a connection.
const DFLT_FP_DIGITS: u8 = 17;

#[inline]
fn fpDigits(db: &Sqlite3) -> u8 {
    if db.nFpDigit == 0 {
        DFLT_FP_DIGITS
    } else {
        db.nFpDigit
    }
}

/// Render a Mem object which is one of MEM_Int, MEM_Real, or MEM_IntReal
/// into p.z (which must hold at least sz bytes).
fn vdbeMemRenderNum(sz: i32, p: &mut Mem, nFpDigit: u8) {
    debug_assert!(p.flags & (MEM_Int | MEM_Real | MEM_IntReal) != 0);
    debug_assert!(sz > 22);
    let out: Vec<u8> = if p.flags & (MEM_Int | MEM_IntReal) != 0 {
        /* sqlite3Int64ToText() */
        let mut o = p.u.i.to_string().into_bytes();
        if p.flags & MEM_IntReal != 0 {
            o.extend_from_slice(b".0");
        }
        o
    } else {
        let mut acc = StrAccum::default();
        sqlite3StrAccumInit(&mut acc, sz, 0);
        sqlite3_str_appendf(
            &mut acc,
            b"%!.*g",
            &[PfArg::Int(nFpDigit as i64), PfArg::Dbl(p.u.r)],
        );
        acc.zText
    };
    let len = out.len();
    if p.z.len() < len + 1 {
        p.z.resize(len + 1, 0);
    }
    p.z[..len].copy_from_slice(&out);
    p.z[len] = 0;
    p.n = len as i32;
}

/// C: `int sqlite3VdbeMemValidStrRep(Mem *p)` (SQLITE_DEBUG only).
///
/// Check that the string value of pMem agrees with its integer or real
/// value.
pub fn sqlite3VdbeMemValidStrRep(db: &mut Sqlite3, p: &mut Mem) -> i32 {
    if (p.flags & MEM_Str) == 0 {
        return 1;
    }
    if (p.flags & (MEM_Int | MEM_Real | MEM_IntReal)) == 0 {
        return 1;
    }
    let mut tmp = Mem::default();
    tmp.flags = p.flags;
    tmp.u.i = p.u.i;
    tmp.u.r = p.u.r;
    tmp.z = vec![0; 100];
    vdbeMemRenderNum(100, &mut tmp, fpDigits(db));
    let zBuf = tmp.bytes();
    let z = p.bytes();
    let (mut i, incr) = if p.enc != SQLITE_UTF8 {
        (if p.enc == SQLITE_UTF16BE { 1 } else { 0 }, 2)
    } else {
        (0usize, 1usize)
    };
    for &c in zBuf {
        if z.get(i).copied().unwrap_or(0) != c {
            return 0;
        }
        i += incr;
    }
    1
}

/// Encoding change.  `db` is needed to translate between UTF-8 and UTF-16;
/// without it (a Mem with no connection) only no-op changes succeed (as
/// with SQLITE_OMIT_UTF16).
fn changeEncoding(db: Option<&mut Sqlite3>, pMem: &mut Mem, desiredEnc: u8) -> i32 {
    debug_assert!(
        desiredEnc == SQLITE_UTF8 || desiredEnc == SQLITE_UTF16LE || desiredEnc == SQLITE_UTF16BE
    );
    if (pMem.flags & MEM_Str) == 0 {
        pMem.enc = desiredEnc;
        return SQLITE_OK;
    }
    if pMem.enc == desiredEnc {
        return SQLITE_OK;
    }
    match db {
        /* MemTranslate() may return SQLITE_OK or SQLITE_NOMEM. If NOMEM is
         ** returned, then the encoding of the value may not have changed. */
        Some(db) => crate::utf::sqlite3VdbeMemTranslate(db, pMem, desiredEnc),
        None => SQLITE_ERROR,
    }
}

/// C: `int sqlite3VdbeChangeEncoding(Mem *pMem, int desiredEnc)`
///
/// If pMem is an object with a valid string representation, this routine
/// ensures the internal encoding for the string representation is
/// 'desiredEnc', one of SQLITE_UTF8, SQLITE_UTF16LE or SQLITE_UTF16BE.
pub fn sqlite3VdbeChangeEncoding(db: &mut Sqlite3, pMem: &mut Mem, desiredEnc: i32) -> i32 {
    changeEncoding(Some(db), pMem, desiredEnc as u8)
}

/// Make sure pMem->z points to a writable allocation of at least n bytes.
/// If bPreserve is true, the content of pMem->z is kept.
fn memGrow(pMem: &mut Mem, n: i32, bPreserve: i32) -> i32 {
    debug_assert!(bPreserve == 0 || pMem.flags & (MEM_Blob | MEM_Str) != 0);
    let n = n.max(0) as usize;
    if bPreserve == 0 {
        pMem.z.clear();
    }
    if pMem.z.len() < n {
        pMem.z.resize(n, 0);
    }
    if (pMem.flags & MEM_Dyn) != 0 {
        /* pMem->xDel((void *)(pMem->z)) */
        pMem.pPtr = None;
    }
    pMem.flags &= !(MEM_Dyn | MEM_Ephem | MEM_Static);
    SQLITE_OK
}

/// C: `SQLITE_NOINLINE int sqlite3VdbeMemGrow(Mem *pMem, int n, int bPreserve)`
pub fn sqlite3VdbeMemGrow(db: &mut Sqlite3, pMem: &mut Mem, n: i32, bPreserve: i32) -> i32 {
    let _ = db;
    memGrow(pMem, n, bPreserve)
}

/// C: `int sqlite3VdbeMemClearAndResize(Mem *pMem, int szNew)`
///
/// Change the pMem->zMalloc allocation to be at least szNew bytes.  Any
/// prior string or blob content may be discarded.  MEM_Int, MEM_Real,
/// MEM_IntReal and MEM_Null values are preserved.
pub fn sqlite3VdbeMemClearAndResize(pMem: &mut Mem, szNew: i32) -> i32 {
    if (pMem.z.len() as i64) < szNew as i64 || (pMem.flags & MEM_Dyn) != 0 {
        return memGrow(pMem, szNew, 0);
    }
    pMem.flags &= MEM_Null | MEM_Int | MEM_Real | MEM_IntReal;
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemZeroTerminateIfAble(Mem *pMem)`
///
/// If pMem is already a string, detect if it is a zero-terminated string,
/// or make it into one if possible, and mark it as such.  Return true if
/// the string is zero-terminated after this routine is called.
pub fn sqlite3VdbeMemZeroTerminateIfAble(pMem: &mut Mem) -> i32 {
    if (pMem.flags & (MEM_Str | MEM_Term | MEM_Ephem | MEM_Static)) != MEM_Str {
        /* pMem must be a string, and it cannot be an ephemeral or static
         ** string */
        return 0;
    }
    if pMem.enc != SQLITE_UTF8 {
        return 0;
    }
    if (pMem.flags & MEM_Dyn) != 0 {
        return 0;
    }
    let n = pMem.n.max(0) as usize;
    if pMem.z.len() >= n + 1 {
        pMem.z[n] = 0;
        pMem.flags |= MEM_Term;
        return 1;
    }
    0
}

/// It is already known that pMem contains an unterminated string.  Add the
/// zero terminator (three bytes of zero, for UTF-16).
fn vdbeMemAddTerminator(pMem: &mut Mem) -> i32 {
    let n = pMem.n.max(0) as usize;
    if memGrow(pMem, (n + 3) as i32, 1) != 0 {
        return SQLITE_NOMEM;
    }
    pMem.z[n] = 0;
    pMem.z[n + 1] = 0;
    pMem.z[n + 2] = 0;
    pMem.flags |= MEM_Term;
    SQLITE_OK
}

fn memMakeWriteable(pMem: &mut Mem) -> i32 {
    if (pMem.flags & (MEM_Str | MEM_Blob)) != 0 {
        if ExpandBlob(pMem) != 0 {
            return SQLITE_NOMEM;
        }
        if pMem.z.is_empty() || (pMem.flags & (MEM_Ephem | MEM_Static)) != 0 {
            let rc = vdbeMemAddTerminator(pMem);
            if rc != 0 {
                return rc;
            }
        }
    }
    pMem.flags &= !MEM_Ephem;
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemMakeWriteable(Mem *pMem)`
///
/// Change pMem so that its MEM_Str or MEM_Blob value is stored in
/// MEM.zMalloc, where it can be safely written.
pub fn sqlite3VdbeMemMakeWriteable(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memMakeWriteable(pMem)
}

/// The C macro ExpandBlob(P): expand a MEM_Zero blob.
#[inline]
pub fn ExpandBlob(pMem: &mut Mem) -> i32 {
    if pMem.flags & MEM_Zero != 0 {
        memExpandBlob(pMem)
    } else {
        0
    }
}

fn memExpandBlob(pMem: &mut Mem) -> i32 {
    debug_assert!(pMem.flags & MEM_Zero != 0);
    debug_assert!((pMem.flags & MEM_Blob) != 0 || pMem.MemNullNochng());

    /* Set nByte to the number of bytes required to store the expanded
     ** blob. */
    let mut nByte = pMem.n + pMem.u.nZero;
    if nByte <= 0 {
        if (pMem.flags & MEM_Blob) == 0 {
            return SQLITE_OK;
        }
        nByte = 1;
    }
    if memGrow(pMem, nByte, 1) != 0 {
        return SQLITE_NOMEM;
    }
    let n = pMem.n.max(0) as usize;
    let nz = pMem.u.nZero.max(0) as usize;
    if pMem.z.len() < n + nz {
        pMem.z.resize(n + nz, 0);
    }
    pMem.z[n..n + nz].fill(0);
    pMem.n += pMem.u.nZero;
    pMem.flags &= !(MEM_Zero | MEM_Term);
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemExpandBlob(Mem *pMem)`
///
/// If the given Mem* has a zero-filled tail, turn it into an ordinary blob
/// stored in dynamically allocated space.
pub fn sqlite3VdbeMemExpandBlob(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memExpandBlob(pMem)
}

fn memNulTerminate(pMem: &mut Mem) -> i32 {
    if (pMem.flags & (MEM_Term | MEM_Str)) != MEM_Str {
        SQLITE_OK /* Nothing to do */
    } else {
        vdbeMemAddTerminator(pMem)
    }
}

/// C: `int sqlite3VdbeMemNulTerminate(Mem *pMem)`
///
/// Make sure the given Mem is \u0000 terminated.
pub fn sqlite3VdbeMemNulTerminate(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memNulTerminate(pMem)
}

fn memStringify(db: Option<&mut Sqlite3>, nFp: u8, pMem: &mut Mem, enc: u8, bForce: u8) -> i32 {
    const nByte: i32 = 32;
    debug_assert!(pMem.flags & MEM_Zero == 0);
    debug_assert!(pMem.flags & (MEM_Str | MEM_Blob) == 0);
    debug_assert!(pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal) != 0);

    if sqlite3VdbeMemClearAndResize(pMem, nByte) != 0 {
        pMem.enc = 0;
        return SQLITE_NOMEM;
    }

    vdbeMemRenderNum(nByte, pMem, nFp);
    pMem.enc = SQLITE_UTF8;
    pMem.flags |= MEM_Str | MEM_Term;
    if bForce != 0 {
        pMem.flags &= !(MEM_Int | MEM_Real | MEM_IntReal);
    }
    changeEncoding(db, pMem, enc);
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemStringify(Mem *pMem, u8 enc, u8 bForce)`
///
/// Add MEM_Str to the set of representations for the given Mem.  This
/// routine is only called if pMem is a number of some kind, not a NULL or a
/// BLOB.  Existing representations MEM_Int, MEM_Real, or MEM_IntReal are
/// invalidated if bForce is true but are retained if bForce is false.
pub fn sqlite3VdbeMemStringify(db: &mut Sqlite3, pMem: &mut Mem, enc: u8, bForce: u8) -> i32 {
    let nFp = fpDigits(db);
    memStringify(Some(db), nFp, pMem, enc, bForce)
}

/// C: `int sqlite3VdbeMemFinalize(Mem *pMem, FuncDef *pFunc)`
///
/// Memory cell pMem contains the context of an aggregate function.  This
/// routine calls the finalize method for that function.  The result of the
/// aggregate is stored back into pMem.
///
/// Return SQLITE_ERROR if the finalizer reports an error.  SQLITE_OK
/// otherwise.
pub fn sqlite3VdbeMemFinalize(db: &mut Sqlite3, pMem: &mut Mem, pFunc: &FuncDefRef) -> i32 {
    debug_assert!(pFunc.xFinalize.is_some());
    let mut t = Mem::default();
    t.flags = MEM_Null;
    let enc = db.ENC();
    let isError;
    {
        let mut ctx = Context {
            db,
            pOut: &mut t,
            pFunc: pFunc.clone(),
            pMem: Some(&mut *pMem),
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
        };
        if let Some(xFinalize) = pFunc.xFinalize.clone() {
            xFinalize(&mut ctx); /* IMP: R-24505-23230 */
        }
        isError = ctx.isError;
    }
    /* memcpy(pMem, &t, sizeof(t)) */
    *pMem = t;
    isError
}

/// C: `int sqlite3VdbeMemAggValue(Mem *pAccum, Mem *pOut, FuncDef *pFunc)`
///
/// Memory cell pAccum contains the context of an aggregate function.  This
/// routine calls the xValue method for that function and stores the
/// results in memory cell pOut.
pub fn sqlite3VdbeMemAggValue(
    db: &mut Sqlite3,
    pAccum: &mut Mem,
    pOut: &mut Mem,
    pFunc: &FuncDefRef,
) -> i32 {
    debug_assert!(pFunc.xValue.is_some());
    sqlite3VdbeMemSetNull(pOut);
    let enc = db.ENC();
    let mut ctx = Context {
        db,
        pOut,
        pFunc: pFunc.clone(),
        pMem: Some(pAccum),
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
    };
    if let Some(xValue) = pFunc.xValue.clone() {
        xValue(&mut ctx);
    }
    ctx.isError
}

/// If the memory cell contains a value that must be freed (pointer value,
/// RowSet, aggregate context), free it and set the Mem to NULL.
///
/// Without a connection the aggregate context is simply dropped (the C
/// code would call xFinalize; see vdbeMemClearExternAndSetNullDb()).
fn vdbeMemClearExternAndSetNull(p: &mut Mem) {
    debug_assert!(p.VdbeMemDynamic());
    if p.flags & MEM_Agg != 0 {
        p.pAgg = None;
        p.u.pDef = None;
    }
    if p.flags & MEM_Dyn != 0 {
        p.pPtr = None;
    }
    p.flags = MEM_Null;
}

/// Like vdbeMemClearExternAndSetNull() but calls the xFinalize method of an
/// aggregate as C does.
fn vdbeMemClearExternAndSetNullDb(db: &mut Sqlite3, p: &mut Mem) {
    debug_assert!(p.VdbeMemDynamic());
    if p.flags & MEM_Agg != 0 {
        if let Some(pDef) = p.u.pDef.clone() {
            if pDef.xFinalize.is_some() {
                sqlite3VdbeMemFinalize(db, p, &pDef);
            }
        }
        debug_assert!((p.flags & MEM_Agg) == 0 || p.u.pDef.is_none());
    }
    vdbeMemClearExternAndSetNull_rest(p);
}

fn vdbeMemClearExternAndSetNull_rest(p: &mut Mem) {
    p.pAgg = None;
    if p.flags & MEM_Dyn != 0 {
        p.pPtr = None;
    }
    p.flags = MEM_Null;
}

/// Release memory held by the Mem p.
fn vdbeMemClear(p: &mut Mem) {
    if p.VdbeMemDynamic() {
        vdbeMemClearExternAndSetNull(p);
    }
    p.z = Vec::new();
}

/// C: `void sqlite3VdbeMemRelease(Mem *p)`
///
/// Release any memory resources held by the Mem.  The aggregate context of
/// a MEM_Agg cell is dropped without calling xFinalize; use
/// sqlite3VdbeMemReleaseDb() to get the C behaviour.
pub fn sqlite3VdbeMemRelease(p: &mut Mem) {
    debug_assert!(checkMemInvariants(p) != 0);
    if p.VdbeMemDynamic() || !p.z.is_empty() {
        vdbeMemClear(p);
    }
    p.pPtr = None;
}

/// sqlite3VdbeMemRelease() with access to the connection: a MEM_Agg cell
/// is finalized (xFinalize is invoked) as in C.
pub fn sqlite3VdbeMemReleaseDb(db: &mut Sqlite3, p: &mut Mem) {
    if p.VdbeMemDynamic() {
        vdbeMemClearExternAndSetNullDb(db, p);
    }
    sqlite3VdbeMemRelease(p);
}

/// C: `void sqlite3VdbeMemReleaseMalloc(Mem *p)`
///
/// Like sqlite3VdbeMemRelease() but for cases where we know in advance
/// that the Mem is not MEM_Dyn or MEM_Agg.
pub fn sqlite3VdbeMemReleaseMalloc(p: &mut Mem) {
    debug_assert!(!p.VdbeMemDynamic());
    if !p.z.is_empty() {
        vdbeMemClear(p);
    }
}

/// C: `i64 sqlite3VdbeIntValue(const Mem *pMem)`
///
/// Return some kind of integer value which is the best we can do at
/// representing the value that *pMem describes as an integer.
pub fn sqlite3VdbeIntValue(db: &mut Sqlite3, pMem: &Mem) -> i64 {
    let _ = db;
    memIntValueAny(pMem)
}

/// sqlite3VdbeIntValue() without a connection.
pub fn memIntValueAny(pMem: &Mem) -> i64 {
    let flags = pMem.flags;
    if flags & (MEM_Int | MEM_IntReal) != 0 {
        pMem.u.i
    } else if flags & MEM_Real != 0 {
        sqlite3RealToI64(pMem.u.r)
    } else if (flags & (MEM_Str | MEM_Blob)) != 0 && !pMem.z.is_empty() {
        memIntValue(pMem)
    } else {
        0
    }
}

fn memIntValue(pMem: &Mem) -> i64 {
    let mut value = 0i64;
    let z = pMem.bytes();
    crate::util::sqlite3Atoi64(Some(z), &mut value, z.len() as i32, pMem.enc);
    value
}

/// Slow path of sqlite3MemRealValueRC() for strings that are not UTF-8 or
/// that are not zero-terminated.
fn sqlite3MemRealValueRCSlowPath(pMem: &Mem, pValue: &mut f64) -> i32 {
    *pValue = 0.0;
    if pMem.enc == SQLITE_UTF8 {
        let zCopy = pMem.bytes().to_vec();
        crate::util::sqlite3AtoF(Some(&zCopy), pValue)
    } else {
        let z = pMem.bytes();
        let n = (pMem.n & !1).max(0) as usize;
        let mut zCopy: Vec<u8> = Vec::with_capacity(n / 2 + 2);
        let mut i = 0usize;
        let at = |k: usize| z.get(k).copied().unwrap_or(0);
        if pMem.enc == SQLITE_UTF16LE {
            while i + 1 < n {
                zCopy.push(at(i));
                if at(i + 1) != 0 {
                    break;
                }
                i += 2;
            }
        } else {
            while i + 1 < n {
                if at(i) != 0 {
                    break;
                }
                zCopy.push(at(i + 1));
                i += 2;
            }
        }
        let mut rc = crate::util::sqlite3AtoF(Some(&zCopy), pValue);
        if i < n {
            rc = -100;
        }
        rc
    }
}

fn memRealValueRC(pMem: &mut Mem, pValue: &mut f64) -> i32 {
    debug_assert!(pMem.flags & (MEM_Str | MEM_Blob) != 0);
    if pMem.z.is_empty() {
        *pValue = 0.0;
        0
    } else if pMem.enc == SQLITE_UTF8
        && ((pMem.flags & MEM_Term) != 0 || sqlite3VdbeMemZeroTerminateIfAble(pMem) != 0)
    {
        crate::util::sqlite3AtoF(Some(pMem.bytes()), pValue)
    } else if pMem.n == 0 {
        *pValue = 0.0;
        0
    } else {
        sqlite3MemRealValueRCSlowPath(pMem, pValue)
    }
}

/// C: `int sqlite3MemRealValueRC(Mem *pMem, double *pValue)`
///
/// Invoke sqlite3AtoF() on the text value of pMem.  Write the translation
/// of the text input into *pValue.
///
///    rc==0         =>   ERROR: Input string not well-formed, or OOM
///    rc<0          =>   Some prefix of the input is well-formed
///    rc>0          =>   All of the input is well-formed
///    (rc&2)==0     =>   The number is expressed as an integer, with no
///                       decimal point or eNNN suffix.
pub fn sqlite3MemRealValueRC(db: &mut Sqlite3, pMem: &mut Mem, pValue: &mut f64) -> i32 {
    let _ = db;
    memRealValueRC(pMem, pValue)
}

/// C: `double sqlite3VdbeRealValue(Mem *pMem)`
///
/// Return the best representation of pMem that we can get into a double.
pub fn sqlite3VdbeRealValue(db: &mut Sqlite3, pMem: &mut Mem) -> f64 {
    let _ = db;
    memRealValueAny(pMem)
}

/// sqlite3VdbeRealValue() without a connection.
pub fn memRealValueAny(pMem: &mut Mem) -> f64 {
    if pMem.flags & MEM_Real != 0 {
        pMem.u.r
    } else if pMem.flags & (MEM_Int | MEM_IntReal) != 0 {
        pMem.u.i as f64
    } else if pMem.flags & (MEM_Str | MEM_Blob) != 0 {
        let mut r = 0.0;
        memRealValueRC(pMem, &mut r);
        r
    } else {
        0.0
    }
}

/// C: `int sqlite3VdbeBooleanValue(Mem *pMem, int ifNull)`
///
/// Return 1 if pMem represents true, and return 0 if pMem represents
/// false.  Return the value ifNull if pMem is NULL.
pub fn sqlite3VdbeBooleanValue(pMem: &mut Mem, ifNull: i32) -> i32 {
    if pMem.flags & (MEM_Int | MEM_IntReal) != 0 {
        return (pMem.u.i != 0) as i32;
    }
    if pMem.flags & MEM_Null != 0 {
        return ifNull;
    }
    (memRealValueAny(pMem) != 0.0) as i32
}

/// sqlite3VdbeIntegerAffinity() without a connection.
pub fn memIntegerAffinity(pMem: &mut Mem) {
    debug_assert!(pMem.flags & (MEM_Real | MEM_IntReal) != 0);
    if pMem.flags & MEM_IntReal != 0 {
        pMem.MemSetTypeFlag(MEM_Int);
    } else {
        let ix = sqlite3RealToI64(pMem.u.r);

        /* Only mark the value as an integer if
         **
         **    (1) the round-trip conversion real->int->real is a no-op, and
         **    (2) The integer is neither the largest nor the smallest
         **        possible integer (ticket #3922)
         */
        if pMem.u.r == ix as f64 && ix > SMALLEST_INT64 && ix < LARGEST_INT64 {
            pMem.u.i = ix;
            pMem.MemSetTypeFlag(MEM_Int);
        }
    }
}

/// C: `void sqlite3VdbeIntegerAffinity(Mem *pMem)`
///
/// The MEM structure is already a MEM_Real or MEM_IntReal.  Try to make it
/// a MEM_Int if we can.
pub fn sqlite3VdbeIntegerAffinity(db: &mut Sqlite3, pMem: &mut Mem) {
    let _ = db;
    memIntegerAffinity(pMem)
}

/// sqlite3VdbeMemIntegerify() without a connection.
pub fn memIntegerify(pMem: &mut Mem) -> i32 {
    pMem.u.i = memIntValueAny(pMem);
    pMem.MemSetTypeFlag(MEM_Int);
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemIntegerify(Mem *pMem)`
///
/// Convert pMem to type integer.  Invalidate any prior representations.
pub fn sqlite3VdbeMemIntegerify(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memIntegerify(pMem)
}

/// sqlite3VdbeMemRealify() without a connection.
pub fn memRealify(pMem: &mut Mem) -> i32 {
    pMem.u.r = memRealValueAny(pMem);
    pMem.MemSetTypeFlag(MEM_Real);
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemRealify(Mem *pMem)`
///
/// Convert pMem so that it is of type MEM_Real.  Invalidate any prior
/// representations.
pub fn sqlite3VdbeMemRealify(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memRealify(pMem)
}

/// C: `int sqlite3RealSameAsInt(double r1, sqlite3_int64 i)`
///
/// Compare a floating point value to an integer.  Return true if the two
/// values are the same within the precision of the floating point value.
pub fn sqlite3RealSameAsInt(r1: f64, i: i64) -> i32 {
    let r2 = i as f64;
    (r1 == 0.0
        || (r1.to_bits() == r2.to_bits() && i >= -2251799813685248i64 && i < 2251799813685248i64))
        as i32
}

/// C: `i64 sqlite3RealToI64(double r)`
///
/// Convert a floating point value to its closest integer.
pub fn sqlite3RealToI64(r: f64) -> i64 {
    if r < -9223372036854774784.0 {
        return SMALLEST_INT64;
    }
    if r > 9223372036854774784.0 {
        return LARGEST_INT64;
    }
    r as i64
}

/// sqlite3VdbeMemNumerify() without a connection.
pub fn memNumerify(pMem: &mut Mem) -> i32 {
    if (pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal | MEM_Null)) == 0 {
        debug_assert!((pMem.flags & (MEM_Blob | MEM_Str)) != 0);
        let mut r = 0.0f64;
        let rc = memRealValueRC(pMem, &mut r);
        pMem.u.r = r;
        let mut ix: i64 = 0;
        let isInt = if (rc & 2) == 0 && {
            let z = pMem.bytes();
            crate::util::sqlite3Atoi64(Some(z), &mut ix, z.len() as i32, pMem.enc) < 2
        } {
            true
        } else {
            ix = sqlite3RealToI64(pMem.u.r);
            sqlite3RealSameAsInt(pMem.u.r, ix) != 0
        };
        if isInt {
            pMem.u.i = ix;
            pMem.MemSetTypeFlag(MEM_Int);
        } else {
            pMem.MemSetTypeFlag(MEM_Real);
        }
    }
    debug_assert!((pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal | MEM_Null)) != 0);
    pMem.flags &= !(MEM_Str | MEM_Blob | MEM_Zero);
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemNumerify(Mem *pMem)`
///
/// Convert pMem so that it has type MEM_Real or MEM_Int.  Invalidate any
/// prior representations.  Every effort is made to force the conversion,
/// even if the input is a string that does not look completely like a
/// number.
pub fn sqlite3VdbeMemNumerify(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    let _ = db;
    memNumerify(pMem)
}

/// C: `int sqlite3VdbeMemCast(Mem *pMem, u8 aff, u8 encoding)`
///
/// Cast the datatype of the value in pMem according to the affinity "aff".
/// Casting is different from applying affinity in that a cast is forced.
pub fn sqlite3VdbeMemCast(db: &mut Sqlite3, pMem: &mut Mem, aff: u8, encoding: u8) -> i32 {
    if pMem.flags & MEM_Null != 0 {
        return SQLITE_OK;
    }
    match aff {
        SQLITE_AFF_BLOB => {
            /* Really a cast to BLOB */
            if (pMem.flags & MEM_Blob) == 0 {
                crate::vdbe::sqlite3ValueApplyAffinity(db, pMem, SQLITE_AFF_TEXT, encoding);
                if pMem.flags & MEM_Str != 0 {
                    pMem.MemSetTypeFlag(MEM_Blob);
                }
            } else {
                pMem.flags &= !(MEM_TypeMask & !MEM_Blob);
            }
        }
        SQLITE_AFF_NUMERIC => {
            memNumerify(pMem);
        }
        SQLITE_AFF_INTEGER => {
            memIntegerify(pMem);
        }
        SQLITE_AFF_REAL => {
            memRealify(pMem);
        }
        _ => {
            debug_assert!(aff == SQLITE_AFF_TEXT);
            debug_assert!(MEM_Str == (MEM_Blob >> 3));
            pMem.flags |= (pMem.flags & MEM_Blob) >> 3;
            crate::vdbe::sqlite3ValueApplyAffinity(db, pMem, SQLITE_AFF_TEXT, encoding);
            pMem.flags &= !(MEM_Int | MEM_Real | MEM_IntReal | MEM_Blob | MEM_Zero);
            if encoding != SQLITE_UTF8 {
                pMem.n &= !1;
            }
            let rc = changeEncoding(Some(db), pMem, encoding);
            if rc != 0 {
                return rc;
            }
            sqlite3VdbeMemZeroTerminateIfAble(pMem);
        }
    }
    SQLITE_OK
}

/// C: `void sqlite3VdbeMemInit(Mem *pMem, sqlite3 *db, u16 flags)`
///
/// Initialize bulk memory to be a consistent Mem object.
pub fn sqlite3VdbeMemInit(db: &mut Sqlite3, pMem: &mut Mem, flags: u16) {
    let _ = db;
    debug_assert!((flags & !MEM_TypeMask) == 0);
    pMem.flags = flags;
    pMem.z = Vec::new();
}

/// C: `void sqlite3VdbeMemSetNull(Mem *pMem)`
///
/// Delete any previous value and set the value stored in *pMem to NULL.
/// The buffer (C zMalloc) is preserved.  The aggregate context of a
/// MEM_Agg cell is dropped without calling xFinalize (see
/// sqlite3VdbeMemSetNullDb()).
pub fn sqlite3VdbeMemSetNull(pMem: &mut Mem) {
    if pMem.VdbeMemDynamic() {
        vdbeMemClearExternAndSetNull(pMem);
    } else {
        pMem.flags = MEM_Null;
    }
    pMem.pPtr = None;
}

/// sqlite3VdbeMemSetNull() with access to the connection: a MEM_Agg cell
/// is finalized (xFinalize is invoked) as in C.
pub fn sqlite3VdbeMemSetNullDb(db: &mut Sqlite3, pMem: &mut Mem) {
    if pMem.VdbeMemDynamic() {
        vdbeMemClearExternAndSetNullDb(db, pMem);
    } else {
        pMem.flags = MEM_Null;
    }
    pMem.pPtr = None;
}

/// C: `void sqlite3ValueSetNull(sqlite3_value *p)`
pub fn sqlite3ValueSetNull(p: &mut Mem) {
    sqlite3VdbeMemSetNull(p);
}

/// C: `void sqlite3VdbeMemSetZeroBlob(Mem *pMem, int n)`
///
/// Delete any previous value and set the value to be a BLOB of length n
/// containing all zeros.
pub fn sqlite3VdbeMemSetZeroBlob(pMem: &mut Mem, n: i32) {
    sqlite3VdbeMemRelease(pMem);
    pMem.flags = MEM_Blob | MEM_Zero;
    pMem.n = 0;
    pMem.u.nZero = if n < 0 { 0 } else { n };
    pMem.enc = SQLITE_UTF8;
    pMem.z = Vec::new();
}

/// C: `void sqlite3VdbeMemSetInt64(Mem *pMem, i64 val)`
///
/// Delete any previous value and set the value stored in *pMem to val,
/// manifest type INTEGER.
pub fn sqlite3VdbeMemSetInt64(pMem: &mut Mem, val: i64) {
    if pMem.VdbeMemDynamic() {
        /* vdbeReleaseAndSetInt64() */
        sqlite3VdbeMemSetNull(pMem);
    }
    pMem.u.i = val;
    pMem.flags = MEM_Int;
}

/// C: `void sqlite3MemSetArrayInt64(sqlite3_value *aMem, int iIdx, i64 val)`
///
/// Set the iIdx'th entry of array aMem[] to contain integer value val.
/// Rust: takes the array.
pub fn sqlite3MemSetArrayInt64(aMem: &mut [Mem], iIdx: i32, val: i64) {
    sqlite3VdbeMemSetInt64(&mut aMem[iIdx as usize], val);
}

/// C: `void sqlite3NoopDestructor(void *p)`: a no-op destructor.
pub fn sqlite3NoopDestructor(p: &mut [u8]) {
    let _ = p;
}

/// C: `void sqlite3VdbeMemSetPointer( Mem *pMem, void *pPtr, const char *zPType, void (*xDestructor)(void*) )`
///
/// Set the value stored in *pMem (which should already be a NULL) to be a
/// pointer of type zPType.  Rust: the pointer is an `Rc<dyn Any>`; the
/// destructor is its drop.
pub fn sqlite3VdbeMemSetPointer(
    pMem: &mut Mem,
    pPtr: Rc<dyn std::any::Any>,
    zPType: Option<&[u8]>,
) {
    debug_assert!(pMem.flags == MEM_Null);
    vdbeMemClear(pMem);
    let ty = String::from_utf8_lossy(zPType.unwrap_or(b"")).into_owned();
    pMem.u.zPType = Some(Rc::from(ty.as_str()));
    pMem.pPtr = Some(pPtr);
    pMem.flags = MEM_Null | MEM_Dyn | MEM_Subtype | MEM_Term;
    pMem.eSubtype = b'p';
}

/// C: `void sqlite3VdbeMemSetDouble(Mem *pMem, double val)`
///
/// Delete any previous value and set the value stored in *pMem to val,
/// manifest type REAL.
pub fn sqlite3VdbeMemSetDouble(pMem: &mut Mem, val: f64) {
    sqlite3VdbeMemSetNull(pMem);
    if !val.is_nan() {
        pMem.u.r = val;
        pMem.flags = MEM_Real;
    }
}

/// C: `int sqlite3VdbeMemIsRowSet(const Mem *pMem)`
///
/// Return true if the Mem holds a RowSet object.
pub fn sqlite3VdbeMemIsRowSet(pMem: &Mem) -> i32 {
    ((pMem.flags & (MEM_Blob | MEM_Dyn)) == (MEM_Blob | MEM_Dyn)
        && pMem
            .pPtr
            .as_ref()
            .map(|p| p.is::<RefCell<crate::rowset::RowSet>>())
            .unwrap_or(false)) as i32
}

/// The RowSet held by a Mem set with sqlite3VdbeMemSetRowSet() (C:
/// `(RowSet*)pMem->z`).
pub fn sqlite3VdbeMemRowSet(pMem: &Mem) -> Option<Rc<RefCell<crate::rowset::RowSet>>> {
    if sqlite3VdbeMemIsRowSet(pMem) == 0 {
        return None;
    }
    pMem.pPtr
        .clone()
        .and_then(|p| p.downcast::<RefCell<crate::rowset::RowSet>>().ok())
}

/// C: `int sqlite3VdbeMemSetRowSet(Mem *pMem)`
///
/// Delete any previous value and set the value of pMem to be an empty
/// boolean index.  The RowSet is stored in `pMem.pPtr` as an
/// `Rc<RefCell<RowSet>>` (see sqlite3VdbeMemRowSet()).
pub fn sqlite3VdbeMemSetRowSet(db: &mut Sqlite3, pMem: &mut Mem) -> i32 {
    debug_assert!(sqlite3VdbeMemIsRowSet(pMem) == 0);
    sqlite3VdbeMemRelease(pMem);
    let p = match crate::rowset::sqlite3RowSetInit(db) {
        Some(p) => p,
        None => return SQLITE_NOMEM,
    };
    let rs: Rc<RefCell<crate::rowset::RowSet>> = Rc::new(RefCell::new(*p));
    pMem.pPtr = Some(rs);
    pMem.flags = MEM_Blob | MEM_Dyn;
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemTooBig(Mem *p)`
///
/// Return true if the Mem object contains a TEXT or BLOB that is too large
/// - whose size exceeds SQLITE_MAX_LENGTH.
pub fn sqlite3VdbeMemTooBig(db: &mut Sqlite3, p: &mut Mem) -> i32 {
    memTooBig(db.aLimit[SQLITE_LIMIT_LENGTH as usize], p)
}

fn memTooBig(iLimit: i32, p: &Mem) -> i32 {
    if p.flags & (MEM_Str | MEM_Blob) != 0 {
        let mut n = p.n as i64;
        if p.flags & MEM_Zero != 0 {
            n += p.u.nZero as i64;
        }
        return (n > iLimit as i64) as i32;
    }
    0
}

/// C: `void sqlite3VdbeMemAboutToChange(Vdbe *pVdbe, Mem *pMem)`
/// (SQLITE_DEBUG only: shallow-copy tracking is not kept in this port).
pub fn sqlite3VdbeMemAboutToChange(db: &mut Sqlite3, pVdbe: &mut Vdbe, pMem: &mut Mem) {
    let _ = (db, pVdbe, pMem);
}

/// memcpy(pTo, pFrom, MEMCELLSIZE): copy the value part of a Mem (u, z, n,
/// flags, enc, eSubtype) plus the pointer value.
fn memCopyCell(pTo: &mut Mem, pFrom: &Mem) {
    pTo.u = pFrom.u.clone();
    pTo.z.clear();
    pTo.z.extend_from_slice(pFrom.bytes());
    if pFrom.flags & MEM_Term != 0 {
        pTo.z.push(0);
        if pFrom.enc != SQLITE_UTF8 {
            pTo.z.push(0);
        }
    }
    pTo.n = pFrom.n;
    pTo.flags = pFrom.flags;
    pTo.enc = pFrom.enc;
    pTo.eSubtype = pFrom.eSubtype;
    pTo.pPtr = pFrom.pPtr.clone();
}

/// C: `void sqlite3VdbeMemShallowCopy(Mem *pTo, const Mem *pFrom, int srcType)`
///
/// Make an shallow copy of pFrom into pTo.  Prior contents of pTo are
/// freed.  (Rust: the content is copied, but the flags are those of a C
/// shallow copy: srcType, either MEM_Ephem or MEM_Static.)
pub fn sqlite3VdbeMemShallowCopy(db: &mut Sqlite3, pTo: &mut Mem, pFrom: &Mem, srcType: i32) {
    let _ = db;
    memShallowCopy(pTo, pFrom, srcType as u16);
}

/// sqlite3VdbeMemShallowCopy() without a connection.
pub fn memShallowCopy(pTo: &mut Mem, pFrom: &Mem, srcType: u16) {
    debug_assert!(sqlite3VdbeMemIsRowSet(pFrom) == 0);
    if pTo.VdbeMemDynamic() {
        /* vdbeClrCopy() */
        vdbeMemClearExternAndSetNull(pTo);
    }
    memCopyCell(pTo, pFrom);
    if (pFrom.flags & MEM_Static) == 0 {
        pTo.flags &= !(MEM_Dyn | MEM_Static | MEM_Ephem);
        debug_assert!(srcType == MEM_Ephem || srcType == MEM_Static);
        pTo.flags |= srcType;
    }
}

/// C: `int sqlite3VdbeMemCopy(Mem *pTo, const Mem *pFrom)`
///
/// Make a full copy of pFrom into pTo.  Prior contents of pTo are freed
/// before the copy is made.
pub fn sqlite3VdbeMemCopy(pTo: &mut Mem, pFrom: &Mem) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(sqlite3VdbeMemIsRowSet(pFrom) == 0);
    if pTo.VdbeMemDynamic() {
        vdbeMemClearExternAndSetNull(pTo);
    }
    memCopyCell(pTo, pFrom);
    pTo.flags &= !MEM_Dyn;
    if pTo.flags & (MEM_Str | MEM_Blob) != 0 {
        if 0 == (pFrom.flags & MEM_Static) {
            pTo.flags |= MEM_Ephem;
            rc = memMakeWriteable(pTo);
        }
    }
    rc
}

/// C: `void sqlite3VdbeMemMove(Mem *pTo, Mem *pFrom)`
///
/// Transfer the contents of pFrom to pTo.  Any existing value in pTo is
/// freed.  pFrom contains an SQL NULL when this routine returns.
pub fn sqlite3VdbeMemMove(db: &mut Sqlite3, pTo: &mut Mem, pFrom: &mut Mem) {
    let _ = db;
    memMove(pTo, pFrom);
}

/// sqlite3VdbeMemMove() without a connection.
pub fn memMove(pTo: &mut Mem, pFrom: &mut Mem) {
    sqlite3VdbeMemRelease(pTo);
    *pTo = std::mem::take(pFrom);
    /* pFrom keeps its scalar fields (memcpy semantics) but loses its buffer */
    pFrom.u.i = pTo.u.i;
    pFrom.u.r = pTo.u.r;
    pFrom.u.nZero = pTo.u.nZero;
    pFrom.n = pTo.n;
    pFrom.enc = pTo.enc;
    pFrom.eSubtype = pTo.eSubtype;
    pFrom.uTemp = pTo.uTemp;
    pFrom.flags = MEM_Null;
}

/// strlen() of a C string held in a slice: up to the first NUL or the end.
#[inline]
fn cstrlen(z: &[u8]) -> usize {
    z.iter().position(|&c| c == 0).unwrap_or(z.len())
}

/// The core of sqlite3VdbeMemSetStr() (always the SQLITE_TRANSIENT
/// strategy: the bytes are copied into the Mem).  Returns
/// Err(()) when the string is longer than iLimit.
fn memSetStrCore(pMem: &mut Mem, z: &[u8], n: i64, mut enc: u8, iLimit: i32) -> Result<(), ()> {
    let mut nByte = n;
    let flags: u16;
    debug_assert!(sqlite3VdbeMemIsRowSet(pMem) == 0);
    debug_assert!(enc != 0 || n >= 0);
    if nByte < 0 {
        debug_assert!(enc != 0);
        if enc == SQLITE_UTF8 {
            nByte = cstrlen(z) as i64;
        } else {
            nByte = 0;
            while nByte <= iLimit as i64 {
                let a = z.get(nByte as usize).copied().unwrap_or(0);
                let b = z.get(nByte as usize + 1).copied().unwrap_or(0);
                if (a | b) == 0 {
                    break;
                }
                nByte += 2;
            }
            if nByte as usize > z.len() {
                nByte = z.len() as i64;
            }
        }
        flags = MEM_Str | MEM_Term;
    } else if enc == 0 {
        flags = MEM_Blob;
        enc = SQLITE_UTF8;
    } else {
        flags = MEM_Str;
    }
    if nByte as usize > z.len() {
        /* The C code would read past the end of the buffer */
        nByte = z.len() as i64;
    }
    if nByte > iLimit as i64 {
        sqlite3VdbeMemSetNull(pMem);
        return Err(());
    }

    /* SQLITE_TRANSIENT: copy the string into a buffer managed by the Mem */
    let mut nAlloc = nByte;
    if flags & MEM_Term != 0 {
        nAlloc += if enc == SQLITE_UTF8 { 1 } else { 2 };
    }
    sqlite3VdbeMemClearAndResize(pMem, nAlloc.max(32) as i32);
    let nb = nByte as usize;
    pMem.z[..nb].copy_from_slice(&z[..nb]);
    for k in nb..nAlloc as usize {
        pMem.z[k] = 0;
    }

    pMem.n = (nByte & 0x7fffffff) as i32;
    pMem.flags = flags;
    pMem.enc = enc;
    pMem.pPtr = None;
    Ok(())
}

/// C: `int sqlite3VdbeMemSetStr( Mem *pMem, const char *z, i64 n, u8 enc, void (*xDel)(void*) )`
///
/// Change the value of a Mem to be a string or a BLOB.  The string is
/// always copied (SQLITE_TRANSIENT).
///
/// If the string is too large (if it exceeds the SQLITE_LIMIT_LENGTH size
/// limit) then the Mem is set to NULL and SQLITE_TOOBIG is returned.
///
/// The "enc" parameter is the text encoding for the string, or zero to
/// store a blob.  If n is negative, then the string consists of all bytes
/// up to but excluding the first zero character.
pub fn sqlite3VdbeMemSetStr(
    db: &mut Sqlite3,
    pMem: &mut Mem,
    z: Option<&[u8]>,
    n: i64,
    enc: u8,
) -> i32 {
    /* If z is a NULL pointer, set pMem to contain an SQL NULL. */
    let z = match z {
        None => {
            sqlite3VdbeMemSetNull(pMem);
            return SQLITE_OK;
        }
        Some(z) => z,
    };
    let iLimit = db.aLimit[SQLITE_LIMIT_LENGTH as usize];
    if memSetStrCore(pMem, z, n, enc, iLimit).is_err() {
        return crate::util::sqlite3ErrorToParser(db, SQLITE_TOOBIG);
    }
    if pMem.enc > SQLITE_UTF8 && crate::utf::sqlite3VdbeMemHandleBom(pMem) != 0 {
        return SQLITE_NOMEM;
    }
    SQLITE_OK
}

/// sqlite3VdbeMemSetStr() for a Mem without a connection (C `pMem->db==0`):
/// the limit is SQLITE_MAX_LENGTH.
pub fn memSetStrNoDb(pMem: &mut Mem, z: Option<&[u8]>, n: i64, enc: u8) -> i32 {
    let z = match z {
        None => {
            sqlite3VdbeMemSetNull(pMem);
            return SQLITE_OK;
        }
        Some(z) => z,
    };
    if memSetStrCore(pMem, z, n, enc, SQLITE_MAX_LENGTH).is_err() {
        return SQLITE_TOOBIG;
    }
    if pMem.enc > SQLITE_UTF8 && crate::utf::sqlite3VdbeMemHandleBom(pMem) != 0 {
        return SQLITE_NOMEM;
    }
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemSetText( Mem *pMem, const char *z, i64 n, void (*xDel)(void*) )`
///
/// Like sqlite3VdbeMemSetStr() except enc is always SQLITE_UTF8.
pub fn sqlite3VdbeMemSetText(db: &mut Sqlite3, pMem: &mut Mem, z: Option<&[u8]>, n: i64) -> i32 {
    debug_assert!(sqlite3VdbeMemIsRowSet(pMem) == 0);

    /* If z is a NULL pointer, set pMem to contain an SQL NULL. */
    let z = match z {
        None => {
            sqlite3VdbeMemSetNull(pMem);
            return SQLITE_OK;
        }
        Some(z) => z,
    };

    let mut nByte = n;
    let flags: u16;
    if nByte < 0 {
        nByte = cstrlen(z) as i64;
        flags = MEM_Str | MEM_Term;
    } else {
        flags = MEM_Str;
    }
    if nByte as usize > z.len() {
        nByte = z.len() as i64;
    }
    if nByte > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        sqlite3VdbeMemSetNull(pMem);
        return crate::util::sqlite3ErrorToParser(db, SQLITE_TOOBIG);
    }

    /* SQLITE_TRANSIENT */
    let nAlloc = nByte + 1;
    sqlite3VdbeMemClearAndResize(pMem, nAlloc.max(32) as i32);
    let nb = nByte as usize;
    pMem.z[..nb].copy_from_slice(&z[..nb]);
    pMem.z[nb] = 0;
    pMem.flags = flags;
    pMem.n = (nByte & 0x7fffffff) as i32;
    pMem.enc = SQLITE_UTF8;
    pMem.pPtr = None;
    SQLITE_OK
}

/// C: `int sqlite3VdbeMemFromBtree( BtCursor *pCur, u32 offset, u32 amt, Mem *pMem )`
///
/// Move data out of a btree key or data field and into a Mem structure.
pub fn sqlite3VdbeMemFromBtree(pCur: &BtCursorP, offset: u32, amt: u32, pMem: &mut Mem) -> i32 {
    pMem.flags = MEM_Null;
    if amt >= SQLITE_MAX_ALLOCATION_SIZE as u32 {
        return SQLITE_NOMEM;
    }
    if (amt as u64) + (offset as u64) > crate::btree::sqlite3BtreeMaxRecordSize(pCur) as u64 {
        return SQLITE_CORRUPT;
    }
    let mut rc = sqlite3VdbeMemClearAndResize(pMem, (amt + 1) as i32);
    if rc == SQLITE_OK {
        rc = crate::btree::sqlite3BtreePayload(pCur, offset, amt, &mut pMem.z[..amt as usize]);
        if rc == SQLITE_OK {
            pMem.z[amt as usize] = 0; /* Overrun area used when reading malformed records */
            pMem.flags = MEM_Blob;
            pMem.n = amt as i32;
        } else {
            sqlite3VdbeMemRelease(pMem);
        }
    }
    rc
}

/// C: `int sqlite3VdbeMemFromBtreeZeroOffset( BtCursor *pCur, u32 amt, Mem *pMem )`
pub fn sqlite3VdbeMemFromBtreeZeroOffset(pCur: &BtCursorP, amt: u32, pMem: &mut Mem) -> i32 {
    let mut available: u32 = 0; /* Number of bytes available on the local btree page */
    let mut rc = SQLITE_OK;

    debug_assert!(!pMem.VdbeMemDynamic());
    pMem.z = crate::btree::sqlite3BtreePayloadFetch(pCur, &mut available);

    if amt <= available {
        pMem.flags = MEM_Blob | MEM_Ephem;
        pMem.n = amt as i32;
    } else {
        rc = sqlite3VdbeMemFromBtree(pCur, 0, amt, pMem);
    }
    rc
}

/// The pVal argument is known to be a value other than NULL.  Convert it
/// into a string with encoding enc and return a reference to its bytes (a
/// zero-terminated version is kept in pVal->z).
fn valueToText<'a>(
    db: Option<&mut Sqlite3>,
    nFp: u8,
    pVal: &'a mut Mem,
    enc: u8,
) -> Option<&'a [u8]> {
    let mut db = db;
    debug_assert!((pVal.flags & MEM_Null) == 0);
    let encx = enc & !SQLITE_UTF16_ALIGNED;
    if pVal.flags & (MEM_Blob | MEM_Str) != 0 {
        if ExpandBlob(pVal) != 0 {
            return None;
        }
        pVal.flags |= MEM_Str;
        if pVal.enc != encx {
            changeEncoding(db.as_deref_mut(), pVal, encx);
        }
        /* (alignment of UTF-16 strings is not an issue: Vec<u8>) */
        memNulTerminate(pVal); /* IMP: R-31275-44060 */
    } else {
        memStringify(db.as_deref_mut(), nFp, pVal, encx, 0);
    }
    if pVal.enc == encx {
        Some(pVal.bytes())
    } else {
        None
    }
}

/// sqlite3ValueText() returning a reference to the bytes of the value.
/// `db` is used for the number of significant digits and for encoding
/// translations (None: a Mem without a connection).
pub fn valueTextRef<'a>(db: Option<&mut Sqlite3>, pVal: &'a mut Mem, enc: u8) -> Option<&'a [u8]> {
    let nFp = match &db {
        Some(d) => fpDigits(d),
        None => DFLT_FP_DIGITS,
    };
    if (pVal.flags & (MEM_Str | MEM_Term)) == (MEM_Str | MEM_Term) && pVal.enc == enc {
        return Some(pVal.bytes());
    }
    if pVal.flags & MEM_Null != 0 {
        return None;
    }
    valueToText(db, nFp, pVal, enc)
}

/// C: `const void * sqlite3ValueText(sqlite3_value* pVal, u8 enc)`
///
/// Works like sqlite3_value_text(), except the data returned is in the
/// encoding specified by the second parameter.  Rust: returns a copy of
/// the bytes (without the terminator).
pub fn sqlite3ValueText(db: &mut Sqlite3, pVal: &mut Mem, enc: u8) -> Option<Vec<u8>> {
    valueTextRef(Some(db), pVal, enc).map(|s| s.to_vec())
}

/// C: `int sqlite3ValueIsOfClass(const sqlite3_value *pVal, void(*xFree)(void*))`
///
/// Return true if pVal is a string or blob value that uses the destructor
/// xFree.  Strings are always owned by the Mem in this port, so this is
/// always false.
pub fn sqlite3ValueIsOfClass(pVal: &Mem, xFree: ()) -> i32 {
    let _ = xFree;
    if (pVal.flags & (MEM_Str | MEM_Blob)) != 0 && (pVal.flags & MEM_Dyn) != 0 {
        return 0;
    }
    0
}

/// C: `sqlite3_value * sqlite3ValueNew(sqlite3 *db)`
///
/// Create a new sqlite3_value object (never fails in this port).
pub fn sqlite3ValueNew(db: &mut Sqlite3) -> Option<Box<Mem>> {
    let _ = db;
    let mut p = Box::new(Mem::default());
    p.flags = MEM_Null;
    Some(p)
}

/// Context object passed by sqlite3Stat4ProbeSetValue() through to
/// valueNew().
struct ValueNewStat4Ctx<'r> {
    pIdx: IndexRef,
    ppRec: &'r mut Option<Box<UnpackedRecord>>,
    iVal: i32,
}

/// Allocate a new value.  With a stat4 context, make sure the
/// UnpackedRecord exists (the value itself is moved into it by the caller:
/// see stat4StoreValue()).  Returns false on allocation failure.
fn valueNew(db: &mut Sqlite3, p: Option<&mut ValueNewStat4Ctx>) -> Option<Box<Mem>> {
    if let Some(p) = p {
        if p.ppRec.is_none() {
            let pIdx = p.pIdx.clone();
            let nCol = pIdx.borrow().nColumn as usize;
            let pKeyInfo = crate::build::sqlite3KeyInfoOfIndex(db, &pIdx)?;
            let mut aMem = Vec::with_capacity(nCol);
            for _ in 0..nCol {
                let mut m = Mem::default();
                m.flags = MEM_Null;
                aMem.push(m);
            }
            let pRec = UnpackedRecord {
                pKeyInfo: Some(pKeyInfo),
                aMem,
                ..Default::default()
            };
            *p.ppRec = Some(Box::new(pRec));
        }
        let pRec = p.ppRec.as_deref_mut().unwrap();
        pRec.nField = (p.iVal + 1) as u16;
        if let Some(m) = pRec.aMem.get_mut(p.iVal as usize) {
            sqlite3VdbeMemSetNull(m);
        }
    }
    sqlite3ValueNew(db)
}

/// Store a value produced for a stat4 context into its UnpackedRecord.
fn stat4StoreValue(p: &mut ValueNewStat4Ctx, pVal: Box<Mem>) {
    if let Some(pRec) = p.ppRec.as_deref_mut() {
        if let Some(m) = pRec.aMem.get_mut(p.iVal as usize) {
            *m = *pVal;
        }
    }
}

/// The expression object indicated by the second argument is guaranteed to
/// be a scalar SQL function.  If all function arguments are SQL literals,
/// one of the SQLITE_FUNC_CONSTANT or _SLOCHNG function flags is set, and
/// the SQLITE_FUNC_NEEDCOLL function flag is not set, then this routine
/// attempts to invoke the SQL function (SQLITE_ENABLE_STAT4 only: this
/// path is only reached through the sqlite3Stat4*() entry points).
fn valueFromFunction(
    db: &mut Sqlite3,
    p: ExprId,
    enc: u8,
    aff: u8,
    ppVal: &mut Option<Box<Mem>>,
    pCtx: &mut ValueNewStat4Ctx,
) -> i32 {
    let mut rc = SQLITE_OK;
    let (pList, zToken) = {
        let e = db.ex(p);
        debug_assert!(e.flags & EP_TokenOnly == 0);
        debug_assert!(e.UseXList());
        (e.x.pList, e.u.zToken.clone())
    };
    let nVal = if pList.is_null() {
        0
    } else {
        db.el(pList).nExpr()
    };
    let pFunc = match crate::callback::sqlite3FindFunction(db, zToken.as_deref(), nVal, enc, 0) {
        Some(f) => f,
        None => return SQLITE_OK,
    };
    if (pFunc.funcFlags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) == 0
        || (pFunc.funcFlags & (SQLITE_FUNC_NEEDCOLL | SQLITE_FUNC_RUNONLY)) != 0
    {
        return SQLITE_OK;
    }

    let mut apVal: Vec<Mem> = Vec::new();
    let mut pVal: Option<Box<Mem>> = None;
    'out: {
        if !pList.is_null() {
            for i in 0..nVal {
                let pE = db.el(pList).a[i as usize].pExpr;
                let mut v: Option<Box<Mem>> = None;
                rc = sqlite3Stat4ValueFromExpr(db, pE, aff, &mut v);
                match v {
                    Some(v) if rc == SQLITE_OK => apVal.push(*v),
                    _ => break 'out,
                }
            }
        }

        pVal = valueNew(db, Some(pCtx));
        let pv = match pVal.as_deref_mut() {
            Some(v) => v,
            None => {
                rc = SQLITE_NOMEM;
                break 'out;
            }
        };

        let dbenc = db.ENC();
        let isError;
        {
            let mut ctx = Context {
                db: &mut *db,
                pOut: &mut *pv,
                pFunc: pFunc.clone(),
                pMem: None,
                pAuxData: None,
                iOp: 0,
                pOpInfo: None,
                isError: 0,
                enc: dbenc,
                skipFlag: 0,
                argc: nVal as u16,
                zErr: None,
                pColl: None,
                piCurrentTime: None,
            };
            if let Some(x) = pFunc.xSFunc.clone() {
                x(&mut ctx, &mut apVal);
            }
            isError = ctx.isError;
        }
        if isError != 0 {
            rc = isError;
            let z = valueTextRef(Some(&mut *db), pv, SQLITE_UTF8).map(|s| s.to_vec());
            errmsg!(db, "%s", z.as_deref());
        } else {
            crate::vdbe::sqlite3ValueApplyAffinity(db, pv, aff, SQLITE_UTF8);
            debug_assert!(rc == SQLITE_OK);
            rc = changeEncoding(Some(&mut *db), pv, enc);
            if rc == SQLITE_OK && sqlite3VdbeMemTooBig(db, pv) != 0 {
                rc = SQLITE_TOOBIG;
                db.pp().nErr += 1;
            }
        }
    }

    if rc != SQLITE_OK {
        pVal = None;
        db.pp().rc = rc;
    }
    for m in apVal.iter_mut() {
        sqlite3VdbeMemRelease(m);
    }
    *ppVal = pVal;
    rc
}

/// Extract a value from the supplied expression in the manner described
/// above sqlite3ValueFromExpr().
fn valueFromExpr(
    db: &mut Sqlite3,
    pExpr: ExprId,
    enc: u8,
    affinity: u8,
    ppVal: &mut Option<Box<Mem>>,
    mut pCtx: Option<&mut ValueNewStat4Ctx>,
) -> i32 {
    let mut pExpr = pExpr;
    let mut pVal: Option<Box<Mem>> = None;
    let mut negInt: i64 = 1;
    let mut zNeg: &[u8] = b"";
    let mut rc = SQLITE_OK;

    debug_assert!(!pExpr.is_null());
    let mut op;
    loop {
        op = db.ex(pExpr).op;
        if op == TK_UPLUS || op == TK_SPAN {
            pExpr = db.ex(pExpr).pLeft;
        } else {
            break;
        }
    }
    if op == TK_REGISTER {
        op = db.ex(pExpr).op2;
    }

    if op == TK_CAST {
        let (zTok, pLeft) = {
            let e = db.ex(pExpr);
            (e.u.zToken.clone(), e.pLeft)
        };
        let aff = crate::build::sqlite3AffinityType(zTok.as_deref(), None);
        rc = valueFromExpr(db, pLeft, enc, aff, ppVal, pCtx);
        if let Some(v) = ppVal.as_deref_mut() {
            rc = ExpandBlob(v);
            sqlite3VdbeMemCast(db, v, aff, enc);
            crate::vdbe::sqlite3ValueApplyAffinity(db, v, affinity, enc);
        }
        return rc;
    }

    /* Handle negative integers in a single step.  This is needed in the
     ** case when the value is -9223372036854775808. Except - do not do this
     ** for hexadecimal literals.  */
    if op == TK_UMINUS {
        let pLeft = db.ex(pExpr).pLeft;
        let l = db.ex(pLeft);
        if l.op == TK_INTEGER || l.op == TK_FLOAT {
            let t = l.token();
            if l.HasProperty(EP_IntValue)
                || t.first().copied().unwrap_or(0) != b'0'
                || (t.get(1).copied().unwrap_or(0) & !0x20) != b'X'
            {
                pExpr = pLeft;
                op = db.ex(pExpr).op;
                negInt = -1;
                zNeg = b"-";
            }
        }
    }

    if op == TK_STRING || op == TK_FLOAT || op == TK_INTEGER {
        pVal = valueNew(db, pCtx.as_deref_mut());
        let (isIntValue, iValue, zToken) = {
            let e = db.ex(pExpr);
            (e.HasProperty(EP_IntValue), e.u.iValue, e.u.zToken.clone())
        };
        {
            let v = pVal.as_deref_mut().unwrap();
            if isIntValue {
                sqlite3VdbeMemSetInt64(v, (iValue as i64).wrapping_mul(negInt));
            } else {
                let mut iVal: i64 = 0;
                if op == TK_INTEGER
                    && 0 == crate::util::sqlite3DecOrHexToI64(zToken.as_deref(), &mut iVal)
                {
                    sqlite3VdbeMemSetInt64(v, iVal.wrapping_mul(negInt));
                } else {
                    let mut zVal = zNeg.to_vec();
                    zVal.extend_from_slice(zToken.as_deref().unwrap_or(b""));
                    sqlite3VdbeMemSetStr(db, v, Some(&zVal), -1, SQLITE_UTF8);
                }
            }
            if affinity == SQLITE_AFF_BLOB {
                if op == TK_FLOAT {
                    let mut r = 0.0;
                    crate::util::sqlite3AtoF(Some(v.bytes()), &mut r);
                    v.u.r = r;
                    v.flags = MEM_Real;
                } else if op == TK_INTEGER {
                    /* This case is required by -9223372036854775808 and other
                     ** strings that look like integers but cannot be handled by
                     ** the sqlite3DecOrHexToI64() call above.  */
                    crate::vdbe::sqlite3ValueApplyAffinity(db, v, SQLITE_AFF_NUMERIC, SQLITE_UTF8);
                }
            } else {
                crate::vdbe::sqlite3ValueApplyAffinity(db, v, affinity, SQLITE_UTF8);
            }
            debug_assert!((v.flags & MEM_IntReal) == 0);
            if v.flags & (MEM_Int | MEM_IntReal | MEM_Real) != 0 {
                v.flags &= !MEM_Str;
            }
            if enc != SQLITE_UTF8 {
                rc = changeEncoding(Some(&mut *db), v, enc);
            }
        }
    } else if op == TK_UMINUS {
        /* This branch happens for multiple negative signs.  Ex: -(-5) */
        let pLeft = db.ex(pExpr).pLeft;
        if SQLITE_OK == valueFromExpr(db, pLeft, enc, affinity, &mut pVal, pCtx.as_deref_mut())
            && pVal.is_some()
        {
            let v = pVal.as_deref_mut().unwrap();
            memNumerify(v);
            if v.flags & MEM_Real != 0 {
                v.u.r = -v.u.r;
            } else if v.u.i == SMALLEST_INT64 {
                v.u.r = -(SMALLEST_INT64 as f64);
                v.MemSetTypeFlag(MEM_Real);
            } else {
                v.u.i = -v.u.i;
            }
            crate::vdbe::sqlite3ValueApplyAffinity(db, v, affinity, enc);
        }
    } else if op == TK_NULL {
        pVal = valueNew(db, pCtx.as_deref_mut());
        if let Some(v) = pVal.as_deref_mut() {
            sqlite3VdbeMemSetNull(v);
        }
    } else if op == TK_BLOB {
        let zTok = db.ex(pExpr).u.zToken.clone().unwrap_or_default();
        debug_assert!(zTok.first() == Some(&b'x') || zTok.first() == Some(&b'X'));
        debug_assert!(zTok.get(1) == Some(&b'\''));
        pVal = valueNew(db, pCtx.as_deref_mut());
        let zVal: &[u8] = if zTok.len() >= 2 { &zTok[2..] } else { b"" };
        let nVal = cstrlen(zVal) as i32 - 1;
        let blob = crate::util::sqlite3HexToBlob(db, Some(zVal), nVal);
        let v = pVal.as_deref_mut().unwrap();
        sqlite3VdbeMemSetStr(db, v, blob.as_deref(), (nVal / 2) as i64, 0);
    } else if op == TK_FUNCTION && pCtx.is_some() {
        rc = valueFromFunction(
            db,
            pExpr,
            enc,
            affinity,
            &mut pVal,
            pCtx.as_deref_mut().unwrap(),
        );
    } else if op == TK_TRUEFALSE {
        let t = db.ex(pExpr).token().len();
        pVal = valueNew(db, pCtx.as_deref_mut());
        if let Some(v) = pVal.as_deref_mut() {
            v.flags = MEM_Int;
            v.u.i = (t == 4) as i64; /* pExpr->u.zToken[4]==0 */
            crate::vdbe::sqlite3ValueApplyAffinity(db, v, affinity, enc);
        }
    }

    *ppVal = pVal;
    rc
}

/// C: `int sqlite3ValueFromExpr( sqlite3 *db, const Expr *pExpr, u8 enc, u8 affinity, sqlite3_value **ppVal )`
///
/// Create a new sqlite3_value object, containing the value of pExpr.
///
/// This only works for very simple expressions that consist of one
/// constant token (i.e. "5", "5.1", "'a string'").  If the expression can
/// be converted directly into a value, then the value is allocated and
/// written to *ppVal.  Otherwise *ppVal is set to None.
pub fn sqlite3ValueFromExpr(
    db: &mut Sqlite3,
    pExpr: ExprId,
    enc: u8,
    affinity: u8,
    ppVal: &mut Option<Box<Mem>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut pVal: Option<Box<Mem>> = None;
    if !pExpr.is_null() {
        rc = valueFromExpr(db, pExpr, enc, affinity, &mut pVal, None);
        if rc == SQLITE_OK && affinity == SQLITE_AFF_REAL {
            if let Some(v) = pVal.as_deref_mut() {
                if (v.flags & MEM_Int) != 0
                    && (v.u.i < -140737488355328i64 || v.u.i > 140737488355327i64)
                {
                    /* If the integer value is too large to fit in a 6-byte
                     ** integer and the affinity is REAL, convert it to a real
                     ** value now. */
                    memRealify(v);
                }
            }
        }
    }
    *ppVal = pVal;
    rc
}

/// Attempt to extract a value from pExpr and use it to construct *ppVal.
/// (SQLITE_ENABLE_STAT4)
fn stat4ValueFromExpr(
    db: &mut Sqlite3,
    pExpr: ExprId,
    affinity: u8,
    mut pAlloc: Option<&mut ValueNewStat4Ctx>,
    ppVal: &mut Option<Box<Mem>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut pVal: Option<Box<Mem>> = None;

    /* Skip over any TK_COLLATE nodes */
    let pExpr = if pExpr.is_null() {
        pExpr
    } else {
        crate::expr::sqlite3ExprSkipCollate(db, pExpr)
    };

    if pExpr.is_null() {
        pVal = valueNew(db, pAlloc.as_deref_mut());
        if let Some(v) = pVal.as_deref_mut() {
            sqlite3VdbeMemSetNull(v);
        }
    } else if db.ex(pExpr).op == TK_VARIABLE && (db.flags & SQLITE_EnableQPSG) == 0 {
        let iBindVar = db.ex(pExpr).iColumn as i32;
        let mut pv = db.pp().pVdbe.take();
        if let Some(v) = pv.as_deref_mut() {
            crate::vdbeaux::sqlite3VdbeReprepareOnBind(db, v, iBindVar, 0);
        }
        db.pp().pVdbe = pv;
        if let Some(id) = db.ppr().pReprepare {
            pVal = valueNew(db, pAlloc.as_deref_mut());
            let enc = db.ENC();
            let mut copy: Option<Mem> = None;
            if db.stmt_valid(id) {
                if let Some(v) = db.aVdbe[id.idx as usize].v.as_deref() {
                    if let Some(m) = v.aVar.get((iBindVar - 1) as usize) {
                        let mut t = Mem::default();
                        rc = sqlite3VdbeMemCopy(&mut t, m);
                        copy = Some(t);
                    }
                }
            }
            if let (Some(pv), Some(t)) = (pVal.as_deref_mut(), copy) {
                *pv = t;
                crate::vdbe::sqlite3ValueApplyAffinity(db, pv, affinity, enc);
            }
        }
    } else {
        let enc = db.ENC();
        rc = valueFromExpr(db, pExpr, enc, affinity, &mut pVal, pAlloc.as_deref_mut());
    }

    *ppVal = pVal;
    rc
}

/// C: `int sqlite3Stat4ProbeSetValue( Parse *pParse, Index *pIdx, UnpackedRecord **ppRec, Expr *pExpr, int nElem, int iVal, int *pnExtract )`
///
/// Allocate and populate an UnpackedRecord structure intended to be
/// compared against sample index keys stored in the sqlite_stat4 table.
pub fn sqlite3Stat4ProbeSetValue(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    ppRec: &mut Option<Box<UnpackedRecord>>,
    pExpr: ExprId,
    nElem: i32,
    iVal: i32,
    pnExtract: &mut i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut nExtract = 0;

    if pExpr.is_null() || db.ex(pExpr).op != TK_SELECT {
        let mut alloc = ValueNewStat4Ctx {
            pIdx: pIdx.clone(),
            ppRec,
            iVal: 0,
        };
        for i in 0..nElem {
            let pElem = if pExpr.is_null() {
                ExprId::NULL
            } else {
                crate::expr::sqlite3VectorFieldSubexpr(db, pExpr, i)
            };
            let aff = crate::where_::sqlite3IndexColumnAffinity(db, pIdx, iVal + i);
            alloc.iVal = iVal + i;
            let mut pVal: Option<Box<Mem>> = None;
            rc = stat4ValueFromExpr(db, pElem, aff, Some(&mut alloc), &mut pVal);
            match pVal {
                None => break,
                Some(v) => stat4StoreValue(&mut alloc, v),
            }
            nExtract += 1;
        }
    }

    *pnExtract = nExtract;
    rc
}

/// C: `int sqlite3Stat4ValueFromExpr( Parse *pParse, Expr *pExpr, u8 affinity, sqlite3_value **ppVal )`
///
/// Attempt to extract a value from expression pExpr using the methods as
/// described for sqlite3Stat4ProbeSetValue() above.
pub fn sqlite3Stat4ValueFromExpr(
    db: &mut Sqlite3,
    pExpr: ExprId,
    affinity: u8,
    ppVal: &mut Option<Box<Mem>>,
) -> i32 {
    stat4ValueFromExpr(db, pExpr, affinity, None, ppVal)
}

/// getVarint32() macro: read a 32-bit varint, return the number of bytes.
fn getVarint32(a: &[u8], v: &mut u32) -> u32 {
    match a.first() {
        Some(&b) if b < 0x80 => {
            *v = b as u32;
            1
        }
        Some(_) => crate::util::sqlite3GetVarint32(a, v) as u32,
        None => {
            *v = 0;
            1
        }
    }
}

/// C: `int sqlite3Stat4Column( sqlite3 *db, const void *pRec, int nRec, int iCol, sqlite3_value **ppVal )`
///
/// Extract the iCol-th column from the nRec-byte record in pRec.  Write the
/// column value into *ppVal.  If *ppVal is initially None then a new value
/// is allocated.
pub fn sqlite3Stat4Column(
    db: &mut Sqlite3,
    pRec: &[u8],
    nRec: i32,
    iCol: i32,
    ppVal: &mut Option<Box<Mem>>,
) -> i32 {
    let mut t: u32 = 0;
    let mut nHdr: u32 = 0;
    let mut szField: u32 = 0;
    let a = pRec;

    debug_assert!(iCol > 0);
    let mut iHdr = getVarint32(a, &mut nHdr);
    if nHdr > nRec as u32 || iHdr >= nHdr {
        return SQLITE_CORRUPT;
    }
    let mut iField = nHdr as i64;
    for _i in 0..=iCol {
        iHdr += getVarint32(a.get(iHdr as usize..).unwrap_or(&[]), &mut t);
        if iHdr > nHdr {
            return SQLITE_CORRUPT;
        }
        szField = crate::vdbeaux::sqlite3VdbeSerialTypeLen(t);
        iField += szField as i64;
    }
    if iField > nRec as i64 {
        return SQLITE_CORRUPT;
    }
    if ppVal.is_none() {
        *ppVal = sqlite3ValueNew(db);
    }
    let enc = db.ENC();
    let pMem = ppVal.as_deref_mut().unwrap();
    let off = (iField - szField as i64) as usize;
    crate::vdbeaux::sqlite3VdbeSerialGet(a.get(off..).unwrap_or(&[]), t, pMem);
    pMem.enc = enc;
    SQLITE_OK
}

/// C: `void sqlite3Stat4ProbeFree(UnpackedRecord *pRec)`
///
/// The argument must be an UnpackedRecord object returned by an earlier
/// call to sqlite3Stat4ProbeSetValue().  This call releases it.
pub fn sqlite3Stat4ProbeFree(db: &mut Sqlite3, pRec: &mut UnpackedRecord) {
    for m in pRec.aMem.iter_mut() {
        sqlite3VdbeMemRelease(m);
    }
    if let Some(k) = pRec.pKeyInfo.take() {
        crate::select::sqlite3KeyInfoUnref(db, &k);
    }
}

/// C: `void sqlite3ValueSetStr( sqlite3_value *v, int n, const void *z, u8 enc, void (*xDel)(void*) )`
///
/// Change the string value of an sqlite3_value object (a value without a
/// connection: the length limit is SQLITE_MAX_LENGTH).
pub fn sqlite3ValueSetStr(v: &mut Mem, n: i32, z: &[u8], enc: u8) {
    memSetStrNoDb(v, Some(z), n as i64, enc);
}

/// C: `void sqlite3ValueFree(sqlite3_value *v)`
pub fn sqlite3ValueFree(db: &mut Sqlite3, v: &mut Mem) {
    let _ = db;
    sqlite3VdbeMemRelease(v);
}

/// C: `int sqlite3ValueBytes(sqlite3_value *pVal, u8 enc)`
///
/// Return the number of bytes in the sqlite3_value object assuming that it
/// uses the encoding "enc".
pub fn sqlite3ValueBytes(pVal: &mut Mem, enc: u8) -> i32 {
    valueBytesDb(None, pVal, enc)
}

/// sqlite3ValueBytes() with an optional connection (for nFpDigit and
/// encoding translations).
pub fn valueBytesDb(db: Option<&mut Sqlite3>, pVal: &mut Mem, enc: u8) -> i32 {
    let p = &*pVal;
    debug_assert!((p.flags & MEM_Null) == 0 || (p.flags & (MEM_Str | MEM_Blob)) == 0);
    if (p.flags & MEM_Str) != 0 && p.enc == enc {
        return p.n;
    }
    if (p.flags & MEM_Str) != 0 && enc != SQLITE_UTF8 && p.enc != SQLITE_UTF8 {
        return p.n;
    }
    if (p.flags & MEM_Blob) != 0 {
        if p.flags & MEM_Zero != 0 {
            return p.n + p.u.nZero;
        } else {
            return p.n;
        }
    }
    if p.flags & MEM_Null != 0 {
        return 0;
    }
    /* valueBytes() */
    let nFp = match &db {
        Some(d) => fpDigits(d),
        None => DFLT_FP_DIGITS,
    };
    if valueToText(db, nFp, pVal, enc).is_some() {
        pVal.n
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Mem {
        let mut m = Mem::default();
        m.flags = MEM_Null;
        m
    }

    #[test]
    fn set_int_and_double() {
        let mut m = mem();
        sqlite3VdbeMemSetInt64(&mut m, 42);
        assert_eq!(m.flags, MEM_Int);
        assert_eq!(memIntValueAny(&m), 42);
        sqlite3VdbeMemSetDouble(&mut m, 1.5);
        assert_eq!(m.flags, MEM_Real);
        assert_eq!(memRealValueAny(&mut m), 1.5);
        sqlite3VdbeMemSetDouble(&mut m, f64::NAN);
        assert_eq!(m.flags, MEM_Null);
    }

    #[test]
    fn stringify_int() {
        let mut m = mem();
        sqlite3VdbeMemSetInt64(&mut m, -1234567890123);
        let t = valueTextRef(None, &mut m, SQLITE_UTF8).map(|s| s.to_vec());
        assert_eq!(t.as_deref(), Some(&b"-1234567890123"[..]));
        assert_eq!(
            m.flags & (MEM_Str | MEM_Term | MEM_Int),
            MEM_Str | MEM_Term | MEM_Int
        );
        m.flags = MEM_IntReal;
        m.u.i = 7;
        let t = valueTextRef(None, &mut m, SQLITE_UTF8).map(|s| s.to_vec());
        assert_eq!(t.as_deref(), Some(&b"7.0"[..]));
    }

    #[test]
    fn set_str_and_copy() {
        let mut m = mem();
        assert_eq!(
            memSetStrNoDb(&mut m, Some(b"hello\0world"), -1, SQLITE_UTF8),
            0
        );
        assert_eq!(m.n, 5);
        assert_eq!(m.flags, MEM_Str | MEM_Term);
        assert_eq!(m.bytes(), b"hello");
        let mut c = mem();
        sqlite3VdbeMemCopy(&mut c, &m);
        assert_eq!(c.bytes(), b"hello");
        assert_eq!(c.flags & MEM_Ephem, 0);
        let mut d = mem();
        memMove(&mut d, &mut c);
        assert_eq!(d.bytes(), b"hello");
        assert_eq!(c.flags, MEM_Null);
        assert!(memSetStrNoDb(&mut m, Some(b"abc"), 3, 0) == 0);
        assert_eq!(m.flags, MEM_Blob);
    }

    #[test]
    fn zeroblob_expand() {
        let mut m = mem();
        sqlite3VdbeMemSetZeroBlob(&mut m, 4);
        assert_eq!(sqlite3ValueBytes(&mut m, SQLITE_UTF8), 4);
        assert_eq!(ExpandBlob(&mut m), 0);
        assert_eq!(m.n, 4);
        assert_eq!(m.bytes(), &[0, 0, 0, 0]);
        assert_eq!(m.flags, MEM_Blob);
    }

    #[test]
    fn real_to_int() {
        assert_eq!(sqlite3RealToI64(1e300), LARGEST_INT64);
        assert_eq!(sqlite3RealToI64(-1e300), SMALLEST_INT64);
        assert_eq!(sqlite3RealToI64(-3.7), -3);
        assert_eq!(sqlite3RealSameAsInt(3.0, 3), 1);
        assert_eq!(sqlite3RealSameAsInt(0.0, 0), 1);
        let mut m = mem();
        sqlite3VdbeMemSetDouble(&mut m, 5.0);
        memIntegerAffinity(&mut m);
        assert_eq!(m.flags, MEM_Int);
        assert_eq!(m.u.i, 5);
    }
}
