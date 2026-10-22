//! Port of pcache.c: the page cache layer used by the pager on top of
//! pcache1.
//!
//! The C dirty list is a doubly-linked list threaded through the PgHdr
//! objects (pDirty = head = most recently made dirty, pDirtyTail = least
//! recently).  Here it is an ordered map keyed by a sequence number
//! (`PgHdr::dirtySeq`): the largest key is the head, the smallest the
//! tail; C `pDirtyPrev` (towards the head) is the next larger key and
//! `pDirtyNext` the next smaller one.  Ordering and the spill/LRU
//! behaviour are those of the C code.
//!
//! Functions that in C reach the cache through `PgHdr::pCache` take the
//! `PCache` as an extra leading parameter.

use crate::int::*;
use crate::pager::Pager;
use crate::pcache1::*;
use std::collections::BTreeMap;

/// The xStress callback: try to make a dirty page clean (C
/// `int (*xStress)(void*,PgHdr*)`, the pager is the void* argument).
pub type XStress = fn(&mut Pager, &DbPage) -> i32;

/// A complete page cache is an instance of this structure (C PCache).
#[derive(Default)]
pub struct PCache {
    /// List of dirty pages in LRU order, keyed by PgHdr::dirtySeq
    dirty: BTreeMap<u64, DbPage>,
    nDirtySeq: u64,
    /// Last synced page in dirty page list (its dirtySeq)
    pSynced: Option<u64>,
    /// Sum of ref counts over all pages
    pub nRefSum: i64,
    /// Configured cache size
    szCache: i32,
    /// Size before spilling occurs
    szSpill: i32,
    /// Size of every page in this cache
    szPage: i32,
    /// Size of extra space for each page
    szExtra: i32,
    /// True if pages are on backing store
    bPurgeable: u8,
    /// eCreate value for for xFetch()
    eCreate: u8,
    /// Call to try make a page clean
    xStress: Option<XStress>,
    /// Pluggable cache module
    pCache: Option<PCache1>,
}

// Allowed values for second argument to pcacheManageDirtyList()
const PCACHE_DIRTYLIST_REMOVE: u8 = 1;
const PCACHE_DIRTYLIST_ADD: u8 = 2;
const PCACHE_DIRTYLIST_FRONT: u8 = 3;

/// C: `int sqlite3PcachePageSanity(PgHdr *pPg)` (SQLITE_DEBUG only)
pub fn sqlite3PcachePageSanity(pPg: &DbPage) -> i32 {
    let p = pPg.borrow();
    if (p.flags as i32 & PGHDR_CLEAN) != 0 {
        debug_assert!((p.flags as i32 & PGHDR_DIRTY) == 0);
        debug_assert!(p.dirtySeq == 0);
    } else {
        debug_assert!((p.flags as i32 & PGHDR_DIRTY) != 0);
    }
    if (p.flags as i32 & PGHDR_WRITEABLE) != 0 {
        debug_assert!((p.flags as i32 & PGHDR_DIRTY) != 0);
    }
    1
}

/// Manage pPage's participation on the dirty list.  Bits of the
/// addRemove argument determines what operation to do.
fn pcacheManageDirtyList(p: &mut PCache, pPage: &DbPage, addRemove: u8) {
    if (addRemove & PCACHE_DIRTYLIST_REMOVE) != 0 {
        let seq = pPage.borrow().dirtySeq;
        debug_assert!(seq != 0 && p.dirty.contains_key(&seq));
        // Update the PCache1.pSynced variable if necessary.
        if p.pSynced == Some(seq) {
            p.pSynced = p.dirty.range(seq + 1..).next().map(|(k, _)| *k);
        }
        p.dirty.remove(&seq);
        pPage.borrow_mut().dirtySeq = 0;
        if p.dirty.is_empty() {
            // If there are now no dirty pages in the cache, set eCreate to
            // 2.  This is an optimization that allows sqlite3PcacheFetch()
            // to skip searching for a dirty page to eject from the cache
            // when it might otherwise have to.
            debug_assert!(p.bPurgeable == 0 || p.eCreate == 1);
            p.eCreate = 2;
        }
    }
    if (addRemove & PCACHE_DIRTYLIST_ADD) != 0 {
        if p.dirty.is_empty() && p.bPurgeable != 0 {
            debug_assert!(p.eCreate == 2);
            p.eCreate = 1;
        }
        p.nDirtySeq += 1;
        let seq = p.nDirtySeq;
        let needSync = {
            let mut pg = pPage.borrow_mut();
            pg.dirtySeq = seq;
            (pg.flags as i32 & PGHDR_NEED_SYNC) != 0
        };
        p.dirty.insert(seq, pPage.clone());

        // If pSynced is NULL and this page has a clear NEED_SYNC flag, set
        // pSynced to point to it. Checking the NEED_SYNC flag is an
        // optimization, as if pSynced points to a page with the NEED_SYNC
        // flag set sqlite3PcacheFetchStress() searches through all newer
        // entries of the dirty-list for a page with NEED_SYNC clear anyway.
        if p.pSynced.is_none() && !needSync {
            p.pSynced = Some(seq);
        }
    }
}

/// Wrapper around the pluggable caches xUnpin method. If the cache is
/// being used for an in-memory database, this function is a no-op.
fn pcacheUnpin(pCache: &mut PCache, p: &DbPage) {
    if pCache.bPurgeable != 0 {
        pcache1Unpin(pCache.pCache.as_mut().unwrap(), p, 0);
    }
}

/// Compute the number of pages of cache requested.   p->szCache is the
/// cache size requested by the "PRAGMA cache_size" statement.
fn numberOfCachePages(p: &PCache) -> i32 {
    if p.szCache >= 0 {
        // IMPLEMENTATION-OF: R-42059-47211 If the argument N is positive
        // then the suggested cache size is set to N.
        p.szCache
    } else {
        // IMPLEMENTATION-OF: R-59858-46238 If the argument N is negative,
        // then the number of cache pages is adjusted to be a number of
        // pages that would use approximately abs(N*1024) bytes of memory
        // based on the current page size.
        let mut n = (-1024 * p.szCache as i64) / ((p.szPage + p.szExtra) as i64);
        if n > 1000000000 {
            n = 1000000000;
        }
        n as i32
    }
}

/// C: `int sqlite3PcacheInitialize(void)`
pub fn sqlite3PcacheInitialize() -> i32 {
    sqlite3PCacheSetDefault();
    SQLITE_OK
}

/// C: `void sqlite3PcacheShutdown(void)`
pub fn sqlite3PcacheShutdown() {}

/// C: `int sqlite3PcacheSize(void)` (the 64-bit C value)
pub fn sqlite3PcacheSize() -> i32 {
    80
}

/// C: `int sqlite3PcacheOpen( int szPage, int szExtra, int bPurgeable, int (*xStress)(void*,PgHdr*), void *pStress, PCache *p )`
///
/// Create a new PCache object.  Signature change: `xStress` is an
/// `Option<XStress>` (the pager is passed to it), no pStress.
pub fn sqlite3PcacheOpen(
    szPage: i32,
    szExtra: i32,
    bPurgeable: i32,
    xStress: Option<XStress>,
    p: &mut crate::pcache::PCache,
) -> i32 {
    *p = PCache::default();
    p.szPage = 1;
    p.szExtra = szExtra;
    debug_assert!(szExtra >= 8); // First 8 bytes will be zeroed
    p.bPurgeable = bPurgeable as u8;
    p.eCreate = 2;
    p.xStress = xStress;
    p.szCache = 100;
    p.szSpill = 1;
    sqlite3PcacheSetPageSize(p, szPage)
}

/// C: `int sqlite3PcacheSetPageSize(PCache *pCache, int szPage)`
///
/// Change the page size for PCache object. The caller must ensure that
/// there are no outstanding page references when this function is called.
pub fn sqlite3PcacheSetPageSize(pCache: &mut crate::pcache::PCache, szPage: i32) -> i32 {
    debug_assert!(pCache.nRefSum == 0 && pCache.dirty.is_empty());
    if pCache.szPage != 0 {
        let mut pNew = pcache1Create(
            szPage,
            pCache.szExtra + crate::pcache::sqlite3HeaderSizePcache(),
            pCache.bPurgeable as i32,
        );
        pcache1Cachesize(&mut pNew, numberOfCachePages(pCache));
        if let Some(old) = pCache.pCache.take() {
            pcache1Destroy(old);
        }
        pCache.pCache = Some(pNew);
        pCache.szPage = szPage;
    }
    SQLITE_OK
}

/// C: `sqlite3_pcache_page * sqlite3PcacheFetch( PCache *pCache, Pgno pgno, int createFlag )`
///
/// Try to obtain a page from the cache.  Returns the raw pcache1 page
/// (to be passed to sqlite3PcacheFetchFinish()).
pub fn sqlite3PcacheFetch(
    pCache: &mut crate::pcache::PCache,
    pgno: Pgno,
    createFlag: i32,
) -> Option<DbPage> {
    debug_assert!(createFlag == 3 || createFlag == 0);
    debug_assert!(
        pCache.eCreate
            == if pCache.bPurgeable != 0 && !pCache.dirty.is_empty() {
                1
            } else {
                2
            }
    );
    // eCreate defines what to do if the page does not exist.
    //    0     Do not allocate a new page.  (createFlag==0)
    //    1     Allocate a new page if doing so is inexpensive.
    //          (createFlag==1 AND bPurgeable AND pDirty)
    //    2     Allocate a new page even it doing so is difficult.
    //          (createFlag==1 AND !(bPurgeable AND pDirty)
    let eCreate = createFlag & pCache.eCreate as i32;
    pcache1Fetch(pCache.pCache.as_mut().unwrap(), pgno, eCreate)
}

/// C: `int sqlite3PcacheFetchStress( PCache *pCache, Pgno pgno, sqlite3_pcache_page **ppPage )`
///
/// If the sqlite3PcacheFetch() routine is unable to allocate a new page
/// because no clean pages are available for reuse and the cache size
/// limit has been reached, then this routine can be invoked to try harder
/// to allocate a page.  This routine might invoke the stress callback to
/// spill dirty pages to the journal.  It will then try to allocate the
/// new page and will only fail to allocate a new page on an OOM error.
///
/// Signature change: takes the Pager (owner of the PCache, passed to the
/// xStress callback).
pub fn sqlite3PcacheFetchStress(
    pPager: &mut Pager,
    pgno: Pgno,
    ppPage: &mut Option<DbPage>,
) -> i32 {
    if pPager.pPCache.eCreate == 2 {
        return 0;
    }
    if sqlite3PcachePagecount(&mut pPager.pPCache) > pPager.pPCache.szSpill {
        // Find a dirty page to write-out and recycle. First try to find a
        // page that does not require a journal-sync (one with
        // PGHDR_NEED_SYNC cleared), but if that is not possible settle for
        // any other unreferenced dirty page.
        //
        // If the LRU page in the dirty list that has a clear
        // PGHDR_NEED_SYNC flag is currently referenced, then the following
        // may leave pSynced set incorrectly (pointing to other than the LRU
        // page with NEED_SYNC cleared). This is Ok, as pSynced is just an
        // optimization.
        let pCache = &mut pPager.pPCache;
        let mut pPg: Option<DbPage> = None;
        if let Some(s) = pCache.pSynced {
            for (_, p) in pCache.dirty.range(s..) {
                let b = p.borrow();
                if !(b.nRef != 0 || (b.flags as i32 & PGHDR_NEED_SYNC) != 0) {
                    pPg = Some(p.clone());
                    break;
                }
            }
        }
        pCache.pSynced = pPg.as_ref().map(|p| p.borrow().dirtySeq);
        if pPg.is_none() {
            for (_, p) in pCache.dirty.iter() {
                if p.borrow().nRef == 0 {
                    pPg = Some(p.clone());
                    break;
                }
            }
        }
        if let Some(pPg) = pPg {
            let xStress = pCache.xStress.expect("xStress");
            let rc = xStress(pPager, &pPg);
            if rc != SQLITE_OK && rc != SQLITE_BUSY {
                return rc;
            }
        }
    }
    *ppPage = pcache1Fetch(pPager.pPCache.pCache.as_mut().unwrap(), pgno, 2);
    if ppPage.is_none() {
        SQLITE_NOMEM
    } else {
        SQLITE_OK
    }
}

/// This is a helper routine for sqlite3PcacheFetchFinish()
///
/// In the uncommon case where the page being fetched has not been
/// initialized, this routine is invoked to do the initialization.
fn pcacheFetchFinishWithInit(pCache: &mut PCache, pgno: Pgno, pPage: &DbPage) -> DbPage {
    {
        let mut p = pPage.borrow_mut();
        debug_assert!(!p.bInit);
        p.dirtySeq = 0;
        p.pPager = std::rc::Weak::new();
        p.nRef = 0;
        p.bInit = true;
        p.pExtra = Default::default();
        p.pgno = pgno;
        p.flags = PGHDR_CLEAN as u16;
    }
    sqlite3PcacheFetchFinish(pCache, pgno, pPage)
}

/// C: `PgHdr * sqlite3PcacheFetchFinish( PCache *pCache, Pgno pgno, sqlite3_pcache_page *pPage )`
///
/// This routine converts the sqlite3_pcache_page object returned by
/// sqlite3PcacheFetch() into an initialized PgHdr object.  This routine
/// must be called after sqlite3PcacheFetch() in order to get a usable
/// result.
pub fn sqlite3PcacheFetchFinish(
    pCache: &mut crate::pcache::PCache,
    pgno: Pgno,
    pPage: &DbPage,
) -> DbPage {
    if !pPage.borrow().bInit {
        return pcacheFetchFinishWithInit(pCache, pgno, pPage);
    }
    pCache.nRefSum += 1;
    pPage.borrow_mut().nRef += 1;
    pPage.clone()
}

/// C: `void SQLITE_NOINLINE sqlite3PcacheRelease(PgHdr *p)`
///
/// Decrement the reference count on a page. If the page is clean and the
/// reference count drops to 0, then it is made eligible for recycling.
pub fn sqlite3PcacheRelease(pCache: &mut crate::pcache::PCache, p: &DbPage) {
    pCache.nRefSum -= 1;
    let (nRef, flags) = {
        let mut pg = p.borrow_mut();
        debug_assert!(pg.nRef > 0);
        pg.nRef -= 1;
        (pg.nRef, pg.flags as i32)
    };
    if nRef == 0 {
        if (flags & PGHDR_CLEAN) != 0 {
            pcacheUnpin(pCache, p);
        } else {
            pcacheManageDirtyList(pCache, p, PCACHE_DIRTYLIST_FRONT);
        }
    }
}

/// C: `void sqlite3PcacheRef(PgHdr *p)`
///
/// Increase the reference count of a supplied page by 1.
pub fn sqlite3PcacheRef(pCache: &mut crate::pcache::PCache, p: &DbPage) {
    let mut pg = p.borrow_mut();
    debug_assert!(pg.nRef > 0);
    pg.nRef += 1;
    pCache.nRefSum += 1;
}

/// C: `void sqlite3PcacheDrop(PgHdr *p)`
///
/// Drop a page from the cache. There must be exactly one reference to the
/// page. This function deletes that reference, so after it returns the
/// page pointed to by p is invalid.
pub fn sqlite3PcacheDrop(pCache: &mut crate::pcache::PCache, p: &DbPage) {
    debug_assert!(p.borrow().nRef == 1);
    if (p.borrow().flags as i32 & PGHDR_DIRTY) != 0 {
        pcacheManageDirtyList(pCache, p, PCACHE_DIRTYLIST_REMOVE);
    }
    pCache.nRefSum -= 1;
    pcache1Unpin(pCache.pCache.as_mut().unwrap(), p, 1);
}

/// C: `void sqlite3PcacheMakeDirty(PgHdr *p)`
///
/// Make sure the page is marked as dirty. If it isn't dirty already, make
/// it so.
pub fn sqlite3PcacheMakeDirty(pCache: &mut crate::pcache::PCache, p: &DbPage) {
    let flags = {
        let pg = p.borrow();
        debug_assert!(pg.nRef > 0);
        pg.flags as i32
    };
    if (flags & (PGHDR_CLEAN | PGHDR_DONT_WRITE)) != 0 {
        let clean = {
            let mut pg = p.borrow_mut();
            pg.flags &= !(PGHDR_DONT_WRITE as u16);
            if (pg.flags as i32 & PGHDR_CLEAN) != 0 {
                pg.flags ^= (PGHDR_DIRTY | PGHDR_CLEAN) as u16;
                true
            } else {
                false
            }
        };
        if clean {
            pcacheManageDirtyList(pCache, p, PCACHE_DIRTYLIST_ADD);
        }
    }
}

/// C: `void sqlite3PcacheMakeClean(PgHdr *p)`
///
/// Make sure the page is marked as clean. If it isn't clean already, make
/// it so.
pub fn sqlite3PcacheMakeClean(pCache: &mut crate::pcache::PCache, p: &DbPage) {
    debug_assert!((p.borrow().flags as i32 & PGHDR_DIRTY) != 0);
    debug_assert!((p.borrow().flags as i32 & PGHDR_CLEAN) == 0);
    pcacheManageDirtyList(pCache, p, PCACHE_DIRTYLIST_REMOVE);
    let nRef = {
        let mut pg = p.borrow_mut();
        pg.flags &= !((PGHDR_DIRTY | PGHDR_NEED_SYNC | PGHDR_WRITEABLE) as u16);
        pg.flags |= PGHDR_CLEAN as u16;
        pg.nRef
    };
    if nRef == 0 {
        pcacheUnpin(pCache, p);
    }
}

/// C: `void sqlite3PcacheCleanAll(PCache *pCache)`
///
/// Make every page in the cache clean.
pub fn sqlite3PcacheCleanAll(pCache: &mut crate::pcache::PCache) {
    while let Some(p) = pCache.dirty.values().next_back().cloned() {
        sqlite3PcacheMakeClean(pCache, &p);
    }
}

/// C: `void sqlite3PcacheClearWritable(PCache *pCache)`
///
/// Clear the PGHDR_NEED_SYNC and PGHDR_WRITEABLE flag from all dirty
/// pages.
pub fn sqlite3PcacheClearWritable(pCache: &mut crate::pcache::PCache) {
    for p in pCache.dirty.values() {
        p.borrow_mut().flags &= !((PGHDR_NEED_SYNC | PGHDR_WRITEABLE) as u16);
    }
    pCache.pSynced = pCache.dirty.keys().next().copied();
}

/// C: `void sqlite3PcacheClearSyncFlags(PCache *pCache)`
///
/// Clear the PGHDR_NEED_SYNC flag from all dirty pages.
pub fn sqlite3PcacheClearSyncFlags(pCache: &mut crate::pcache::PCache) {
    for p in pCache.dirty.values() {
        p.borrow_mut().flags &= !(PGHDR_NEED_SYNC as u16);
    }
    pCache.pSynced = pCache.dirty.keys().next().copied();
}

/// C: `void sqlite3PcacheMove(PgHdr *p, Pgno newPgno)`
///
/// Change the page number of page p to newPgno.
pub fn sqlite3PcacheMove(pCache: &mut crate::pcache::PCache, p: &DbPage, newPgno: Pgno) {
    debug_assert!(p.borrow().nRef > 0);
    debug_assert!(newPgno > 0);
    let pOther = pcache1Fetch(pCache.pCache.as_mut().unwrap(), newPgno, 0);
    if let Some(pOther) = pOther {
        if pOther.borrow().bInit {
            // C: pXPage = (PgHdr*)pOther->pExtra
            {
                let mut x = pOther.borrow_mut();
                debug_assert!(x.nRef == 0);
                x.nRef += 1;
            }
            pCache.nRefSum += 1;
            sqlite3PcacheDrop(pCache, &pOther);
        } else {
            // A raw page that was never finished: just discard it.
            pcache1Unpin(pCache.pCache.as_mut().unwrap(), &pOther, 1);
        }
    }
    let oldPgno = p.borrow().pgno;
    pcache1Rekey(pCache.pCache.as_mut().unwrap(), p, oldPgno, newPgno);
    let flags = {
        let mut pg = p.borrow_mut();
        pg.pgno = newPgno;
        pg.flags as i32
    };
    if (flags & PGHDR_DIRTY) != 0 && (flags & PGHDR_NEED_SYNC) != 0 {
        pcacheManageDirtyList(pCache, p, PCACHE_DIRTYLIST_FRONT);
    }
}

/// C: `void sqlite3PcacheTruncate(PCache *pCache, Pgno pgno)`
///
/// Drop every cache entry whose page number is greater than "pgno". The
/// caller must ensure that there are no outstanding references to any
/// pages other than page 1 with a page number greater than pgno.
///
/// If there is a reference to page 1 and the pgno parameter passed to
/// this function is 0, then the data area associated with page 1 is
/// zeroed, but the page object is not dropped.
pub fn sqlite3PcacheTruncate(pCache: &mut crate::pcache::PCache, pgno: Pgno) {
    if pCache.pCache.is_some() {
        let mut pgno = pgno;
        let pages: Vec<DbPage> = pCache.dirty.values().rev().cloned().collect();
        for p in pages {
            // This routine never gets call with a positive pgno except
            // right after sqlite3PcacheCleanAll().  So if there are dirty
            // pages, it must be that pgno==0.
            let ppgno = p.borrow().pgno;
            debug_assert!(ppgno > 0);
            if ppgno > pgno {
                debug_assert!((p.borrow().flags as i32 & PGHDR_DIRTY) != 0);
                sqlite3PcacheMakeClean(pCache, &p);
            }
        }
        if pgno == 0 && pCache.nRefSum != 0 {
            let pPage1 = pcache1Fetch(pCache.pCache.as_mut().unwrap(), 1, 0);
            if let Some(pPage1) = pPage1 {
                // Page 1 is always available in cache, because
                // pCache->nRefSum>0
                for b in pPage1.borrow_mut().aData.iter_mut() {
                    *b = 0;
                }
                pgno = 1;
            }
        }
        pcache1Truncate(pCache.pCache.as_mut().unwrap(), pgno + 1);
    }
}

/// C: `void sqlite3PcacheClose(PCache *pCache)`
///
/// Close a cache.
pub fn sqlite3PcacheClose(pCache: &mut crate::pcache::PCache) {
    if let Some(c) = pCache.pCache.take() {
        pcache1Destroy(c);
    }
    pCache.dirty.clear();
}

/// C: `void sqlite3PcacheClear(PCache *pCache)`
///
/// Discard the contents of the cache.
pub fn sqlite3PcacheClear(pCache: &mut crate::pcache::PCache) {
    sqlite3PcacheTruncate(pCache, 0);
}

/// C: `PgHdr * sqlite3PcacheDirtyList(PCache *pCache)`
///
/// Return a list of all dirty pages in the cache, sorted by page number.
/// Signature change: the list is a Vec (C: linked through PgHdr.pDirty).
pub fn sqlite3PcacheDirtyList(pCache: &mut crate::pcache::PCache) -> Vec<DbPage> {
    let mut v: Vec<DbPage> = pCache.dirty.values().rev().cloned().collect();
    v.sort_by_key(|p| p.borrow().pgno);
    v
}

/// C: `i64 sqlite3PcacheRefCount(PCache *pCache)`
///
/// Return the total number of references to all pages held by the cache.
///
/// This is not the total number of pages referenced, but the sum of the
/// reference count for all pages.
pub fn sqlite3PcacheRefCount(pCache: &mut crate::pcache::PCache) -> i64 {
    pCache.nRefSum
}

/// C: `i64 sqlite3PcachePageRefcount(PgHdr *p)`
///
/// Return the number of references to the page supplied as an argument.
pub fn sqlite3PcachePageRefcount(p: &DbPage) -> i64 {
    p.borrow().nRef
}

/// C: `int sqlite3PcachePagecount(PCache *pCache)`
///
/// Return the total number of pages in the cache.
pub fn sqlite3PcachePagecount(pCache: &mut crate::pcache::PCache) -> i32 {
    pcache1Pagecount(pCache.pCache.as_ref().unwrap())
}

/// C: `int sqlite3PcacheGetCachesize(PCache *pCache)`
///
/// Get the suggested cache-size value.
pub fn sqlite3PcacheGetCachesize(pCache: &mut crate::pcache::PCache) -> i32 {
    numberOfCachePages(pCache)
}

/// C: `void sqlite3PcacheSetCachesize(PCache *pCache, int mxPage)`
///
/// Set the suggested cache-size value.
pub fn sqlite3PcacheSetCachesize(pCache: &mut crate::pcache::PCache, mxPage: i32) {
    pCache.szCache = mxPage;
    let n = numberOfCachePages(pCache);
    pcache1Cachesize(pCache.pCache.as_mut().unwrap(), n);
}

/// C: `int sqlite3PcacheSetSpillsize(PCache *p, int mxPage)`
///
/// Set the suggested cache-spill value.  Make no changes if if the
/// argument is zero.  Return the effective cache-spill size, which will
/// be the larger of the szSpill and szCache.
pub fn sqlite3PcacheSetSpillsize(p: &mut crate::pcache::PCache, mxPage: i32) -> i32 {
    let mut mxPage = mxPage;
    if mxPage != 0 {
        if mxPage < 0 {
            mxPage = ((-1024 * mxPage as i64) / ((p.szPage + p.szExtra) as i64)) as i32;
        }
        p.szSpill = mxPage;
    }
    let mut res = numberOfCachePages(p);
    if res < p.szSpill {
        res = p.szSpill;
    }
    res
}

/// C: `void sqlite3PcacheShrink(PCache *pCache)`
///
/// Free up as much memory as possible from the page cache.
pub fn sqlite3PcacheShrink(pCache: &mut crate::pcache::PCache) {
    pcache1Shrink(pCache.pCache.as_mut().unwrap());
}

/// C: `int sqlite3HeaderSizePcache(void)`
///
/// Return the size of the header added by this middleware layer in the
/// page-cache hierarchy (the 64-bit C value).
pub fn sqlite3HeaderSizePcache() -> i32 {
    80
}

/// C: `int sqlite3PCachePercentDirty(PCache *pCache)`
///
/// Return the number of dirty pages currently in the cache, as a
/// percentage of the configured cache size.
pub fn sqlite3PCachePercentDirty(pCache: &mut crate::pcache::PCache) -> i32 {
    let nDirty = pCache.dirty.len() as i64;
    let nCache = numberOfCachePages(pCache) as i64;
    if nCache != 0 {
        ((nDirty * 100) / nCache) as i32
    } else {
        0
    }
}

/// C: `int sqlite3PCacheIsDirty(PCache *pCache)`
///
/// Return true if there are one or more dirty pages in the cache. Else
/// false.
pub fn sqlite3PCacheIsDirty(pCache: &mut crate::pcache::PCache) -> i32 {
    (!pCache.dirty.is_empty()) as i32
}

/// C: `void sqlite3PcacheIterateDirty(PCache *pCache, void (*xIter)(PgHdr *))`
///
/// For all dirty pages currently in the cache, invoke the specified
/// callback (head of the dirty list first).
pub fn sqlite3PcacheIterateDirty(
    pCache: &mut crate::pcache::PCache,
    xIter: &mut dyn FnMut(&DbPage),
) {
    let v: Vec<DbPage> = pCache.dirty.values().rev().cloned().collect();
    for p in &v {
        xIter(p);
    }
}
