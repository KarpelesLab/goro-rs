//! Port of vdbesort.c: the VdbeSorter object, used in concert with a
//! VdbeCursor to sort large numbers of keys for CREATE INDEX statements or
//! by SELECT statements with ORDER BY clauses that cannot be satisfied
//! using indexes and without LIMIT clauses.
//!
//! Interface (unchanged from C):
//!
//!    sqlite3VdbeSorterInit()       Create a new VdbeSorter object.
//!    sqlite3VdbeSorterWrite()      Add a single new row to the VdbeSorter.
//!    sqlite3VdbeSorterRewind()     Sort all content previously added.
//!    sqlite3VdbeSorterNext()       Advance the read cursor.
//!    sqlite3VdbeSorterRowkey()     Return the complete binary blob for the
//!                                  row currently under the read cursor.
//!    sqlite3VdbeSorterCompare()    Compare the current row with a blob.
//!    sqlite3VdbeSorterClose()      Close the VdbeSorter object.
//!    sqlite3VdbeSorterReset()      Refurbish the VdbeSorter for reuse.
//!
//! This port keeps every record in memory: the PMA spill to temporary files
//! and the worker threads of the C implementation are not implemented
//! (single-threaded C sorts the same records in the same order: records
//! that compare equal come out in insertion order, which is what the
//! in-memory merge sort below produces; it is the exact algorithm of C's
//! vdbeSorterSort(), including the order of the comparisons).  The record
//! comparison functions (generic, and the integer/text/real fast paths)
//! are ported as in C.

use crate::int::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Hard-coded maximum amount of data to accumulate in memory before
/// flushing to a level 0 PMA (kept for reference).
const SQLITE_MAX_PMASZ: i64 = 1 << 29;

const SORTER_TYPE_INTEGER: u8 = 0x01;
const SORTER_TYPE_TEXT: u8 = 0x02;
const SORTER_TYPE_REAL: u8 = 0x04;

/// A record being sorted (C SorterRecord: header + data).  Records form a
/// singly linked list through `pNext` (an index into VdbeSorter::aRec, -1
/// for the end of the list).
struct SorterRecord {
    data: Vec<u8>,
    pNext: i32,
}

/// Main sorter structure.  A single instance of this is allocated for each
/// sorter cursor created by the VDBE.
pub struct VdbeSorter {
    /// Minimum PMA size, in bytes (unused: no PMAs)
    pub mnPmaSize: i32,
    /// Maximum PMA size, in bytes (unused: no PMAs)
    pub mxPmaSize: i32,
    /// Largest serialized key seen so far
    pub mxKeysize: i32,
    /// Main database page size
    pub pgsz: i32,
    /// How to compare records (copy of the cursor's KeyInfo, possibly with
    /// a smaller nKeyField)
    pub pKeyInfo: KeyInfoRef,
    /// Used by sqlite3VdbeSorterCompare()
    pUnpacked: Option<Box<UnpackedRecord>>,
    /// Space to unpack a record during the sort (C aTask[0].pUnpacked)
    pTaskUnpacked: Option<Box<UnpackedRecord>>,
    /// Storage of the records of the in-memory list
    aRec: Vec<SorterRecord>,
    /// Head of the linked list of records (C list.pList), -1 if empty
    pList: i32,
    /// Size of pList as PMA in bytes (C list.szPMA)
    szPMA: i64,
    /// True if one or more PMAs created (always 0 here)
    pub bUsePMA: u8,
    /// Size of aTask[] array (main thread only)
    pub nTask: u8,
    pub typeMask: u8,
}

/// The comparison context of a sort (C SortSubtask fields used by the
/// comparison functions).
struct SortCtx<'a> {
    db: &'a mut Sqlite3,
    r2: &'a mut UnpackedRecord,
    nKeyField: u16,
    sortFlags0: u8,
}

type SorterCompare = fn(&mut SortCtx, &mut bool, &[u8], &[u8]) -> i32;

/// getVarint32NR(): read a 32-bit varint.
fn getVarint32(p: &[u8]) -> u32 {
    match p.first() {
        Some(&b) if b < 0x80 => b as u32,
        Some(_) => {
            let mut v = 0u32;
            crate::util::sqlite3GetVarint32(p, &mut v);
            v
        }
        None => 0,
    }
}

#[inline]
fn at(p: &[u8], i: usize) -> u8 {
    p.get(i).copied().unwrap_or(0)
}

/// A version of vdbeSorterCompare() that assumes that it has already been
/// determined that the first field of key1 is equal to the first field of
/// key2.
fn vdbeSorterCompareTail(
    t: &mut SortCtx,
    pbKey2Cached: &mut bool,
    pKey1: &[u8],
    pKey2: &[u8],
) -> i32 {
    if !*pbKey2Cached {
        crate::vdbeaux::sqlite3VdbeRecordUnpack(pKey2.len() as i32, pKey2, t.r2);
        *pbKey2Cached = true;
    }
    crate::vdbeaux::sqlite3VdbeRecordCompareWithSkip(t.db, pKey1.len() as i32, pKey1, t.r2, 1)
}

/// Compare key1 with key2.  Use the KeyInfo for the collation sequences
/// used by the comparison.  If *pbKey2Cached is true, the unpacked version
/// of key2 is already in t.r2.
fn vdbeSorterCompare(t: &mut SortCtx, pbKey2Cached: &mut bool, pKey1: &[u8], pKey2: &[u8]) -> i32 {
    if !*pbKey2Cached {
        crate::vdbeaux::sqlite3VdbeRecordUnpack(pKey2.len() as i32, pKey2, t.r2);
        *pbKey2Cached = true;
    }
    crate::vdbeaux::sqlite3VdbeRecordCompare(pKey1.len() as i32, pKey1, t.r2)
}

/// A specially optimized version of vdbeSorterCompare() that assumes that
/// the first field of each key is a TEXT value and that the collation
/// sequence to compare them with is BINARY.
fn vdbeSorterCompareText(t: &mut SortCtx, pbKey2Cached: &mut bool, p1: &[u8], p2: &[u8]) -> i32 {
    let v1 = p1.get(at(p1, 0) as usize..).unwrap_or(&[]); /* Pointer to value 1 */
    let v2 = p2.get(at(p2, 0) as usize..).unwrap_or(&[]); /* Pointer to value 2 */

    let n1 = getVarint32(p1.get(1..).unwrap_or(&[])) as i32;
    let n2 = getVarint32(p2.get(1..).unwrap_or(&[])) as i32;
    let len = ((n1.min(n2) - 13) / 2).max(0) as usize;
    let a = &v1[..len.min(v1.len())];
    let b = &v2[..len.min(v2.len())];
    let mut res = match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    if res == 0 {
        res = n1 - n2;
    }

    if res == 0 {
        if t.nKeyField > 1 {
            res = vdbeSorterCompareTail(t, pbKey2Cached, p1, p2);
        }
    } else {
        debug_assert!((t.sortFlags0 & KEYINFO_ORDER_BIGNULL) == 0);
        if t.sortFlags0 != 0 {
            res = -res;
        }
    }
    res
}

/// A specially optimized version of vdbeSorterCompare() that assumes that
/// the first field of each key is an INTEGER value.
fn vdbeSorterCompareInt(t: &mut SortCtx, pbKey2Cached: &mut bool, p1: &[u8], p2: &[u8]) -> i32 {
    let s1 = at(p1, 1) as i32; /* Left hand serial type */
    let s2 = at(p2, 1) as i32; /* Right hand serial type */
    let o1 = at(p1, 0) as usize;
    let o2 = at(p2, 0) as usize;
    let v1 = |i: usize| at(p1, o1 + i);
    let v2 = |i: usize| at(p2, o2 + i);
    let mut res: i32;

    debug_assert!((s1 > 0 && s1 < 7) || s1 == 8 || s1 == 9);
    debug_assert!((s2 > 0 && s2 < 7) || s2 == 8 || s2 == 9);

    if s1 == s2 {
        /* The two values have the same sign. Compare using memcmp(). */
        const aLen: [u8; 10] = [0, 1, 2, 3, 4, 6, 8, 0, 0, 0];
        let n = aLen[s1 as usize] as usize;
        res = 0;
        for i in 0..n {
            res = v1(i) as i32 - v2(i) as i32;
            if res != 0 {
                if ((v1(0) ^ v2(0)) & 0x80) != 0 {
                    res = if v1(0) & 0x80 != 0 { -1 } else { 1 };
                }
                break;
            }
        }
    } else if s1 > 7 && s2 > 7 {
        res = s1 - s2;
    } else {
        if s2 > 7 {
            res = 1;
        } else if s1 > 7 {
            res = -1;
        } else {
            res = s1 - s2;
        }
        debug_assert!(res != 0);

        if res > 0 {
            if v1(0) & 0x80 != 0 {
                res = -1;
            }
        } else if v2(0) & 0x80 != 0 {
            res = 1;
        }
    }

    if res == 0 {
        if t.nKeyField > 1 {
            res = vdbeSorterCompareTail(t, pbKey2Cached, p1, p2);
        }
    } else if t.sortFlags0 != 0 {
        debug_assert!((t.sortFlags0 & KEYINFO_ORDER_BIGNULL) == 0);
        res = -res;
    }
    res
}

/// Helper function for vdbeSorterCompareReal(): the first elements of both
/// keys have been decoded into r1 and r2.  If r1==r2, break the tie with a
/// comparison of subsequent elements from each key.
fn vdbeSorterFinishRealCompare(
    t: &mut SortCtx,
    pbKey2Cached: &mut bool,
    pKey1: &[u8],
    pKey2: &[u8],
    r1: f64,
    r2: f64,
) -> i32 {
    let mut res = if r1 < r2 {
        -1
    } else if r1 > r2 {
        1
    } else {
        0
    };
    if res == 0 {
        if t.nKeyField > 1 {
            res = vdbeSorterCompareTail(t, pbKey2Cached, pKey1, pKey2);
        }
    } else if t.sortFlags0 != 0 {
        debug_assert!((t.sortFlags0 & KEYINFO_ORDER_BIGNULL) == 0);
        res = -res;
    }
    res
}

/// Read the 8-byte big-endian value at p[off..].
fn get8byte(p: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    for (i, x) in b.iter_mut().enumerate() {
        *x = at(p, off + i);
    }
    u64::from_be_bytes(b)
}

/// Helper function for vdbeSorterCompareReal(): buffer p[] is a record
/// where the first term is either a floating-point value, or an integer
/// stand-in for a floating point value.  Extract the value.
fn vdbeSorterGetReal(p: &[u8]) -> f64 {
    debug_assert!(at(p, 0) < 0x80);
    debug_assert!(at(p, 1) > 0 && at(p, 1) < 10);
    if at(p, 1) == 7 {
        f64::from_bits(get8byte(p, at(p, 0) as usize))
    } else {
        let mut m = Mem::default();
        m.u.i = 0;
        crate::vdbeaux::sqlite3VdbeSerialGet(
            p.get(at(p, 0) as usize..).unwrap_or(&[]),
            at(p, 1) as u32,
            &mut m,
        );
        debug_assert!(m.flags == MEM_Int);
        m.u.i as f64
    }
}

/// Comparing two floating-point values where one or both are represented
/// by integers.
fn vdbeSorterCompareRealInt(t: &mut SortCtx, pbKey2Cached: &mut bool, p1: &[u8], p2: &[u8]) -> i32 {
    if at(p1, 1) == 6 || at(p2, 1) == 6 {
        /* 64-bit integer values cannot be represented exactly by a double so
         ** must be handled by the generalized comparison function. */
        vdbeSorterCompare(t, pbKey2Cached, p1, p2)
    } else {
        let r1 = vdbeSorterGetReal(p1);
        let r2 = vdbeSorterGetReal(p2);
        vdbeSorterFinishRealCompare(t, pbKey2Cached, p1, p2, r1, r2)
    }
}

/// Comparison function optimized for the case where the first term of both
/// keys are either MEM_Real or MEM_RealInt.
fn vdbeSorterCompareReal(t: &mut SortCtx, pbKey2Cached: &mut bool, p1: &[u8], p2: &[u8]) -> i32 {
    debug_assert!(at(p1, 0) < 0x80 && at(p2, 0) < 0x80);
    if at(p1, 1) != 7 || at(p2, 1) != 7 {
        /* One or both floating point values are stored as INTEGER.  This
         ** might be because of the MEM_RealInt encoding. */
        return vdbeSorterCompareRealInt(t, pbKey2Cached, p1, p2);
    }
    let r1 = f64::from_bits(get8byte(p1, at(p1, 0) as usize));
    let r2 = f64::from_bits(get8byte(p2, at(p2, 0) as usize));
    vdbeSorterFinishRealCompare(t, pbKey2Cached, p1, p2, r1, r2)
}

/// C: `int sqlite3VdbeSorterInit( sqlite3 *db, int nField, VdbeCursor *pCsr )`
///
/// Initialize the temporary index cursor just opened as a sorter cursor.
///
/// Usually, the sorter module uses the value of (pCsr->pKeyInfo->nKeyField)
/// to determine the number of fields that should be compared from the
/// records being sorted.  However, if the value passed as argument nField
/// is non-zero and the sorter is able to guarantee a stable sort (always
/// the case here: no worker threads), nField is used instead.
pub fn sqlite3VdbeSorterInit(db: &mut Sqlite3, nField: i32, pCsr: &mut VdbeCursor) -> i32 {
    let nWorker = 0; /* no worker threads in this port */
    debug_assert!(pCsr.pKeyInfo.is_some());
    debug_assert!(pCsr.isEphemeral == 0);
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let src = match &pCsr.pKeyInfo {
        Some(k) => k.clone(),
        None => return SQLITE_ERROR,
    };
    let mut keyInfo = src.borrow().clone();
    debug_assert!(keyInfo.nKeyField <= keyInfo.nAllField);
    if nField != 0 && nWorker == 0 {
        keyInfo.nKeyField = nField as u16;
        debug_assert!(nField <= keyInfo.nAllField as i32);
    }

    let pgsz = match db.aDb.first().and_then(|d| d.pBt.clone()) {
        Some(pBt) => crate::btree::sqlite3BtreeGetPageSize(&pBt),
        None => 4096,
    };

    let mut typeMask = 0u8;
    let bDflt = match (keyInfo.aColl.first(), &db.pDfltColl) {
        (None, _) | (Some(None), _) => true,
        (Some(Some(c)), Some(d)) => Rc::ptr_eq(c, d),
        (Some(Some(_)), None) => false,
    };
    if keyInfo.nAllField < 13
        && bDflt
        && (keyInfo.aSortFlags.first().copied().unwrap_or(0) & KEYINFO_ORDER_BIGNULL) == 0
    {
        typeMask = SORTER_TYPE_INTEGER | SORTER_TYPE_TEXT | SORTER_TYPE_REAL;
    }

    /* The C code sizes the PMAs from the cache size; kept for reference. */
    let mut mxCache = db
        .aDb
        .first()
        .map(|d| d.pSchema.cache_size as i64)
        .unwrap_or(0);
    if mxCache < 0 {
        mxCache *= -1024;
    } else {
        mxCache *= pgsz as i64;
    }
    mxCache = mxCache.min(SQLITE_MAX_PMASZ);

    pCsr.pSorter = Some(Box::new(VdbeSorter {
        mnPmaSize: 0,
        mxPmaSize: mxCache as i32,
        mxKeysize: 0,
        pgsz,
        pKeyInfo: Rc::new(RefCell::new(keyInfo)),
        pUnpacked: None,
        pTaskUnpacked: None,
        aRec: Vec::new(),
        pList: -1,
        szPMA: 0,
        bUsePMA: 0,
        nTask: (nWorker + 1) as u8,
        typeMask,
    }));
    SQLITE_OK
}

/// C: `void sqlite3VdbeSorterReset(sqlite3 *db, VdbeSorter *pSorter)`
///
/// Reset a sorting cursor back to its original empty state.
pub fn sqlite3VdbeSorterReset(db: &mut Sqlite3, pSorter: &mut crate::vdbesort::VdbeSorter) {
    let _ = db;
    pSorter.pTaskUnpacked = None;
    pSorter.aRec = Vec::new();
    pSorter.pList = -1;
    pSorter.szPMA = 0;
    pSorter.bUsePMA = 0;
    pSorter.mxKeysize = 0;
    pSorter.pUnpacked = None;
}

/// C: `void sqlite3VdbeSorterClose(sqlite3 *db, VdbeCursor *pCsr)`
///
/// Free any cursor components allocated by sqlite3VdbeSorterXXX routines.
pub fn sqlite3VdbeSorterClose(db: &mut Sqlite3, pCsr: &mut VdbeCursor) {
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    if let Some(mut pSorter) = pCsr.pSorter.take() {
        /* Increment db->nSpill by the total number of bytes of data written
         ** to temp files by this sort operation (none here).  */
        db.nSpill += 0;
        sqlite3VdbeSorterReset(db, &mut pSorter);
    }
}

/// Merge the two sorted lists p1 and p2 into a single list.
fn vdbeSorterMerge(
    t: &mut SortCtx,
    xCompare: SorterCompare,
    aRec: &mut [SorterRecord],
    mut p1: i32,
    mut p2: i32,
) -> i32 {
    let mut pFinal: i32 = -1;
    let mut tail: i32 = -1; /* C: pp */
    let mut bCached = false;

    debug_assert!(p1 >= 0 && p2 >= 0);
    let mut link = |aRec: &mut [SorterRecord], tail: &mut i32, x: i32| {
        if *tail < 0 {
            pFinal = x;
        } else {
            aRec[*tail as usize].pNext = x;
        }
        *tail = x;
    };
    loop {
        let res = xCompare(
            t,
            &mut bCached,
            &aRec[p1 as usize].data,
            &aRec[p2 as usize].data,
        );

        if res <= 0 {
            link(aRec, &mut tail, p1);
            p1 = aRec[p1 as usize].pNext;
            if p1 < 0 {
                aRec[tail as usize].pNext = p2;
                break;
            }
        } else {
            link(aRec, &mut tail, p2);
            p2 = aRec[p2 as usize].pNext;
            bCached = false;
            if p2 < 0 {
                aRec[tail as usize].pNext = p1;
                break;
            }
        }
    }
    pFinal
}

/// Return the SorterCompare function to compare values collected by the
/// sorter object passed as the only argument.
fn vdbeSorterGetCompare(p: &VdbeSorter) -> SorterCompare {
    if p.typeMask & SORTER_TYPE_INTEGER != 0 {
        vdbeSorterCompareInt
    } else if p.typeMask & SORTER_TYPE_TEXT != 0 {
        vdbeSorterCompareText
    } else if p.typeMask & SORTER_TYPE_REAL != 0 {
        vdbeSorterCompareReal
    } else {
        vdbeSorterCompare
    }
}

/// Sort the linked list of records headed at pSorter->pList.  Return
/// SQLITE_OK if successful, or an SQLite error code otherwise.
fn vdbeSorterSort(db: &mut Sqlite3, pSorter: &mut VdbeSorter) -> i32 {
    /* vdbeSortAllocUnpacked() */
    if pSorter.pTaskUnpacked.is_none() {
        let k = pSorter.pKeyInfo.clone();
        let mut r = match crate::vdbeaux::sqlite3VdbeAllocUnpackedRecord(db, &k) {
            Some(r) => r,
            None => return SQLITE_NOMEM,
        };
        r.nField = k.borrow().nKeyField;
        r.errCode = 0;
        pSorter.pTaskUnpacked = Some(r);
    }
    let mut r2 = pSorter.pTaskUnpacked.take().unwrap();
    let xCompare = vdbeSorterGetCompare(pSorter);
    let (nKeyField, sortFlags0) = {
        let k = pSorter.pKeyInfo.borrow();
        (k.nKeyField, k.aSortFlags.first().copied().unwrap_or(0))
    };
    let rc;
    {
        let mut t = SortCtx {
            db,
            r2: &mut r2,
            nKeyField,
            sortFlags0,
        };
        let aRec = &mut pSorter.aRec[..];
        let mut aSlot: [i32; 64] = [-1; 64];

        let mut p = pSorter.pList;
        while p >= 0 {
            let pNext = aRec[p as usize].pNext;
            aRec[p as usize].pNext = -1;
            let mut i = 0usize;
            while aSlot[i] >= 0 {
                p = vdbeSorterMerge(&mut t, xCompare, aRec, p, aSlot[i]);
                aSlot[i] = -1;
                i += 1;
            }
            aSlot[i] = p;
            p = pNext;
        }

        p = -1;
        for i in 0..64 {
            if aSlot[i] < 0 {
                continue;
            }
            p = if p >= 0 {
                vdbeSorterMerge(&mut t, xCompare, aRec, p, aSlot[i])
            } else {
                aSlot[i]
            };
        }
        pSorter.pList = p;
        rc = t.r2.errCode as i32;
    }
    pSorter.pTaskUnpacked = Some(r2);
    rc
}

/// C: `int sqlite3VdbeSorterWrite( const VdbeCursor *pCsr, Mem *pVal )`
///
/// Add a record to the sorter.  Rust: takes `&mut VdbeCursor` (the C
/// cursor is const but the sorter it points to is modified).
pub fn sqlite3VdbeSorterWrite(pCsr: &mut VdbeCursor, pVal: &mut Mem) -> i32 {
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let pSorter = match pCsr.pSorter.as_deref_mut() {
        Some(s) => s,
        None => return SQLITE_ERROR,
    };
    let z = pVal.bytes();
    let t = getVarint32(z.get(1..).unwrap_or(&[]));
    if t > 0 && t < 10 {
        if t == 7 {
            pSorter.typeMask &= SORTER_TYPE_REAL;
        } else {
            pSorter.typeMask &= SORTER_TYPE_INTEGER | SORTER_TYPE_REAL;
        }
    } else if t > 10 && (t & 0x01) != 0 {
        pSorter.typeMask &= SORTER_TYPE_TEXT;
    } else {
        pSorter.typeMask = 0;
    }

    /* (No flush to a PMA: every record is kept in memory.) */
    let n = z.len() as i64;
    let nPMA = n + crate::util::sqlite3VarintLen(n as u64) as i64;
    pSorter.szPMA += nPMA;
    if nPMA > pSorter.mxKeysize as i64 {
        pSorter.mxKeysize = nPMA as i32;
    }

    let idx = pSorter.aRec.len() as i32;
    pSorter.aRec.push(SorterRecord {
        data: z.to_vec(),
        pNext: pSorter.pList,
    });
    pSorter.pList = idx;
    SQLITE_OK
}

/// C: `int sqlite3VdbeSorterRewind(const VdbeCursor *pCsr, int *pbEof)`
///
/// Once the sorter has been populated by calls to sqlite3VdbeSorterWrite,
/// this function is called to prepare for iterating through the records in
/// sorted order.  Rust: takes `db` (record comparison) and
/// `&mut VdbeCursor`.
pub fn sqlite3VdbeSorterRewind(db: &mut Sqlite3, pCsr: &mut VdbeCursor, pbEof: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let pSorter = match pCsr.pSorter.as_deref_mut() {
        Some(s) => s,
        None => return SQLITE_ERROR,
    };

    /* If no data has been written to disk, then do not do so now. Instead,
     ** sort the VdbeSorter.pRecord list. The vdbe layer will read data
     ** directly from the in-memory list.  */
    debug_assert!(pSorter.bUsePMA == 0);
    if pSorter.pList >= 0 {
        *pbEof = 0;
        rc = vdbeSorterSort(db, pSorter);
    } else {
        *pbEof = 1;
    }
    rc
}

/// C: `int sqlite3VdbeSorterNext(sqlite3 *db, const VdbeCursor *pCsr)`
///
/// Advance to the next element in the sorter.  Return value:
///
///    SQLITE_OK     success
///    SQLITE_DONE   end of data
///    otherwise     some kind of error.
pub fn sqlite3VdbeSorterNext(db: &mut Sqlite3, pCsr: &mut VdbeCursor) -> i32 {
    let _ = db;
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let pSorter = match pCsr.pSorter.as_deref_mut() {
        Some(s) => s,
        None => return SQLITE_ERROR,
    };
    let pFree = pSorter.pList;
    if pFree < 0 {
        return SQLITE_DONE;
    }
    let r = &mut pSorter.aRec[pFree as usize];
    pSorter.pList = r.pNext;
    r.pNext = -1;
    r.data = Vec::new(); /* vdbeSorterRecordFree() */
    if pSorter.pList >= 0 {
        SQLITE_OK
    } else {
        SQLITE_DONE
    }
}

/// Return the current key (a buffer owned by the sorter).
fn vdbeSorterRowkey(pSorter: &VdbeSorter) -> &[u8] {
    if pSorter.pList < 0 {
        return &[];
    }
    &pSorter.aRec[pSorter.pList as usize].data
}

/// C: `int sqlite3VdbeSorterRowkey(const VdbeCursor *pCsr, Mem *pOut)`
///
/// Copy the current sorter key into the memory cell pOut.
pub fn sqlite3VdbeSorterRowkey(pCsr: &VdbeCursor, pOut: &mut Mem) -> i32 {
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let pSorter = match pCsr.pSorter.as_deref() {
        Some(s) => s,
        None => return SQLITE_ERROR,
    };
    let pKey = vdbeSorterRowkey(pSorter);
    let nKey = pKey.len();
    if crate::vdbemem::sqlite3VdbeMemClearAndResize(pOut, nKey as i32) != 0 {
        return SQLITE_NOMEM;
    }
    pOut.n = nKey as i32;
    pOut.MemSetTypeFlag(MEM_Blob);
    pOut.z[..nKey].copy_from_slice(pKey);
    SQLITE_OK
}

/// C: `int sqlite3VdbeSorterCompare( const VdbeCursor *pCsr, Mem *pVal, int nKeyCol, int *pRes )`
///
/// Compare the key in memory cell pVal with the key that the sorter cursor
/// passed as the first argument currently points to.  For the purposes of
/// the comparison, ignore the rowid field at the end of each record.
///
/// If the sorter cursor key contains any NULL values, consider it to be
/// less than pVal.  Even if pVal also contains NULL values.
///
/// This routine forms the core of the OP_SorterCompare opcode, which in
/// turn is used to verify uniqueness when constructing a UNIQUE INDEX.
/// Rust: takes `db` and `&mut VdbeCursor`.
pub fn sqlite3VdbeSorterCompare(
    db: &mut Sqlite3,
    pCsr: &mut VdbeCursor,
    pVal: &mut Mem,
    nKeyCol: i32,
    pRes: &mut i32,
) -> i32 {
    debug_assert!(pCsr.eCurType == CURTYPE_SORTER);
    let pKeyInfo = match &pCsr.pKeyInfo {
        Some(k) => k.clone(),
        None => return SQLITE_ERROR,
    };
    let pSorter = match pCsr.pSorter.as_deref_mut() {
        Some(s) => s,
        None => return SQLITE_ERROR,
    };
    if pSorter.pUnpacked.is_none() {
        let mut r = match crate::vdbeaux::sqlite3VdbeAllocUnpackedRecord(db, &pKeyInfo) {
            Some(r) => r,
            None => return SQLITE_NOMEM,
        };
        r.nField = nKeyCol as u16;
        pSorter.pUnpacked = Some(r);
    }
    let mut r2 = pSorter.pUnpacked.take().unwrap();
    debug_assert!(r2.nField as i32 == nKeyCol);

    let pKey = vdbeSorterRowkey(pSorter);
    crate::vdbeaux::sqlite3VdbeRecordUnpack(pKey.len() as i32, pKey, &mut r2);
    let mut hasNull = false;
    for i in 0..nKeyCol as usize {
        if r2
            .aMem
            .get(i)
            .map(|m| m.flags & MEM_Null != 0)
            .unwrap_or(false)
        {
            hasNull = true;
            break;
        }
    }
    if hasNull {
        *pRes = -1;
    } else {
        let z = pVal.bytes();
        *pRes = crate::vdbeaux::sqlite3VdbeRecordCompare(z.len() as i32, z, &mut r2);
    }
    pSorter.pUnpacked = Some(r2);
    SQLITE_OK
}
