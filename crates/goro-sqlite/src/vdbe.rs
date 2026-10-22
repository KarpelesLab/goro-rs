//! Port of vdbe.c: the bytecode interpreter (sqlite3VdbeExec()) and a few
//! value helpers (affinities, numeric type detection).
//!
//! # Design notes
//!
//! * The opcode loop keeps the program in `p.aOp` (it is not taken out):
//!   each step copies the scalar operands of the current instruction and
//!   reads `p.aOp[pc].p4` through a field borrow, so that `p.aMem`,
//!   `p.apCsr` ... can be borrowed at the same time.  Opcodes that rewrite
//!   themselves (OP_String8 -> OP_String, OP_AggStep -> OP_AggStep1,
//!   OP_Once / OP_Init P1 counters) write into `p.aOp[pc]`.
//! * Every C `goto` target of sqlite3VdbeExec() is an [`Ex`] variant (or a
//!   pc update): an opcode implementation returns `Ok(Fl::Next)` (continue
//!   with pc+1), `Ok(Fl::Check)` (the `check_for_interrupt` label) or
//!   `Err(Ex::...)` (abort_due_to_error, no_mem, too_big,
//!   abort_due_to_interrupt, vdbe_return).
//! * Sub-programs (triggers, FK actions) run in a [`VdbeFrame`]; see its
//!   documentation for the push/pop protocol shared with vdbeaux.rs.

use crate::btree::*;
use crate::int::*;
use crate::printf::PfArg;
use crate::vdbeapi::{sqlite3_value_text, sqlite3_value_type};
use crate::vdbeaux::*;
use crate::vdbemem::*;
use std::rc::Rc;

/// The C LEGACY_SCHEMA_TABLE name.
const LEGACY_SCHEMA_TABLE: &[u8] = b"sqlite_master";

/// When a sub-program is executed (OP_Program), an instance of this
/// structure stores the state of the calling (parent) frame.
///
/// Rust port (defined by the vdbeaux.c port, mirroring C's VdbeFrame):
/// * the frame stack is `Vdbe::pFrame`; each frame owns its parent frame
///   through `pParent` (C pParent).
/// * OP_Program moves the caller's arrays into the frame
///   (`aOp`/`aMem`/`apCsr` = C aOp/aMem/apCsr of the *parent*) and installs
///   the sub-program's in the Vdbe (`v.aOp = pProgram.aOp.clone()`, fresh
///   `aMem` of nChildMem MEM_Undefined cells, `apCsr` of nChildCsr None).
///   `aOnce` is the sub-program's OP_Once bitmask (as in C: pFrame->aOnce
///   belongs to the child), zeroed on each invocation.
/// * sqlite3VdbeFrameRestore() closes the child's cursors and moves the
///   parent arrays back into the Vdbe; the child's registers/cursors end
///   up in `aChildMem`/`apChildCsr`.  The caller then disposes of the frame
///   with sqlite3VdbeFrameDelete().  Frames are not cached for reuse in
///   register P3 as C does (nothing observable depends on that reuse: the
///   child registers are always rewritten before use and aOnce is reset).
#[derive(Default)]
pub struct VdbeFrame {
    /// Parent of this frame, or None if the parent is the main program.
    pub pParent: Option<Box<VdbeFrame>>,
    /// Program instructions of the parent frame.
    pub aOp: Vec<VdbeOp>,
    /// Registers of the parent frame.
    pub aMem: Vec<Mem>,
    /// Cursors of the parent frame.
    pub apCsr: Vec<Option<Box<VdbeCursor>>>,
    /// OP_Once bitmask of the child (sub-program) frame.
    pub aOnce: Vec<u8>,
    /// Copy of SubProgram.token.
    pub token: usize,
    /// Last insert rowid (sqlite3.lastRowid) of the parent.
    pub lastRowid: i64,
    /// Auxdata allocations of the parent.
    pub pAuxData: Vec<AuxData>,
    pub nCursor: i32,
    /// Program counter in the parent (calling) frame.
    pub pc: i32,
    pub nOp: i32,
    pub nMem: i32,
    pub nChildMem: i32,
    pub nChildCsr: i32,
    /// Statement changes (Vdbe.nChange) of the parent.
    pub nChange: i64,
    /// Value of db->nChange.
    pub nDbChange: i64,
    /// Rust: the child's registers once the frame has been restored.
    pub aChildMem: Vec<Mem>,
    /// Rust: the child's cursors once the frame has been restored.
    pub apChildCsr: Vec<Option<Box<VdbeCursor>>>,
}

/// The outermost frame (the one whose saved state is the main program).
fn root_frame(p: &mut Vdbe) -> Option<&mut VdbeFrame> {
    let mut f = p.pFrame.as_deref_mut()?;
    while f.pParent.is_some() {
        f = f.pParent.as_deref_mut().unwrap();
    }
    Some(f)
}

/// sqlite3VdbeError(p, fmt, ...)
macro_rules! verr {
    ($db:expr, $p:expr, $fmt:expr $(, $a:expr)* $(,)?) => {
        crate::vdbeaux::sqlite3VdbeError(
            $db,
            $p,
            Some(AsRef::<[u8]>::as_ref($fmt)),
            &[$(crate::printf::PfArg::from($a)),*],
        )
    };
}

/// SQLITE_CORRUPT_BKPT
#[inline]
fn corrupt_bkpt() -> i32 {
    crate::mainapi::sqlite3CorruptError(line!() as i32)
}

/// Two distinct mutable registers.
fn two_mut(v: &mut [Mem], a: usize, b: usize) -> (&mut Mem, &mut Mem) {
    debug_assert!(a != b);
    if a < b {
        let (l, r) = v.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = v.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}

/// The argument registers v[start..start+n] and the distinct register
/// v[one] (None if `one` lies inside the argument range).
fn args_and_one(
    v: &mut [Mem],
    start: usize,
    n: usize,
    one: usize,
) -> Option<(&mut [Mem], &mut Mem)> {
    if one >= start && one < start + n {
        return None;
    }
    if one < start {
        let (l, r) = v.split_at_mut(start);
        Some((&mut r[..n], &mut l[one]))
    } else {
        let (l, r) = v.split_at_mut(one);
        Some((&mut l[start..start + n], &mut r[0]))
    }
}

/// Copies of the registers v[start..start+n] (the C code points an
/// UnpackedRecord at the registers themselves).
fn reg_copies(v: &[Mem], start: i32, n: i32) -> Vec<Mem> {
    let s = start.max(0) as usize;
    let e = (s + n.max(0) as usize).min(v.len());
    v[s.min(e)..e].to_vec()
}

/// Byte at offset i of a record buffer (0 past the end: the C code may
/// over-read a few bytes harmlessly).
#[inline]
fn byte_at(z: &[u8], i: usize) -> u8 {
    if i < z.len() { z[i] } else { 0 }
}

/// sqlite3GetVarint32() at offset `off` of `z`, tolerant of truncated
/// buffers.  Returns the number of bytes read.
fn get_varint32_at(z: &[u8], off: usize, v: &mut u32) -> usize {
    if off + 9 <= z.len() {
        crate::util::sqlite3GetVarint32(&z[off..], v) as usize
    } else {
        let mut buf = [0u8; 9];
        for (k, b) in buf.iter_mut().enumerate() {
            *b = byte_at(z, off + k);
        }
        crate::util::sqlite3GetVarint32(&buf, v) as usize
    }
}

/// ExpandBlob(P)
#[inline]
fn ExpandBlob(db: &mut Sqlite3, p: &mut Mem) -> i32 {
    if (p.flags & MEM_Zero) != 0 {
        sqlite3VdbeMemExpandBlob(db, p)
    } else {
        0
    }
}

/// Deephemeralize(P): returns true on OOM (goto no_mem).
#[inline]
fn Deephemeralize(db: &mut Sqlite3, p: &mut Mem) -> bool {
    (p.flags & MEM_Ephem) != 0 && sqlite3VdbeMemMakeWriteable(db, p) != 0
}

#[inline]
fn isSorter(c: &VdbeCursor) -> bool {
    c.eCurType == CURTYPE_SORTER
}

/// Store a UTF-8 byte string in a Mem (MEM_Str|MEM_Term), as the C code
/// does when it points pOut->z at a static string.
fn mem_set_static_str(m: &mut Mem, z: &[u8], enc: u8) {
    if m.VdbeMemDynamic() {
        sqlite3VdbeMemSetNull(m);
    }
    m.z.clear();
    m.z.extend_from_slice(z);
    m.n = z.len() as i32;
    m.flags = MEM_Str | MEM_Static | MEM_Term;
    m.enc = enc;
}

/// Allocate a cursor for slot iCur, closing whatever cursor occupied the
/// slot (allocateCursor()).  The new cursor is p.apCsr[iCur].
fn allocateCursor(db: &mut Sqlite3, p: &mut Vdbe, iCur: i32, nField: i32, eCurType: u8) {
    debug_assert!(iCur >= 0 && (iCur as usize) < p.apCsr.len());
    let i = iCur as usize;
    if i >= p.apCsr.len() {
        p.apCsr.resize_with(i + 1, || None);
    }
    if let Some(mut old) = p.apCsr[i].take() {
        sqlite3VdbeFreeCursorNN(db, p, &mut old);
    }
    let nf = nField.max(0) as usize;
    let mut pCx = Box::new(VdbeCursor::default());
    pCx.eCurType = eCurType;
    pCx.nField = nField as i16;
    pCx.pAltCursor = -1;
    pCx.aType = vec![0; nf + 1];
    pCx.aOffset = vec![0; nf + 2];
    p.apCsr[i] = Some(pCx);
}

/// Try to convert the string value of pRec into an integer (alsoAnInt()).
fn alsoAnInt(pRec: &Mem, rValue: f64, piValue: &mut i64) -> bool {
    let iValue = sqlite3RealToI64(rValue);
    if sqlite3RealSameAsInt(rValue, iValue) != 0 {
        *piValue = iValue;
        return true;
    }
    0 == crate::util::sqlite3Atoi64(Some(pRec.bytes()), piValue, pRec.n, pRec.enc)
}

/// Try to convert a value into a numeric representation if we can do so
/// without loss of information (applyNumericAffinity()).
fn applyNumericAffinity(db: &mut Sqlite3, pRec: &mut Mem, bTryForInt: i32) {
    let mut rValue = 0.0f64;
    debug_assert!((pRec.flags & (MEM_Str | MEM_Int | MEM_Real | MEM_IntReal)) == MEM_Str);
    let rc = sqlite3MemRealValueRC(db, pRec, &mut rValue);
    if rc <= 0 {
        return;
    }
    let mut iv = 0i64;
    if (rc & 2) == 0 && alsoAnInt(pRec, rValue, &mut iv) {
        pRec.u.i = iv;
        pRec.flags |= MEM_Int;
    } else {
        pRec.u.r = rValue;
        pRec.flags |= MEM_Real;
        if bTryForInt != 0 {
            sqlite3VdbeIntegerAffinity(db, pRec);
        }
    }
    // TEXT->NUMERIC is many->one.  Hence, it is important to invalidate the
    // string representation after computing a numeric equivalent.
    pRec.flags &= !MEM_Str;
}

/// Processing is determine by the affinity parameter (applyAffinity()).
fn applyAffinity(db: &mut Sqlite3, pRec: &mut Mem, affinity: u8, enc: u8) {
    if affinity >= SQLITE_AFF_NUMERIC {
        if (pRec.flags & MEM_Int) == 0 {
            if (pRec.flags & (MEM_Real | MEM_IntReal)) == 0 {
                if (pRec.flags & MEM_Str) != 0 {
                    applyNumericAffinity(db, pRec, 1);
                }
            } else if affinity <= SQLITE_AFF_REAL {
                sqlite3VdbeIntegerAffinity(db, pRec);
            }
        }
    } else if affinity == SQLITE_AFF_TEXT {
        // Only attempt the conversion to TEXT if there is an integer or real
        // representation (blob and NULL do not get converted) but no string
        // representation.
        if 0 == (pRec.flags & MEM_Str) {
            if (pRec.flags & (MEM_Real | MEM_Int | MEM_IntReal)) != 0 {
                sqlite3VdbeMemStringify(db, pRec, enc, 1);
            }
        }
        pRec.flags &= !(MEM_Real | MEM_Int | MEM_IntReal);
    }
}

/// C: `int sqlite3_value_numeric_type(sqlite3_value *pVal)`
pub fn sqlite3_value_numeric_type(db: &mut Sqlite3, pVal: &mut Mem) -> i32 {
    let mut eType = sqlite3_value_type(pVal);
    if eType == SQLITE_TEXT {
        applyNumericAffinity(db, pVal, 0);
        eType = sqlite3_value_type(pVal);
    }
    eType
}

/// C: `void sqlite3ValueApplyAffinity( sqlite3_value *pVal, u8 affinity, u8 enc )`
/// Rust: takes the connection (needed by the numeric conversions).
pub fn sqlite3ValueApplyAffinity(db: &mut Sqlite3, pVal: &mut Mem, affinity: u8, enc: u8) {
    applyAffinity(db, pVal, affinity, enc);
}

/// pMem currently only holds a string type (or maybe a BLOB that we can
/// interpret as a string if we want to).  Compute its corresponding
/// numeric type (computeNumericType()).
fn computeNumericType(db: &mut Sqlite3, pMem: &mut Mem) -> u16 {
    debug_assert!((pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal)) == 0);
    debug_assert!((pMem.flags & (MEM_Str | MEM_Blob)) != 0);
    if ExpandBlob(db, pMem) != 0 {
        pMem.u.i = 0;
        return MEM_Int;
    }
    let mut r = 0.0f64;
    let rc = sqlite3MemRealValueRC(db, pMem, &mut r);
    pMem.u.r = r;
    let mut ix = 0i64;
    if rc <= 0 {
        if (rc & 2) == 0
            && crate::util::sqlite3Atoi64(Some(pMem.bytes()), &mut ix, pMem.n, pMem.enc) <= 1
        {
            pMem.u.i = ix;
            return MEM_Int;
        } else {
            return MEM_Real;
        }
    } else if (rc & 2) == 0
        && crate::util::sqlite3Atoi64(Some(pMem.bytes()), &mut ix, pMem.n, pMem.enc) == 0
    {
        pMem.u.i = ix;
        return MEM_Int;
    }
    MEM_Real
}

/// Return the numeric type for pMem, either MEM_Int or MEM_Real or both or
/// none (numericType()).
fn numericType(db: &mut Sqlite3, pMem: &mut Mem) -> u16 {
    if (pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal | MEM_Null)) != 0 {
        return pMem.flags & (MEM_Int | MEM_Real | MEM_IntReal | MEM_Null);
    }
    debug_assert!((pMem.flags & (MEM_Str | MEM_Blob)) != 0);
    computeNumericType(db, pMem)
}

/// C: `void sqlite3VdbeMemPrettyPrint(Mem *pMem, StrAccum *pStr)`
/// Write a nice string representation of the contents of cell pMem into
/// buffer zBuf (SQLITE_DEBUG helper).
pub fn sqlite3VdbeMemPrettyPrint(pMem: &mut Mem, pStr: &mut StrAccum) {
    use crate::printf::{sqlite3_str_appendchar, sqlite3_str_appendf};
    let f = pMem.flags;
    const ENCNAMES: [&[u8]; 4] = [b"(X)", b"(8)", b"(16LE)", b"(16BE)"];
    let kind = |f: u16| -> u8 {
        if (f & MEM_Dyn) != 0 {
            b'z'
        } else if (f & MEM_Static) != 0 {
            b't'
        } else if (f & MEM_Ephem) != 0 {
            b'e'
        } else {
            b's'
        }
    };
    let z = pMem.bytes().to_vec();
    if (f & MEM_Blob) != 0 {
        let c = kind(f);
        sqlite3_str_appendf(pStr, b"%cx[", &[PfArg::from(c as i32)]);
        for &b in z.iter().take(25) {
            sqlite3_str_appendf(pStr, b"%02X", &[PfArg::from(b as i32)]);
        }
        sqlite3_str_appendf(pStr, b"|", &[]);
        for &b in z.iter().take(25) {
            sqlite3_str_appendchar(pStr, 1, if b < 32 || b > 126 { b'.' } else { b });
        }
        sqlite3_str_appendf(pStr, b"]", &[]);
        if (f & MEM_Zero) != 0 {
            sqlite3_str_appendf(pStr, b"+%dz", &[PfArg::from(pMem.u.nZero)]);
        }
    } else if (f & MEM_Str) != 0 {
        let c = kind(f);
        sqlite3_str_appendf(
            pStr,
            b" %c%d[",
            &[PfArg::from(c as i32), PfArg::from(pMem.n)],
        );
        for &b in z.iter().take(25) {
            sqlite3_str_appendchar(pStr, 1, if b >= 0x20 && b <= 0x7f { b } else { b'.' });
        }
        sqlite3_str_appendf(
            pStr,
            b"]%s",
            &[PfArg::from(ENCNAMES[(pMem.enc & 3) as usize])],
        );
        if (f & MEM_Term) != 0 {
            sqlite3_str_appendf(pStr, b"(0-term)", &[]);
        }
    }
}

/// Print the value of a register for tracing purposes (memTracePrint()).
fn memTracePrint(p: &mut Mem) -> String {
    let mut out = String::new();
    if p.flags == MEM_Undefined {
        out.push_str(" undefined");
    } else if (p.flags & MEM_Null) != 0 {
        out.push_str(if (p.flags & MEM_Zero) != 0 {
            " NULL-nochng"
        } else {
            " NULL"
        });
    } else if (p.flags & (MEM_Int | MEM_Str)) == (MEM_Int | MEM_Str) {
        out.push_str(&format!(" si:{}", p.u.i));
    } else if (p.flags & MEM_IntReal) != 0 {
        out.push_str(&format!(" ir:{}", p.u.i));
    } else if (p.flags & MEM_Int) != 0 {
        out.push_str(&format!(" i:{}", p.u.i));
    } else if (p.flags & MEM_Real) != 0 {
        out.push_str(&format!(" r:{:.17e}", p.u.r));
    } else if sqlite3VdbeMemIsRowSet(p) != 0 {
        out.push_str(" (rowset)");
    } else {
        let mut acc = StrAccum::default();
        crate::printf::sqlite3StrAccumInit(&mut acc, 1000, 0);
        sqlite3VdbeMemPrettyPrint(p, &mut acc);
        let s = crate::printf::sqlite3StrAccumFinish(&mut acc).unwrap_or_default();
        out.push(' ');
        out.push_str(&String::from_utf8_lossy(&s));
    }
    if (p.flags & MEM_Subtype) != 0 {
        out.push_str(&format!(" subtype=0x{:02x}", p.eSubtype));
    }
    out
}

/// C: `void sqlite3VdbeRegisterDump(Vdbe *v)`
/// Show the values of all registers in the virtual machine (debugging
/// aid).
pub fn sqlite3VdbeRegisterDump(v: &mut Vdbe) {
    let n = (v.nMem as usize).min(v.aMem.len());
    for i in 1..n {
        let s = memTracePrint(&mut v.aMem[i]);
        println!("R[{}] = {}", i, s);
    }
}

/// Return the register of OP_xxx P2 after "pre-releasing" it: the output
/// register of an "out2" opcode becomes an integer (out2Prerelease()).
#[inline]
fn out2Prerelease(pOut: &mut Mem) -> &mut Mem {
    if pOut.VdbeMemDynamic() {
        sqlite3VdbeMemSetNull(pOut);
    }
    pOut.flags = MEM_Int;
    pOut
}

/// Compute a bloom filter hash using pOp->p4.i registers from aMem[]
/// beginning at pOp->p3 (filterHash()).
fn filterHash(db: &mut Sqlite3, aMem: &[Mem], p3: i32, n: i32) -> u64 {
    let mut h: u64 = 0;
    for i in p3..p3 + n {
        let p = &aMem[i as usize];
        if (p.flags & (MEM_Int | MEM_IntReal)) != 0 {
            h = h.wrapping_add(p.u.i as u64);
        } else if (p.flags & MEM_Real) != 0 {
            h = h.wrapping_add(sqlite3VdbeIntValue(db, p) as u64);
        } else if (p.flags & MEM_Str) != 0 {
            let z = p.bytes();
            let nn = z.len();
            h = h.wrapping_add(p.n as i64 as u64);
            if nn >= 8 {
                let mut x = [0u8; 8];
                x.copy_from_slice(&z[..8]);
                h = h.wrapping_add(u64::from_ne_bytes(x));
                x.copy_from_slice(&z[nn - 8..nn]);
                h = h.wrapping_add(u64::from_ne_bytes(x));
            } else {
                let mut x = [0u8; 8];
                x[..nn].copy_from_slice(z);
                h = h.wrapping_add(u64::from_ne_bytes(x));
            }
        } else if (p.flags & MEM_Blob) != 0 {
            let z = p.bytes();
            let nn = z.len();
            if nn > 0 {
                let mut x = [0u8; 8];
                let k = nn.min(8);
                x[..k].copy_from_slice(&z[..k]);
                h = h.wrapping_add(u64::from_ne_bytes(x));
            }
            h = h.wrapping_add(p.n as i64 as u64);
            if (p.flags & MEM_Zero) != 0 {
                h = h.wrapping_add(p.u.nZero as i64 as u64);
            }
        }
    }
    h
}

/// For OP_Column, factor out the case where content is loaded from
/// overflow pages, so that the code to implement this case is separate
/// the common case where content is on the same page
/// (vdbeColumnFromOverflow()).
fn vdbeColumnFromOverflow(
    db: &mut Sqlite3,
    pC: &mut VdbeCursor,
    iCol: i32,
    t: u32,
    iOffset: i64,
    cacheStatus: u32,
    colCacheCtr: u32,
    pDest: &mut Mem,
) -> i32 {
    let encoding = pDest.enc;
    let len = sqlite3VdbeSerialTypeLen(t) as i32;
    debug_assert!(pC.eCurType == CURTYPE_BTREE);
    if len > db.aLimit[SQLITE_LIMIT_LENGTH as usize] {
        return SQLITE_TOOBIG;
    }
    let pCrsr = pC.pCursor.clone().expect("btree cursor");
    if len > 4000 && pC.pKeyInfo.is_none() {
        // Cache large column values that are on overflow pages so that if
        // they are reloaded, they do not have to be read a second time.
        if pC.colCache == 0 {
            pC.pCache = Some(Box::default());
            pC.colCache = 1;
        }
        let pCache = pC.pCache.as_mut().unwrap();
        let ofst = sqlite3BtreeOffset(&pCrsr);
        if pCache.pCValue.is_empty()
            || pCache.iCol != iCol
            || pCache.cacheStatus != cacheStatus
            || pCache.colCacheCtr != colCacheCtr
            || pCache.iOffset != ofst
        {
            let mut buf = vec![0u8; len as usize + 3];
            let rc =
                sqlite3BtreePayload(&pCrsr, iOffset as u32, len as u32, &mut buf[..len as usize]);
            if rc != 0 {
                pCache.pCValue = Vec::new();
                return rc;
            }
            pCache.pCValue = buf;
            pCache.iCol = iCol;
            pCache.cacheStatus = cacheStatus;
            pCache.colCacheCtr = colCacheCtr;
            pCache.iOffset = ofst;
        }
        let v = pCache.pCValue[..len as usize].to_vec();
        let rc;
        if (t & 1) != 0 {
            rc = sqlite3VdbeMemSetStr(db, pDest, Some(&v), len as i64, encoding);
            pDest.flags |= MEM_Term;
        } else {
            rc = sqlite3VdbeMemSetStr(db, pDest, Some(&v), len as i64, 0);
            pDest.enc = encoding;
        }
        pDest.flags &= !MEM_Ephem;
        rc
    } else {
        let rc = sqlite3VdbeMemFromBtree(&pCrsr, iOffset as u32, len as u32, pDest);
        if rc != 0 {
            return rc;
        }
        let buf = std::mem::take(&mut pDest.z);
        sqlite3VdbeSerialGet(&buf, t, pDest);
        if pDest.z.is_empty() && (pDest.flags & (MEM_Str | MEM_Blob)) != 0 {
            pDest.z = buf;
        }
        if (t & 1) != 0 && encoding == SQLITE_UTF8 {
            pDest.flags |= MEM_Term;
        }
        pDest.flags &= !MEM_Ephem;
        rc
    }
}

/// Send a "statement aborts" message to the error log
/// (sqlite3VdbeLogAbort()).
fn sqlite3VdbeLogAbort(p: &Vdbe, rc: i32, pc: i32) {
    let zSql = p.zSql.clone().unwrap_or_default();
    let mut zPrefix: Vec<u8> = Vec::new();
    if p.pFrame.is_some() {
        match p.aOp.first().map(|o| &o.p4) {
            Some(P4::Z(z)) if z.len() >= 3 => {
                zPrefix.extend_from_slice(b"/* ");
                zPrefix.extend_from_slice(&z[3..]);
                zPrefix.extend_from_slice(b" */ ");
            }
            _ => zPrefix.extend_from_slice(b"/* unknown trigger */ "),
        }
    }
    let zErr = p.zErrMsg.clone().unwrap_or_default();
    crate::printf::sqlite3_log(
        rc,
        b"statement aborts at %d: %s; [%s%s]",
        &[
            PfArg::from(pc),
            PfArg::from(&zErr),
            PfArg::from(&zPrefix),
            PfArg::from(&zSql),
        ],
    );
}

/// Return the symbolic name for the data type of a pMem
/// (vdbeMemTypeName()).
fn vdbeMemTypeName(pMem: &mut Mem) -> &'static [u8] {
    const AZ_TYPES: [&[u8]; 5] = [b"INT", b"REAL", b"TEXT", b"BLOB", b"NULL"];
    AZ_TYPES[(sqlite3_value_type(pMem) - 1) as usize]
}

/// Access the RowSet object stored in a register by
/// sqlite3VdbeMemSetRowSet() (vdbemem.rs keeps it in `Mem::pPtr` as an
/// `Rc<RefCell<RowSet>>`; the register has MEM_Blob|MEM_Dyn set).
fn mem_rowset(m: &Mem) -> Option<Rc<std::cell::RefCell<crate::rowset::RowSet>>> {
    crate::vdbemem::sqlite3VdbeMemRowSet(m)
}

/// Result of vdbeIndexKeyCompare(): true if the key of the index entry
/// pCsr points to equals the record in pMem.
fn vdbeIndexKeyCompare(pCsr: &BtCursorP, pMem: &Mem, pRc: &mut i32) -> bool {
    let nKey = sqlite3BtreePayloadSize(pCsr);
    let mut ret = false;
    if nKey == pMem.n as u32 && (pMem.flags & MEM_Blob) != 0 {
        let mut m = Mem::default();
        *pRc = sqlite3VdbeMemFromBtreeZeroOffset(pCsr, nKey, &mut m);
        ret =
            *pRc != SQLITE_OK || pMem.bytes() == &m.bytes()[..(nKey as usize).min(m.bytes().len())];
        sqlite3VdbeMemReleaseMalloc(&mut m);
    }
    ret
}

// ---------------------------------------------------------------------------
// The interpreter
// ---------------------------------------------------------------------------

/// How the main loop continues after an opcode.
enum Fl {
    /// Go on with the instruction after s.pc.
    Next,
    /// check_for_interrupt, then go on with the instruction after s.pc.
    Check,
}

/// Non-local exits of an opcode (the C goto labels).
enum Ex {
    /// abort_due_to_error (s.rc holds the error code)
    Error,
    /// no_mem
    NoMem,
    /// too_big
    TooBig,
    /// abort_due_to_interrupt
    Interrupt,
    /// vdbe_return (s.rc holds the return code)
    Return,
}

/// Local variables of sqlite3VdbeExec() that live across opcodes.
struct ExecState {
    /// Address of the current instruction (C: pOp - aOp).
    pc: i32,
    /// Value to return.
    rc: i32,
    /// Reset schema after an error if positive.
    resetSchemaOnFault: u8,
    /// The database encoding.
    encoding: u8,
    /// Result of last comparison.
    iCompare: i32,
    /// Number of virtual machine steps.
    nVmStep: u64,
    /// Invoke xProgress() when nVmStep reaches this.
    nProgressLimit: u64,
    /// Column cache counter.
    colCacheCtr: u32,
}

/// The check_for_interrupt label: test the interrupt flag and invoke the
/// progress callback if needed.
fn check_for_interrupt(db: &mut Sqlite3, s: &mut ExecState) -> Result<(), Ex> {
    if db.isInterrupted.get() != 0 {
        return Err(Ex::Interrupt);
    }
    progress_check(db, s)
}

/// Call the progress callback if it is configured and the required number
/// of VDBE ops have been executed.
fn progress_check(db: &mut Sqlite3, s: &mut ExecState) -> Result<(), Ex> {
    while s.nVmStep >= s.nProgressLimit && db.xProgress.is_some() {
        debug_assert!(db.nProgressOps != 0);
        s.nProgressLimit = s.nProgressLimit.wrapping_add(db.nProgressOps as u64);
        let x = db.xProgress.clone().unwrap();
        if x() != 0 {
            s.nProgressLimit = LARGEST_UINT64;
            s.rc = SQLITE_INTERRUPT;
            return Err(Ex::Error);
        }
    }
    Ok(())
}

/// C: `int sqlite3VdbeExec( Vdbe *p )`
///
/// Execute as much of a VDBE program as we can.  This is the core of
/// sqlite3_step().  The statement has been taken out of its db.aVdbe slot
/// by the caller, so `db` and `p` are disjoint.
pub fn sqlite3VdbeExec(db: &mut Sqlite3, p: &mut Vdbe) -> i32 {
    let mut s = ExecState {
        pc: p.pc,
        rc: SQLITE_OK,
        resetSchemaOnFault: 0,
        encoding: db.ENC(),
        iCompare: 0,
        nVmStep: 0,
        nProgressLimit: LARGEST_UINT64,
        colCacheCtr: 0,
    };
    debug_assert!(p.eVdbeState == VDBE_RUN_STATE);
    if p.lockMask != 0 {
        sqlite3VdbeEnter(db, p);
    }
    if db.xProgress.is_some() {
        let iPrior = p.aCounter[SQLITE_STMTSTATUS_VM_STEP as usize];
        debug_assert!(0 < db.nProgressOps);
        s.nProgressLimit = (db.nProgressOps - (iPrior % db.nProgressOps)) as u64;
    } else {
        s.nProgressLimit = LARGEST_UINT64;
    }
    let mut exit: Ex;
    'run: {
        if p.rc == SQLITE_NOMEM {
            // This happens if a malloc() inside a call to
            // sqlite3_column_text() or sqlite3_column_text16() failed.
            exit = Ex::NoMem;
            break 'run;
        }
        debug_assert!(p.rc == SQLITE_OK || (p.rc & 0xff) == SQLITE_BUSY);
        p.rc = SQLITE_OK;
        p.iCurrentTime = 0;
        debug_assert!(p.explain == 0);
        db.busyHandler.borrow_mut().nBusy = 0;
        if db.isInterrupted.get() != 0 {
            exit = Ex::Interrupt;
            break 'run;
        }
        loop {
            // Errors are detected by individual opcodes, with an immediate
            // jump to abort_due_to_error.
            debug_assert!(s.rc == SQLITE_OK);
            s.nVmStep += 1;
            match vdbe_step(db, p, &mut s) {
                Ok(Fl::Next) => {}
                Ok(Fl::Check) => {
                    if let Err(e) = check_for_interrupt(db, &mut s) {
                        exit = e;
                        break 'run;
                    }
                }
                Err(e) => {
                    exit = e;
                    break 'run;
                }
            }
            s.pc += 1;
        }
    }
    loop {
        match exit {
            Ex::TooBig => {
                verr!(db, p, b"string or blob too big");
                s.rc = SQLITE_TOOBIG;
                exit = Ex::Error;
            }
            Ex::NoMem => {
                db.mallocFailed = 1;
                verr!(db, p, b"out of memory");
                s.rc = SQLITE_NOMEM_BKPT;
                exit = Ex::Error;
            }
            Ex::Interrupt => {
                s.rc = SQLITE_INTERRUPT;
                exit = Ex::Error;
            }
            Ex::Error => {
                // abort_due_to_error: execution is finished with an error.
                let mut rc = s.rc;
                if db.mallocFailed != 0 {
                    rc = SQLITE_NOMEM_BKPT;
                } else if rc == SQLITE_IOERR_CORRUPTFS {
                    rc = corrupt_bkpt();
                }
                if p.zErrMsg.is_none() && rc != SQLITE_IOERR_NOMEM {
                    let z = crate::mainapi::sqlite3ErrStr(rc);
                    verr!(db, p, b"%s", z.as_deref());
                }
                p.rc = rc;
                crate::util::sqlite3SystemError(db, rc);
                sqlite3VdbeLogAbort(p, rc, s.pc);
                if p.eVdbeState == VDBE_RUN_STATE {
                    sqlite3VdbeHalt(db, p);
                }
                if rc == SQLITE_IOERR_NOMEM {
                    db.mallocFailed = 1;
                }
                if rc == SQLITE_CORRUPT && db.autoCommit == 0 {
                    db.flags |= SQLITE_CorruptRdOnly;
                }
                s.rc = SQLITE_ERROR;
                if s.resetSchemaOnFault > 0 {
                    crate::build::sqlite3ResetOneSchema(db, s.resetSchemaOnFault as i32 - 1);
                }
                exit = Ex::Return;
            }
            Ex::Return => {
                // vdbe_return: the only way out of this procedure.
                if let Err(e) = progress_check(db, &mut s) {
                    exit = e;
                    continue;
                }
                let c = &mut p.aCounter[SQLITE_STMTSTATUS_VM_STEP as usize];
                *c = c.wrapping_add(s.nVmStep as u32);
                if p.lockMask != 0 {
                    sqlite3VdbeLeave(p);
                }
                return s.rc;
            }
        }
    }
}

/// Collating sequence of the OP_CollSeq immediately before instruction pc
/// (what sqlite3GetFuncCollSeq() finds through pCtx->iOp-1).
fn func_coll(p: &Vdbe, pc: i32) -> Option<CollSeqRef> {
    if pc > 0 {
        let o = &p.aOp[(pc - 1) as usize];
        if o.opcode == OP_CollSeq {
            if let P4::Coll(c) = &o.p4 {
                return Some(c.clone());
            }
        }
    }
    None
}

/// Execute one instruction (the body of the C `switch( pOp->opcode )`).
fn vdbe_step(db: &mut Sqlite3, p: &mut Vdbe, s: &mut ExecState) -> Result<Fl, Ex> {
    let pc = s.pc;
    // There is an implied "Halt 0 0 0" instruction at the very end of every
    // program.
    let (opcode, p1, p2, p3, p5, p4type) = match p.aOp.get(pc as usize) {
        Some(o) => (o.opcode, o.p1, o.p2, o.p3, o.p5, o.p4type),
        None => (OP_Halt, 0, 0, 0, 0u16, P4_NOTUSED),
    };
    let encoding = s.encoding;

    macro_rules! jump_to_p2 {
        () => {{
            s.pc = p2 - 1;
            return Ok(Fl::Next);
        }};
    }
    macro_rules! jump_to_p2_and_check_for_interrupt {
        () => {{
            s.pc = p2 - 1;
            return Ok(Fl::Check);
        }};
    }
    macro_rules! abort {
        ($rc:expr) => {{
            s.rc = $rc;
            return Err(Ex::Error);
        }};
    }
    macro_rules! chk {
        ($e:expr) => {{
            let rc_: i32 = $e;
            if rc_ != SQLITE_OK {
                abort!(rc_);
            }
        }};
    }
    macro_rules! vdbe_return {
        ($rc:expr) => {{
            s.rc = $rc;
            return Err(Ex::Return);
        }};
    }
    macro_rules! mem {
        ($i:expr) => {
            p.aMem[($i) as usize]
        };
    }
    macro_rules! csr {
        ($i:expr) => {
            p.apCsr[($i) as usize]
                .as_deref_mut()
                .expect("VDBE cursor not open")
        };
    }
    macro_rules! p4 {
        () => {
            p.aOp[pc as usize].p4
        };
    }
    macro_rules! p4_i {
        () => {
            match &p.aOp[pc as usize].p4 {
                P4::I(i) => *i,
                _ => 0,
            }
        };
    }
    macro_rules! p4_z {
        () => {
            match &p.aOp[pc as usize].p4 {
                P4::Z(z) => Some(z.clone()),
                _ => None,
            }
        };
    }
    macro_rules! btcsr {
        ($c:expr) => {
            $c.pCursor.clone().expect("btree cursor")
        };
    }
    macro_rules! pbt {
        ($i:expr) => {
            db.aDb[($i) as usize]
                .pBt
                .clone()
                .expect("database not open")
        };
    }

    match opcode {
        // Opcode:  Goto * P2 * * *
        //
        // An unconditional jump to address P2.
        OP_Goto => {
            jump_to_p2_and_check_for_interrupt!();
        }

        // Opcode:  Gosub P1 P2 * * *
        //
        // Write the current address onto register P1 and then jump to
        // address P2.
        OP_Gosub => {
            let pIn1 = &mut mem!(p1);
            debug_assert!(!pIn1.VdbeMemDynamic());
            pIn1.flags = MEM_Int;
            pIn1.u.i = pc as i64;
            jump_to_p2_and_check_for_interrupt!();
        }

        // Opcode:  Return P1 P2 P3 * *
        //
        // Jump to the address stored in register P1.  If P3 is 1, the jump
        // is only taken if register P1 holds an integer.
        OP_Return => {
            let pIn1 = &mem!(p1);
            if (pIn1.flags & MEM_Int) != 0 {
                s.pc = pIn1.u.i as i32;
            }
            Ok(Fl::Next)
        }

        // Opcode: InitCoroutine P1 P2 P3 * *
        //
        // Set up register P1 so that it will Yield to the coroutine located
        // at address P3.  If P2!=0 jump over the coroutine to address P2.
        OP_InitCoroutine => {
            let pOut = &mut mem!(p1);
            debug_assert!(!pOut.VdbeMemDynamic());
            pOut.u.i = (p3 - 1) as i64;
            pOut.flags = MEM_Int;
            if p2 == 0 {
                return Ok(Fl::Next);
            }
            jump_to_p2!();
        }

        // Opcode:  EndCoroutine P1 * * * *
        //
        // The instruction at the address in register P1 is a Yield.  Jump to
        // the P2 parameter of that Yield.
        OP_EndCoroutine => {
            let iCaller = mem!(p1).u.i as usize;
            debug_assert!(p.aOp[iCaller].opcode == OP_Yield);
            let callerP2 = p.aOp[iCaller].p2;
            mem!(p1).u.i = (pc - 1) as i64;
            s.pc = callerP2 - 1;
            Ok(Fl::Next)
        }

        // Opcode:  Yield P1 P2 * * *
        //
        // Swap the program counter with the value in register P1.
        OP_Yield => {
            let pIn1 = &mut mem!(p1);
            debug_assert!(!pIn1.VdbeMemDynamic());
            pIn1.flags = MEM_Int;
            let pcDest = pIn1.u.i as i32;
            pIn1.u.i = pc as i64;
            s.pc = pcDest;
            Ok(Fl::Next)
        }

        // Opcode:  HaltIfNull  P1 P2 P3 P4 P5
        //
        // Check the value in register P3.  If it is NULL then Halt using
        // parameter P1, P2, and P4 as if this were a Halt instruction.
        OP_HaltIfNull => {
            if (mem!(p3).flags & MEM_Null) == 0 {
                return Ok(Fl::Next);
            }
            op_halt(db, p, s, pc, p1, p2, p3, p5, p4type)
        }

        // Opcode:  Halt P1 P2 P3 P4 P5
        //
        // Exit immediately.  All open cursors, etc are closed automatically.
        OP_Halt => op_halt(db, p, s, pc, p1, p2, p3, p5, p4type),

        // Opcode: Integer P1 P2 * * *
        //
        // The 32-bit integer value P1 is written into register P2.
        OP_Integer => {
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = p1 as i64;
            Ok(Fl::Next)
        }

        // Opcode: Int64 P1 P2 P3 * *
        //
        // The 64-bit integer value (P3<<32)|P1 is written into register P2.
        OP_Int64 => {
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = INT32_TO_64(p1, p3);
            Ok(Fl::Next)
        }

        // Opcode: Real P1 P2 P3 * *
        //
        // The 64-bit floating point value (P3<<32)|P1 is written into
        // register P2.
        OP_Real => {
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.flags = MEM_Real;
            pOut.u.r = f64::from_bits(INT32_TO_64(p1, p3) as u64);
            debug_assert!(!pOut.u.r.is_nan());
            Ok(Fl::Next)
        }

        // Opcode: String8 * P2 * P4 *
        //
        // P4 points to a nul terminated UTF-8 string.  This opcode is
        // transformed into an OP_String before it is executed for the
        // first time.
        OP_String8 | OP_String => {
            if opcode == OP_String8 {
                let n = match &p4!() {
                    P4::Z(z) => z.len() as i32,
                    _ => 0,
                };
                {
                    let o = &mut p.aOp[pc as usize];
                    o.p1 = n;
                }
                if n > db.aLimit[SQLITE_LIMIT_LENGTH as usize] {
                    out2Prerelease(&mut mem!(p2));
                    return Err(Ex::TooBig);
                }
                p.aOp[pc as usize].opcode = OP_String;
            }
            // Opcode: String P1 P2 P3 P4 P5
            //
            // The string value P4 of length P1 (bytes) is stored in
            // register P2.  If P3 is not zero and the content of register
            // P3 is equal to P5, then the datatype of the register P2 is
            // converted to BLOB.
            let n = p.aOp[pc as usize].p1;
            let pOut = out2Prerelease(&mut p.aMem[p2 as usize]);
            pOut.z.clear();
            if let P4::Z(z) = &p.aOp[pc as usize].p4 {
                let k = (n.max(0) as usize).min(z.len());
                pOut.z.extend_from_slice(&z[..k]);
            }
            pOut.flags = MEM_Str | MEM_Static | MEM_Term;
            pOut.n = n;
            pOut.enc = encoding;
            if p3 > 0 {
                let v = p.aMem[p3 as usize].u.i;
                debug_assert!((p.aMem[p3 as usize].flags & MEM_Int) != 0);
                if v == p5 as i64 {
                    p.aMem[p2 as usize].flags = MEM_Blob | MEM_Static | MEM_Term;
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: BeginSubrtn * P2 * * *
        // Opcode: Null P1 P2 P3 * *
        //
        // Write a NULL into registers P2.  If P3 greater than P2, then also
        // write NULLs into register P3 and every register in between P2 and
        // P3.  If P1 is not zero, then also set the MEM_Cleared flag.
        OP_BeginSubrtn | OP_Null => {
            let nullFlag = if p1 != 0 {
                MEM_Null | MEM_Cleared
            } else {
                MEM_Null
            };
            {
                let pOut = out2Prerelease(&mut mem!(p2));
                pOut.flags = nullFlag;
                pOut.n = 0;
            }
            let mut cnt = p3 - p2;
            let mut i = p2;
            while cnt > 0 {
                i += 1;
                let pOut = &mut mem!(i);
                sqlite3VdbeMemSetNull(pOut);
                pOut.flags = nullFlag;
                pOut.n = 0;
                cnt -= 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: SoftNull P1 * * * *
        //
        // Set register P1 to have the value NULL as seen by the OP_MakeRecord
        // instruction, but do not free any string or blob memory associated
        // with the register.
        OP_SoftNull => {
            let pOut = &mut mem!(p1);
            pOut.flags = (pOut.flags & !(MEM_Undefined | MEM_AffMask)) | MEM_Null;
            Ok(Fl::Next)
        }

        // Opcode: Blob P1 P2 * P4 *
        //
        // P4 points to a blob of data P1 bytes long.  Store this blob in
        // register P2.  If P4 is a NULL pointer, then construct a zero-filled
        // blob that is P1 bytes long in P2.
        OP_Blob => {
            let z = p4_z!();
            let pOut = out2Prerelease(&mut p.aMem[p2 as usize]);
            match z {
                None => {
                    sqlite3VdbeMemSetZeroBlob(pOut, p1);
                    if sqlite3VdbeMemExpandBlob(db, pOut) != 0 {
                        return Err(Ex::NoMem);
                    }
                }
                Some(z) => {
                    let k = (p1.max(0) as usize).min(z.len());
                    sqlite3VdbeMemSetStr(db, pOut, Some(&z[..k]), p1 as i64, 0);
                }
            }
            pOut.enc = encoding;
            Ok(Fl::Next)
        }

        // Opcode: Variable P1 P2 * * *
        //
        // Transfer the values of bound parameter P1 into register P2.
        OP_Variable => {
            debug_assert!(p1 > 0 && p1 <= p.nVar as i32);
            if sqlite3VdbeMemTooBig(db, &mut p.aVar[(p1 - 1) as usize]) != 0 {
                return Err(Ex::TooBig);
            }
            let v = p.aVar[(p1 - 1) as usize].clone();
            let pOut = &mut mem!(p2);
            if pOut.VdbeMemDynamic() {
                sqlite3VdbeMemSetNull(pOut);
            }
            let uTemp = pOut.uTemp;
            *pOut = v;
            pOut.uTemp = uTemp;
            pOut.flags &= !(MEM_Dyn | MEM_Ephem);
            pOut.flags |= MEM_Static | MEM_FromBind;
            Ok(Fl::Next)
        }

        // Opcode: Move P1 P2 P3 * *
        //
        // Move the P3 values in register P1..P1+P3-1 over into registers
        // P2..P2+P3-1.  Registers P1..P1+P3-1 are left holding a NULL.
        OP_Move => {
            let mut n = p3;
            let mut i1 = p1;
            let mut i2 = p2;
            debug_assert!(n > 0 && p1 > 0 && p2 > 0);
            loop {
                {
                    let (pOut, pIn1) = two_mut(&mut p.aMem, i2 as usize, i1 as usize);
                    sqlite3VdbeMemMove(db, pOut, pIn1);
                    if Deephemeralize(db, pOut) {
                        return Err(Ex::NoMem);
                    }
                }
                i1 += 1;
                i2 += 1;
                n -= 1;
                if n == 0 {
                    break;
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: Copy P1 P2 P3 * P5
        //
        // Make a copy of registers P1..P1+P3 into registers P2..P2+P3.  If
        // the 0x0002 bit of P5 is set then also clear the MEM_Subtype flag
        // in the destination.
        OP_Copy => {
            let mut n = p3;
            let mut i1 = p1;
            let mut i2 = p2;
            loop {
                {
                    let (pOut, pIn1) = two_mut(&mut p.aMem, i2 as usize, i1 as usize);
                    sqlite3VdbeMemShallowCopy(db, pOut, pIn1, MEM_Ephem as i32);
                    if Deephemeralize(db, pOut) {
                        return Err(Ex::NoMem);
                    }
                    if (pOut.flags & MEM_Subtype) != 0 && (p5 & 0x0002) != 0 {
                        pOut.flags &= !MEM_Subtype;
                    }
                }
                if n == 0 {
                    break;
                }
                n -= 1;
                i1 += 1;
                i2 += 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: SCopy P1 P2 * * *
        //
        // Make a shallow copy of register P1 into register P2.
        OP_SCopy => {
            let (pOut, pIn1) = two_mut(&mut p.aMem, p2 as usize, p1 as usize);
            sqlite3VdbeMemShallowCopy(db, pOut, pIn1, MEM_Ephem as i32);
            Ok(Fl::Next)
        }

        // Opcode: IntCopy P1 P2 * * *
        //
        // Transfer the integer value held in register P1 into register P2.
        OP_IntCopy => {
            let v = mem!(p1).u.i;
            debug_assert!((mem!(p1).flags & MEM_Int) != 0);
            sqlite3VdbeMemSetInt64(&mut mem!(p2), v);
            Ok(Fl::Next)
        }

        // Opcode: FkCheck * * * * *
        //
        // Halt with an SQLITE_CONSTRAINT error if there are any unresolved
        // foreign key constraint violations.
        OP_FkCheck => {
            chk!(sqlite3VdbeCheckFkImmediate(p));
            Ok(Fl::Next)
        }

        // Opcode: ResultRow P1 P2 * * *
        //
        // The registers P1 through P1+P2-1 contain a single row of results.
        // This opcode causes the sqlite3_step() call to terminate with an
        // SQLITE_ROW return code.
        OP_ResultRow => {
            debug_assert!(p.nResColumn as i32 == p2);
            p.cacheCtr = (p.cacheCtr.wrapping_add(2)) | 1;
            p.iResultRow = p1;
            if db.mallocFailed != 0 {
                return Err(Ex::NoMem);
            }
            if (db.mTrace as u32 & SQLITE_TRACE_ROW) != 0 {
                if let Some(x) = db.xTrace.clone() {
                    let z = p.zSql.clone().unwrap_or_default();
                    x(SQLITE_TRACE_ROW, &z);
                }
            }
            p.pc = pc + 1;
            vdbe_return!(SQLITE_ROW);
        }

        // Opcode: Concat P1 P2 P3 * *
        //
        // Add the text in register P1 onto the end of the text in register
        // P2 and store the result in register P3.
        OP_Concat => {
            debug_assert!(p1 != p3);
            let mut flags1 = mem!(p1).flags;
            if ((flags1 | mem!(p2).flags) & MEM_Null) != 0 {
                sqlite3VdbeMemSetNull(&mut mem!(p3));
                return Ok(Fl::Next);
            }
            if (flags1 & (MEM_Str | MEM_Blob)) == 0 {
                if sqlite3VdbeMemStringify(db, &mut mem!(p1), encoding, 0) != 0 {
                    return Err(Ex::NoMem);
                }
                flags1 = mem!(p1).flags & !MEM_Str;
            } else if (flags1 & MEM_Zero) != 0 {
                if sqlite3VdbeMemExpandBlob(db, &mut mem!(p1)) != 0 {
                    return Err(Ex::NoMem);
                }
                flags1 = mem!(p1).flags & !MEM_Str;
            }
            let mut flags2 = mem!(p2).flags;
            if (flags2 & (MEM_Str | MEM_Blob)) == 0 {
                if sqlite3VdbeMemStringify(db, &mut mem!(p2), encoding, 0) != 0 {
                    return Err(Ex::NoMem);
                }
                flags2 = mem!(p2).flags & !MEM_Str;
            } else if (flags2 & MEM_Zero) != 0 {
                if sqlite3VdbeMemExpandBlob(db, &mut mem!(p2)) != 0 {
                    return Err(Ex::NoMem);
                }
                flags2 = mem!(p2).flags & !MEM_Str;
            }
            let mut nByte: i64 = mem!(p1).n as i64 + mem!(p2).n as i64;
            if nByte > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64 {
                return Err(Ex::TooBig);
            }
            let mut z = Vec::with_capacity(nByte as usize + 2);
            z.extend_from_slice(mem!(p2).bytes());
            z.extend_from_slice(mem!(p1).bytes());
            if p3 != p2 {
                mem!(p2).flags = flags2;
            }
            mem!(p1).flags = flags1;
            if encoding > SQLITE_UTF8 {
                nByte &= !1;
            }
            z.truncate(nByte as usize);
            let pOut = &mut mem!(p3);
            if p3 != p2 && pOut.VdbeMemDynamic() {
                sqlite3VdbeMemSetNull(pOut);
            }
            pOut.z = z;
            pOut.MemSetTypeFlag(MEM_Str);
            pOut.flags |= MEM_Term;
            pOut.n = nByte as i32;
            pOut.enc = encoding;
            Ok(Fl::Next)
        }

        // Opcode: Add P1 P2 P3 * *
        // Opcode: Multiply P1 P2 P3 * *
        // Opcode: Subtract P1 P2 P3 * *
        // Opcode: Divide P1 P2 P3 * *
        // Opcode: Remainder P1 P2 P3 * *
        //
        // Arithmetic: r[P3] = r[P2] op r[P1].  Integer overflow falls back
        // to floating point; division by zero yields NULL.
        OP_Add | OP_Subtract | OP_Multiply | OP_Divide | OP_Remainder => {
            let mut type1 = mem!(p1).flags;
            let mut type2 = mem!(p2).flags;
            enum R {
                Int(i64),
                Real(f64),
                Null,
            }
            let int_math = |iA: i64, iB: i64| -> Option<R> {
                // None means "goto fp_math"
                let mut iB = iB;
                let mut iA = iA;
                match opcode {
                    OP_Add => {
                        if crate::util::sqlite3AddInt64(&mut iB, iA) != 0 {
                            return None;
                        }
                    }
                    OP_Subtract => {
                        if crate::util::sqlite3SubInt64(&mut iB, iA) != 0 {
                            return None;
                        }
                    }
                    OP_Multiply => {
                        if crate::util::sqlite3MulInt64(&mut iB, iA) != 0 {
                            return None;
                        }
                    }
                    OP_Divide => {
                        if iA == 0 {
                            return Some(R::Null);
                        }
                        if iA == -1 && iB == SMALLEST_INT64 {
                            return None;
                        }
                        iB /= iA;
                    }
                    _ => {
                        if iA == 0 {
                            return Some(R::Null);
                        }
                        if iA == -1 {
                            iA = 1;
                        }
                        iB %= iA;
                    }
                }
                Some(R::Int(iB))
            };
            let mut res: Option<R> = None;
            let mut do_fp = false;
            if (type1 & type2 & MEM_Int) != 0 {
                res = int_math(mem!(p1).u.i, mem!(p2).u.i);
                do_fp = res.is_none();
            } else if ((type1 | type2) & MEM_Null) != 0 {
                res = Some(R::Null);
            } else {
                type1 = numericType(db, &mut mem!(p1));
                type2 = numericType(db, &mut mem!(p2));
                if (type1 & type2 & MEM_Int) != 0 {
                    res = int_math(mem!(p1).u.i, mem!(p2).u.i);
                    do_fp = res.is_none();
                } else {
                    do_fp = true;
                }
            }
            if do_fp {
                // fp_math:
                let rA = sqlite3VdbeRealValue(db, &mut mem!(p1));
                let mut rB = sqlite3VdbeRealValue(db, &mut mem!(p2));
                let mut isnull = false;
                match opcode {
                    OP_Add => rB += rA,
                    OP_Subtract => rB -= rA,
                    OP_Multiply => rB *= rA,
                    OP_Divide => {
                        if rA == 0.0 {
                            isnull = true;
                        } else {
                            rB /= rA;
                        }
                    }
                    _ => {
                        let mut iA = sqlite3VdbeIntValue(db, &mem!(p1));
                        let iB = sqlite3VdbeIntValue(db, &mem!(p2));
                        if iA == 0 {
                            isnull = true;
                        } else {
                            if iA == -1 {
                                iA = 1;
                            }
                            rB = (iB % iA) as f64;
                        }
                    }
                }
                res = Some(if isnull || rB.is_nan() {
                    R::Null
                } else {
                    R::Real(rB)
                });
            }
            let pOut = &mut mem!(p3);
            match res.unwrap() {
                R::Int(i) => {
                    pOut.u.i = i;
                    pOut.MemSetTypeFlag(MEM_Int);
                }
                R::Real(r) => {
                    pOut.u.r = r;
                    pOut.MemSetTypeFlag(MEM_Real);
                }
                R::Null => {
                    // arithmetic_result_is_null:
                    sqlite3VdbeMemSetNull(pOut);
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: CollSeq P1 * * P4
        //
        // P4 is a pointer to a CollSeq object.  If the next call to a user
        // function or aggregate calls sqlite3GetFuncCollSeq(), this
        // collation sequence will be returned.  If P1 is not zero, then it
        // is a register that a subsequent min() or max() aggregate will set
        // to 1 if the current row is not the minimum or maximum.
        OP_CollSeq => {
            if p1 != 0 {
                sqlite3VdbeMemSetInt64(&mut mem!(p1), 0);
            }
            Ok(Fl::Next)
        }

        // Opcode: BitAnd P1 P2 P3 * *
        // Opcode: BitOr P1 P2 P3 * *
        // Opcode: ShiftLeft P1 P2 P3 * *
        // Opcode: ShiftRight P1 P2 P3 * *
        OP_BitAnd | OP_BitOr | OP_ShiftLeft | OP_ShiftRight => {
            if ((mem!(p1).flags | mem!(p2).flags) & MEM_Null) != 0 {
                sqlite3VdbeMemSetNull(&mut mem!(p3));
                return Ok(Fl::Next);
            }
            let mut iA = sqlite3VdbeIntValue(db, &mem!(p2));
            let mut iB = sqlite3VdbeIntValue(db, &mem!(p1));
            let mut op = opcode;
            if op == OP_BitAnd {
                iA &= iB;
            } else if op == OP_BitOr {
                iA |= iB;
            } else if iB != 0 {
                debug_assert!(OP_ShiftRight == OP_ShiftLeft + 1);
                // If shifting by a negative amount, shift in the other
                // direction
                if iB < 0 {
                    op = 2 * OP_ShiftLeft + 1 - op;
                    iB = if iB > -64 { -iB } else { 64 };
                }
                if iB >= 64 {
                    iA = if iA >= 0 || op == OP_ShiftLeft { 0 } else { -1 };
                } else {
                    let mut uA = iA as u64;
                    if op == OP_ShiftLeft {
                        uA <<= iB;
                    } else {
                        uA >>= iB;
                        // Sign-extend on a right shift of a negative number
                        if iA < 0 {
                            uA |= u64::MAX << (64 - iB);
                        }
                    }
                    iA = uA as i64;
                }
            }
            let pOut = &mut mem!(p3);
            pOut.u.i = iA;
            pOut.MemSetTypeFlag(MEM_Int);
            Ok(Fl::Next)
        }

        // Opcode: AddImm  P1 P2 * * *
        //
        // Add the constant P2 to the value in register P1.
        OP_AddImm => {
            let pIn1 = &mut mem!(p1);
            sqlite3VdbeMemIntegerify(db, pIn1);
            pIn1.u.i = (pIn1.u.i as u64).wrapping_add(p2 as i64 as u64) as i64;
            Ok(Fl::Next)
        }

        // Opcode: MustBeInt P1 P2 * * *
        //
        // Force the value in register P1 to be an integer.  If it cannot be
        // converted without data loss jump to P2, or raise SQLITE_MISMATCH
        // if P2==0.
        OP_MustBeInt => {
            let pIn1 = &mut p.aMem[p1 as usize];
            if (pIn1.flags & MEM_Int) == 0 {
                if (pIn1.flags & MEM_Real) != 0 {
                    let r = pIn1.u.r;
                    if r >= -9223372036854775808.0 && r < 9223372036854775808.0 {
                        let ix = sqlite3RealToI64(r);
                        if ix as f64 == r {
                            pIn1.u.i = ix;
                            pIn1.MemSetTypeFlag(MEM_Int);
                        }
                    }
                } else {
                    applyAffinity(db, pIn1, SQLITE_AFF_NUMERIC, encoding);
                }
                if (pIn1.flags & MEM_Int) == 0 {
                    if p2 == 0 {
                        abort!(SQLITE_MISMATCH);
                    } else {
                        jump_to_p2!();
                    }
                }
            }
            pIn1.MemSetTypeFlag(MEM_Int);
            Ok(Fl::Next)
        }

        // Opcode: RealAffinity P1 * * * *
        //
        // If register P1 holds an integer convert it to a real value.
        OP_RealAffinity => {
            let pIn1 = &mut mem!(p1);
            if (pIn1.flags & (MEM_Int | MEM_IntReal)) != 0 {
                sqlite3VdbeMemRealify(db, pIn1);
            }
            Ok(Fl::Next)
        }

        // Opcode: Cast P1 P2 * * *
        //
        // Force the value in register P1 to be the type defined by P2.
        OP_Cast => {
            let pIn1 = &mut p.aMem[p1 as usize];
            chk!(ExpandBlob(db, pIn1));
            chk!(sqlite3VdbeMemCast(db, pIn1, p2 as u8, encoding));
            Ok(Fl::Next)
        }

        // Opcode: Eq P1 P2 P3 P4 P5 (and Ne, Lt, Le, Gt, Ge)
        //
        // Compare the values in register P1 and P3.  If reg(P3) op reg(P1)
        // then jump to address P2.
        OP_Eq | OP_Ne | OP_Lt | OP_Le | OP_Gt | OP_Ge => {
            use crate::global::{sqlite3aEQb, sqlite3aGTb, sqlite3aLTb};
            let mut flags1 = mem!(p1).flags;
            let mut flags3 = mem!(p3).flags;
            let res: i32;
            if (flags1 & flags3 & MEM_Int) != 0 {
                // Common case of comparison of two integers
                let (a3, a1) = (mem!(p3).u.i, mem!(p1).u.i);
                if a3 > a1 {
                    if sqlite3aGTb(opcode) != 0 {
                        jump_to_p2!();
                    }
                    s.iCompare = 1;
                } else if a3 < a1 {
                    if sqlite3aLTb(opcode) != 0 {
                        jump_to_p2!();
                    }
                    s.iCompare = -1;
                } else {
                    if sqlite3aEQb(opcode) != 0 {
                        jump_to_p2!();
                    }
                    s.iCompare = 0;
                }
                return Ok(Fl::Next);
            }
            if ((flags1 | flags3) & MEM_Null) != 0 {
                // One or both operands are NULL
                if (p5 & SQLITE_NULLEQ) != 0 {
                    // If SQLITE_NULLEQ is set (which will only happen if the
                    // operator is OP_Eq or OP_Ne) then take the jump or not
                    // depending on whether or not both operands are null.
                    if (flags1 & flags3 & MEM_Null) != 0 && (flags3 & MEM_Cleared) == 0 {
                        res = 0; // Operands are equal
                    } else {
                        res = if (flags3 & MEM_Null) != 0 { -1 } else { 1 };
                    }
                } else {
                    // SQLITE_NULLEQ is clear and at least one operand is
                    // NULL, then the result is always NULL.  The jump is
                    // taken if the SQLITE_JUMPIFNULL bit is set.
                    if (p5 & SQLITE_JUMPIFNULL) != 0 {
                        jump_to_p2!();
                    }
                    s.iCompare = 1; // Operands are not equal
                    return Ok(Fl::Next);
                }
            } else {
                // Neither operand is NULL and we couldn't do the special
                // high-speed integer comparison case.
                let affinity = (p5 as u8) & SQLITE_AFF_MASK;
                if affinity >= SQLITE_AFF_NUMERIC {
                    if ((flags1 | flags3) & MEM_Str) != 0 {
                        if (flags1 & (MEM_Int | MEM_IntReal | MEM_Real | MEM_Str)) == MEM_Str {
                            applyNumericAffinity(db, &mut mem!(p1), 0);
                            flags3 = mem!(p3).flags;
                        }
                        if (flags3 & (MEM_Int | MEM_IntReal | MEM_Real | MEM_Str)) == MEM_Str {
                            applyNumericAffinity(db, &mut mem!(p3), 0);
                        }
                    }
                } else if affinity == SQLITE_AFF_TEXT && ((flags1 | flags3) & MEM_Str) != 0 {
                    if (flags1 & MEM_Str) != 0 {
                        mem!(p1).flags &= !(MEM_Int | MEM_Real | MEM_IntReal);
                    } else if (flags1 & (MEM_Int | MEM_Real | MEM_IntReal)) != 0 {
                        sqlite3VdbeMemStringify(db, &mut mem!(p1), encoding, 1);
                        flags1 = (mem!(p1).flags & !MEM_TypeMask) | (flags1 & MEM_TypeMask);
                        if p1 == p3 {
                            flags3 = flags1 | MEM_Str;
                        }
                    }
                    if (flags3 & MEM_Str) != 0 {
                        mem!(p3).flags &= !(MEM_Int | MEM_Real | MEM_IntReal);
                    } else if (flags3 & (MEM_Int | MEM_Real | MEM_IntReal)) != 0 {
                        sqlite3VdbeMemStringify(db, &mut mem!(p3), encoding, 1);
                        flags3 = (mem!(p3).flags & !MEM_TypeMask) | (flags3 & MEM_TypeMask);
                    }
                }
                let pColl = match &p4!() {
                    P4::Coll(c) => Some(c.clone()),
                    _ => None,
                };
                res = sqlite3MemCompare(
                    db,
                    &p.aMem[p3 as usize],
                    &p.aMem[p1 as usize],
                    pColl.as_ref(),
                );
            }

            // At this point, res is negative, zero, or positive if reg[P1]
            // is less than, equal to, or greater than reg[P3].
            let res2 = if res < 0 {
                sqlite3aLTb(opcode)
            } else if res == 0 {
                sqlite3aEQb(opcode)
            } else {
                sqlite3aGTb(opcode)
            };
            s.iCompare = res;

            // Undo any changes made by applyAffinity() to the input
            // registers.
            mem!(p3).flags = flags3;
            mem!(p1).flags = flags1;

            if res2 != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: ElseEq * P2 * * *
        //
        // This opcode must follow an OP_Lt or OP_Gt comparison operator.  If
        // the result of the prior comparison was equal, jump to P2.
        OP_ElseEq => {
            if s.iCompare == 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: Permutation * * * P4 *
        //
        // Set the permutation used by the OP_Compare operator in the next
        // instruction.  The permutation is stored in the P4 operand.
        OP_Permutation => Ok(Fl::Next),

        // Opcode: Compare P1 P2 P3 P4 P5
        //
        // Compare two vectors of registers in reg(P1)..reg(P1+P3-1) and
        // reg(P2)..reg(P2+P3-1).  Save the result for the next OP_Jump.
        OP_Compare => {
            let aPermute: Option<Rc<Vec<u32>>> = if (p5 & OPFLAG_PERMUTE) == 0 {
                None
            } else {
                match &p.aOp[(pc - 1) as usize].p4 {
                    P4::Ai(a) => Some(a.clone()),
                    _ => None,
                }
            };
            let n = p3;
            let pKeyInfo = match &p4!() {
                P4::KeyInfo(k) => k.clone(),
                _ => panic!("OP_Compare without KeyInfo"),
            };
            let ki = pKeyInfo.borrow();
            for i in 0..n as usize {
                let idx = match &aPermute {
                    Some(a) => a[i + 1] as i32,
                    None => i as i32,
                };
                let pColl = ki.aColl.get(i).cloned().flatten();
                let sf = ki.aSortFlags.get(i).copied().unwrap_or(0);
                let bRev = (sf & KEYINFO_ORDER_DESC) != 0;
                let a1 = (p1 + idx) as usize;
                let a2 = (p2 + idx) as usize;
                s.iCompare = sqlite3MemCompare(db, &p.aMem[a1], &p.aMem[a2], pColl.as_ref());
                if s.iCompare != 0 {
                    if (sf & KEYINFO_ORDER_BIGNULL) != 0
                        && ((p.aMem[a1].flags & MEM_Null) != 0
                            || (p.aMem[a2].flags & MEM_Null) != 0)
                    {
                        s.iCompare = -s.iCompare;
                    }
                    if bRev {
                        s.iCompare = -s.iCompare;
                    }
                    break;
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: Jump P1 P2 P3 * *
        //
        // Jump to the instruction at address P1, P2, or P3 depending on
        // whether in the most recent OP_Compare instruction the P1 vector
        // was less than, equal to, or greater than the P2 vector.
        OP_Jump => {
            if s.iCompare < 0 {
                s.pc = p1 - 1;
            } else if s.iCompare == 0 {
                s.pc = p2 - 1;
            } else {
                s.pc = p3 - 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: And P1 P2 P3 * *
        // Opcode: Or P1 P2 P3 * *
        OP_And | OP_Or => {
            let mut v1 = sqlite3VdbeBooleanValue(&mut mem!(p1), 2);
            let v2 = sqlite3VdbeBooleanValue(&mut mem!(p2), 2);
            if opcode == OP_And {
                const AND_LOGIC: [i32; 9] = [0, 0, 0, 0, 1, 2, 0, 2, 2];
                v1 = AND_LOGIC[(v1 * 3 + v2) as usize];
            } else {
                const OR_LOGIC: [i32; 9] = [0, 1, 2, 1, 1, 1, 2, 1, 2];
                v1 = OR_LOGIC[(v1 * 3 + v2) as usize];
            }
            let pOut = &mut mem!(p3);
            if v1 == 2 {
                pOut.MemSetTypeFlag(MEM_Null);
            } else {
                pOut.u.i = v1 as i64;
                pOut.MemSetTypeFlag(MEM_Int);
            }
            Ok(Fl::Next)
        }

        // Opcode: IsTrue P1 P2 P3 P4 *
        //
        // This opcode implements the IS TRUE, IS FALSE, IS NOT TRUE, and IS
        // NOT FALSE operators.
        OP_IsTrue => {
            let v = sqlite3VdbeBooleanValue(&mut mem!(p1), p3) ^ p4_i!();
            sqlite3VdbeMemSetInt64(&mut mem!(p2), v as i64);
            Ok(Fl::Next)
        }

        // Opcode: Not P1 P2 * * *
        OP_Not => {
            if (mem!(p1).flags & MEM_Null) == 0 {
                let b = sqlite3VdbeBooleanValue(&mut mem!(p1), 0);
                sqlite3VdbeMemSetInt64(&mut mem!(p2), (b == 0) as i64);
            } else {
                sqlite3VdbeMemSetNull(&mut mem!(p2));
            }
            Ok(Fl::Next)
        }

        // Opcode: BitNot P1 P2 * * *
        OP_BitNot => {
            sqlite3VdbeMemSetNull(&mut mem!(p2));
            if (mem!(p1).flags & MEM_Null) == 0 {
                let v = !sqlite3VdbeIntValue(db, &mem!(p1));
                let pOut = &mut mem!(p2);
                pOut.flags = MEM_Int;
                pOut.u.i = v;
            }
            Ok(Fl::Next)
        }

        // Opcode: Once P1 P2 P3 * *
        //
        // Fall through to the next instruction the first time this opcode
        // is encountered on each invocation of the byte-code program.  Jump
        // to P2 on the second and all subsequent encounters.
        OP_Once => {
            if let Some(pFrame) = p.pFrame.as_deref_mut() {
                let iAddr = pc as usize;
                if pFrame.aOnce.len() <= iAddr / 8 {
                    pFrame.aOnce.resize(iAddr / 8 + 1, 0);
                }
                if (pFrame.aOnce[iAddr / 8] & (1 << (iAddr & 7))) != 0 {
                    jump_to_p2!();
                }
                pFrame.aOnce[iAddr / 8] |= 1 << (iAddr & 7);
            } else if p.aOp[0].p1 == p1 {
                jump_to_p2!();
            }
            let v = p.aOp[0].p1;
            p.aOp[pc as usize].p1 = v;
            Ok(Fl::Next)
        }

        // Opcode: If P1 P2 P3 * *
        OP_If => {
            let c = sqlite3VdbeBooleanValue(&mut mem!(p1), p3);
            if c != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IfNot P1 P2 P3 * *
        OP_IfNot => {
            let c = sqlite3VdbeBooleanValue(&mut mem!(p1), (p3 == 0) as i32) == 0;
            if c {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IsNull P1 P2 * * *
        OP_IsNull => {
            if (mem!(p1).flags & MEM_Null) != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IsType P1 P2 P3 P4 P5
        //
        // Jump to P2 if the type of a column in a btree is one of the types
        // specified by the P5 bitmask.
        OP_IsType => {
            let typeMask: u16;
            if p1 >= 0 {
                let p4i = p4_i!();
                let pC = csr!(p1);
                if p3 < pC.nHdrParsed as i32 {
                    let serialType = pC.aType[p3 as usize];
                    if serialType >= 12 {
                        typeMask = if (serialType & 1) != 0 { 0x04 } else { 0x08 };
                    } else {
                        const AMASK: [u16; 12] = [
                            0x10, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x2, 0x01, 0x01, 0x10, 0x10,
                        ];
                        typeMask = AMASK[serialType as usize];
                    }
                } else {
                    typeMask = 1 << (p4i - 1);
                }
            } else {
                typeMask = 1 << (sqlite3_value_type(&mut mem!(p3)) - 1);
            }
            if (typeMask & p5) != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: ZeroOrNull P1 P2 P3 * *
        OP_ZeroOrNull => {
            if (mem!(p1).flags & MEM_Null) != 0 || (mem!(p3).flags & MEM_Null) != 0 {
                sqlite3VdbeMemSetNull(&mut mem!(p2));
            } else {
                sqlite3VdbeMemSetInt64(&mut mem!(p2), 0);
            }
            Ok(Fl::Next)
        }

        // Opcode: NotNull P1 P2 * * *
        OP_NotNull => {
            if (mem!(p1).flags & MEM_Null) == 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IfNullRow P1 P2 P3 * *
        //
        // Check the cursor P1 to see if it is currently pointing at a NULL
        // row.  If it is, then set register P3 to NULL and jump immediately
        // to P2.
        OP_IfNullRow => {
            let isnull = match p.apCsr[p1 as usize].as_deref() {
                Some(c) => c.nullRow != 0,
                None => false,
            };
            if isnull {
                sqlite3VdbeMemSetNull(&mut mem!(p3));
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: Offset P1 P2 P3 * *
        //
        // Store in register r[P3] the byte offset into the database file
        // that is the start of the payload for the record at which that
        // cursor P1 is currently pointing (sqlite_offset()).
        OP_Offset => {
            let iCur = p1 as usize;
            let isbt = matches!(p.apCsr.get(iCur).and_then(|c| c.as_deref()), Some(c) if c.eCurType == CURTYPE_BTREE);
            if !isbt {
                sqlite3VdbeMemSetNull(&mut mem!(p3));
            } else {
                let pC = csr!(p1);
                if pC.deferredMoveto != 0 {
                    chk!(sqlite3VdbeFinishMoveto(db, pC));
                }
                let cur = btcsr!(pC);
                if sqlite3BtreeEof(&cur) != 0 {
                    sqlite3VdbeMemSetNull(&mut mem!(p3));
                } else {
                    sqlite3VdbeMemSetInt64(&mut mem!(p3), sqlite3BtreeOffset(&cur));
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: Column P1 P2 P3 P4 P5
        //
        // Interpret the data that cursor P1 points to as a structure built
        // using the MakeRecord instruction and extract the P2-th column.
        OP_Column => op_column(db, p, s, pc, p1, p2, p3, p5, p4type),

        // Opcode: TypeCheck P1 P2 P3 P4 *
        //
        // Apply affinities to the range of P2 registers beginning with P1.
        // Take the affinities from the Table object in P4.  If any value
        // cannot be coerced into the correct type, then raise an error.
        OP_TypeCheck => {
            let pTab = match &p4!() {
                P4::Tab(t) => t.clone(),
                _ => panic!("OP_TypeCheck without table"),
            };
            let (i0, nCol) = if p3 < 2 {
                (0, pTab.borrow().nCol as i32)
            } else {
                (p3 - 2, p3 - 1)
            };
            let mut iIn = p1;
            let mut i = i0;
            while i < nCol {
                let (colFlags, aff, eCType) = {
                    let t = pTab.borrow();
                    let c = &t.aCol[i as usize];
                    (c.colFlags, c.affinity, c.eCType)
                };
                if (colFlags & COLFLAG_GENERATED) != 0 && p3 < 2 {
                    if (colFlags & COLFLAG_VIRTUAL) != 0 {
                        i += 1;
                        continue;
                    }
                    if p3 != 0 {
                        iIn += 1;
                        i += 1;
                        continue;
                    }
                }
                let pIn1 = &mut p.aMem[iIn as usize];
                applyAffinity(db, pIn1, aff, encoding);
                if (pIn1.flags & MEM_Null) == 0 {
                    let bad = match eCType {
                        COLTYPE_BLOB => (pIn1.flags & MEM_Blob) == 0,
                        COLTYPE_INTEGER | COLTYPE_INT => (pIn1.flags & MEM_Int) == 0,
                        COLTYPE_TEXT => (pIn1.flags & MEM_Str) == 0,
                        COLTYPE_REAL => {
                            if (pIn1.flags & MEM_Int) != 0 {
                                // When applying REAL affinity, if the result
                                // is still an MEM_Int that will fit in 6
                                // bytes, then change the type to MEM_IntReal.
                                if pIn1.u.i <= 140737488355327 && pIn1.u.i >= -140737488355328 {
                                    pIn1.flags |= MEM_IntReal;
                                    pIn1.flags &= !MEM_Int;
                                } else {
                                    pIn1.u.r = pIn1.u.i as f64;
                                    pIn1.flags |= MEM_Real;
                                    pIn1.flags &= !MEM_Int;
                                }
                                false
                            } else {
                                (pIn1.flags & (MEM_Real | MEM_IntReal)) == 0
                            }
                        }
                        _ => false, // COLTYPE_ANY.  Accept anything.
                    };
                    if bad {
                        // vdbe_type_error:
                        let zType = vdbeMemTypeName(pIn1);
                        let (zTab, zCol) = {
                            let t = pTab.borrow();
                            (t.zName.clone(), t.aCol[i as usize].zCnName.clone())
                        };
                        let zStd = crate::global::sqlite3StdType[(eCType - 1) as usize];
                        verr!(
                            db,
                            p,
                            b"cannot store %s value in %s column %s.%s",
                            zType,
                            zStd,
                            &zTab,
                            &zCol
                        );
                        abort!(SQLITE_CONSTRAINT_DATATYPE);
                    }
                }
                iIn += 1;
                i += 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: Affinity P1 P2 * P4 *
        //
        // Apply affinities to a range of P2 registers starting with P1.
        // P4 is a string that is P2 characters long.
        OP_Affinity => {
            let zAffinity = p4_z!().unwrap_or_default();
            let mut i = p1 as usize;
            for (k, &aff) in zAffinity.iter().enumerate() {
                if aff == 0 {
                    break;
                }
                let pIn1 = &mut p.aMem[i];
                applyAffinity(db, pIn1, aff, encoding);
                if aff == SQLITE_AFF_REAL && (pIn1.flags & MEM_Int) != 0 {
                    // When applying REAL affinity, if the result is still an
                    // MEM_Int that will fit in 6 bytes, then change the type
                    // to MEM_IntReal so that we keep the high-resolution
                    // integer value but know that the type really wants to
                    // be REAL.
                    pIn1.u.r = pIn1.u.i as f64;
                    pIn1.flags |= MEM_Real;
                    pIn1.flags &= !(MEM_Int | MEM_Str);
                }
                if k + 1 >= zAffinity.len() || zAffinity[k + 1] == 0 {
                    break;
                }
                i += 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: MakeRecord P1 P2 P3 P4 *
        //
        // Convert P2 registers beginning with P1 into the record format use
        // as a data record in a database table or as a key in an index.
        OP_MakeRecord => op_makerecord(db, p, s, pc, p1, p2, p3, p5),

        // Opcode: Count P1 P2 P3 * *
        //
        // Store the number of entries (an integer value) in the table or
        // index opened by cursor P1 in register P2.
        OP_Count => {
            let pCrsr = btcsr!(csr!(p1));
            let mut nEntry: i64 = 0;
            if p3 != 0 {
                nEntry = sqlite3BtreeRowCountEst(db, &pCrsr);
            } else {
                chk!(sqlite3BtreeCount(db, &pCrsr, &mut nEntry));
            }
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = nEntry;
            Ok(Fl::Check)
        }

        // Opcode: Savepoint P1 * * P4 *
        //
        // Open, release or rollback the savepoint named by parameter P4.
        OP_Savepoint => op_savepoint(db, p, s, pc, p1),

        // Opcode: AutoCommit P1 P2 * * *
        //
        // Set the database auto-commit flag to P1 (1 or 0).  If P2 is true,
        // roll back any currently active btree transactions.
        OP_AutoCommit => {
            let desiredAutoCommit = p1;
            let iRollback = p2;
            debug_assert!(db.nVdbeActive > 0);
            if desiredAutoCommit != db.autoCommit as i32 {
                if iRollback != 0 {
                    crate::mainapi::sqlite3RollbackAll(db, SQLITE_ABORT_ROLLBACK);
                    db.autoCommit = 1;
                } else if desiredAutoCommit != 0 && db.nVdbeWrite > 0 {
                    // If this instruction implements a COMMIT and other VMs
                    // are writing return an error indicating that the other
                    // VMs must complete first.
                    verr!(
                        db,
                        p,
                        b"cannot commit transaction - SQL statements in progress"
                    );
                    abort!(SQLITE_BUSY);
                } else {
                    let rc = sqlite3VdbeCheckFkDeferred(db, p);
                    if rc != SQLITE_OK {
                        vdbe_return!(rc);
                    }
                    db.autoCommit = desiredAutoCommit as u8;
                }
                if sqlite3VdbeHalt(db, p) == SQLITE_BUSY {
                    p.pc = pc;
                    db.autoCommit = (1 - desiredAutoCommit) as u8;
                    p.rc = SQLITE_BUSY;
                    vdbe_return!(SQLITE_BUSY);
                }
                crate::mainapi::sqlite3CloseSavepoints(db);
                if p.rc == SQLITE_OK {
                    vdbe_return!(SQLITE_DONE);
                } else {
                    vdbe_return!(SQLITE_ERROR);
                }
            } else {
                let z: &[u8] = if desiredAutoCommit == 0 {
                    b"cannot start a transaction within a transaction"
                } else if iRollback != 0 {
                    b"cannot rollback - no transaction is active"
                } else {
                    b"cannot commit - no transaction is active"
                };
                verr!(db, p, z);
                abort!(SQLITE_ERROR);
            }
        }

        // Opcode: Transaction P1 P2 P3 P4 P5
        //
        // Begin a transaction on database P1 if a transaction is not already
        // active.  If P2 is non-zero, then a write-transaction is started.
        // If P5!=0 then this opcode also checks the schema cookie against P3
        // and the schema generation counter against P4.
        OP_Transaction => {
            let mut rc = SQLITE_OK;
            if p2 != 0 && (db.flags & (SQLITE_QueryOnly | SQLITE_CorruptRdOnly)) != 0 {
                if (db.flags & SQLITE_QueryOnly) != 0 {
                    // Writes prohibited by the "PRAGMA query_only=TRUE"
                    // statement
                    abort!(SQLITE_READONLY);
                } else {
                    // Writes prohibited due to a prior SQLITE_CORRUPT in the
                    // current transaction
                    abort!(SQLITE_CORRUPT);
                }
            }
            let mut iMeta: i32 = 0;
            if let Some(pBt) = db.aDb[p1 as usize].pBt.clone() {
                rc = sqlite3BtreeBeginTrans(db, &pBt, p2, &mut iMeta);
                if rc != SQLITE_OK {
                    if (rc & 0xff) == SQLITE_BUSY {
                        p.pc = pc;
                        p.rc = rc;
                        vdbe_return!(rc);
                    }
                    abort!(rc);
                }
                if p.usesStmtJournal != 0 && p2 != 0 && (db.autoCommit == 0 || db.nVdbeRead > 1) {
                    if p.iStatement == 0 {
                        db.nStatement += 1;
                        p.iStatement = db.nSavepoint + db.nStatement;
                    }
                    rc = crate::vtab::sqlite3VtabSavepoint(db, SAVEPOINT_BEGIN, p.iStatement - 1);
                    if rc == SQLITE_OK {
                        rc = sqlite3BtreeBeginStmt(db, &pBt, p.iStatement);
                    }
                    // Store the current value of the database handles
                    // deferred constraint counter.
                    p.nStmtDefCons = db.nDeferredCons;
                    p.nStmtDefImmCons = db.nDeferredImmCons;
                }
            }
            if rc == SQLITE_OK
                && p5 != 0
                && (iMeta != p3 || db.aDb[p1 as usize].pSchema.iGeneration != p4_i!())
            {
                // IMPLEMENTATION-OF: R-03189-51135 As each SQL statement
                // runs, the schema version is checked to ensure that the
                // schema has not changed since the SQL statement was
                // prepared.
                p.zErrMsg = Some(b"database schema has changed".to_vec());
                // If the schema-cookie from the database file matches the
                // cookie stored with the in-memory representation of the
                // schema, do not reload the schema from the database file.
                if db.aDb[p1 as usize].pSchema.schema_cookie != iMeta {
                    crate::build::sqlite3ResetOneSchema(db, p1);
                }
                p.expired = 1;
                rc = SQLITE_SCHEMA;
                // Set changeCntOn to 0 to prevent the value returned by
                // sqlite3_changes() from being modified in sqlite3VdbeHalt().
                p.changeCntOn = 0;
            }
            chk!(rc);
            Ok(Fl::Next)
        }

        // Opcode: ReadCookie P1 P2 P3 * *
        //
        // Read cookie number P3 from database P1 and write it into register
        // P2.
        OP_ReadCookie => {
            let mut iMeta: u32 = 0;
            sqlite3BtreeGetMeta(&pbt!(p1), p3, &mut iMeta);
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = iMeta as i32 as i64;
            Ok(Fl::Next)
        }

        // Opcode: SetCookie P1 P2 P3 * P5
        //
        // Write the integer value P3 into cookie number P2 of database P1.
        OP_SetCookie => {
            let pBt = pbt!(p1);
            let rc = sqlite3BtreeUpdateMeta(&pBt, p2, p3 as u32);
            if p2 == BTREE_SCHEMA_VERSION {
                // When the schema cookie changes, record the new cookie
                // internally
                db.aDb[p1 as usize].pSchema.schema_cookie =
                    (p3 as u32).wrapping_sub(p5 as u32) as i32;
                db.mDbFlags |= DBFLAG_SchemaChange;
                crate::fkey::sqlite3FkClearTriggerCache(db, p1);
            } else if p2 == BTREE_FILE_FORMAT {
                // Record changes in the file format
                db.aDb[p1 as usize].pSchema.file_format = p3 as u8;
            }
            if p1 == 1 {
                // Invalidate all prepared statements whenever the TEMP
                // database schema is changed.  Ticket #1644
                sqlite3ExpirePreparedStatements(db, 0);
                p.expired = 0;
            }
            chk!(rc);
            Ok(Fl::Next)
        }

        // Opcode: ReopenIdx P1 P2 P3 P4 P5
        //
        // The ReopenIdx opcode works like OP_OpenRead except that it first
        // checks to see if the cursor on P1 is already open on the same
        // b-tree and if it is this opcode becomes a no-op.
        OP_ReopenIdx | OP_OpenRead | OP_OpenWrite => {
            op_open(db, p, s, pc, opcode, p1, p2, p3, p5, p4type)
        }

        // Opcode: OpenDup P1 P2 * * *
        //
        // Open a new cursor P1 that points to the same ephemeral table as
        // cursor P2.
        OP_OpenDup => {
            let (nField, pKeyInfo, isTable, pgnoRoot, isOrdered, pBtx) = {
                let pOrig = csr!(p2);
                debug_assert!(pOrig.isEphemeral != 0);
                pOrig.noReuse = 1;
                (
                    pOrig.nField as i32,
                    pOrig.pKeyInfo.clone(),
                    pOrig.isTable,
                    pOrig.pgnoRoot,
                    pOrig.isOrdered,
                    pOrig.pBtx.clone(),
                )
            };
            allocateCursor(db, p, p1, nField, CURTYPE_BTREE);
            let pCx = csr!(p1);
            pCx.nullRow = 1;
            pCx.isEphemeral = 1;
            pCx.pKeyInfo = pKeyInfo.clone();
            pCx.isTable = isTable;
            pCx.pgnoRoot = pgnoRoot;
            pCx.isOrdered = isOrdered;
            pCx.pBtx = pBtx.clone();
            pCx.noReuse = 1;
            let bt = pBtx.expect("ephemeral btree");
            // The sqlite3BtreeCursor() routine can only fail for the first
            // cursor opened for a database.  Since there is already an open
            // cursor when this opcode is run, it cannot fail.
            let _ = sqlite3BtreeCursor(&bt, pgnoRoot, BTREE_WRCSR, pKeyInfo, &mut pCx.pCursor);
            Ok(Fl::Next)
        }

        // Opcode: OpenEphemeral P1 P2 P3 P4 P5
        // Opcode: OpenAutoindex P1 P2 * P4 *
        //
        // Open a new cursor P1 to a transient table.  The cursor is always
        // opened read/write even if the main database is read-only.
        OP_OpenAutoindex | OP_OpenEphemeral => {
            const VFS_FLAGS: i32 = SQLITE_OPEN_READWRITE
                | SQLITE_OPEN_CREATE
                | SQLITE_OPEN_EXCLUSIVE
                | SQLITE_OPEN_DELETEONCLOSE
                | SQLITE_OPEN_TRANSIENT_DB;
            if p3 > 0 {
                // Make register reg[P3] into a value that can be used as the
                // data form sqlite3BtreeInsert() where the length of the data
                // is zero.
                debug_assert!(p2 == 0);
                let m = &mut mem!(p3);
                m.n = 0;
                m.z.clear();
            }
            let reuse = match p.apCsr[p1 as usize].as_deref() {
                Some(c) => c.noReuse == 0 && p2 <= c.nField as i32,
                None => false,
            };
            let mut rc;
            if reuse {
                // If the ephemeral table is already open and has no
                // duplicates from OP_OpenDup, then erase all existing
                // content so that the table is empty again.
                let pCx = csr!(p1);
                pCx.seqCount = 0;
                pCx.cacheStatus = CACHE_STALE as u32;
                let bt = pCx.pBtx.clone().expect("ephemeral btree");
                let mut nChange = 0i64;
                rc = sqlite3BtreeClearTable(&bt, pCx.pgnoRoot as i32, &mut nChange);
            } else {
                allocateCursor(db, p, p1, p2, CURTYPE_BTREE);
                let pVfs = db.pVfs.clone();
                let pKeyInfo = match &p4!() {
                    P4::KeyInfo(k) => Some(k.clone()),
                    _ => None,
                };
                let mut pBtx: Option<BtreeP> = None;
                rc = sqlite3BtreeOpen(
                    db,
                    &pVfs,
                    None,
                    &mut pBtx,
                    BTREE_OMIT_JOURNAL | BTREE_SINGLE | p5 as i32,
                    VFS_FLAGS,
                );
                csr!(p1).isEphemeral = 1;
                csr!(p1).pBtx = pBtx.clone();
                if rc == SQLITE_OK {
                    let bt = pBtx.clone().unwrap();
                    let mut dummy = 0i32;
                    rc = sqlite3BtreeBeginTrans(db, &bt, 1, &mut dummy);
                    if rc == SQLITE_OK {
                        // If a transient index is required, create it by
                        // calling sqlite3BtreeCreateTable() with the
                        // BTREE_BLOBKEY flag before opening it.  If a
                        // transient table is required, just use the
                        // automatically created table with root-page 1.
                        let pCx = csr!(p1);
                        pCx.pKeyInfo = pKeyInfo.clone();
                        if let Some(ki) = pKeyInfo {
                            let mut pg: Pgno = 0;
                            rc = sqlite3BtreeCreateTable(&bt, &mut pg, BTREE_BLOBKEY | p5 as i32);
                            pCx.pgnoRoot = pg;
                            if rc == SQLITE_OK {
                                rc = sqlite3BtreeCursor(
                                    &bt,
                                    pg,
                                    BTREE_WRCSR,
                                    Some(ki),
                                    &mut pCx.pCursor,
                                );
                            }
                            pCx.isTable = 0;
                        } else {
                            pCx.pgnoRoot = SCHEMA_ROOT as Pgno;
                            rc = sqlite3BtreeCursor(
                                &bt,
                                SCHEMA_ROOT as Pgno,
                                BTREE_WRCSR,
                                None,
                                &mut pCx.pCursor,
                            );
                            pCx.isTable = 1;
                        }
                    }
                    csr!(p1).isOrdered = (p5 as i32 != BTREE_UNORDERED) as u8;
                    if rc != 0 {
                        crate::btree::sqlite3BtreeClose(db, &bt);
                        // Not required; helps with static analysis
                        p.apCsr[p1 as usize] = None;
                    }
                }
            }
            chk!(rc);
            csr!(p1).nullRow = 1;
            Ok(Fl::Next)
        }

        // Opcode: SorterOpen P1 P2 P3 P4 *
        //
        // This opcode works like OP_OpenEphemeral except that it opens a
        // transient index that is specifically designed to sort large
        // tables using an external merge-sort algorithm.
        OP_SorterOpen => {
            let pKeyInfo = match &p4!() {
                P4::KeyInfo(k) => Some(k.clone()),
                _ => None,
            };
            allocateCursor(db, p, p1, p2, CURTYPE_SORTER);
            let pCx = csr!(p1);
            pCx.pKeyInfo = pKeyInfo;
            chk!(crate::vdbesort::sqlite3VdbeSorterInit(db, p3, pCx));
            Ok(Fl::Next)
        }

        // Opcode: SequenceTest P1 P2 * * *
        //
        // P1 is a sorter cursor.  If the sequence counter is currently zero,
        // jump to P2.  Regardless of whether or not the jump is taken,
        // increment the the sequence value.
        OP_SequenceTest => {
            let pC = csr!(p1);
            let v = pC.seqCount;
            pC.seqCount += 1;
            if v == 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: OpenPseudo P1 P2 P3 * *
        //
        // Open a new cursor that points to a fake table that contains a
        // single row of data.  The content of that one row is the content
        // of memory register P2.
        OP_OpenPseudo => {
            allocateCursor(db, p, p1, p3, CURTYPE_PSEUDO);
            let pCx = csr!(p1);
            pCx.nullRow = 1;
            pCx.seekResult = p2;
            pCx.isTable = 1;
            // Give this pseudo-cursor a fake BtCursor pointer so that pCx
            // can be safely passed to sqlite3VdbeCursorMoveto().
            pCx.pCursor = sqlite3BtreeFakeValidCursor();
            Ok(Fl::Next)
        }

        // Opcode: Close P1 * * * *
        //
        // Close a cursor previously opened as P1.
        OP_Close => {
            if let Some(mut c) = p.apCsr[p1 as usize].take() {
                sqlite3VdbeFreeCursorNN(db, p, &mut c);
            }
            Ok(Fl::Next)
        }

        // Opcode: ColumnsUsed P1 P2 P3 * *
        //
        // (SQLITE_ENABLE_COLUMN_USED_MASK only.)
        OP_ColumnsUsed => Ok(Fl::Next),

        // Opcode: SeekGE P1 P2 P3 P4 * (and SeekGT, SeekLT, SeekLE)
        OP_SeekLT | OP_SeekLE | OP_SeekGE | OP_SeekGT => op_seek(db, p, s, pc, opcode, p1, p2, p3),

        // Opcode: SeekScan  P1 P2 * * P5
        //
        // This opcode is a prefix opcode to OP_SeekGE.  ...  Step the cursor
        // up to P1 times looking for a match before doing a full seek.
        OP_SeekScan => {
            let (n1p1, n1p3, n1p4) = {
                let o = &p.aOp[(pc + 1) as usize];
                let i = match &o.p4 {
                    P4::I(i) => *i,
                    _ => 0,
                };
                (o.p1, o.p3, i)
            };
            let pCrsr = btcsr!(csr!(n1p1));
            if sqlite3BtreeCursorIsValidNN(&pCrsr) == 0 {
                return Ok(Fl::Next);
            }
            let mut nStep = p1;
            let mut r = UnpackedRecord::default();
            r.pKeyInfo = csr!(n1p1).pKeyInfo.clone();
            r.nField = n1p4 as u16;
            r.default_rc = 0;
            r.aMem = reg_copies(&p.aMem, n1p3, n1p4);
            let mut res = 0i32;
            loop {
                chk!(sqlite3VdbeIdxKeyCompare(db, csr!(n1p1), &mut r, &mut res));
                let mut fail = false;
                if res > 0 && p5 == 0 {
                    fail = true;
                } else if res >= 0 {
                    // Jump to This.P2, bypassing the OP_SeekGE opcode
                    jump_to_p2!();
                } else if nStep <= 0 {
                    break;
                } else {
                    nStep -= 1;
                    csr!(n1p1).cacheStatus = CACHE_STALE as u32;
                    let rc = sqlite3BtreeNext(&pCrsr, 0);
                    if rc != 0 {
                        if rc == SQLITE_DONE {
                            fail = true;
                        } else {
                            abort!(rc);
                        }
                    }
                }
                if fail {
                    // seekscan_search_fail: Jump to SeekGE.P2, ending the
                    // loop
                    let np2 = p.aOp[(pc + 1) as usize].p2;
                    s.pc = np2 - 1;
                    return Ok(Fl::Next);
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: SeekHit P1 P2 P3 * *
        //
        // Increase or decrease the seekHit value for cursor P1, if necessary,
        // so that it is no less than P2 and no greater than P3.
        OP_SeekHit => {
            let pC = csr!(p1);
            if (pC.seekHit as i32) < p2 {
                pC.seekHit = p2 as u16;
            } else if pC.seekHit as i32 > p3 {
                pC.seekHit = p3 as u16;
            }
            Ok(Fl::Next)
        }

        // Opcode: IfNoHope P1 P2 P3 P4 *
        // Opcode: NoConflict P1 P2 P3 P4 *
        // Opcode: NotFound P1 P2 P3 P4 *
        // Opcode: Found P1 P2 P3 P4 *
        OP_IfNoHope | OP_NoConflict | OP_NotFound | OP_Found => {
            let p4i = p4_i!();
            if opcode == OP_IfNoHope && csr!(p1).seekHit as i32 >= p4i {
                return Ok(Fl::Next);
            }
            let pCrsr = btcsr!(csr!(p1));
            let pKeyInfo = csr!(p1).pKeyInfo.clone();
            let nField = p4i as u16;
            let rc;
            let mut seekResult = 0i32;
            if nField > 0 {
                // Key values in an array of registers
                let mut r = UnpackedRecord::default();
                r.aMem = reg_copies(&p.aMem, p3, nField as i32);
                r.nField = nField;
                r.pKeyInfo = pKeyInfo;
                r.default_rc = 0;
                rc = sqlite3BtreeIndexMoveto(db, &pCrsr, &mut r, &mut seekResult);
            } else {
                // Composite key generated by OP_MakeRecord
                if ExpandBlob(db, &mut mem!(p3)) != 0 {
                    return Err(Ex::NoMem);
                }
                let ki = pKeyInfo.expect("index cursor without KeyInfo");
                let mut pIdxKey = match sqlite3VdbeAllocUnpackedRecord(db, &ki) {
                    Some(r) => r,
                    None => return Err(Ex::NoMem),
                };
                let key = mem!(p3).bytes().to_vec();
                sqlite3VdbeRecordUnpack(key.len() as i32, &key, &mut pIdxKey);
                pIdxKey.default_rc = 0;
                rc = sqlite3BtreeIndexMoveto(db, &pCrsr, &mut pIdxKey, &mut seekResult);
            }
            csr!(p1).seekResult = seekResult;
            chk!(rc);
            let alreadyExists = seekResult == 0;
            {
                let pC = csr!(p1);
                pC.nullRow = 1 - alreadyExists as u8;
                pC.deferredMoveto = 0;
                pC.cacheStatus = CACHE_STALE as u32;
            }
            if opcode == OP_Found {
                if alreadyExists {
                    jump_to_p2!();
                }
            } else {
                if !alreadyExists {
                    jump_to_p2!();
                }
                if opcode == OP_NoConflict {
                    // For the OP_NoConflict opcode, take the jump if any of
                    // the input fields are NULL, since any key with a NULL
                    // will not conflict
                    for ii in 0..nField as i32 {
                        if (mem!(p3 + ii).flags & MEM_Null) != 0 {
                            jump_to_p2!();
                        }
                    }
                }
                if opcode == OP_IfNoHope {
                    csr!(p1).seekHit = p4i as u16;
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: SeekRowid P1 P2 P3 * *
        // Opcode: NotExists P1 P2 P3 * *
        OP_SeekRowid | OP_NotExists => {
            let iKey: i64;
            let pIn3 = &mem!(p3);
            if opcode == OP_SeekRowid && (pIn3.flags & (MEM_Int | MEM_IntReal)) == 0 {
                // If pIn3->u.i does not contain an integer, compute iKey as
                // the integer value of pIn3.  Jump to P2 if pIn3 cannot be
                // converted into an integer without loss of information.
                let mut x = pIn3.clone();
                if (x.flags & MEM_Str) != 0 {
                    applyNumericAffinity(db, &mut x, 1);
                }
                if (x.flags & MEM_Int) != 0 {
                    iKey = x.u.i;
                } else {
                    if (x.flags & MEM_Real) == 0
                        || x.u.r < -9223372036854775808.0
                        || x.u.r > 9223372036854774784.0
                    {
                        jump_to_p2!();
                    }
                    let k = sqlite3RealToI64(x.u.r);
                    if k as f64 != x.u.r {
                        jump_to_p2!();
                    }
                    iKey = k;
                }
            } else {
                iKey = pIn3.u.i;
            }
            // notExistsWithKey:
            let pCrsr = btcsr!(csr!(p1));
            let mut res = 0i32;
            let mut rc = sqlite3BtreeTableMoveto(db, &pCrsr, iKey, 0, &mut res);
            {
                let pC = csr!(p1);
                pC.movetoTarget = iKey; // Used by OP_Delete
                pC.nullRow = 0;
                pC.cacheStatus = CACHE_STALE as u32;
                pC.deferredMoveto = 0;
                pC.seekResult = res;
            }
            if res != 0 {
                if p2 == 0 {
                    rc = corrupt_bkpt();
                } else {
                    jump_to_p2!();
                }
            }
            chk!(rc);
            Ok(Fl::Next)
        }

        // Opcode: Sequence P1 P2 * * *
        //
        // Find the next available sequence number for cursor P1.  Write the
        // sequence number into register P2.
        OP_Sequence => {
            let pC = csr!(p1);
            let v = pC.seqCount;
            pC.seqCount += 1;
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = v;
            Ok(Fl::Next)
        }

        // Opcode: NewRowid P1 P2 P3 * *
        //
        // Get a new integer record number (a.k.a "rowid") used as the key to
        // a table.
        OP_NewRowid => {
            const MAX_ROWID: i64 = i64::MAX;
            let mut v: i64 = 0;
            let mut res: i32 = 0;
            out2Prerelease(&mut mem!(p2));
            let pCrsr = btcsr!(csr!(p1));
            if csr!(p1).useRandomRowid == 0 {
                chk!(sqlite3BtreeLast(db, &pCrsr, &mut res));
                if res != 0 {
                    v = 1; // IMP: R-61914-48074
                } else {
                    v = sqlite3BtreeIntegerKey(&pCrsr);
                    if v >= MAX_ROWID {
                        csr!(p1).useRandomRowid = 1;
                    } else {
                        v += 1; // IMP: R-29538-34987
                    }
                }
            }
            if p3 != 0 {
                let useRandom = csr!(p1).useRandomRowid != 0;
                let pMem: &mut Mem = if p.pFrame.is_some() {
                    &mut root_frame(p).unwrap().aMem[p3 as usize]
                } else {
                    &mut p.aMem[p3 as usize]
                };
                sqlite3VdbeMemIntegerify(db, pMem);
                debug_assert!((pMem.flags & MEM_Int) != 0);
                if pMem.u.i == MAX_ROWID || useRandom {
                    abort!(SQLITE_FULL); // IMP: R-17817-00630
                }
                if v < pMem.u.i + 1 {
                    v = pMem.u.i + 1;
                }
                pMem.u.i = v;
            }
            if csr!(p1).useRandomRowid != 0 {
                // IMPLEMENTATION-OF: R-07677-41881 If the largest ROWID is
                // equal to the largest possible integer (9223372036854775807)
                // then the database engine starts picking positive candidate
                // ROWIDs at random until it finds one that is not previously
                // used.
                debug_assert!(p3 == 0);
                let mut cnt = 0;
                let mut rc;
                loop {
                    let mut buf = [0u8; 8];
                    crate::random::sqlite3_randomness(8, &mut buf);
                    v = i64::from_ne_bytes(buf);
                    v &= MAX_ROWID >> 1;
                    v += 1; // Ensure that v is greater than zero
                    rc = sqlite3BtreeTableMoveto(db, &pCrsr, v, 0, &mut res);
                    if !(rc == SQLITE_OK && res == 0) {
                        break;
                    }
                    cnt += 1;
                    if cnt >= 100 {
                        break;
                    }
                }
                chk!(rc);
                if res == 0 {
                    abort!(SQLITE_FULL); // IMP: R-38219-53002
                }
                debug_assert!(v > 0);
            }
            {
                let pC = csr!(p1);
                pC.deferredMoveto = 0;
                pC.cacheStatus = CACHE_STALE as u32;
            }
            mem!(p2).u.i = v;
            Ok(Fl::Next)
        }

        // Opcode: Insert P1 P2 P3 P4 P5
        //
        // Write an entry into the table of cursor P1.  The key is the integer
        // in register P3, the data is the content of register P2.
        OP_Insert => {
            let nKey = mem!(p3).u.i;
            let iDb = csr!(p1).iDb;
            let hook = if p4type == P4_TABLE && db.xUpdateCallback.is_some() {
                let zDb = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();
                let zTab = match &p4!() {
                    P4::Tab(t) => t.borrow().zName.clone(),
                    _ => Vec::new(),
                };
                Some((zDb, zTab))
            } else {
                None
            };
            if (p5 & OPFLAG_NCHANGE) != 0 {
                p.nChange += 1;
                if (p5 & OPFLAG_LASTROWID) != 0 {
                    db.lastRowid = nKey;
                }
            }
            let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
            let seekResult = if (p5 & OPFLAG_USESEEKRESULT) != 0 {
                pC.seekResult
            } else {
                0
            };
            let pCrsr = btcsr!(pC);
            let pData = &p.aMem[p2 as usize];
            let x = BtreePayload {
                pKey: None,
                nKey,
                pData: Some(pData.bytes()),
                aMem: None,
                nMem: 0,
                nData: pData.n,
                nZero: if (pData.flags & MEM_Zero) != 0 {
                    pData.u.nZero
                } else {
                    0
                },
            };
            let rc = sqlite3BtreeInsert(
                &pCrsr,
                &x,
                (p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT)) as i32,
                seekResult,
            );
            pC.deferredMoveto = 0;
            pC.cacheStatus = CACHE_STALE as u32;
            s.colCacheCtr = s.colCacheCtr.wrapping_add(1);

            // Invoke the update-hook if required.
            chk!(rc);
            if let Some((zDb, zTab)) = hook {
                if let Some(cb) = db.xUpdateCallback.clone() {
                    let op = if (p5 & OPFLAG_ISUPDATE) != 0 {
                        SQLITE_UPDATE
                    } else {
                        SQLITE_INSERT
                    };
                    cb(op, &zDb, &zTab, nKey);
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: RowCell P1 P2 P3 * *
        //
        // P1 and P2 are both open cursors.  Both must be opened on the same
        // type of table - intkey or index.  This opcode is used as part of
        // copying the current row from P2 into P1.
        OP_RowCell => {
            let pDest = btcsr!(csr!(p1));
            let pSrc = btcsr!(csr!(p2));
            let iKey = if p3 != 0 { mem!(p3).u.i } else { 0 };
            chk!(sqlite3BtreeTransferRow(&pDest, &pSrc, iKey));
            Ok(Fl::Next)
        }

        // Opcode: Delete P1 P2 P3 P4 P5
        //
        // Delete the record at which the P1 cursor is currently pointing.
        OP_Delete => {
            let opflags = p2 as u16;
            let iDb = csr!(p1).iDb;
            let pCrsr = btcsr!(csr!(p1));
            // If the update-hook will be invoked, set zDb to the name of the
            // db to pass as to it.  Also set local pTab to a copy of
            // p4.pTab.
            let hook = if p4type == P4_TABLE && db.xUpdateCallback.is_some() {
                let zDb = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();
                let pTab = match &p4!() {
                    P4::Tab(t) => Some(t.clone()),
                    _ => None,
                };
                let pC = csr!(p1);
                if (p5 & OPFLAG_SAVEPOSITION) != 0 && pC.isTable != 0 {
                    pC.movetoTarget = sqlite3BtreeIntegerKey(&pCrsr);
                }
                Some((zDb, pTab))
            } else {
                None
            };
            let rc = sqlite3BtreeDelete(&pCrsr, p5 as u8);
            {
                let pC = csr!(p1);
                pC.cacheStatus = CACHE_STALE as u32;
                pC.seekResult = 0;
            }
            s.colCacheCtr = s.colCacheCtr.wrapping_add(1);
            chk!(rc);

            // Invoke the update-hook if required.
            if (opflags & OPFLAG_NCHANGE) != 0 {
                p.nChange += 1;
                if let (Some(cb), Some((zDb, Some(pTab)))) = (db.xUpdateCallback.clone(), hook) {
                    let (hasRowid, zName) = {
                        let t = pTab.borrow();
                        (t.HasRowid(), t.zName.clone())
                    };
                    if hasRowid {
                        let target = csr!(p1).movetoTarget;
                        cb(SQLITE_DELETE, &zDb, &zName, target);
                    }
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: ResetCount * * * * *
        //
        // The value of the change counter is copied to the database handle
        // change counter (returned by subsequent calls to
        // sqlite3_changes()).  Then the VMs internal change counter resets
        // to 0.
        OP_ResetCount => {
            sqlite3VdbeSetChanges(db, p.nChange);
            p.nChange = 0;
            Ok(Fl::Next)
        }

        // Opcode: SorterCompare P1 P2 P3 P4
        //
        // P1 is a sorter cursor.  This instruction compares a prefix of the
        // record blob in register P3 against a prefix of the entry that the
        // sorter cursor currently points to.
        OP_SorterCompare => {
            let nKeyCol = p4_i!();
            let mut res = 0;
            let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
            let rc = crate::vdbesort::sqlite3VdbeSorterCompare(
                db,
                pC,
                &mut p.aMem[p3 as usize],
                nKeyCol,
                &mut res,
            );
            chk!(rc);
            if res != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: SorterData P1 P2 P3 * *
        //
        // Write into register P2 the current sorter data for sorter cursor
        // P1.  Then clear the column header cache on cursor P3.
        OP_SorterData => {
            let pC = p.apCsr[p1 as usize].as_deref().expect("cursor");
            let rc = crate::vdbesort::sqlite3VdbeSorterRowkey(pC, &mut p.aMem[p2 as usize]);
            chk!(rc);
            csr!(p3).cacheStatus = CACHE_STALE as u32;
            Ok(Fl::Next)
        }

        // Opcode: RowData P1 P2 P3 * *
        //
        // Write into register P2 the complete row content for the row at
        // which cursor P1 is currently pointing.
        OP_RowData => {
            out2Prerelease(&mut mem!(p2));
            let pCrsr = btcsr!(csr!(p1));
            let n = sqlite3BtreePayloadSize(&pCrsr);
            if n > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as u32 {
                return Err(Ex::TooBig);
            }
            chk!(sqlite3VdbeMemFromBtreeZeroOffset(&pCrsr, n, &mut mem!(p2)));
            if p3 == 0 && Deephemeralize(db, &mut mem!(p2)) {
                return Err(Ex::NoMem);
            }
            Ok(Fl::Next)
        }

        // Opcode: Rowid P1 P2 * * *
        //
        // Store in register P2 an integer which is the key of the table
        // entry that P1 is currently point to.
        OP_Rowid => {
            out2Prerelease(&mut mem!(p2));
            let v: i64;
            let pC = csr!(p1);
            if pC.nullRow != 0 {
                mem!(p2).flags = MEM_Null;
                return Ok(Fl::Next);
            } else if pC.deferredMoveto != 0 {
                v = pC.movetoTarget;
            } else if pC.eCurType == CURTYPE_VTAB {
                verr!(db, p, b"virtual tables are not supported");
                abort!(SQLITE_ERROR);
            } else {
                chk!(sqlite3VdbeCursorRestore(pC));
                if pC.nullRow != 0 {
                    mem!(p2).flags = MEM_Null;
                    return Ok(Fl::Next);
                }
                v = sqlite3BtreeIntegerKey(&btcsr!(pC));
            }
            mem!(p2).u.i = v;
            Ok(Fl::Next)
        }

        // Opcode: NullRow P1 P2 * * *
        //
        // Move the cursor P1 to a null row.  Any OP_Column operations that
        // occur while the cursor is on the null row will always write a
        // NULL.  If P2==0 also clear the btree cursor.
        OP_NullRow => {
            if p.apCsr[p1 as usize].is_none() {
                // If the cursor is not already open, create a special kind
                // of pseudo-cursor that always gives null rows.
                allocateCursor(db, p, p1, 1, CURTYPE_PSEUDO);
                let pC = csr!(p1);
                pC.seekResult = 0;
                pC.isTable = 1;
                pC.noReuse = 1;
                pC.pCursor = sqlite3BtreeFakeValidCursor();
            }
            let pC = csr!(p1);
            pC.nullRow = (p2 == 0) as u8;
            pC.cacheStatus = CACHE_STALE as u32;
            if pC.eCurType == CURTYPE_BTREE {
                sqlite3BtreeClearCursor(&btcsr!(pC));
            }
            Ok(Fl::Next)
        }

        // Opcode: SeekEnd P1 * * * *
        // Opcode: Last P1 P2 * * *
        OP_SeekEnd | OP_Last => {
            let pCrsr = btcsr!(csr!(p1));
            let mut res = 0;
            if opcode == OP_SeekEnd {
                csr!(p1).seekResult = -1;
                if sqlite3BtreeCursorIsValidNN(&pCrsr) != 0 {
                    return Ok(Fl::Next);
                }
            }
            let rc = sqlite3BtreeLast(db, &pCrsr, &mut res);
            {
                let pC = csr!(p1);
                pC.nullRow = res as u8;
                pC.deferredMoveto = 0;
                pC.cacheStatus = CACHE_STALE as u32;
            }
            chk!(rc);
            if p2 > 0 && res != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IfSizeBetween P1 P2 P3 P4 *
        //
        // Let N be the approximate number of rows in the table or index with
        // cursor P1 and let X be 10*log2(N) if N is positive or -1 if N is
        // zero.  Jump to P2 if X is in between P3 and P4, inclusive.
        OP_IfSizeBetween => {
            let pCrsr = btcsr!(csr!(p1));
            let mut res = 0;
            chk!(sqlite3BtreeFirst(db, &pCrsr, &mut res));
            let sz: i64 = if res != 0 {
                -1 // -Infinity encoding
            } else {
                let n = sqlite3BtreeRowCountEst(db, &pCrsr);
                crate::util::sqlite3LogEst(n as u64) as i64
            };
            if sz >= p3 as i64 && sz <= p4_i!() as i64 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: SorterSort P1 P2 * * *
        // Opcode: Sort P1 P2 * * *
        // Opcode: Rewind P1 P2 * * *
        OP_SorterSort | OP_Sort | OP_Rewind => {
            if opcode != OP_Rewind {
                p.aCounter[SQLITE_STMTSTATUS_SORT as usize] += 1;
            }
            let mut res = 1;
            let rc;
            let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
            if isSorter(pC) {
                rc = crate::vdbesort::sqlite3VdbeSorterRewind(db, pC, &mut res);
            } else {
                let pCrsr = btcsr!(pC);
                rc = sqlite3BtreeFirst(db, &pCrsr, &mut res);
                pC.deferredMoveto = 0;
                pC.cacheStatus = CACHE_STALE as u32;
            }
            chk!(rc);
            pC.nullRow = res as u8;
            if p2 > 0 && res != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: IfEmpty P1 P2 * * *
        //
        // Check to see if the b-tree table that cursor P1 references is
        // empty and jump to P2 if it is.
        OP_IfEmpty => {
            let pCrsr = btcsr!(csr!(p1));
            let mut res = 0;
            chk!(sqlite3BtreeIsEmpty(db, &pCrsr, &mut res));
            if res != 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: Next P1 P2 P3 * P5
        // Opcode: Prev P1 P2 P3 * P5
        // Opcode: SorterNext P1 P2 * * P5
        OP_SorterNext | OP_Prev | OP_Next => {
            let rc = if opcode == OP_SorterNext {
                let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
                crate::vdbesort::sqlite3VdbeSorterNext(db, pC)
            } else {
                let pCrsr = btcsr!(csr!(p1));
                if opcode == OP_Prev {
                    sqlite3BtreePrevious(&pCrsr, p3)
                } else {
                    sqlite3BtreeNext(&pCrsr, p3)
                }
            };
            // next_tail:
            let pC = csr!(p1);
            pC.cacheStatus = CACHE_STALE as u32;
            if rc == SQLITE_OK {
                pC.nullRow = 0;
                let c = &mut p.aCounter[p5 as usize];
                *c = c.wrapping_add(1);
                jump_to_p2_and_check_for_interrupt!();
            }
            if rc != SQLITE_DONE {
                abort!(rc);
            }
            pC.nullRow = 1;
            Ok(Fl::Check)
        }

        // Opcode: IdxInsert P1 P2 P3 P4 P5
        //
        // Register P2 holds an SQL index key made using the MakeRecord
        // instructions.  This opcode writes that key into the index P1.
        OP_IdxInsert => {
            if (p5 & OPFLAG_NCHANGE) != 0 {
                p.nChange += 1;
            }
            chk!(ExpandBlob(db, &mut mem!(p2)));
            let nMem = p4_i!();
            let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
            let pCrsr = btcsr!(pC);
            let seekResult = if (p5 & OPFLAG_USESEEKRESULT) != 0 {
                pC.seekResult
            } else {
                0
            };
            let pIn2 = &p.aMem[p2 as usize];
            let aEnd = ((p3.max(0) + nMem.max(0)) as usize).min(p.aMem.len());
            let aStart = (p3.max(0) as usize).min(aEnd);
            let x = BtreePayload {
                pKey: Some(pIn2.bytes()),
                nKey: pIn2.n as i64,
                pData: None,
                aMem: Some(&p.aMem[aStart..aEnd]),
                nMem: nMem as u16,
                nData: 0,
                nZero: 0,
            };
            let rc = sqlite3BtreeInsert(
                &pCrsr,
                &x,
                (p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT)) as i32,
                seekResult,
            );
            pC.cacheStatus = CACHE_STALE as u32;
            chk!(rc);
            Ok(Fl::Next)
        }

        // Opcode: SorterInsert P1 P2 * * *
        //
        // Register P2 holds an SQL index key made using the MakeRecord
        // instructions.  This opcode writes that key into the sorter P1.
        OP_SorterInsert => {
            chk!(ExpandBlob(db, &mut mem!(p2)));
            let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
            chk!(crate::vdbesort::sqlite3VdbeSorterWrite(
                pC,
                &mut p.aMem[p2 as usize]
            ));
            Ok(Fl::Next)
        }

        // Opcode: IdxDelete P1 P2 P3 P4 P5
        //
        // The content of P3 registers starting at register P2 form an
        // unpacked index key.  This opcode removes that entry from the index
        // opened by cursor P1.
        OP_IdxDelete => {
            let pCrsr = btcsr!(csr!(p1));
            let mut r = UnpackedRecord::default();
            r.pKeyInfo = csr!(p1).pKeyInfo.clone();
            r.nField = p5;
            r.default_rc = 0;
            r.aMem = reg_copies(&p.aMem, p2, p5 as i32);
            let mut res = 0;
            chk!(sqlite3BtreeIndexMoveto(db, &pCrsr, &mut r, &mut res));
            if res != 0 {
                let pIdx = match &p4!() {
                    P4::Idx(i) => i.clone(),
                    _ => panic!("OP_IdxDelete without index"),
                };
                chk!(sqlite3VdbeFindIndexKey(
                    db, &pCrsr, &pIdx, &mut r, &mut res, 0
                ));
                if res != 0 {
                    if crate::build::sqlite3WritableSchema(db) == 0 {
                        let rc = crate::mainapi::sqlite3ReportError(
                            SQLITE_CORRUPT_INDEX,
                            line!() as i32,
                            Some(b"index corruption"),
                        );
                        abort!(rc);
                    }
                    let pC = csr!(p1);
                    pC.cacheStatus = CACHE_STALE as u32;
                    pC.seekResult = 0;
                    return Ok(Fl::Next);
                }
            }
            if p3 != 0 {
                let mut rc = SQLITE_OK;
                if vdbeIndexKeyCompare(&pCrsr, &p.aMem[p3 as usize], &mut rc) {
                    chk!(rc);
                    sqlite3VdbeMemSetNull(&mut mem!(p3));
                    return Ok(Fl::Next);
                }
            }
            chk!(sqlite3BtreeDelete(&pCrsr, BTREE_AUXDELETE as u8));
            let pC = csr!(p1);
            pC.cacheStatus = CACHE_STALE as u32;
            pC.seekResult = 0;
            Ok(Fl::Next)
        }

        // Opcode: DeferredSeek P1 * P3 P4 *
        // Opcode: IdxRowid P1 P2 * * *
        OP_DeferredSeek | OP_IdxRowid => {
            // The IdxRowid and Seek opcodes are combined because of the
            // commonality of sqlite3VdbeCursorRestore() and
            // sqlite3VdbeIdxRowid().
            chk!(sqlite3VdbeCursorRestore(csr!(p1)));
            if csr!(p1).nullRow == 0 {
                let mut rowid: i64 = 0;
                let pCrsr = btcsr!(csr!(p1));
                chk!(sqlite3VdbeIdxRowid(db, &pCrsr, &mut rowid));
                if opcode == OP_DeferredSeek {
                    let aAltMap = match &p4!() {
                        P4::Ai(a) => Some(a.clone()),
                        _ => None,
                    };
                    let pTabCur = csr!(p3);
                    pTabCur.nullRow = 0;
                    pTabCur.movetoTarget = rowid;
                    pTabCur.deferredMoveto = 1;
                    pTabCur.cacheStatus = CACHE_STALE as u32;
                    pTabCur.aAltMap = aAltMap;
                    pTabCur.pAltCursor = p1;
                } else {
                    let pOut = out2Prerelease(&mut mem!(p2));
                    pOut.u.i = rowid;
                }
            } else {
                sqlite3VdbeMemSetNull(&mut mem!(p2));
            }
            Ok(Fl::Next)
        }

        // Opcode: FinishSeek P1 * * * *
        //
        // If cursor P1 was previously moved via OP_DeferredSeek, complete
        // that seek operation now, without further delay.
        OP_FinishSeek => {
            let pC = csr!(p1);
            if pC.deferredMoveto != 0 {
                chk!(sqlite3VdbeFinishMoveto(db, pC));
            }
            Ok(Fl::Next)
        }

        // Opcode: IdxGE P1 P2 P3 P4 * (and IdxGT, IdxLT, IdxLE)
        OP_IdxLE | OP_IdxGT | OP_IdxLT | OP_IdxGE => {
            let mut r = UnpackedRecord::default();
            let nf = p4_i!();
            r.pKeyInfo = csr!(p1).pKeyInfo.clone();
            r.nField = nf as u16;
            r.default_rc = if opcode < OP_IdxLT { -1 } else { 0 };
            r.aMem = reg_copies(&p.aMem, p3, nf);
            let mut res;
            // Inlined version of sqlite3VdbeIdxKeyCompare()
            {
                let pCur = btcsr!(csr!(p1));
                let nCellKey = sqlite3BtreePayloadSize(&pCur) as i64;
                // nCellKey will always be between 0 and 0xffffffff because
                // of the way that btreeParseCellPtr() and
                // sqlite3GetVarint32() are implemented
                if nCellKey <= 0 || nCellKey > 0x7fffffff {
                    abort!(corrupt_bkpt());
                }
                let mut m = Mem::default();
                sqlite3VdbeMemInit(db, &mut m, 0);
                chk!(sqlite3VdbeMemFromBtreeZeroOffset(
                    &pCur,
                    nCellKey as u32,
                    &mut m
                ));
                res = sqlite3VdbeRecordCompareWithSkip(db, m.n, m.bytes(), &mut r, 0);
                sqlite3VdbeMemReleaseMalloc(&mut m);
            }
            // End of inlined sqlite3VdbeIdxKeyCompare()
            if (opcode & 1) == (OP_IdxLT & 1) {
                res = -res;
            } else {
                res += 1;
            }
            if res > 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: Destroy P1 P2 P3 * *
        //
        // Delete an entire database table or index whose root page in the
        // database file is given by P1.
        OP_Destroy => {
            out2Prerelease(&mut mem!(p2)).flags = MEM_Null;
            if db.nVdbeRead > db.nVDestroy + 1 {
                p.errorAction = OE_Abort;
                abort!(SQLITE_LOCKED);
            } else {
                let iDb = p3;
                let mut iMoved = 0;
                let rc = sqlite3BtreeDropTable(&pbt!(iDb), p1, &mut iMoved);
                let pOut = &mut mem!(p2);
                pOut.flags = MEM_Int;
                pOut.u.i = iMoved as i64;
                chk!(rc);
                if iMoved != 0 {
                    crate::build::sqlite3RootPageMoved(db, iDb, iMoved as Pgno, p1 as Pgno);
                    // All OP_Destroy operations occur on the same btree
                    debug_assert!(
                        s.resetSchemaOnFault == 0 || s.resetSchemaOnFault as i32 == iDb + 1
                    );
                    s.resetSchemaOnFault = (iDb + 1) as u8;
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: Clear P1 P2 P3
        //
        // Delete all contents of the database table or index whose root page
        // in the database file is given by P1.
        OP_Clear => {
            let mut nChange: i64 = 0;
            let rc = sqlite3BtreeClearTable(&pbt!(p2), p1, &mut nChange);
            if p3 != 0 {
                p.nChange += nChange;
                if p3 > 0 {
                    let m = &mut mem!(p3);
                    m.u.i = m.u.i.wrapping_add(nChange);
                }
            }
            chk!(rc);
            Ok(Fl::Next)
        }

        // Opcode: ResetSorter P1 * * * *
        //
        // Delete all contents from the ephemeral table or sorter that is
        // open on cursor P1.
        OP_ResetSorter => {
            let pC = csr!(p1);
            if isSorter(pC) {
                if let Some(srt) = pC.pSorter.as_deref_mut() {
                    crate::vdbesort::sqlite3VdbeSorterReset(db, srt);
                }
            } else {
                chk!(sqlite3BtreeClearTableOfCursor(&btcsr!(pC)));
            }
            Ok(Fl::Next)
        }

        // Opcode: CreateBtree P1 P2 P3 * *
        //
        // Allocate a new b-tree in the main database file if P1==0 or in the
        // TEMP database file if P1==1 or in an attached database if P1>1.
        OP_CreateBtree => {
            out2Prerelease(&mut mem!(p2));
            let mut pgno: Pgno = 0;
            chk!(sqlite3BtreeCreateTable(&pbt!(p1), &mut pgno, p3));
            mem!(p2).u.i = pgno as i64;
            Ok(Fl::Next)
        }

        // Opcode: SqlExec P1 P2 * P4 *
        //
        // Run the SQL statement or statements specified in the P4 string.
        OP_SqlExec => {
            db.nSqlExec += 1;
            let mut zErr: Option<ZStr> = None;
            let xAuth = db.xAuth.clone();
            let mTrace = db.mTrace;
            let savedAnalysisLimit = db.nAnalysisLimit;
            if (p1 & 0x0001) != 0 {
                db.xAuth = None;
                db.mTrace = 0;
            }
            if (p1 & 0x0002) != 0 {
                db.nAnalysisLimit = p2;
            }
            let zSql = p4_z!();
            let rc = crate::legacy::sqlite3_exec(db, zSql.as_deref(), None, &mut zErr);
            db.nSqlExec -= 1;
            db.xAuth = xAuth;
            db.mTrace = mTrace;
            db.nAnalysisLimit = savedAnalysisLimit;
            if zErr.is_some() || rc != 0 {
                verr!(db, p, b"%s", zErr.as_deref());
                if rc == SQLITE_NOMEM {
                    return Err(Ex::NoMem);
                }
                abort!(rc);
            }
            Ok(Fl::Next)
        }

        // Opcode: ParseSchema P1 * * P4 P5
        //
        // Read and parse all entries from the schema table of database P1
        // that match the WHERE clause P4.  If P4 is a NULL pointer, then the
        // entire schema for P1 is reparsed.
        OP_ParseSchema => op_parseschema(db, p, s, pc, p1, p5),

        // Opcode: LoadAnalysis P1 * * * *
        //
        // Read the sqlite_stat1 table for database P1 and load the content
        // of that table into the internal index hash table.
        OP_LoadAnalysis => {
            chk!(crate::analyze::sqlite3AnalysisLoad(db, p1));
            Ok(Fl::Next)
        }

        // Opcode: DropTable P1 * * P4 *
        //
        // Remove the internal (in-memory) data structures that describe the
        // table named P4 in database P1.
        OP_DropTable => {
            let z = p4_z!();
            crate::build::sqlite3UnlinkAndDeleteTable(db, p1, z.as_deref());
            Ok(Fl::Next)
        }

        // Opcode: DropIndex P1 * * P4 *
        OP_DropIndex => {
            let z = p4_z!();
            crate::build::sqlite3UnlinkAndDeleteIndex(db, p1, z.as_deref());
            Ok(Fl::Next)
        }

        // Opcode: DropTrigger P1 * * P4 *
        OP_DropTrigger => {
            let z = p4_z!();
            crate::trigger::sqlite3UnlinkAndDeleteTrigger(db, p1, z.as_deref());
            Ok(Fl::Next)
        }

        // Opcode: IntegrityCk P1 P2 P3 P4 P5
        //
        // Do an analysis of the currently open database.  Store in register
        // (P1+1) the text of an error message describing any problems.
        OP_IntegrityCk => {
            let nRoot = p2;
            let aRoot = match &p4!() {
                P4::Ai(a) => a.clone(),
                _ => Rc::new(vec![0]),
            };
            let mxErr = (mem!(p1).u.i + 1) as i32;
            let mut nErr = 0;
            let mut z: Option<ZStr> = None;
            let pBt = pbt!(p5);
            let e = (aRoot.len()).min(1 + nRoot.max(0) as usize);
            let rc = sqlite3BtreeIntegrityCheck(
                db,
                &pBt,
                &aRoot[1.min(e)..e],
                &mut p.aMem[p3 as usize..],
                nRoot,
                mxErr,
                &mut nErr,
                &mut z,
            );
            sqlite3VdbeMemSetNull(&mut mem!(p1 + 1));
            if nErr == 0 {
                debug_assert!(z.is_none());
            } else if rc != 0 {
                abort!(rc);
            } else {
                mem!(p1).u.i -= (nErr - 1) as i64;
                let zz = z.unwrap_or_default();
                sqlite3VdbeMemSetStr(
                    db,
                    &mut p.aMem[(p1 + 1) as usize],
                    Some(&zz),
                    zz.len() as i64,
                    SQLITE_UTF8,
                );
            }
            sqlite3VdbeChangeEncoding(db, &mut p.aMem[(p1 + 1) as usize], encoding as i32);
            Ok(Fl::Check)
        }

        // Opcode: IFindKey P1 P2 P3 P4 *
        //
        // This instruction always follows an OP_Found with the same P1, P2
        // and P3 values as this instruction and a non-zero P4 value.
        OP_IFindKey => {
            let pIdx = match &p4!() {
                P4::Idx(i) => i.clone(),
                _ => panic!("OP_IFindKey without index"),
            };
            let mut r = UnpackedRecord::default();
            let nColumn = pIdx.borrow().nColumn;
            r.aMem = reg_copies(&p.aMem, p3, nColumn as i32);
            r.nField = nColumn;
            r.pKeyInfo = csr!(p1).pKeyInfo.clone();
            let pCrsr = btcsr!(csr!(p1));
            let mut res = 0;
            let rc = sqlite3VdbeFindIndexKey(db, &pCrsr, &pIdx, &mut r, &mut res, 1);
            if rc != 0 || res != 0 {
                jump_to_p2!();
            }
            csr!(p1).nullRow = 0;
            Ok(Fl::Next)
        }

        // Opcode: RowSetAdd P1 P2 * * *
        //
        // Insert the integer value held by register P2 into a RowSet object
        // held in register P1.
        OP_RowSetAdd => {
            let v = mem!(p2).u.i;
            if (mem!(p1).flags & MEM_Blob) == 0 {
                if sqlite3VdbeMemSetRowSet(db, &mut mem!(p1)) != 0 {
                    return Err(Ex::NoMem);
                }
            }
            if let Some(rs) = mem_rowset(&mem!(p1)) {
                crate::rowset::sqlite3RowSetInsert(&mut rs.borrow_mut(), v);
            }
            Ok(Fl::Next)
        }

        // Opcode: RowSetRead P1 P2 P3 * *
        //
        // Extract the smallest value from the RowSet object in P1 and put
        // that value into register P3.  Or, if RowSet object P1 is initially
        // empty, leave P3 unchanged and jump to instruction P2.
        OP_RowSetRead => {
            let mut val: i64 = 0;
            let got = if (mem!(p1).flags & MEM_Blob) == 0 {
                false
            } else {
                match mem_rowset(&mem!(p1)) {
                    Some(rs) => {
                        crate::rowset::sqlite3RowSetNext(&mut rs.borrow_mut(), &mut val) != 0
                    }
                    None => false,
                }
            };
            if !got {
                // The boolean index is empty
                sqlite3VdbeMemSetNull(&mut mem!(p1));
                jump_to_p2_and_check_for_interrupt!();
            } else {
                // A value was pulled from the index
                sqlite3VdbeMemSetInt64(&mut mem!(p3), val);
            }
            Ok(Fl::Check)
        }

        // Opcode: RowSetTest P1 P2 P3 P4
        //
        // Register P3 is assumed to hold a 64-bit integer value.  If
        // register P1 contains a RowSet object and that RowSet object
        // contains the value held in P3, jump to register P2.  Otherwise,
        // insert the integer in P3 into the RowSet and continue on to the
        // next opcode.
        OP_RowSetTest => {
            let iSet = p4_i!();
            let v = mem!(p3).u.i;
            // If there is anything other than a rowset object in memory cell
            // P1, delete it now and initialize P1 with an empty rowset
            if (mem!(p1).flags & MEM_Blob) == 0 {
                if sqlite3VdbeMemSetRowSet(db, &mut mem!(p1)) != 0 {
                    return Err(Ex::NoMem);
                }
            }
            if let Some(rs) = mem_rowset(&mem!(p1)) {
                if iSet != 0 {
                    let exists = crate::rowset::sqlite3RowSetTest(&mut rs.borrow_mut(), iSet, v);
                    if exists != 0 {
                        jump_to_p2!();
                    }
                }
                if iSet >= 0 {
                    crate::rowset::sqlite3RowSetInsert(&mut rs.borrow_mut(), v);
                }
            }
            Ok(Fl::Next)
        }

        // Opcode: Program P1 P2 P3 P4 P5
        //
        // Execute the trigger program passed as P4 (type P4_SUBPROGRAM).
        OP_Program => op_program(db, p, s, pc, p1, p2, p3, p5),

        // Opcode: Param P1 P2 * * *
        //
        // This opcode is only ever present in sub-programs called via the
        // OP_Program instruction.  Copy a value currently stored in a memory
        // cell of the calling (parent) frame to cell P2 in the current
        // frames address space.
        OP_Param => {
            out2Prerelease(&mut mem!(p2));
            let pFrame = p
                .pFrame
                .as_deref()
                .expect("OP_Param outside of a sub-program");
            let idx = p1 + pFrame.aOp[pFrame.pc as usize].p1;
            let pIn = &pFrame.aMem[idx as usize];
            sqlite3VdbeMemShallowCopy(db, &mut p.aMem[p2 as usize], pIn, MEM_Ephem as i32);
            Ok(Fl::Next)
        }

        // Opcode: FkCounter P1 P2 * * *
        //
        // Increment a "constraint counter" by P2 (P2 may be negative or
        // positive).  If P1 is non-zero, the database constraint counter is
        // incremented (deferred foreign key constraints).  Otherwise, if P1
        // is zero, the statement counter is incremented (immediate foreign
        // key constraints).
        OP_FkCounter => {
            if p1 != 0 {
                db.nDeferredCons += p2 as i64;
            } else if (db.flags & SQLITE_DeferFKs) != 0 {
                db.nDeferredImmCons += p2 as i64;
            } else {
                p.nFkConstraint += p2 as i64;
            }
            Ok(Fl::Next)
        }

        // Opcode: FkIfZero P1 P2 * * *
        //
        // This opcode tests if a foreign key constraint-counter is currently
        // zero.  If so, jump to instruction P2.
        OP_FkIfZero => {
            if p1 != 0 {
                if db.nDeferredCons == 0 && db.nDeferredImmCons == 0 {
                    jump_to_p2!();
                }
            } else if p.nFkConstraint == 0 && db.nDeferredImmCons == 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: MemMax P1 P2 * * *
        //
        // P1 is a register in the root frame of this VM (the root frame is
        // different from the current frame if this instruction is being
        // executed within a sub-program).  Set the value of register P1 to
        // the maximum of its current value and the value in register P2.
        OP_MemMax => {
            sqlite3VdbeMemIntegerify(db, &mut mem!(p2));
            let v2 = mem!(p2).u.i;
            let pIn1: &mut Mem = if p.pFrame.is_some() {
                &mut root_frame(p).unwrap().aMem[p1 as usize]
            } else {
                &mut p.aMem[p1 as usize]
            };
            sqlite3VdbeMemIntegerify(db, pIn1);
            if pIn1.u.i < v2 {
                pIn1.u.i = v2;
            }
            Ok(Fl::Next)
        }

        // Opcode: IfPos P1 P2 P3 * *
        //
        // Register P1 must contain an integer.  If the value of register P1
        // is 1 or greater, subtract P3 from the value in P1 and jump to P2.
        OP_IfPos => {
            let pIn1 = &mut mem!(p1);
            if pIn1.u.i > 0 {
                pIn1.u.i -= p3 as i64;
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: OffsetLimit P1 P2 P3 * *
        //
        // This opcode performs a commonly used computation associated with
        // LIMIT and OFFSET processing.
        OP_OffsetLimit => {
            let mut x = mem!(p1).u.i;
            let i3 = mem!(p3).u.i;
            let pOut = out2Prerelease(&mut mem!(p2));
            if x <= 0 || crate::util::sqlite3AddInt64(&mut x, if i3 > 0 { i3 } else { 0 }) != 0 {
                // If the LIMIT is less than or equal to zero, loop forever.
                // This is documented.  But also, if the LIMIT+OFFSET exceeds
                // 2^63 then also loop forever.  This is undocumented.
                pOut.u.i = -1;
            } else {
                pOut.u.i = x;
            }
            Ok(Fl::Next)
        }

        // Opcode: IfNotZero P1 P2 * * *
        //
        // Register P1 must contain an integer.  If the content of register
        // P1 is initially greater than zero, then decrement the value in
        // register P1.  If it is non-zero (negative or positive) and then
        // also jump to P2.  If register P1 is initially zero, leave it
        // unchanged and fall through.
        OP_IfNotZero => {
            let pIn1 = &mut mem!(p1);
            if pIn1.u.i != 0 {
                if pIn1.u.i > 0 {
                    pIn1.u.i -= 1;
                }
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: DecrJumpZero P1 P2 * * *
        //
        // Register P1 must hold an integer.  Decrement the value in P1 and
        // jump to P2 if the new value is exactly zero.
        OP_DecrJumpZero => {
            let pIn1 = &mut mem!(p1);
            if pIn1.u.i > SMALLEST_INT64 {
                pIn1.u.i -= 1;
            }
            if pIn1.u.i == 0 {
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: AggStep * P2 P3 P4 P5
        // Opcode: AggInverse * P2 P3 P4 P5
        // Opcode: AggStep1 P1 P2 P3 P4 P5
        OP_AggInverse | OP_AggStep | OP_AggStep1 => {
            op_aggstep(db, p, s, pc, opcode, p1, p2, p3, p5)
        }

        // Opcode: AggFinal P1 P2 * P4 *
        // Opcode: AggValue * P2 P3 P4 *
        //
        // P1 is the memory location that is the accumulator for an aggregate
        // or window function.  Execute the finalizer function for an
        // aggregate and store the result in P1 (or P3 for AggValue).
        OP_AggValue | OP_AggFinal => {
            let pFunc = match &p4!() {
                P4::Func(f) => f.clone(),
                P4::FuncCtx(f, _) => f.clone(),
                _ => panic!("OP_AggFinal without function"),
            };
            let rc;
            let iMem;
            if p3 != 0 {
                let (acc, out) = two_mut(&mut p.aMem, p1 as usize, p3 as usize);
                rc = sqlite3VdbeMemAggValue(db, acc, out, &pFunc);
                iMem = p3;
            } else {
                rc = sqlite3VdbeMemFinalize(db, &mut mem!(p1), &pFunc);
                iMem = p1;
            }
            if rc != 0 {
                let z = sqlite3_value_text(&mut mem!(iMem)).map(|x| x.to_vec());
                verr!(db, p, b"%s", z.as_deref());
                abort!(rc);
            }
            sqlite3VdbeChangeEncoding(db, &mut mem!(iMem), encoding as i32);
            Ok(Fl::Next)
        }

        // Opcode: Checkpoint P1 P2 P3 * *
        //
        // Checkpoint database P1.  This is a no-op if P1 is not currently in
        // WAL mode.
        OP_Checkpoint => {
            let mut aRes: [i32; 3] = [0, -1, -1];
            let (mut a1, mut a2) = (-1, -1);
            let rc = crate::mainapi::sqlite3Checkpoint(db, p1, p2, Some(&mut a1), Some(&mut a2));
            aRes[1] = a1;
            aRes[2] = a2;
            if rc != 0 {
                if rc != SQLITE_BUSY {
                    abort!(rc);
                }
                aRes[0] = 1;
            }
            for i in 0..3 {
                sqlite3VdbeMemSetInt64(&mut mem!(p3 + i), aRes[i as usize] as i64);
            }
            Ok(Fl::Next)
        }

        // Opcode: JournalMode P1 P2 P3 * *
        //
        // Change the journal mode of database P1 to P3.  Write a string
        // containing the final journal-mode to register P2.
        OP_JournalMode => op_journalmode(db, p, s, pc, p1, p2, p3),

        // Opcode: Vacuum P1 P2 * * *
        //
        // Vacuum the entire database P1.
        OP_Vacuum => {
            let pOut = if p2 != 0 {
                Some(&mut p.aMem[p2 as usize])
            } else {
                None
            };
            chk!(crate::vacuum::sqlite3RunVacuum(
                db,
                &mut p.zErrMsg,
                p1,
                pOut
            ));
            Ok(Fl::Next)
        }

        // Opcode: IncrVacuum P1 P2 * * *
        //
        // Perform a single step of the incremental vacuum procedure on the
        // P1 database.  If the vacuum has finished, jump to instruction P2.
        OP_IncrVacuum => {
            let rc = sqlite3BtreeIncrVacuum(&pbt!(p1));
            if rc != 0 {
                if rc != SQLITE_DONE {
                    abort!(rc);
                }
                jump_to_p2!();
            }
            Ok(Fl::Next)
        }

        // Opcode: Expire P1 P2 * * *
        //
        // Cause precompiled statements to expire.
        OP_Expire => {
            if p1 == 0 {
                sqlite3ExpirePreparedStatements(db, p2);
            } else {
                p.expired = (p2 + 1) as u8;
            }
            Ok(Fl::Next)
        }

        // Opcode: CursorLock P1 * * * *
        //
        // Lock the btree to which cursor P1 is pointing so that the btree
        // cannot be written by an other cursor.
        OP_CursorLock => {
            sqlite3BtreeCursorPin(&btcsr!(csr!(p1)));
            Ok(Fl::Next)
        }

        // Opcode: CursorUnlock P1 * * * *
        OP_CursorUnlock => {
            sqlite3BtreeCursorUnpin(&btcsr!(csr!(p1)));
            Ok(Fl::Next)
        }

        // Opcode: TableLock P1 P2 P3 P4 *
        //
        // Obtain a lock on a particular table.  This opcode is only used
        // when SQLITE_OMIT_SHARED_CACHE is not defined: a no-op here.
        OP_TableLock => Ok(Fl::Next),

        // Virtual table opcodes.  Virtual tables are not ported yet.
        OP_VBegin | OP_VCreate | OP_VDestroy | OP_VOpen | OP_VCheck | OP_VInitIn | OP_VFilter
        | OP_VColumn | OP_VNext | OP_VRename | OP_VUpdate => op_vtab_unsupported(db, p, s),

        // Opcode: Pagecount P1 P2 * * *
        //
        // Write the current number of pages in database P1 to memory cell
        // P2.
        OP_Pagecount => {
            let n = sqlite3BtreeLastPage(&pbt!(p1));
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = n as i64;
            Ok(Fl::Next)
        }

        // Opcode: MaxPgcnt P1 P2 P3 * *
        //
        // Try to set the maximum page count for database P1 to the value in
        // P3.  Do not let the maximum page count fall below the current page
        // count and do not change the maximum page count value if P3==0.
        OP_MaxPgcnt => {
            let pBt = pbt!(p1);
            let mut newMax: u32 = 0;
            if p3 != 0 {
                newMax = sqlite3BtreeLastPage(&pBt);
                if newMax < p3 as u32 {
                    newMax = p3 as u32;
                }
            }
            let v = sqlite3BtreeMaxPageCount(&pBt, newMax);
            let pOut = out2Prerelease(&mut mem!(p2));
            pOut.u.i = v as i64;
            Ok(Fl::Next)
        }

        // Opcode: Function P1 P2 P3 P4 *
        // Opcode: PureFunc P1 P2 P3 P4 *
        //
        // Invoke a user function (P4 is a pointer to an sqlite3_context
        // object that contains a pointer to the function to be run) with
        // arguments taken from register P2 and successors.  The result of
        // the function is stored in register P3.
        OP_PureFunc | OP_Function => op_function(db, p, s, pc, opcode, p1, p2, p3, p5),

        // Opcode: ClrSubtype P1 * * * *
        //
        // Clear the subtype from register P1.
        OP_ClrSubtype => {
            mem!(p1).flags &= !MEM_Subtype;
            Ok(Fl::Next)
        }

        // Opcode: GetSubtype P1 P2 * * *
        //
        // Extract the subtype value from register P1 and write that subtype
        // into register P2.  If P1 has no subtype, then P1 gets a NULL.
        OP_GetSubtype => {
            if (mem!(p1).flags & MEM_Subtype) != 0 {
                let st = mem!(p1).eSubtype;
                sqlite3VdbeMemSetInt64(&mut mem!(p2), st as i64);
            } else {
                sqlite3VdbeMemSetNull(&mut mem!(p2));
            }
            Ok(Fl::Next)
        }

        // Opcode: SetSubtype P1 P2 * * *
        //
        // Set the subtype value of register P2 to the integer from register
        // P1.  If P1 is NULL, clear the subtype from p2.
        OP_SetSubtype => {
            if (mem!(p1).flags & MEM_Null) != 0 {
                mem!(p2).flags &= !MEM_Subtype;
            } else {
                let st = (mem!(p1).u.i & 0xff) as u8;
                let pOut = &mut mem!(p2);
                pOut.flags |= MEM_Subtype;
                pOut.eSubtype = st;
            }
            Ok(Fl::Next)
        }

        // Opcode: FilterAdd P1 * P3 P4 *
        //
        // Compute a hash on the P4 registers starting with r[P3] and add
        // that hash to the bloom filter contained in r[P1].
        OP_FilterAdd => {
            let n = p4_i!();
            let mut h = filterHash(db, &p.aMem, p3, n);
            let pIn1 = &mut mem!(p1);
            let nbits = (pIn1.n as i64 * 8) as u64;
            if nbits > 0 {
                h %= nbits;
                pIn1.z[(h / 8) as usize] |= 1 << (h & 7);
            }
            Ok(Fl::Next)
        }

        // Opcode: Filter P1 P2 P3 P4 *
        //
        // Compute a hash on the key contained in the P4 registers starting
        // with r[P3].  Check to see if that hash is found in the bloom
        // filter hosted by register P1.  If it is not present then maybe
        // jump to P2.  Otherwise fall through.
        OP_Filter => {
            let n = p4_i!();
            let mut h = filterHash(db, &p.aMem, p3, n);
            let pIn1 = &mem!(p1);
            let nbits = (pIn1.n as i64 * 8) as u64;
            let hit = if nbits > 0 {
                h %= nbits;
                (pIn1.z[(h / 8) as usize] & (1 << (h & 7))) != 0
            } else {
                true
            };
            if !hit {
                p.aCounter[SQLITE_STMTSTATUS_FILTER_HIT as usize] += 1;
                jump_to_p2!();
            } else {
                p.aCounter[SQLITE_STMTSTATUS_FILTER_MISS as usize] += 1;
            }
            Ok(Fl::Next)
        }

        // Opcode: Trace P1 P2 * P4 *
        // Opcode: Init P1 P2 P3 P4 *
        //
        // Programs contain a single instance of this opcode as the very
        // first opcode.  If tracing is enabled (by the sqlite3_trace())
        // interface, then the UTF-8 string contained in P4 is emitted on the
        // trace callback.  Or if P4 is blank, use the string returned by
        // sqlite3_sql().
        OP_Trace | OP_Init => {
            if (db.mTrace as u32 & (SQLITE_TRACE_STMT | SQLITE_TRACE_LEGACY)) != 0
                && p.minWriteFileFormat != 254
            {
                let zTrace = match p4_z!() {
                    Some(z) => Some(z),
                    None => p.zSql.clone(),
                };
                if let Some(zTrace) = zTrace {
                    if (db.mTrace as u32 & SQLITE_TRACE_LEGACY) != 0 {
                        let z = crate::vdbetrace::sqlite3VdbeExpandSql(db, p, Some(&zTrace))
                            .unwrap_or_default();
                        if let Some(x) = db.xTrace.clone() {
                            x(SQLITE_TRACE_LEGACY, &z);
                        }
                    } else if db.nVdbeExec > 1 {
                        let z = crate::mprintf!(db, b"-- %s", &zTrace);
                        if let Some(x) = db.xTrace.clone() {
                            x(SQLITE_TRACE_STMT, &z);
                        }
                    } else if let Some(x) = db.xTrace.clone() {
                        x(SQLITE_TRACE_STMT, &zTrace);
                    }
                }
            }
            if p1 >= crate::global::sqlite3GlobalConfig().iOnceResetThreshold {
                if opcode == OP_Trace {
                    return Ok(Fl::Next);
                }
                for i in 1..p.aOp.len() {
                    if p.aOp[i].opcode == OP_Once {
                        p.aOp[i].p1 = 0;
                    }
                }
                p.aOp[pc as usize].p1 = 0;
            }
            p.aOp[pc as usize].p1 += 1;
            p.aCounter[SQLITE_STMTSTATUS_RUN as usize] += 1;
            jump_to_p2!();
        }

        // Opcode: CursorHint P1 * P3 P4 *
        //
        // (SQLITE_ENABLE_CURSOR_HINTS only.)
        OP_CursorHint => Ok(Fl::Next),

        // Opcode:  Abortable   * * * * *
        //
        // Verify that an Abort can happen (SQLITE_DEBUG only).
        OP_Abortable => Ok(Fl::Next),

        // Opcode:  ReleaseReg   P1 P2 P3 * P5
        //
        // Release registers from service (SQLITE_DEBUG only; a no-op here).
        OP_ReleaseReg => Ok(Fl::Next),

        // Opcode: Noop * * * * *
        // Opcode: Explain P1 P2 P3 P4 *
        //
        // This is really OP_Noop, OP_Explain
        _ => Ok(Fl::Next),
    }
}

// ---------------------------------------------------------------------------
// Opcode implementations factored out of vdbe_step()
// ---------------------------------------------------------------------------

/// `goto abort_due_to_error` with the given rc.
macro_rules! abort_rc {
    ($s:expr, $rc:expr) => {{
        $s.rc = $rc;
        return Err(Ex::Error);
    }};
}

/// `if( rc ) goto abort_due_to_error;`
macro_rules! chk_rc {
    ($s:expr, $e:expr) => {{
        let rc_: i32 = $e;
        if rc_ != SQLITE_OK {
            abort_rc!($s, rc_);
        }
    }};
}

/// OP_Halt (also reached by OP_HaltIfNull and by running off the end of
/// the program).
fn op_halt(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    p5: u16,
    p4type: i8,
) -> Result<Fl, Ex> {
    debug_assert!(p4type == P4_NOTUSED || p4type == P4_STATIC || p4type == P4_DYNAMIC);
    if p.pFrame.is_some() && p1 == SQLITE_OK {
        // Halt the sub-program. Return control to the parent frame.
        let mut pFrame = p.pFrame.take().unwrap();
        p.pFrame = pFrame.pParent.take();
        p.nFrame -= 1;
        sqlite3VdbeSetChanges(db, p.nChange);
        let mut pcx = sqlite3VdbeFrameRestore(db, p, &mut pFrame);
        if p2 == OE_Ignore as i32 {
            // Instruction pcx is the OP_Program that invoked the sub-program
            // currently being halted.  If the p2 instruction of this OP_Halt
            // instruction is set to OE_Ignore, then the sub-program is
            // throwing an IGNORE exception.  In this case jump to the address
            // specified as the p2 of the calling OP_Program.
            pcx = p.aOp[pcx as usize].p2 - 1;
        }
        // Rust: frames are not cached in register P3 of the OP_Program (see
        // VdbeFrame); dispose of it now.
        sqlite3VdbeFrameDelete(db, &mut pFrame);
        s.pc = pcx;
        return Ok(Fl::Next);
    }
    p.rc = p1;
    p.errorAction = p2 as u8;
    debug_assert!(p5 <= 4);
    if p.rc != 0 {
        let z4: Option<ZStr> = match p.aOp.get(pc as usize).map(|o| &o.p4) {
            Some(P4::Z(z)) => Some(z.clone()),
            _ => None,
        };
        if p3 > 0 && p4type == P4_NOTUSED {
            let zErr = sqlite3ValueText(db, &mut p.aMem[p3 as usize], SQLITE_UTF8);
            verr!(db, p, b"%s", zErr.as_deref());
        } else if p5 != 0 {
            const AZ_TYPE: [&[u8]; 4] = [b"NOT NULL", b"UNIQUE", b"CHECK", b"FOREIGN KEY"];
            verr!(db, p, b"%s constraint failed", AZ_TYPE[(p5 - 1) as usize]);
            if let Some(z) = z4 {
                let mut m = p.zErrMsg.take().unwrap_or_default();
                m.extend_from_slice(b": ");
                m.extend_from_slice(&z);
                p.zErrMsg = Some(m);
            }
        } else {
            verr!(db, p, b"%s", z4.as_deref());
        }
        sqlite3VdbeLogAbort(p, p1, pc);
    }
    let mut rc = sqlite3VdbeHalt(db, p);
    debug_assert!(rc == SQLITE_BUSY || rc == SQLITE_OK || rc == SQLITE_ERROR);
    if rc == SQLITE_BUSY {
        p.rc = SQLITE_BUSY;
    } else {
        rc = if p.rc != 0 { SQLITE_ERROR } else { SQLITE_DONE };
    }
    s.rc = rc;
    Err(Ex::Return)
}

/// Fill in pC->aType[i] and aOffset[i] values through the p2-th field
/// (the op_column_read_header part of OP_Column).  `ext` is the record
/// header when it had to be loaded separately (C: sMem), otherwise the
/// header is read from pC->aRow.  Returns false if the record is corrupt.
fn column_read_header(pC: &mut VdbeCursor, ext: Option<&[u8]>, p2: u32) -> bool {
    let VdbeCursor {
        aRow,
        aType,
        aOffset,
        nHdrParsed,
        iHdrOffset,
        payloadSize,
        ..
    } = pC;
    let zData: &[u8] = match ext {
        Some(x) => x,
        None => &aRow[..],
    };
    let mut i = *nHdrParsed as usize;
    let mut offset64: u64 = aOffset[i] as u64;
    let mut zHdr = *iHdrOffset as usize;
    let zEndHdr = aOffset[0] as usize;
    loop {
        if i >= aType.len() {
            aType.resize(i + 1, 0);
        }
        let t0 = byte_at(zData, zHdr);
        if t0 < 0x80 {
            aType[i] = t0 as u32;
            zHdr += 1;
            offset64 += sqlite3VdbeOneByteSerialTypeLen(t0) as u64;
        } else {
            let mut t = 0u32;
            zHdr += get_varint32_at(zData, zHdr, &mut t);
            aType[i] = t;
            offset64 += sqlite3VdbeSerialTypeLen(t) as u64;
        }
        i += 1;
        if i >= aOffset.len() {
            aOffset.resize(i + 1, 0);
        }
        aOffset[i] = (offset64 & 0xffffffff) as u32;
        if !((i as u32) <= p2 && zHdr < zEndHdr) {
            break;
        }
    }
    // The record is corrupt if any of the following are true:
    // (1) the bytes of the header extend past the declared header size
    // (2) the entire header was used but not all data was used
    // (3) the end of the data extends beyond the end of the record.
    if (zHdr >= zEndHdr && (zHdr > zEndHdr || offset64 != *payloadSize as u64))
        || offset64 > *payloadSize as u64
    {
        if aOffset[0] == 0 {
            i = 0;
            zHdr = zEndHdr;
        } else {
            return false;
        }
    }
    *nHdrParsed = i as u16;
    *iHdrOffset = zHdr as u32;
    true
}

/// OP_Column
fn op_column(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    p1: i32,
    p2arg: i32,
    p3: i32,
    p5: u16,
    p4type: i8,
) -> Result<Fl, Ex> {
    let encoding = s.encoding;
    let cacheCtr = p.cacheCtr;
    let mut iCur = p1;
    let mut p2 = p2arg as u32;
    let i3 = p3 as usize;

    // op_column_corrupt:
    macro_rules! column_corrupt {
        () => {{
            if p.aOp[0].p3 > 0 {
                s.pc = p.aOp[0].p3 - 1;
                return Ok(Fl::Next);
            } else {
                abort_rc!(s, corrupt_bkpt());
            }
        }};
    }

    'restart: loop {
        let pC = p.apCsr[iCur as usize]
            .as_deref_mut()
            .expect("VDBE cursor not open");
        debug_assert!(
            p2 < pC.nField as u32 || (pC.eCurType == CURTYPE_PSEUDO && pC.seekResult == 0)
        );
        let mut fast_header = false;
        if pC.cacheStatus != cacheCtr {
            if pC.nullRow != 0 {
                if pC.eCurType == CURTYPE_PSEUDO && pC.seekResult > 0 {
                    // For the special case of as pseudo-cursor, the
                    // seekResult field identifies the register that holds
                    // the record
                    let pReg = &p.aMem[pC.seekResult as usize];
                    pC.payloadSize = pReg.n as u32;
                    pC.szRow = pReg.n as u32;
                    pC.aRow = pReg.bytes().to_vec();
                } else {
                    sqlite3VdbeMemSetNull(&mut p.aMem[i3]);
                    return Ok(Fl::Next);
                }
            } else {
                let pCrsr = pC.pCursor.clone().expect("btree cursor");
                if pC.deferredMoveto != 0 {
                    let iMap = match &pC.aAltMap {
                        Some(m) => m.get(1 + p2 as usize).copied().unwrap_or(0),
                        None => 0,
                    };
                    if iMap > 0 {
                        iCur = pC.pAltCursor;
                        p2 = iMap - 1;
                        continue 'restart;
                    }
                    chk_rc!(s, sqlite3VdbeFinishMoveto(db, pC));
                } else if sqlite3BtreeCursorHasMoved(&pCrsr) != 0 {
                    chk_rc!(s, sqlite3VdbeHandleMovedCursor(pC));
                    continue 'restart;
                }
                pC.payloadSize = sqlite3BtreePayloadSize(&pCrsr);
                let mut szRow = 0u32;
                pC.aRow = sqlite3BtreePayloadFetch(&pCrsr, &mut szRow);
                pC.szRow = szRow;
                debug_assert!(pC.szRow <= 65536); // Maximum page size is 64KiB
            }
            pC.cacheStatus = cacheCtr;
            let b0 = byte_at(&pC.aRow, 0);
            if b0 < 0x80 {
                pC.aOffset[0] = b0 as u32;
                pC.iHdrOffset = 1;
            } else {
                let mut v = 0u32;
                pC.iHdrOffset = get_varint32_at(&pC.aRow, 0, &mut v) as u32;
                pC.aOffset[0] = v;
            }
            pC.nHdrParsed = 0;

            if pC.szRow < pC.aOffset[0] {
                // pC->aRow does not have to hold the entire row, but it does
                // at least need to cover the header of the record.  If
                // pC->aRow does not contain the complete header, then set it
                // to zero, forcing the header to be dynamically allocated.
                pC.aRow = Vec::new();
                pC.szRow = 0;

                // Make sure a corrupt database has not given us an oversize
                // header.  Do this now to avoid an oversize memory
                // allocation.
                if pC.aOffset[0] > 98307 || pC.aOffset[0] > pC.payloadSize {
                    column_corrupt!();
                }
            } else {
                // This is an optimization.  By skipping over the first few
                // tests (ex: pC->nHdrParsed<=p2) in the next section, we
                // achieve a measurable performance gain.
                fast_header = true;
            }
        } else {
            let moved = match &pC.pCursor {
                Some(c) => sqlite3BtreeCursorHasMoved(c) != 0,
                None => false,
            };
            if moved {
                chk_rc!(s, sqlite3VdbeHandleMovedCursor(pC));
                continue 'restart;
            }
        }

        // Make sure at least the first p2+1 entries of the header have been
        // parsed and valid information is in aOffset[] and pC->aType[].
        if fast_header || (pC.nHdrParsed as u32) <= p2 {
            // If there is more header available for parsing in the record,
            // try to extract additional fields up through the p2+1-th field
            if fast_header || pC.iHdrOffset < pC.aOffset[0] {
                // Make sure zData points to enough of the record to cover
                // the header.
                let ok = if pC.aRow.is_empty() && !fast_header {
                    let mut sMem = Mem::default();
                    let pCrsr = pC.pCursor.clone().expect("btree cursor");
                    chk_rc!(
                        s,
                        sqlite3VdbeMemFromBtreeZeroOffset(&pCrsr, pC.aOffset[0], &mut sMem)
                    );
                    let hdr = sMem.bytes().to_vec();
                    let ok = column_read_header(pC, Some(&hdr), p2);
                    sqlite3VdbeMemRelease(&mut sMem);
                    ok
                } else {
                    column_read_header(pC, None, p2)
                };
                if !ok {
                    column_corrupt!();
                }
            }

            // If after trying to extract new entries from the header,
            // nHdrParsed is still not up to p2, that means that the record
            // has fewer than p2 columns.  So the result will be either the
            // default value or a NULL.
            if (pC.nHdrParsed as u32) <= p2 {
                if p4type == P4_MEM {
                    if let P4::Mem(m) = &p.aOp[pc as usize].p4 {
                        sqlite3VdbeMemShallowCopy(db, &mut p.aMem[i3], m, MEM_Static as i32);
                    }
                } else {
                    sqlite3VdbeMemSetNull(&mut p.aMem[i3]);
                }
                return Ok(Fl::Next);
            }
        }
        let t = pC.aType[p2 as usize];

        // Extract the content for the p2+1-th column.  Control can only
        // reach this point if aOffset[p2], aOffset[p2+1], and pC->aType[p2]
        // are all valid.
        let pDest = &mut p.aMem[i3];
        if pDest.VdbeMemDynamic() {
            sqlite3VdbeMemSetNull(pDest);
        }
        let off = pC.aOffset[p2 as usize];
        if pC.szRow >= pC.aOffset[p2 as usize + 1] {
            // This is the common case where the desired content fits on the
            // original page - where the content is not on an overflow page.
            //
            // The big switch() is an in-line variant of
            // sqlite3VdbeSerialGet() that has been optimized for the
            // OP_Column opcode.
            let zData = &pC.aRow[(off as usize).min(pC.aRow.len())..];
            let be = |n: usize| -> u64 {
                let mut v: u64 = 0;
                for k in 0..n {
                    v = (v << 8) | byte_at(zData, k) as u64;
                }
                v
            };
            match t {
                0 | 11 => {
                    pDest.flags = MEM_Null;
                }
                1 => {
                    pDest.u.i = byte_at(zData, 0) as i8 as i64;
                    pDest.flags = MEM_Int;
                }
                2 => {
                    pDest.u.i = be(2) as u16 as i16 as i64;
                    pDest.flags = MEM_Int;
                }
                3 => {
                    let v = be(3) as i64;
                    pDest.u.i = (v << 40) >> 40;
                    pDest.flags = MEM_Int;
                }
                4 => {
                    pDest.u.i = be(4) as u32 as i32 as i64;
                    pDest.flags = MEM_Int;
                }
                5 => {
                    let v = be(6) as i64;
                    pDest.u.i = (v << 16) >> 16;
                    pDest.flags = MEM_Int;
                }
                6 => {
                    pDest.u.i = be(8) as i64;
                    pDest.flags = MEM_Int;
                }
                7 => {
                    let r = f64::from_bits(be(8));
                    pDest.flags = if r.is_nan() { MEM_Null } else { MEM_Real };
                    pDest.u.r = r;
                }
                8 | 9 => {
                    pDest.u.i = (t - 8) as i64;
                    pDest.flags = MEM_Int;
                }
                10 => {
                    // Internal use only: NULL with virtual table UPDATE
                    // no-change flag set
                    pDest.flags = MEM_Null | MEM_Zero;
                    pDest.u.nZero = 0;
                    pDest.n = 0;
                }
                _ => {
                    // If the column value is a string or blob, we need a
                    // persistent value, not a MEM_Ephem value.
                    let len = ((t - 12) / 2) as usize;
                    pDest.n = len as i32;
                    pDest.enc = encoding;
                    if pDest.z.capacity() < len + 2 {
                        if len as i64 > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64 {
                            return Err(Ex::TooBig);
                        }
                        pDest.flags = MEM_Null;
                    }
                    pDest.z.clear();
                    pDest.z.extend_from_slice(&zData[..len.min(zData.len())]);
                    if pDest.z.len() < len {
                        pDest.z.resize(len, 0);
                    }
                    pDest.flags = if (t & 1) != 0 {
                        MEM_Str | MEM_Term
                    } else {
                        MEM_Blob
                    };
                }
            }
        } else {
            pDest.enc = encoding;
            // This branch happens only when content is on overflow pages
            let p5b = p5 & OPFLAG_BYTELENARG;
            if (p5b != 0
                && (p5b == OPFLAG_TYPEOFARG
                    || (t >= 12 && ((t & 1) == 0 || p5b == OPFLAG_BYTELENARG))))
                || sqlite3VdbeSerialTypeLen(t) == 0
            {
                // Content is irrelevant for
                //    1. the typeof() function,
                //    2. the length(X) function if X is a blob, and
                //    3. if the content length is zero.
                // So we might as well use bogus content rather than reading
                // content from disk.
                let n = sqlite3VdbeSerialTypeLen(t) as usize;
                if n <= crate::global::sqlite3CtypeMap.len() {
                    sqlite3VdbeSerialGet(&crate::global::sqlite3CtypeMap[..], t, pDest);
                } else {
                    let zeros = vec![0u8; n];
                    sqlite3VdbeSerialGet(&zeros, t, pDest);
                }
            } else {
                let rc = vdbeColumnFromOverflow(
                    db,
                    pC,
                    p2 as i32,
                    t,
                    off as i64,
                    cacheCtr,
                    s.colCacheCtr,
                    pDest,
                );
                if rc != 0 {
                    if rc == SQLITE_NOMEM {
                        return Err(Ex::NoMem);
                    }
                    if rc == SQLITE_TOOBIG {
                        return Err(Ex::TooBig);
                    }
                    abort_rc!(s, rc);
                }
            }
        }
        // op_column_out:
        return Ok(Fl::Next);
    }
}

/// OP_MakeRecord
fn op_makerecord(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    _p5: u16,
) -> Result<Fl, Ex> {
    let encoding = s.encoding;
    // Assuming the record contains N fields, the record format looks like
    // this:
    //
    // | hdr-size | type 0 | type 1 | ... | type N-1 | data0 | ... | data N-1 |
    //
    // Data(0) is taken from register P1.  Data(1) comes from register P1+1
    // and so forth.
    let mut nData: u64 = 0; // Number of bytes of data space
    let mut nHdr: i32 = 0; // Number of bytes of header space
    let mut nZero: i64 = 0; // Number of zero bytes at the end of the record
    let zAffinity: Option<ZStr> = match &p.aOp[pc as usize].p4 {
        P4::Z(z) => Some(z.clone()),
        _ => None,
    };
    let iData0 = p1 as usize;
    let nField = p2;
    let iLast = iData0 + nField as usize - 1;
    debug_assert!(p3 < p1 || p3 >= p1 + p2);

    // Apply the requested affinity to all inputs
    if let Some(za) = &zAffinity {
        let mut i = iData0;
        for &a in za.iter() {
            if a == 0 {
                break;
            }
            let pRec = &mut p.aMem[i];
            applyAffinity(db, pRec, a, encoding);
            if a == SQLITE_AFF_REAL && (pRec.flags & MEM_Int) != 0 {
                pRec.flags |= MEM_IntReal;
                pRec.flags &= !MEM_Int;
            }
            i += 1;
        }
    }

    // Loop through the elements that will make up the record to figure out
    // how much space is required for the new record.  After this loop, the
    // Mem.uTemp field of each term holds the serial-type that will be used
    // for that term in the generated record.
    let mut i = iLast;
    loop {
        let minWriteFileFormat = p.minWriteFileFormat;
        let pRec = &mut p.aMem[i];
        if (pRec.flags & MEM_Null) != 0 {
            if (pRec.flags & MEM_Zero) != 0 {
                // Values with MEM_Null and MEM_Zero are created by xColumn
                // virtual table methods that never invoke
                // sqlite3_result_xxxxx() while computing an unchanging
                // column value in an UPDATE statement.  Give such values a
                // special internal-use-only serial-type of 10 so that they
                // can be passed through to xUpdate and have a true
                // sqlite3_value_nochange().
                pRec.uTemp = 10;
            } else {
                pRec.uTemp = 0;
            }
            nHdr += 1;
        } else if (pRec.flags & (MEM_Int | MEM_IntReal)) != 0 {
            // Figure out whether to use 1, 2, 4, 6 or 8 bytes.
            let iv = pRec.u.i;
            let uu: u64 = if iv < 0 { !iv as u64 } else { iv as u64 };
            nHdr += 1;
            if uu <= 127 {
                if (iv & 1) == iv && minWriteFileFormat >= 4 {
                    pRec.uTemp = 8 + uu as u32;
                } else {
                    nData += 1;
                    pRec.uTemp = 1;
                }
            } else if uu <= 32767 {
                nData += 2;
                pRec.uTemp = 2;
            } else if uu <= 8388607 {
                nData += 3;
                pRec.uTemp = 3;
            } else if uu <= 2147483647 {
                nData += 4;
                pRec.uTemp = 4;
            } else if uu <= 140737488355327 {
                nData += 6;
                pRec.uTemp = 5;
            } else {
                nData += 8;
                if (pRec.flags & MEM_IntReal) != 0 {
                    // If the value is IntReal and is going to take up 8
                    // bytes to store as an integer, then we might as well
                    // make it an 8-byte floating point value
                    pRec.u.r = pRec.u.i as f64;
                    pRec.flags &= !MEM_IntReal;
                    pRec.flags |= MEM_Real;
                    pRec.uTemp = 7;
                } else {
                    pRec.uTemp = 6;
                }
            }
        } else if (pRec.flags & MEM_Real) != 0 {
            nHdr += 1;
            nData += 8;
            pRec.uTemp = 7;
        } else {
            let mut len = pRec.n as u32;
            let mut serial_type: u32 = (len * 2) + 12 + ((pRec.flags & MEM_Str) != 0) as u32;
            if (pRec.flags & MEM_Zero) != 0 {
                serial_type += pRec.u.nZero as u32 * 2;
                if nData != 0 {
                    let nz = pRec.u.nZero;
                    if sqlite3VdbeMemExpandBlob(db, pRec) != 0 {
                        return Err(Ex::NoMem);
                    }
                    len += nz as u32;
                } else {
                    nZero += pRec.u.nZero as i64;
                }
            }
            nData += len as u64;
            nHdr += crate::util::sqlite3VarintLen(serial_type as u64);
            pRec.uTemp = serial_type;
        }
        if i == iData0 {
            break;
        }
        i -= 1;
    }

    // EVIDENCE-OF: R-22564-11647 The header begins with a single varint
    // which determines the total number of bytes in the header.  The varint
    // value is the size of the header in bytes including the size varint
    // itself.
    if nHdr <= 126 {
        // The common case
        nHdr += 1;
    } else {
        // Rare case of a really large header
        let nVarint = crate::util::sqlite3VarintLen(nHdr as u64);
        nHdr += nVarint;
        if nVarint < crate::util::sqlite3VarintLen(nHdr as u64) {
            nHdr += 1;
        }
    }
    let nByte: i64 = nHdr as i64 + nData as i64;

    // Make sure the output register has a buffer large enough to store the
    // new record.
    if nByte + nZero > db.aLimit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        return Err(Ex::TooBig);
    }
    let mut z: Vec<u8> = vec![0u8; nByte as usize];
    let mut zHdr: usize = 0;
    let mut zPayload: usize = nHdr as usize;

    // Write the record
    if nHdr < 0x80 {
        z[zHdr] = nHdr as u8;
        zHdr += 1;
    } else {
        zHdr += crate::util::sqlite3PutVarint(&mut z[zHdr..], nHdr as u64) as usize;
    }
    const SMALL_TYPE_SIZES: [usize; 8] = [0, 1, 2, 3, 4, 6, 8, 8];
    let mut i = iData0;
    loop {
        let pRec = &p.aMem[i];
        let serial_type = pRec.uTemp;
        // EVIDENCE-OF: R-06529-47362 Following the size varint are one or
        // more additional varints, one per column.
        // EVIDENCE-OF: R-64536-51728 The values for each column in the
        // record immediately follow the header.
        if serial_type <= 7 {
            z[zHdr] = serial_type as u8;
            zHdr += 1;
            if serial_type != 0 {
                let v: u64 = if serial_type == 7 {
                    pRec.u.r.to_bits()
                } else {
                    pRec.u.i as u64
                };
                let len = SMALL_TYPE_SIZES[serial_type as usize];
                let b = v.to_be_bytes();
                z[zPayload..zPayload + len].copy_from_slice(&b[8 - len..]);
                zPayload += len;
            }
        } else if serial_type < 0x80 {
            z[zHdr] = serial_type as u8;
            zHdr += 1;
            if serial_type >= 14 && pRec.n > 0 {
                let src = pRec.bytes();
                z[zPayload..zPayload + src.len()].copy_from_slice(src);
                zPayload += src.len();
            }
        } else {
            zHdr += crate::util::sqlite3PutVarint(&mut z[zHdr..], serial_type as u64) as usize;
            if pRec.n != 0 {
                let src = pRec.bytes();
                z[zPayload..zPayload + src.len()].copy_from_slice(src);
                zPayload += src.len();
            }
        }
        if i == iLast {
            break;
        }
        i += 1;
    }

    // Identify the output register
    let pOut = &mut p.aMem[p3 as usize];
    if pOut.VdbeMemDynamic() {
        sqlite3VdbeMemSetNull(pOut);
    }
    pOut.z = z;
    pOut.n = nByte as i32;
    pOut.flags = MEM_Blob;
    if nZero != 0 {
        pOut.u.nZero = nZero as i32;
        pOut.flags |= MEM_Zero;
    }
    Ok(Fl::Next)
}

/// OP_Savepoint
fn op_savepoint(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    p1: i32,
) -> Result<Fl, Ex> {
    let zName: ZStr = match &p.aOp[pc as usize].p4 {
        P4::Z(z) => z.clone(),
        _ => Vec::new(),
    };
    let mut rc = SQLITE_OK;

    // Assert that the p1 parameter is valid.  Also that if there is no open
    // transaction, then there cannot be any savepoints.
    debug_assert!(p1 == SAVEPOINT_BEGIN || p1 == SAVEPOINT_RELEASE || p1 == SAVEPOINT_ROLLBACK);
    debug_assert!(db.pSavepoint.is_empty() || db.autoCommit == 0);

    if p1 == SAVEPOINT_BEGIN {
        if db.nVdbeWrite > 0 {
            // A new savepoint cannot be created if there are active write
            // statements (i.e. open read/write incremental blob handles).
            verr!(db, p, b"cannot open savepoint - SQL statements in progress");
            rc = SQLITE_BUSY;
        } else {
            // This call is Ok even if this savepoint is actually a
            // transaction savepoint (and therefore should not prompt
            // xSavepoint()) callbacks.
            let n = db.nStatement + db.nSavepoint;
            chk_rc!(s, crate::vtab::sqlite3VtabSavepoint(db, SAVEPOINT_BEGIN, n));

            // Create a new savepoint structure.
            let pNew = Savepoint {
                zName: zName.clone(),
                nDeferredCons: db.nDeferredCons,
                nDeferredImmCons: db.nDeferredImmCons,
            };
            // If there is no open transaction, then mark this as a special
            // "transaction savepoint".
            if db.autoCommit != 0 {
                db.autoCommit = 0;
                db.isTransactionSavepoint = 1;
            } else {
                db.nSavepoint += 1;
            }
            // Link the new savepoint into the database handle's list.
            db.pSavepoint.insert(0, pNew);
        }
    } else {
        // Find the named savepoint.  If there is no such savepoint, then an
        // an error is returned to the user.
        let found = db
            .pSavepoint
            .iter()
            .position(|sp| crate::util::sqlite3StrICmp(Some(&sp.zName), Some(&zName)) == 0);
        match found {
            None => {
                verr!(db, p, b"no such savepoint: %s", &zName);
                rc = SQLITE_ERROR;
            }
            Some(_) if db.nVdbeWrite > 0 && p1 == SAVEPOINT_RELEASE => {
                // It is not possible to release (commit) a savepoint if
                // there are active write statements.
                verr!(
                    db,
                    p,
                    b"cannot release savepoint - SQL statements in progress"
                );
                rc = SQLITE_BUSY;
            }
            Some(iFound) => {
                let mut iSavepoint = iFound as i32;
                // Determine whether or not this is a transaction savepoint.
                // If so, and this is a RELEASE command, then the current
                // transaction is committed.
                let isTransaction =
                    iFound + 1 == db.pSavepoint.len() && db.isTransactionSavepoint != 0;
                if isTransaction && p1 == SAVEPOINT_RELEASE {
                    rc = sqlite3VdbeCheckFkDeferred(db, p);
                    if rc != SQLITE_OK {
                        s.rc = rc;
                        return Err(Ex::Return);
                    }
                    db.autoCommit = 1;
                    if sqlite3VdbeHalt(db, p) == SQLITE_BUSY {
                        p.pc = pc;
                        db.autoCommit = 0;
                        p.rc = SQLITE_BUSY;
                        s.rc = SQLITE_BUSY;
                        return Err(Ex::Return);
                    }
                    rc = p.rc;
                    if rc != 0 {
                        db.autoCommit = 0;
                    } else {
                        db.isTransactionSavepoint = 0;
                    }
                } else {
                    let isSchemaChange;
                    iSavepoint = db.nSavepoint - iSavepoint - 1;
                    if p1 == SAVEPOINT_ROLLBACK {
                        isSchemaChange = (db.mDbFlags & DBFLAG_SchemaChange) != 0;
                        for ii in 0..db.aDb.len() {
                            if let Some(bt) = db.aDb[ii].pBt.clone() {
                                rc = sqlite3BtreeTripAllCursors(
                                    &bt,
                                    SQLITE_ABORT_ROLLBACK,
                                    (!isSchemaChange) as i32,
                                );
                                if rc != SQLITE_OK {
                                    abort_rc!(s, rc);
                                }
                            }
                        }
                    } else {
                        isSchemaChange = false;
                    }
                    for ii in 0..db.aDb.len() {
                        if let Some(bt) = db.aDb[ii].pBt.clone() {
                            rc = sqlite3BtreeSavepoint(&bt, p1, iSavepoint);
                            if rc != SQLITE_OK {
                                abort_rc!(s, rc);
                            }
                        }
                    }
                    if isSchemaChange {
                        sqlite3ExpirePreparedStatements(db, 0);
                        crate::build::sqlite3ResetAllSchemasOfConnection(db);
                        db.mDbFlags |= DBFLAG_SchemaChange;
                    }
                }
                chk_rc!(s, rc);

                // Regardless of whether this is a RELEASE or ROLLBACK,
                // destroy all savepoints nested inside of the savepoint
                // being operated on.
                for _ in 0..iFound {
                    if db.pSavepoint.is_empty() {
                        break;
                    }
                    db.pSavepoint.remove(0);
                    db.nSavepoint -= 1;
                }

                // If it is a RELEASE, then destroy the savepoint being
                // operated on too.  If it is a ROLLBACK TO, then set the
                // number of deferred constraint violations present in the
                // database to the value stored when the savepoint was
                // created.
                if p1 == SAVEPOINT_RELEASE {
                    if !db.pSavepoint.is_empty() {
                        db.pSavepoint.remove(0);
                    }
                    if !isTransaction {
                        db.nSavepoint -= 1;
                    }
                } else if let Some(sp) = db.pSavepoint.first() {
                    db.nDeferredCons = sp.nDeferredCons;
                    db.nDeferredImmCons = sp.nDeferredImmCons;
                }

                if !isTransaction || p1 == SAVEPOINT_ROLLBACK {
                    chk_rc!(s, crate::vtab::sqlite3VtabSavepoint(db, p1, iSavepoint));
                }
            }
        }
    }
    chk_rc!(s, rc);
    if p.eVdbeState == VDBE_HALT_STATE {
        s.rc = SQLITE_DONE;
        return Err(Ex::Return);
    }
    Ok(Fl::Next)
}

/// OP_ReopenIdx, OP_OpenRead, OP_OpenWrite
fn op_open(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    opcode: u8,
    p1: i32,
    p2: i32,
    p3: i32,
    p5: u16,
    p4type: i8,
) -> Result<Fl, Ex> {
    let hints = (p5 & (OPFLAG_BULKCSR | OPFLAG_SEEKEQ)) as u32;
    if opcode == OP_ReopenIdx {
        if let Some(pCur) = p.apCsr[p1 as usize].as_deref() {
            if pCur.pgnoRoot == p2 as u32 {
                // Guaranteed by the code generator
                debug_assert!(pCur.iDb as i32 == p3);
                if let Some(c) = &pCur.pCursor {
                    sqlite3BtreeClearCursor(c);
                    // open_cursor_set_hints:
                    sqlite3BtreeCursorHintFlags(c, hints);
                }
                return Ok(Fl::Next);
            }
        }
        // If the cursor is not currently open or is open on a different
        // index, then fall through into OP_OpenRead to force a reopen
    }
    debug_assert!(opcode == OP_OpenRead || opcode == OP_ReopenIdx || p.readOnly == 0);
    if p.expired == 1 {
        abort_rc!(s, SQLITE_ABORT_ROLLBACK);
    }
    let mut nField = 0;
    let mut pKeyInfo: Option<KeyInfoRef> = None;
    let mut p2v = p2 as u32;
    let iDb = p3;
    let pX = db.aDb[iDb as usize].pBt.clone().expect("database not open");
    let wrFlag;
    if opcode == OP_OpenWrite {
        wrFlag = BTREE_WRCSR | (p5 & OPFLAG_FORDELETE) as i32;
        let ff = db.aDb[iDb as usize].pSchema.file_format;
        if ff < p.minWriteFileFormat {
            p.minWriteFileFormat = ff;
        }
        if (p5 & OPFLAG_P2ISREG) != 0 {
            let pIn2 = &mut p.aMem[p2v as usize];
            sqlite3VdbeMemIntegerify(db, pIn2);
            p2v = pIn2.u.i as u32;
            // The p2 value always comes from a prior OP_CreateBtree opcode
            // and that opcode will always set the p2 value to 2 or more or
            // else fail.
            debug_assert!(p2v >= 2);
        }
    } else {
        wrFlag = 0;
    }
    match &p.aOp[pc as usize].p4 {
        P4::KeyInfo(k) => {
            nField = k.borrow().nAllField as i32;
            pKeyInfo = Some(k.clone());
        }
        P4::I(i) => nField = *i,
        _ => {}
    }
    allocateCursor(db, p, p1, nField, CURTYPE_BTREE);
    let pCur = p.apCsr[p1 as usize].as_deref_mut().unwrap();
    pCur.iDb = iDb as i8;
    pCur.nullRow = 1;
    pCur.isOrdered = 1;
    pCur.pgnoRoot = p2v;
    let rc = sqlite3BtreeCursor(&pX, p2v, wrFlag, pKeyInfo.clone(), &mut pCur.pCursor);
    pCur.pKeyInfo = pKeyInfo;
    // Set the VdbeCursor.isTable variable.  Previous versions of SQLite used
    // to check if the root-page flags were sane at this point and report
    // database corruption if they were not, but this check has since moved
    // into the btree layer.
    pCur.isTable = (p4type != P4_KEYINFO) as u8;
    // open_cursor_set_hints:
    if let Some(c) = &pCur.pCursor {
        sqlite3BtreeCursorHintFlags(c, hints);
    }
    chk_rc!(s, rc);
    Ok(Fl::Next)
}

/// OP_SeekLT, OP_SeekLE, OP_SeekGE, OP_SeekGT
fn op_seek(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    opcode: u8,
    p1: i32,
    p2: i32,
    p3: i32,
) -> Result<Fl, Ex> {
    let mut oc = opcode as i32;
    let mut eqOnly = false;
    let mut res: i32 = 0;
    let pCrsr;
    {
        let pC = p.apCsr[p1 as usize].as_deref_mut().expect("cursor");
        pC.nullRow = 0;
        pC.deferredMoveto = 0;
        pC.cacheStatus = CACHE_STALE as u32;
        pCrsr = pC.pCursor.clone().expect("btree cursor");
    }
    let isTable = p.apCsr[p1 as usize].as_deref().unwrap().isTable != 0;
    let mut not_found = false;
    if isTable {
        // The OPFLAG_SEEKEQ/BTREE_SEEK_EQ flag is only set on index cursors
        //
        // The input value in P3 might be of any type: integer, real, string,
        // blob, or NULL.  But it needs to be an integer before we can do the
        // seek, so convert it.
        let pIn3 = &mut p.aMem[p3 as usize];
        let flags3 = pIn3.flags;
        if (flags3 & (MEM_Int | MEM_Real | MEM_IntReal | MEM_Str)) == MEM_Str {
            applyNumericAffinity(db, pIn3, 0);
        }
        let iKey = sqlite3VdbeIntValue(db, pIn3); // Get the integer key value
        let newType = pIn3.flags; // Record the type after applying numeric affinity
        pIn3.flags = flags3; // But convert the type back to its original

        // If the P3 value could not be converted into an integer without
        // loss of information, then special processing is required...
        if (newType & (MEM_Int | MEM_IntReal)) == 0 {
            if (newType & MEM_Real) == 0 {
                if (newType & MEM_Null) != 0 || oc >= OP_SeekGE as i32 {
                    s.pc = p2 - 1;
                    return Ok(Fl::Next);
                } else {
                    chk_rc!(s, sqlite3BtreeLast(db, &pCrsr, &mut res));
                    not_found = true;
                }
            }
            if !not_found {
                let c = sqlite3IntFloatCompare(iKey, pIn3.u.r);
                // If the approximation iKey is larger than the actual real
                // search term, substitute >= for > and < for <=.  e.g. if the
                // search term is 4.9 and the integer approximation 5:
                //
                //        (x >  4.9)    ->     (x >= 5)
                //        (x <= 4.9)    ->     (x <  5)
                if c > 0 {
                    debug_assert!(OP_SeekGE as i32 == OP_SeekGT as i32 - 1);
                    debug_assert!(OP_SeekLT as i32 == OP_SeekLE as i32 - 1);
                    if (oc & 0x0001) == (OP_SeekGT as i32 & 0x0001) {
                        oc -= 1;
                    }
                }
                // If the approximation iKey is smaller than the actual real
                // search term, substitute <= for < and > for >=.
                else if c < 0 {
                    debug_assert!(OP_SeekLE as i32 == OP_SeekLT as i32 + 1);
                    debug_assert!(OP_SeekGT as i32 == OP_SeekGE as i32 + 1);
                    if (oc & 0x0001) == (OP_SeekLT as i32 & 0x0001) {
                        oc += 1;
                    }
                }
            }
        }
        if !not_found {
            let rc = sqlite3BtreeTableMoveto(db, &pCrsr, iKey, 0, &mut res);
            p.apCsr[p1 as usize].as_deref_mut().unwrap().movetoTarget = iKey; // Used by OP_Delete
            chk_rc!(s, rc);
        }
    } else {
        // For a cursor with the OPFLAG_SEEKEQ/BTREE_SEEK_EQ hint, only the
        // OP_SeekGE and OP_SeekLE opcodes are allowed, and these must be
        // immediately followed by an OP_IdxGT or OP_IdxLT opcode,
        // respectively, with the same key.
        if sqlite3BtreeCursorHasHint(&pCrsr, BTREE_SEEK_EQ as u32) != 0 {
            eqOnly = true;
        }
        let nField = match &p.aOp[pc as usize].p4 {
            P4::I(i) => *i,
            _ => 0,
        };
        let mut r = UnpackedRecord::default();
        r.pKeyInfo = p.apCsr[p1 as usize].as_deref().unwrap().pKeyInfo.clone();
        r.nField = nField as u16;
        // The next line of code computes as follows, only faster:
        //   if( oc==OP_SeekGT || oc==OP_SeekLE ){
        //     r.default_rc = -1;
        //   }else{
        //     r.default_rc = +1;
        //   }
        r.default_rc = if (1 & (oc - OP_SeekLT as i32)) != 0 {
            -1
        } else {
            1
        };
        r.aMem = reg_copies(&p.aMem, p3, nField);
        r.eqSeen = 0;
        chk_rc!(s, sqlite3BtreeIndexMoveto(db, &pCrsr, &mut r, &mut res));
        if eqOnly && r.eqSeen == 0 {
            debug_assert!(res != 0);
            not_found = true;
        }
    }
    if !not_found {
        if oc >= OP_SeekGE as i32 {
            debug_assert!(oc == OP_SeekGE as i32 || oc == OP_SeekGT as i32);
            if res < 0 || (res == 0 && oc == OP_SeekGT as i32) {
                res = 0;
                let rc = sqlite3BtreeNext(&pCrsr, 0);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        res = 1;
                    } else {
                        abort_rc!(s, rc);
                    }
                }
            } else {
                res = 0;
            }
        } else {
            debug_assert!(oc == OP_SeekLT as i32 || oc == OP_SeekLE as i32);
            if res > 0 || (res == 0 && oc == OP_SeekLT as i32) {
                res = 0;
                let rc = sqlite3BtreePrevious(&pCrsr, 0);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        res = 1;
                    } else {
                        abort_rc!(s, rc);
                    }
                }
            } else {
                // res might be negative because the table is empty.  Check
                // to see if this is the case.
                res = sqlite3BtreeEof(&pCrsr);
            }
        }
    }
    // seek_not_found:
    if res != 0 {
        s.pc = p2 - 1;
    } else if eqOnly {
        debug_assert!(
            p.aOp[(pc + 1) as usize].opcode == OP_IdxLT
                || p.aOp[(pc + 1) as usize].opcode == OP_IdxGT
        );
        s.pc += 1; // Skip the OP_IdxLt or OP_IdxGT that follows
    }
    Ok(Fl::Next)
}

/// OP_ParseSchema
fn op_parseschema(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    p1: i32,
    p5: u16,
) -> Result<Fl, Ex> {
    // Any prepared statement that invokes this opcode will hold mutexes on
    // every btree.  This is a prerequisite for invoking
    // sqlite3InitCallback().
    let iDb = p1;
    let mut rc;
    let zWhere: Option<ZStr> = match &p.aOp[pc as usize].p4 {
        P4::Z(z) => Some(z.clone()),
        _ => None,
    };
    match zWhere {
        None => {
            crate::callback::sqlite3SchemaClear(db, iDb);
            db.mDbFlags &= !DBFLAG_SchemaKnownOk;
            rc = crate::prepare::sqlite3InitOne(db, iDb, &mut p.zErrMsg, p5 as u32);
            db.mDbFlags |= DBFLAG_SchemaChange;
            p.expired = 0;
        }
        Some(zWhere) => {
            let pBt = db.aDb[iDb as usize].pBt.clone().expect("database not open");
            let mut initData = InitData {
                iDb,
                rc: SQLITE_OK,
                mInitFlags: 0,
                nInitRow: 0,
                mxPage: sqlite3BtreeLastPage(&pBt),
                pzErrMsg: p.zErrMsg.take(),
            };
            let zDbSName = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();
            let zSql = crate::mprintf!(
                db,
                b"SELECT*FROM\"%w\".%s WHERE %s ORDER BY rowid",
                &zDbSName,
                LEGACY_SCHEMA_TABLE,
                &zWhere
            );
            db.init.busy = 1;
            // C: sqlite3_exec(db, zSql, sqlite3InitCallback, &initData, 0).
            // The callback cannot borrow the connection while sqlite3_exec()
            // runs, so the rows are collected first and then passed to
            // sqlite3InitCallback() in order (stopping at the first row it
            // rejects, as sqlite3_exec() would).
            let mut rows: Vec<Vec<Option<Vec<u8>>>> = Vec::new();
            {
                let mut cb = |argv: &[Option<&[u8]>], _cols: &[&[u8]]| -> i32 {
                    rows.push(argv.iter().map(|a| a.map(|x| x.to_vec())).collect());
                    0
                };
                let mut zErr: Option<ZStr> = None;
                rc = crate::legacy::sqlite3_exec(db, Some(&zSql), Some(&mut cb), &mut zErr);
            }
            if rc == SQLITE_OK {
                for row in &rows {
                    let argv: Vec<Option<&[u8]>> = row.iter().map(|a| a.as_deref()).collect();
                    if crate::prepare::sqlite3InitCallback(db, &mut initData, Some(&argv)) != 0 {
                        rc = SQLITE_ABORT;
                        crate::util::sqlite3Error(db, SQLITE_ABORT);
                        break;
                    }
                }
            }
            if rc == SQLITE_OK {
                rc = initData.rc;
            }
            if rc == SQLITE_OK && initData.nInitRow == 0 {
                // The OP_ParseSchema opcode with a non-NULL P4 argument
                // should parse at least one SQL statement.  Any less than
                // that indicates that the sqlite_schema table is corrupt.
                rc = corrupt_bkpt();
            }
            db.init.busy = 0;
            p.zErrMsg = initData.pzErrMsg.take();
        }
    }
    if rc != 0 {
        crate::build::sqlite3ResetAllSchemasOfConnection(db);
        if rc == SQLITE_NOMEM {
            return Err(Ex::NoMem);
        }
        abort_rc!(s, rc);
    }
    Ok(Fl::Next)
}

/// OP_Program
fn op_program(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    _p1: i32,
    _p2: i32,
    _p3: i32,
    p5: u16,
) -> Result<Fl, Ex> {
    let pProgramRef: SubProgramRef = match &p.aOp[pc as usize].p4 {
        P4::Program(sp) => sp.clone(),
        _ => panic!("OP_Program without sub-program"),
    };
    let pProgram = pProgramRef.borrow();

    // If the p5 flag is clear, then recursive invocation of triggers is
    // disabled for backwards compatibility (p5 is set if this sub-program is
    // really a trigger, not a foreign key action, and the flag set and
    // cleared by the "PRAGMA recursive_triggers" command is clear).
    //
    // It is recursive invocation of triggers, at the SQL level, that is
    // disabled.  In some cases a single trigger may generate more than one
    // SubProgram (if the trigger may be executed with more than one
    // different ON CONFLICT algorithm).  SubProgram structures associated
    // with a single trigger all have the same value for the
    // SubProgram.token variable.
    if p5 != 0 {
        let t = pProgram.token;
        let mut f = p.pFrame.as_deref();
        while let Some(fr) = f {
            if fr.token == t {
                return Ok(Fl::Next);
            }
            f = fr.pParent.as_deref();
        }
    }

    if p.nFrame >= db.aLimit[SQLITE_LIMIT_TRIGGER_DEPTH as usize] {
        verr!(db, p, b"triggers nested too deep");
        abort_rc!(s, SQLITE_ERROR);
    }

    // SubProgram.nMem is set to the number of memory cells used by the
    // program stored in SubProgram.aOp.  As well as these, one memory cell
    // is required for each cursor used by the program.  Set local variable
    // nMem (and later, VdbeFrame.nChildMem) to this value.
    //
    // Rust: C allocates the frame in register P3 (pRt) and reuses it on
    // later invocations; here a fresh frame is built each time (see
    // VdbeFrame) and register P3 is left alone.
    let mut nMem = pProgram.nMem + pProgram.nCsr;
    if pProgram.nCsr == 0 {
        nMem += 1;
    }
    let mut pFrame = Box::new(VdbeFrame {
        nChildMem: nMem,
        nChildCsr: pProgram.nCsr,
        pc: pc,
        nMem: p.nMem,
        nCursor: p.nCursor,
        nOp: p.aOp.len() as i32,
        token: pProgram.token,
        ..Default::default()
    });
    pFrame.aOp = std::mem::replace(&mut p.aOp, pProgram.aOp.clone());
    pFrame.aMem = std::mem::replace(
        &mut p.aMem,
        (0..nMem.max(1)).map(|_| Mem::default()).collect(),
    );
    pFrame.apCsr = std::mem::replace(&mut p.apCsr, (0..pProgram.nCsr).map(|_| None).collect());

    p.nFrame += 1;
    pFrame.pParent = p.pFrame.take();
    pFrame.lastRowid = db.lastRowid;
    pFrame.nChange = p.nChange;
    pFrame.nDbChange = db.nChange;
    pFrame.pAuxData = std::mem::take(&mut p.pAuxData);
    p.nChange = 0;
    p.nMem = pFrame.nChildMem;
    p.nCursor = pFrame.nChildCsr;
    pFrame.aOnce = vec![0u8; (pProgram.aOp.len() + 7) / 8];
    p.pFrame = Some(pFrame);
    s.pc = -1;
    Ok(Fl::Check)
}

/// OP_AggStep, OP_AggInverse, OP_AggStep1
fn op_aggstep(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    opcode: u8,
    p1: i32,
    p2: i32,
    p3: i32,
    p5: u16,
) -> Result<Fl, Ex> {
    let encoding = s.encoding;
    if opcode != OP_AggStep1 {
        // Allocate the context object (C: sqlite3_context plus an
        // accumulator cell) and turn this opcode into OP_AggStep1.
        let n = p5;
        let pFunc = match &p.aOp[pc as usize].p4 {
            P4::Func(f) => f.clone(),
            P4::FuncCtx(f, _) => f.clone(),
            _ => panic!("OP_AggStep without function"),
        };
        let o = &mut p.aOp[pc as usize];
        o.p4type = P4_FUNCCTX;
        o.p4 = P4::FuncCtx(pFunc, n);
        // OP_AggInverse must have P1==1 and OP_AggStep must have P1==0
        debug_assert!(o.p1 == (opcode == OP_AggInverse) as i32);
        o.opcode = OP_AggStep1;
    }
    let (pFunc, argc) = match &p.aOp[pc as usize].p4 {
        P4::FuncCtx(f, n) => (f.clone(), *n as usize),
        _ => panic!("OP_AggStep1 without context"),
    };
    let xFunc = if p1 != 0 {
        pFunc.xInverse.clone()
    } else {
        pFunc.xSFunc.clone()
    };
    let xFunc = xFunc.expect("aggregate without step function");
    let pColl = func_coll(p, pc);

    p.aMem[p3 as usize].n += 1;
    let mut t = Mem::default();
    sqlite3VdbeMemInit(db, &mut t, MEM_Null);
    let isError;
    let skipFlag;
    {
        let Vdbe {
            aMem,
            pAuxData,
            iCurrentTime,
            ..
        } = &mut *p;
        match args_and_one(aMem, p2 as usize, argc, p3 as usize) {
            Some((args, acc)) => {
                let mut ctx = Context {
                    db: &mut *db,
                    pOut: &mut t,
                    pFunc: pFunc.clone(),
                    pMem: Some(acc),
                    pAuxData: Some(pAuxData),
                    iOp: pc,
                    isError: 0,
                    enc: encoding,
                    skipFlag: 0,
                    argc: argc as u16,
                    zErr: None,
                    pColl,
                    pCurrentTime: Some(iCurrentTime),
                    pOpInfo: Some((OP_AggStep1, p5)),
                };
                xFunc(&mut ctx, args);
                isError = ctx.isError;
                skipFlag = ctx.skipFlag;
            }
            None => {
                // The accumulator overlaps the arguments (never generated by
                // the code generator): pass copies of the arguments.
                let mut args = reg_copies(aMem, p2, argc as i32);
                let acc = &mut aMem[p3 as usize];
                let mut ctx = Context {
                    db: &mut *db,
                    pOut: &mut t,
                    pFunc: pFunc.clone(),
                    pMem: Some(acc),
                    pAuxData: Some(pAuxData),
                    iOp: pc,
                    isError: 0,
                    enc: encoding,
                    skipFlag: 0,
                    argc: argc as u16,
                    zErr: None,
                    pColl,
                    pCurrentTime: Some(iCurrentTime),
                    pOpInfo: Some((OP_AggStep1, p5)),
                };
                xFunc(&mut ctx, &mut args);
                isError = ctx.isError;
                skipFlag = ctx.skipFlag;
            }
        }
    }
    if isError != 0 {
        let mut rc = SQLITE_OK;
        if isError > 0 {
            let z = sqlite3_value_text(&mut t).map(|x| x.to_vec());
            verr!(db, p, b"%s", z.as_deref());
            rc = isError;
        }
        if skipFlag != 0 {
            debug_assert!(p.aOp[(pc - 1) as usize].opcode == OP_CollSeq);
            let i = p.aOp[(pc - 1) as usize].p1;
            if i != 0 {
                sqlite3VdbeMemSetInt64(&mut p.aMem[i as usize], 1);
            }
        }
        sqlite3VdbeMemRelease(&mut t);
        t.flags = MEM_Null;
        if rc != 0 {
            abort_rc!(s, rc);
        }
    }
    debug_assert!(t.flags == MEM_Null);
    Ok(Fl::Next)
}

/// OP_Function, OP_PureFunc
fn op_function(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    pc: i32,
    opcode: u8,
    p1: i32,
    p2: i32,
    p3: i32,
    p5: u16,
) -> Result<Fl, Ex> {
    let encoding = s.encoding;
    let (pFunc, argc) = match &p.aOp[pc as usize].p4 {
        P4::FuncCtx(f, n) => (f.clone(), *n as usize),
        P4::Func(f) => (f.clone(), f.nArg.max(0) as usize),
        _ => panic!("OP_Function without function"),
    };
    let xSFunc = pFunc.xSFunc.clone().expect("function without xSFunc");
    let pColl = func_coll(p, pc);

    // The output register may be one of the arguments: in that case the
    // function writes into a fresh cell which then replaces the register.
    let i3 = p3 as usize;
    let overlap = i3 >= p2 as usize && i3 < p2 as usize + argc;
    let mut out = if overlap {
        Mem::default()
    } else {
        std::mem::take(&mut p.aMem[i3])
    };
    out.MemSetTypeFlag(MEM_Null);
    let isError;
    {
        let Vdbe {
            aMem,
            pAuxData,
            iCurrentTime,
            ..
        } = &mut *p;
        let s0 = p2 as usize;
        let args = &mut aMem[s0..s0 + argc];
        let mut ctx = Context {
            db: &mut *db,
            pOut: &mut out,
            pFunc: pFunc.clone(),
            pMem: None,
            pAuxData: Some(pAuxData),
            iOp: pc,
            isError: 0,
            enc: encoding,
            skipFlag: 0,
            argc: argc as u16,
            zErr: None,
            pColl,
            pCurrentTime: Some(iCurrentTime),
            pOpInfo: Some((opcode, p5)),
        };
        xSFunc(&mut ctx, args);
        isError = ctx.isError;
    }
    p.aMem[i3] = out;

    // If the function returned an error, throw an exception
    if isError != 0 {
        let mut rc = SQLITE_OK;
        if isError > 0 {
            let z = sqlite3_value_text(&mut p.aMem[i3]).map(|x| x.to_vec());
            verr!(db, p, b"%s", z.as_deref());
            rc = isError;
        }
        sqlite3VdbeDeleteAuxData(db, &mut p.pAuxData, pc, p1);
        if rc != 0 {
            abort_rc!(s, rc);
        }
    }
    Ok(Fl::Next)
}

/// OP_JournalMode
fn op_journalmode(
    db: &mut Sqlite3,
    p: &mut Vdbe,
    s: &mut ExecState,
    _pc: i32,
    p1: i32,
    p2: i32,
    p3: i32,
) -> Result<Fl, Ex> {
    use crate::pager::*;
    let encoding = s.encoding;
    out2Prerelease(&mut p.aMem[p2 as usize]);
    let mut eNew = p3;
    debug_assert!(
        eNew == PAGER_JOURNALMODE_DELETE
            || eNew == PAGER_JOURNALMODE_TRUNCATE
            || eNew == PAGER_JOURNALMODE_PERSIST
            || eNew == PAGER_JOURNALMODE_OFF
            || eNew == PAGER_JOURNALMODE_MEMORY
            || eNew == PAGER_JOURNALMODE_WAL
            || eNew == PAGER_JOURNALMODE_QUERY
    );
    let pBt = db.aDb[p1 as usize].pBt.clone().expect("database not open");
    let pPager = sqlite3BtreePager(&pBt);
    let eOld = sqlite3PagerGetJournalMode(&mut pPager.borrow_mut());
    if eNew == PAGER_JOURNALMODE_QUERY {
        eNew = eOld;
    }
    if sqlite3PagerOkToChangeJournalMode(&mut pPager.borrow_mut()) == 0 {
        eNew = eOld;
    }

    let zFilename = sqlite3PagerFilename(&pPager.borrow(), 1);

    // Do not allow a transition to journal_mode=WAL for a database in
    // temporary storage or if the VFS does not support shared memory
    if eNew == PAGER_JOURNALMODE_WAL
        && (crate::util::sqlite3Strlen30(zFilename.as_deref()) == 0 // Temp file
            || sqlite3PagerWalSupported(&mut pPager.borrow_mut()) == 0)
    // No shared-memory support
    {
        eNew = eOld;
    }

    let mut rc = SQLITE_OK;
    if eNew != eOld && (eOld == PAGER_JOURNALMODE_WAL || eNew == PAGER_JOURNALMODE_WAL) {
        if db.autoCommit == 0 || db.nVdbeRead > 1 {
            let z: &[u8] = if eNew == PAGER_JOURNALMODE_WAL {
                b"into"
            } else {
                b"out of"
            };
            verr!(
                db,
                p,
                b"cannot change %s wal mode from within a transaction",
                z
            );
            abort_rc!(s, SQLITE_ERROR);
        } else {
            if eOld == PAGER_JOURNALMODE_WAL {
                // If leaving WAL mode, close the log file.  If successful,
                // the call to PagerCloseWal() checkpoints and deletes the
                // write-ahead-log file.  An EXCLUSIVE lock may still be held
                // on the database file after a successful return.
                rc = sqlite3PagerCloseWal(db, &mut pPager.borrow_mut());
                if rc == SQLITE_OK {
                    sqlite3PagerSetJournalMode(&mut pPager.borrow_mut(), eNew);
                }
            } else if eOld == PAGER_JOURNALMODE_MEMORY {
                // Cannot transition directly from MEMORY to WAL.  Use mode
                // OFF as an intermediate
                sqlite3PagerSetJournalMode(&mut pPager.borrow_mut(), PAGER_JOURNALMODE_OFF);
            }

            // Open a transaction on the database file.  Regardless of the
            // journal mode, this transaction always uses a rollback journal.
            debug_assert!(sqlite3BtreeTxnState(db, &pBt) != SQLITE_TXN_WRITE);
            if rc == SQLITE_OK {
                rc = sqlite3BtreeSetVersion(
                    db,
                    &pBt,
                    if eNew == PAGER_JOURNALMODE_WAL { 2 } else { 1 },
                );
            }
        }
    }

    if rc != 0 {
        eNew = eOld;
    }
    eNew = sqlite3PagerSetJournalMode(&mut pPager.borrow_mut(), eNew);

    let zMode = crate::pragma::sqlite3JournalModename(eNew).unwrap_or_default();
    let pOut = &mut p.aMem[p2 as usize];
    mem_set_static_str(pOut, &zMode, SQLITE_UTF8);
    sqlite3VdbeChangeEncoding(db, pOut, encoding as i32);
    chk_rc!(s, rc);
    Ok(Fl::Next)
}

/// The virtual-table opcodes (OP_VBegin, OP_VCreate, OP_VDestroy,
/// OP_VOpen, OP_VCheck, OP_VInitIn, OP_VFilter, OP_VColumn, OP_VNext,
/// OP_VRename, OP_VUpdate).  Virtual tables are not ported yet.
fn op_vtab_unsupported(db: &mut Sqlite3, p: &mut Vdbe, s: &mut ExecState) -> Result<Fl, Ex> {
    verr!(db, p, b"virtual tables are not supported");
    abort_rc!(s, SQLITE_ERROR);
}
