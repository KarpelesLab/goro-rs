//! Port of pcache1.c: the default page cache implementation.
//!
//! The configuration mirrored is the default threadsafe build without
//! SQLITE_CONFIG_PAGECACHE memory, i.e. pcache1 "mode 1": every PCache1
//! has its own private PGroup (pcache1.separateCache==1), so the group
//! counters live in the PCache1 itself.  Pages are `DbPage` objects
//! (Rc<RefCell<PgHdr>>) kept in a HashMap; the LRU list of unpinned pages
//! is an ordered map keyed by a sequence number (largest = most recently
//! unpinned, C lru.pLruNext; smallest = next page to recycle, C
//! lru.pLruPrev).  The bulk-allocation of pcache1InitBulk() only affects
//! memory layout and is not reproduced.

use crate::int::*;
use crate::pager::PgHdr;
use std::collections::{BTreeMap, HashMap};

/// Each page cache is an instance of the following object (C PCache1 plus
/// the fields of its private PGroup).
#[derive(Default)]
pub struct PCache1 {
    // --- PGroup ---
    /// Sum of nMax for purgeable caches
    nMaxPage: u32,
    /// Sum of nMin for purgeable caches
    nMinPage: u32,
    /// nMaxpage + 10 - nMinPage
    mxPinned: u32,
    /// Number of purgeable pages allocated
    nPurgeable: u32,
    /// The LRU list of unpinned pages, keyed by PgHdr::lruSeq
    lru: BTreeMap<u64, DbPage>,
    nLruSeq: u64,

    // --- PCache1 ---
    /// Size of database content section
    szPage: i32,
    /// sizeof(MemPage)+sizeof(PgHdr)
    #[allow(dead_code)]
    szExtra: i32,
    /// Total size of one pcache line
    #[allow(dead_code)]
    szAlloc: i32,
    /// True if cache is purgeable
    bPurgeable: i32,
    /// Minimum number of pages reserved
    nMin: u32,
    /// Configured "cache_size" value
    nMax: u32,
    /// nMax*9/10
    n90pct: u32,
    /// Largest key seen since xTruncate()
    iMaxKey: u32,
    /// pnPurgeable points here when not used
    nPurgeableDummy: u32,
    /// Number of pages in the LRU list
    nRecyclable: u32,
    /// Total number of pages in apHash
    nPage: u32,
    /// Hash table of all pages
    apHash: HashMap<u32, DbPage>,
}

const ROUND8_SIZEOF_PGHDR1: i32 = 56;

impl PCache1 {
    fn pnPurgeable(&mut self) -> &mut u32 {
        if self.bPurgeable != 0 {
            &mut self.nPurgeable
        } else {
            &mut self.nPurgeableDummy
        }
    }
}

/// Break the reference cycles a page may hold (the btree's MemPage keeps
/// an Rc to its own DbPage) when the page is freed.
fn release_page_object(p: &DbPage) {
    if let Ok(mut pg) = p.try_borrow_mut() {
        pg.pExtra = Default::default();
        pg.lruSeq = 0;
    }
}

/// Allocate a new page object initially associated with cache pCache.
fn pcache1AllocPage(pCache: &mut PCache1, aData: Option<Vec<u8>>) -> DbPage {
    let data = match aData {
        Some(mut d) => {
            d.resize(pCache.szPage as usize, 0);
            d
        }
        None => vec![0u8; pCache.szPage as usize],
    };
    let p = PgHdr::new_raw(data);
    *pCache.pnPurgeable() += 1;
    p
}

/// Free a page object allocated by pcache1AllocPage().
fn pcache1FreePage(pCache: &mut PCache1, p: &DbPage) {
    release_page_object(p);
    let n = pCache.pnPurgeable();
    *n = n.wrapping_sub(1);
}

/// Return true if it desirable to avoid allocating a new page cache
/// entry (no soft heap limit and no SQLITE_CONFIG_PAGECACHE in this port).
fn pcache1UnderMemoryPressure(_pCache: &PCache1) -> bool {
    false
}

/// This function is used internally to remove the page pPage from the
/// PGroup LRU list, if is part of it.
fn pcache1PinPage(pCache: &mut PCache1, pPage: &DbPage) {
    let seq = {
        let mut p = pPage.borrow_mut();
        let s = p.lruSeq;
        p.lruSeq = 0;
        s
    };
    debug_assert!(seq != 0);
    pCache.lru.remove(&seq);
    pCache.nRecyclable -= 1;
}

/// Remove the page supplied as an argument from the hash table in which
/// it is stored.
fn pcache1RemoveFromHash(pCache: &mut PCache1, pPage: &DbPage, freeFlag: bool) {
    let iKey = pPage.borrow().iKey;
    pCache.apHash.remove(&iKey);
    pCache.nPage -= 1;
    if freeFlag {
        pcache1FreePage(pCache, pPage);
    }
}

/// If there are currently more than nMaxPage pages allocated, try to
/// recycle pages to reduce the number allocated to nMaxPage.
fn pcache1EnforceMaxPage(pCache: &mut PCache1) {
    while pCache.nPurgeable > pCache.nMaxPage {
        let p = match pCache.lru.values().next() {
            Some(p) => p.clone(),
            None => break,
        };
        pcache1PinPage(pCache, &p);
        pcache1RemoveFromHash(pCache, &p, true);
    }
}

/// Discard all pages from cache pCache with a page number (key value)
/// greater than or equal to iLimit. Any pinned pages that meet this
/// criteria are unpinned before they are discarded.
fn pcache1TruncateUnsafe(pCache: &mut PCache1, iLimit: u32) {
    debug_assert!(pCache.iMaxKey >= iLimit);
    let mut keys: Vec<u32> = pCache
        .apHash
        .keys()
        .copied()
        .filter(|&k| k >= iLimit)
        .collect();
    keys.sort_unstable();
    for k in keys {
        let pPage = pCache.apHash.remove(&k).unwrap();
        pCache.nPage -= 1;
        if pPage.borrow().lruSeq != 0 {
            pcache1PinPage(pCache, &pPage);
        }
        pcache1FreePage(pCache, &pPage);
    }
}

/// Implementation of the sqlite3_pcache.xCreate method.
///
/// Allocate a new cache.
pub(crate) fn pcache1Create(szPage: i32, szExtra: i32, bPurgeable: i32) -> PCache1 {
    debug_assert!((szPage & (szPage - 1)) == 0 && szPage >= 512 && szPage <= 65536);
    let mut pCache = PCache1 {
        mxPinned: 10,
        szPage,
        szExtra,
        szAlloc: szPage + szExtra + ROUND8_SIZEOF_PGHDR1,
        bPurgeable: (bPurgeable != 0) as i32,
        ..Default::default()
    };
    if bPurgeable != 0 {
        pCache.nMin = 10;
        pCache.nMinPage += pCache.nMin;
        pCache.mxPinned = pCache
            .nMaxPage
            .wrapping_add(10)
            .wrapping_sub(pCache.nMinPage);
    }
    pCache
}

/// Implementation of the sqlite3_pcache.xCachesize method.
///
/// Configure the cache_size limit for a cache.
pub(crate) fn pcache1Cachesize(pCache: &mut PCache1, nMax: i32) {
    debug_assert!(nMax >= 0);
    if pCache.bPurgeable != 0 {
        let mut n = nMax as u32;
        let lim = 0x7fff0000u32
            .wrapping_sub(pCache.nMaxPage)
            .wrapping_add(pCache.nMax);
        if n > lim {
            n = lim;
        }
        pCache.nMaxPage = pCache.nMaxPage.wrapping_add(n.wrapping_sub(pCache.nMax));
        pCache.mxPinned = pCache
            .nMaxPage
            .wrapping_add(10)
            .wrapping_sub(pCache.nMinPage);
        pCache.nMax = n;
        pCache.n90pct = ((pCache.nMax as u64) * 9 / 10) as u32;
        pcache1EnforceMaxPage(pCache);
    }
}

/// Implementation of the sqlite3_pcache.xShrink method.
///
/// Free up as much memory as possible.
pub(crate) fn pcache1Shrink(pCache: &mut PCache1) {
    if pCache.bPurgeable != 0 {
        let savedMaxPage = pCache.nMaxPage;
        pCache.nMaxPage = 0;
        pcache1EnforceMaxPage(pCache);
        pCache.nMaxPage = savedMaxPage;
    }
}

/// Implementation of the sqlite3_pcache.xPagecount method.
pub(crate) fn pcache1Pagecount(pCache: &PCache1) -> i32 {
    pCache.nPage as i32
}

/// Implement steps 3, 4, and 5 of the pcache1Fetch() algorithm described
/// in the header comment of pcache1.c.
fn pcache1FetchStage2(pCache: &mut PCache1, iKey: u32, createFlag: i32) -> Option<DbPage> {
    // Step 3: Abort if createFlag is 1 but the cache is nearly full
    debug_assert!(pCache.nPage >= pCache.nRecyclable);
    let nPinned = pCache.nPage - pCache.nRecyclable;
    if createFlag == 1
        && (nPinned >= pCache.mxPinned
            || nPinned >= pCache.n90pct
            || (pcache1UnderMemoryPressure(pCache) && pCache.nRecyclable < nPinned))
    {
        return None;
    }

    // Step 4. Try to recycle a page.
    let mut aData: Option<Vec<u8>> = None;
    let mut recycled = false;
    if pCache.bPurgeable != 0
        && !pCache.lru.is_empty()
        && ((pCache.nPage + 1 >= pCache.nMax) || pcache1UnderMemoryPressure(pCache))
    {
        let pPage = pCache.lru.values().next().unwrap().clone();
        pcache1RemoveFromHash(pCache, &pPage, false);
        pcache1PinPage(pCache, &pPage);
        // Same cache, hence same szAlloc: reuse the page buffer.  The
        // page object itself is replaced by a fresh one (C zeroes the
        // first 8 bytes of pExtra so that the PgHdr is reinitialized).
        aData = Some(std::mem::take(&mut pPage.borrow_mut().aData));
        release_page_object(&pPage);
        recycled = true;
    }

    // Step 5. If a usable page buffer has still not been found, attempt
    // to allocate a new one.
    let pPage = if recycled {
        // pcache1PinPage already accounted; the purgeable count is
        // unchanged by recycling within the same cache.
        let d = aData.unwrap();
        let mut d = d;
        d.resize(pCache.szPage as usize, 0);
        PgHdr::new_raw(d)
    } else {
        pcache1AllocPage(pCache, None)
    };

    {
        let mut p = pPage.borrow_mut();
        p.iKey = iKey;
        p.lruSeq = 0;
        p.bInit = false;
    }
    pCache.nPage += 1;
    pCache.apHash.insert(iKey, pPage.clone());
    if iKey > pCache.iMaxKey {
        pCache.iMaxKey = iKey;
    }
    Some(pPage)
}

/// Implementation of the sqlite3_pcache.xFetch method.
///
/// Fetch a page by key value.
pub(crate) fn pcache1Fetch(pCache: &mut PCache1, iKey: u32, createFlag: i32) -> Option<DbPage> {
    debug_assert!(pCache.bPurgeable != 0 || createFlag != 1);
    // Step 1: Search the hash table for an existing entry.
    let pPage = pCache.apHash.get(&iKey).cloned();
    // Step 2: If the page was found in the hash table, then return it.
    // If the page was not in the hash table and createFlag is 0, abort.
    // Otherwise (page not in hash and createFlag!=0) continue with
    // subsequent steps to try to create the page.
    match pPage {
        Some(p) => {
            if p.borrow().lruSeq != 0 {
                pcache1PinPage(pCache, &p);
            }
            Some(p)
        }
        None if createFlag != 0 => pcache1FetchStage2(pCache, iKey, createFlag),
        None => None,
    }
}

/// Implementation of the sqlite3_pcache.xUnpin method.
///
/// Mark a page as unpinned (eligible for asynchronous recycling).
pub(crate) fn pcache1Unpin(pCache: &mut PCache1, pPage: &DbPage, reuseUnlikely: i32) {
    debug_assert!(pPage.borrow().lruSeq == 0);
    if reuseUnlikely != 0 || pCache.nPurgeable > pCache.nMaxPage {
        pcache1RemoveFromHash(pCache, pPage, true);
    } else {
        // Add the page to the PGroup LRU list.
        pCache.nLruSeq += 1;
        let seq = pCache.nLruSeq;
        pPage.borrow_mut().lruSeq = seq;
        pCache.lru.insert(seq, pPage.clone());
        pCache.nRecyclable += 1;
    }
}

/// Implementation of the sqlite3_pcache.xRekey method.
pub(crate) fn pcache1Rekey(pCache: &mut PCache1, pPage: &DbPage, iOld: u32, iNew: u32) {
    debug_assert!(pPage.borrow().iKey == iOld);
    debug_assert!(iOld != iNew);
    if let Some(p) = pCache.apHash.remove(&iOld) {
        debug_assert!(std::rc::Rc::ptr_eq(&p, pPage));
    }
    debug_assert!(!pCache.apHash.contains_key(&iNew));
    pPage.borrow_mut().iKey = iNew;
    pCache.apHash.insert(iNew, pPage.clone());
    if iNew > pCache.iMaxKey {
        pCache.iMaxKey = iNew;
    }
}

/// Implementation of the sqlite3_pcache.xTruncate method.
///
/// Discard all unpinned pages in the cache with a page number equal to
/// or greater than parameter iLimit. Any pinned pages with a page number
/// equal to or greater than iLimit are implicitly unpinned.
pub(crate) fn pcache1Truncate(pCache: &mut PCache1, iLimit: u32) {
    if iLimit <= pCache.iMaxKey {
        pcache1TruncateUnsafe(pCache, iLimit);
        pCache.iMaxKey = iLimit.wrapping_sub(1);
    }
}

/// Implementation of the sqlite3_pcache.xDestroy method.
///
/// Destroy a cache allocated using pcache1Create().
pub(crate) fn pcache1Destroy(mut pCache: PCache1) {
    if pCache.nPage != 0 {
        pcache1TruncateUnsafe(&mut pCache, 0);
    }
    pCache.nMaxPage = pCache.nMaxPage.wrapping_sub(pCache.nMax);
    pCache.nMinPage = pCache.nMinPage.wrapping_sub(pCache.nMin);
    pCache.mxPinned = pCache
        .nMaxPage
        .wrapping_add(10)
        .wrapping_sub(pCache.nMinPage);
    pcache1EnforceMaxPage(&mut pCache);
    for p in pCache.apHash.values() {
        release_page_object(p);
    }
}

/// C: `void sqlite3PCacheBufferSetup(void *pBuf, int sz, int n)`
///
/// SQLITE_CONFIG_PAGECACHE memory is not supported: no-op.
pub fn sqlite3PCacheBufferSetup(pBuf: &mut [u8], sz: i32, n: i32) {
    let _ = (pBuf, sz, n);
}

/// C: `void sqlite3PCacheSetDefault(void)`
///
/// This port has a single built-in page cache: no-op.
pub fn sqlite3PCacheSetDefault() {}

/// C: `int sqlite3HeaderSizePcache1(void)`
///
/// Return the size of the header on each page of this PCACHE
/// implementation (the value of the 64-bit C build).
pub fn sqlite3HeaderSizePcache1() -> i32 {
    ROUND8_SIZEOF_PGHDR1
}

/// C: `sqlite3_mutex * sqlite3Pcache1Mutex(void)`
pub fn sqlite3Pcache1Mutex() -> Option<Box<()>> {
    None
}

/// C: `int sqlite3PcacheReleaseMemory(int nReq)`
///
/// Only does anything with SQLITE_ENABLE_MEMORY_MANAGEMENT (not enabled).
pub fn sqlite3PcacheReleaseMemory(nReq: i32) -> i32 {
    let _ = nReq;
    0
}

/// C: `void sqlite3PcacheStats( int *pnCurrent, int *pnMax, int *pnMin, int *pnRecyclable )`
///
/// Statistics of the global PGroup, which is unused in mode 1.
pub fn sqlite3PcacheStats(
    pnCurrent: &mut i32,
    pnMax: &mut i32,
    pnMin: &mut i32,
    pnRecyclable: &mut i32,
) {
    *pnCurrent = 0;
    *pnMax = 0;
    *pnMin = 0;
    *pnRecyclable = 0;
}
