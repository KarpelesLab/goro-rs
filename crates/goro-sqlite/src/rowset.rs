//! Port of rowset.c: the RowSet object, a set of rowids used by
//! OP_RowSetAdd / OP_RowSetRead / OP_RowSetTest.
//!
//! The RowSet supports two modes of use exactly as in C: a sequence of
//! INSERTs followed by NEXTs (sorted extraction), or interleaved INSERTs
//! and TESTs in batches (a forest of balanced binary trees).
//!
//! Rust: the entries live in a vector (the C chunk allocator) and refer to
//! each other by index; index 0 is the NULL pointer (and doubles as the
//! scratch "head" entry of rowSetEntryMerge()).

use crate::int::*;

/// Index of a RowSetEntry in RowSet::a (0 = NULL).
type E = u32;
const NIL: E = 0;

#[derive(Clone, Copy, Default)]
struct RowSetEntry {
    /// ROWID value for this entry
    v: i64,
    /// Right subtree (larger entries) or list
    pRight: E,
    /// Left subtree (smaller entries)
    pLeft: E,
}

/// A RowSet in an instance of the following structure.
pub struct RowSet {
    /// All allocated entries (C pChunk / pFresh); a[0] is unused.
    a: Vec<RowSetEntry>,
    /// List of entries using pRight
    pEntry: E,
    /// Last entry on the pEntry list
    pLast: E,
    /// List of binary trees of entries
    pForest: E,
    /// Various flags
    rsFlags: u16,
    /// Current insert batch
    iBatch: i32,
}

const ROWSET_SORTED: u16 = 0x01;
const ROWSET_NEXT: u16 = 0x02;

impl RowSet {
    #[inline]
    fn e(&self, i: E) -> &RowSetEntry {
        &self.a[i as usize]
    }
    #[inline]
    fn em(&mut self, i: E) -> &mut RowSetEntry {
        &mut self.a[i as usize]
    }
}

/// C: `RowSet * sqlite3RowSetInit(sqlite3 *db)`
///
/// Allocate a RowSet object.
pub fn sqlite3RowSetInit(_db: &mut Sqlite3) -> Option<Box<crate::rowset::RowSet>> {
    Some(Box::new(rowSetNew()))
}

/// A new, empty RowSet (sqlite3RowSetInit() without a connection).
pub fn rowSetNew() -> RowSet {
    RowSet {
        a: vec![RowSetEntry::default()],
        pEntry: NIL,
        pLast: NIL,
        pForest: NIL,
        rsFlags: ROWSET_SORTED,
        iBatch: 0,
    }
}

/// C: `void sqlite3RowSetClear(void *pArg)`
///
/// Deallocate all chunks from a RowSet.  This frees all memory that the
/// RowSet has allocated over its lifetime.
///
/// Rust: takes the RowSet itself (was `(db, &mut [u8])`).
pub fn sqlite3RowSetClear(p: &mut RowSet) {
    p.a.truncate(1);
    p.a.shrink_to_fit();
    p.pEntry = NIL;
    p.pLast = NIL;
    p.pForest = NIL;
    p.rsFlags = ROWSET_SORTED;
}

/// C: `void sqlite3RowSetDelete(void *pArg)`
///
/// Rust: takes the RowSet itself (was `(db, &mut [u8])`); the object is
/// freed when dropped.
pub fn sqlite3RowSetDelete(p: &mut RowSet) {
    sqlite3RowSetClear(p);
}

/// Allocate a new RowSetEntry object that is associated with the given
/// RowSet.
fn rowSetEntryAlloc(p: &mut RowSet) -> E {
    p.a.push(RowSetEntry::default());
    (p.a.len() - 1) as E
}

/// C: `void sqlite3RowSetInsert(RowSet *p, i64 rowid)`
///
/// Insert a new value into a RowSet.
pub fn sqlite3RowSetInsert(p: &mut crate::rowset::RowSet, rowid: i64) {
    // This routine is never called after sqlite3RowSetNext()
    debug_assert!((p.rsFlags & ROWSET_NEXT) == 0);
    let pEntry = rowSetEntryAlloc(p);
    p.em(pEntry).v = rowid;
    p.em(pEntry).pRight = NIL;
    let pLast = p.pLast;
    if pLast != NIL {
        if rowid <= p.e(pLast).v {
            // Avoid unnecessary sorts by preserving the ROWSET_SORTED flags
            // where possible
            p.rsFlags &= !ROWSET_SORTED;
        }
        p.em(pLast).pRight = pEntry;
    } else {
        p.pEntry = pEntry;
    }
    p.pLast = pEntry;
}

/// Merge two lists of RowSetEntry objects.  Remove duplicates.  The input
/// lists are connected via pRight pointers and are assumed to be in sorted
/// order.
fn rowSetEntryMerge(p: &mut RowSet, mut pA: E, mut pB: E) -> E {
    // a[0] is the "head" entry.
    let head = NIL;
    let mut pTail = head;
    debug_assert!(pA != NIL && pB != NIL);
    loop {
        if p.e(pA).v <= p.e(pB).v {
            if p.e(pA).v < p.e(pB).v {
                p.em(pTail).pRight = pA;
                pTail = pA;
            }
            pA = p.e(pA).pRight;
            if pA == NIL {
                p.em(pTail).pRight = pB;
                break;
            }
        } else {
            p.em(pTail).pRight = pB;
            pTail = pB;
            pB = p.e(pB).pRight;
            if pB == NIL {
                p.em(pTail).pRight = pA;
                break;
            }
        }
    }
    let r = p.e(head).pRight;
    p.em(head).pRight = NIL;
    r
}

/// Sort all elements on the list of RowSetEntry objects into order of
/// increasing v.
fn rowSetEntrySort(p: &mut RowSet, mut pIn: E) -> E {
    let mut aBucket: [E; 40] = [NIL; 40];
    while pIn != NIL {
        let pNext = p.e(pIn).pRight;
        p.em(pIn).pRight = NIL;
        let mut i = 0usize;
        while aBucket[i] != NIL {
            pIn = rowSetEntryMerge(p, aBucket[i], pIn);
            aBucket[i] = NIL;
            i += 1;
        }
        aBucket[i] = pIn;
        pIn = pNext;
    }
    pIn = aBucket[0];
    for i in 1..aBucket.len() {
        if aBucket[i] == NIL {
            continue;
        }
        pIn = if pIn != NIL {
            rowSetEntryMerge(p, pIn, aBucket[i])
        } else {
            aBucket[i]
        };
    }
    pIn
}

/// The input, pIn, is a binary tree (or subtree) of RowSetEntry objects.
/// Convert this tree into a linked list connected by the pRight pointers
/// and return (first, last).
fn rowSetTreeToList(p: &mut RowSet, pIn: E) -> (E, E) {
    debug_assert!(pIn != NIL);
    let first;
    let last;
    let l = p.e(pIn).pLeft;
    if l != NIL {
        let (f, lst) = rowSetTreeToList(p, l);
        p.em(lst).pRight = pIn;
        first = f;
    } else {
        first = pIn;
    }
    let r = p.e(pIn).pRight;
    if r != NIL {
        let (f, lst) = rowSetTreeToList(p, r);
        p.em(pIn).pRight = f;
        last = lst;
    } else {
        last = pIn;
    }
    debug_assert!(p.e(last).pRight == NIL);
    (first, last)
}

/// Convert a sorted list of elements (connected by pRight) into a binary
/// tree with depth of iDepth.
fn rowSetNDeepTree(p: &mut RowSet, ppList: &mut E, iDepth: i32) -> E {
    if *ppList == NIL {
        // Prevent unnecessary deep recursion when we run out of entries
        return NIL;
    }
    let r;
    if iDepth > 1 {
        // This branch causes a *balanced* tree to be generated.
        let pLeft = rowSetNDeepTree(p, ppList, iDepth - 1);
        r = *ppList;
        if r == NIL {
            return pLeft;
        }
        p.em(r).pLeft = pLeft;
        *ppList = p.e(r).pRight;
        let right = rowSetNDeepTree(p, ppList, iDepth - 1);
        p.em(r).pRight = right;
    } else {
        r = *ppList;
        *ppList = p.e(r).pRight;
        p.em(r).pLeft = NIL;
        p.em(r).pRight = NIL;
    }
    r
}

/// Convert a sorted list of elements into a binary tree.  Make the tree as
/// deep as it needs to be in order to contain the entire list.
fn rowSetListToTree(p: &mut RowSet, pList: E) -> E {
    debug_assert!(pList != NIL);
    let mut r = pList;
    let mut pList = p.e(r).pRight;
    p.em(r).pLeft = NIL;
    p.em(r).pRight = NIL;
    let mut iDepth = 1;
    while pList != NIL {
        let pLeft = r;
        r = pList;
        pList = p.e(r).pRight;
        p.em(r).pLeft = pLeft;
        let right = rowSetNDeepTree(p, &mut pList, iDepth);
        p.em(r).pRight = right;
        iDepth += 1;
    }
    r
}

/// C: `int sqlite3RowSetNext(RowSet *p, i64 *pRowid)`
///
/// Extract the smallest element from the RowSet.  Write the element into
/// *pRowid.  Return 1 on success.  Return 0 if the RowSet is already
/// empty.
pub fn sqlite3RowSetNext(p: &mut crate::rowset::RowSet, pRowid: &mut i64) -> i32 {
    debug_assert!(p.pForest == NIL); // Cannot be used with sqlite3RowSetText()

    // Merge the forest into a single sorted list on first call
    if (p.rsFlags & ROWSET_NEXT) == 0 {
        if (p.rsFlags & ROWSET_SORTED) == 0 {
            let e = p.pEntry;
            p.pEntry = rowSetEntrySort(p, e);
        }
        p.rsFlags |= ROWSET_SORTED | ROWSET_NEXT;
    }

    // Return the next entry on the list
    if p.pEntry != NIL {
        *pRowid = p.e(p.pEntry).v;
        p.pEntry = p.e(p.pEntry).pRight;
        if p.pEntry == NIL {
            // Free memory immediately, rather than waiting on
            // sqlite3_finalize()
            sqlite3RowSetClear(p);
        }
        1
    } else {
        0
    }
}

/// C: `int sqlite3RowSetTest(RowSet *pRowSet, int iBatch, sqlite3_int64 iRowid)`
///
/// Check to see if element iRowid was inserted into the rowset as part of
/// any insert batch prior to iBatch.  Return 1 or 0.
pub fn sqlite3RowSetTest(pRowSet: &mut crate::rowset::RowSet, iBatch: i32, iRowid: i64) -> i32 {
    let rs = pRowSet;
    // This routine is never called after sqlite3RowSetNext()
    debug_assert!((rs.rsFlags & ROWSET_NEXT) == 0);

    // Sort entries into the forest on the first test of a new batch.
    if iBatch != rs.iBatch {
        let mut p = rs.pEntry;
        if p != NIL {
            // ppPrevTree: None = &pRowSet->pForest, Some(t) = &t->pRight
            let mut ppPrevTree: Option<E> = None;
            if (rs.rsFlags & ROWSET_SORTED) == 0 {
                // Only sort the current set of entries if they need it
                p = rowSetEntrySort(rs, p);
            }
            let mut pTree = rs.pForest;
            while pTree != NIL {
                ppPrevTree = Some(pTree);
                if rs.e(pTree).pLeft == NIL {
                    let t = rowSetListToTree(rs, p);
                    rs.em(pTree).pLeft = t;
                    break;
                } else {
                    let l = rs.e(pTree).pLeft;
                    let (pAux, _pTail) = rowSetTreeToList(rs, l);
                    rs.em(pTree).pLeft = NIL;
                    p = rowSetEntryMerge(rs, pAux, p);
                }
                pTree = rs.e(pTree).pRight;
            }
            if pTree == NIL {
                pTree = rowSetEntryAlloc(rs);
                match ppPrevTree {
                    None => rs.pForest = pTree,
                    Some(t) => rs.em(t).pRight = pTree,
                }
                rs.em(pTree).v = 0;
                rs.em(pTree).pRight = NIL;
                let t = rowSetListToTree(rs, p);
                rs.em(pTree).pLeft = t;
            }
            rs.pEntry = NIL;
            rs.pLast = NIL;
            rs.rsFlags |= ROWSET_SORTED;
        }
        rs.iBatch = iBatch;
    }

    // Test to see if the iRowid value appears anywhere in the forest.
    let mut pTree = rs.pForest;
    while pTree != NIL {
        let mut p = rs.e(pTree).pLeft;
        while p != NIL {
            let v = rs.e(p).v;
            if v < iRowid {
                p = rs.e(p).pRight;
            } else if v > iRowid {
                p = rs.e(p).pLeft;
            } else {
                return 1;
            }
        }
        pTree = rs.e(pTree).pRight;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_next_sorted_dedup() {
        let mut r = rowSetNew();
        for v in [5i64, 3, 9, 3, 1, 5, 7, -2] {
            sqlite3RowSetInsert(&mut r, v);
        }
        let mut out = Vec::new();
        let mut x = 0;
        while sqlite3RowSetNext(&mut r, &mut x) != 0 {
            out.push(x);
        }
        assert_eq!(out, vec![-2, 1, 3, 5, 7, 9]);
    }

    #[test]
    fn batches() {
        let mut r = rowSetNew();
        // batch 0: nothing tested yet
        assert_eq!(sqlite3RowSetTest(&mut r, 0, 1), 0);
        for v in 0..100i64 {
            sqlite3RowSetInsert(&mut r, v * 2);
        }
        assert_eq!(sqlite3RowSetTest(&mut r, 1, 10), 1);
        assert_eq!(sqlite3RowSetTest(&mut r, 1, 11), 0);
        for v in (0..50i64).rev() {
            sqlite3RowSetInsert(&mut r, v * 2 + 1);
        }
        // Not visible until the next batch
        assert_eq!(sqlite3RowSetTest(&mut r, 1, 11), 0);
        assert_eq!(sqlite3RowSetTest(&mut r, 2, 11), 1);
        for v in 0..200 {
            let expect = (v < 200 && v % 2 == 0) || (v % 2 == 1 && v < 100);
            assert_eq!(sqlite3RowSetTest(&mut r, 2, v), expect as i32, "{v}");
        }
    }
}
