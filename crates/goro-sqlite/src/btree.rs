//! Port of btree.c (and btreeInt.h): the B-tree layer.
//!
//! This file implements an external (disk-based) database using BTrees.
//! See the header comment of btree.c and the file format documentation
//! for details.  The port follows the C code function by function.
//!
//! Rust-specific representation choices (see PORTING.md §7):
//!
//! * A page is a `DbPage` (`Rc<RefCell<PgHdr>>`); the btree's per-page
//!   state (C `MemPage`) lives in `PgHdr::pExtra` next to the page image
//!   `PgHdr::aData`.  C pointers into a page become byte offsets into
//!   `aData` (`MemPage::aCellIdx` is `cellOffset`, `aDataOfst` is
//!   `childPtrSize`, `aDataEnd` is `pageSize`, `CellInfo::pPayload` is an
//!   offset into the buffer holding the cell).
//! * Overflow cells (`MemPage::apOvfl`) are owned copies of the cell.
//! * `BtShared` keeps weak references to its open cursors (C pCursor list,
//!   most recently opened first).
//! * There is no shared cache (SQLITE_OMIT_SHARED_CACHE): table locks are
//!   no-ops, a BtShared belongs to exactly one Btree.  No mmap.
//! * The btree layer cannot reference the connection: functions that need
//!   connection state take `db` (see the signature notes in the report).
//!   `BtShared::dbFlags` mirrors `db->flags` (refreshed by BeginTrans) for
//!   the PRAGMA cell_size_check test in btreeInitPage().

#![allow(unused_variables)]

use crate::bitvec::*;
use crate::int::*;
use crate::pager::*;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};

// ---------------------------------------------------------------------------
// btreeInt.h
// ---------------------------------------------------------------------------

/// The maximum cell size assuming a maximum page size (MX_CELL_SIZE).
#[inline]
fn MX_CELL_SIZE(pageSize: u32) -> i32 {
    pageSize as i32 - 8
}

/// The maximum number of cells on a single page of the database (MX_CELL).
#[inline]
fn MX_CELL(pageSize: u32) -> u32 {
    (pageSize - 8) / 6
}

/// The header string that appears at the beginning of every SQLite database.
static zMagicHeader: [u8; 16] = *b"SQLite format 3\0";

const BTALLOC_ANY: u8 = 0; /* Allocate any page */
const BTALLOC_EXACT: u8 = 1; /* Allocate exact page if possible */
const BTALLOC_LE: u8 = 2; /* Allocate any page <= the parameter */

/// Number of neighbors on either side of pPage (NN)
#[allow(dead_code)]
const NN: usize = 1;
/// (NN*2+1): Total pages involved in the balance (NB)
const NB: usize = 3;

/// Size of the BtCursor.apPage[] / aiIdx[] arrays.
const MAX_DEPTH_M1: usize = (BTCURSOR_MAX_DEPTH - 1) as usize;

/// The C `sizeof(MemPage)` rounded up to 8 on a 64-bit host
/// (sqlite3HeaderSizeBtree()).
const SIZEOF_MEMPAGE: i32 = 136;

/// Reference to a BtShared.
pub type BtSharedP = Rc<RefCell<BtShared>>;

/// Implementation of MemPage.xCellSize: (page, buffer holding the cell,
/// offset of the cell in the buffer) -> size of the cell on the page.
pub type XCellSize = fn(&MemPage, &[u8], usize) -> u16;
/// Implementation of MemPage.xParseCell: (page, buffer, offset of the cell
/// in the buffer, output).  CellInfo::pPayload is an offset in the buffer.
pub type XParseCell = fn(&MemPage, &[u8], usize, &mut CellInfo);

/// As each page of the file is loaded into memory, an instance of the
/// following structure is appended and initialized to zero.  This structure
/// stores information about the page that is decoded from the raw file page.
/// (C MemPage; lives in PgHdr::pExtra.)
#[derive(Clone)]
pub struct MemPage {
    /// True if previously initialized. MUST BE FIRST!
    pub isInit: u8,
    /// True if table b-trees.  False for index b-trees
    pub intKey: u8,
    /// True if the leaf of an intKey table
    pub intKeyLeaf: u8,
    /// Page number for this page
    pub pgno: Pgno,
    /* Only the first 8 bytes (above) are zeroed by pager.c when a new page
     ** is allocated. All fields that follow must be initialized before use */
    /// True if a leaf page
    pub leaf: u8,
    /// 100 for page 1.  0 otherwise
    pub hdrOffset: u8,
    /// 0 if leaf==1.  4 if leaf==0
    pub childPtrSize: u8,
    /// min(maxLocal,127)
    pub max1bytePayload: u8,
    /// Number of overflow cell bodies in aCell[]
    pub nOverflow: u8,
    /// Copy of BtShared.maxLocal or BtShared.maxLeaf
    pub maxLocal: u16,
    /// Copy of BtShared.minLocal or BtShared.minLeaf
    pub minLocal: u16,
    /// Index in aData of first cell pointer (also C aCellIdx)
    pub cellOffset: u16,
    /// Number of free bytes on the page. -1 for unknown
    pub nFree: i32,
    /// Number of cells on this page, local and ovfl
    pub nCell: u16,
    /// Mask for page offset
    pub maskPage: u16,
    /// Insert the i-th overflow cell before the aiOvfl-th non-overflow cell
    pub aiOvfl: [u16; 4],
    /// The body of overflow cells (owned copies)
    pub apOvfl: [Vec<u8>; 4],
    /// Pointer to BtShared that this page is part of
    pub pBt: Weak<RefCell<BtShared>>,
    /// Rust: copy of pBt->usableSize (set when the page is initialized)
    pub usableSize: u32,
    /// Rust: copy of pBt->pageSize; C aDataEnd is aData+pageSize
    pub pageSize: u32,
    /// cellSizePtr method
    pub xCellSize: XCellSize,
    /// btreeParseCell method
    pub xParseCell: XParseCell,
}

impl Default for MemPage {
    fn default() -> MemPage {
        MemPage {
            isInit: 0,
            intKey: 0,
            intKeyLeaf: 0,
            pgno: 0,
            leaf: 0,
            hdrOffset: 0,
            childPtrSize: 0,
            max1bytePayload: 0,
            nOverflow: 0,
            maxLocal: 0,
            minLocal: 0,
            cellOffset: 0,
            nFree: 0,
            nCell: 0,
            maskPage: 0,
            aiOvfl: [0; 4],
            apOvfl: Default::default(),
            pBt: Weak::new(),
            usableSize: 0,
            pageSize: 0,
            xCellSize: cellSizePtr,
            xParseCell: btreeParseCellPtrIndex,
        }
    }
}

impl MemPage {
    /// C aDataOfst: same as aData for leaves.  aData+4 for interior.
    #[inline]
    fn aDataOfst(&self) -> usize {
        self.childPtrSize as usize
    }
    /// C aCellIdx
    #[inline]
    fn aCellIdx(&self) -> usize {
        self.cellOffset as usize
    }
    /// C aDataEnd
    #[inline]
    fn aDataEnd(&self) -> usize {
        self.pageSize as usize
    }
    /// A copy of the page descriptor without the overflow cells.
    fn lite(&self) -> MemPage {
        MemPage {
            apOvfl: Default::default(),
            pBt: self.pBt.clone(),
            ..*self
        }
    }
    /// The C pBt->btsFlags of this page's BtShared.
    fn btsFlags(&self) -> u16 {
        match self.pBt.upgrade() {
            Some(b) => b.borrow().btsFlags,
            None => 0,
        }
    }
}

/// A Btree handle.  A database connection contains a pointer to an instance
/// of this object for every database file that it has open.
pub struct Btree {
    /// Sharable content of this btree
    pub pBt: BtSharedP,
    /// TRANS_NONE, TRANS_READ or TRANS_WRITE
    pub inTrans: u8,
    /// True if we can share pBt with another db (always 0)
    pub sharable: u8,
    /// True if db currently has pBt locked
    pub locked: u8,
    /// True if there are one or more Incrblob cursors
    pub hasIncrblobCur: u8,
    /// Number of nested calls to sqlite3BtreeEnter()
    pub wantToLock: i32,
    /// Number of backup operations reading this btree
    pub nBackup: i32,
    /// Combines with pBt->pPager->iDataVersion
    pub iBDataVersion: u32,
    /// Calls to sqlite3BtreeMovetoUnpacked() (SQLITE_DEBUG)
    pub nSeek: u64,
}

/// An instance of this object represents a single database file.
pub struct BtShared {
    /// The page cache
    pub pPager: PagerP,
    /// A list of all open cursors (most recently opened first)
    pub pCursor: Vec<Weak<RefCell<BtCursor>>>,
    /// First page of the database
    pub pPage1: Option<DbPage>,
    /// Flags to sqlite3BtreeOpen()
    pub openFlags: u8,
    /// True if auto-vacuum is enabled
    pub autoVacuum: u8,
    /// True if incr-vacuum is enabled
    pub incrVacuum: u8,
    /// True to truncate db on commit
    pub bDoTruncate: u8,
    /// Transaction state
    pub inTransaction: u8,
    /// Maximum first byte of cell for a 1-byte payload
    pub max1bytePayload: u8,
    /// Desired number of extra bytes per page
    pub nReserveWanted: u8,
    /// Boolean parameters.  See BTS_* macros below
    pub btsFlags: u16,
    /// Maximum local payload in non-LEAFDATA tables
    pub maxLocal: u16,
    /// Minimum local payload in non-LEAFDATA tables
    pub minLocal: u16,
    /// Maximum local payload in a LEAFDATA table
    pub maxLeaf: u16,
    /// Minimum local payload in a LEAFDATA table
    pub minLeaf: u16,
    /// Total number of bytes on a page
    pub pageSize: u32,
    /// Number of usable bytes on each page
    pub usableSize: u32,
    /// Number of open transactions (read + write)
    pub nTransaction: i32,
    /// Number of pages in the database
    pub nPage: u32,
    /// Set of pages moved to free-list this transaction
    pub pHasContent: Option<Box<Bitvec>>,
    /// Temp space sufficient to hold a single cell (empty: not allocated)
    pub pTmpSpace: Vec<u8>,
    /// Size of last cell written by TransferRow()
    pub nPreformatSize: i32,
    /// Rust: copy of db->flags, refreshed by sqlite3BtreeBeginTrans()
    pub dbFlags: u64,
}

/// An instance of the following structure is used to hold information
/// about a cell.  The parseCellPtr() function fills in this structure
/// based on information extract from the raw disk page.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CellInfo {
    /// The key for INTKEY tables, or nPayload otherwise
    pub nKey: i64,
    /// Offset of the start of payload (in the buffer holding the cell)
    pub pPayload: usize,
    /// Bytes of payload
    pub nPayload: u32,
    /// Amount of payload held locally, not on overflow
    pub nLocal: u16,
    /// Size of the cell content on the main b-tree page
    pub nSize: u16,
}

/// A cursor is a pointer to a particular entry within a particular
/// b-tree within a database file.
#[derive(Default)]
pub struct BtCursor {
    /// One of the CURSOR_XXX constants (see below)
    pub eState: u8,
    /// zero or more BTCF_* flags defined below
    pub curFlags: u8,
    /// Flags to send to sqlite3PagerGet()
    pub curPagerFlags: u8,
    /// As configured by CursorSetHints()
    pub hints: u8,
    /// Prev() is noop if negative. Next() is noop if positive.
    /// Error code if eState==CURSOR_FAULT
    pub skipNext: i32,
    /// The Btree to which this cursor belongs
    pub pBtree: Option<BtreeP>,
    /// Cache of overflow page locations
    pub aOverflow: Vec<Pgno>,
    /// Saved key that was cursor last known position
    pub pKey: Option<Vec<u8>>,
    /// The BtShared this cursor points to
    pub pBt: Option<BtSharedP>,
    /// A parse of the cell we are pointing at
    pub info: CellInfo,
    /// Size of pKey, or last integer key
    pub nKey: i64,
    /// The root page of this tree
    pub pgnoRoot: Pgno,
    /// Index of current page in apPage
    pub iPage: i8,
    /// Value of apPage[0]->intKey
    pub curIntKey: u8,
    /// Current index for apPage[iPage]
    pub ix: u16,
    /// Current index in apPage[i]
    pub aiIdx: [u16; MAX_DEPTH_M1],
    /// Arg passed to comparison function
    pub pKeyInfo: Option<KeyInfoRef>,
    /// Current page
    pub pPage: Option<DbPage>,
    /// Stack of parents of current page
    pub apPage: [Option<DbPage>; MAX_DEPTH_M1],
}

impl BtCursor {
    #[inline]
    fn bt(&self) -> &BtSharedP {
        self.pBt.as_ref().expect("cursor not open")
    }
    #[inline]
    fn page(&self) -> &DbPage {
        self.pPage.as_ref().expect("cursor has no page")
    }
    #[inline]
    fn btree(&self) -> &BtreeP {
        self.pBtree.as_ref().expect("cursor not open")
    }
}

/// Content of a b-tree cell for sqlite3BtreeInsert().
pub struct BtreePayload<'a> {
    pub pKey: Option<&'a [u8]>,
    pub nKey: i64,
    pub pData: Option<&'a [u8]>,
    pub aMem: Option<&'a [Mem]>,
    pub nMem: u16,
    pub nData: i32,
    pub nZero: i32,
}

// ---------------------------------------------------------------------------
// Small helpers (macros of btreeInt.h / sqliteInt.h)
// ---------------------------------------------------------------------------

/// SQLITE_CORRUPT_BKPT (sqlite3CorruptError() only logs).
#[inline]
fn SQLITE_CORRUPT_BKPT() -> i32 {
    SQLITE_CORRUPT
}
#[inline]
fn SQLITE_CORRUPT_PGNO(_p: Pgno) -> i32 {
    SQLITE_CORRUPT
}
#[inline]
fn SQLITE_CORRUPT_PAGE(_p: &MemPage) -> i32 {
    SQLITE_CORRUPT
}

/// Read a byte; a read past the end of the buffer returns 0 (C would read
/// the padding that follows the page image).
#[inline]
fn at(a: &[u8], i: usize) -> u8 {
    if i < a.len() { a[i] } else { 0 }
}

#[inline]
fn get2byte(a: &[u8], i: usize) -> usize {
    ((at(a, i) as usize) << 8) | at(a, i + 1) as usize
}
#[inline]
fn get2byteAligned(a: &[u8], i: usize) -> usize {
    get2byte(a, i)
}
#[inline]
fn get2byteNotZero(a: &[u8], i: usize) -> usize {
    ((((get2byte(a, i) as i32) - 1) & 0xffff) + 1) as usize
}
#[inline]
fn put2byte(a: &mut [u8], i: usize, v: usize) {
    a[i] = (v >> 8) as u8;
    a[i + 1] = v as u8;
}
#[inline]
fn get4byte(a: &[u8], i: usize) -> u32 {
    ((at(a, i) as u32) << 24)
        | ((at(a, i + 1) as u32) << 16)
        | ((at(a, i + 2) as u32) << 8)
        | (at(a, i + 3) as u32)
}
#[inline]
fn put4byte(a: &mut [u8], i: usize, v: u32) {
    a[i] = (v >> 24) as u8;
    a[i + 1] = (v >> 16) as u8;
    a[i + 2] = (v >> 8) as u8;
    a[i + 3] = v as u8;
}

/// sqlite3GetVarint() on a[off..]: returns the number of bytes read.
fn getVarint(a: &[u8], off: usize, v: &mut u64) -> u8 {
    let mut x: u64 = 0;
    for i in 0..8 {
        let b = at(a, off + i);
        x = (x << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            *v = x;
            return (i + 1) as u8;
        }
    }
    x = (x << 8) | at(a, off + 8) as u64;
    *v = x;
    9
}

/// sqlite3VarintValue()
#[inline]
fn varintValue(a: &[u8], off: usize) -> i64 {
    let mut v = 0u64;
    getVarint(a, off, &mut v);
    v as i64
}

/// sqlite3PutVarint(): write v at a[off..], return the number of bytes.
fn putVarint(a: &mut [u8], off: usize, v: u64) -> usize {
    if v <= 0x7f {
        a[off] = (v & 0x7f) as u8;
        return 1;
    }
    if v <= 0x3fff {
        a[off] = (((v >> 7) & 0x7f) | 0x80) as u8;
        a[off + 1] = (v & 0x7f) as u8;
        return 2;
    }
    let mut v = v;
    if v & ((0xff000000u64) << 32) != 0 {
        a[off + 8] = v as u8;
        v >>= 8;
        let mut i = 7i32;
        while i >= 0 {
            a[off + i as usize] = ((v & 0x7f) | 0x80) as u8;
            v >>= 7;
            i -= 1;
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
    let mut i = 0usize;
    let mut j = n as i32 - 1;
    while j >= 0 {
        a[off + i] = buf[j as usize];
        j -= 1;
        i += 1;
    }
    n
}

/// putVarint32()
#[inline]
fn putVarint32(a: &mut [u8], off: usize, v: u32) -> usize {
    if v < 0x80 {
        a[off] = v as u8;
        1
    } else {
        putVarint(a, off, v as u64)
    }
}

/// sqlite3AbsInt32()
#[inline]
fn sqlite3AbsInt32(x: i32) -> i32 {
    if x >= 0 {
        return x;
    }
    if x == i32::MIN {
        return 0x7fffffff;
    }
    -x
}

/// PENDING_BYTE_PAGE(pBt)
#[inline]
fn PENDING_BYTE_PAGE(pageSize: u32) -> Pgno {
    ((crate::global::sqlite3PendingByte / (pageSize as i64)) + 1) as Pgno
}

/// Borrow the MemPage of a page.
#[inline]
fn mp(pg: &DbPage) -> Ref<'_, MemPage> {
    Ref::map(pg.borrow(), |h| &h.pExtra)
}
/// Mutably borrow the MemPage of a page.
#[inline]
fn mpm(pg: &DbPage) -> RefMut<'_, MemPage> {
    RefMut::map(pg.borrow_mut(), |h| &mut h.pExtra)
}
/// The MemPage.pgno of a page.
#[inline]
fn pgnoOf(pg: &DbPage) -> Pgno {
    pg.borrow().pExtra.pgno
}

/// The pager of a BtShared.
#[inline]
fn pagerOf(pBt: &BtSharedP) -> PagerP {
    pBt.borrow().pPager.clone()
}

/// The page 1 of a BtShared (must be loaded).
#[inline]
fn page1Of(pBt: &BtSharedP) -> DbPage {
    pBt.borrow().pPage1.clone().expect("page 1 not loaded")
}

/// get4byte(&pBt->pPage1->aData[off])
#[inline]
fn page1Get4(pBt: &BtSharedP, off: usize) -> u32 {
    let p1 = page1Of(pBt);
    let v = get4byte(&p1.borrow().aData, off);
    v
}

/// The open cursors of a BtShared (strong references).
fn cursorList(pBt: &BtSharedP) -> Vec<BtCursorP> {
    pBt.borrow()
        .pCursor
        .iter()
        .filter_map(|w| w.upgrade())
        .collect()
}

/// True if `c` is the cursor whose content is `cur` (pointer identity).
#[inline]
fn isSameCursor(c: &BtCursorP, cur: *const BtCursor) -> bool {
    c.as_ptr() as *const BtCursor == cur
}

// ---------------------------------------------------------------------------
// Shared cache stubs, debug helpers
// ---------------------------------------------------------------------------

/// C: `int sqlite3_enable_shared_cache(int enable)`
///
/// This build omits the shared cache (SQLITE_OMIT_SHARED_CACHE): the
/// function exists for API compatibility and has no effect.
pub fn sqlite3_enable_shared_cache(enable: i32) -> i32 {
    SQLITE_OK
}

/// C: `sqlite3_uint64 sqlite3BtreeSeekCount(Btree *pBt)` (SQLITE_DEBUG)
pub fn sqlite3BtreeSeekCount(pBt: &BtreeP) -> u64 {
    let mut p = pBt.borrow_mut();
    let n = p.nSeek;
    p.nSeek = 0;
    n
}

/// Invalidate the overflow cache of the cursor passed as the first argument.
#[inline]
fn invalidateOverflowCache(pCur: &mut BtCursor) {
    pCur.curFlags &= !(BTCF_ValidOvfl as u8);
}

/// Invalidate the overflow page-list cache for all cursors opened
/// on the shared btree structure pBt.
fn invalidateAllOverflowCache(pBt: &BtSharedP) {
    for p in cursorList(pBt) {
        if let Ok(mut c) = p.try_borrow_mut() {
            invalidateOverflowCache(&mut c);
        } else {
            debug_assert!(false, "cursor borrowed during invalidateAllOverflowCache");
        }
    }
}

/// This function is called before modifying the contents of a table
/// to invalidate any incrblob cursors that are open on the
/// row or one of the rows being modified.
///
/// If argument isClearTable is true, then the entire contents of the
/// table is about to be deleted. In this case invalidate all incrblob
/// cursors open on any row within the table with root-page pgnoRoot.
///
/// Otherwise, if argument isClearTable is false, then the row with
/// rowid iRow is being replaced or deleted. In this case invalidate
/// only those incrblob cursors open on that specific row.
///
/// `pSelf` is the cursor (if any) currently borrowed by the caller.
fn invalidateIncrblobCursors(
    pBtree: &BtreeP,
    pgnoRoot: Pgno,
    iRow: i64,
    isClearTable: i32,
    mut pSelf: Option<&mut BtCursor>,
) {
    debug_assert!(pBtree.borrow().hasIncrblobCur != 0);
    let pBt = pBtree.borrow().pBt.clone();
    pBtree.borrow_mut().hasIncrblobCur = 0;
    let selfPtr: *const BtCursor = match &pSelf {
        Some(c) => &**c as *const BtCursor,
        None => std::ptr::null(),
    };
    let mut has = 0u8;
    for p in cursorList(&pBt) {
        let mut f = |c: &mut BtCursor| {
            if (c.curFlags as i32 & BTCF_Incrblob) != 0 {
                has = 1;
                if c.pgnoRoot == pgnoRoot && (isClearTable != 0 || c.info.nKey == iRow) {
                    c.eState = CURSOR_INVALID as u8;
                }
            }
        };
        if isSameCursor(&p, selfPtr) {
            if let Some(c) = pSelf.as_deref_mut() {
                f(c);
            }
        } else {
            f(&mut p.borrow_mut());
        }
    }
    if has != 0 {
        pBtree.borrow_mut().hasIncrblobCur = 1;
    }
}

/// Set bit pgno of the BtShared.pHasContent bitvec. This is called
/// when a page that previously contained data becomes a free-list leaf
/// page.
///
/// The BtShared.pHasContent bitvec exists to work around an obscure
/// bug caused by the interaction of two useful IO optimizations surrounding
/// free-list leaf pages (see btree.c for the full explanation).
fn btreeSetHasContent(pBt: &BtSharedP, pgno: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    let mut bt = pBt.borrow_mut();
    if bt.pHasContent.is_none() {
        debug_assert!(pgno <= bt.nPage);
        bt.pHasContent = sqlite3BitvecCreate(bt.nPage);
        if bt.pHasContent.is_none() {
            rc = SQLITE_NOMEM;
        }
    }
    if rc == SQLITE_OK {
        let p = bt.pHasContent.as_mut().unwrap();
        if pgno <= sqlite3BitvecSize(p) {
            rc = sqlite3BitvecSet(p, pgno);
        }
    }
    rc
}

/// Query the BtShared.pHasContent vector.
///
/// This function is called when a free-list leaf page is removed from the
/// free-list for reuse. It returns false if it is safe to retrieve the
/// page from the pager layer with the 'no-content' flag set. True otherwise.
fn btreeGetHasContent(pBt: &BtSharedP, pgno: Pgno) -> bool {
    let mut bt = pBt.borrow_mut();
    match bt.pHasContent.as_mut() {
        Some(p) => pgno > sqlite3BitvecSize(p) || sqlite3BitvecTestNotNull(p, pgno) != 0,
        None => false,
    }
}

/// Clear (destroy) the BtShared.pHasContent bitvec. This should be
/// invoked at the conclusion of each write-transaction.
fn btreeClearHasContent(pBt: &BtSharedP) {
    let p = pBt.borrow_mut().pHasContent.take();
    if let Some(mut p) = p {
        sqlite3BitvecDestroy(&mut p);
    }
}

/// Release all of the apPage[] pages for a cursor.
fn btreeReleaseAllCursorPages(pCur: &mut BtCursor) {
    if pCur.iPage >= 0 {
        for i in 0..pCur.iPage as usize {
            if let Some(pg) = pCur.apPage[i].take() {
                releasePageNotNull(&pg);
            }
        }
        if let Some(pg) = pCur.pPage.take() {
            releasePageNotNull(&pg);
        }
        pCur.iPage = -1;
    }
}

/// The cursor passed as the only argument must point to a valid entry
/// when this function is called (i.e. have eState==CURSOR_VALID). This
/// function saves the current cursor key in variables pCur->nKey and
/// pCur->pKey. SQLITE_OK is returned if successful or an SQLite error
/// code otherwise.
///
/// If the cursor is open on an intkey table, then the integer key
/// (the rowid) is stored in pCur->nKey and pCur->pKey is left set to
/// NULL. If the cursor is open on a non-intkey table, then pCur->pKey is
/// set to point to a malloced buffer pCur->nKey bytes in size containing
/// the key.
fn saveCursorKey(pCur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(CURSOR_VALID as u8 == pCur.eState);
    /* assert( 0==pCur->pKey ): may not hold after an error on a corrupt db */

    if pCur.curIntKey != 0 {
        /* Only the rowid is required for a table btree */
        pCur.nKey = btreeIntegerKey(pCur);
    } else {
        /* For an index btree, save the complete key content. It is possible
         ** that the current key is corrupt. In that case, it is possible that
         ** the sqlite3VdbeRecordUnpack() function may overread the buffer by
         ** up to the size of 1 varint plus 1 8-byte value when the cursor
         ** position is restored. Hence the 17 bytes of padding allocated
         ** below. */
        pCur.nKey = btreePayloadSize(pCur) as i64;
        let mut pKey = vec![0u8; pCur.nKey as usize + 9 + 8];
        rc = accessPayload(pCur, 0, pCur.nKey as u32, &mut pKey, 0);
        if rc == SQLITE_OK {
            pCur.pKey = Some(pKey);
        }
    }
    /* assert( !pCur->curIntKey || !pCur->pKey ) */
    rc
}

/// Save the current cursor position in the variables BtCursor.nKey
/// and BtCursor.pKey. The cursor's state is set to CURSOR_REQUIRESEEK.
///
/// The caller must ensure that the cursor is valid (has eState==CURSOR_VALID)
/// prior to calling this routine.
fn saveCursorPosition(pCur: &mut BtCursor) -> i32 {
    debug_assert!(CURSOR_VALID as u8 == pCur.eState || CURSOR_SKIPNEXT as u8 == pCur.eState);
    /* assert( 0==pCur->pKey ): may not hold after an error on a corrupt db */

    if pCur.curFlags as i32 & BTCF_Pinned != 0 {
        return SQLITE_CONSTRAINT_PINNED;
    }
    if pCur.eState == CURSOR_SKIPNEXT as u8 {
        pCur.eState = CURSOR_VALID as u8;
    } else {
        pCur.skipNext = 0;
    }

    let rc = saveCursorKey(pCur);
    if rc == SQLITE_OK {
        btreeReleaseAllCursorPages(pCur);
        pCur.eState = CURSOR_REQUIRESEEK as u8;
    }

    pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl | BTCF_AtLast) as u8);
    rc
}

/// Save the positions of all cursors (except pExcept) that are open on
/// the table with root-page iRoot.  "Saving the cursor position" means that
/// the location in the btree is remembered in such a way that it can be
/// moved back to the same spot after the btree has been modified.  This
/// routine is called just before cursor pExcept is used to modify the
/// table, for example in BtreeDelete() or BtreeInsert().
///
/// If there are two or more cursors on the same btree, then all such
/// cursors should have their BTCF_Multiple flag set.  The btreeCursor()
/// routine enforces that rule.  This routine only needs to be called in
/// the uncommon case when pExpect has the BTCF_Multiple flag set.
///
/// If pExpect!=NULL and if no other cursors are found on the same root-page,
/// then the BTCF_Multiple flag on pExpect is cleared, to avoid another
/// pointless call to this routine.
///
/// Implementation note:  This routine merely checks to see if any cursors
/// need to be saved.  It calls out to saveCursorsOnList() in the (unusual)
/// event that cursors are in need to being saved.
fn saveAllCursors(pBt: &BtSharedP, iRoot: Pgno, pExcept: Option<&mut BtCursor>) -> i32 {
    let excPtr: *const BtCursor = match &pExcept {
        Some(c) => &**c as *const BtCursor,
        None => std::ptr::null(),
    };
    let list = cursorList(pBt);
    let mut first = None;
    for (k, p) in list.iter().enumerate() {
        if !isSameCursor(p, excPtr) && (0 == iRoot || p.borrow().pgnoRoot == iRoot) {
            first = Some(k);
            break;
        }
    }
    if let Some(k) = first {
        return saveCursorsOnList(&list[k..], iRoot, excPtr);
    }
    if let Some(c) = pExcept {
        c.curFlags &= !(BTCF_Multiple as u8);
    }
    SQLITE_OK
}

/// This helper routine to saveAllCursors does the actual work of saving
/// the cursors if and when a cursor is found that actually requires saving.
/// The common case is that no cursors need to be saved, so this routine is
/// broken out from its caller to avoid unnecessary stack pointer movement.
fn saveCursorsOnList(list: &[BtCursorP], iRoot: Pgno, pExcept: *const BtCursor) -> i32 {
    for p in list {
        if !isSameCursor(p, pExcept) {
            let mut c = p.borrow_mut();
            if 0 == iRoot || c.pgnoRoot == iRoot {
                if c.eState == CURSOR_VALID as u8 || c.eState == CURSOR_SKIPNEXT as u8 {
                    let rc = saveCursorPosition(&mut c);
                    if SQLITE_OK != rc {
                        return rc;
                    }
                } else {
                    btreeReleaseAllCursorPages(&mut c);
                }
            }
        }
    }
    SQLITE_OK
}

/// C: `void sqlite3BtreeClearCursor(BtCursor *pCur)`
///
/// Clear the current cursor position.
pub fn sqlite3BtreeClearCursor(pCur: &BtCursorP) {
    btreeClearCursor(&mut pCur.borrow_mut());
}
fn btreeClearCursor(pCur: &mut BtCursor) {
    pCur.pKey = None;
    pCur.eState = CURSOR_INVALID as u8;
}

/// sqlite3VdbeAllocUnpackedRecord(): an UnpackedRecord with space for
/// nKeyField+1 values.
fn allocUnpackedRecord(pKeyInfo: &KeyInfoRef) -> UnpackedRecord {
    let n = pKeyInfo.borrow().nKeyField as usize + 1;
    UnpackedRecord {
        pKeyInfo: Some(pKeyInfo.clone()),
        aMem: (0..n).map(|_| Mem::default()).collect(),
        nField: n as u16,
        ..Default::default()
    }
}

/// In this version of BtreeMoveto, pKey is a packed index record
/// such as is generated by the OP_MakeRecord opcode.  Unpack the
/// record and then call sqlite3BtreeIndexMoveto() to do the work.
fn btreeMoveto(
    pCur: &mut BtCursor,
    pKey: Option<&[u8]>,
    nKey: i64,
    bias: i32,
    pRes: &mut i32,
) -> i32 {
    let rc;
    if let Some(key) = pKey {
        let pKeyInfo = pCur.pKeyInfo.clone().expect("index cursor without KeyInfo");
        debug_assert!(nKey == nKey as i32 as i64);
        let mut pIdxKey = allocUnpackedRecord(&pKeyInfo);
        crate::vdbeaux::sqlite3VdbeRecordUnpack(nKey as i32, key, &mut pIdxKey);
        if pIdxKey.nField == 0 || pIdxKey.nField > pKeyInfo.borrow().nAllField {
            rc = SQLITE_CORRUPT_BKPT();
        } else {
            rc = btreeIndexMoveto(pCur, &mut pIdxKey, pRes);
        }
    } else {
        rc = btreeTableMoveto(pCur, nKey, bias, pRes);
    }
    rc
}

/// Restore the cursor to the position it was in (or as close to as possible)
/// when saveCursorPosition() was called. Note that this call deletes the
/// saved position info stored by saveCursorPosition(), so there can be
/// at most one effective restoreCursorPosition() call after each
/// saveCursorPosition().
fn btreeRestoreCursorPosition(pCur: &mut BtCursor) -> i32 {
    let mut skipNext = 0;
    debug_assert!(pCur.eState >= CURSOR_REQUIRESEEK as u8);
    if pCur.eState == CURSOR_FAULT as u8 {
        return pCur.skipNext;
    }
    pCur.eState = CURSOR_INVALID as u8;
    let pKey = pCur.pKey.take();
    let nKey = pCur.nKey;
    let rc = btreeMoveto(pCur, pKey.as_deref(), nKey, 0, &mut skipNext);
    if rc == SQLITE_OK {
        pCur.pKey = None;
        debug_assert!(pCur.eState == CURSOR_VALID as u8 || pCur.eState == CURSOR_INVALID as u8);
        if skipNext != 0 {
            pCur.skipNext = skipNext;
        }
        if pCur.skipNext != 0 && pCur.eState == CURSOR_VALID as u8 {
            pCur.eState = CURSOR_SKIPNEXT as u8;
        }
    } else {
        pCur.pKey = pKey;
    }
    rc
}

/// restoreCursorPosition(p)
#[inline]
fn restoreCursorPosition(p: &mut BtCursor) -> i32 {
    if p.eState >= CURSOR_REQUIRESEEK as u8 {
        btreeRestoreCursorPosition(p)
    } else {
        SQLITE_OK
    }
}

/// C: `int sqlite3BtreeCursorHasMoved(BtCursor *pCur)`
///
/// Determine whether or not a cursor has moved from the position where
/// it was last placed, or has been invalidated for any other reason.
/// Cursors can move when the row they are pointing at is deleted out
/// from under them, for example.  Cursor might also move if a btree
/// is rebalanced.
///
/// Use the separate sqlite3BtreeCursorRestore() routine to restore a cursor
/// back to where it ought to be if this routine returns true.
pub fn sqlite3BtreeCursorHasMoved(pCur: &BtCursorP) -> i32 {
    (CURSOR_VALID as u8 != pCur.borrow().eState) as i32
}

/// C: `BtCursor * sqlite3BtreeFakeValidCursor(void)`
///
/// Return a pointer to a fake BtCursor object that will always answer
/// false to the sqlite3BtreeCursorHasMoved() routine above.  The fake
/// cursor returned must not be used with any other Btree interface.
pub fn sqlite3BtreeFakeValidCursor() -> Option<BtCursorP> {
    let c = BtCursor {
        eState: CURSOR_VALID as u8,
        iPage: -1,
        ..Default::default()
    };
    Some(Rc::new(RefCell::new(c)))
}

/// C: `int sqlite3BtreeCursorRestore(BtCursor *pCur, int *pDifferentRow)`
///
/// This routine restores a cursor back to its original position after it
/// has been moved by some outside activity (such as a btree rebalance or
/// a row having been deleted out from under the cursor).
///
/// On success, the *pDifferentRow parameter is false if the cursor is left
/// pointing at exactly the same row.  *pDifferntRow is the row the cursor
/// was pointing to has been deleted, forcing the cursor to point to some
/// nearby row.
///
/// This routine should only be called for a cursor that just returned
/// TRUE from sqlite3BtreeCursorHasMoved().
pub fn sqlite3BtreeCursorRestore(pCur: &BtCursorP, pDifferentRow: &mut i32) -> i32 {
    let mut cur = pCur.borrow_mut();
    debug_assert!(cur.eState != CURSOR_VALID as u8);
    let rc = restoreCursorPosition(&mut cur);
    if rc != 0 {
        *pDifferentRow = 1;
        return rc;
    }
    if cur.eState != CURSOR_VALID as u8 {
        *pDifferentRow = 1;
    } else {
        *pDifferentRow = 0;
    }
    SQLITE_OK
}

/// C: `void sqlite3BtreeCursorHint(BtCursor *pCur, int eHintType, ...)`
///
/// Only compiled with SQLITE_ENABLE_CURSOR_HINTS (not in this build) and
/// only used by systems that substitute their own storage engine: no-op.
pub fn sqlite3BtreeCursorHint(pCur: &BtCursorP, eHintType: i32, args: &[crate::printf::PfArg]) {}

/// C: `BtCursor * sqlite3BtreeCursorHintTblCsr(BtCursor *pCsr)`
///
/// SQLITE_ENABLE_CURSOR_HINTS+SQLITE_DEBUG only: always NULL here.
pub fn sqlite3BtreeCursorHintTblCsr(pCsr: &BtCursorP) -> Option<BtCursorP> {
    None
}

/// C: `void sqlite3BtreeCursorHintFlags(BtCursor *pCur, unsigned x)`
///
/// Provide flag hints to the cursor.
pub fn sqlite3BtreeCursorHintFlags(pCur: &BtCursorP, x: u32) {
    debug_assert!(x == BTREE_SEEK_EQ as u32 || x == BTREE_BULKLOAD as u32 || x == 0);
    pCur.borrow_mut().hints = x as u8;
}

// ---------------------------------------------------------------------------
// Pointer map
// ---------------------------------------------------------------------------

/// Given a page number of a regular database page, return the page
/// number for the pointer-map page that contains the entry for the
/// input page number.
///
/// Return 0 (not a valid page) for pgno==1 since there is
/// no pointer map associated with page 1.  The integrity_check logic
/// requires that ptrmapPageno(*,1)!=1.
fn ptrmapPageno(pBt: &BtShared, pgno: Pgno) -> Pgno {
    if pgno < 2 {
        return 0;
    }
    let nPagesPerMapPage = (pBt.usableSize / 5) + 1;
    let iPtrMap = (pgno - 2) / nPagesPerMapPage;
    let mut ret = (iPtrMap * nPagesPerMapPage) + 2;
    if ret == PENDING_BYTE_PAGE(pBt.pageSize) {
        ret += 1;
    }
    ret
}

/// PTRMAP_PAGENO(pBt, pgno)
#[inline]
fn PTRMAP_PAGENO(pBt: &BtSharedP, pgno: Pgno) -> Pgno {
    ptrmapPageno(&pBt.borrow(), pgno)
}
/// PTRMAP_PTROFFSET(pgptrmap, pgno)
#[inline]
fn PTRMAP_PTROFFSET(pgptrmap: Pgno, pgno: Pgno) -> i64 {
    5 * (pgno as i64 - pgptrmap as i64 - 1)
}
/// PTRMAP_ISPAGE(pBt, pgno)
#[inline]
fn PTRMAP_ISPAGE(pBt: &BtSharedP, pgno: Pgno) -> bool {
    PTRMAP_PAGENO(pBt, pgno) == pgno
}
/// PENDING_BYTE_PAGE(pBt) for a BtSharedP
#[inline]
fn pendingBytePage(pBt: &BtSharedP) -> Pgno {
    PENDING_BYTE_PAGE(pBt.borrow().pageSize)
}

/// sqlite3PagerGet() on the pager of pBt.
#[inline]
fn pagerGet(pBt: &BtSharedP, pgno: Pgno, ppPage: &mut Option<DbPage>, flags: i32) -> i32 {
    let pager = pagerOf(pBt);
    let rc = sqlite3PagerGet(&mut pager.borrow_mut(), pgno, ppPage, flags);
    rc
}

/// Write an entry into the pointer map.
///
/// This routine updates the pointer map entry for page number 'key'
/// so that it maps to type 'eType' and parent page number 'pgno'.
///
/// If *pRC is initially non-zero (non-SQLITE_OK) then this routine is
/// a no-op.  If an error occurs, the appropriate error code is written
/// into *pRC.
fn ptrmapPut(pBt: &BtSharedP, key: Pgno, eType: u8, parent: Pgno, pRC: &mut i32) {
    if *pRC != 0 {
        return;
    }
    debug_assert!(pBt.borrow().autoVacuum != 0);
    if key == 0 {
        *pRC = SQLITE_CORRUPT_BKPT();
        return;
    }
    let iPtrmap = PTRMAP_PAGENO(pBt, key);
    let mut pDbPage = None;
    let rc = pagerGet(pBt, iPtrmap, &mut pDbPage, 0);
    if rc != SQLITE_OK {
        *pRC = rc;
        return;
    }
    let pDbPage = pDbPage.unwrap();
    'exit: {
        if pDbPage.borrow().pExtra.isInit != 0 {
            /* The first byte of the extra data is the MemPage.isInit byte.
             ** If that byte is set, it means this page is also being used
             ** as a btree page. */
            *pRC = SQLITE_CORRUPT_BKPT();
            break 'exit;
        }
        let offset = PTRMAP_PTROFFSET(iPtrmap, key);
        if offset < 0 {
            *pRC = SQLITE_CORRUPT_BKPT();
            break 'exit;
        }
        let offset = offset as usize;
        let differs = {
            let g = pDbPage.borrow();
            eType != g.aData[offset] || get4byte(&g.aData, offset + 1) != parent
        };
        if differs {
            let rc = sqlite3PagerWrite(&pDbPage);
            *pRC = rc;
            if rc == SQLITE_OK {
                let mut g = pDbPage.borrow_mut();
                g.aData[offset] = eType;
                put4byte(&mut g.aData, offset + 1, parent);
            }
        }
    }
    sqlite3PagerUnref(&pDbPage);
}

/// Read an entry from the pointer map.
///
/// This routine retrieves the pointer map entry for page 'key', writing
/// the type and parent page number to *pEType and *pPgno respectively.
/// An error code is returned if something goes wrong, otherwise SQLITE_OK.
fn ptrmapGet(pBt: &BtSharedP, key: Pgno, pEType: &mut u8, pPgno: Option<&mut Pgno>) -> i32 {
    let iPtrmap = PTRMAP_PAGENO(pBt, key);
    let mut pDbPage = None;
    let rc = pagerGet(pBt, iPtrmap, &mut pDbPage, 0);
    if rc != 0 {
        return rc;
    }
    let pDbPage = pDbPage.unwrap();
    let offset = PTRMAP_PTROFFSET(iPtrmap, key);
    if offset < 0 {
        sqlite3PagerUnref(&pDbPage);
        return SQLITE_CORRUPT_BKPT();
    }
    let offset = offset as usize;
    {
        let g = pDbPage.borrow();
        *pEType = at(&g.aData, offset);
        if let Some(p) = pPgno {
            *p = get4byte(&g.aData, offset + 1);
        }
    }
    sqlite3PagerUnref(&pDbPage);
    if *pEType < 1 || *pEType > 5 {
        return SQLITE_CORRUPT_PGNO(iPtrmap);
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Cell parsing
// ---------------------------------------------------------------------------

/// Given a btree page and a cell index (0 means the first cell on
/// the page, 1 means the second cell, and so forth) return the offset
/// of the cell content in aData.
///
/// This routine works only for pages that do not contain overflow cells.
#[inline]
fn findCell(p: &MemPage, a: &[u8], i: usize) -> usize {
    (p.maskPage as usize) & get2byteAligned(a, p.aCellIdx() + 2 * i)
}
/// findCellPastPtr() does the same except it skips past the initial
/// 4-byte child pointer found on interior pages, if there is one.
#[inline]
fn findCellPastPtr(p: &MemPage, a: &[u8], i: usize) -> usize {
    p.aDataOfst() + ((p.maskPage as usize) & get2byteAligned(a, p.aCellIdx() + 2 * i))
}

/// This is common tail processing for btreeParseCellPtr() and
/// btreeParseCellPtrIndex() for the case when the cell does not fit entirely
/// on a single B-tree page.  Make necessary adjustments to the CellInfo
/// structure.
fn btreeParseCellAdjustSizeForOverflow(pPage: &MemPage, iCell: usize, pInfo: &mut CellInfo) {
    /* If the payload will not fit completely on the local page, we have
     ** to decide how much to store locally and how much to spill onto
     ** overflow pages.  The strategy is to minimize the amount of unused
     ** space on overflow pages while keeping the amount of local storage
     ** in between minLocal and maxLocal.
     **
     ** Warning:  changing the way overflow payload is distributed in any
     ** way will result in an incompatible file format.
     */
    let minLocal = pPage.minLocal as u32;
    let maxLocal = pPage.maxLocal as u32;
    let surplus =
        minLocal.wrapping_add(pInfo.nPayload.wrapping_sub(minLocal) % (pPage.usableSize - 4));
    if surplus <= maxLocal {
        pInfo.nLocal = surplus as u16;
    } else {
        pInfo.nLocal = minLocal as u16;
    }
    pInfo.nSize = ((pInfo.pPayload + pInfo.nLocal as usize - iCell) as u16).wrapping_add(4);
}

/// Given a record with nPayload bytes of payload stored within btree
/// page pPage, return the number of bytes of payload stored locally.
fn btreePayloadToLocal(pPage: &MemPage, nPayload: i64) -> i32 {
    let maxLocal = pPage.maxLocal as i64;
    debug_assert!(nPayload >= 0);
    if nPayload <= maxLocal {
        nPayload as i32
    } else {
        let minLocal = pPage.minLocal as i64;
        let surplus = minLocal + (nPayload - minLocal) % (pPage.usableSize as i64 - 4);
        if surplus <= maxLocal {
            surplus as i32
        } else {
            minLocal as i32
        }
    }
}

/// The following routines are implementations of the MemPage.xParseCell()
/// method.
///
/// Parse a cell content block and fill in the CellInfo structure.
///
/// btreeParseCellPtr()        =>   table btree leaf nodes
/// btreeParseCellNoPayload()  =>   table btree internal nodes
/// btreeParseCellPtrIndex()   =>   index btree nodes
///
/// There is also a wrapper function btreeParseCell() that works for
/// all MemPage types and that references the cell by index rather than
/// by pointer.
fn btreeParseCellPtrNoPayload(pPage: &MemPage, a: &[u8], iCell: usize, pInfo: &mut CellInfo) {
    debug_assert!(pPage.leaf == 0);
    debug_assert!(pPage.childPtrSize == 4);
    let mut k = 0u64;
    pInfo.nSize = 4 + getVarint(a, iCell + 4, &mut k) as u16;
    pInfo.nKey = k as i64;
    pInfo.nPayload = 0;
    pInfo.nLocal = 0;
    pInfo.pPayload = 0;
}

/// Inlined getVarint32() loop used by the cell parsers: returns the value
/// and advances *pIter past it (at most 9 bytes).
#[inline]
fn parseVarint32(a: &[u8], pIter: &mut usize) -> u64 {
    let mut n = at(a, *pIter) as u64;
    if n >= 0x80 {
        let end = *pIter + 8;
        n &= 0x7f;
        loop {
            *pIter += 1;
            n = (n << 7) | (at(a, *pIter) & 0x7f) as u64;
            if !(at(a, *pIter) >= 0x80 && *pIter < end) {
                break;
            }
        }
    }
    *pIter += 1;
    n
}

fn btreeParseCellPtr(pPage: &MemPage, a: &[u8], iCell: usize, pInfo: &mut CellInfo) {
    debug_assert!(pPage.leaf == 0 || pPage.leaf == 1);
    debug_assert!(pPage.intKeyLeaf != 0);
    debug_assert!(pPage.childPtrSize == 0);
    let mut iter = iCell;

    /* The next block of code is equivalent to:
     **
     **     pIter += getVarint32(pIter, nPayload);
     */
    let nPayload = parseVarint32(a, &mut iter) & 0xffffffff;

    /* The next block of code is equivalent to:
     **
     **     pIter += sqlite3GetVarint(pIter, (u64*)&pInfo->nKey);
     */
    let mut iKey = 0u64;
    iter += getVarint(a, iter, &mut iKey) as usize;

    pInfo.nKey = iKey as i64;
    pInfo.nPayload = nPayload as u32;
    pInfo.pPayload = iter;
    debug_assert!(pPage.maxLocal as i32 <= BT_MAX_LOCAL);
    if nPayload <= pPage.maxLocal as u64 {
        /* This is the (easy) common case where the entire payload fits
         ** on the local page.  No overflow is required.
         */
        pInfo.nSize = (nPayload as u16).wrapping_add((iter - iCell) as u16);
        if pInfo.nSize < 4 {
            pInfo.nSize = 4;
        }
        pInfo.nLocal = nPayload as u16;
    } else {
        btreeParseCellAdjustSizeForOverflow(pPage, iCell, pInfo);
    }
}

fn btreeParseCellPtrIndex(pPage: &MemPage, a: &[u8], iCell: usize, pInfo: &mut CellInfo) {
    debug_assert!(pPage.leaf == 0 || pPage.leaf == 1);
    debug_assert!(pPage.intKeyLeaf == 0);
    let mut iter = iCell + pPage.childPtrSize as usize;
    let nPayload = parseVarint32(a, &mut iter) as u32;
    pInfo.nKey = nPayload as i64;
    pInfo.nPayload = nPayload;
    pInfo.pPayload = iter;
    debug_assert!(pPage.maxLocal as i32 <= BT_MAX_LOCAL);
    if nPayload <= pPage.maxLocal as u32 {
        /* This is the (easy) common case where the entire payload fits
         ** on the local page.  No overflow is required.
         */
        pInfo.nSize = (nPayload as u16).wrapping_add((iter - iCell) as u16);
        if pInfo.nSize < 4 {
            pInfo.nSize = 4;
        }
        pInfo.nLocal = nPayload as u16;
    } else {
        btreeParseCellAdjustSizeForOverflow(pPage, iCell, pInfo);
    }
}

/// btreeParseCell(): parse the iCell-th cell of the page.
#[inline]
fn btreeParseCell(pg: &PgHdr, iCell: usize, pInfo: &mut CellInfo) {
    let p = &pg.pExtra;
    (p.xParseCell)(p, &pg.aData, findCell(p, &pg.aData, iCell), pInfo);
}

/// The following routines are implementations of the MemPage.xCellSize
/// method.
///
/// Compute the total number of bytes that a Cell needs in the cell
/// data area of the btree-page.  The return number includes the cell
/// data header and the local payload, but not any overflow page or
/// the space used by the cell pointer.
///
/// cellSizePtrNoPayload()    =>   table internal nodes
/// cellSizePtrTableLeaf()    =>   table leaf nodes
/// cellSizePtr()             =>   index internal nodes
/// cellSizeIdxLeaf()         =>   index leaf nodes
fn cellSizePtr(pPage: &MemPage, a: &[u8], iCell: usize) -> u16 {
    let mut iter = iCell + 4;
    debug_assert!(pPage.childPtrSize == 4);
    let mut nSize = parseVarint32(a, &mut iter) as u32;
    if nSize <= pPage.maxLocal as u32 {
        nSize = nSize.wrapping_add((iter - iCell) as u32);
    } else {
        let minLocal = pPage.minLocal as u32;
        nSize = minLocal.wrapping_add(nSize.wrapping_sub(minLocal) % (pPage.usableSize - 4));
        if nSize > pPage.maxLocal as u32 {
            nSize = minLocal;
        }
        nSize = nSize.wrapping_add(4 + (iter - iCell) as u16 as u32);
    }
    nSize as u16
}
fn cellSizePtrIdxLeaf(pPage: &MemPage, a: &[u8], iCell: usize) -> u16 {
    let mut iter = iCell;
    debug_assert!(pPage.childPtrSize == 0);
    let mut nSize = parseVarint32(a, &mut iter) as u32;
    if nSize <= pPage.maxLocal as u32 {
        nSize = nSize.wrapping_add((iter - iCell) as u32);
        if nSize < 4 {
            nSize = 4;
        }
    } else {
        let minLocal = pPage.minLocal as u32;
        nSize = minLocal.wrapping_add(nSize.wrapping_sub(minLocal) % (pPage.usableSize - 4));
        if nSize > pPage.maxLocal as u32 {
            nSize = minLocal;
        }
        nSize = nSize.wrapping_add(4 + (iter - iCell) as u16 as u32);
    }
    nSize as u16
}
fn cellSizePtrNoPayload(pPage: &MemPage, a: &[u8], iCell: usize) -> u16 {
    debug_assert!(pPage.childPtrSize == 4);
    let mut iter = iCell + 4;
    let end = iter + 9;
    loop {
        let b = at(a, iter);
        iter += 1;
        if !((b & 0x80) != 0 && iter < end) {
            break;
        }
    }
    (iter - iCell) as u16
}
fn cellSizePtrTableLeaf(pPage: &MemPage, a: &[u8], iCell: usize) -> u16 {
    let mut iter = iCell;
    let mut nSize = parseVarint32(a, &mut iter) as u32;
    /* pIter now points at the 64-bit integer key value, a variable length
     ** integer. The following block moves pIter to point at the first byte
     ** past the end of the key value. */
    let mut k = 0;
    loop {
        let b = at(a, iter);
        iter += 1;
        k += 1;
        if b & 0x80 == 0 {
            break;
        }
        if k == 8 {
            iter += 1;
            break;
        }
    }
    if nSize <= pPage.maxLocal as u32 {
        nSize = nSize.wrapping_add((iter - iCell) as u32);
        if nSize < 4 {
            nSize = 4;
        }
    } else {
        let minLocal = pPage.minLocal as u32;
        nSize = minLocal.wrapping_add(nSize.wrapping_sub(minLocal) % (pPage.usableSize - 4));
        if nSize > pPage.maxLocal as u32 {
            nSize = minLocal;
        }
        nSize = nSize.wrapping_add(4 + (iter - iCell) as u16 as u32);
    }
    nSize as u16
}

/// cellSize(pPage, iCell)
#[inline]
fn cellSize(pg: &PgHdr, iCell: usize) -> u16 {
    let p = &pg.pExtra;
    (p.xCellSize)(p, &pg.aData, findCell(p, &pg.aData, iCell))
}

/// The cell at a[iCell..] is currently part of page pSrc but will
/// ultimately be part of pPage.  (pSrc and pPage are often the same.)  If
/// pCell contains a pointer to an overflow page, insert an entry into the
/// pointer-map for the overflow page that will be valid after pCell has
/// been moved to pPage.
///
/// `srcEnd` is Some(offset of pSrc->aDataEnd in `a`) when the buffer `a`
/// is the image of page pSrc, None when the cell lives in a separate
/// buffer.  No page borrow may be held by the caller (ptrmapPut() fetches
/// a page).
fn ptrmapPutOvflPtr(pPage: &MemPage, a: &[u8], iCell: usize, srcEnd: Option<usize>, pRC: &mut i32) {
    if *pRC != 0 {
        return;
    }
    let mut info = CellInfo::default();
    (pPage.xParseCell)(pPage, a, iCell, &mut info);
    if (info.nLocal as u32) < info.nPayload {
        if let Some(end) = srcEnd {
            /* SQLITE_OVERFLOW(pSrc->aDataEnd, pCell, pCell+info.nLocal) */
            if iCell < end && end < iCell + info.nLocal as usize {
                *pRC = SQLITE_CORRUPT_BKPT();
                return;
            }
        }
        let ovfl = get4byte(a, iCell + info.nSize as usize - 4);
        let pBt = pPage.pBt.upgrade().expect("page without BtShared");
        ptrmapPut(&pBt, ovfl, PTRMAP_OVERFLOW1 as u8, pPage.pgno, pRC);
    }
}

/// ptrmapPutOvflPtr() for the iCell-th cell of page pg (pPage==pSrc).
fn ptrmapPutOvflPtrOfCell(pg: &DbPage, iCell: usize, pRC: &mut i32) {
    if *pRC != 0 {
        return;
    }
    let (p, cell, end) = {
        let g = pg.borrow();
        let off = findCell(&g.pExtra, &g.aData, iCell);
        let sz = cellSize(&g, iCell) as usize;
        let e = std::cmp::min(off + sz, g.aData.len());
        (
            g.pExtra.lite(),
            g.aData[off..e].to_vec(),
            g.pExtra.aDataEnd() - off,
        )
    };
    ptrmapPutOvflPtr(&p, &cell, 0, Some(end), pRC);
}

// ---------------------------------------------------------------------------
// Page content management
// ---------------------------------------------------------------------------

/// Defragment the page given. This routine reorganizes cells within the
/// page so that there are no free-blocks on the free-block list.
///
/// Parameter nMaxFrag is the maximum amount of fragmented space that may be
/// present in the page after this routine returns.
///
/// EVIDENCE-OF: R-44582-60138 SQLite may from time to time reorganize a
/// b-tree page so that there are no freeblocks or fragment bytes, all
/// unused bytes are contained in the unallocated space region, and all
/// cells are packed tightly at the end of the page.
fn defragmentPage(pg: &mut PgHdr, nMaxFrag: i32) -> i32 {
    let PgHdr {
        aData: data,
        pExtra: pPage,
        ..
    } = pg;
    debug_assert!(pPage.nOverflow == 0);
    let hdr = pPage.hdrOffset as usize;
    let cellOffset = pPage.cellOffset as usize;
    let nCell = pPage.nCell as usize;
    debug_assert!(nCell == get2byte(data, hdr + 3));
    let iCellFirst = cellOffset + 2 * nCell;
    let usableSize = pPage.usableSize as usize;
    let mut cbrk: usize;

    'out: {
        /* This block handles pages with two or fewer free blocks and nMaxFrag
         ** or fewer fragmented bytes. In this case it is faster to move the
         ** two (or one) blocks of cells using memmove() and add the required
         ** offsets to each pointer in the cell-pointer array than it is to
         ** reconstruct the entire page.  */
        if data[hdr + 7] as i32 <= nMaxFrag {
            let iFree = get2byte(data, hdr + 1);
            if iFree > usableSize - 4 {
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            if iFree != 0 {
                let iFree2 = get2byte(data, iFree);
                if iFree2 > usableSize - 4 {
                    return SQLITE_CORRUPT_PAGE(pPage);
                }
                if 0 == iFree2 || (data[iFree2] == 0 && data[iFree2 + 1] == 0) {
                    let pEnd = cellOffset + nCell * 2;
                    let mut sz2 = 0usize;
                    let mut sz = get2byte(data, iFree + 2);
                    let top = get2byte(data, hdr + 5);
                    if top >= iFree {
                        return SQLITE_CORRUPT_PAGE(pPage);
                    }
                    if iFree2 != 0 {
                        if iFree + sz > iFree2 {
                            return SQLITE_CORRUPT_PAGE(pPage);
                        }
                        sz2 = get2byte(data, iFree2 + 2);
                        if iFree2 + sz2 > usableSize {
                            return SQLITE_CORRUPT_PAGE(pPage);
                        }
                        data.copy_within(iFree + sz..iFree2, iFree + sz + sz2);
                        sz += sz2;
                    } else if iFree + sz > usableSize {
                        return SQLITE_CORRUPT_PAGE(pPage);
                    }

                    cbrk = top + sz;
                    debug_assert!(cbrk + (iFree - top) <= usableSize);
                    data.copy_within(top..iFree, cbrk);
                    let mut pAddr = cellOffset;
                    while pAddr < pEnd {
                        let pc = get2byte(data, pAddr);
                        if pc < iFree {
                            put2byte(data, pAddr, pc + sz);
                        } else if pc < iFree2 {
                            put2byte(data, pAddr, pc + sz2);
                        }
                        pAddr += 2;
                    }
                    break 'out;
                }
            }
        }

        cbrk = usableSize;
        let iCellLast = usableSize - 4;
        let iCellStart = get2byte(data, hdr + 5);
        if nCell > 0 {
            let temp: Vec<u8> = data[..usableSize].to_vec();
            let src = &temp;
            for i in 0..nCell {
                let pAddr = cellOffset + i * 2;
                let pc = get2byte(data, pAddr);
                /* These conditions have already been verified in btreeInitPage()
                 ** if PRAGMA cell_size_check=ON.
                 */
                if pc > iCellLast {
                    return SQLITE_CORRUPT_PAGE(pPage);
                }
                let size = (pPage.xCellSize)(pPage, src, pc) as usize;
                if cbrk < size || cbrk - size < iCellStart || pc + size > usableSize {
                    return SQLITE_CORRUPT_PAGE(pPage);
                }
                cbrk -= size;
                debug_assert!(cbrk + size <= usableSize && cbrk >= iCellStart);
                put2byte(data, pAddr, cbrk);
                data[cbrk..cbrk + size].copy_from_slice(&src[pc..pc + size]);
            }
        }
        data[hdr + 7] = 0;
    }

    /* defragment_out: */
    debug_assert!(pPage.nFree >= 0);
    if data[hdr + 7] as i64 + cbrk as i64 - iCellFirst as i64 != pPage.nFree as i64 {
        return SQLITE_CORRUPT_PAGE(pPage);
    }
    debug_assert!(cbrk >= iCellFirst);
    put2byte(data, hdr + 5, cbrk);
    data[hdr + 1] = 0;
    data[hdr + 2] = 0;
    for b in &mut data[iCellFirst..cbrk] {
        *b = 0;
    }
    SQLITE_OK
}

/// Search the free-list on page pPg for space to store a cell nByte bytes in
/// size. If one can be found, return the offset of the space and remove it
/// from the free-list.
///
/// If no suitable space can be found on the free-list, return None.
///
/// This function may detect corruption within pPg.  If corruption is
/// detected then *pRc is set to SQLITE_CORRUPT and None is returned.
///
/// Slots on the free list that are between 1 and 3 bytes larger than nByte
/// will be ignored if adding the extra space to the fragmentation count
/// causes the fragmentation count to exceed 60.
fn pageFindSlot(pPg: &MemPage, aData: &mut [u8], nByte: i32, pRc: &mut i32) -> Option<usize> {
    let hdr = pPg.hdrOffset as usize;
    let mut iAddr = hdr + 1;
    let mut pc = get2byte(aData, iAddr) as i32;
    let maxPC = pPg.usableSize as i32 - nByte;

    debug_assert!(pc > 0);
    while pc <= maxPC {
        /* EVIDENCE-OF: R-22710-53328 The third and fourth bytes of each
         ** freeblock form a big-endian integer which is the size of the freeblock
         ** in bytes, including the 4-byte header. */
        let size = get2byte(aData, pc as usize + 2) as i32;
        let x = size - nByte;
        if x >= 0 {
            if x < 4 {
                /* EVIDENCE-OF: R-11498-58022 In a well-formed b-tree page, the total
                 ** number of bytes in fragments may not exceed 60. */
                if aData[hdr + 7] > 57 {
                    return None;
                }

                /* Remove the slot from the free-list. Update the number of
                 ** fragmented bytes within the page. */
                aData[iAddr] = aData[pc as usize];
                aData[iAddr + 1] = aData[pc as usize + 1];
                aData[hdr + 7] = aData[hdr + 7].wrapping_add(x as u8);
                return Some(pc as usize);
            } else if x + pc > maxPC {
                /* This slot extends off the end of the usable part of the page */
                *pRc = SQLITE_CORRUPT_PAGE(pPg);
                return None;
            } else {
                /* The slot remains on the free-list. Reduce its size to account
                 ** for the portion used by the new allocation. */
                put2byte(aData, pc as usize + 2, x as usize);
            }
            return Some((pc + x) as usize);
        }
        iAddr = pc as usize;
        pc = get2byte(aData, pc as usize) as i32;
        if pc <= iAddr as i32 {
            if pc != 0 {
                /* The next slot in the chain comes before the current slot */
                *pRc = SQLITE_CORRUPT_PAGE(pPg);
            }
            return None;
        }
    }
    if pc > maxPC + nByte - 4 {
        /* The free slot chain extends off the end of the page */
        *pRc = SQLITE_CORRUPT_PAGE(pPg);
    }
    None
}

/// Allocate nByte bytes of space from within the B-Tree page passed
/// as the first argument. Write into *pIdx the index into pPage->aData[]
/// of the first byte of allocated space. Return either SQLITE_OK or
/// an error code (usually SQLITE_CORRUPT).
///
/// The caller guarantees that there is sufficient space to make the
/// allocation.  This routine might need to defragment in order to bring
/// all the space together, however.  This routine will avoid using
/// the first two bytes past the cell pointer area since presumably this
/// allocation is being made in order to insert a new cell, so we will
/// also end up needing a new cell pointer.
fn allocateSpace(pg: &mut PgHdr, nByte: i32, pIdx: &mut i32) -> i32 {
    let hdr = pg.pExtra.hdrOffset as usize;
    let mut rc = SQLITE_OK;
    debug_assert!(nByte >= 0);
    debug_assert!(pg.pExtra.nFree >= nByte);
    debug_assert!(pg.pExtra.nOverflow == 0);
    debug_assert!(nByte < pg.pExtra.usableSize as i32 - 8);

    debug_assert!(pg.pExtra.cellOffset as usize == hdr + 12 - 4 * pg.pExtra.leaf as usize);
    let gap = pg.pExtra.cellOffset as i32 + 2 * pg.pExtra.nCell as i32;
    debug_assert!(gap <= 65536);
    /* EVIDENCE-OF: R-29356-02391 If the database uses a 65536-byte page size
     ** and the reserved space is zero (the usual value for reserved space)
     ** then the cell content offset of an empty page wants to be 65536.
     ** However, that integer is too large to be stored in a 2-byte unsigned
     ** integer, so a value of 0 is used in its place. */
    let mut top = get2byte(&pg.aData, hdr + 5) as i32;
    if gap > top {
        if top == 0 && pg.pExtra.usableSize == 65536 {
            top = 65536;
        } else {
            return SQLITE_CORRUPT_PAGE(&pg.pExtra);
        }
    } else if top > pg.pExtra.usableSize as i32 {
        return SQLITE_CORRUPT_PAGE(&pg.pExtra);
    }

    /* If there is enough space between gap and top for one more cell pointer,
     ** and if the freelist is not empty, then search the
     ** freelist looking for a slot big enough to satisfy the request.
     */
    if (pg.aData[hdr + 2] != 0 || pg.aData[hdr + 1] != 0) && gap + 2 <= top {
        let PgHdr {
            aData, pExtra: p, ..
        } = &mut *pg;
        let pSpace = pageFindSlot(p, aData, nByte, &mut rc);
        if let Some(g2) = pSpace {
            debug_assert!(g2 + nByte as usize <= p.usableSize as usize);
            *pIdx = g2 as i32;
            if g2 as i32 <= gap {
                return SQLITE_CORRUPT_PAGE(p);
            } else {
                return SQLITE_OK;
            }
        } else if rc != 0 {
            return rc;
        }
    }

    /* The request could not be fulfilled using a freelist slot.  Check
     ** to see if defragmentation is necessary.
     */
    if gap + 2 + nByte > top {
        debug_assert!(pg.pExtra.nCell > 0);
        debug_assert!(pg.pExtra.nFree >= 0);
        let nMaxFrag = std::cmp::min(4, pg.pExtra.nFree - (2 + nByte));
        rc = defragmentPage(pg, nMaxFrag);
        if rc != 0 {
            return rc;
        }
        top = get2byteNotZero(&pg.aData, hdr + 5) as i32;
        debug_assert!(gap + 2 + nByte <= top);
    }

    /* Allocate memory from the gap in between the cell pointer array
     ** and the cell content area.  The btreeComputeFreeSpace() call has already
     ** validated the freelist.  Given that the freelist is valid, there
     ** is no way that the allocation can extend off the end of the page.
     ** The assert() below verifies the previous sentence.
     */
    top -= nByte;
    put2byte(&mut pg.aData, hdr + 5, top as usize);
    debug_assert!(top + nByte <= pg.pExtra.usableSize as i32);
    *pIdx = top;
    SQLITE_OK
}

/// Return a section of the pPage->aData to the freelist.
/// The first byte of the new free block is pPage->aData[iStart]
/// and the size of the block is iSize bytes.
///
/// Adjacent freeblocks are coalesced.
///
/// Even though the freeblock list was checked by btreeComputeFreeSpace(),
/// that routine will not detect overlap between cells or freeblocks.  Nor
/// does it detect cells or freeblocks that encroach into the reserved bytes
/// at the end of the page.  So do additional corruption checks inside this
/// routine and return SQLITE_CORRUPT if any problems are found.
fn freeSpace(pg: &mut PgHdr, iStart: i32, iSize: i32) -> i32 {
    let PgHdr {
        aData: data,
        pExtra: pPage,
        ..
    } = pg;
    let mut iStart = iStart as usize;
    let mut iSize = iSize as usize;
    let iOrigSize = iSize;
    let mut iEnd = iStart + iSize;
    let mut nFrag: usize = 0;
    let usableSize = pPage.usableSize as usize;
    let mut iFreeBlk: usize;

    debug_assert!(iSize >= 4); /* Minimum cell size is 4 */

    /* The list of freeblocks must be in ascending order.  Find the
     ** spot on the list where iStart should be inserted.
     */
    let hdr = pPage.hdrOffset as usize;
    let mut iPtr = hdr + 1;
    if data[iPtr + 1] == 0 && data[iPtr] == 0 {
        iFreeBlk = 0; /* Shortcut for the case when the freelist is empty */
    } else {
        loop {
            iFreeBlk = get2byte(data, iPtr);
            if iFreeBlk >= iStart {
                break;
            }
            if iFreeBlk <= iPtr {
                if iFreeBlk == 0 {
                    break;
                } /* TH3: corrupt082.100 */
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            iPtr = iFreeBlk;
        }
        if iFreeBlk > usableSize - 4 {
            /* TH3: corrupt081.100 */
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        debug_assert!(iFreeBlk > iPtr || iFreeBlk == 0);

        /* At this point:
         **    iFreeBlk:   First freeblock after iStart, or zero if none
         **    iPtr:       The address of a pointer to iFreeBlk
         **
         ** Check to see if iFreeBlk should be coalesced onto the end of iStart.
         */
        if iFreeBlk != 0 && iEnd + 3 >= iFreeBlk {
            if iEnd > iFreeBlk {
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            nFrag = iFreeBlk - iEnd;
            iEnd = iFreeBlk + get2byte(data, iFreeBlk + 2);
            if iEnd > usableSize {
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            iSize = iEnd - iStart;
            iFreeBlk = get2byte(data, iFreeBlk);
        }

        /* If iPtr is another freeblock (that is, if iPtr is not the freelist
         ** pointer in the page header) then check to see if iStart should be
         ** coalesced onto the end of iPtr.
         */
        if iPtr > hdr + 1 {
            let iPtrEnd = iPtr + get2byte(data, iPtr + 2);
            if iPtrEnd + 3 >= iStart {
                if iPtrEnd > iStart {
                    return SQLITE_CORRUPT_PAGE(pPage);
                }
                nFrag += iStart - iPtrEnd;
                iSize = iEnd - iPtr;
                iStart = iPtr;
            }
        }
        if nFrag > data[hdr + 7] as usize {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        data[hdr + 7] -= nFrag as u8;
    }
    let x = get2byte(data, hdr + 5);
    if pPage.btsFlags() as i32 & BTS_FAST_SECURE != 0 {
        /* Overwrite deleted information with zeros when the secure_delete
         ** option is enabled */
        for b in &mut data[iStart..iStart + iSize] {
            *b = 0;
        }
    }
    if iStart <= x {
        /* The new freeblock is at the beginning of the cell content area,
         ** so just extend the cell content area rather than create another
         ** freelist entry */
        if iStart < x {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        if iPtr != hdr + 1 {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        put2byte(data, hdr + 1, iFreeBlk);
        put2byte(data, hdr + 5, iEnd);
    } else {
        /* Insert the new freeblock into the freelist */
        put2byte(data, iPtr, iStart);
        put2byte(data, iStart, iFreeBlk);
        debug_assert!(iSize <= 0xffff);
        put2byte(data, iStart + 2, iSize);
    }
    pPage.nFree += iOrigSize as i32;
    SQLITE_OK
}

/// BtShared values used when (re)initializing a page.
#[derive(Clone, Copy)]
struct BtParams {
    pageSize: u32,
    usableSize: u32,
    maxLocal: u16,
    minLocal: u16,
    maxLeaf: u16,
    minLeaf: u16,
    max1bytePayload: u8,
    btsFlags: u16,
    dbFlags: u64,
}

fn btParams(pBt: &BtShared) -> BtParams {
    BtParams {
        pageSize: pBt.pageSize,
        usableSize: pBt.usableSize,
        maxLocal: pBt.maxLocal,
        minLocal: pBt.minLocal,
        maxLeaf: pBt.maxLeaf,
        minLeaf: pBt.minLeaf,
        max1bytePayload: pBt.max1bytePayload,
        btsFlags: pBt.btsFlags,
        dbFlags: pBt.dbFlags,
    }
}

/// The BtParams of the BtShared a page belongs to.
fn pageParams(pPage: &MemPage) -> BtParams {
    let b = pPage.pBt.upgrade().expect("page without BtShared");
    let p = btParams(&b.borrow());
    p
}

/// Decode the flags byte (the first byte of the header) for a page
/// and initialize fields of the MemPage structure accordingly.
///
/// Only the following combinations are supported.  Anything different
/// indicates a corrupt database files:
///
///         PTF_ZERODATA                             (0x02,  2)
///         PTF_LEAFDATA | PTF_INTKEY                (0x05,  5)
///         PTF_ZERODATA | PTF_LEAF                  (0x0a, 10)
///         PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF     (0x0d, 13)
fn decodeFlags(pPage: &mut MemPage, flagByte: i32, pBt: &BtParams) -> i32 {
    debug_assert!(pPage.hdrOffset == if pPage.pgno == 1 { 100 } else { 0 });
    pPage.max1bytePayload = pBt.max1bytePayload;
    pPage.usableSize = pBt.usableSize;
    pPage.pageSize = pBt.pageSize;
    if flagByte >= (PTF_ZERODATA | PTF_LEAF) {
        pPage.childPtrSize = 0;
        pPage.leaf = 1;
        if flagByte == (PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF) {
            pPage.intKeyLeaf = 1;
            pPage.xCellSize = cellSizePtrTableLeaf;
            pPage.xParseCell = btreeParseCellPtr;
            pPage.intKey = 1;
            pPage.maxLocal = pBt.maxLeaf;
            pPage.minLocal = pBt.minLeaf;
        } else if flagByte == (PTF_ZERODATA | PTF_LEAF) {
            pPage.intKey = 0;
            pPage.intKeyLeaf = 0;
            pPage.xCellSize = cellSizePtrIdxLeaf;
            pPage.xParseCell = btreeParseCellPtrIndex;
            pPage.maxLocal = pBt.maxLocal;
            pPage.minLocal = pBt.minLocal;
        } else {
            pPage.intKey = 0;
            pPage.intKeyLeaf = 0;
            pPage.xCellSize = cellSizePtrIdxLeaf;
            pPage.xParseCell = btreeParseCellPtrIndex;
            return SQLITE_CORRUPT_PAGE(pPage);
        }
    } else {
        pPage.childPtrSize = 4;
        pPage.leaf = 0;
        if flagByte == PTF_ZERODATA {
            pPage.intKey = 0;
            pPage.intKeyLeaf = 0;
            pPage.xCellSize = cellSizePtr;
            pPage.xParseCell = btreeParseCellPtrIndex;
            pPage.maxLocal = pBt.maxLocal;
            pPage.minLocal = pBt.minLocal;
        } else if flagByte == (PTF_LEAFDATA | PTF_INTKEY) {
            pPage.intKeyLeaf = 0;
            pPage.xCellSize = cellSizePtrNoPayload;
            pPage.xParseCell = btreeParseCellPtrNoPayload;
            pPage.intKey = 1;
            pPage.maxLocal = pBt.maxLeaf;
            pPage.minLocal = pBt.minLeaf;
        } else {
            pPage.intKey = 0;
            pPage.intKeyLeaf = 0;
            pPage.xCellSize = cellSizePtr;
            pPage.xParseCell = btreeParseCellPtrIndex;
            return SQLITE_CORRUPT_PAGE(pPage);
        }
    }
    SQLITE_OK
}

/// Compute the amount of freespace on the page.  In other words, fill
/// in the pPage->nFree field.
fn btreeComputeFreeSpace(pg: &mut PgHdr) -> i32 {
    let PgHdr {
        aData: data,
        pExtra: pPage,
        ..
    } = pg;
    debug_assert!(pPage.isInit == 1);
    debug_assert!(pPage.nFree < 0);

    let usableSize = pPage.usableSize as i32;
    let hdr = pPage.hdrOffset as usize;
    /* EVIDENCE-OF: R-58015-48175 The two-byte integer at offset 5 designates
     ** the start of the cell content area. A zero value for this integer is
     ** interpreted as 65536. */
    let top = get2byteNotZero(data, hdr + 5) as i32;
    let iCellFirst = hdr as i32 + 8 + pPage.childPtrSize as i32 + 2 * pPage.nCell as i32;
    let iCellLast = usableSize - 4;

    /* Compute the total free space on the page
     ** EVIDENCE-OF: R-23588-34450 The two-byte integer at offset 1 gives the
     ** start of the first freeblock on the page, or is zero if there are no
     ** freeblocks. */
    let mut pc = get2byte(data, hdr + 1) as i32;
    let mut nFree = data[hdr + 7] as i32 + top; /* Init nFree to non-freeblock free space */
    if pc > 0 {
        let mut next: u32;
        let mut size: u32;
        if pc < top {
            /* EVIDENCE-OF: R-55530-52930 In a well-formed b-tree page, there will
             ** always be at least one cell before the first freeblock.
             */
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        loop {
            if pc > iCellLast {
                /* Freeblock off the end of the page */
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            next = get2byte(data, pc as usize) as u32;
            size = get2byte(data, pc as usize + 2) as u32;
            if size < 4 {
                /* Minimum freeblock size is 4. */
                return SQLITE_CORRUPT_PAGE(pPage);
            }
            nFree += size as i32;
            if next < pc as u32 + size + 4 {
                break;
            }
            pc = next as i32;
        }
        if next > 0 {
            /* Freeblock not in ascending order */
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        if pc as u32 + size > usableSize as u32 {
            /* Last freeblock extends past page end */
            return SQLITE_CORRUPT_PAGE(pPage);
        }
    }

    /* At this point, nFree contains the sum of the offset to the start
     ** of the cell-content area plus the number of free bytes within
     ** the cell-content area. If this is greater than the usable-size
     ** of the page, then the page must be corrupted. This check also
     ** serves to verify that the offset to the start of the cell-content
     ** area, according to the page header, lies within the page.
     */
    if nFree > usableSize || nFree < iCellFirst {
        return SQLITE_CORRUPT_PAGE(pPage);
    }
    pPage.nFree = (nFree - iCellFirst) as u16 as i32;
    SQLITE_OK
}

/// btreeComputeFreeSpace() on a DbPage.
#[inline]
fn computeFreeSpace(pg: &DbPage) -> i32 {
    btreeComputeFreeSpace(&mut pg.borrow_mut())
}

/// Do additional sanity check after btreeInitPage() if
/// PRAGMA cell_size_check=ON
fn btreeCellSizeCheck(pg: &PgHdr) -> i32 {
    let pPage = &pg.pExtra;
    let data = &pg.aData;
    let iCellFirst = pPage.cellOffset as usize + 2 * pPage.nCell as usize;
    let usableSize = pPage.usableSize as usize;
    let mut iCellLast = usableSize - 4;
    let cellOffset = pPage.cellOffset as usize;
    if pPage.leaf == 0 {
        iCellLast -= 1;
    }
    for i in 0..pPage.nCell as usize {
        let pc = get2byteAligned(data, cellOffset + i * 2);
        if pc < iCellFirst || pc > iCellLast {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        let sz = (pPage.xCellSize)(pPage, data, pc) as usize;
        if pc + sz > usableSize {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
    }
    SQLITE_OK
}

/// Initialize the auxiliary information for a disk block.
///
/// Return SQLITE_OK on success.  If we see that the page does
/// not contain a well-formed database page, then return
/// SQLITE_CORRUPT.  Note that a return of SQLITE_OK does not
/// guarantee that the page is well-formed.  It only shows that
/// we failed to detect any corruption.
fn btreeInitPage(pg: &mut PgHdr) -> i32 {
    debug_assert!(pg.pExtra.isInit == 0);
    let pBt = pageParams(&pg.pExtra);
    let hdr = pg.pExtra.hdrOffset as usize;
    /* EVIDENCE-OF: R-28594-02890 The one-byte flag at offset 0 indicating
     ** the b-tree page type. */
    let flagByte = pg.aData[hdr] as i32;
    {
        let pPage = &mut pg.pExtra;
        if decodeFlags(pPage, flagByte, &pBt) != 0 {
            return SQLITE_CORRUPT_PAGE(pPage);
        }
        debug_assert!(pBt.pageSize >= 512 && pBt.pageSize <= 65536);
        pPage.maskPage = (pBt.pageSize - 1) as u16;
        pPage.nOverflow = 0;
        pPage.cellOffset = (pPage.hdrOffset as usize + 8 + pPage.childPtrSize as usize) as u16;
    }
    /* EVIDENCE-OF: R-37002-32774 The two-byte integer at offset 3 gives the
     ** number of cells on the page. */
    let nCell = get2byte(&pg.aData, hdr + 3);
    let pPage = &mut pg.pExtra;
    pPage.nCell = nCell as u16;
    if nCell as u32 > MX_CELL(pBt.pageSize) {
        /* To many cells for a single page.  The page must be corrupt */
        return SQLITE_CORRUPT_PAGE(pPage);
    }
    /* EVIDENCE-OF: R-24089-57979 If a page contains no cells (which is only
     ** possible for a root page of a table that contains no rows) then the
     ** offset to the cell content area will equal the page size minus the
     ** bytes of reserved space. */
    pPage.nFree = -1; /* Indicate that this value is yet uncomputed */
    pPage.isInit = 1;
    if pBt.dbFlags & SQLITE_CellSizeCk != 0 {
        return btreeCellSizeCheck(pg);
    }
    SQLITE_OK
}

/// Set up a raw page so that it looks like a database page holding
/// no entries.
fn zeroPage(pg: &mut PgHdr, flags: i32) {
    let PgHdr {
        aData: data,
        pExtra: pPage,
        ..
    } = pg;
    let pBt = pageParams(pPage);
    let hdr = pPage.hdrOffset as usize;
    if pBt.btsFlags as i32 & BTS_FAST_SECURE != 0 {
        for b in &mut data[hdr..pBt.usableSize as usize] {
            *b = 0;
        }
    }
    data[hdr] = flags as u8;
    let first = hdr + if (flags & PTF_LEAF) == 0 { 12 } else { 8 };
    for b in &mut data[hdr + 1..hdr + 5] {
        *b = 0;
    }
    data[hdr + 7] = 0;
    put2byte(data, hdr + 5, pBt.usableSize as usize);
    pPage.nFree = (pBt.usableSize as usize - first) as u16 as i32;
    decodeFlags(pPage, flags, &pBt);
    pPage.cellOffset = first as u16;
    pPage.nOverflow = 0;
    debug_assert!(pBt.pageSize >= 512 && pBt.pageSize <= 65536);
    pPage.maskPage = (pBt.pageSize - 1) as u16;
    pPage.nCell = 0;
    pPage.isInit = 1;
}

/// Convert a DbPage obtained from the pager into a MemPage used by
/// the btree layer.
fn btreePageFromDbPage(pDbPage: &DbPage, pgno: Pgno, pBt: &BtSharedP) {
    let mut g = pDbPage.borrow_mut();
    let pPage = &mut g.pExtra;
    if pgno != pPage.pgno {
        pPage.pBt = Rc::downgrade(pBt);
        pPage.pgno = pgno;
        pPage.hdrOffset = if pgno == 1 { 100 } else { 0 };
        let b = pBt.borrow();
        pPage.usableSize = b.usableSize;
        pPage.pageSize = b.pageSize;
    }
}

/// Get a page from the pager.  Initialize the MemPage.pBt and
/// MemPage.aData elements if needed.  See also: btreeGetUnusedPage().
///
/// If the PAGER_GET_NOCONTENT flag is set, it means that we do not care
/// about the content of the page at this time.  So do not go to the disk
/// to fetch the content.  Just fill in the content with zeros for now.
/// If in the future we call sqlite3PagerWrite() on this page, that
/// means we have started to be concerned about content and the disk
/// read should occur at that point.
fn btreeGetPage(pBt: &BtSharedP, pgno: Pgno, ppPage: &mut Option<DbPage>, flags: i32) -> i32 {
    debug_assert!(flags == 0 || flags == PAGER_GET_NOCONTENT || flags == PAGER_GET_READONLY);
    let mut pDbPage = None;
    let rc = pagerGet(pBt, pgno, &mut pDbPage, flags);
    if rc != 0 {
        return rc;
    }
    let pg = pDbPage.unwrap();
    btreePageFromDbPage(&pg, pgno, pBt);
    *ppPage = Some(pg);
    SQLITE_OK
}

/// Retrieve a page from the pager cache. If the requested page is not
/// already in the pager cache return NULL. Initialize the MemPage.pBt and
/// MemPage.aData elements if needed.
fn btreePageLookup(pBt: &BtSharedP, pgno: Pgno) -> Option<DbPage> {
    let pager = pagerOf(pBt);
    let pDbPage = sqlite3PagerLookup(&mut pager.borrow_mut(), pgno);
    if let Some(pg) = pDbPage {
        btreePageFromDbPage(&pg, pgno, pBt);
        return Some(pg);
    }
    None
}

/// Return the size of the database file in pages. If there is any kind of
/// error, return ((unsigned int)-1).
#[inline]
fn btreePagecount(pBt: &BtSharedP) -> Pgno {
    pBt.borrow().nPage
}

/// C: `Pgno sqlite3BtreeLastPage(Btree *p)`
pub fn sqlite3BtreeLastPage(p: &BtreeP) -> Pgno {
    let pBt = p.borrow().pBt.clone();
    btreePagecount(&pBt)
}

/// Get a page from the pager and initialize it.
fn getAndInitPage(pBt: &BtSharedP, pgno: Pgno, ppPage: &mut Option<DbPage>, bReadOnly: i32) -> i32 {
    if pgno > btreePagecount(pBt) {
        *ppPage = None;
        return SQLITE_CORRUPT_BKPT();
    }
    let mut pDbPage = None;
    let rc = pagerGet(pBt, pgno, &mut pDbPage, bReadOnly);
    if rc != 0 {
        *ppPage = None;
        return rc;
    }
    let pg = pDbPage.unwrap();
    if pg.borrow().pExtra.isInit == 0 {
        btreePageFromDbPage(&pg, pgno, pBt);
        let rc = btreeInitPage(&mut pg.borrow_mut());
        if rc != SQLITE_OK {
            releasePage(Some(&pg));
            *ppPage = None;
            return rc;
        }
    }
    *ppPage = Some(pg);
    SQLITE_OK
}

/// Release a MemPage.  This should be called once for each prior
/// call to btreeGetPage.
///
/// Page1 is a special case and must be released using releasePageOne().
#[inline]
fn releasePageNotNull(pPage: &DbPage) {
    sqlite3PagerUnrefNotNull(pPage);
}
#[inline]
fn releasePage(pPage: Option<&DbPage>) {
    if let Some(p) = pPage {
        releasePageNotNull(p);
    }
}
#[inline]
fn releasePageOne(pPage: &DbPage) {
    sqlite3PagerUnrefPageOne(pPage);
}

/// Get an unused page.
///
/// This works just like btreeGetPage() with the addition:
///
///   *  If the page is already in use for some other purpose, immediately
///      release it and return an SQLITE_CURRUPT error.
///   *  Make sure the isInit flag is clear
fn btreeGetUnusedPage(pBt: &BtSharedP, pgno: Pgno, ppPage: &mut Option<DbPage>, flags: i32) -> i32 {
    let rc = btreeGetPage(pBt, pgno, ppPage, flags);
    if rc == SQLITE_OK {
        let pg = ppPage.clone().unwrap();
        if sqlite3PagerPageRefcount(&pg) > 1 {
            releasePage(Some(&pg));
            *ppPage = None;
            return SQLITE_CORRUPT_BKPT();
        }
        pg.borrow_mut().pExtra.isInit = 0;
    } else {
        *ppPage = None;
    }
    rc
}

/// During a rollback, when the pager reloads information into the cache
/// so that the cache is restored to its original state at the start of
/// the transaction, for each page restored this routine is called.
///
/// This routine needs to reset the extra data section at the end of the
/// page to agree with the restored data.
fn pageReinit(pData: &DbPage) {
    debug_assert!(sqlite3PagerPageRefcount(pData) > 0);
    let isInit = pData.borrow().pExtra.isInit;
    if isInit != 0 {
        pData.borrow_mut().pExtra.isInit = 0;
        if sqlite3PagerPageRefcount(pData) > 1 {
            /* pPage might not be a btree page;  it might be an overflow page
             ** or ptrmap page or a free page.  In those cases, the following
             ** call to btreeInitPage() will likely return SQLITE_CORRUPT.
             ** But no harm is done by this.  And it is very important that
             ** btreeInitPage() be called on every btree page so we make
             ** the call for every page that comes in for re-initializing. */
            btreeInitPage(&mut pData.borrow_mut());
        }
    }
}

// ---------------------------------------------------------------------------
// Opening and closing
// ---------------------------------------------------------------------------

/// The connection state the btree layer reads through `p->db` in C.  The
/// public entry points that take `db` build one; the internal functions
/// only use this (so that the layer can be exercised without a full
/// connection).
pub struct BtDbState {
    /// db->flags
    pub flags: u64,
    /// sqlite3TempInMemory(db)
    pub tempInMemory: i32,
    /// sqlite3WritableSchema(db)
    pub writableSchema: i32,
    /// db->nSavepoint
    pub nSavepoint: i32,
    /// db->nVdbeRead
    pub nVdbeRead: i32,
    /// db->szMmap
    pub szMmap: i64,
    /// db->busyHandler
    pub busyHandler: Rc<RefCell<BusyHandler>>,
}

impl BtDbState {
    pub fn new(db: &mut Sqlite3) -> BtDbState {
        BtDbState {
            flags: db.flags,
            tempInMemory: crate::mainapi::sqlite3TempInMemory(db),
            writableSchema: crate::build::sqlite3WritableSchema(db),
            nSavepoint: db.nSavepoint,
            nVdbeRead: db.nVdbeRead,
            szMmap: db.szMmap,
            busyHandler: db.busyHandler.clone(),
        }
    }
}

/// Invoke the busy handler for a btree.
fn btreeInvokeBusyHandler(db: &BtDbState) -> bool {
    let r = crate::mainapi::sqlite3InvokeBusyHandler(&mut db.busyHandler.borrow_mut());
    r != 0
}

/// C: `int sqlite3BtreeOpen( sqlite3_vfs *pVfs, const char *zFilename, sqlite3 *db, Btree **ppBtree, int flags, int vfsFlags )`
///
/// Open a database file.
///
/// zFilename is the name of the database file.  If zFilename is NULL
/// then an ephemeral database is created.  The ephemeral database might
/// be exclusively in memory, or it might use a disk-based memory cache.
/// Either way, the ephemeral database will be automatically deleted
/// when sqlite3BtreeClose() is called.
///
/// If zFilename is ":memory:" then an in-memory database is created
/// that is automatically destroyed when it is closed.
///
/// The "flags" parameter is a bitmask that might contain bits like
/// BTREE_OMIT_JOURNAL and/or BTREE_MEMORY.
pub fn sqlite3BtreeOpen(
    db: &mut Sqlite3,
    pVfs: &Rc<crate::os::Vfs>,
    zFilename: Option<&[u8]>,
    ppBtree: &mut Option<BtreeP>,
    flags: i32,
    vfsFlags: i32,
) -> i32 {
    let st = BtDbState::new(db);
    btreeOpen(&st, pVfs, zFilename, ppBtree, flags, vfsFlags)
}

/// sqlite3BtreeOpen() body.
pub fn btreeOpen(
    db: &BtDbState,
    pVfs: &Rc<crate::os::Vfs>,
    zFilename: Option<&[u8]>,
    ppBtree: &mut Option<BtreeP>,
    flags: i32,
    vfsFlags: i32,
) -> i32 {
    let mut flags = flags;
    let mut vfsFlags = vfsFlags;
    let mut zDbHeader = [0u8; 100]; /* Database header content */

    /* True if opening an ephemeral, temporary database */
    let isTempDb = match zFilename {
        None => true,
        Some(z) => z.is_empty() || z[0] == 0,
    };

    /* Set the variable isMemdb to true for an in-memory database, or
     ** false for a file-based database.
     */
    let isMemdb = zFilename.map_or(false, |z| {
        let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
        &z[..n] == b":memory:"
    }) || (isTempDb && db.tempInMemory != 0)
        || (vfsFlags & SQLITE_OPEN_MEMORY) != 0;

    debug_assert!((flags & 0xff) == flags); /* flags fit in 8 bits */

    /* Only a BTREE_SINGLE database can be BTREE_UNORDERED */
    debug_assert!((flags & BTREE_UNORDERED) == 0 || (flags & BTREE_SINGLE) != 0);

    /* A BTREE_SINGLE database is always a temporary and/or ephemeral */
    debug_assert!((flags & BTREE_SINGLE) == 0 || isTempDb);

    if isMemdb {
        flags |= BTREE_MEMORY;
    }
    if (vfsFlags & SQLITE_OPEN_MAIN_DB) != 0 && (isMemdb || isTempDb) {
        vfsFlags = (vfsFlags & !SQLITE_OPEN_MAIN_DB) | SQLITE_OPEN_TEMP_DB;
    }

    let mut pPager: Option<PagerP> = None;
    let mut rc = sqlite3PagerOpen(
        pVfs,
        &mut pPager,
        zFilename,
        SIZEOF_MEMPAGE,
        flags,
        vfsFlags,
        Some(pageReinit),
    );
    if rc == SQLITE_OK {
        let pager = pPager.clone().unwrap();
        sqlite3PagerSetMmapLimit(&mut pager.borrow_mut(), db.szMmap);
        rc = sqlite3PagerReadFileheader(
            &mut pager.borrow_mut(),
            zDbHeader.len() as i32,
            &mut zDbHeader,
        );
    }
    if rc != SQLITE_OK {
        if let Some(pager) = pPager {
            sqlite3PagerClose(None, &mut pager.borrow_mut());
        }
        *ppBtree = None;
        return rc;
    }
    let pager = pPager.unwrap();
    let pBt: BtSharedP = Rc::new(RefCell::new(BtShared {
        pPager: pager.clone(),
        pCursor: Vec::new(),
        pPage1: None,
        openFlags: flags as u8,
        autoVacuum: 0,
        incrVacuum: 0,
        bDoTruncate: 0,
        inTransaction: TRANS_NONE as u8,
        max1bytePayload: 0,
        nReserveWanted: 0,
        btsFlags: 0,
        maxLocal: 0,
        minLocal: 0,
        maxLeaf: 0,
        minLeaf: 0,
        pageSize: 0,
        usableSize: 0,
        nTransaction: 0,
        nPage: 0,
        pHasContent: None,
        pTmpSpace: Vec::new(),
        nPreformatSize: 0,
        dbFlags: db.flags,
    }));
    {
        let bh = db.busyHandler.clone();
        let x: Rc<dyn Fn() -> i32> =
            Rc::new(move || crate::mainapi::sqlite3InvokeBusyHandler(&mut bh.borrow_mut()));
        sqlite3PagerSetBusyHandler(&mut pager.borrow_mut(), Some(x));
    }
    let nReserve: u8;
    {
        let ro = sqlite3PagerIsreadonly(&mut pager.borrow_mut());
        let mut bt = pBt.borrow_mut();
        if ro != 0 {
            bt.btsFlags |= BTS_READ_ONLY as u16;
        }
        /* EVIDENCE-OF: R-51873-39618 The page size for a database file is
         ** determined by the 2-byte integer located at an offset of 16 bytes from
         ** the beginning of the database file. */
        bt.pageSize = ((zDbHeader[16] as u32) << 8) | ((zDbHeader[17] as u32) << 16);
        if bt.pageSize < 512
            || bt.pageSize > SQLITE_MAX_PAGE_SIZE as u32
            || ((bt.pageSize - 1) & bt.pageSize) != 0
        {
            bt.pageSize = 0;
            /* If the magic name ":memory:" will create an in-memory database, then
             ** leave the autoVacuum mode at 0 (do not auto-vacuum), even if
             ** SQLITE_DEFAULT_AUTOVACUUM is true. On the other hand, if
             ** SQLITE_OMIT_MEMORYDB has been defined, then ":memory:" is just a
             ** regular file-name. In this case the auto-vacuum applies as per normal.
             */
            if zFilename.is_some() && !isMemdb {
                bt.autoVacuum = if SQLITE_DEFAULT_AUTOVACUUM != 0 { 1 } else { 0 };
                bt.incrVacuum = if SQLITE_DEFAULT_AUTOVACUUM == 2 { 1 } else { 0 };
            }
            nReserve = 0;
        } else {
            /* EVIDENCE-OF: R-37497-42412 The size of the reserved region is
             ** determined by the one-byte unsigned integer found at an offset of 20
             ** into the database file header. */
            nReserve = zDbHeader[20];
            bt.btsFlags |= BTS_PAGESIZE_FIXED as u16;
            bt.autoVacuum = if get4byte(&zDbHeader, 36 + 4 * 4) != 0 {
                1
            } else {
                0
            };
            bt.incrVacuum = if get4byte(&zDbHeader, 36 + 7 * 4) != 0 {
                1
            } else {
                0
            };
        }
    }
    let mut pageSize = pBt.borrow().pageSize;
    rc = sqlite3PagerSetPagesize(&mut pager.borrow_mut(), &mut pageSize, nReserve as i32);
    if rc != SQLITE_OK {
        sqlite3PagerClose(None, &mut pager.borrow_mut());
        *ppBtree = None;
        return rc;
    }
    {
        let mut bt = pBt.borrow_mut();
        bt.pageSize = pageSize;
        bt.usableSize = pageSize - nReserve as u32;
        debug_assert!((bt.pageSize & 7) == 0); /* 8-byte alignment of pageSize */
    }

    let p: BtreeP = Rc::new(RefCell::new(Btree {
        pBt: pBt.clone(),
        inTrans: TRANS_NONE as u8,
        sharable: 0,
        locked: 0,
        hasIncrblobCur: 0,
        wantToLock: 0,
        nBackup: 0,
        iBDataVersion: 0,
        nSeek: 0,
    }));

    /* If the B-Tree was successfully opened, set the pager-cache size to the
     ** default value. Except, when opening on an existing shared pager-cache,
     ** do not change the pager-cache size.
     */
    if sqlite3BtreeSchema(&p, 0, ()).is_none() {
        sqlite3PagerSetCachesize(&mut pager.borrow_mut(), SQLITE_DEFAULT_CACHE_SIZE);
    }
    /* C: sqlite3OsFileControlHint(pFile, SQLITE_FCNTL_PDB, &pBt->db) only
     ** matters for the unix VFS's shared-memory deadlock detection with
     ** shared cache; it carries a C pointer and is omitted here. */
    *ppBtree = Some(p);
    SQLITE_OK
}

/// Make sure pBt->pTmpSpace points to an allocation of
/// MX_CELL_SIZE(pBt) bytes with a 4-byte prefix for a left-child
/// pointer.
fn allocateTempSpace(pBt: &mut BtShared) -> i32 {
    debug_assert!(pBt.pTmpSpace.is_empty());
    /* One of the uses of pBt->pTmpSpace is to format cells before
     ** inserting them into a leaf page (function fillInCell()). If
     ** a cell is less than 4 bytes in size, it is rounded up to 4 bytes
     ** by the various routines that manipulate binary cells. Which
     ** can mean that fillInCell() only initializes the first 2 or 3
     ** bytes of pTmpSpace, but that the first 4 bytes are copied from
     ** it into a database page. ... So zero the space here.
     */
    pBt.pTmpSpace = vec![0u8; pBt.pageSize as usize];
    SQLITE_OK
}

/// Free the pBt->pTmpSpace allocation
fn freeTempSpace(pBt: &mut BtShared) {
    pBt.pTmpSpace = Vec::new();
}

/// C: `int sqlite3BtreeClose(Btree *p)`
///
/// Close an open database and invalidate all cursors.
pub fn sqlite3BtreeClose(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    let nVdbeRead = db.nVdbeRead;
    btreeClose(Some(db), p, nVdbeRead)
}

/// sqlite3BtreeClose() body.  `db` may be None when called from
/// sqlite3BtreeCloseCursor() on a BTREE_SINGLE (ephemeral) btree.
pub fn btreeClose(db: Option<&mut Sqlite3>, p: &BtreeP, nVdbeRead: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();

    /* Rollback any active transaction and free the handle structure.
     ** The call to sqlite3BtreeRollback() drops any table-locks held by
     ** this handle.
     */
    btreeRollback(p, SQLITE_OK, 0, nVdbeRead);

    /* Clean out and delete the BtShared object. */
    debug_assert!(cursorList(&pBt).is_empty());
    let pager = pagerOf(&pBt);
    sqlite3PagerClose(db, &mut pager.borrow_mut());
    let mut bt = pBt.borrow_mut();
    freeTempSpace(&mut bt);
    bt.pCursor.clear();
    bt.pPage1 = None;
    SQLITE_OK
}

/// C: `int sqlite3BtreeSetCacheSize(Btree *p, int mxPage)`
///
/// Change the "soft" limit on the number of pages in the cache.
/// Unused and unmodified pages will be recycled when the number of
/// pages in the cache exceeds this soft limit.  But the size of the
/// cache is allowed to grow larger than this limit if it contains
/// dirty pages or pages still in active use.
pub fn sqlite3BtreeSetCacheSize(db: &mut Sqlite3, p: &BtreeP, mxPage: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let pager = pagerOf(&pBt);
    sqlite3PagerSetCachesize(&mut pager.borrow_mut(), mxPage);
    SQLITE_OK
}

/// C: `int sqlite3BtreeSetSpillSize(Btree *p, int mxPage)`
///
/// Change the "spill" limit on the number of pages in the cache.
/// If the number of pages exceeds this limit during a write transaction,
/// the pager might attempt to "spill" pages to the journal early in
/// order to free up memory.
///
/// The value returned is the current spill size.  If zero is passed
/// as an argument, no changes are made to the spill size setting, so
/// using mxPage of 0 is a way to query the current spill size.
pub fn sqlite3BtreeSetSpillSize(db: &mut Sqlite3, p: &BtreeP, mxPage: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let pager = pagerOf(&pBt);
    let res = sqlite3PagerSetSpillsize(&mut pager.borrow_mut(), mxPage);
    res
}

/// C: `int sqlite3BtreeSetMmapLimit(Btree *p, sqlite3_int64 szMmap)`
///
/// Change the limit on the amount of the database file that may be
/// memory mapped.
pub fn sqlite3BtreeSetMmapLimit(db: &mut Sqlite3, p: &BtreeP, szMmap: i64) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let pager = pagerOf(&pBt);
    sqlite3PagerSetMmapLimit(&mut pager.borrow_mut(), szMmap);
    SQLITE_OK
}

/// C: `int sqlite3BtreeSetPagerFlags( Btree *p, unsigned pgFlags )`
///
/// Change the way data is synced to disk in order to increase or decrease
/// how well the database resists damage due to OS crashes and power
/// failures.
pub fn sqlite3BtreeSetPagerFlags(db: &mut Sqlite3, p: &BtreeP, pgFlags: u32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let pager = pagerOf(&pBt);
    sqlite3PagerSetFlags(&mut pager.borrow_mut(), pgFlags);
    SQLITE_OK
}

/// C: `int sqlite3BtreeSetPageSize(Btree *p, int pageSize, int nReserve, int iFix)`
///
/// Change the default pages size and the number of reserved bytes per page.
/// Or, if the page size has already been fixed, return SQLITE_READONLY
/// without changing anything.
///
/// The page size must be a power of 2 between 512 and 65536.  If the page
/// size supplied does not meet this constraint then the page size is not
/// changed.
///
/// If parameter nReserve is less than zero, then the number of reserved
/// bytes per page is left unchanged.
///
/// If the iFix!=0 then the BTS_PAGESIZE_FIXED flag is set so that the page size
/// and autovacuum mode can no longer be changed.
pub fn sqlite3BtreeSetPageSize(p: &BtreeP, pageSize: i32, nReserve: i32, iFix: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut pageSize = pageSize;
    let mut nReserve = nReserve;
    debug_assert!(nReserve >= 0 && nReserve <= 255);
    let x;
    {
        let mut bt = pBt.borrow_mut();
        bt.nReserveWanted = nReserve as u8;
        x = (bt.pageSize - bt.usableSize) as i32;
        if x == nReserve && (pageSize == 0 || pageSize as u32 == bt.pageSize) {
            return SQLITE_OK;
        }
        if nReserve < x {
            nReserve = x;
        }
        if bt.btsFlags as i32 & BTS_PAGESIZE_FIXED != 0 {
            return SQLITE_READONLY;
        }
        debug_assert!(nReserve >= 0 && nReserve <= 255);
        if pageSize >= 512 && pageSize <= SQLITE_MAX_PAGE_SIZE && ((pageSize - 1) & pageSize) == 0 {
            debug_assert!((pageSize & 7) == 0);
            debug_assert!(bt.pCursor.iter().all(|w| w.upgrade().is_none()));
            if nReserve > 32 && pageSize == 512 {
                pageSize = 1024;
            }
            bt.pageSize = pageSize as u32;
            freeTempSpace(&mut bt);
        }
    }
    let pager = pagerOf(&pBt);
    let mut ps = pBt.borrow().pageSize;
    let rc = sqlite3PagerSetPagesize(&mut pager.borrow_mut(), &mut ps, nReserve);
    let mut bt = pBt.borrow_mut();
    bt.pageSize = ps;
    bt.usableSize = bt.pageSize - nReserve as u16 as u32;
    if iFix != 0 {
        bt.btsFlags |= BTS_PAGESIZE_FIXED as u16;
    }
    rc
}

/// C: `int sqlite3BtreeGetPageSize(Btree *p)`
///
/// Return the currently defined page size
pub fn sqlite3BtreeGetPageSize(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let n = pBt.borrow().pageSize as i32;
    n
}

/// C: `int sqlite3BtreeGetReserveNoMutex(Btree *p)`
///
/// This function is similar to sqlite3BtreeGetReserve(), except that it
/// may only be called if it is guaranteed that the b-tree mutex is already
/// held.
pub fn sqlite3BtreeGetReserveNoMutex(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let bt = pBt.borrow();
    (bt.pageSize - bt.usableSize) as i32
}

/// C: `int sqlite3BtreeGetRequestedReserve(Btree *p)`
///
/// Return the number of bytes of space at the end of every page that
/// are intentionally left unused.  This is the "reserved" space that is
/// sometimes used by extensions.
///
/// The value returned is the larger of the current reserve size and
/// the latest reserve size requested by SQLITE_FILECTRL_RESERVE_BYTES.
/// The amount of reserve can only grow - never shrink.
pub fn sqlite3BtreeGetRequestedReserve(p: &BtreeP) -> i32 {
    let n1 = {
        let pBt = p.borrow().pBt.clone();
        let n = pBt.borrow().nReserveWanted as i32;
        n
    };
    let n2 = sqlite3BtreeGetReserveNoMutex(p);
    if n1 > n2 { n1 } else { n2 }
}

/// C: `Pgno sqlite3BtreeMaxPageCount(Btree *p, Pgno mxPage)`
///
/// Set the maximum page count for a database if mxPage is positive.
/// No changes are made if mxPage is 0 or negative.
/// Regardless of the value of mxPage, return the maximum page count.
pub fn sqlite3BtreeMaxPageCount(p: &BtreeP, mxPage: Pgno) -> Pgno {
    let pBt = p.borrow().pBt.clone();
    let pager = pagerOf(&pBt);
    let n = sqlite3PagerMaxPageCount(&mut pager.borrow_mut(), mxPage);
    n
}

/// C: `int sqlite3BtreeSecureDelete(Btree *p, int newFlag)`
///
/// Change the values for the BTS_SECURE_DELETE and BTS_OVERWRITE flags:
///
///    newFlag==0       Both BTS_SECURE_DELETE and BTS_OVERWRITE are cleared
///    newFlag==1       BTS_SECURE_DELETE set and BTS_OVERWRITE is cleared
///    newFlag==2       BTS_SECURE_DELETE cleared and BTS_OVERWRITE is set
///    newFlag==(-1)    No changes
///
/// This routine acts as a query if newFlag is less than zero
pub fn sqlite3BtreeSecureDelete(p: &BtreeP, newFlag: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut bt = pBt.borrow_mut();
    if newFlag >= 0 {
        bt.btsFlags &= !(BTS_FAST_SECURE as u16);
        bt.btsFlags |= (BTS_SECURE_DELETE * newFlag) as u16;
    }
    let b = (bt.btsFlags as i32 & BTS_FAST_SECURE) / BTS_SECURE_DELETE;
    b
}

/// C: `int sqlite3BtreeSetAutoVacuum(Btree *p, int autoVacuum)`
///
/// Change the 'auto-vacuum' property of the database. If the 'autoVacuum'
/// parameter is non-zero, then auto-vacuum mode is enabled. If zero, it
/// is disabled. The default value for the auto-vacuum property is
/// determined by the SQLITE_DEFAULT_AUTOVACUUM macro.
pub fn sqlite3BtreeSetAutoVacuum(p: &BtreeP, autoVacuum: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut bt = pBt.borrow_mut();
    let mut rc = SQLITE_OK;
    let av = autoVacuum as u8;
    if (bt.btsFlags as i32 & BTS_PAGESIZE_FIXED) != 0
        && (if av != 0 { 1 } else { 0 }) != bt.autoVacuum
    {
        rc = SQLITE_READONLY;
    } else {
        bt.autoVacuum = if av != 0 { 1 } else { 0 };
        bt.incrVacuum = if av == 2 { 1 } else { 0 };
    }
    rc
}

/// C: `int sqlite3BtreeGetAutoVacuum(Btree *p)`
///
/// Return the value of the 'auto-vacuum' property. If auto-vacuum is
/// enabled 1 is returned. Otherwise 0.
pub fn sqlite3BtreeGetAutoVacuum(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let bt = pBt.borrow();
    if bt.autoVacuum == 0 {
        BTREE_AUTOVACUUM_NONE
    } else if bt.incrVacuum == 0 {
        BTREE_AUTOVACUUM_FULL
    } else {
        BTREE_AUTOVACUUM_INCR
    }
}

/// Get a reference to pPage1 of the database file.  This will
/// also acquire a readlock on that file.
///
/// SQLITE_OK is returned on success.  If the file is not a
/// well-formed database file, then SQLITE_CORRUPT is returned.
/// SQLITE_BUSY is returned if the database is locked.  SQLITE_NOMEM
/// is returned if we run out of memory.
fn lockBtree(pBt: &BtSharedP, db: &BtDbState) -> i32 {
    debug_assert!(pBt.borrow().pPage1.is_none());
    let pager = pagerOf(pBt);
    let mut rc = sqlite3PagerSharedLock(&mut pager.borrow_mut());
    if rc != SQLITE_OK {
        return rc;
    }
    let mut pPage1 = None;
    rc = btreeGetPage(pBt, 1, &mut pPage1, 0);
    if rc != SQLITE_OK {
        return rc;
    }
    let pPage1 = pPage1.unwrap();

    /* Do some checking to help insure the file we opened really is
     ** a valid database file.
     */
    let mut nPage = get4byte(&pPage1.borrow().aData, 28);
    let mut nPageFile: i32 = 0;
    sqlite3PagerPagecount(&mut pager.borrow_mut(), &mut nPageFile);
    let nPageFile = nPageFile as u32;
    {
        let g = pPage1.borrow();
        if nPage == 0 || g.aData[24..28] != g.aData[92..96] {
            nPage = nPageFile;
        }
    }
    if (db.flags & SQLITE_ResetDatabase) != 0 {
        nPage = 0;
    }
    'page1_init_failed: {
        if nPage > 0 {
            let page1: Vec<u8> = pPage1.borrow().aData[..100].to_vec();
            rc = SQLITE_NOTADB;
            /* EVIDENCE-OF: R-43737-39999 Every valid SQLite database file begins
             ** with the following 16 bytes (in hex): 53 51 4c 69 74 65 20 66 6f 72 6d
             ** 61 74 20 33 00. */
            if page1[..16] != zMagicHeader[..] {
                break 'page1_init_failed;
            }

            if page1[18] > 2 {
                pBt.borrow_mut().btsFlags |= BTS_READ_ONLY as u16;
            }
            if page1[19] > 2 {
                break 'page1_init_failed;
            }

            /* If the read version is set to 2, this database should be accessed
             ** in WAL mode. If the log is not already open, open it now. Then
             ** return SQLITE_OK and return without populating BtShared.pPage1.
             ** The caller detects this and calls this function again. This is
             ** required as the version of page 1 currently in the page1 buffer
             ** may not be the latest version - there may be a newer one in the log
             ** file.
             */
            if page1[19] == 2 && (pBt.borrow().btsFlags as i32 & BTS_NO_WAL) == 0 {
                let mut isOpen = 0;
                rc = sqlite3PagerOpenWal(&mut pager.borrow_mut(), &mut isOpen);
                if rc != SQLITE_OK {
                    break 'page1_init_failed;
                } else {
                    /* setDefaultSyncFlag(): no-op since
                     ** SQLITE_DEFAULT_SYNCHRONOUS==SQLITE_DEFAULT_WAL_SYNCHRONOUS */
                    if isOpen == 0 {
                        releasePageOne(&pPage1);
                        return SQLITE_OK;
                    }
                }
                rc = SQLITE_NOTADB;
            }

            /* EVIDENCE-OF: R-15465-20813 The maximum and minimum embedded payload
             ** fractions and the leaf payload fraction values must be 64, 32, and 32.
             **
             ** The original design allowed these amounts to vary, but as of
             ** version 3.6.0, we require them to be fixed.
             */
            if page1[21..24] != [64u8, 32, 32] {
                break 'page1_init_failed;
            }
            /* EVIDENCE-OF: R-51873-39618 The page size for a database file is
             ** determined by the 2-byte integer located at an offset of 16 bytes from
             ** the beginning of the database file. */
            let pageSize: u32 = ((page1[16] as u32) << 8) | ((page1[17] as u32) << 16);
            /* EVIDENCE-OF: R-25008-21688 The size of a page is a power of two
             ** between 512 and 65536 inclusive. */
            if (pageSize.wrapping_sub(1) & pageSize) != 0
                || pageSize > SQLITE_MAX_PAGE_SIZE as u32
                || pageSize <= 256
            {
                break 'page1_init_failed;
            }
            debug_assert!((pageSize & 7) == 0);
            /* EVIDENCE-OF: R-59310-51205 The "reserved space" size in the 1-byte
             ** integer at offset 20 is the number of bytes of space at the end of
             ** each page to reserve for extensions.
             **
             ** EVIDENCE-OF: R-37497-42412 The size of the reserved region is
             ** determined by the one-byte unsigned integer found at an offset of 20
             ** into the database file header. */
            let usableSize = pageSize - page1[20] as u32;
            if pageSize != pBt.borrow().pageSize {
                /* After reading the first page of the database assuming a page size
                 ** of BtShared.pageSize, we have discovered that the page-size is
                 ** actually pageSize. Unlock the database, leave pBt->pPage1 at
                 ** zero and return SQLITE_OK. The caller will call this function
                 ** again with the correct page-size.
                 */
                releasePageOne(&pPage1);
                {
                    let mut bt = pBt.borrow_mut();
                    bt.usableSize = usableSize;
                    bt.pageSize = pageSize;
                    bt.btsFlags |= BTS_PAGESIZE_FIXED as u16;
                    freeTempSpace(&mut bt);
                }
                let mut ps = pageSize;
                rc = sqlite3PagerSetPagesize(
                    &mut pager.borrow_mut(),
                    &mut ps,
                    (pageSize - usableSize) as i32,
                );
                pBt.borrow_mut().pageSize = ps;
                return rc;
            }
            if nPage > nPageFile {
                if db.writableSchema == 0 {
                    rc = SQLITE_CORRUPT_BKPT();
                    break 'page1_init_failed;
                } else {
                    nPage = nPageFile;
                }
            }
            /* EVIDENCE-OF: R-28312-64704 However, the usable size is not allowed to
             ** be less than 480. In other words, if the page size is 512, then the
             ** reserved space size cannot exceed 32. */
            if usableSize < 480 {
                break 'page1_init_failed;
            }
            let mut bt = pBt.borrow_mut();
            bt.btsFlags |= BTS_PAGESIZE_FIXED as u16;
            bt.pageSize = pageSize;
            bt.usableSize = usableSize;
            bt.autoVacuum = if get4byte(&page1, 36 + 4 * 4) != 0 {
                1
            } else {
                0
            };
            bt.incrVacuum = if get4byte(&page1, 36 + 7 * 4) != 0 {
                1
            } else {
                0
            };
        }

        /* maxLocal is the maximum amount of payload to store locally for
         ** a cell.  Make sure it is small enough so that at least minFanout
         ** cells can will fit on one page.  We assume a 10-byte page header.
         ** Besides the payload, the cell must store:
         **     2-byte pointer to the cell
         **     4-byte child pointer
         **     9-byte nKey value
         **     4-byte nData value
         **     4-byte overflow page pointer
         ** So a cell consists of a 2-byte pointer, a header which is as much as
         ** 17 bytes long, 0 to N bytes of payload, and an optional 4 byte overflow
         ** page pointer.
         */
        let mut bt = pBt.borrow_mut();
        bt.maxLocal = ((bt.usableSize - 12) * 64 / 255 - 23) as u16;
        bt.minLocal = ((bt.usableSize - 12) * 32 / 255 - 23) as u16;
        bt.maxLeaf = (bt.usableSize - 35) as u16;
        bt.minLeaf = ((bt.usableSize - 12) * 32 / 255 - 23) as u16;
        if bt.maxLocal > 127 {
            bt.max1bytePayload = 127;
        } else {
            bt.max1bytePayload = bt.maxLocal as u8;
        }
        debug_assert!(bt.maxLeaf as i32 + 23 <= MX_CELL_SIZE(bt.pageSize));
        bt.pPage1 = Some(pPage1.clone());
        bt.nPage = nPage;
        drop(bt);
        /* The MemPage of page 1 caches values of BtShared: refresh them. */
        {
            let b = pBt.borrow();
            let mut g = pPage1.borrow_mut();
            g.pExtra.usableSize = b.usableSize;
            g.pExtra.pageSize = b.pageSize;
        }
        return SQLITE_OK;
    }

    /* page1_init_failed: */
    releasePageOne(&pPage1);
    pBt.borrow_mut().pPage1 = None;
    rc
}

/// Return the number of cursors open on pBt (for asserts).
fn countValidCursors(pBt: &BtSharedP, wrOnly: i32) -> i32 {
    let mut r = 0;
    for p in cursorList(pBt) {
        if let Ok(c) = p.try_borrow() {
            if (wrOnly == 0 || (c.curFlags as i32 & BTCF_WriteFlag) != 0)
                && c.eState != CURSOR_FAULT as u8
            {
                r += 1;
            }
        }
    }
    r
}

/// If there are no outstanding cursors and we are not in the middle
/// of a transaction but there is a read lock on the database, then
/// this routine unrefs the first page of the database file which
/// has the effect of releasing the read lock.
///
/// If there is a transaction in progress, this routine is a no-op.
fn unlockBtreeIfUnused(pBt: &BtSharedP) {
    let pPage1 = {
        let mut bt = pBt.borrow_mut();
        if bt.inTransaction == TRANS_NONE as u8 && bt.pPage1.is_some() {
            bt.pPage1.take()
        } else {
            None
        }
    };
    if let Some(pPage1) = pPage1 {
        releasePageOne(&pPage1);
    }
}

/// If pBt points to an empty file then convert that empty file
/// into a new empty database by initializing the first page of
/// the database.
fn newDatabase(pBt: &BtSharedP) -> i32 {
    if pBt.borrow().nPage > 0 {
        return SQLITE_OK;
    }
    let pP1 = page1Of(pBt);
    let rc = sqlite3PagerWrite(&pP1);
    if rc != 0 {
        return rc;
    }
    let (pageSize, usableSize, autoVacuum, incrVacuum) = {
        let b = pBt.borrow();
        (b.pageSize, b.usableSize, b.autoVacuum, b.incrVacuum)
    };
    {
        let mut g = pP1.borrow_mut();
        let data = &mut g.aData;
        data[..16].copy_from_slice(&zMagicHeader);
        data[16] = ((pageSize >> 8) & 0xff) as u8;
        data[17] = ((pageSize >> 16) & 0xff) as u8;
        data[18] = 1;
        data[19] = 1;
        debug_assert!(usableSize <= pageSize && usableSize + 255 >= pageSize);
        data[20] = (pageSize - usableSize) as u8;
        data[21] = 64;
        data[22] = 32;
        data[23] = 32;
        for b in &mut data[24..100] {
            *b = 0;
        }
        zeroPage(&mut g, PTF_INTKEY | PTF_LEAF | PTF_LEAFDATA);
    }
    pBt.borrow_mut().btsFlags |= BTS_PAGESIZE_FIXED as u16;
    {
        let mut g = pP1.borrow_mut();
        debug_assert!(autoVacuum == 1 || autoVacuum == 0);
        debug_assert!(incrVacuum == 1 || incrVacuum == 0);
        put4byte(&mut g.aData, 36 + 4 * 4, autoVacuum as u32);
        put4byte(&mut g.aData, 36 + 7 * 4, incrVacuum as u32);
        g.aData[31] = 1;
    }
    pBt.borrow_mut().nPage = 1;
    SQLITE_OK
}

/// C: `int sqlite3BtreeNewDb(Btree *p)`
///
/// Initialize the first page of the database file (creating a database
/// consisting of a single page and no schema objects). Return SQLITE_OK
/// if successful, or an SQLite error code otherwise.
pub fn sqlite3BtreeNewDb(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    pBt.borrow_mut().nPage = 0;
    newDatabase(&pBt)
}

/// Attempt to start a new transaction. A write-transaction
/// is started if the second argument is nonzero, otherwise a read-
/// transaction.  If the second argument is 2 or more and exclusive
/// transaction is started, meaning that no other process is allowed
/// to access the database.  A preexisting transaction may not be
/// upgraded to exclusive by calling this routine a second time - the
/// exclusivity flag only works for a new transaction.
///
/// If an initial attempt to acquire the lock fails because of lock contention
/// and the database was previously unlocked, then invoke the busy handler
/// if there is one.  But if there was previously a read-lock, do not
/// invoke the busy handler - just return SQLITE_BUSY.  SQLITE_BUSY is
/// returned when there is already a read-lock in order to avoid a deadlock.
fn btreeBeginTrans(db: &BtDbState, p: &BtreeP, wrflag: i32, pSchemaVersion: &mut i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let pPager = pagerOf(&pBt);
    let mut rc = SQLITE_OK;

    'trans_begun: {
        /* If the btree is already in a write-transaction, or it
         ** is already in a read-transaction and a read-transaction
         ** is requested, this is a no-op.
         */
        let inTrans = p.borrow().inTrans as i32;
        if inTrans == TRANS_WRITE || (inTrans == TRANS_READ && wrflag == 0) {
            break 'trans_begun;
        }

        if (db.flags & SQLITE_ResetDatabase) != 0
            && sqlite3PagerIsreadonly(&mut pPager.borrow_mut()) == 0
        {
            pBt.borrow_mut().btsFlags &= !(BTS_READ_ONLY as u16);
        }

        /* Write transactions are not possible on a read-only database */
        if (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) != 0 && wrflag != 0 {
            rc = SQLITE_READONLY;
            break 'trans_begun;
        }

        {
            let mut bt = pBt.borrow_mut();
            bt.btsFlags &= !(BTS_INITIALLY_EMPTY as u16);
            if bt.nPage == 0 {
                bt.btsFlags |= BTS_INITIALLY_EMPTY as u16;
            }
        }
        loop {
            /* sqlite3PagerWalDb(pPager, p->db): SQLITE_ENABLE_SETLK_TIMEOUT only */

            /* Call lockBtree() until either pBt->pPage1 is populated or
             ** lockBtree() returns something other than SQLITE_OK. lockBtree()
             ** may return SQLITE_OK but leave pBt->pPage1 set to 0 if after
             ** reading page 1 it discovers that the page-size of the database
             ** file is not pBt->pageSize. In this case lockBtree() will update
             ** pBt->pageSize to the page-size of the file on disk.
             */
            while pBt.borrow().pPage1.is_none() {
                rc = lockBtree(&pBt, db);
                if rc != SQLITE_OK {
                    break;
                }
            }

            if rc == SQLITE_OK && wrflag != 0 {
                if (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) != 0 {
                    rc = SQLITE_READONLY;
                } else {
                    let tim = db.tempInMemory;
                    rc = sqlite3PagerBegin(&mut pPager.borrow_mut(), (wrflag > 1) as i32, tim);
                    if rc == SQLITE_OK {
                        rc = newDatabase(&pBt);
                    } else if rc == SQLITE_BUSY_SNAPSHOT
                        && pBt.borrow().inTransaction == TRANS_NONE as u8
                    {
                        /* if there was no transaction opened when this function was
                         ** called and SQLITE_BUSY_SNAPSHOT is returned, change the error
                         ** code to SQLITE_BUSY. */
                        rc = SQLITE_BUSY;
                    }
                }
            }

            if rc != SQLITE_OK {
                /* (void)sqlite3PagerWalWriteLock(pPager, 0): no-op */
                unlockBtreeIfUnused(&pBt);
            }
            if !((rc & 0xFF) == SQLITE_BUSY
                && pBt.borrow().inTransaction == TRANS_NONE as u8
                && btreeInvokeBusyHandler(db))
            {
                break;
            }
        }

        if rc == SQLITE_OK {
            let mut pb = p.borrow_mut();
            let mut bt = pBt.borrow_mut();
            if pb.inTrans == TRANS_NONE as u8 {
                bt.nTransaction += 1;
            }
            pb.inTrans = if wrflag != 0 { TRANS_WRITE } else { TRANS_READ } as u8;
            if pb.inTrans > bt.inTransaction {
                bt.inTransaction = pb.inTrans;
            }
            drop(pb);
            drop(bt);
            if wrflag != 0 {
                let pPage1 = page1Of(&pBt);

                /* If the db-size header field is incorrect (as it may be if an old
                 ** client has been writing the database file), update it now. Doing
                 ** this sooner rather than later means the database size can safely
                 ** re-read the database size from page 1 if a savepoint or transaction
                 ** rollback occurs within the transaction.
                 */
                let nPage = pBt.borrow().nPage;
                if nPage != get4byte(&pPage1.borrow().aData, 28) {
                    rc = sqlite3PagerWrite(&pPage1);
                    if rc == SQLITE_OK {
                        put4byte(&mut pPage1.borrow_mut().aData, 28, nPage);
                    }
                }
            }
        }
    }

    /* trans_begun: */
    if rc == SQLITE_OK {
        *pSchemaVersion = page1Get4(&pBt, 40) as i32;
        if wrflag != 0 {
            /* This call makes sure that the pager has the correct number of
             ** open savepoints. If the second parameter is greater than 0 and
             ** the sub-journal is not already open, then it will be opened here.
             */
            rc = sqlite3PagerOpenSavepoint(&mut pPager.borrow_mut(), db.nSavepoint);
        }
    }
    rc
}

/// C: `int sqlite3BtreeBeginTrans(Btree *p, int wrflag, int *pSchemaVersion)`
pub fn sqlite3BtreeBeginTrans(
    db: &mut Sqlite3,
    p: &BtreeP,
    wrflag: i32,
    pSchemaVersion: &mut i32,
) -> i32 {
    let (sharable, inTrans) = {
        let b = p.borrow();
        (b.sharable, b.inTrans as i32)
    };
    if sharable != 0 || inTrans == TRANS_NONE || (inTrans == TRANS_READ && wrflag != 0) {
        let st = BtDbState::new(db);
        return btreeBeginTransFull(&st, p, wrflag, pSchemaVersion);
    }
    /* Fast path: no connection state needed but db->flags and nSavepoint */
    let pBt = p.borrow().pBt.clone();
    pBt.borrow_mut().dbFlags = db.flags;
    *pSchemaVersion = page1Get4(&pBt, 40) as i32;
    if wrflag != 0 {
        let pager = pagerOf(&pBt);
        let rc = sqlite3PagerOpenSavepoint(&mut pager.borrow_mut(), db.nSavepoint);
        rc
    } else {
        SQLITE_OK
    }
}

/// sqlite3BtreeBeginTrans() on a BtDbState.
pub fn btreeBeginTransFull(
    db: &BtDbState,
    p: &BtreeP,
    wrflag: i32,
    pSchemaVersion: &mut i32,
) -> i32 {
    let (sharable, inTrans, pBt) = {
        let b = p.borrow();
        (b.sharable, b.inTrans as i32, b.pBt.clone())
    };
    /* Rust: keep the copy of db->flags used by btreeInitPage() current. */
    pBt.borrow_mut().dbFlags = db.flags;
    if sharable != 0 || inTrans == TRANS_NONE || (inTrans == TRANS_READ && wrflag != 0) {
        return btreeBeginTrans(db, p, wrflag, pSchemaVersion);
    }
    *pSchemaVersion = page1Get4(&pBt, 40) as i32;
    if wrflag != 0 {
        /* This call makes sure that the pager has the correct number of
         ** open savepoints. If the second parameter is greater than 0 and
         ** the sub-journal is not already open, then it will be opened here.
         */
        let pager = pagerOf(&pBt);
        let rc = sqlite3PagerOpenSavepoint(&mut pager.borrow_mut(), db.nSavepoint);
        rc
    } else {
        SQLITE_OK
    }
}

// ---------------------------------------------------------------------------
// Auto-vacuum
// ---------------------------------------------------------------------------

/// Set the pointer-map entries for all children of page pPage. Also, if
/// pPage contains cells that point to overflow pages, set the pointer
/// map entries for the overflow pages as well.
fn setChildPtrmaps(pg: &DbPage) -> i32 {
    let mut rc = {
        let mut g = pg.borrow_mut();
        if g.pExtra.isInit != 0 {
            SQLITE_OK
        } else {
            btreeInitPage(&mut g)
        }
    };
    if rc != SQLITE_OK {
        return rc;
    }
    let (nCell, leaf, pgno, pBt) = {
        let m = mp(pg);
        (m.nCell as usize, m.leaf, m.pgno, m.pBt.upgrade().unwrap())
    };

    for i in 0..nCell {
        ptrmapPutOvflPtrOfCell(pg, i, &mut rc);

        if leaf == 0 {
            let childPgno = {
                let g = pg.borrow();
                let off = findCell(&g.pExtra, &g.aData, i);
                get4byte(&g.aData, off)
            };
            ptrmapPut(&pBt, childPgno, PTRMAP_BTREE as u8, pgno, &mut rc);
        }
    }

    if leaf == 0 {
        let childPgno = {
            let g = pg.borrow();
            get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8)
        };
        ptrmapPut(&pBt, childPgno, PTRMAP_BTREE as u8, pgno, &mut rc);
    }

    rc
}

/// Somewhere on pPage is a pointer to page iFrom.  Modify this pointer so
/// that it points to iTo. Parameter eType describes the type of pointer to
/// be modified, as  follows:
///
/// PTRMAP_BTREE:     pPage is a btree-page. The pointer points at a child
///                   page of pPage.
///
/// PTRMAP_OVERFLOW1: pPage is a btree-page. The pointer points at an overflow
///                   page pointed to by one of the cells on pPage.
///
/// PTRMAP_OVERFLOW2: pPage is an overflow-page. The pointer points at the next
///                   overflow page in the list.
fn modifyPagePointer(pg: &mut PgHdr, iFrom: Pgno, iTo: Pgno, eType: u8) -> i32 {
    if eType == PTRMAP_OVERFLOW2 as u8 {
        /* The pointer is always the first 4 bytes of the page in this case.  */
        if get4byte(&pg.aData, 0) != iFrom {
            return SQLITE_CORRUPT_PAGE(&pg.pExtra);
        }
        put4byte(&mut pg.aData, 0, iTo);
    } else {
        let rc = if pg.pExtra.isInit != 0 {
            SQLITE_OK
        } else {
            btreeInitPage(pg)
        };
        if rc != 0 {
            return rc;
        }
        let PgHdr {
            aData, pExtra: p, ..
        } = pg;
        let nCell = p.nCell as usize;
        let usableSize = p.usableSize as usize;

        let mut i = 0;
        while i < nCell {
            let pCell = findCell(p, aData, i);
            if eType == PTRMAP_OVERFLOW1 as u8 {
                let mut info = CellInfo::default();
                (p.xParseCell)(p, aData, pCell, &mut info);
                if (info.nLocal as u32) < info.nPayload {
                    if pCell + info.nSize as usize > usableSize {
                        return SQLITE_CORRUPT_PAGE(p);
                    }
                    if iFrom == get4byte(aData, pCell + info.nSize as usize - 4) {
                        put4byte(aData, pCell + info.nSize as usize - 4, iTo);
                        break;
                    }
                }
            } else {
                if pCell + 4 > usableSize {
                    return SQLITE_CORRUPT_PAGE(p);
                }
                if get4byte(aData, pCell) == iFrom {
                    put4byte(aData, pCell, iTo);
                    break;
                }
            }
            i += 1;
        }

        if i == nCell {
            let hdr = p.hdrOffset as usize;
            if eType != PTRMAP_BTREE as u8 || get4byte(aData, hdr + 8) != iFrom {
                return SQLITE_CORRUPT_PAGE(p);
            }
            put4byte(aData, hdr + 8, iTo);
        }
    }
    SQLITE_OK
}

/// Move the open database page pDbPage to location iFreePage in the
/// database. The pDbPage reference remains valid.
///
/// The isCommit flag indicates that there is no need to remember that
/// the journal needs to be sync()ed before database page pDbPage->pgno
/// can be written to. The caller has already promised not to write to that
/// page.
fn relocatePage(
    pBt: &BtSharedP,
    pDbPage: &DbPage,
    eType: u8,
    iPtrPage: Pgno,
    iFreePage: Pgno,
    isCommit: i32,
) -> i32 {
    let iDbPage = pgnoOf(pDbPage);
    let pPager = pagerOf(pBt);

    debug_assert!(
        eType == PTRMAP_OVERFLOW2 as u8
            || eType == PTRMAP_OVERFLOW1 as u8
            || eType == PTRMAP_BTREE as u8
            || eType == PTRMAP_ROOTPAGE as u8
    );
    if iDbPage < 3 {
        return SQLITE_CORRUPT_BKPT();
    }

    /* Move page iDbPage from its current location to page number iFreePage */
    let mut rc = sqlite3PagerMovepage(&mut pPager.borrow_mut(), pDbPage, iFreePage, isCommit);
    if rc != SQLITE_OK {
        return rc;
    }
    mpm(pDbPage).pgno = iFreePage;

    /* If pDbPage was a btree-page, then it may have child pages and/or cells
     ** that point to overflow pages. The pointer map entries for all these
     ** pages need to be changed.
     **
     ** If pDbPage is an overflow page, then the first 4 bytes may store a
     ** pointer to a subsequent overflow page. If this is the case, then
     ** the pointer map needs to be updated for the subsequent overflow page.
     */
    if eType == PTRMAP_BTREE as u8 || eType == PTRMAP_ROOTPAGE as u8 {
        rc = setChildPtrmaps(pDbPage);
        if rc != SQLITE_OK {
            return rc;
        }
    } else {
        let nextOvfl = get4byte(&pDbPage.borrow().aData, 0);
        if nextOvfl != 0 {
            ptrmapPut(pBt, nextOvfl, PTRMAP_OVERFLOW2 as u8, iFreePage, &mut rc);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    /* Fix the database pointer on page iPtrPage that pointed at iDbPage so
     ** that it points at iFreePage. Also fix the pointer map entry for
     ** iPtrPage.
     */
    if eType != PTRMAP_ROOTPAGE as u8 {
        let mut pPtrPage = None;
        rc = btreeGetPage(pBt, iPtrPage, &mut pPtrPage, 0);
        if rc != SQLITE_OK {
            return rc;
        }
        let pPtrPage = pPtrPage.unwrap();
        rc = sqlite3PagerWrite(&pPtrPage);
        if rc != SQLITE_OK {
            releasePage(Some(&pPtrPage));
            return rc;
        }
        rc = modifyPagePointer(&mut pPtrPage.borrow_mut(), iDbPage, iFreePage, eType);
        releasePage(Some(&pPtrPage));
        if rc == SQLITE_OK {
            ptrmapPut(pBt, iFreePage, eType, iPtrPage, &mut rc);
        }
    }
    rc
}

/// Perform a single step of an incremental-vacuum. If successful, return
/// SQLITE_OK. If there is no work to do (and therefore no point in
/// calling this function again), return SQLITE_DONE. Or, if an error
/// occurs, return some other error code.
///
/// More specifically, this function attempts to re-organize the database so
/// that the last page of the file currently in use is no longer in use.
///
/// Parameter nFin is the number of pages that this database would contain
/// were this function called until it returns SQLITE_DONE.
///
/// If the bCommit parameter is non-zero, this function assumes that the
/// caller will keep calling incrVacuumStep() until it returns SQLITE_DONE
/// or an error. bCommit is passed true for an auto-vacuum-on-commit
/// operation, or false for an incremental vacuum.
fn incrVacuumStep(pBt: &BtSharedP, nFin: Pgno, iLastPg: Pgno, bCommit: i32) -> i32 {
    let mut iLastPg = iLastPg;
    let mut rc;
    debug_assert!(iLastPg > nFin);

    if !PTRMAP_ISPAGE(pBt, iLastPg) && iLastPg != pendingBytePage(pBt) {
        let mut eType = 0u8;
        let mut iPtrPage: Pgno = 0;

        let nFreeList = page1Get4(pBt, 36);
        if nFreeList == 0 {
            return SQLITE_DONE;
        }

        rc = ptrmapGet(pBt, iLastPg, &mut eType, Some(&mut iPtrPage));
        if rc != SQLITE_OK {
            return rc;
        }
        if eType == PTRMAP_ROOTPAGE as u8 {
            return SQLITE_CORRUPT_BKPT();
        }

        if eType == PTRMAP_FREEPAGE as u8 {
            if bCommit == 0 {
                /* Remove the page from the files free-list. This is not required
                 ** if bCommit is non-zero. In that case, the free-list will be
                 ** truncated to zero after this function returns, so it doesn't
                 ** matter if it still contains some garbage entries.
                 */
                let mut iFreePg: Pgno = 0;
                let mut pFreePg = None;
                rc = allocateBtreePage(pBt, &mut pFreePg, &mut iFreePg, iLastPg, BTALLOC_EXACT);
                if rc != SQLITE_OK {
                    return rc;
                }
                debug_assert!(iFreePg == iLastPg);
                releasePage(pFreePg.as_ref());
            }
        } else {
            let mut iFreePg: Pgno = 0; /* Index of free page to move pLastPg to */
            let mut eMode = BTALLOC_ANY; /* Mode parameter for allocateBtreePage() */
            let mut iNear: Pgno = 0; /* nearby parameter for allocateBtreePage() */

            let mut pLastPg = None;
            rc = btreeGetPage(pBt, iLastPg, &mut pLastPg, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            let pLastPg = pLastPg.unwrap();

            /* If bCommit is zero, this loop runs exactly once and page pLastPg
             ** is swapped with the first free page pulled off the free list.
             **
             ** On the other hand, if bCommit is greater than zero, then keep
             ** looping until a free-page located within the first nFin pages
             ** of the file is found.
             */
            if bCommit == 0 {
                eMode = BTALLOC_LE;
                iNear = nFin;
            }
            loop {
                let mut pFreePg = None;
                let dbSize = btreePagecount(pBt);
                rc = allocateBtreePage(pBt, &mut pFreePg, &mut iFreePg, iNear, eMode);
                if rc != SQLITE_OK {
                    releasePage(Some(&pLastPg));
                    return rc;
                }
                releasePage(pFreePg.as_ref());
                if iFreePg > dbSize {
                    releasePage(Some(&pLastPg));
                    return SQLITE_CORRUPT_BKPT();
                }
                if !(bCommit != 0 && iFreePg > nFin) {
                    break;
                }
            }
            debug_assert!(iFreePg < iLastPg);

            rc = relocatePage(pBt, &pLastPg, eType, iPtrPage, iFreePg, bCommit);
            releasePage(Some(&pLastPg));
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    if bCommit == 0 {
        loop {
            iLastPg -= 1;
            if !(iLastPg == pendingBytePage(pBt) || PTRMAP_ISPAGE(pBt, iLastPg)) {
                break;
            }
        }
        let mut bt = pBt.borrow_mut();
        bt.bDoTruncate = 1;
        bt.nPage = iLastPg;
    }
    SQLITE_OK
}

/// The database opened by the first argument is an auto-vacuum database
/// nOrig pages in size containing nFree free pages. Return the expected
/// size of the database in pages following an auto-vacuum operation.
fn finalDbSize(pBt: &BtSharedP, nOrig: Pgno, nFree: Pgno) -> Pgno {
    let nEntry = (pBt.borrow().usableSize / 5) as Pgno; /* Number of entries on one ptrmap page */
    let nPtrmap = (nFree
        .wrapping_sub(nOrig)
        .wrapping_add(PTRMAP_PAGENO(pBt, nOrig))
        .wrapping_add(nEntry))
        / nEntry;
    let mut nFin = nOrig.wrapping_sub(nFree).wrapping_sub(nPtrmap);
    let pbp = pendingBytePage(pBt);
    if nOrig > pbp && nFin < pbp {
        nFin = nFin.wrapping_sub(1);
    }
    while PTRMAP_ISPAGE(pBt, nFin) || nFin == pbp {
        nFin = nFin.wrapping_sub(1);
    }
    nFin
}

/// C: `int sqlite3BtreeIncrVacuum(Btree *p)`
///
/// A write-transaction must be opened before calling this function.
/// It performs a single unit of work towards an incremental vacuum.
///
/// If the incremental vacuum is finished after this function has run,
/// SQLITE_DONE is returned. If it is not finished, but no error occurred,
/// SQLITE_OK is returned. Otherwise an SQLite error code.
pub fn sqlite3BtreeIncrVacuum(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut rc;
    if pBt.borrow().autoVacuum == 0 {
        rc = SQLITE_DONE;
    } else {
        let nOrig = btreePagecount(&pBt);
        let nFree = page1Get4(&pBt, 36);
        let nFin = finalDbSize(&pBt, nOrig, nFree);

        if nOrig < nFin || nFree >= nOrig {
            rc = SQLITE_CORRUPT_BKPT();
        } else if nFree > 0 {
            rc = saveAllCursors(&pBt, 0, None);
            if rc == SQLITE_OK {
                invalidateAllOverflowCache(&pBt);
                rc = incrVacuumStep(&pBt, nFin, nOrig, 0);
            }
            if rc == SQLITE_OK {
                let pPage1 = page1Of(&pBt);
                rc = sqlite3PagerWrite(&pPage1);
                let nPage = pBt.borrow().nPage;
                put4byte(&mut pPage1.borrow_mut().aData, 28, nPage);
            }
        } else {
            rc = SQLITE_DONE;
        }
    }
    rc
}

/// This routine is called prior to sqlite3PagerCommit when a transaction
/// is committed for an auto-vacuum database.
fn autoVacuumCommit(xAutovac: Option<&BtAutovac>, p: &BtreeP) -> i32 {
    let mut rc = SQLITE_OK;
    let pBt = p.borrow().pBt.clone();
    let pPager = pagerOf(&pBt);

    invalidateAllOverflowCache(&pBt);
    debug_assert!(pBt.borrow().autoVacuum != 0);
    if pBt.borrow().incrVacuum == 0 {
        let nOrig = btreePagecount(&pBt); /* Database size before freeing */
        if PTRMAP_ISPAGE(&pBt, nOrig) || nOrig == pendingBytePage(&pBt) {
            /* It is not possible to create a database for which the final page
             ** is either a pointer-map page or the pending-byte page. If one
             ** is encountered, this indicates corruption.
             */
            return SQLITE_CORRUPT_BKPT();
        }

        let nFree = page1Get4(&pBt, 36); /* Number of pages on the freelist initially */
        let mut nVac: Pgno; /* Number of pages to vacuum */
        if let Some((xAutovacPages, zName)) = xAutovac {
            let pageSize = pBt.borrow().pageSize;
            nVac = xAutovacPages(zName, nOrig, nFree, pageSize);
            if nVac > nFree {
                nVac = nFree;
            }
            if nVac == 0 {
                return SQLITE_OK;
            }
        } else {
            nVac = nFree;
        }
        let nFin = finalDbSize(&pBt, nOrig, nVac); /* Number of pages in database after autovacuuming */
        if nFin > nOrig {
            return SQLITE_CORRUPT_BKPT();
        }
        if nFin < nOrig {
            rc = saveAllCursors(&pBt, 0, None);
        }
        let mut iFree = nOrig;
        while iFree > nFin && rc == SQLITE_OK {
            rc = incrVacuumStep(&pBt, nFin, iFree, (nVac == nFree) as i32);
            iFree -= 1;
        }
        if (rc == SQLITE_DONE || rc == SQLITE_OK) && nFree > 0 {
            let pPage1 = page1Of(&pBt);
            rc = sqlite3PagerWrite(&pPage1);
            {
                let mut g = pPage1.borrow_mut();
                if nVac == nFree {
                    put4byte(&mut g.aData, 32, 0);
                    put4byte(&mut g.aData, 36, 0);
                }
                put4byte(&mut g.aData, 28, nFin);
            }
            let mut bt = pBt.borrow_mut();
            bt.bDoTruncate = 1;
            bt.nPage = nFin;
        }
        if rc != SQLITE_OK {
            sqlite3PagerRollback(&mut pPager.borrow_mut());
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Commit / rollback / savepoints
// ---------------------------------------------------------------------------

/// C: `int sqlite3BtreeCommitPhaseOne(Btree *p, const char *zSuperJrnl)`
///
/// This routine does the first phase of a two-phase commit.  This routine
/// causes a rollback journal to be created (if it does not already exist)
/// and populated with enough information so that if a power loss occurs
/// the database can be restored to its original state by playing back
/// the journal.  Then the contents of the journal are flushed out to
/// the disk.  After the journal is safely on oxide, the changes to the
/// database are written into the database file and flushed to oxide.
/// At the end of this call, the rollback journal still exists on the
/// disk and we are still holding all locks, so the transaction has not
/// committed.  See sqlite3BtreeCommitPhaseTwo() for the second phase of the
/// commit process.
///
/// This call is a no-op if no write-transaction is currently active on pBt.
///
/// Otherwise, sync the database file for the btree pBt. zSuperJrnl points to
/// the name of a super-journal file that should be written into the
/// individual journal file, or is NULL, indicating no super-journal file
/// (single database transaction).
pub fn sqlite3BtreeCommitPhaseOne(db: &mut Sqlite3, p: &BtreeP, zSuperJrnl: Option<&[u8]>) -> i32 {
    /* db->xAutovacPages and the schema name of p (autoVacuumCommit()) */
    let mut xAutovac: Option<BtAutovac> = None;
    if let Some(x) = db.xAutovacPages.clone() {
        let mut iDb = 0usize;
        while iDb < db.aDb.len() {
            if let Some(b) = &db.aDb[iDb].pBt {
                if Rc::ptr_eq(b, p) {
                    break;
                }
            }
            iDb += 1;
        }
        let zName: Vec<u8> = db
            .aDb
            .get(iDb)
            .and_then(|d| d.zDbSName.clone())
            .unwrap_or_default();
        xAutovac = Some((x, zName));
    }
    btreeCommitPhaseOne(xAutovac.as_ref(), p, zSuperJrnl)
}

/// db->xAutovacPages with the schema name of the btree.
pub type BtAutovac = (Rc<dyn Fn(&[u8], u32, u32, u32) -> u32>, ZStr);

/// sqlite3BtreeCommitPhaseOne() body.
pub fn btreeCommitPhaseOne(
    xAutovac: Option<&BtAutovac>,
    p: &BtreeP,
    zSuperJrnl: Option<&[u8]>,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p.borrow().inTrans == TRANS_WRITE as u8 {
        let pBt = p.borrow().pBt.clone();
        if pBt.borrow().autoVacuum != 0 {
            rc = autoVacuumCommit(xAutovac, p);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        let pager = pagerOf(&pBt);
        let (bDoTruncate, nPage) = {
            let b = pBt.borrow();
            (b.bDoTruncate, b.nPage)
        };
        if bDoTruncate != 0 {
            sqlite3PagerTruncateImage(&mut pager.borrow_mut(), nPage);
        }
        rc = sqlite3PagerCommitPhaseOne(&mut pager.borrow_mut(), zSuperJrnl, 0);
    }
    rc
}

/// This function is called from both BtreeCommitPhaseTwo() and BtreeRollback()
/// at the conclusion of a transaction.
fn btreeEndTransaction(p: &BtreeP, nVdbeRead: i32) {
    let pBt = p.borrow().pBt.clone();
    pBt.borrow_mut().bDoTruncate = 0;
    let inTrans = p.borrow().inTrans;
    if inTrans > TRANS_NONE as u8 && nVdbeRead > 1 {
        /* If there are other active statements that belong to this database
         ** handle, downgrade to a read-only transaction. The other statements
         ** may still be reading from the database.  */
        p.borrow_mut().inTrans = TRANS_READ as u8;
    } else {
        /* If the handle had any kind of transaction open, decrement the
         ** transaction count of the shared btree. If the transaction count
         ** reaches 0, set the shared state to TRANS_NONE. The unlockBtreeIfUnused()
         ** call below will unlock the pager.  */
        if inTrans != TRANS_NONE as u8 {
            let mut bt = pBt.borrow_mut();
            bt.nTransaction -= 1;
            if 0 == bt.nTransaction {
                bt.inTransaction = TRANS_NONE as u8;
            }
        }

        /* Set the current transaction state to TRANS_NONE and unlock the
         ** pager if this call closed the only read or write transaction.  */
        p.borrow_mut().inTrans = TRANS_NONE as u8;
        unlockBtreeIfUnused(&pBt);
    }
}

/// C: `int sqlite3BtreeCommitPhaseTwo(Btree *p, int bCleanup)`
///
/// Commit the transaction currently in progress.
///
/// This routine implements the second phase of a 2-phase commit.  The
/// sqlite3BtreeCommitPhaseOne() routine does the first phase and should
/// be invoked prior to calling this routine.  The sqlite3BtreeCommitPhaseOne()
/// routine did all the work of writing information out to disk and flushing the
/// contents so that they are written onto the disk platter.  All this
/// routine has to do is delete or truncate or zero the header in the
/// the rollback journal (which causes the transaction to commit) and
/// drop locks.
///
/// Normally, if an error occurs while the pager layer is attempting to
/// finalize the underlying journal file, this function returns an error and
/// the upper layer will attempt a rollback. However, if the second argument
/// is non-zero then this b-tree transaction is part of a multi-file
/// transaction. In this case, the transaction has already been committed
/// (by deleting a super-journal file) and the caller will ignore this
/// functions return code. So, even if an error occurs in the pager layer,
/// reset the b-tree objects internal state to indicate that the write
/// transaction has been closed. This is quite safe, as the pager will have
/// transitioned to the error state.
///
/// This will release the write lock on the database file.  If there
/// are no active cursors, it also releases the read lock.
pub fn sqlite3BtreeCommitPhaseTwo(db: &mut Sqlite3, p: &BtreeP, bCleanup: i32) -> i32 {
    let nVdbeRead = db.nVdbeRead;
    btreeCommitPhaseTwo(p, bCleanup, nVdbeRead)
}
pub fn btreeCommitPhaseTwo(p: &BtreeP, bCleanup: i32, nVdbeRead: i32) -> i32 {
    if p.borrow().inTrans == TRANS_NONE as u8 {
        return SQLITE_OK;
    }

    /* If the handle has a write-transaction open, commit the shared-btrees
     ** transaction and set the shared state to TRANS_READ.
     */
    if p.borrow().inTrans == TRANS_WRITE as u8 {
        let pBt = p.borrow().pBt.clone();
        debug_assert!(pBt.borrow().inTransaction == TRANS_WRITE as u8);
        debug_assert!(pBt.borrow().nTransaction > 0);
        let pager = pagerOf(&pBt);
        let rc = sqlite3PagerCommitPhaseTwo(&mut pager.borrow_mut());
        if rc != SQLITE_OK && bCleanup == 0 {
            return rc;
        }
        {
            let mut pb = p.borrow_mut();
            pb.iBDataVersion = pb.iBDataVersion.wrapping_sub(1); /* Compensate for pPager->iDataVersion++; */
        }
        pBt.borrow_mut().inTransaction = TRANS_READ as u8;
        btreeClearHasContent(&pBt);
    }

    btreeEndTransaction(p, nVdbeRead);
    SQLITE_OK
}

/// C: `int sqlite3BtreeCommit(Btree *p)`
///
/// Do both phases of a commit.
pub fn sqlite3BtreeCommit(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    let mut rc = sqlite3BtreeCommitPhaseOne(db, p, None);
    if rc == SQLITE_OK {
        rc = sqlite3BtreeCommitPhaseTwo(db, p, 0);
    }
    rc
}

/// C: `int sqlite3BtreeTripAllCursors(Btree *pBtree, int errCode, int writeOnly)`
///
/// This routine sets the state to CURSOR_FAULT and the error
/// code to errCode for every cursor on any BtShared that pBtree
/// references.  Or if the writeOnly flag is set to 1, then only
/// trip write cursors and leave read cursors unchanged.
///
/// Every cursor is a candidate to be tripped, including cursors
/// that belong to other database connections that happen to be
/// sharing the cache with pBtree.
///
/// This routine gets called when a rollback occurs. If the writeOnly
/// flag is true, then only write-cursors need be tripped - read-only
/// cursors save their current positions so that they may continue
/// following the rollback. Or, if writeOnly is false, all cursors are
/// tripped. In general, writeOnly is false if the transaction being
/// rolled back modified the database schema. In this case b-tree root
/// pages may be moved or deleted from the database altogether, making
/// it unsafe for read cursors to continue.
///
/// If the writeOnly flag is true and an error is encountered while
/// saving the current position of a read-only cursor, all cursors,
/// including all read-cursors are tripped.
///
/// SQLITE_OK is returned if successful, or if an error occurs while
/// saving a cursor position, an SQLite error code.
pub fn sqlite3BtreeTripAllCursors(pBtree: &BtreeP, errCode: i32, writeOnly: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!((writeOnly == 0 || writeOnly == 1) && BTCF_WriteFlag == 1);
    let pBt = pBtree.borrow().pBt.clone();
    for p in cursorList(&pBt) {
        let mut c = p.borrow_mut();
        if writeOnly != 0 && (c.curFlags as i32 & BTCF_WriteFlag) == 0 {
            if c.eState == CURSOR_VALID as u8 || c.eState == CURSOR_SKIPNEXT as u8 {
                rc = saveCursorPosition(&mut c);
                if rc != SQLITE_OK {
                    drop(c);
                    let _ = sqlite3BtreeTripAllCursors(pBtree, rc, 0);
                    break;
                }
            }
        } else {
            btreeClearCursor(&mut c);
            c.eState = CURSOR_FAULT as u8;
            c.skipNext = errCode;
        }
        btreeReleaseAllCursorPages(&mut c);
    }
    rc
}

/// Set the pBt->nPage field correctly, according to the current
/// state of the database.  Assume pBt->pPage1 is valid.
fn btreeSetNPage(pBt: &BtSharedP, pPage1: &DbPage) {
    let mut nPage = get4byte(&pPage1.borrow().aData, 28) as i32;
    if nPage == 0 {
        let pager = pagerOf(pBt);
        sqlite3PagerPagecount(&mut pager.borrow_mut(), &mut nPage);
    }
    pBt.borrow_mut().nPage = nPage as u32;
}

/// C: `int sqlite3BtreeRollback(Btree *p, int tripCode, int writeOnly)`
///
/// Rollback the transaction in progress.
///
/// If tripCode is not SQLITE_OK then cursors will be invalidated (tripped).
/// Only write cursors are tripped if writeOnly is true but all cursors are
/// tripped if writeOnly is false.  Any attempt to use
/// a tripped cursor will result in an error.
///
/// This will release the write lock on the database file.  If there
/// are no active cursors, it also releases the read lock.
pub fn sqlite3BtreeRollback(db: &mut Sqlite3, p: &BtreeP, tripCode: i32, writeOnly: i32) -> i32 {
    let nVdbeRead = db.nVdbeRead;
    btreeRollback(p, tripCode, writeOnly, nVdbeRead)
}
pub fn btreeRollback(p: &BtreeP, tripCode: i32, writeOnly: i32, nVdbeRead: i32) -> i32 {
    let mut rc;
    let mut tripCode = tripCode;
    let mut writeOnly = writeOnly;
    let pBt = p.borrow().pBt.clone();

    debug_assert!(writeOnly == 1 || writeOnly == 0);
    debug_assert!(tripCode == SQLITE_ABORT_ROLLBACK || tripCode == SQLITE_OK);
    if tripCode == SQLITE_OK {
        tripCode = saveAllCursors(&pBt, 0, None);
        rc = tripCode;
        if rc != 0 {
            writeOnly = 0;
        }
    } else {
        rc = SQLITE_OK;
    }
    if tripCode != 0 {
        let rc2 = sqlite3BtreeTripAllCursors(p, tripCode, writeOnly);
        debug_assert!(rc == SQLITE_OK || (writeOnly == 0 && rc2 == SQLITE_OK));
        if rc2 != SQLITE_OK {
            rc = rc2;
        }
    }

    if p.borrow().inTrans == TRANS_WRITE as u8 {
        debug_assert!(TRANS_WRITE as u8 == pBt.borrow().inTransaction);
        let pager = pagerOf(&pBt);
        let rc2 = sqlite3PagerRollback(&mut pager.borrow_mut());
        if rc2 != SQLITE_OK {
            rc = rc2;
        }

        /* The rollback may have destroyed the pPage1->aData value.  So
         ** call btreeGetPage() on page 1 again to make
         ** sure pPage1->aData is set correctly. */
        let mut pPage1 = None;
        if btreeGetPage(&pBt, 1, &mut pPage1, 0) == SQLITE_OK {
            let pPage1 = pPage1.unwrap();
            btreeSetNPage(&pBt, &pPage1);
            releasePageOne(&pPage1);
        }
        debug_assert!(countValidCursors(&pBt, 1) == 0);
        pBt.borrow_mut().inTransaction = TRANS_READ as u8;
        btreeClearHasContent(&pBt);
    }

    btreeEndTransaction(p, nVdbeRead);
    rc
}

/// C: `int sqlite3BtreeBeginStmt(Btree *p, int iStatement)`
///
/// Start a statement subtransaction. The subtransaction can be rolled
/// back independently of the main transaction. You must start a transaction
/// before starting a subtransaction. The subtransaction is ended automatically
/// if the main transaction commits or rolls back.
///
/// Statement subtransactions are used around individual SQL statements
/// that are contained within a BEGIN...COMMIT block.  If a constraint
/// error occurs within the statement, the effect of that one statement
/// can be rolled back without having to rollback the entire transaction.
///
/// A statement sub-transaction is implemented as an anonymous savepoint. The
/// value passed as the second parameter is the total number of savepoints,
/// including the new anonymous savepoint, open on the B-Tree. i.e. if there
/// are no active savepoints and no other statement-transactions open,
/// iStatement is 1. This anonymous savepoint can be released or rolled back
/// using the sqlite3BtreeSavepoint() function.
pub fn sqlite3BtreeBeginStmt(db: &mut Sqlite3, p: &BtreeP, iStatement: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    debug_assert!(p.borrow().inTrans == TRANS_WRITE as u8);
    debug_assert!((pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0);
    debug_assert!(iStatement > 0);
    debug_assert!(iStatement > db.nSavepoint);
    debug_assert!(pBt.borrow().inTransaction == TRANS_WRITE as u8);
    /* At the pager level, a statement transaction is a savepoint with
     ** an index greater than all savepoints created explicitly using
     ** SQL statements. It is illegal to open, release or rollback any
     ** such savepoints while the statement transaction savepoint is active.
     */
    let pager = pagerOf(&pBt);
    let rc = sqlite3PagerOpenSavepoint(&mut pager.borrow_mut(), iStatement);
    rc
}

/// C: `int sqlite3BtreeSavepoint(Btree *p, int op, int iSavepoint)`
///
/// The second argument to this function, op, is always SAVEPOINT_ROLLBACK
/// or SAVEPOINT_RELEASE. This function either releases or rolls back the
/// savepoint identified by parameter iSavepoint, depending on the value
/// of op.
///
/// Normally, iSavepoint is greater than or equal to zero. However, if op is
/// SAVEPOINT_ROLLBACK, then iSavepoint may also be -1. In this case the
/// contents of the entire transaction are rolled back. This is different
/// from a normal transaction rollback, as no locks are released and the
/// transaction remains open.
pub fn sqlite3BtreeSavepoint(p: &BtreeP, op: i32, iSavepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;
    if p.borrow().inTrans == TRANS_WRITE as u8 {
        let pBt = p.borrow().pBt.clone();
        debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK);
        debug_assert!(iSavepoint >= 0 || (iSavepoint == -1 && op == SAVEPOINT_ROLLBACK));
        if op == SAVEPOINT_ROLLBACK {
            rc = saveAllCursors(&pBt, 0, None);
        }
        if rc == SQLITE_OK {
            let pager = pagerOf(&pBt);
            rc = sqlite3PagerSavepoint(&mut pager.borrow_mut(), op, iSavepoint);
        }
        if rc == SQLITE_OK {
            if iSavepoint < 0 && (pBt.borrow().btsFlags as i32 & BTS_INITIALLY_EMPTY) != 0 {
                pBt.borrow_mut().nPage = 0;
            }
            rc = newDatabase(&pBt);
            let pPage1 = page1Of(&pBt);
            btreeSetNPage(&pBt, &pPage1);

            /* pBt->nPage might be zero if the database was corrupt when
             ** the transaction was started. Otherwise, it must be at least 1.  */
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Cursors
// ---------------------------------------------------------------------------

/// Create a new cursor for the BTree whose root is on the page
/// iTable. If a read-only cursor is requested, it is assumed that
/// the caller already has at least a read-only transaction open
/// on the database already. If a write-cursor is requested, then
/// the caller is assumed to have an open write transaction.
///
/// If the BTREE_WRCSR bit of wrFlag is clear, then the cursor can only
/// be used for reading.  If the BTREE_WRCSR bit is set, then the cursor
/// can be used for reading or for writing if other conditions for writing
/// are also met.  These are the conditions that must be met in order
/// for writing to be allowed:
///
/// 1:  The cursor must have been opened with wrFlag containing BTREE_WRCSR
///
/// 2:  Other database connections that share the same pager cache
///     but which are not in the READ_UNCOMMITTED state may not have
///     cursors open with wrFlag==0 on the same table.  Otherwise
///     the changes made by this write cursor would be visible to
///     the read cursors in the other database connection.
///
/// 3:  The database must be writable (not on read-only media)
///
/// 4:  There must be an active transaction.
///
/// The BTREE_FORDELETE bit of wrFlag may optionally be set if BTREE_WRCSR
/// is set.  If FORDELETE is set, that is a hint to the implementation that
/// this cursor will only be used to seek to and delete entries of an index
/// as part of a larger DELETE statement.  The FORDELETE hint is not used by
/// this implementation.  But in a hypothetical alternative storage engine
/// in which index entries are automatically deleted when corresponding table
/// rows are deleted, the FORDELETE flag is a hint that all SEEK and DELETE
/// operations on this cursor can be no-ops and all READ operations can
/// return a null row (2-bytes: 0x01 0x00).
///
/// No checking is done to make sure that page iTable really is the
/// root page of a b-tree.  If it is not, then the cursor acquired
/// will not work correctly.
///
/// It is assumed that the sqlite3BtreeCursorZero() has been called
/// on pCur to initialize the memory space prior to invoking this routine.
fn btreeCursor(
    p: &BtreeP,
    iTable: Pgno,
    wrFlag: i32,
    pKeyInfo: Option<KeyInfoRef>,
    ppCur: &mut Option<BtCursorP>,
) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut iTable = iTable;
    debug_assert!(
        wrFlag == 0 || wrFlag == BTREE_WRCSR || wrFlag == (BTREE_WRCSR | BTREE_FORDELETE)
    );

    /* Assert that the caller has opened the required transaction. */
    debug_assert!(p.borrow().inTrans > TRANS_NONE as u8);
    debug_assert!(wrFlag == 0 || p.borrow().inTrans == TRANS_WRITE as u8);
    debug_assert!(pBt.borrow().pPage1.is_some());
    debug_assert!(wrFlag == 0 || (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0);

    if iTable <= 1 {
        if iTable < 1 {
            return SQLITE_CORRUPT_BKPT();
        } else if btreePagecount(&pBt) == 0 {
            debug_assert!(wrFlag == 0);
            iTable = 0;
        }
    }

    /* Now that no other errors can occur, finish filling in the BtCursor
     ** variables and link the cursor into the BtShared list.  */
    let mut cur = BtCursor::default();
    cur.pgnoRoot = iTable;
    cur.iPage = -1;
    cur.pKeyInfo = pKeyInfo;
    cur.pBtree = Some(p.clone());
    cur.pBt = Some(pBt.clone());
    cur.curFlags = 0;
    /* If there are two or more cursors on the same btree, then all such
     ** cursors *must* have the BTCF_Multiple flag set. */
    for pX in cursorList(&pBt) {
        let mut x = pX.borrow_mut();
        if x.pgnoRoot == iTable {
            x.curFlags |= BTCF_Multiple as u8;
            cur.curFlags = BTCF_Multiple as u8;
        }
    }
    cur.eState = CURSOR_INVALID as u8;
    let mut rc = SQLITE_OK;
    if wrFlag != 0 {
        cur.curFlags |= BTCF_WriteFlag as u8;
        cur.curPagerFlags = 0;
    } else {
        cur.curPagerFlags = PAGER_GET_READONLY as u8;
    }
    let pCur = Rc::new(RefCell::new(cur));
    {
        let mut bt = pBt.borrow_mut();
        bt.pCursor.retain(|w| w.strong_count() > 0);
        bt.pCursor.insert(0, Rc::downgrade(&pCur));
        if wrFlag != 0 && bt.pTmpSpace.is_empty() {
            rc = allocateTempSpace(&mut bt);
        }
    }
    *ppCur = Some(pCur);
    rc
}

/// C: `int sqlite3BtreeCursor( Btree *p, Pgno iTable, int wrFlag, struct KeyInfo *pKeyInfo, BtCursor *pCur )`
pub fn sqlite3BtreeCursor(
    p: &BtreeP,
    iTable: Pgno,
    wrFlag: i32,
    pKeyInfo: Option<KeyInfoRef>,
    ppCur: &mut Option<BtCursorP>,
) -> i32 {
    btreeCursor(p, iTable, wrFlag, pKeyInfo, ppCur)
}

/// C: `int sqlite3BtreeCursorSize(void)`
///
/// Return the size of a BtCursor object in bytes (the C value on a 64-bit
/// host; only used for memory accounting).
pub fn sqlite3BtreeCursorSize() -> i32 {
    296
}

/// C: `int sqlite3BtreeClosesWithCursor( Btree *pBtree, BtCursor *pCur )`
///
/// Return true if and only if the Btree object will be automatically
/// closed with the BtCursor closes.  This is used within assert() statements
/// only.
pub fn sqlite3BtreeClosesWithCursor(pBtree: &BtreeP, pCur: &BtCursorP) -> i32 {
    let pBt = pBtree.borrow().pBt.clone();
    if (pBt.borrow().openFlags as i32 & BTREE_SINGLE) == 0 {
        return 0;
    }
    let list = cursorList(&pBt);
    if list.len() != 1 || !Rc::ptr_eq(&list[0], pCur) {
        return 0;
    }
    match &pCur.borrow().pBtree {
        Some(b) if Rc::ptr_eq(b, pBtree) => 1,
        _ => 0,
    }
}

/// C: `void sqlite3BtreeCursorZero(BtCursor *p)`
///
/// Initialize memory that will be converted into a BtCursor object.
///
/// The simple approach here would be to memset() the entire object
/// to zero.  But it turns out that the apPage[] and aiIdx[] arrays
/// do not need to be zeroed and they are large, so we can save a lot
/// of run-time by skipping the initialization of those elements.
pub fn sqlite3BtreeCursorZero(p: &BtCursorP) {
    let mut c = p.borrow_mut();
    c.eState = 0;
    c.curFlags = 0;
    c.curPagerFlags = 0;
    c.hints = 0;
    c.skipNext = 0;
    c.pBtree = None;
    c.aOverflow = Vec::new();
    c.pKey = None;
}

/// C: `int sqlite3BtreeCloseCursor(BtCursor *pCur)`
///
/// Close a cursor.  The read lock on the database file is released
/// when the last cursor is closed.
pub fn sqlite3BtreeCloseCursor(pCur: &BtCursorP) -> i32 {
    let pBtree = pCur.borrow().pBtree.clone();
    if let Some(pBtree) = pBtree {
        let pBt = pCur.borrow().bt().clone();
        {
            let mut bt = pBt.borrow_mut();
            let me = Rc::as_ptr(pCur);
            bt.pCursor
                .retain(|w| w.strong_count() > 0 && w.as_ptr() != me);
        }
        {
            let mut c = pCur.borrow_mut();
            btreeReleaseAllCursorPages(&mut c);
        }
        unlockBtreeIfUnused(&pBt);
        {
            let mut c = pCur.borrow_mut();
            c.aOverflow = Vec::new();
            c.pKey = None;
        }
        if (pBt.borrow().openFlags as i32 & BTREE_SINGLE) != 0 && cursorList(&pBt).is_empty() {
            /* Since the BtShared is not sharable, there is no need to
             ** worry about the missing sqlite3BtreeLeave() call here.  */
            debug_assert!(pBtree.borrow().sharable == 0);
            btreeClose(None, &pBtree, 0);
        }
        pCur.borrow_mut().pBtree = None;
    }
    SQLITE_OK
}

/// Make sure the BtCursor* given in the argument has a valid
/// BtCursor.info structure.  If it is not already valid, call
/// btreeParseCell() to fill it in.
///
/// BtCursor.info is a cache of the information in the current cell.
/// Using this cache reduces the number of calls to btreeParseCell().
fn getCellInfo(pCur: &mut BtCursor) {
    if pCur.info.nSize == 0 {
        pCur.curFlags |= BTCF_ValidNKey as u8;
        btreeParseCell(
            &pCur.pPage.as_ref().unwrap().borrow(),
            pCur.ix as usize,
            &mut pCur.info,
        );
    }
}

/// C: `int sqlite3BtreeCursorIsValid(BtCursor *pCur)`
///
/// Return true if the given BtCursor is valid.  A valid cursor is one
/// that is currently pointing to a row in a (non-empty) table.
/// This is a verification routine is used only within assert() statements.
pub fn sqlite3BtreeCursorIsValid(pCur: &BtCursorP) -> i32 {
    (pCur.borrow().eState == CURSOR_VALID as u8) as i32
}

/// C: `int sqlite3BtreeCursorIsValidNN(BtCursor *pCur)`
pub fn sqlite3BtreeCursorIsValidNN(pCur: &BtCursorP) -> i32 {
    (pCur.borrow().eState == CURSOR_VALID as u8) as i32
}

/// C: `i64 sqlite3BtreeIntegerKey(BtCursor *pCur)`
///
/// Return the value of the integer key or "rowid" for a table btree.
/// This routine is only valid for a cursor that is pointing into a
/// ordinary table btree.  If the cursor points to an index btree or
/// is invalid, the result of this routine is undefined.
pub fn sqlite3BtreeIntegerKey(pCur: &BtCursorP) -> i64 {
    btreeIntegerKey(&mut pCur.borrow_mut())
}
fn btreeIntegerKey(pCur: &mut BtCursor) -> i64 {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.curIntKey != 0);
    getCellInfo(pCur);
    pCur.info.nKey
}

/// C: `void sqlite3BtreeCursorPin(BtCursor *pCur)`
///
/// Pin or unpin a cursor.
pub fn sqlite3BtreeCursorPin(pCur: &BtCursorP) {
    let mut c = pCur.borrow_mut();
    debug_assert!((c.curFlags as i32 & BTCF_Pinned) == 0);
    c.curFlags |= BTCF_Pinned as u8;
}

/// C: `void sqlite3BtreeCursorUnpin(BtCursor *pCur)`
pub fn sqlite3BtreeCursorUnpin(pCur: &BtCursorP) {
    let mut c = pCur.borrow_mut();
    debug_assert!((c.curFlags as i32 & BTCF_Pinned) != 0);
    c.curFlags &= !(BTCF_Pinned as u8);
}

/// C: `i64 sqlite3BtreeOffset(BtCursor *pCur)`
///
/// Return the offset into the database file for the start of the
/// payload to which the cursor is pointing.
pub fn sqlite3BtreeOffset(pCur: &BtCursorP) -> i64 {
    let mut c = pCur.borrow_mut();
    debug_assert!(c.eState == CURSOR_VALID as u8);
    getCellInfo(&mut c);
    let pageSize = c.bt().borrow().pageSize as i64;
    let pgno = pgnoOf(c.page()) as i64;
    pageSize * (pgno - 1) + c.info.pPayload as i64
}

/// C: `u32 sqlite3BtreePayloadSize(BtCursor *pCur)`
///
/// Return the number of bytes of payload for the entry that pCur is
/// currently pointing to.  For table btrees, this will be the amount
/// of data.  For index btrees, this will be the size of the key.
///
/// The caller must guarantee that the cursor is pointing to a non-NULL
/// valid entry.  In other words, the calling procedure must guarantee
/// that the cursor has Cursor.eState==CURSOR_VALID.
pub fn sqlite3BtreePayloadSize(pCur: &BtCursorP) -> u32 {
    btreePayloadSize(&mut pCur.borrow_mut())
}
fn btreePayloadSize(pCur: &mut BtCursor) -> u32 {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    getCellInfo(pCur);
    pCur.info.nPayload
}

/// C: `sqlite3_int64 sqlite3BtreeMaxRecordSize(BtCursor *pCur)`
///
/// Return an upper bound on the size of any record for the table
/// that the cursor is pointing into.
///
/// This is an optimization.  Everything will still work if this
/// routine always returns 2147483647 (which is the largest record
/// that SQLite can handle) or more.  But returning a smaller value might
/// prevent large memory allocations when trying to interpret a
/// corrupt database.
///
/// The current implementation merely returns the size of the underlying
/// database file.
pub fn sqlite3BtreeMaxRecordSize(pCur: &BtCursorP) -> i64 {
    let c = pCur.borrow();
    debug_assert!(c.eState == CURSOR_VALID as u8);
    let bt = c.bt().borrow();
    bt.pageSize as i64 * bt.nPage as i64
}

/// Given the page number of an overflow page in the database (parameter
/// ovfl), this function finds the page number of the next page in the
/// linked list of overflow pages. If possible, it uses the auto-vacuum
/// pointer-map data instead of reading the content of page ovfl to do so.
///
/// If an error occurs an SQLite error code is returned. Otherwise:
///
/// The page number of the next overflow page in the linked list is
/// written to *pPgnoNext. If page ovfl is the last page in its linked
/// list, *pPgnoNext is set to zero.
///
/// If ppPage is not NULL, and a reference to the MemPage object corresponding
/// to page number pOvfl was obtained, then *ppPage is set to point to that
/// reference. It is the responsibility of the caller to call releasePage()
/// on *ppPage to free the reference. In no reference was obtained (because
/// the pointer-map was used to obtain the value for *pPgnoNext), then
/// *ppPage is set to zero.
fn getOverflowPage(
    pBt: &BtSharedP,
    ovfl: Pgno,
    ppPage: Option<&mut Option<DbPage>>,
    pPgnoNext: &mut Pgno,
) -> i32 {
    let mut next: Pgno = 0;
    let mut pPage: Option<DbPage> = None;
    let mut rc = SQLITE_OK;

    /* Try to find the next page in the overflow list using the
     ** autovacuum pointer-map pages. Guess that the next page in
     ** the overflow list is page number (ovfl+1). If that guess turns
     ** out to be wrong, fall back to loading the data of page
     ** number ovfl to determine the next page number.
     */
    if pBt.borrow().autoVacuum != 0 {
        let mut pgno: Pgno = 0;
        let mut iGuess = ovfl.wrapping_add(1);
        let mut eType = 0u8;

        while PTRMAP_ISPAGE(pBt, iGuess) || iGuess == pendingBytePage(pBt) {
            iGuess = iGuess.wrapping_add(1);
        }

        if iGuess <= btreePagecount(pBt) {
            rc = ptrmapGet(pBt, iGuess, &mut eType, Some(&mut pgno));
            if rc == SQLITE_OK && eType == PTRMAP_OVERFLOW2 as u8 && pgno == ovfl {
                next = iGuess;
                rc = SQLITE_DONE;
            }
        }
    }

    debug_assert!(next == 0 || rc == SQLITE_DONE);
    if rc == SQLITE_OK {
        let flags = if ppPage.is_none() {
            PAGER_GET_READONLY
        } else {
            0
        };
        rc = btreeGetPage(pBt, ovfl, &mut pPage, flags);
        if rc == SQLITE_OK {
            next = get4byte(&pPage.as_ref().unwrap().borrow().aData, 0);
        }
    }

    *pPgnoNext = next;
    if let Some(pp) = ppPage {
        *pp = pPage;
    } else {
        releasePage(pPage.as_ref());
    }
    if rc == SQLITE_DONE { SQLITE_OK } else { rc }
}

/// Copy data from a buffer to a page, or from a page to a buffer.
///
/// pPayload is the offset in the page image pDbPage, pBuf the buffer.
/// If eOp is false, then nByte bytes of data are copied from pPayload to
/// pBuf. Otherwise, the data is copied from pBuf to pPayload.  When
/// writing, sqlite3PagerWrite() is called on pDbPage first.
fn copyPayload(pDbPage: &DbPage, pPayload: usize, pBuf: &mut [u8], nByte: usize, eOp: i32) -> i32 {
    if eOp != 0 {
        /* Copy data from buffer to page (a write operation) */
        let rc = sqlite3PagerWrite(pDbPage);
        if rc != SQLITE_OK {
            return rc;
        }
        pDbPage.borrow_mut().aData[pPayload..pPayload + nByte].copy_from_slice(&pBuf[..nByte]);
    } else {
        /* Copy data from page to buffer (a read operation) */
        pBuf[..nByte].copy_from_slice(&pDbPage.borrow().aData[pPayload..pPayload + nByte]);
    }
    SQLITE_OK
}

/// This function is used to read or overwrite payload information
/// for the entry that the pCur cursor is pointing to. The eOp
/// argument is interpreted as follows:
///
///   0: The operation is a read. Populate the overflow cache.
///   1: The operation is a write. Populate the overflow cache.
///
/// A total of "amt" bytes are read or written beginning at "offset".
/// Data is read to or from the buffer pBuf.
///
/// The content being read or written might appear on the main page
/// or be scattered out on multiple overflow pages.
///
/// If the current cursor entry uses one or more overflow pages
/// this function may allocate space for and lazily populate
/// the overflow page-list cache array (BtCursor.aOverflow).
/// Subsequent calls use this cache to make seeking to the supplied offset
/// more efficient.
///
/// Once an overflow page-list cache has been allocated, it must be
/// invalidated if some other cursor writes to the same table, or if
/// the cursor is moved to a different row. Additionally, in auto-vacuum
/// mode, the following events may invalidate an overflow page-list cache.
///
///   * An incremental vacuum,
///   * A commit in auto_vacuum="full" mode,
///   * Creating a table (may require moving an overflow page).
fn accessPayload(pCur: &mut BtCursor, offset: u32, amt: u32, pBuf: &mut [u8], eOp: i32) -> i32 {
    let mut offset = offset;
    let mut amt = amt;
    let mut rc = SQLITE_OK;
    let mut iIdx: usize = 0;
    let mut bi: usize = 0; /* Current position in pBuf */
    let pPage = pCur.page().clone(); /* Btree page of current entry */
    let pBt = pCur.bt().clone(); /* Btree this cursor belongs to */

    debug_assert!(eOp == 0 || eOp == 1);
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    if pCur.ix >= mp(&pPage).nCell {
        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
    }

    getCellInfo(pCur);
    let aPayload = pCur.info.pPayload;
    let nLocal = pCur.info.nLocal as u32;
    debug_assert!(offset as u64 + amt as u64 <= pCur.info.nPayload as u64);

    let (usableSize, nPageDb) = {
        let b = pBt.borrow();
        (b.usableSize, b.nPage)
    };
    if aPayload as u64 > usableSize.wrapping_sub(nLocal) as u64 {
        /* Trying to read or write past the end of the data is an error.  The
         ** conditional above is really:
         **    &aPayload[pCur->info.nLocal] > &pPage->aData[pBt->usableSize]
         ** but is recast into its current form to avoid integer overflow problems
         */
        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
    }

    /* Check if data must be read/written to/from the btree page itself. */
    if offset < nLocal {
        let mut a = amt;
        if a + offset > nLocal {
            a = nLocal - offset;
        }
        rc = copyPayload(
            &pPage,
            aPayload + offset as usize,
            &mut pBuf[bi..],
            a as usize,
            eOp,
        );
        offset = 0;
        bi += a as usize;
        amt -= a;
    } else {
        offset -= nLocal;
    }

    if rc == SQLITE_OK && amt > 0 {
        let ovflSize = usableSize - 4; /* Bytes content per ovfl page */
        let mut nextPage = get4byte(&pPage.borrow().aData, aPayload + nLocal as usize);

        /* If the BtCursor.aOverflow[] has not been allocated, allocate it now.
         **
         ** The aOverflow[] array is sized at one entry for each overflow page
         ** in the overflow chain. The page number of the first overflow page is
         ** stored in aOverflow[0], etc. A value of 0 in the aOverflow[] array
         ** means "not yet known" (the cache is lazily populated).
         */
        if (pCur.curFlags as i32 & BTCF_ValidOvfl) == 0 {
            let mut nOvfl = pCur.info.nPayload as i64;
            nOvfl = (nOvfl - nLocal as i64 + ovflSize as i64 - 1) / ovflSize as i64;
            let nOvfl = nOvfl.max(0) as usize;
            if pCur.aOverflow.len() < nOvfl {
                pCur.aOverflow.resize(nOvfl * 2, 0);
            }
            for x in &mut pCur.aOverflow[..nOvfl] {
                *x = 0;
            }
            pCur.curFlags |= BTCF_ValidOvfl as u8;
        } else {
            /* If the overflow page-list cache has been allocated and the
             ** entry for the first required overflow page is valid, skip
             ** directly to it.
             */
            let k = (offset / ovflSize) as usize;
            if pCur.aOverflow.get(k).copied().unwrap_or(0) != 0 {
                iIdx = k;
                nextPage = pCur.aOverflow[iIdx];
                offset %= ovflSize;
            }
        }

        debug_assert!(rc == SQLITE_OK && amt > 0);
        while nextPage != 0 {
            /* If required, populate the overflow page-list cache. */
            if nextPage > nPageDb {
                return SQLITE_CORRUPT_BKPT();
            }
            if pCur.aOverflow.len() <= iIdx + 1 {
                pCur.aOverflow.resize(iIdx + 2, 0);
            }
            pCur.aOverflow[iIdx] = nextPage;

            if offset >= ovflSize {
                /* The only reason to read this page is to obtain the page
                 ** number for the next page in the overflow chain. The page
                 ** data is not required. So first try to lookup the overflow
                 ** page-list cache, if any, then fall back to the getOverflowPage()
                 ** function.
                 */
                if pCur.aOverflow[iIdx + 1] != 0 {
                    nextPage = pCur.aOverflow[iIdx + 1];
                } else {
                    let cur = nextPage;
                    rc = getOverflowPage(&pBt, cur, None, &mut nextPage);
                }
                offset -= ovflSize;
            } else {
                /* Need to read this page properly. It contains some of the
                 ** range of data that is being read (eOp==0) or written (eOp!=0).
                 */
                let mut a = amt;
                if a + offset > ovflSize {
                    a = ovflSize - offset;
                }

                /* SQLITE_DIRECT_OVERFLOW_READ (enabled by default):
                 ** If all the following are true:
                 **
                 **   1) this is a read operation, and
                 **   2) data is required from the start of this overflow page, and
                 **   3) there are no dirty pages in the page-cache
                 **   4) the database is file-backed, and
                 **   5) the page is not in the WAL file
                 **   6) at least 4 bytes have already been read into the output buffer
                 **
                 ** then data can be read directly from the database file into the
                 ** output buffer, bypassing the page-cache altogether. This speeds
                 ** up loading large records that span many overflow pages.
                 */
                let direct = eOp == 0 /* (1) */
                    && offset == 0 /* (2) */
                    && {
                        let pager = pagerOf(&pBt);
                        let ok = sqlite3PagerDirectReadOk(&mut pager.borrow_mut(), nextPage);
                        ok != 0 /* (3,4,5) */
                    }
                    && bi >= 4; /* (6) */
                let mut pDbPage = None;
                if direct {
                    let aWrite = bi - 4;
                    let aSave: [u8; 4] = pBuf[aWrite..bi].try_into().unwrap();
                    let pageSize = pBt.borrow().pageSize as i64;
                    let pager = pagerOf(&pBt);
                    {
                        let mut pg = pager.borrow_mut();
                        let fd = sqlite3PagerFile(&mut pg);
                        rc = crate::os::sqlite3OsRead(
                            fd,
                            &mut pBuf[aWrite..bi + a as usize],
                            a as i32 + 4,
                            pageSize * (nextPage as i64 - 1),
                        );
                    }
                    nextPage = get4byte(pBuf, aWrite);
                    pBuf[aWrite..bi].copy_from_slice(&aSave);
                } else {
                    rc = pagerGet(
                        &pBt,
                        nextPage,
                        &mut pDbPage,
                        if eOp == 0 { PAGER_GET_READONLY } else { 0 },
                    );
                }
                if !direct && rc == SQLITE_OK {
                    let pDbPage = pDbPage.unwrap();
                    if eOp != 0
                        && (sqlite3PagerPageRefcount(&pDbPage) != 1
                            || pDbPage.borrow().pExtra.isInit != 0)
                    {
                        sqlite3PagerUnref(&pDbPage);
                        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
                    }
                    nextPage = get4byte(&pDbPage.borrow().aData, 0);
                    rc = copyPayload(
                        &pDbPage,
                        offset as usize + 4,
                        &mut pBuf[bi..],
                        a as usize,
                        eOp,
                    );
                    sqlite3PagerUnref(&pDbPage);
                    offset = 0;
                }
                amt -= a;
                if amt == 0 {
                    return rc;
                }
                bi += a as usize;
            }
            if rc != 0 {
                break;
            }
            iIdx += 1;
        }
    }

    if rc == SQLITE_OK && amt > 0 {
        /* Overflow chain ends prematurely */
        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
    }
    rc
}

/// C: `int sqlite3BtreePayload(BtCursor *pCur, u32 offset, u32 amt, void *pBuf)`
///
/// Read part of the payload for the row at which that cursor pCur is currently
/// pointing.  "amt" bytes will be transferred into pBuf[].  The transfer
/// begins at "offset".
///
/// pCur can be pointing to either a table or an index b-tree.
/// If pointing to a table btree, then the content section is read.  If
/// pCur is pointing to an index b-tree then the key section is read.
///
/// For sqlite3BtreePayload(), the caller must ensure that pCur is pointing
/// to a valid row in the table.  For sqlite3BtreePayloadChecked(), the
/// cursor might be invalid or might need to be restored before being read.
///
/// Return SQLITE_OK on success or an error code if anything goes
/// wrong.  An error is returned if "offset+amt" is larger than
/// the available payload.
pub fn sqlite3BtreePayload(pCur: &BtCursorP, offset: u32, amt: u32, pBuf: &mut [u8]) -> i32 {
    let mut c = pCur.borrow_mut();
    debug_assert!(c.eState == CURSOR_VALID as u8);
    debug_assert!(c.iPage >= 0 && c.pPage.is_some());
    accessPayload(&mut c, offset, amt, pBuf, 0)
}

/// This variant of sqlite3BtreePayload() works even if the cursor has not
/// in the CURSOR_VALID state.  It is only used by the sqlite3_blob_read()
/// interface.
fn accessPayloadChecked(pCur: &mut BtCursor, offset: u32, amt: u32, pBuf: &mut [u8]) -> i32 {
    if pCur.eState == CURSOR_INVALID as u8 {
        return SQLITE_ABORT;
    }
    let rc = btreeRestoreCursorPosition(pCur);
    if rc != 0 {
        rc
    } else {
        accessPayload(pCur, offset, amt, pBuf, 0)
    }
}

/// C: `int sqlite3BtreePayloadChecked(BtCursor *pCur, u32 offset, u32 amt, void *pBuf)`
pub fn sqlite3BtreePayloadChecked(pCur: &BtCursorP, offset: u32, amt: u32, pBuf: &mut [u8]) -> i32 {
    let mut c = pCur.borrow_mut();
    if c.eState == CURSOR_VALID as u8 {
        accessPayload(&mut c, offset, amt, pBuf, 0)
    } else {
        accessPayloadChecked(&mut c, offset, amt, pBuf)
    }
}

/// Return a copy of the payload for the entry that the cursor is pointing
/// to and set *pAmt to the number of bytes of payload available.  This is
/// only the local part of the payload (C returns a pointer into the page).
///
/// This routine is an optimization.  It is common for the entire key
/// and data to fit on the local page and for there to be no overflow
/// pages.  When that is so, this routine can be used to access the
/// key and data without making a copy.  If the key and/or data spills
/// onto overflow pages, then accessPayload() must be used to reassemble
/// the key/data and copy it into a preallocated buffer.
fn fetchPayload(pCur: &mut BtCursor, pAmt: &mut u32) -> Vec<u8> {
    debug_assert!(pCur.iPage >= 0 && pCur.pPage.is_some());
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.info.nSize > 0);
    let g = pCur.page().borrow();
    let end = g.pExtra.aDataEnd();
    let mut amt = pCur.info.nLocal as i64;
    let p = pCur.info.pPayload;
    if amt > end as i64 - p as i64 {
        /* There is too little space on the page for the expected amount
         ** of local content. Database must be corrupt. */
        amt = std::cmp::max(0, end as i64 - p as i64);
    }
    *pAmt = amt as u32;
    if amt == 0 {
        return Vec::new();
    }
    g.aData[p..p + amt as usize].to_vec()
}

/// C: `const void * sqlite3BtreePayloadFetch(BtCursor *pCur, u32 *pAmt)`
/// Rust: returns a copy of the local payload of the current cell.
///
/// For the entry that cursor pCur is point to, return as
/// many bytes of the key or data as are available on the local
/// b-tree page.  Write the number of available bytes into *pAmt.
///
/// The pointer returned is ephemeral.  The key/data may move
/// or be destroyed on the next call to any Btree routine,
/// including calls from other threads against the same cache.
/// Hence, a mutex on the BtShared should be held prior to calling
/// this routine.
///
/// These routines is used to get quick access to key and data
/// in the common case where no overflow pages are used.
pub fn sqlite3BtreePayloadFetch(pCur: &BtCursorP, pAmt: &mut u32) -> Vec<u8> {
    fetchPayload(&mut pCur.borrow_mut(), pAmt)
}

/// Move the cursor down to a new child page.  The newPgno argument is the
/// page number of the child page to move to.
///
/// This function returns SQLITE_CORRUPT if the page-header flags field of
/// the new child page does not match the flags field of the parent (i.e.
/// if an intkey page appears to be the parent of a non-intkey page, or
/// vice-versa).
fn moveToChild(pCur: &mut BtCursor, newPgno: u32) -> i32 {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.iPage < BTCURSOR_MAX_DEPTH as i8);
    debug_assert!(pCur.iPage >= 0);
    if pCur.iPage >= (BTCURSOR_MAX_DEPTH - 1) as i8 {
        return SQLITE_CORRUPT_BKPT();
    }
    pCur.info.nSize = 0;
    pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
    let ip = pCur.iPage as usize;
    pCur.aiIdx[ip] = pCur.ix;
    pCur.apPage[ip] = pCur.pPage.take();
    pCur.ix = 0;
    pCur.iPage += 1;
    let pBt = pCur.bt().clone();
    let flags = pCur.curPagerFlags as i32;
    let mut rc = getAndInitPage(&pBt, newPgno, &mut pCur.pPage, flags);
    debug_assert!(pCur.pPage.is_some() || rc != SQLITE_OK);
    if rc == SQLITE_OK {
        let bad = {
            let m = mp(pCur.page());
            m.nCell < 1 || m.intKey != pCur.curIntKey
        };
        if bad {
            let pg = pCur.pPage.take();
            releasePage(pg.as_ref());
            rc = SQLITE_CORRUPT_PGNO(newPgno);
        }
    }
    if rc != 0 {
        pCur.iPage -= 1;
        pCur.pPage = pCur.apPage[pCur.iPage as usize].take();
    }
    rc
}

/// Move the cursor up to the parent page.
///
/// pCur->idx is set to the cell index that contains the pointer
/// to the page we are coming from.  If we are coming from the
/// right-most child page then pCur->idx is set to one more than
/// the largest cell index.
fn moveToParent(pCur: &mut BtCursor) {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.iPage > 0);
    debug_assert!(pCur.pPage.is_some());
    pCur.info.nSize = 0;
    pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
    pCur.ix = pCur.aiIdx[pCur.iPage as usize - 1];
    let pLeaf = pCur.pPage.take().unwrap();
    pCur.iPage -= 1;
    pCur.pPage = pCur.apPage[pCur.iPage as usize].take();
    releasePageNotNull(&pLeaf);
}

/// Move the cursor to point to the root page of its b-tree structure.
///
/// If the table has a virtual root page, then the cursor is moved to point
/// to the virtual root page instead of the actual root page. A table has a
/// virtual root page when the actual root page contains no cells and a
/// single child page. This can only happen with the table rooted at page 1.
///
/// If the b-tree structure is empty, the cursor state is set to
/// CURSOR_INVALID and this routine returns SQLITE_EMPTY. Otherwise,
/// the cursor is set to point to the first cell located on the root
/// (or virtual root) page and the cursor state is set to CURSOR_VALID.
///
/// If this function returns successfully, it may be assumed that the
/// page-header flags indicate that the [virtual] root-page is the expected
/// kind of b-tree page (i.e. if when opening the cursor the caller did not
/// specify a KeyInfo structure the flags byte is set to 0x05 or 0x0D,
/// indicating a table b-tree, or if the caller did specify a KeyInfo
/// structure the flags byte is set to 0x02 or 0x0A, indicating an index
/// b-tree).
fn moveToRoot(pCur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pCur.eState < CURSOR_REQUIRESEEK as u8 || pCur.iPage < 0);
    debug_assert!(pCur.pgnoRoot > 0 || pCur.iPage < 0);

    let mut skipInit = false;
    if pCur.iPage >= 0 {
        if pCur.iPage != 0 {
            let pg = pCur.pPage.take().unwrap();
            releasePageNotNull(&pg);
            loop {
                pCur.iPage -= 1;
                if pCur.iPage == 0 {
                    break;
                }
                let pg = pCur.apPage[pCur.iPage as usize].take().unwrap();
                releasePageNotNull(&pg);
            }
            pCur.pPage = pCur.apPage[0].take();
            skipInit = true;
        }
    } else if pCur.pgnoRoot == 0 {
        pCur.eState = CURSOR_INVALID as u8;
        return SQLITE_EMPTY;
    } else {
        debug_assert!(pCur.iPage == -1);
        if pCur.eState >= CURSOR_REQUIRESEEK as u8 {
            if pCur.eState == CURSOR_FAULT as u8 {
                debug_assert!(pCur.skipNext != SQLITE_OK);
                return pCur.skipNext;
            }
            btreeClearCursor(pCur);
        }
        let pBt = pCur.bt().clone();
        let root = pCur.pgnoRoot;
        let flags = pCur.curPagerFlags as i32;
        rc = getAndInitPage(&pBt, root, &mut pCur.pPage, flags);
        if rc != SQLITE_OK {
            pCur.eState = CURSOR_INVALID as u8;
            return rc;
        }
        pCur.iPage = 0;
        let ik = mp(pCur.page()).intKey;
        pCur.curIntKey = ik;
    }
    let pRoot = pCur.page().clone();

    if !skipInit {
        /* If pCur->pKeyInfo is not NULL, then the caller that opened this cursor
         ** expected to open it on an index b-tree. Otherwise, if pKeyInfo is
         ** NULL, the caller expects a table b-tree. If this is not the case,
         ** return an SQLITE_CORRUPT error.
         **
         ** Earlier versions of SQLite assumed that this test could not fail
         ** if the root page was already loaded when this function was called (i.e.
         ** if pCur->iPage>=0). But this is not so if the database is corrupted
         ** in such a way that page pRoot is linked into a second b-tree table
         ** (or the freelist).  */
        let m = mp(&pRoot);
        debug_assert!(m.intKey == 1 || m.intKey == 0);
        if m.isInit == 0 || (pCur.pKeyInfo.is_none() as u8) != m.intKey {
            return SQLITE_CORRUPT_PAGE(&m);
        }
    }

    /* skip_init: */
    pCur.ix = 0;
    pCur.info.nSize = 0;
    pCur.curFlags &= !((BTCF_AtLast | BTCF_ValidNKey | BTCF_ValidOvfl) as u8);

    let (nCell, leaf, pgno, subpage) = {
        let g = pRoot.borrow();
        let m = &g.pExtra;
        (
            m.nCell,
            m.leaf,
            m.pgno,
            get4byte(&g.aData, m.hdrOffset as usize + 8),
        )
    };
    if nCell > 0 {
        pCur.eState = CURSOR_VALID as u8;
    } else if leaf == 0 {
        if pgno != 1 {
            return SQLITE_CORRUPT_BKPT();
        }
        pCur.eState = CURSOR_VALID as u8;
        rc = moveToChild(pCur, subpage);
    } else {
        pCur.eState = CURSOR_INVALID as u8;
        rc = SQLITE_EMPTY;
    }
    rc
}

/// Move the cursor down to the left-most leaf entry beneath the
/// entry to which it is currently pointing.
///
/// The left-most leaf is the one with the smallest key - the first
/// in ascending order.
fn moveToLeftmost(pCur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    while rc == SQLITE_OK {
        let pgno = {
            let g = pCur.page().borrow();
            if g.pExtra.leaf != 0 {
                break;
            }
            debug_assert!(pCur.ix < g.pExtra.nCell);
            get4byte(&g.aData, findCell(&g.pExtra, &g.aData, pCur.ix as usize))
        };
        rc = moveToChild(pCur, pgno);
    }
    rc
}

/// Move the cursor down to the right-most leaf entry beneath the
/// page to which it is currently pointing.  Notice the difference
/// between moveToLeftmost() and moveToRightmost().  moveToLeftmost()
/// finds the left-most entry beneath the *entry* whereas moveToRightmost()
/// finds the right-most entry beneath the *page*.
///
/// The right-most entry is the one with the largest key - the last
/// key in ascending order.
fn moveToRightmost(pCur: &mut BtCursor) -> i32 {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    loop {
        let (leaf, nCell, pgno) = {
            let g = pCur.page().borrow();
            (
                g.pExtra.leaf,
                g.pExtra.nCell,
                get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8),
            )
        };
        if leaf != 0 {
            pCur.ix = nCell.wrapping_sub(1);
            break;
        }
        pCur.ix = nCell;
        let rc = moveToChild(pCur, pgno);
        if rc != 0 {
            return rc;
        }
    }
    debug_assert!(pCur.info.nSize == 0);
    debug_assert!((pCur.curFlags as i32 & BTCF_ValidNKey) == 0);
    SQLITE_OK
}

/// C: `int sqlite3BtreeFirst(BtCursor *pCur, int *pRes)`
///
/// Move the cursor to the first entry in the table.  Return SQLITE_OK
/// on success.  Set *pRes to 0 if the cursor actually points to something
/// or set *pRes to 1 if the table is empty.
pub fn sqlite3BtreeFirst(db: &mut Sqlite3, pCur: &BtCursorP, pRes: &mut i32) -> i32 {
    cursorFirst(&mut pCur.borrow_mut(), pRes)
}
pub fn cursorFirst(pCur: &mut BtCursor, pRes: &mut i32) -> i32 {
    let mut rc = moveToRoot(pCur);
    if rc == SQLITE_OK {
        *pRes = 0;
        rc = moveToLeftmost(pCur);
    } else if rc == SQLITE_EMPTY {
        *pRes = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// C: `int sqlite3BtreeIsEmpty(BtCursor *pCur, int *pRes)`
///
/// Set *pRes to 1 (true) if the BTree pointed to by cursor pCur contains zero
/// rows of content.  Set *pRes to 0 (false) if the table contains content.
/// Return SQLITE_OK on success or some error code (ex: SQLITE_NOMEM) if
/// something goes wrong.
pub fn sqlite3BtreeIsEmpty(db: &mut Sqlite3, pCur: &BtCursorP, pRes: &mut i32) -> i32 {
    let mut c = pCur.borrow_mut();
    if c.eState == CURSOR_VALID as u8 {
        *pRes = 0;
        return SQLITE_OK;
    }
    let mut rc = moveToRoot(&mut c);
    if rc == SQLITE_EMPTY {
        *pRes = 1;
        rc = SQLITE_OK;
    } else {
        *pRes = 0;
    }
    rc
}

/// Move the cursor to the last entry in the table.  Return SQLITE_OK
/// on success.  Set *pRes to 0 if the cursor actually points to something
/// or set *pRes to 1 if the table is empty.
fn btreeLast(pCur: &mut BtCursor, pRes: &mut i32) -> i32 {
    let mut rc = moveToRoot(pCur);
    if rc == SQLITE_OK {
        debug_assert!(pCur.eState == CURSOR_VALID as u8);
        *pRes = 0;
        rc = moveToRightmost(pCur);
        if rc == SQLITE_OK {
            pCur.curFlags |= BTCF_AtLast as u8;
        } else {
            pCur.curFlags &= !(BTCF_AtLast as u8);
        }
    } else if rc == SQLITE_EMPTY {
        *pRes = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// C: `int sqlite3BtreeLast(BtCursor *pCur, int *pRes)`
pub fn sqlite3BtreeLast(db: &mut Sqlite3, pCur: &BtCursorP, pRes: &mut i32) -> i32 {
    cursorLast(&mut pCur.borrow_mut(), pRes)
}
pub fn cursorLast(pCur: &mut BtCursor, pRes: &mut i32) -> i32 {
    /* If the cursor already points to the last entry, this is a no-op. */
    if CURSOR_VALID as u8 == pCur.eState && (pCur.curFlags as i32 & BTCF_AtLast) != 0 {
        *pRes = 0;
        return SQLITE_OK;
    }
    btreeLast(pCur, pRes)
}

/// C: `int sqlite3BtreeTableMoveto( BtCursor *pCur, i64 intKey, int biasRight, int *pRes )`
///
/// Move the cursor so that it points to an entry in a table (a.k.a INTKEY)
/// table near the key intKey.   Return a success code.
///
/// If an exact match is not found, then the cursor is always
/// left pointing at a leaf page which would hold the entry if it
/// were present.  The cursor might point to an entry that comes
/// before or after the key.
///
/// An integer is written into *pRes which is the result of
/// comparing the key with the entry to which the cursor is
/// pointing.  The meaning of the integer written into
/// *pRes is as follows:
///
///     *pRes<0      The cursor is left pointing at an entry that
///                  is smaller than intKey or if the table is empty
///                  and the cursor is therefore left point to nothing.
///
///     *pRes==0     The cursor is left pointing at an entry that
///                  exactly matches intKey.
///
///     *pRes>0      The cursor is left pointing at an entry that
///                  is larger than intKey.
pub fn sqlite3BtreeTableMoveto(
    db: &mut Sqlite3,
    pCur: &BtCursorP,
    intKey: i64,
    biasRight: i32,
    pRes: &mut i32,
) -> i32 {
    btreeTableMoveto(&mut pCur.borrow_mut(), intKey, biasRight, pRes)
}
pub fn btreeTableMoveto(pCur: &mut BtCursor, intKey: i64, biasRight: i32, pRes: &mut i32) -> i32 {
    let mut rc;

    debug_assert!(pCur.pKeyInfo.is_none());
    debug_assert!(pCur.eState != CURSOR_VALID as u8 || pCur.curIntKey != 0);

    /* If the cursor is already positioned at the point we are trying
     ** to move to, then just return without doing any work */
    if pCur.eState == CURSOR_VALID as u8 && (pCur.curFlags as i32 & BTCF_ValidNKey) != 0 {
        if pCur.info.nKey == intKey {
            *pRes = 0;
            return SQLITE_OK;
        }
        if pCur.info.nKey < intKey {
            if (pCur.curFlags as i32 & BTCF_AtLast) != 0 {
                *pRes = -1;
                return SQLITE_OK;
            }
            /* If the requested key is one more than the previous key, then
             ** try to get there using sqlite3BtreeNext() rather than a full
             ** binary search.  This is an optimization only.  The correct answer
             ** is still obtained without this case, only a little more slowly. */
            if pCur.info.nKey.wrapping_add(1) == intKey {
                *pRes = 0;
                rc = cursorNext(pCur, 0);
                if rc == SQLITE_OK {
                    getCellInfo(pCur);
                    if pCur.info.nKey == intKey {
                        return SQLITE_OK;
                    }
                } else if rc != SQLITE_DONE {
                    return rc;
                }
            }
        }
    }

    if let Some(b) = &pCur.pBtree {
        b.borrow_mut().nSeek += 1; /* Performance measurement during testing */
    }

    rc = moveToRoot(pCur);
    if rc != 0 {
        if rc == SQLITE_EMPTY {
            *pRes = -1;
            return SQLITE_OK;
        }
        return rc;
    }
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.curIntKey != 0);

    loop {
        let pPage = pCur.page().clone();
        let mut lwr: i32;
        let mut upr: i32;
        let mut idx: i32;
        let mut c: i32 = 0;
        let leaf;
        let nCell;
        /* Binary search within the page */
        let found: Option<i64> = {
            let g = pPage.borrow();
            let m = &g.pExtra;
            let data = &g.aData;
            /* pPage->nCell must be greater than zero. If this is the root-page
             ** the cursor would have been INVALID above and this for(;;) loop
             ** not run. If this is not the root-page, then the moveToChild() routine
             ** would have already detected db corruption. Similarly, pPage must
             ** be the right kind (index or table) of b-tree page. Otherwise
             ** a moveToChild() or moveToRoot() call would have detected corruption.  */
            debug_assert!(m.nCell > 0);
            debug_assert!(m.intKey != 0);
            leaf = m.leaf;
            nCell = m.nCell as i32;
            lwr = 0;
            upr = m.nCell as i32 - 1;
            debug_assert!(biasRight == 0 || biasRight == 1);
            idx = upr >> (1 - biasRight); /* idx = biasRight ? upr : (lwr+upr)/2; */
            let mut found = None;
            loop {
                let mut pCell = findCellPastPtr(m, data, idx as usize);
                if m.intKeyLeaf != 0 {
                    loop {
                        let b = at(data, pCell);
                        pCell += 1;
                        if b < 0x80 {
                            break;
                        }
                        if pCell >= m.aDataEnd() {
                            return SQLITE_CORRUPT_PAGE(m);
                        }
                    }
                }
                let nCellKey = varintValue(data, pCell);
                if nCellKey < intKey {
                    lwr = idx + 1;
                    if lwr > upr {
                        c = -1;
                        break;
                    }
                } else if nCellKey > intKey {
                    upr = idx - 1;
                    if lwr > upr {
                        c = 1;
                        break;
                    }
                } else {
                    debug_assert!(nCellKey == intKey);
                    found = Some(nCellKey);
                    break;
                }
                debug_assert!(lwr + upr >= 0);
                idx = (lwr + upr) >> 1; /* idx = (lwr+upr)/2; */
            }
            found
        };
        if let Some(nCellKey) = found {
            pCur.ix = idx as u16;
            if leaf == 0 {
                lwr = idx;
                /* goto moveto_table_next_layer; */
            } else {
                pCur.curFlags |= BTCF_ValidNKey as u8;
                pCur.info.nKey = nCellKey;
                pCur.info.nSize = 0;
                *pRes = 0;
                return SQLITE_OK;
            }
        } else {
            debug_assert!(lwr == upr + 1 || leaf == 0);
            if leaf != 0 {
                pCur.ix = idx as u16;
                *pRes = c;
                rc = SQLITE_OK;
                break; /* goto moveto_table_finish */
            }
        }
        /* moveto_table_next_layer: */
        let chldPg = {
            let g = pPage.borrow();
            if lwr >= nCell {
                get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8)
            } else {
                get4byte(&g.aData, findCell(&g.pExtra, &g.aData, lwr as usize))
            }
        };
        pCur.ix = lwr as u16;
        rc = moveToChild(pCur, chldPg);
        if rc != 0 {
            break;
        }
    }
    /* moveto_table_finish: */
    pCur.info.nSize = 0;
    debug_assert!((pCur.curFlags as i32 & BTCF_ValidOvfl) == 0);
    rc
}

/// Compare the "idx"-th cell on the page pPage against the key
/// pointing to by pIdxKey using xRecordCompare.  Return negative or
/// zero if the cell is less than or equal pIdxKey.  Return positive
/// if unknown.
///
///    Return value negative:     Cell at pCur[idx] less than pIdxKey
///
///    Return value is zero:      Cell at pCur[idx] equals pIdxKey
///
///    Return value positive:     Nothing is known about the relationship
///                               of the cell at pCur[idx] and pIdxKey.
///
/// This routine is part of an optimization.  It is always safe to return
/// a positive value as that will cause the optimization to be skipped.
fn indexCellCompare(
    pg: &PgHdr,
    idx: usize,
    pIdxKey: &mut UnpackedRecord,
    xRecordCompare: RecordCompare,
) -> i32 {
    let pPage = &pg.pExtra;
    let data = &pg.aData;
    let c;
    let pCell = findCellPastPtr(pPage, data, idx);
    let mut nCell = at(data, pCell) as usize;
    if nCell <= pPage.max1bytePayload as usize {
        /* This branch runs if the record-size field of the cell is a
         ** single byte varint and the record fits entirely on the main
         ** b-tree page.  */
        if pCell + nCell >= pPage.aDataEnd() {
            return 99;
        }
        c = xRecordCompare(&data[pCell + 1..pCell + 1 + nCell], pIdxKey);
    } else if (at(data, pCell + 1) & 0x80) == 0 && {
        nCell = ((nCell & 0x7f) << 7) + at(data, pCell + 1) as usize;
        nCell <= pPage.maxLocal as usize
    } {
        /* The record-size field is a 2 byte varint and the record
         ** fits entirely on the main b-tree page.  */
        if pCell + nCell >= pPage.aDataEnd() {
            return 99;
        }
        c = xRecordCompare(&data[pCell + 2..pCell + 2 + nCell], pIdxKey);
    } else {
        /* If the record extends into overflow pages, do not attempt
         ** the optimization. */
        c = 99;
    }
    c
}

/// Return true (non-zero) if pCur is current pointing to the last
/// page of a table.
fn cursorOnLastPage(pCur: &BtCursor) -> bool {
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    for i in 0..pCur.iPage as usize {
        let nCell = mp(pCur.apPage[i].as_ref().unwrap()).nCell;
        if pCur.aiIdx[i] < nCell {
            return false;
        }
    }
    true
}

/// C: `int sqlite3BtreeIndexMoveto( BtCursor *pCur, UnpackedRecord *pIdxKey, int *pRes )`
///
/// Move the cursor so that it points to an entry in an index table
/// near the key pIdxKey.   Return a success code.
///
/// If an exact match is not found, then the cursor is always
/// left pointing at a leaf page which would hold the entry if it
/// were present.  The cursor might point to an entry that comes
/// before or after the key.
///
/// An integer is written into *pRes which is the result of
/// comparing the key with the entry to which the cursor is
/// pointing.  The meaning of the integer written into
/// *pRes is as follows:
///
///     *pRes<0      The cursor is left pointing at an entry that
///                  is smaller than pIdxKey or if the table is empty
///                  and the cursor is therefore left point to nothing.
///
///     *pRes==0     The cursor is left pointing at an entry that
///                  exactly matches pIdxKey.
///
///     *pRes>0      The cursor is left pointing at an entry that
///                  is larger than pIdxKey.
///
/// The pIdxKey->eqSeen field is set to 1 if there
/// exists an entry in the table that exactly matches pIdxKey.
pub fn sqlite3BtreeIndexMoveto(
    db: &mut Sqlite3,
    pCur: &BtCursorP,
    pIdxKey: &mut UnpackedRecord,
    pRes: &mut i32,
) -> i32 {
    btreeIndexMoveto(&mut pCur.borrow_mut(), pIdxKey, pRes)
}
pub fn btreeIndexMoveto(pCur: &mut BtCursor, pIdxKey: &mut UnpackedRecord, pRes: &mut i32) -> i32 {
    let mut rc;

    debug_assert!(pCur.pKeyInfo.is_some());
    if let Some(b) = &pCur.pBtree {
        b.borrow_mut().nSeek += 1; /* Performance measurement during testing */
    }

    let xRecordCompare = crate::vdbeaux::sqlite3VdbeFindCompare(pIdxKey);
    pIdxKey.errCode = 0;
    debug_assert!(pIdxKey.default_rc == 1 || pIdxKey.default_rc == 0 || pIdxKey.default_rc == -1);

    /* Check to see if we can skip a lot of work.  Two cases:
     **
     **    (1) If the cursor is already pointing to the very last cell
     **        in the table and the pIdxKey search key is greater than or
     **        equal to that last cell, then no movement is required.
     **
     **    (2) If the cursor is on the last page of the table and the first
     **        cell on that last page is less than or equal to the pIdxKey
     **        search key, then we can start the search on the current page
     **        without needing to go back to root.
     */
    let mut bypass = false;
    if pCur.eState == CURSOR_VALID as u8 && mp(pCur.page()).leaf != 0 && cursorOnLastPage(pCur) {
        let pg = pCur.page().clone();
        let nCell = mp(&pg).nCell;
        if pCur.ix == nCell.wrapping_sub(1) {
            let c = indexCellCompare(&pg.borrow(), pCur.ix as usize, pIdxKey, xRecordCompare);
            if c <= 0 && pIdxKey.errCode as i32 == SQLITE_OK {
                *pRes = c;
                return SQLITE_OK; /* Cursor already pointing at the correct spot */
            }
        }
        if !bypass
            && pCur.iPage > 0
            && indexCellCompare(&pg.borrow(), 0, pIdxKey, xRecordCompare) <= 0
            && pIdxKey.errCode as i32 == SQLITE_OK
        {
            pCur.curFlags &= !((BTCF_ValidOvfl | BTCF_AtLast) as u8);
            if mp(&pg).isInit == 0 {
                return SQLITE_CORRUPT_BKPT();
            }
            bypass = true; /* Start search on the current page */
        }
        if !bypass {
            pIdxKey.errCode = SQLITE_OK as u8;
        }
    }

    if !bypass {
        rc = moveToRoot(pCur);
        if rc != 0 {
            if rc == SQLITE_EMPTY {
                *pRes = -1;
                return SQLITE_OK;
            }
            return rc;
        }
    }

    /* bypass_moveto_root: */
    debug_assert!(pCur.eState == CURSOR_VALID as u8);
    debug_assert!(pCur.curIntKey == 0);
    'moveto_index_finish: loop {
        let pPage = pCur.page().clone();
        let mut lwr: i32 = 0;
        let mut upr: i32 = mp(&pPage).nCell as i32 - 1;
        let mut idx: i32 = upr >> 1; /* idx = (lwr+upr)/2; */
        let mut c: i32;
        let (leaf, nCellPg) = {
            let m = mp(&pPage);
            debug_assert!(m.nCell > 0);
            debug_assert!(m.intKey == 0);
            (m.leaf, m.nCell as i32)
        };
        loop {
            /* The maximum supported page-size is 65536 bytes. This means that
             ** the maximum number of record bytes stored on an index B-Tree
             ** page is less than 16384 bytes and may be stored as a 2-byte
             ** varint. This information is used to attempt to avoid parsing
             ** the entire cell by checking for the cases where the record is
             ** stored entirely within the b-tree page by inspecting the first
             ** 2 bytes of the cell.
             */
            enum Step {
                Cmp(i32),
                Corrupt,
                Overflow(usize),
            }
            let step = {
                let g = pPage.borrow();
                let m = &g.pExtra;
                let data = &g.aData;
                let pCell = findCellPastPtr(m, data, idx as usize);
                let mut nCell = at(data, pCell) as usize;
                if nCell <= m.max1bytePayload as usize {
                    /* This branch runs if the record-size field of the cell is a
                     ** single byte varint and the record fits entirely on the main
                     ** b-tree page.  */
                    if pCell + nCell >= m.aDataEnd() {
                        Step::Corrupt
                    } else {
                        Step::Cmp(xRecordCompare(&data[pCell + 1..pCell + 1 + nCell], pIdxKey))
                    }
                } else if (at(data, pCell + 1) & 0x80) == 0
                    && {
                        nCell = ((nCell & 0x7f) << 7) + at(data, pCell + 1) as usize;
                        nCell <= m.maxLocal as usize
                    }
                    && pCell + nCell < m.aDataEnd()
                {
                    /* The record-size field is a 2 byte varint and the record
                     ** fits entirely on the main b-tree page.  */
                    Step::Cmp(xRecordCompare(&data[pCell + 2..pCell + 2 + nCell], pIdxKey))
                } else {
                    Step::Overflow(pCell - m.childPtrSize as usize)
                }
            };
            match step {
                Step::Corrupt => {
                    rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
                    break 'moveto_index_finish;
                }
                Step::Cmp(x) => c = x,
                Step::Overflow(pCellBody) => {
                    /* The record flows over onto one or more overflow pages. In
                     ** this case the whole cell needs to be parsed, a buffer allocated
                     ** and accessPayload() used to retrieve the record into the
                     ** buffer before VdbeRecordCompare() can be called.
                     **
                     ** If the record is corrupt, the xRecordCompare routine may read
                     ** up to two varints past the end of the buffer. An extra 18
                     ** bytes of padding is allocated at the end of the buffer in
                     ** case this happens.  */
                    let nOverrun = 18usize; /* Size of the overrun padding */
                    {
                        let g = pPage.borrow();
                        (g.pExtra.xParseCell)(&g.pExtra, &g.aData, pCellBody, &mut pCur.info);
                    }
                    let nCell = pCur.info.nKey as i32;
                    let (usableSize, nPageDb) = {
                        let b = pCur.bt().borrow();
                        (b.usableSize, b.nPage)
                    };
                    if nCell < 2 || (nCell as u32) / usableSize > nPageDb {
                        rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
                        break 'moveto_index_finish;
                    }
                    let mut pCellKey = vec![0u8; nCell as usize + nOverrun];
                    pCur.ix = idx as u16;
                    rc = accessPayload(pCur, 0, nCell as u32, &mut pCellKey, 0);
                    for b in &mut pCellKey[nCell as usize..] {
                        *b = 0; /* Fix uninit warnings */
                    }
                    pCur.curFlags &= !(BTCF_ValidOvfl as u8);
                    if rc != 0 {
                        break 'moveto_index_finish;
                    }
                    c = crate::vdbeaux::sqlite3VdbeRecordCompare(nCell, &pCellKey, pIdxKey);
                }
            }
            if c < 0 {
                lwr = idx + 1;
            } else if c > 0 {
                upr = idx - 1;
            } else {
                debug_assert!(c == 0);
                *pRes = 0;
                rc = SQLITE_OK;
                pCur.ix = idx as u16;
                if pIdxKey.errCode != 0 {
                    rc = SQLITE_CORRUPT_BKPT();
                }
                break 'moveto_index_finish;
            }
            if lwr > upr {
                break;
            }
            debug_assert!(lwr + upr >= 0);
            idx = (lwr + upr) >> 1; /* idx = (lwr+upr)/2 */
        }
        debug_assert!(lwr == upr + 1 || leaf == 0);
        if leaf != 0 {
            pCur.ix = idx as u16;
            *pRes = c;
            rc = SQLITE_OK;
            break 'moveto_index_finish;
        }
        let chldPg = {
            let g = pPage.borrow();
            if lwr >= nCellPg {
                get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8)
            } else {
                get4byte(&g.aData, findCell(&g.pExtra, &g.aData, lwr as usize))
            }
        };

        /* This block is similar to an in-lined version of:
         **
         **    pCur->ix = (u16)lwr;
         **    rc = moveToChild(pCur, chldPg);
         **    if( rc ) break;
         */
        pCur.info.nSize = 0;
        pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
        if pCur.iPage >= (BTCURSOR_MAX_DEPTH - 1) as i8 {
            return SQLITE_CORRUPT_BKPT();
        }
        let ip = pCur.iPage as usize;
        pCur.aiIdx[ip] = lwr as u16;
        pCur.apPage[ip] = pCur.pPage.take();
        pCur.ix = 0;
        pCur.iPage += 1;
        let pBt = pCur.bt().clone();
        let flags = pCur.curPagerFlags as i32;
        rc = getAndInitPage(&pBt, chldPg, &mut pCur.pPage, flags);
        if rc == SQLITE_OK {
            let bad = {
                let m = mp(pCur.page());
                m.nCell < 1 || m.intKey != pCur.curIntKey
            };
            if bad {
                let pg = pCur.pPage.take();
                releasePage(pg.as_ref());
                rc = SQLITE_CORRUPT_PGNO(chldPg);
            }
        }
        if rc != 0 {
            pCur.iPage -= 1;
            pCur.pPage = pCur.apPage[pCur.iPage as usize].take();
            break;
        }
        /*
         ***** End of in-lined moveToChild() call */
    }
    /* moveto_index_finish: */
    pCur.info.nSize = 0;
    debug_assert!((pCur.curFlags as i32 & BTCF_ValidOvfl) == 0);
    rc
}

/// C: `int sqlite3BtreeEof(BtCursor *pCur)`
///
/// Return TRUE if the cursor is not pointing at an entry of the table.
///
/// TRUE will be returned after a call to sqlite3BtreeNext() moves
/// past the last entry in the table or sqlite3BtreePrev() moves past
/// the first entry.  TRUE is also returned if the table is empty.
pub fn sqlite3BtreeEof(pCur: &BtCursorP) -> i32 {
    /* TODO: What if the cursor is in CURSOR_REQUIRESEEK but all table entries
     ** have been deleted? This API will need to change to return an error code
     ** as well as the boolean result value.
     */
    (CURSOR_VALID as u8 != pCur.borrow().eState) as i32
}

/// C: `i64 sqlite3BtreeRowCountEst(BtCursor *pCur)`
///
/// Return an estimate for the number of rows in the table that pCur is
/// pointing to.  Return a negative number if no estimate is currently
/// available.
pub fn sqlite3BtreeRowCountEst(db: &mut Sqlite3, pCur: &BtCursorP) -> i64 {
    let c = pCur.borrow();
    /* Currently this interface is only called by the OP_IfSizeBetween
     ** opcode and the OP_Count opcode with P3=1.  In either case,
     ** the cursor will always be valid unless the btree is empty. */
    if c.eState != CURSOR_VALID as u8 {
        return 0;
    }
    if mp(c.page()).leaf == 0 {
        return -1;
    }

    let mut n = mp(c.page()).nCell as i64;
    for i in 0..c.iPage as usize {
        n *= mp(c.apPage[i].as_ref().unwrap()).nCell as i64 + 1;
    }
    n
}

/// Advance the cursor to the next entry in the database.
/// Return value:
///
///    SQLITE_OK        success
///    SQLITE_DONE      cursor is already pointing at the last element
///    otherwise        some kind of error occurred
///
/// The main entry point is sqlite3BtreeNext().  That routine is optimized
/// for the common case of merely incrementing the cell counter BtCursor.aiIdx
/// to the next cell on the current page.  The (slower) btreeNext() helper
/// routine is called when it is necessary to move to a different page or
/// to restore the cursor.
///
/// If bit 0x01 of the F argument in sqlite3BtreeNext(C,F) is 1, then the
/// cursor corresponds to an SQL index and this routine could have been
/// skipped if the SQL index had been a unique index.  The F argument
/// is a hint to the implement.  SQLite btree implementation does not use
/// this hint, but COMDB2 does.
fn btreeNext(pCur: &mut BtCursor) -> i32 {
    if pCur.eState != CURSOR_VALID as u8 {
        debug_assert!((pCur.curFlags as i32 & BTCF_ValidOvfl) == 0);
        let rc = restoreCursorPosition(pCur);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID as u8 == pCur.eState {
            return SQLITE_DONE;
        }
        if pCur.eState == CURSOR_SKIPNEXT as u8 {
            pCur.eState = CURSOR_VALID as u8;
            if pCur.skipNext > 0 {
                return SQLITE_OK;
            }
        }
    }

    let mut pPage = pCur.page().clone();
    pCur.ix = pCur.ix.wrapping_add(1);
    let idx = pCur.ix;
    if mp(&pPage).isInit == 0 {
        return SQLITE_CORRUPT_BKPT();
    }

    if idx >= mp(&pPage).nCell {
        if mp(&pPage).leaf == 0 {
            let pgno = {
                let g = pPage.borrow();
                get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8)
            };
            let rc = moveToChild(pCur, pgno);
            if rc != 0 {
                return rc;
            }
            return moveToLeftmost(pCur);
        }
        loop {
            if pCur.iPage == 0 {
                pCur.eState = CURSOR_INVALID as u8;
                return SQLITE_DONE;
            }
            moveToParent(pCur);
            pPage = pCur.page().clone();
            if pCur.ix < mp(&pPage).nCell {
                break;
            }
        }
        if mp(&pPage).intKey != 0 {
            return cursorNext(pCur, 0);
        } else {
            return SQLITE_OK;
        }
    }
    if mp(&pPage).leaf != 0 {
        SQLITE_OK
    } else {
        moveToLeftmost(pCur)
    }
}

/// C: `int sqlite3BtreeNext(BtCursor *pCur, int flags)`
pub fn sqlite3BtreeNext(pCur: &BtCursorP, flags: i32) -> i32 {
    cursorNext(&mut pCur.borrow_mut(), flags)
}
fn cursorNext(pCur: &mut BtCursor, flags: i32) -> i32 {
    debug_assert!(flags == 0 || flags == 1);
    pCur.info.nSize = 0;
    pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
    if pCur.eState != CURSOR_VALID as u8 {
        return btreeNext(pCur);
    }
    let (nCell, leaf) = {
        let m = mp(pCur.page());
        (m.nCell, m.leaf)
    };
    pCur.ix = pCur.ix.wrapping_add(1);
    if pCur.ix >= nCell {
        pCur.ix -= 1;
        return btreeNext(pCur);
    }
    if leaf != 0 {
        SQLITE_OK
    } else {
        moveToLeftmost(pCur)
    }
}

/// Step the cursor to the back to the previous entry in the database.
/// Return values:
///
///     SQLITE_OK     success
///     SQLITE_DONE   the cursor is already on the first element of the table
///     otherwise     some kind of error occurred
///
/// The main entry point is sqlite3BtreePrevious().  That routine is optimized
/// for the common case of merely decrementing the cell counter BtCursor.aiIdx
/// to the previous cell on the current page.  The (slower) btreePrevious()
/// helper routine is called when it is necessary to move to a different page
/// or to restore the cursor.
fn btreePrevious(pCur: &mut BtCursor) -> i32 {
    let mut rc;
    debug_assert!((pCur.curFlags as i32 & (BTCF_AtLast | BTCF_ValidOvfl | BTCF_ValidNKey)) == 0);
    debug_assert!(pCur.info.nSize == 0);
    if pCur.eState != CURSOR_VALID as u8 {
        rc = restoreCursorPosition(pCur);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID as u8 == pCur.eState {
            return SQLITE_DONE;
        }
        if CURSOR_SKIPNEXT as u8 == pCur.eState {
            pCur.eState = CURSOR_VALID as u8;
            if pCur.skipNext < 0 {
                return SQLITE_OK;
            }
        }
    }

    let pPage = pCur.page().clone();
    if mp(&pPage).isInit == 0 {
        return SQLITE_CORRUPT_BKPT();
    }
    if mp(&pPage).leaf == 0 {
        let idx = pCur.ix as usize;
        let pgno = {
            let g = pPage.borrow();
            get4byte(&g.aData, findCell(&g.pExtra, &g.aData, idx))
        };
        rc = moveToChild(pCur, pgno);
        if rc != 0 {
            return rc;
        }
        rc = moveToRightmost(pCur);
    } else {
        while pCur.ix == 0 {
            if pCur.iPage == 0 {
                pCur.eState = CURSOR_INVALID as u8;
                return SQLITE_DONE;
            }
            moveToParent(pCur);
        }
        debug_assert!(pCur.info.nSize == 0);
        debug_assert!((pCur.curFlags as i32 & BTCF_ValidOvfl) == 0);

        pCur.ix -= 1;
        let (intKey, leaf) = {
            let m = mp(pCur.page());
            (m.intKey, m.leaf)
        };
        if intKey != 0 && leaf == 0 {
            rc = cursorPrevious(pCur, 0);
        } else {
            rc = SQLITE_OK;
        }
    }
    rc
}

/// C: `int sqlite3BtreePrevious(BtCursor *pCur, int flags)`
pub fn sqlite3BtreePrevious(pCur: &BtCursorP, flags: i32) -> i32 {
    cursorPrevious(&mut pCur.borrow_mut(), flags)
}
fn cursorPrevious(pCur: &mut BtCursor, flags: i32) -> i32 {
    debug_assert!(flags == 0 || flags == 1);
    pCur.curFlags &= !((BTCF_AtLast | BTCF_ValidOvfl | BTCF_ValidNKey) as u8);
    pCur.info.nSize = 0;
    if pCur.eState != CURSOR_VALID as u8 || pCur.ix == 0 || mp(pCur.page()).leaf == 0 {
        return btreePrevious(pCur);
    }
    pCur.ix -= 1;
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Page allocation
// ---------------------------------------------------------------------------

/// Allocate a new page from the database file.
///
/// The new page is marked as dirty.  (In other words, sqlite3PagerWrite()
/// has already been called on the new page.)  The new page has also
/// been referenced and the calling routine is responsible for calling
/// sqlite3PagerUnref() on the new page when it is done.
///
/// SQLITE_OK is returned on success.  Any other return value indicates
/// an error.  *ppPage is set to NULL in the event of an error.
///
/// If the "nearby" parameter is not 0, then an effort is made to
/// locate a page close to the page number "nearby".  This can be used in an
/// attempt to keep related pages close to each other in the database file,
/// which in turn can make database access faster.
///
/// If the eMode parameter is BTALLOC_EXACT and the nearby page exists
/// anywhere on the free-list, then it is guaranteed to be returned.  If
/// eMode is BTALLOC_LT then the page returned will be less than or equal
/// to nearby if any such page exists.  If eMode is BTALLOC_ANY then there
/// are no restrictions on which page is returned.
fn allocateBtreePage(
    pBt: &BtSharedP,
    ppPage: &mut Option<DbPage>,
    pPgno: &mut Pgno,
    nearby: Pgno,
    eMode: u8,
) -> i32 {
    let mut rc;
    let mut pTrunk: Option<DbPage> = None;
    let mut pPrevTrunk: Option<DbPage> = None;

    debug_assert!(eMode == BTALLOC_ANY || (nearby > 0 && pBt.borrow().autoVacuum != 0));
    let pPage1 = page1Of(pBt);
    let mxPage = btreePagecount(pBt); /* Total size of the database file */
    /* EVIDENCE-OF: R-21003-45125 The 4-byte big-endian integer at offset 36
     ** stores the total number of pages on the freelist. */
    let n = get4byte(&pPage1.borrow().aData, 36); /* Number of pages on the freelist */
    if n >= mxPage {
        return SQLITE_CORRUPT_BKPT();
    }
    let usableSize = pBt.borrow().usableSize;
    if n > 0 {
        /* There are pages on the freelist.  Reuse one of those pages. */
        let mut iTrunk: Pgno;
        let mut searchList: u8 = 0; /* If the free-list must be searched for 'nearby' */
        let mut nSearch: u32 = 0; /* Count of the number of search attempts */

        /* If eMode==BTALLOC_EXACT and a query of the pointer-map
         ** shows that the page 'nearby' is somewhere on the free-list, then
         ** the entire-list will be searched for that page.
         */
        if eMode == BTALLOC_EXACT {
            if nearby <= mxPage {
                let mut eType = 0u8;
                debug_assert!(nearby > 0);
                debug_assert!(pBt.borrow().autoVacuum != 0);
                rc = ptrmapGet(pBt, nearby, &mut eType, None);
                if rc != 0 {
                    return rc;
                }
                if eType == PTRMAP_FREEPAGE as u8 {
                    searchList = 1;
                }
            }
        } else if eMode == BTALLOC_LE {
            searchList = 1;
        }

        /* Decrement the free-list count by 1. Set iTrunk to the index of the
         ** first free-list trunk page. iPrevTrunk is initially 1.
         */
        rc = sqlite3PagerWrite(&pPage1);
        if rc != 0 {
            return rc;
        }
        put4byte(&mut pPage1.borrow_mut().aData, 36, n - 1);

        /* The code within this loop is run only once if the 'searchList' variable
         ** is not true. Otherwise, it runs once for each trunk-page on the
         ** free-list until the page 'nearby' is located (eMode==BTALLOC_EXACT)
         ** or until a page less than 'nearby' is located (eMode==BTALLOC_LT)
         */
        'end_allocate_page: loop {
            pPrevTrunk = pTrunk.take();
            if let Some(pt) = &pPrevTrunk {
                /* EVIDENCE-OF: R-01506-11053 The first integer on a freelist trunk page
                 ** is the page number of the next freelist trunk page in the list or
                 ** zero if this is the last freelist trunk page. */
                iTrunk = get4byte(&pt.borrow().aData, 0);
            } else {
                /* EVIDENCE-OF: R-59841-13798 The 4-byte big-endian integer at offset 32
                 ** stores the page number of the first page of the freelist, or zero if
                 ** the freelist is empty. */
                iTrunk = get4byte(&pPage1.borrow().aData, 32);
            }
            let ns = nSearch;
            nSearch += 1;
            if iTrunk > mxPage || ns > n {
                rc = SQLITE_CORRUPT_PGNO(match &pPrevTrunk {
                    Some(p) => pgnoOf(p),
                    None => 1,
                });
            } else {
                rc = btreeGetUnusedPage(pBt, iTrunk, &mut pTrunk, 0);
            }
            if rc != 0 {
                pTrunk = None;
                break 'end_allocate_page;
            }
            let trunk = pTrunk.clone().unwrap();
            /* EVIDENCE-OF: R-13523-04394 The second integer on a freelist trunk page
             ** is the number of leaf page pointers to follow. */
            let k = get4byte(&trunk.borrow().aData, 4);
            if k == 0 && searchList == 0 {
                /* The trunk has no leaves and the list is not being searched.
                 ** So extract the trunk page itself and use it as the newly
                 ** allocated page */
                debug_assert!(pPrevTrunk.is_none());
                rc = sqlite3PagerWrite(&trunk);
                if rc != 0 {
                    break 'end_allocate_page;
                }
                *pPgno = iTrunk;
                let first4: [u8; 4] = trunk.borrow().aData[0..4].try_into().unwrap();
                pPage1.borrow_mut().aData[32..36].copy_from_slice(&first4);
                *ppPage = pTrunk.take();
            } else if k > usableSize / 4 - 2 {
                /* Value of k is out of range.  Database corruption */
                rc = SQLITE_CORRUPT_PGNO(iTrunk);
                break 'end_allocate_page;
            } else if searchList != 0
                && (nearby == iTrunk || (iTrunk < nearby && eMode == BTALLOC_LE))
            {
                /* The list is being searched and this trunk page is the page
                 ** to allocate, regardless of whether it has leaves.
                 */
                *pPgno = iTrunk;
                *ppPage = pTrunk.clone();
                searchList = 0;
                rc = sqlite3PagerWrite(&trunk);
                if rc != 0 {
                    break 'end_allocate_page;
                }
                if k == 0 {
                    let first4: [u8; 4] = trunk.borrow().aData[0..4].try_into().unwrap();
                    match &pPrevTrunk {
                        None => {
                            pPage1.borrow_mut().aData[32..36].copy_from_slice(&first4);
                        }
                        Some(prev) => {
                            rc = sqlite3PagerWrite(prev);
                            if rc != SQLITE_OK {
                                break 'end_allocate_page;
                            }
                            prev.borrow_mut().aData[0..4].copy_from_slice(&first4);
                        }
                    }
                } else {
                    /* The trunk page is required by the caller but it contains
                     ** pointers to free-list leaves. The first leaf becomes a trunk
                     ** page in this case.
                     */
                    let iNewTrunk = get4byte(&trunk.borrow().aData, 8);
                    if iNewTrunk > mxPage {
                        rc = SQLITE_CORRUPT_PGNO(iTrunk);
                        break 'end_allocate_page;
                    }
                    let mut pNewTrunk = None;
                    rc = btreeGetUnusedPage(pBt, iNewTrunk, &mut pNewTrunk, 0);
                    if rc != SQLITE_OK {
                        break 'end_allocate_page;
                    }
                    let pNewTrunk = pNewTrunk.unwrap();
                    rc = sqlite3PagerWrite(&pNewTrunk);
                    if rc != SQLITE_OK {
                        releasePage(Some(&pNewTrunk));
                        break 'end_allocate_page;
                    }
                    {
                        let t = trunk.borrow();
                        let mut nt = pNewTrunk.borrow_mut();
                        nt.aData[0..4].copy_from_slice(&t.aData[0..4]);
                        put4byte(&mut nt.aData, 4, k - 1);
                        let len = ((k - 1) * 4) as usize;
                        nt.aData[8..8 + len].copy_from_slice(&t.aData[12..12 + len]);
                    }
                    releasePage(Some(&pNewTrunk));
                    match &pPrevTrunk {
                        None => {
                            put4byte(&mut pPage1.borrow_mut().aData, 32, iNewTrunk);
                        }
                        Some(prev) => {
                            rc = sqlite3PagerWrite(prev);
                            if rc != 0 {
                                break 'end_allocate_page;
                            }
                            put4byte(&mut prev.borrow_mut().aData, 0, iNewTrunk);
                        }
                    }
                }
                pTrunk = None;
            } else if k > 0 {
                /* Extract a leaf from the trunk */
                let mut closest: u32;
                let mut iPage: Pgno;
                if nearby > 0 {
                    closest = 0;
                    let t = trunk.borrow();
                    let aData = &t.aData;
                    if eMode == BTALLOC_LE {
                        for i in 0..k {
                            iPage = get4byte(aData, (8 + i * 4) as usize);
                            if iPage <= nearby {
                                closest = i;
                                break;
                            }
                        }
                    } else {
                        let mut dist =
                            sqlite3AbsInt32(get4byte(aData, 8).wrapping_sub(nearby) as i32);
                        for i in 1..k {
                            let d2 = sqlite3AbsInt32(
                                get4byte(aData, (8 + i * 4) as usize).wrapping_sub(nearby) as i32,
                            );
                            if d2 < dist {
                                closest = i;
                                dist = d2;
                            }
                        }
                    }
                } else {
                    closest = 0;
                }

                iPage = get4byte(&trunk.borrow().aData, (8 + closest * 4) as usize);
                if iPage > mxPage || iPage < 2 {
                    rc = SQLITE_CORRUPT_PGNO(iTrunk);
                    break 'end_allocate_page;
                }
                if searchList == 0 || (iPage == nearby || (iPage < nearby && eMode == BTALLOC_LE)) {
                    *pPgno = iPage;
                    rc = sqlite3PagerWrite(&trunk);
                    if rc != 0 {
                        break 'end_allocate_page;
                    }
                    {
                        let mut t = trunk.borrow_mut();
                        if closest < k - 1 {
                            let src = (4 + k * 4) as usize;
                            let dst = (8 + closest * 4) as usize;
                            t.aData.copy_within(src..src + 4, dst);
                        }
                        put4byte(&mut t.aData, 4, k - 1);
                    }
                    let noContent = if !btreeGetHasContent(pBt, *pPgno) {
                        PAGER_GET_NOCONTENT
                    } else {
                        0
                    };
                    rc = btreeGetUnusedPage(pBt, *pPgno, ppPage, noContent);
                    if rc == SQLITE_OK {
                        let pg = ppPage.clone().unwrap();
                        rc = sqlite3PagerWrite(&pg);
                        if rc != SQLITE_OK {
                            releasePage(Some(&pg));
                            *ppPage = None;
                        }
                    }
                    searchList = 0;
                }
            }
            releasePage(pPrevTrunk.as_ref());
            pPrevTrunk = None;
            if searchList == 0 {
                break;
            }
        }
    } else {
        /* There are no pages on the freelist, so append a new page to the
         ** database image.
         **
         ** Normally, new pages allocated by this block can be requested from the
         ** pager layer with the 'no-content' flag set. This prevents the pager
         ** from trying to read the pages content from disk. However, if the
         ** current transaction has already run one or more incremental-vacuum
         ** steps, then the page we are about to allocate may contain content
         ** that is required in the event of a rollback. In this case, do
         ** not set the no-content flag. This causes the pager to load and journal
         ** the current page content before overwriting it.
         **
         ** Note that the pager will not actually attempt to load or journal
         ** content for any page that really does lie past the end of the database
         ** file on disk. So the effects of disabling the no-content optimization
         ** here are confined to those pages that lie between the end of the
         ** database image and the end of the database file.
         */
        let bNoContent = if 0 == pBt.borrow().bDoTruncate {
            PAGER_GET_NOCONTENT
        } else {
            0
        };

        rc = sqlite3PagerWrite(&pPage1);
        if rc != 0 {
            return rc;
        }
        let pbp = pendingBytePage(pBt);
        {
            let mut bt = pBt.borrow_mut();
            bt.nPage += 1;
            if bt.nPage == pbp {
                bt.nPage += 1;
            }
        }

        let nPage = pBt.borrow().nPage;
        if pBt.borrow().autoVacuum != 0 && PTRMAP_ISPAGE(pBt, nPage) {
            /* If *pPgno refers to a pointer-map page, allocate two new pages
             ** at the end of the file instead of one. The first allocated page
             ** becomes a new pointer-map page, the second is used by the caller.
             */
            let mut pPg = None;
            debug_assert!(nPage != pbp);
            rc = btreeGetUnusedPage(pBt, nPage, &mut pPg, bNoContent);
            if rc == SQLITE_OK {
                let pg = pPg.unwrap();
                rc = sqlite3PagerWrite(&pg);
                releasePage(Some(&pg));
            }
            if rc != 0 {
                return rc;
            }
            let mut bt = pBt.borrow_mut();
            bt.nPage += 1;
            if bt.nPage == pbp {
                bt.nPage += 1;
            }
        }
        let nPage = pBt.borrow().nPage;
        put4byte(&mut pPage1.borrow_mut().aData, 28, nPage);
        *pPgno = nPage;

        debug_assert!(*pPgno != pbp);
        rc = btreeGetUnusedPage(pBt, *pPgno, ppPage, bNoContent);
        if rc != 0 {
            return rc;
        }
        let pg = ppPage.clone().unwrap();
        rc = sqlite3PagerWrite(&pg);
        if rc != SQLITE_OK {
            releasePage(Some(&pg));
            *ppPage = None;
        }
    }

    /* end_allocate_page: */
    releasePage(pTrunk.as_ref());
    releasePage(pPrevTrunk.as_ref());
    rc
}

/// This function is used to add page iPage to the database file free-list.
/// It is assumed that the page is not already a part of the free-list.
///
/// The value passed as the second argument to this function is optional.
/// If the caller happens to have a pointer to the MemPage object
/// corresponding to page iPage handy, it may pass it as the second value.
/// Otherwise, it may pass NULL.
///
/// If a pointer to a MemPage object is passed as the second argument,
/// its reference count is not altered by this function.
fn freePage2(pBt: &BtSharedP, pMemPage: Option<&DbPage>, iPage: Pgno) -> i32 {
    let mut pTrunk: Option<DbPage> = None; /* Free-list trunk page */
    let mut iTrunk: Pgno = 0; /* Page number of free-list trunk page */
    let pPage1 = page1Of(pBt); /* Local reference to page 1 */
    let mut pPage: Option<DbPage>; /* Page being freed. May be NULL. */
    let mut rc;

    debug_assert!(pMemPage.map_or(true, |p| pgnoOf(p) == iPage));

    if iPage < 2 || iPage > pBt.borrow().nPage {
        return SQLITE_CORRUPT_BKPT();
    }
    if let Some(m) = pMemPage {
        pPage = Some(m.clone());
        sqlite3PagerRef(m);
    } else {
        pPage = btreePageLookup(pBt, iPage);
    }

    let (btsFlags, autoVacuum, usableSize, pageSize) = {
        let b = pBt.borrow();
        (b.btsFlags as i32, b.autoVacuum, b.usableSize, b.pageSize)
    };

    'freepage_out: {
        /* Increment the free page count on pPage1 */
        rc = sqlite3PagerWrite(&pPage1);
        if rc != 0 {
            break 'freepage_out;
        }
        let nFree = get4byte(&pPage1.borrow().aData, 36); /* Initial number of pages on free-list */
        put4byte(&mut pPage1.borrow_mut().aData, 36, nFree + 1);

        if btsFlags & BTS_SECURE_DELETE != 0 {
            /* If the secure_delete option is enabled, then
             ** always fully overwrite deleted information with zeros.
             */
            if pPage.is_none() {
                rc = btreeGetPage(pBt, iPage, &mut pPage, 0);
                if rc != 0 {
                    break 'freepage_out;
                }
            }
            rc = sqlite3PagerWrite(pPage.as_ref().unwrap());
            if rc != 0 {
                break 'freepage_out;
            }
            let pg = pPage.as_ref().unwrap();
            for b in &mut pg.borrow_mut().aData[..pageSize as usize] {
                *b = 0;
            }
        }

        /* If the database supports auto-vacuum, write an entry in the pointer-map
         ** to indicate that the page is free.
         */
        if autoVacuum != 0 {
            ptrmapPut(pBt, iPage, PTRMAP_FREEPAGE as u8, 0, &mut rc);
            if rc != 0 {
                break 'freepage_out;
            }
        }

        /* Now manipulate the actual database free-list structure. There are two
         ** possibilities. If the free-list is currently empty, or if the first
         ** trunk page in the free-list is full, then this page will become a
         ** new free-list trunk page. Otherwise, it will become a leaf of the
         ** first trunk page in the current free-list. This block tests if it
         ** is possible to add the page as a new free-list leaf.
         */
        if nFree != 0 {
            iTrunk = get4byte(&pPage1.borrow().aData, 32);
            if iTrunk > btreePagecount(pBt) {
                rc = SQLITE_CORRUPT_BKPT();
                break 'freepage_out;
            }
            rc = btreeGetPage(pBt, iTrunk, &mut pTrunk, 0);
            if rc != SQLITE_OK {
                break 'freepage_out;
            }
            let trunk = pTrunk.clone().unwrap();

            let nLeaf = get4byte(&trunk.borrow().aData, 4); /* Initial number of leaf cells on trunk page */
            debug_assert!(usableSize > 32);
            if nLeaf > usableSize / 4 - 2 {
                rc = SQLITE_CORRUPT_BKPT();
                break 'freepage_out;
            }
            if nLeaf < usableSize / 4 - 8 {
                /* In this case there is room on the trunk page to insert the page
                 ** being freed as a new leaf.
                 **
                 ** Note that the trunk page is not really full until it contains
                 ** usableSize/4 - 2 entries, not usableSize/4 - 8 entries as we have
                 ** coded.  But due to a coding error in versions of SQLite prior to
                 ** 3.6.0, databases with freelist trunk pages holding more than
                 ** usableSize/4 - 8 entries will be reported as corrupt.  In order
                 ** to maintain backwards compatibility with older versions of SQLite,
                 ** we will continue to restrict the number of entries to usableSize/4 - 8
                 ** for now.  At some point in the future (once everyone has upgraded
                 ** to 3.6.0 or later) we should consider fixing the conditional above
                 ** to read "usableSize/4-2" instead of "usableSize/4-8".
                 **
                 ** EVIDENCE-OF: R-19920-11576 However, newer versions of SQLite still
                 ** avoid using the last six entries in the freelist trunk page array in
                 ** order that database files created by newer versions of SQLite can be
                 ** read by older versions of SQLite.
                 */
                rc = sqlite3PagerWrite(&trunk);
                if rc == SQLITE_OK {
                    {
                        let mut t = trunk.borrow_mut();
                        put4byte(&mut t.aData, 4, nLeaf + 1);
                        put4byte(&mut t.aData, (8 + nLeaf * 4) as usize, iPage);
                    }
                    if let Some(pg) = &pPage {
                        if (btsFlags & BTS_SECURE_DELETE) == 0 {
                            sqlite3PagerDontWrite(pg);
                        }
                    }
                    rc = btreeSetHasContent(pBt, iPage);
                }
                break 'freepage_out;
            }
        }

        /* If control flows to this point, then it was not possible to add the
         ** the page being freed as a leaf page of the first trunk in the free-list.
         ** Possibly because the free-list is empty, or possibly because the
         ** first trunk in the free-list is full. Either way, the page being freed
         ** will become the new first trunk page in the free-list.
         */
        if pPage.is_none() {
            rc = btreeGetPage(pBt, iPage, &mut pPage, 0);
            if SQLITE_OK != rc {
                break 'freepage_out;
            }
        }
        let pg = pPage.clone().unwrap();
        rc = sqlite3PagerWrite(&pg);
        if rc != SQLITE_OK {
            break 'freepage_out;
        }
        {
            let mut g = pg.borrow_mut();
            put4byte(&mut g.aData, 0, iTrunk);
            put4byte(&mut g.aData, 4, 0);
        }
        put4byte(&mut pPage1.borrow_mut().aData, 32, iPage);
    }

    /* freepage_out: */
    if let Some(pg) = &pPage {
        pg.borrow_mut().pExtra.isInit = 0;
    }
    releasePage(pPage.as_ref());
    releasePage(pTrunk.as_ref());
    rc
}

/// freePage(pPage, pRC)
fn freePage(pPage: &DbPage, pRC: &mut i32) {
    if *pRC == SQLITE_OK {
        let (pBt, pgno) = {
            let m = mp(pPage);
            (m.pBt.upgrade().expect("page without BtShared"), m.pgno)
        };
        *pRC = freePage2(&pBt, Some(pPage), pgno);
    }
}

/// Free the overflow pages associated with the given Cell.
fn clearCellOverflow(pg: &DbPage, pCell: usize, pInfo: &CellInfo) -> i32 {
    debug_assert!(pInfo.nLocal as u32 != pInfo.nPayload);
    let (ovflPgnoStart, pBt) = {
        let g = pg.borrow();
        if pCell + pInfo.nSize as usize > g.pExtra.aDataEnd() {
            /* Cell extends past end of page */
            return SQLITE_CORRUPT_PAGE(&g.pExtra);
        }
        (
            get4byte(&g.aData, pCell + pInfo.nSize as usize - 4),
            g.pExtra.pBt.upgrade().expect("page without BtShared"),
        )
    };
    let mut ovflPgno = ovflPgnoStart;
    let usableSize = pBt.borrow().usableSize;
    debug_assert!(usableSize > 4);
    let ovflPageSize = usableSize - 4;
    let mut nOvfl = (pInfo
        .nPayload
        .wrapping_sub(pInfo.nLocal as u32)
        .wrapping_add(ovflPageSize - 1)
        / ovflPageSize) as i32;
    while nOvfl > 0 {
        nOvfl -= 1;
        let mut iNext: Pgno = 0;
        let mut pOvfl: Option<DbPage> = None;
        if ovflPgno < 2 || ovflPgno > btreePagecount(&pBt) {
            /* 0 is not a legal page number and page 1 cannot be an
             ** overflow page. Therefore if ovflPgno<2 or past the end of the
             ** file the database must be corrupt. */
            return SQLITE_CORRUPT_BKPT();
        }
        if nOvfl != 0 {
            let rc = getOverflowPage(&pBt, ovflPgno, Some(&mut pOvfl), &mut iNext);
            if rc != 0 {
                return rc;
            }
        }

        if pOvfl.is_none() {
            pOvfl = btreePageLookup(&pBt, ovflPgno);
        }
        let rc;
        if pOvfl.is_some() && sqlite3PagerPageRefcount(pOvfl.as_ref().unwrap()) != 1 {
            /* There is no reason any cursor should have an outstanding reference
             ** to an overflow page belonging to a cell that is being deleted/updated.
             ** So if there exists more than one reference to this page, then it
             ** must not really be an overflow page and the database must be corrupt.
             ** It is helpful to detect this before calling freePage2(), as
             ** freePage2() may zero the page contents if secure-delete mode is
             ** enabled. If this 'overflow' page happens to be a page that the
             ** caller is iterating through or using in some other way, this
             ** can be problematic.
             */
            rc = SQLITE_CORRUPT_BKPT();
        } else {
            rc = freePage2(&pBt, pOvfl.as_ref(), ovflPgno);
        }

        if let Some(o) = &pOvfl {
            sqlite3PagerUnref(o);
        }
        if rc != 0 {
            return rc;
        }
        ovflPgno = iNext;
    }
    SQLITE_OK
}

/// Call xParseCell to compute the size of a cell.  If the cell contains
/// overflow, then invoke cellClearOverflow to clear out that overflow.
/// Store the result code (SQLITE_OK or some error code) in rc.
///
/// Implemented as macro to force inlining for performance.
fn BTREE_CLEAR_CELL(pg: &DbPage, pCell: usize, sInfo: &mut CellInfo) -> i32 {
    {
        let g = pg.borrow();
        (g.pExtra.xParseCell)(&g.pExtra, &g.aData, pCell, sInfo);
    }
    if sInfo.nLocal as u32 != sInfo.nPayload {
        clearCellOverflow(pg, pCell, sInfo)
    } else {
        SQLITE_OK
    }
}

/// Where fillInCell() writes the next overflow page number.
enum Prior {
    /// pCell[off]
    Cell(usize),
    /// The first 4 bytes of the current overflow page (pToRelease).
    Ovfl,
}

/// Create the byte sequence used to represent a cell on page pPage
/// and write that byte sequence into pCell[].  Overflow pages are
/// allocated and filled in as necessary.  The calling procedure
/// is responsible for making sure sufficient space has been allocated
/// for pCell[].
///
/// Note that pCell does not necessary need to point to the pPage->aData
/// area.  pCell might point to some temporary storage.  The cell will
/// be constructed in this temporary area then copied into pPage->aData
/// later.
fn fillInCell(pPage: &MemPage, pCell: &mut [u8], pX: &BtreePayload, pnSize: &mut i32) -> i32 {
    let mut nPayload: i32;
    let pSrc: &[u8];
    let mut si: usize = 0; /* Offset in pSrc */
    let mut nSrc: i32;
    let mut n: i32;
    let mut nHeader: usize;

    /* Fill in the header. */
    nHeader = pPage.childPtrSize as usize;
    if pPage.intKey != 0 {
        nPayload = pX.nData + pX.nZero;
        pSrc = pX.pData.unwrap_or(&[]);
        nSrc = pX.nData;
        debug_assert!(pPage.intKeyLeaf != 0); /* fillInCell() only called for leaves */
        nHeader += putVarint32(pCell, nHeader, nPayload as u32);
        nHeader += putVarint(pCell, nHeader, pX.nKey as u64);
    } else {
        debug_assert!(pX.nKey <= 0x7fffffff && pX.pKey.is_some());
        nPayload = pX.nKey as i32;
        nSrc = nPayload;
        pSrc = pX.pKey.unwrap_or(&[]);
        nHeader += putVarint32(pCell, nHeader, nPayload as u32);
    }

    /* Fill in the payload */
    let mut pPayload = nHeader; /* Offset in pCell or in the current overflow page */
    if nPayload <= pPage.maxLocal as i32 {
        /* This is the common case where everything fits on the btree page
         ** and no overflow pages are required. */
        n = nHeader as i32 + nPayload;
        if n < 4 {
            n = 4;
            pCell[pPayload + nPayload as usize] = 0;
        }
        *pnSize = n;
        debug_assert!(nSrc <= nPayload);
        let ns = nSrc as usize;
        pCell[pPayload..pPayload + ns].copy_from_slice(&pSrc[..ns]);
        for b in &mut pCell[pPayload + ns..pPayload + nPayload as usize] {
            *b = 0;
        }
        return SQLITE_OK;
    }

    /* If we reach this point, it means that some of the content will need
     ** to spill onto overflow pages.
     */
    let mn = pPage.minLocal as i32;
    n = mn + (nPayload - mn) % (pPage.usableSize as i32 - 4);
    if n > pPage.maxLocal as i32 {
        n = mn;
    }
    let mut spaceLeft = n;
    *pnSize = n + nHeader as i32 + 4;
    let mut pPrior = Prior::Cell(nHeader + n as usize);
    let mut pToRelease: Option<DbPage> = None;
    let mut pgnoOvfl: Pgno = 0;
    let pBt = pPage.pBt.upgrade().expect("page without BtShared");
    let (autoVacuum, usableSize) = {
        let b = pBt.borrow();
        (b.autoVacuum, b.usableSize)
    };

    /* At this point variables should be set as follows:
     **
     **   nPayload           Total payload size in bytes
     **   pPayload           Begin writing payload here
     **   spaceLeft          Space available at pPayload.  If nPayload>spaceLeft,
     **                      that means content must spill into overflow pages.
     **   *pnSize            Size of the local cell (not counting overflow pages)
     **   pPrior             Where to write the pgno of the first overflow page
     */

    /* Write the payload into the local Cell and any extra into overflow pages */
    loop {
        n = nPayload;
        if n > spaceLeft {
            n = spaceLeft;
        }

        {
            let mut og = pToRelease.as_ref().map(|p| p.borrow_mut());
            let dest: &mut [u8] = match og.as_mut() {
                Some(g) => &mut g.aData,
                None => &mut *pCell,
            };
            if nSrc >= n {
                dest[pPayload..pPayload + n as usize].copy_from_slice(&pSrc[si..si + n as usize]);
            } else if nSrc > 0 {
                n = nSrc;
                dest[pPayload..pPayload + n as usize].copy_from_slice(&pSrc[si..si + n as usize]);
            } else {
                for b in &mut dest[pPayload..pPayload + n as usize] {
                    *b = 0;
                }
            }
        }
        nPayload -= n;
        if nPayload <= 0 {
            break;
        }
        pPayload += n as usize;
        if nSrc > 0 {
            si += n as usize;
        }
        nSrc -= n;
        spaceLeft -= n;
        if spaceLeft == 0 {
            let mut pOvfl: Option<DbPage> = None;
            let pgnoPtrmap = pgnoOvfl; /* Overflow page pointer-map entry page */
            if autoVacuum != 0 {
                loop {
                    pgnoOvfl += 1;
                    if !(PTRMAP_ISPAGE(&pBt, pgnoOvfl) || pgnoOvfl == pendingBytePage(&pBt)) {
                        break;
                    }
                }
            }
            let near = pgnoOvfl;
            let mut rc = allocateBtreePage(&pBt, &mut pOvfl, &mut pgnoOvfl, near, 0);
            /* If the database supports auto-vacuum, and the second or subsequent
             ** overflow page is being allocated, add an entry to the pointer-map
             ** for that page now.
             **
             ** If this is the first overflow page, then write a partial entry
             ** to the pointer-map. If we write nothing to this pointer-map slot,
             ** then the optimistic overflow chain processing in clearCell()
             ** may misinterpret the uninitialized values and delete the
             ** wrong pages from the database.
             */
            if autoVacuum != 0 && rc == SQLITE_OK {
                let eType = if pgnoPtrmap != 0 {
                    PTRMAP_OVERFLOW2
                } else {
                    PTRMAP_OVERFLOW1
                } as u8;
                ptrmapPut(&pBt, pgnoOvfl, eType, pgnoPtrmap, &mut rc);
                if rc != 0 {
                    releasePage(pOvfl.as_ref());
                }
            }
            if rc != 0 {
                releasePage(pToRelease.as_ref());
                return rc;
            }

            match pPrior {
                Prior::Cell(off) => put4byte(pCell, off, pgnoOvfl),
                Prior::Ovfl => put4byte(
                    &mut pToRelease.as_ref().unwrap().borrow_mut().aData,
                    0,
                    pgnoOvfl,
                ),
            }
            releasePage(pToRelease.as_ref());
            pToRelease = pOvfl;
            pPrior = Prior::Ovfl;
            put4byte(&mut pToRelease.as_ref().unwrap().borrow_mut().aData, 0, 0);
            pPayload = 4;
            spaceLeft = usableSize as i32 - 4;
        }
    }
    releasePage(pToRelease.as_ref());
    SQLITE_OK
}

/// Remove the i-th cell from pPage.  This routine effects pPage only.
/// The cell content is not freed or deallocated.  It is assumed that
/// the cell content has been copied someplace else.  This routine just
/// removes the reference to the cell from pPage.
///
/// "sz" must be the number of bytes in the cell.
fn dropCell(pg: &mut PgHdr, idx: usize, sz: i32, pRC: &mut i32) {
    if *pRC != 0 {
        return;
    }
    debug_assert!(idx < pg.pExtra.nCell as usize);
    debug_assert!(pg.pExtra.nFree >= 0);
    let ptr = pg.pExtra.aCellIdx() + 2 * idx;
    let usableSize = pg.pExtra.usableSize;
    let pc = get2byte(&pg.aData, ptr) as u32;
    let hdr = pg.pExtra.hdrOffset as usize;
    if pc + sz as u32 > usableSize {
        *pRC = SQLITE_CORRUPT_BKPT();
        return;
    }
    let rc = freeSpace(pg, pc as i32, sz);
    if rc != 0 {
        *pRC = rc;
        return;
    }
    let PgHdr {
        aData: data,
        pExtra: pPage,
        ..
    } = pg;
    pPage.nCell -= 1;
    if pPage.nCell == 0 {
        for b in &mut data[hdr + 1..hdr + 5] {
            *b = 0;
        }
        data[hdr + 7] = 0;
        put2byte(data, hdr + 5, usableSize as usize);
        pPage.nFree = usableSize as i32 - pPage.hdrOffset as i32 - pPage.childPtrSize as i32 - 8;
    } else {
        let n = 2 * (pPage.nCell as usize - idx);
        data.copy_within(ptr + 2..ptr + 2 + n, ptr);
        put2byte(data, hdr + 3, pPage.nCell as usize);
        pPage.nFree += 2;
    }
}

/// dropCell() on a DbPage.
#[inline]
fn dropCellPg(pg: &DbPage, idx: usize, sz: i32, pRC: &mut i32) {
    if *pRC != 0 {
        return;
    }
    dropCell(&mut pg.borrow_mut(), idx, sz, pRC);
}

/// Insert a new cell on pPage at cell index "i".  pCell points to the
/// content of the cell.
///
/// If the cell content will fit on the page, then put it there.  If it
/// will not fit, then make a copy of the cell content into pTemp if
/// pTemp is not null.  Regardless of pTemp, allocate a new entry
/// in pPage->apOvfl[] and make it point to the cell content (either
/// in pTemp or the original pCell) and also record its index.
/// Allocating a new entry in pPage->aCell[] implies that
/// pPage->nOverflow is incremented.
///
/// The insertCellFast() routine below works exactly the same as
/// insertCell() except that it lacks the pTemp and iChild parameters
/// which are assumed zero.  Other than that, the two routines are the
/// same.
///
/// Fixes or enhancements to this routine should be reflected in
/// insertCellFast()!
///
/// Rust: the overflow cell is always copied (apOvfl owns its content),
/// so there is no pTemp parameter.
fn insertCell(pg: &DbPage, i: usize, pCell: &[u8], sz: i32, iChild: Pgno) -> i32 {
    let mut idx = 0i32; /* Where to write new cell content in data[] */
    debug_assert!(iChild > 0);
    let (nOverflow, nFree) = {
        let m = mp(pg);
        debug_assert!(i <= m.nCell as usize + m.nOverflow as usize);
        debug_assert!(m.nFree >= 0);
        (m.nOverflow, m.nFree)
    };
    if nOverflow != 0 || sz + 2 > nFree {
        let mut cell: Vec<u8> = pCell[..std::cmp::min(sz as usize, pCell.len())].to_vec();
        cell.resize(std::cmp::max(sz as usize, 4), 0);
        put4byte(&mut cell, 0, iChild);
        let mut m = mpm(pg);
        let j = m.nOverflow as usize;
        m.nOverflow += 1;
        /* Comparison against ArraySize-1 since we hold back one extra slot
         ** as a contingency.  In other words, never need more than 3 overflow
         ** slots but 4 are allocated, just to be safe. */
        debug_assert!(j < 4 - 1);
        m.apOvfl[j] = cell;
        m.aiOvfl[j] = i as u16;

        /* When multiple overflows occur, they are always sequential and in
         ** sorted order.  This invariants arise because multiple overflows can
         ** only occur when inserting divider cells into the parent page during
         ** balancing, and the dividers are adjacent and sorted.
         */
        debug_assert!(j == 0 || (m.aiOvfl[j - 1] as usize) < i); /* Overflows in sorted order */
        debug_assert!(j == 0 || i == m.aiOvfl[j - 1] as usize + 1); /* Overflows are sequential */
    } else {
        let rc = sqlite3PagerWrite(pg);
        if rc != SQLITE_OK {
            return rc;
        }
        let lite;
        {
            let mut g = pg.borrow_mut();
            let rc = allocateSpace(&mut g, sz, &mut idx);
            if rc != 0 {
                return rc;
            }
            /* The allocateSpace() routine guarantees the following properties
             ** if it returns successfully */
            debug_assert!(idx >= 0);
            debug_assert!(idx + sz <= g.pExtra.usableSize as i32);
            let PgHdr {
                aData: data,
                pExtra: pPage,
                ..
            } = &mut *g;
            pPage.nFree -= (2 + sz) as u16 as i32;
            /* In a corrupt database where an entry in the cell index section of
             ** a btree page has a value of 3 or less, the pCell value might point
             ** as many as 4 bytes in front of the start of the aData buffer for
             ** the source page.  Make sure this does not cause problems by not
             ** reading the first 4 bytes */
            let idx = idx as usize;
            let sz = sz as usize;
            data[idx + 4..idx + sz].copy_from_slice(&pCell[4..sz]);
            put4byte(data, idx, iChild);
            let pIns = pPage.aCellIdx() + i * 2;
            let nMove = 2 * (pPage.nCell as usize - i);
            data.copy_within(pIns..pIns + nMove, pIns + 2);
            put2byte(data, pIns, idx);
            pPage.nCell += 1;
            /* increment the cell count */
            let h = pPage.hdrOffset as usize;
            data[h + 4] = data[h + 4].wrapping_add(1);
            if data[h + 4] == 0 {
                data[h + 3] = data[h + 3].wrapping_add(1);
            }
            debug_assert!(get2byte(data, h + 3) == pPage.nCell as usize);
            lite = pPage.lite();
        }
        let autoVacuum = lite.pBt.upgrade().map_or(0, |b| b.borrow().autoVacuum);
        if autoVacuum != 0 {
            let mut rc2 = SQLITE_OK;
            /* The cell may contain a pointer to an overflow page. If so, write
             ** the entry for the overflow page into the pointer map.
             */
            ptrmapPutOvflPtr(&lite, pCell, 0, None, &mut rc2);
            if rc2 != 0 {
                return rc2;
            }
        }
    }
    SQLITE_OK
}

/// This variant of insertCell() assumes that the pTemp and iChild
/// parameters are both zero.  Use this variant in sqlite3BtreeInsert()
/// for performance improvement, and also so that this variant is only
/// called from that one place, and is thus inlined, and thus runs must
/// faster.
///
/// Fixes or enhancements to this routine should be reflected into
/// the insertCell() routine.
fn insertCellFast(pg: &DbPage, i: usize, pCell: &[u8], sz: i32) -> i32 {
    let mut idx = 0i32; /* Where to write new cell content in data[] */
    let nFree = {
        let m = mp(pg);
        debug_assert!(i <= m.nCell as usize + m.nOverflow as usize);
        debug_assert!(m.nFree >= 0);
        debug_assert!(m.nOverflow == 0);
        m.nFree
    };
    if sz + 2 > nFree {
        let mut m = mpm(pg);
        let j = m.nOverflow as usize;
        m.nOverflow += 1;
        /* Comparison against ArraySize-1 since we hold back one extra slot
         ** as a contingency.  In other words, never need more than 3 overflow
         ** slots but 4 are allocated, just to be safe. */
        debug_assert!(j < 4 - 1);
        m.apOvfl[j] = pCell[..sz as usize].to_vec();
        m.aiOvfl[j] = i as u16;
    } else {
        let rc = sqlite3PagerWrite(pg);
        if rc != SQLITE_OK {
            return rc;
        }
        let lite;
        {
            let mut g = pg.borrow_mut();
            let rc = allocateSpace(&mut g, sz, &mut idx);
            if rc != 0 {
                return rc;
            }
            /* The allocateSpace() routine guarantees the following properties
             ** if it returns successfully */
            debug_assert!(idx >= 0);
            debug_assert!(idx + sz <= g.pExtra.usableSize as i32);
            let PgHdr {
                aData: data,
                pExtra: pPage,
                ..
            } = &mut *g;
            pPage.nFree -= (2 + sz) as u16 as i32;
            let idx = idx as usize;
            let sz = sz as usize;
            data[idx..idx + sz].copy_from_slice(&pCell[..sz]);
            let pIns = pPage.aCellIdx() + i * 2;
            let nMove = 2 * (pPage.nCell as usize - i);
            data.copy_within(pIns..pIns + nMove, pIns + 2);
            put2byte(data, pIns, idx);
            pPage.nCell += 1;
            /* increment the cell count */
            let h = pPage.hdrOffset as usize;
            data[h + 4] = data[h + 4].wrapping_add(1);
            if data[h + 4] == 0 {
                data[h + 3] = data[h + 3].wrapping_add(1);
            }
            debug_assert!(get2byte(data, h + 3) == pPage.nCell as usize);
            lite = pPage.lite();
        }
        let autoVacuum = lite.pBt.upgrade().map_or(0, |b| b.borrow().autoVacuum);
        if autoVacuum != 0 {
            let mut rc2 = SQLITE_OK;
            /* The cell may contain a pointer to an overflow page. If so, write
             ** the entry for the overflow page into the pointer map.
             */
            ptrmapPutOvflPtr(&lite, pCell, 0, None, &mut rc2);
            if rc2 != 0 {
                return rc2;
            }
        }
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Balancing
// ---------------------------------------------------------------------------

/// Where a cell of a CellArray lives (C: a pointer, possibly into a page).
#[derive(Clone, Copy, PartialEq, Debug)]
enum CellSrc {
    /// In the image of CellArray::pages[k]
    Page(usize),
    /// In CellArray::space1 (copies of divider cells)
    Space1,
    /// In CellArray::ovfl[k] (an overflow cell)
    Ovfl(usize),
}

#[derive(Clone, Copy, Debug)]
struct CellRef {
    src: CellSrc,
    off: usize,
}

/// A CellArray object contains a cache of pointers and sizes for a
/// consecutive sequence of cells that might be held on multiple pages.
///
/// The cells in this array are the divider cell or cells from the pParent
/// page plus up to three child pages.  There are a total of nCell cells.
///
/// pRef is a pointer to one of the pages that contributes cells.  This is
/// used to access information such as MemPage.intKey and MemPage.pBt->pageSize
/// which should be common to all pages that contribute cells to this array.
///
/// apCell[] and szCell[] hold, respectively, pointers to the start of each
/// cell and the size of each cell.  Some of the apCell[] pointers might refer
/// to overflow cells.  In other words, some apCel[] pointers might not point
/// to content area of the pages.
///
/// A szCell[] of zero means the size of that cell has not yet been computed.
///
/// Rust: the C apEnd[]/ixNx[] arrays (used to find the end of the buffer a
/// cell comes from, for corruption checks) are replaced by the CellSrc of
/// each cell: a cell from page k "ends" at the end of page k's image.
struct CellArray {
    /// Number of cells in apCell[]
    nCell: i32,
    /// Reference page
    pRef: MemPage,
    /// All cells begin balanced
    apCell: Vec<CellRef>,
    /// Local size of all cells in apCell[]
    szCell: Vec<u16>,
    /// The pages the cells come from (CellSrc::Page)
    pages: Vec<DbPage>,
    /// Space for copies of dividers cells (C aSpace1)
    space1: Vec<u8>,
    /// Copies of overflow cells
    ovfl: Vec<Vec<u8>>,
}

/// Copy a[off..off+sz] to out (bytes past the end of `a` read as zero).
#[inline]
fn copyOut(a: &[u8], off: usize, sz: usize, out: &mut Vec<u8>) {
    out.clear();
    let end = std::cmp::min(off + sz, a.len());
    if off < end {
        out.extend_from_slice(&a[off..end]);
    }
    out.resize(sz, 0);
}

impl CellArray {
    fn new(pRef: MemPage) -> CellArray {
        CellArray {
            nCell: 0,
            pRef,
            apCell: Vec::new(),
            szCell: Vec::new(),
            pages: Vec::new(),
            space1: Vec::new(),
            ovfl: Vec::new(),
        }
    }

    /// Size of cell N computed with pRef->xCellSize.  No page may be
    /// mutably borrowed.
    fn xCellSize(&self, N: usize) -> u16 {
        let r = self.apCell[N];
        match r.src {
            CellSrc::Page(k) => {
                let g = self.pages[k].borrow();
                (self.pRef.xCellSize)(&self.pRef, &g.aData, r.off)
            }
            CellSrc::Space1 => (self.pRef.xCellSize)(&self.pRef, &self.space1, r.off),
            CellSrc::Ovfl(k) => (self.pRef.xCellSize)(&self.pRef, &self.ovfl[k], r.off),
        }
    }

    /// Copy sz bytes of cell N starting at byte `delta` relative to the cell
    /// start (delta may be negative: C `pCell-4`).  `tgt` is the page that
    /// is currently mutably borrowed (with its image) if any.
    fn copyCell(
        &self,
        N: usize,
        delta: isize,
        sz: usize,
        out: &mut Vec<u8>,
        tgt: Option<(&DbPage, &[u8])>,
    ) {
        let r = self.apCell[N];
        let off = r.off as isize + delta;
        if off < 0 {
            /* Bytes before the start of the buffer: zero (they are always
             ** overwritten by the caller). */
            let skip = (-off) as usize;
            let mut tmp = Vec::new();
            self.copyCell(
                N,
                delta + skip as isize,
                sz.saturating_sub(skip),
                &mut tmp,
                tgt,
            );
            out.clear();
            out.resize(skip.min(sz), 0);
            out.extend_from_slice(&tmp);
            return;
        }
        let off = off as usize;
        match r.src {
            CellSrc::Page(k) => match tgt {
                Some((t, data)) if Rc::ptr_eq(t, &self.pages[k]) => copyOut(data, off, sz, out),
                _ => {
                    let g = self.pages[k].borrow();
                    copyOut(&g.aData, off, sz, out)
                }
            },
            CellSrc::Space1 => copyOut(&self.space1, off, sz, out),
            CellSrc::Ovfl(k) => copyOut(&self.ovfl[k], off, sz, out),
        }
    }

    /// True if cell N lies in page pg at an offset in [lo, hi)
    /// (SQLITE_WITHIN(pCell, &pg->aData[lo], &pg->aData[hi])).
    fn within(&self, N: usize, pg: &DbPage, lo: usize, hi: usize) -> bool {
        let r = self.apCell[N];
        match r.src {
            CellSrc::Page(k) => Rc::ptr_eq(&self.pages[k], pg) && r.off >= lo && r.off < hi,
            _ => false,
        }
    }

    /// The C check `apCell[N]+sz > pEnd && apCell[N] < pEnd` where pEnd
    /// is the end of the buffer the cell comes from.
    fn straddlesEnd(&self, N: usize, delta: isize, sz: usize) -> bool {
        let r = self.apCell[N];
        match r.src {
            CellSrc::Page(k) => {
                let end = mp(&self.pages[k]).aDataEnd() as isize;
                let off = r.off as isize + delta;
                off < end && off + sz as isize > end
            }
            _ => false,
        }
    }
}

/// Make sure the cell sizes at idx, idx+1, ..., idx+N-1 have been
/// computed.
fn populateCellCache(p: &mut CellArray, idx: usize, N: usize) {
    debug_assert!(idx + N <= p.nCell as usize);
    for i in idx..idx + N {
        if p.szCell[i] == 0 {
            p.szCell[i] = p.xCellSize(i);
        }
    }
}

/// Return the size of the Nth element of the cell array
fn computeCellSize(p: &mut CellArray, N: usize) -> u16 {
    debug_assert!(N < p.nCell as usize);
    debug_assert!(p.szCell[N] == 0);
    p.szCell[N] = p.xCellSize(N);
    p.szCell[N]
}
fn cachedCellSize(p: &mut CellArray, N: usize) -> u16 {
    debug_assert!(N < p.nCell as usize);
    if p.szCell[N] != 0 {
        return p.szCell[N];
    }
    computeCellSize(p, N)
}

/// Array apCell[] contains pointers to nCell b-tree page cells. The
/// szCell[] array contains the size in bytes of each cell. This function
/// replaces the current contents of page pPg with the contents of the cell
/// array.
///
/// Some of the cells in apCell[] may currently be stored in pPg. This
/// function works around problems caused by this by making a copy of any
/// such cells before overwriting the page data.
///
/// The MemPage.nFree field is invalidated by this function. It is the
/// responsibility of the caller to set it correctly.
fn rebuildPage(pCArray: &CellArray, iFirst: usize, nCell: usize, pPg: &DbPage) -> i32 {
    let mut g = pPg.borrow_mut();
    let hdr = g.pExtra.hdrOffset as usize;
    let usableSize = g.pExtra.usableSize as usize;
    let pEnd = usableSize;
    let mut i = iFirst;
    let iEnd = i + nCell;
    let mut pCellptr = g.pExtra.aCellIdx();

    debug_assert!(nCell > 0);
    debug_assert!(i < iEnd);
    let mut j = get2byte(&g.aData, hdr + 5);
    if j > usableSize {
        j = 0;
    }
    let pTmp: Vec<u8> = g.aData[j..usableSize].to_vec();

    let mut pData = pEnd as isize;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let sz = pCArray.szCell[i] as usize;
        debug_assert!(sz > 0);
        if pCArray.within(i, pPg, j, pEnd) {
            let off = pCArray.apCell[i].off;
            if off + sz > pEnd {
                return SQLITE_CORRUPT_BKPT();
            }
            copyOut(&pTmp, off - j, sz, &mut buf);
        } else {
            if pCArray.straddlesEnd(i, 0, sz) {
                return SQLITE_CORRUPT_BKPT();
            }
            pCArray.copyCell(i, 0, sz, &mut buf, Some((pPg, &g.aData)));
        }

        pData -= sz as isize;
        if pData < 0 {
            return SQLITE_CORRUPT_BKPT();
        }
        put2byte(&mut g.aData, pCellptr, pData as usize);
        pCellptr += 2;
        if pData < pCellptr as isize {
            return SQLITE_CORRUPT_BKPT();
        }
        let pd = pData as usize;
        g.aData[pd..pd + sz].copy_from_slice(&buf);
        i += 1;
        if i >= iEnd {
            break;
        }
    }

    /* The pPg->nFree field is now set incorrectly. The caller will fix it. */
    debug_assert!(nCell < 10922);
    g.pExtra.nCell = nCell as u16;
    g.pExtra.nOverflow = 0;

    put2byte(&mut g.aData, hdr + 1, 0);
    put2byte(&mut g.aData, hdr + 3, nCell);
    put2byte(&mut g.aData, hdr + 5, pData as usize);
    g.aData[hdr + 7] = 0x00;
    SQLITE_OK
}

/// The pCArray objects contains pointers to b-tree cells and the cell sizes.
/// This function attempts to add the cells stored in the array to page pPg.
/// If it cannot (because the page needs to be defragmented before the cells
/// will fit), non-zero is returned. Otherwise, if the cells are added
/// successfully, zero is returned.
///
/// Argument pCellptr points to the first entry in the cell-pointer array
/// (part of page pPg) to populate. After cell apCell[0] is written to the
/// page body, a 16-bit offset is written to pCellptr. And so on, for each
/// cell in the array. It is the responsibility of the caller to ensure
/// that it is safe to overwrite this part of the cell-pointer array.
///
/// When this function is called, *ppData points to the start of the
/// content area on page pPg. If the size of the content area is extended,
/// *ppData is updated to point to the new start of the content area
/// before returning.
///
/// Finally, argument pBegin points to the byte immediately following the
/// end of the space required by this page for the cell-pointer area (for
/// all cells - not just those inserted by the current call). If the content
/// area must be extended to before this point in order to accommodate all
/// cells in apCell[], then the cells do not fit and non-zero is returned.
fn pageInsertArray(
    pPg: &DbPage,
    pBegin: usize,
    ppData: &mut usize,
    pCellptr: usize,
    iFirst: usize,
    nCell: usize,
    pCArray: &CellArray,
) -> i32 {
    let mut i = iFirst;
    let mut pData = *ppData;
    let iEnd = iFirst + nCell;
    let mut pCellptr = pCellptr;
    if iEnd <= iFirst {
        return 0;
    }
    let mut g = pPg.borrow_mut();
    debug_assert!(g.pExtra.hdrOffset == 0); /* Never called on page 1 */
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let sz = pCArray.szCell[i] as usize;
        debug_assert!(sz != 0);
        let mut slot = None;
        if !(g.aData[1] == 0 && g.aData[2] == 0) {
            let mut rc = SQLITE_OK;
            let PgHdr {
                aData, pExtra: p, ..
            } = &mut *g;
            slot = pageFindSlot(p, aData, sz as i32, &mut rc);
        }
        let pSlot = match slot {
            Some(s) => s,
            None => {
                if (pData as isize - pBegin as isize) < sz as isize {
                    return 1;
                }
                pData -= sz;
                pData
            }
        };
        /* pSlot and pCArray->apCell[i] will never overlap on a well-formed
         ** database.  But they might for a corrupt database.  Hence use memmove()
         ** since memcpy() sends SIGABORT with overlapping buffers on OpenBSD */
        if pCArray.straddlesEnd(i, 0, sz) {
            return 1;
        }
        pCArray.copyCell(i, 0, sz, &mut buf, Some((pPg, &g.aData)));
        g.aData[pSlot..pSlot + sz].copy_from_slice(&buf);
        put2byte(&mut g.aData, pCellptr, pSlot);
        pCellptr += 2;
        i += 1;
        if i >= iEnd {
            break;
        }
    }
    *ppData = pData;
    0
}

/// The pCArray object contains pointers to b-tree cells and their sizes.
///
/// This function adds the space associated with each cell in the array
/// that is currently stored within the body of pPg to the pPg free-list.
/// The cell-pointers and other fields of the page are not updated.
///
/// This function returns the total number of cells added to the free-list.
fn pageFreeArray(pPg: &DbPage, iFirst: usize, nCell: usize, pCArray: &CellArray) -> i32 {
    let mut g = pPg.borrow_mut();
    let pEnd = g.pExtra.usableSize as usize;
    let pStart = g.pExtra.hdrOffset as usize + 8 + g.pExtra.childPtrSize as usize;
    let mut nRet = 0;
    let iEnd = iFirst + nCell;
    let mut nFree = 0usize;
    let mut aOfst = [0usize; 10];
    let mut aAfter = [0usize; 10];

    for i in iFirst..iEnd {
        if pCArray.within(i, pPg, pStart, pEnd) {
            /* No need to use cachedCellSize() here.  The sizes of all cells that
             ** are to be freed have already been computing while deciding which
             ** cells need freeing */
            let sz = pCArray.szCell[i] as usize;
            debug_assert!(sz > 0);
            let iOfst = pCArray.apCell[i].off as u16 as usize;
            let iAfter = iOfst + sz;
            let mut j = 0;
            while j < nFree {
                if aOfst[j] == iAfter {
                    aOfst[j] = iOfst;
                    break;
                } else if aAfter[j] == iOfst {
                    aAfter[j] = iAfter;
                    break;
                }
                j += 1;
            }
            if j >= nFree {
                if nFree >= aOfst.len() {
                    for j in 0..nFree {
                        freeSpace(&mut g, aOfst[j] as i32, (aAfter[j] - aOfst[j]) as i32);
                    }
                    nFree = 0;
                }
                aOfst[nFree] = iOfst;
                aAfter[nFree] = iAfter;
                if iAfter > pEnd {
                    return 0;
                }
                nFree += 1;
            }
            nRet += 1;
        }
    }
    for j in 0..nFree {
        freeSpace(&mut g, aOfst[j] as i32, (aAfter[j] - aOfst[j]) as i32);
    }
    nRet
}

/// pCArray contains pointers to and sizes of all cells in the page being
/// balanced.  The current page, pPg, has pPg->nCell cells starting with
/// pCArray->apCell[iOld].  After balancing, this page should hold nNew cells
/// starting at apCell[iNew].
///
/// This routine makes the necessary adjustments to pPg so that it contains
/// the correct cells after being balanced.
///
/// The pPg->nFree field is invalid when this function returns. It is the
/// responsibility of the caller to set it correctly.
fn editPage(pPg: &DbPage, iOld: i32, iNew: i32, nNew: i32, pCArray: &mut CellArray) -> i32 {
    let (hdr, cellIdx, pgNCell, nOverflow, aiOvfl, aDataEnd) = {
        let m = mp(pPg);
        (
            m.hdrOffset as usize,
            m.aCellIdx(),
            m.nCell as i32,
            m.nOverflow as usize,
            m.aiOvfl,
            m.aDataEnd(),
        )
    };
    let pBegin = cellIdx + nNew as usize * 2;
    let mut nCell = pgNCell; /* Cells stored on pPg */
    let iOldEnd = iOld + pgNCell + nOverflow as i32;
    let iNewEnd = iNew + nNew;

    'editpage_fail: {
        /* Remove cells from the start and end of the page */
        debug_assert!(nCell >= 0);
        if iOld < iNew {
            let nShift = pageFreeArray(pPg, iOld as usize, (iNew - iOld) as usize, pCArray);
            if nShift > nCell {
                return SQLITE_CORRUPT_BKPT();
            }
            let mut g = pPg.borrow_mut();
            let from = cellIdx + nShift as usize * 2;
            let len = std::cmp::min(nCell as usize * 2, g.aData.len().saturating_sub(from));
            g.aData.copy_within(from..from + len, cellIdx);
            nCell -= nShift;
        }
        if iNewEnd < iOldEnd {
            let nTail = pageFreeArray(pPg, iNewEnd as usize, (iOldEnd - iNewEnd) as usize, pCArray);
            debug_assert!(nCell >= nTail);
            nCell -= nTail;
        }

        let mut pData = get2byte(&pPg.borrow().aData, hdr + 5);
        if pData < pBegin {
            break 'editpage_fail;
        }
        if pData > aDataEnd {
            break 'editpage_fail;
        }

        /* Add cells to the start of the page */
        if iNew < iOld {
            let nAdd = std::cmp::min(nNew, iOld - iNew);
            debug_assert!(nAdd >= 0);
            let pCellptr = cellIdx;
            {
                let mut g = pPg.borrow_mut();
                let len = nCell as usize * 2;
                g.aData
                    .copy_within(pCellptr..pCellptr + len, pCellptr + nAdd as usize * 2);
            }
            if pageInsertArray(
                pPg,
                pBegin,
                &mut pData,
                pCellptr,
                iNew as usize,
                nAdd as usize,
                pCArray,
            ) != 0
            {
                break 'editpage_fail;
            }
            nCell += nAdd;
        }

        /* Add any overflow cells */
        for i in 0..nOverflow {
            let iCell = (iOld + aiOvfl[i] as i32) - iNew;
            if iCell >= 0 && iCell < nNew {
                let pCellptr = cellIdx + iCell as usize * 2;
                if nCell > iCell {
                    let mut g = pPg.borrow_mut();
                    let len = (nCell - iCell) as usize * 2;
                    g.aData.copy_within(pCellptr..pCellptr + len, pCellptr + 2);
                }
                nCell += 1;
                cachedCellSize(pCArray, (iCell + iNew) as usize);
                if pageInsertArray(
                    pPg,
                    pBegin,
                    &mut pData,
                    pCellptr,
                    (iCell + iNew) as usize,
                    1,
                    pCArray,
                ) != 0
                {
                    break 'editpage_fail;
                }
            }
        }

        /* Append cells to the end of the page */
        debug_assert!(nCell >= 0);
        let pCellptr = cellIdx + nCell as usize * 2;
        if pageInsertArray(
            pPg,
            pBegin,
            &mut pData,
            pCellptr,
            (iNew + nCell) as usize,
            (nNew - nCell) as usize,
            pCArray,
        ) != 0
        {
            break 'editpage_fail;
        }

        debug_assert!(nNew < 10922);
        let mut g = pPg.borrow_mut();
        g.pExtra.nCell = nNew as u16;
        g.pExtra.nOverflow = 0;

        put2byte(&mut g.aData, hdr + 3, nNew as usize);
        put2byte(&mut g.aData, hdr + 5, pData);

        return SQLITE_OK;
    }
    /* editpage_fail: */
    /* Unable to edit this page. Rebuild it from scratch instead. */
    if nNew < 1 {
        return SQLITE_CORRUPT_BKPT();
    }
    populateCellCache(pCArray, iNew as usize, nNew as usize);
    rebuildPage(pCArray, iNew as usize, nNew as usize, pPg)
}

/// This version of balance() handles the common special case where
/// a new entry is being inserted on the extreme right-end of the
/// tree, in other words, when the new entry will become the largest
/// entry in the tree.
///
/// Instead of trying to balance the 3 right-most leaf pages, just add
/// a new page to the right-hand side and put the one new entry in
/// that page.  This leaves the right side of the tree somewhat
/// unbalanced.  But odds are that we will be inserting new entries
/// at the end soon afterwards so the nearly empty page will quickly
/// fill up.  On average.
///
/// pPage is the leaf page which is the right-most page in the tree.
/// pParent is its parent.  pPage must have a single overflow entry
/// which is also the right-most entry on the page.
///
/// The pSpace buffer is used to store a temporary copy of the divider
/// cell that will be inserted into pParent. Such a cell consists of a 4
/// byte page number followed by a variable length integer. In other
/// words, at most 13 bytes. Hence the pSpace buffer must be at
/// least 13 bytes in size.
fn balance_quick(pParent: &DbPage, pPage: &DbPage) -> i32 {
    let pBt = mp(pPage).pBt.upgrade().expect("page without BtShared");
    let mut pNew: Option<DbPage> = None;
    let mut pgnoNew: Pgno = 0;

    debug_assert!(mp(pPage).nOverflow == 1);

    if mp(pPage).nCell == 0 {
        return SQLITE_CORRUPT_BKPT(); /* dbfuzz001.test */
    }

    /* Allocate a new page. This page will become the right-sibling of
     ** pPage. Make the parent page writable, so that the new divider cell
     ** may be inserted. If both these operations are successful, proceed.
     */
    let mut rc = allocateBtreePage(&pBt, &mut pNew, &mut pgnoNew, 0, 0);

    if rc == SQLITE_OK {
        let pNew = pNew.unwrap();
        let mut pSpace = [0u8; 13];
        let mut pOut = 4usize;
        let (pCell, pRef) = {
            let m = mp(pPage);
            (m.apOvfl[0].clone(), m.lite())
        };
        let szCell = (pRef.xCellSize)(&pRef, &pCell, 0);

        zeroPage(&mut pNew.borrow_mut(), PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF);
        let mut b = CellArray::new(pRef);
        b.nCell = 1;
        b.ovfl.push(pCell.clone());
        b.apCell.push(CellRef {
            src: CellSrc::Ovfl(0),
            off: 0,
        });
        b.szCell.push(szCell);
        rc = rebuildPage(&b, 0, 1, &pNew);
        if rc != 0 {
            releasePage(Some(&pNew));
            return rc;
        }
        let usableSize = pBt.borrow().usableSize as i32;
        {
            let mut m = mpm(&pNew);
            m.nFree = usableSize - m.cellOffset as i32 - 2 - szCell as i32;
        }

        /* If this is an auto-vacuum database, update the pointer map
         ** with entries for the new page, and any pointer from the
         ** cell on the page to an overflow page. If either of these
         ** operations fails, the return code is set, but the contents
         ** of the parent page are still manipulated by the code below.
         ** That is Ok, at this point the parent page is guaranteed to
         ** be marked as dirty. Returning an error code will cause a
         ** rollback, undoing any changes made to the parent page.
         */
        if pBt.borrow().autoVacuum != 0 {
            let parentPgno = pgnoOf(pParent);
            ptrmapPut(&pBt, pgnoNew, PTRMAP_BTREE as u8, parentPgno, &mut rc);
            let newLite = mp(&pNew).lite();
            if szCell > newLite.minLocal {
                ptrmapPutOvflPtr(&newLite, &pCell, 0, None, &mut rc);
            }
        }

        /* Create a divider cell to insert into pParent. The divider cell
         ** consists of a 4-byte page number (the page number of pPage) and
         ** a variable length key value (which must be the same value as the
         ** largest key on pPage).
         **
         ** To find the largest key value on pPage, first find the right-most
         ** cell on pPage. The first two fields of this cell are the
         ** record-length (a variable length integer at most 32-bits in size)
         ** and the key value (a variable length integer, may have any value).
         ** The first of the while(...) loops below skips over the record-length
         ** field. The second while(...) loop copies the key value from the
         ** cell on pPage into the pSpace buffer.
         */
        {
            let g = pPage.borrow();
            let mut c = findCell(&g.pExtra, &g.aData, g.pExtra.nCell as usize - 1);
            let mut pStop = c + 9;
            loop {
                let b = at(&g.aData, c);
                c += 1;
                if !((b & 0x80) != 0 && c < pStop) {
                    break;
                }
            }
            pStop = c + 9;
            loop {
                let b = at(&g.aData, c);
                pSpace[pOut] = b;
                pOut += 1;
                c += 1;
                if !((b & 0x80) != 0 && c < pStop) {
                    break;
                }
            }
        }

        /* Insert the new divider cell into pParent. */
        if rc == SQLITE_OK {
            let nCellParent = mp(pParent).nCell as usize;
            let pgnoPage = pgnoOf(pPage);
            rc = insertCell(pParent, nCellParent, &pSpace[..pOut], pOut as i32, pgnoPage);
        }

        /* Set the right-child pointer of pParent to point to the new page. */
        {
            let mut g = pParent.borrow_mut();
            let h = g.pExtra.hdrOffset as usize;
            put4byte(&mut g.aData, h + 8, pgnoNew);
        }

        /* Release the reference to the new page. */
        releasePage(Some(&pNew));
    }

    rc
}

/// This function is used to copy the contents of the b-tree node stored
/// on page pFrom to page pTo. If page pFrom was not a leaf page, then
/// the pointer-map entries for each child page are updated so that the
/// parent page stored in the pointer map is page pTo. If pFrom contained
/// any cells with overflow page pointers, then the corresponding pointer
/// map entries are also updated so that the parent page is page pTo.
///
/// If pFrom is currently carrying any overflow cells (entries in the
/// MemPage.apOvfl[] array), they are not copied to pTo.
///
/// Before returning, page pTo is reinitialized using btreeInitPage().
///
/// The performance of this function is not critical. It is only used by
/// the balance_shallower() and balance_deeper() procedures, neither of
/// which are called often under normal circumstances.
fn copyNodeContent(pFrom: &DbPage, pTo: &DbPage, pRC: &mut i32) {
    if *pRC == SQLITE_OK {
        let rc;
        {
            let f = pFrom.borrow();
            let mut t = pTo.borrow_mut();
            let usableSize = f.pExtra.usableSize as usize;
            let iFromHdr = f.pExtra.hdrOffset as usize;
            let iToHdr = if t.pExtra.pgno == 1 { 100 } else { 0 };

            debug_assert!(f.pExtra.isInit != 0);
            debug_assert!(f.pExtra.nFree >= iToHdr as i32);
            debug_assert!(get2byte(&f.aData, iFromHdr + 5) <= usableSize);

            /* Copy the b-tree node content from page pFrom to page pTo. */
            let iData = get2byte(&f.aData, iFromHdr + 5);
            if iData < usableSize {
                t.aData[iData..usableSize].copy_from_slice(&f.aData[iData..usableSize]);
            }
            let n = f.pExtra.cellOffset as usize + 2 * f.pExtra.nCell as usize;
            let n = std::cmp::min(
                n,
                std::cmp::min(f.aData.len() - iFromHdr, t.aData.len() - iToHdr),
            );
            t.aData[iToHdr..iToHdr + n].copy_from_slice(&f.aData[iFromHdr..iFromHdr + n]);

            /* Reinitialize page pTo so that the contents of the MemPage structure
             ** match the new data. The initialization of pTo can actually fail under
             ** fairly obscure circumstances, even though it is a copy of initialized
             ** page pFrom.
             */
            t.pExtra.isInit = 0;
            let mut r = btreeInitPage(&mut t);
            if r == SQLITE_OK {
                r = btreeComputeFreeSpace(&mut t);
            }
            rc = r;
        }
        if rc != SQLITE_OK {
            *pRC = rc;
            return;
        }

        /* If this is an auto-vacuum database, update the pointer-map entries
         ** for any b-tree or overflow pages that pTo now contains the pointers to.
         */
        let autoVacuum = mp(pFrom).pBt.upgrade().map_or(0, |b| b.borrow().autoVacuum);
        if autoVacuum != 0 {
            *pRC = setChildPtrmaps(pTo);
        }
    }
}

/// This routine redistributes cells on the iParentIdx'th child of pParent
/// (hereafter "the page") and up to 2 siblings so that all pages have about the
/// same amount of free space. Usually a single sibling on either side of the
/// page are used in the balancing, though both siblings might come from one
/// side if the page is the first or last child of its parent. If the page
/// has fewer than 2 siblings (something which can only happen if the page
/// is a root page or a child of a root page) then all available siblings
/// participate in the balancing.
///
/// The number of siblings of the page might be increased or decreased by
/// one or two in an effort to keep pages nearly full but not over full.
///
/// Note that when this routine is called, some of the cells on the page
/// might not actually be stored in MemPage.aData[]. This can happen
/// if the page is overfull. This routine ensures that all cells allocated
/// to the page and its siblings fit into MemPage.aData[] before returning.
///
/// In the course of balancing the page and its siblings, cells may be
/// inserted into or removed from the parent page (pParent). Doing so
/// may cause the parent page to become overfull or underfull. If this
/// happens, it is the responsibility of the caller to invoke the correct
/// balancing routine to fix this problem (see the balance() routine).
///
/// If this routine fails for any reason, it might leave the database
/// in a corrupted state. So if this routine fails, the database should
/// be rolled back.
///
/// The third argument to this function, aOvflSpace, is a pointer to a
/// buffer big enough to hold one page. If while inserting cells into the parent
/// page (pParent) the parent page becomes overfull, this buffer is
/// used to store the parent's overflow cells. Because this function inserts
/// a maximum of four divider cells into the parent page, and the maximum
/// size of a cell stored within an internal node is always less than 1/4
/// of the page-size, the aOvflSpace[] buffer is guaranteed to be large
/// enough for all overflow cells.  (Rust: overflow cells own their
/// content, no aOvflSpace is needed.)
///
/// If aOvflSpace is set to a null pointer, this function returns
/// SQLITE_NOMEM.
fn balance_nonroot(pParent: &DbPage, iParentIdx: i32, isRoot: i32, bBulk: i32) -> i32 {
    let pBt = mp(pParent).pBt.upgrade().expect("page without BtShared");
    let mut nMaxCells: i32 = 0; /* Allocated size of apCell, szCell, aFrom. */
    let mut nNew: usize = 0; /* Number of pages in apNew[] */
    let nOld: usize; /* Number of pages in apOld[] */
    let mut i: i32;
    let mut k: i32;
    let nxDiv: i32; /* Next divider slot in pParent->aCell[] */
    let mut rc = SQLITE_OK; /* The return code */
    let leafCorrection: u16; /* 4 if pPage is a leaf.  0 if not */
    let leafData: i32; /* True if pPage is a leaf of a LEAFDATA tree */
    let usableSpace: i32; /* Bytes in pPage beyond the header */
    let pageFlags: i32; /* Value of pPage->aData[0] */
    let mut iSpace1: usize = 0; /* First unused byte of aSpace1[] */
    let mut apOld: [Option<DbPage>; NB] = Default::default(); /* pPage and up to two siblings */
    let mut apNew: [Option<DbPage>; NB + 2] = Default::default(); /* pPage and up to NB siblings after balancing */
    let pRight: usize; /* Location in parent of right-sibling pointer */
    let mut apDiv: [Vec<u8>; NB - 1] = Default::default(); /* Divider cells in pParent */
    let mut cntNew = [0i32; NB + 3]; /* Index in b.paCell[] of cell after i-th page */
    let mut cntOld = [0i32; NB + 3]; /* Old index in b.apCell[] */
    let mut szNew = [0i32; NB + 3]; /* Combined size of cells placed on i-th page */
    let mut pgno: Pgno; /* Temp var to store a page number in */
    let mut abDone = [0u8; NB + 2]; /* True after i'th new page is populated */
    let mut aPgno = [0 as Pgno; NB + 2]; /* Page numbers of new pages before shuffling */
    let (usableSize, pageSize, autoVacuum, btsFlags) = {
        let b = pBt.borrow();
        (b.usableSize, b.pageSize, b.autoVacuum, b.btsFlags as i32)
    };
    let parentPgno = pgnoOf(pParent);

    let mut b: CellArray;

    'balance_cleanup: {
        /* At this point pParent may have at most one overflow cell. And if
         ** this overflow cell is present, it must be the cell with
         ** index iParentIdx. This scenario comes about when this function
         ** is called (indirectly) from sqlite3BtreeDelete().
         */
        let (pNovfl, pNcell, pHdr) = {
            let m = mp(pParent);
            debug_assert!(m.nOverflow == 0 || m.nOverflow == 1);
            debug_assert!(m.nOverflow == 0 || m.aiOvfl[0] as i32 == iParentIdx);
            debug_assert!(m.nFree >= 0);
            (m.nOverflow as i32, m.nCell as i32, m.hdrOffset as usize)
        };

        /* Find the sibling pages to balance. Also locate the cells in pParent
         ** that divide the siblings. An attempt is made to find NN siblings on
         ** either side of pPage. More siblings are taken from one side, however,
         ** if there are fewer than NN siblings on the other side. If pParent
         ** has NB or fewer children then all children of pParent are taken.
         **
         ** This loop also drops the divider cells from the parent page. This
         ** way, the remainder of the function does not have to deal with any
         ** overflow cells in the parent page, since if any existed they will
         ** have already been removed.
         */
        i = pNovfl + pNcell;
        if i < 2 {
            nxDiv = 0;
        } else {
            debug_assert!(bBulk == 0 || bBulk == 1);
            if iParentIdx == 0 {
                nxDiv = 0;
            } else if iParentIdx == i {
                nxDiv = i - 2 + bBulk;
            } else {
                nxDiv = iParentIdx - 1;
            }
            i = 2 - bBulk;
        }
        nOld = (i + 1) as usize;
        {
            let g = pParent.borrow();
            if (i + nxDiv - pNovfl) == pNcell {
                pRight = pHdr + 8;
            } else {
                pRight = findCell(&g.pExtra, &g.aData, (i + nxDiv - pNovfl) as usize);
            }
            pgno = get4byte(&g.aData, pRight);
        }
        loop {
            let iu = i as usize;
            if rc == SQLITE_OK {
                rc = getAndInitPage(&pBt, pgno, &mut apOld[iu], 0);
            }
            if rc != 0 {
                for x in apOld.iter_mut().take(iu + 1) {
                    *x = None;
                }
                break 'balance_cleanup;
            }
            let po = apOld[iu].clone().unwrap();
            if mp(&po).nFree < 0 {
                rc = computeFreeSpace(&po);
                if rc != 0 {
                    for x in apOld.iter_mut().take(iu) {
                        *x = None;
                    }
                    break 'balance_cleanup;
                }
            }
            nMaxCells += mp(&po).nCell as i32 + 4;
            if i == 0 {
                break;
            }
            i -= 1;
            let iu = i as usize;

            let (curNovfl, aiOvfl0) = {
                let m = mp(pParent);
                (m.nOverflow as i32, m.aiOvfl[0] as i32)
            };
            if curNovfl != 0 && i + nxDiv == aiOvfl0 {
                let mut m = mpm(pParent);
                apDiv[iu] = m.apOvfl[0].clone();
                pgno = get4byte(&apDiv[iu], 0);
                szNew[iu] = (m.xCellSize)(&m, &apDiv[iu], 0) as i32;
                m.nOverflow = 0;
            } else {
                let idx = (i + nxDiv - curNovfl) as usize;
                let mut g = pParent.borrow_mut();
                let off = findCell(&g.pExtra, &g.aData, idx);
                pgno = get4byte(&g.aData, off);
                let sz = (g.pExtra.xCellSize)(&g.pExtra, &g.aData, off) as i32;
                szNew[iu] = sz;

                /* Drop the cell from the parent page. apDiv[i] still points to
                 ** the cell within the parent, even though it has been dropped.
                 ** This is safe because dropping a cell only overwrites the first
                 ** four bytes of it, and this function does not need the first
                 ** four bytes of the divider cell. So the pointer is safe to use
                 ** later on.
                 **
                 ** But not if we are in secure-delete mode. In secure-delete mode,
                 ** the dropCell() routine will overwrite the entire cell with zeroes.
                 ** In this case, temporarily copy the cell into the aOvflSpace[]
                 ** buffer. It will be copied out again as soon as the aSpace[] buffer
                 ** is allocated.
                 **
                 ** Rust: the divider cell is always copied before it is dropped.  */
                let mut v = Vec::new();
                copyOut(&g.aData, off, sz as usize, &mut v);
                apDiv[iu] = v;
                let _ = btsFlags;
                dropCell(&mut g, idx, sz, &mut rc);
            }
        }

        /* Make nMaxCells a multiple of 4 in order to preserve 8-byte
         ** alignment */
        nMaxCells = (nMaxCells + 3) & !3;

        /*
         ** Allocate space for memory structures
         */
        let po0 = apOld[0].clone().unwrap();
        b = CellArray::new(mp(&po0).lite());
        b.apCell.reserve(nMaxCells as usize);
        b.szCell.reserve(nMaxCells as usize);
        b.space1 = vec![0u8; pageSize as usize];
        for x in apOld.iter().take(nOld) {
            b.pages.push(x.clone().unwrap());
        }

        /*
         ** Load pointers to all cells on sibling pages and the divider cells
         ** into the local b.apCell[] array.  Make copies of the divider cells
         ** into space obtained from aSpace1[]. The divider cells have already
         ** been removed from pParent.
         **
         ** If the siblings are on leaf pages, then the child pointers of the
         ** divider cells are stripped from the cells before they are copied
         ** into aSpace1[].  In this way, all cells in b.apCell[] are without
         ** child pointers.  If siblings are not leaves, then all cell in
         ** b.apCell[] include child pointers.  Either way, all cells in b.apCell[]
         ** are alike.
         **
         ** leafCorrection:  4 if pPage is a leaf.  0 if pPage is not a leaf.
         **       leafData:  1 if pPage holds key+data and pParent holds only keys.
         */
        leafCorrection = b.pRef.leaf as u16 * 4;
        leafData = b.pRef.intKeyLeaf as i32;
        let flag0 = po0.borrow().aData[0];
        for iu in 0..nOld {
            let pOld = apOld[iu].clone().unwrap();
            let g = pOld.borrow();
            let m = &g.pExtra;
            let mut limit = m.nCell as usize;
            let maskPage = m.maskPage as usize;
            let mut piCell = m.cellOffset as usize;

            /* Verify that all sibling pages are of the same "type" (table-leaf,
             ** table-interior, index-leaf, or index-interior).
             */
            if g.aData[0] != flag0 {
                rc = SQLITE_CORRUPT_PAGE(m);
                break 'balance_cleanup;
            }

            /* Load b.apCell[] with pointers to all cells in pOld.  If pOld
             ** contains overflow cells, include them in the b.apCell[] array
             ** in the correct spot.
             **
             ** Note that when there are multiple overflow cells, it is always the
             ** case that they are sequential and adjacent.  This invariant arises
             ** because multiple overflows can only occurs when inserting divider
             ** cells into a parent on a prior balance, and divider cells are always
             ** adjacent and are inserted in order.  There is an assert() tagged
             ** with "NOTE 1" in the overflow cell insertion loop to prove this
             ** invariant.
             **
             ** This must be done in advance.  Once the balance starts, the cell
             ** offset section of the btree page will be overwritten and we will no
             ** long be able to find the cells if a pointer to each cell is not saved
             ** first.
             */
            if m.nOverflow > 0 {
                if limit < m.aiOvfl[0] as usize {
                    rc = SQLITE_CORRUPT_PAGE(m);
                    break 'balance_cleanup;
                }
                limit = m.aiOvfl[0] as usize;
                for _ in 0..limit {
                    b.apCell.push(CellRef {
                        src: CellSrc::Page(iu),
                        off: maskPage & get2byteAligned(&g.aData, piCell),
                    });
                    b.szCell.push(0);
                    piCell += 2;
                    b.nCell += 1;
                }
                for kk in 0..m.nOverflow as usize {
                    debug_assert!(kk == 0 || m.aiOvfl[kk - 1] + 1 == m.aiOvfl[kk]); /* NOTE 1 */
                    b.ovfl.push(m.apOvfl[kk].clone());
                    b.apCell.push(CellRef {
                        src: CellSrc::Ovfl(b.ovfl.len() - 1),
                        off: 0,
                    });
                    b.szCell.push(0);
                    b.nCell += 1;
                }
            }
            let piEnd = m.cellOffset as usize + 2 * m.nCell as usize;
            while piCell < piEnd {
                b.apCell.push(CellRef {
                    src: CellSrc::Page(iu),
                    off: maskPage & get2byteAligned(&g.aData, piCell),
                });
                b.szCell.push(0);
                piCell += 2;
                b.nCell += 1;
            }

            cntOld[iu] = b.nCell;
            if iu < nOld - 1 && leafData == 0 {
                let sz = szNew[iu] as usize;
                let pTemp = iSpace1;
                iSpace1 += sz;
                if iSpace1 > b.space1.len() {
                    rc = SQLITE_CORRUPT_BKPT();
                    break 'balance_cleanup;
                }
                let n = std::cmp::min(sz, apDiv[iu].len());
                b.space1[pTemp..pTemp + n].copy_from_slice(&apDiv[iu][..n]);
                b.apCell.push(CellRef {
                    src: CellSrc::Space1,
                    off: pTemp + leafCorrection as usize,
                });
                b.szCell.push((sz as u16).wrapping_sub(leafCorrection));
                let last = b.szCell.len() - 1;
                if m.leaf == 0 {
                    debug_assert!(leafCorrection == 0);
                    debug_assert!(m.hdrOffset == 0);
                    /* The right pointer of the child page pOld becomes the left
                     ** pointer of the divider cell */
                    b.space1[pTemp..pTemp + 4].copy_from_slice(&g.aData[8..12]);
                } else {
                    debug_assert!(leafCorrection == 4);
                    while b.szCell[last] < 4 {
                        /* Do not allow any cells smaller than 4 bytes. If a smaller cell
                         ** does exist, pad it with 0x00 bytes. */
                        if iSpace1 >= b.space1.len() {
                            rc = SQLITE_CORRUPT_BKPT();
                            break 'balance_cleanup;
                        }
                        b.space1[iSpace1] = 0x00;
                        iSpace1 += 1;
                        b.szCell[last] += 1;
                    }
                }
                b.nCell += 1;
            }
        }

        /*
         ** Figure out the number of pages needed to hold all b.nCell cells.
         ** Store this number in "k".  Also compute szNew[] which is the total
         ** size of all cells on the i-th page and cntNew[] which is the index
         ** in b.apCell[] of the cell that divides page i from page i+1.
         ** cntNew[k] should equal b.nCell.
         **
         ** Values computed by this block:
         **
         **           k: The total number of sibling pages
         **    szNew[i]: Spaced used on the i-th sibling page.
         **   cntNew[i]: Index in b.apCell[] and b.szCell[] for the first cell to
         **              the right of the i-th sibling page.
         ** usableSpace: Number of bytes of space available on each sibling.
         **
         */
        usableSpace = usableSize as i32 - 12 + leafCorrection as i32;
        for iu in 0..nOld {
            let p = apOld[iu].clone().unwrap();
            let m = mp(&p);
            debug_assert!(m.nFree >= 0);
            szNew[iu] = usableSpace - m.nFree;
            for j in 0..m.nOverflow as usize {
                szNew[iu] += 2 + (m.xCellSize)(&m, &m.apOvfl[j], 0) as i32;
            }
            cntNew[iu] = cntOld[iu];
        }
        k = nOld as i32;
        i = 0;
        while i < k {
            let iu = i as usize;
            let mut sz: i32;
            while szNew[iu] > usableSpace {
                if i + 1 >= k {
                    k = i + 2;
                    if k > (NB + 2) as i32 {
                        rc = SQLITE_CORRUPT_BKPT();
                        break 'balance_cleanup;
                    }
                    szNew[k as usize - 1] = 0;
                    cntNew[k as usize - 1] = b.nCell;
                }
                if cntNew[iu] < 1 {
                    rc = SQLITE_CORRUPT_BKPT();
                    break 'balance_cleanup;
                }
                sz = 2 + cachedCellSize(&mut b, (cntNew[iu] - 1) as usize) as i32;
                szNew[iu] -= sz;
                if leafData == 0 {
                    if cntNew[iu] < b.nCell {
                        sz = 2 + cachedCellSize(&mut b, cntNew[iu] as usize) as i32;
                    } else {
                        sz = 0;
                    }
                }
                szNew[iu + 1] += sz;
                cntNew[iu] -= 1;
            }
            while cntNew[iu] < b.nCell {
                sz = 2 + cachedCellSize(&mut b, cntNew[iu] as usize) as i32;
                if szNew[iu] + sz > usableSpace {
                    break;
                }
                szNew[iu] += sz;
                cntNew[iu] += 1;
                if leafData == 0 {
                    if cntNew[iu] < b.nCell {
                        sz = 2 + cachedCellSize(&mut b, cntNew[iu] as usize) as i32;
                    } else {
                        sz = 0;
                    }
                }
                szNew[iu + 1] -= sz;
            }
            if cntNew[iu] >= b.nCell {
                k = i + 1;
            } else if cntNew[iu] <= (if i > 0 { cntNew[iu - 1] } else { 0 }) {
                rc = SQLITE_CORRUPT_BKPT();
                break 'balance_cleanup;
            }
            i += 1;
        }

        /*
         ** The packing computed by the previous block is biased toward the siblings
         ** on the left side (siblings with smaller keys). The left siblings are
         ** always nearly full, while the right-most sibling might be nearly empty.
         ** The next block of code attempts to adjust the packing of siblings to
         ** get a better balance.
         **
         ** This adjustment is more than an optimization.  The packing above might
         ** be so out of balance as to be illegal.  For example, the right-most
         ** sibling might be completely empty.  This adjustment is not optional.
         */
        i = k - 1;
        while i > 0 {
            let iu = i as usize;
            let mut szRight = szNew[iu]; /* Size of sibling on the right */
            let mut szLeft = szNew[iu - 1]; /* Size of sibling on the left */
            let mut r = cntNew[iu - 1] - 1; /* Index of right-most cell in left sibling */
            let mut d = r + 1 - leafData; /* Index of first cell to the left of right sibling */
            if d < 0 || d >= b.nCell || r < 0 {
                rc = SQLITE_CORRUPT_BKPT();
                break 'balance_cleanup;
            }
            let _ = cachedCellSize(&mut b, d as usize);
            loop {
                let szR = cachedCellSize(&mut b, r as usize) as i32;
                let szD = b.szCell[d as usize] as i32;
                if szRight != 0
                    && (bBulk != 0
                        || szRight + szD + 2 > szLeft - (szR + if i == k - 1 { 0 } else { 2 }))
                {
                    break;
                }
                szRight += szD + 2;
                szLeft -= szR + 2;
                cntNew[iu - 1] = r;
                r -= 1;
                d -= 1;
                if r < 0 {
                    break;
                }
                if d < 0 {
                    rc = SQLITE_CORRUPT_BKPT();
                    break 'balance_cleanup;
                }
                if b.szCell[d as usize] == 0 {
                    /* C reads b.szCell[d] directly: it has always been computed
                     ** by the previous iteration's cachedCellSize(&b, r). */
                    let _ = cachedCellSize(&mut b, d as usize);
                }
            }
            szNew[iu] = szRight;
            szNew[iu - 1] = szLeft;
            if cntNew[iu - 1] <= (if i > 1 { cntNew[iu - 2] } else { 0 }) {
                rc = SQLITE_CORRUPT_BKPT();
                break 'balance_cleanup;
            }
            i -= 1;
        }

        /* Sanity check:  For a non-corrupt database file one of the following
         ** must be true:
         **    (1) We found one or more cells (cntNew[0])>0), or
         **    (2) pPage is a virtual root page.  A virtual root page is when
         **        the real root page is page 1 and we are the only child of
         **        that page.
         */

        /*
         ** Allocate k new pages.  Reuse old pages where possible.
         */
        pageFlags = flag0 as i32;
        for iu in 0..k as usize {
            if iu < nOld {
                let pNew = apOld[iu].take().unwrap();
                apNew[iu] = Some(pNew.clone());
                rc = sqlite3PagerWrite(&pNew);
                nNew += 1;
                let expect = 1 + (iu as i32 == (iParentIdx - nxDiv)) as i32;
                if sqlite3PagerPageRefcount(&pNew) != expect && rc == SQLITE_OK {
                    rc = SQLITE_CORRUPT_BKPT();
                }
                if rc != 0 {
                    break 'balance_cleanup;
                }
            } else {
                debug_assert!(iu > 0);
                let mut pNew = None;
                let near = if bBulk != 0 { 1 } else { pgno };
                rc = allocateBtreePage(&pBt, &mut pNew, &mut pgno, near, 0);
                if rc != 0 {
                    break 'balance_cleanup;
                }
                let pNew = pNew.unwrap();
                zeroPage(&mut pNew.borrow_mut(), pageFlags);
                apNew[iu] = Some(pNew.clone());
                nNew += 1;
                cntOld[iu] = b.nCell;

                /* Set the pointer-map entry for the new sibling page. */
                if autoVacuum != 0 {
                    ptrmapPut(&pBt, pgnoOf(&pNew), PTRMAP_BTREE as u8, parentPgno, &mut rc);
                    if rc != SQLITE_OK {
                        break 'balance_cleanup;
                    }
                }
            }
        }

        /*
         ** Reassign page numbers so that the new pages are in ascending order.
         ** This helps to keep entries in the disk file in order so that a scan
         ** of the table is closer to a linear scan through the file. That in turn
         ** helps the operating system to deliver pages from the disk more rapidly.
         **
         ** An O(N*N) sort algorithm is used, but since N is never more than NB+2
         ** (5), that is not a performance concern.
         **
         ** When NB==3, this one optimization makes the database about 25% faster
         ** for large insertions and deletions.
         */
        for iu in 0..nNew {
            aPgno[iu] = pgnoOf(apNew[iu].as_ref().unwrap());
        }
        for iu in 0..nNew.saturating_sub(1) {
            let mut iB = iu;
            for j in iu + 1..nNew {
                if pgnoOf(apNew[j].as_ref().unwrap()) < pgnoOf(apNew[iB].as_ref().unwrap()) {
                    iB = j;
                }
            }

            /* If apNew[i] has a page number that is bigger than any of the
             ** subsequence apNew[i] entries, then swap apNew[i] with the subsequent
             ** entry that has the smallest page number (which we know to be
             ** entry apNew[iB]).
             */
            if iB != iu {
                let pa = apNew[iu].clone().unwrap();
                let pb = apNew[iB].clone().unwrap();
                let pgnoA = pgnoOf(&pa);
                let pgnoB = pgnoOf(&pb);
                let pgnoTemp = PENDING_BYTE_PAGE(pageSize);
                let fgA = pa.borrow().flags;
                let fgB = pb.borrow().flags;
                sqlite3PagerRekey(&pa, pgnoTemp, fgB);
                sqlite3PagerRekey(&pb, pgnoA, fgA);
                sqlite3PagerRekey(&pa, pgnoB, fgB);
                mpm(&pa).pgno = pgnoB;
                mpm(&pb).pgno = pgnoA;
            }
        }

        let lastNew = apNew[nNew - 1].clone().unwrap();
        put4byte(&mut pParent.borrow_mut().aData, pRight, pgnoOf(&lastNew));

        /* If the sibling pages are not leaves, ensure that the right-child pointer
         ** of the right-most new sibling page is set to the value that was
         ** originally in the same field of the right-most old sibling page. */
        if (pageFlags & PTF_LEAF) == 0 && nOld != nNew {
            let pOld = if nNew > nOld {
                apNew[nOld - 1].clone().unwrap()
            } else {
                apOld[nOld - 1].clone().unwrap()
            };
            let v: [u8; 4] = pOld.borrow().aData[8..12].try_into().unwrap();
            if !Rc::ptr_eq(&pOld, &lastNew) {
                lastNew.borrow_mut().aData[8..12].copy_from_slice(&v);
            }
        }

        /* Make any required updates to pointer map entries associated with
         ** cells stored on sibling pages following the balance operation. Pointer
         ** map entries associated with divider cells are set by the insertCell()
         ** routine. The associated pointer map entries are:
         **
         **   a) if the cell contains a reference to an overflow chain, the
         **      entry associated with the first page in the overflow chain, and
         **
         **   b) if the sibling pages are not leaves, the child page associated
         **      with the cell.
         **
         ** If the sibling pages are not leaves, then the pointer map entry
         ** associated with the right-child of each sibling may also need to be
         ** updated. This happens below, after the sibling pages have been
         ** populated, not here.
         */
        if autoVacuum != 0 {
            let mut pNew = apNew[0].clone().unwrap();
            let mut pOld = pNew.clone();
            let mut cntOldNext = {
                let m = mp(&pNew);
                m.nCell as i32 + m.nOverflow as i32
            };
            let mut iNew = 0usize;
            let mut iOld = 0usize;

            let mut ii = 0i32;
            while ii < b.nCell {
                let iu = ii as usize;
                while ii == cntOldNext {
                    iOld += 1;
                    debug_assert!(iOld < nNew || iOld < nOld);
                    debug_assert!(iOld < NB);
                    pOld = if iOld < nNew {
                        apNew[iOld].clone().unwrap()
                    } else {
                        apOld[iOld].clone().unwrap()
                    };
                    let m = mp(&pOld);
                    cntOldNext += m.nCell as i32 + m.nOverflow as i32 + (leafData == 0) as i32;
                }
                if ii == cntNew[iNew] {
                    iNew += 1;
                    pNew = apNew[iNew].clone().unwrap();
                    if leafData == 0 {
                        ii += 1;
                        continue;
                    }
                }

                /* Cell pCell is destined for new sibling page pNew. Originally, it
                 ** was either part of sibling page iOld (possibly an overflow cell),
                 ** or else the divider cell to the left of sibling page iOld. So,
                 ** if sibling page iOld had the same page number as pNew, and if
                 ** pCell really was a part of sibling page iOld (not a divider or
                 ** overflow cell), we can skip updating the pointer map entries.  */
                let inOld = b.apCell[iu].src == CellSrc::Page(iOld)
                    && b.apCell[iu].off < mp(&pOld).aDataEnd();
                let newPgno = pgnoOf(&pNew);
                if iOld >= nNew || newPgno != aPgno[iOld] || !inOld {
                    if leafCorrection == 0 {
                        let mut v = Vec::new();
                        b.copyCell(iu, 0, 4, &mut v, None);
                        ptrmapPut(&pBt, get4byte(&v, 0), PTRMAP_BTREE as u8, newPgno, &mut rc);
                    }
                    let newLite = mp(&pNew).lite();
                    if cachedCellSize(&mut b, iu) > newLite.minLocal {
                        let sz = b.szCell[iu] as usize;
                        let mut v = Vec::new();
                        b.copyCell(iu, 0, sz, &mut v, None);
                        let srcEnd = if inOld {
                            Some(mp(&pOld).aDataEnd() - b.apCell[iu].off)
                        } else {
                            None
                        };
                        ptrmapPutOvflPtr(&newLite, &v, 0, srcEnd, &mut rc);
                    }
                    if rc != 0 {
                        break 'balance_cleanup;
                    }
                }
                ii += 1;
            }
        }

        /* Insert new divider cells into pParent. */
        for iu in 0..nNew - 1 {
            let pNew = apNew[iu].clone().unwrap();
            let mut j = cntNew[iu] as usize;
            debug_assert!(j < nMaxCells as usize);
            let mut sz = b.szCell[j] as i32 + leafCorrection as i32;
            let mut pCell: Vec<u8> = Vec::new();
            let (newLeaf, newLite) = {
                let m = mp(&pNew);
                (m.leaf, m.lite())
            };
            let mut srcDelta: isize = 0;
            let mut fromCellArray = true;
            if newLeaf == 0 {
                b.copyCell(j, 0, sz as usize, &mut pCell, None);
                pNew.borrow_mut().aData[8..12].copy_from_slice(&pCell[0..4]);
            } else if leafData != 0 {
                /* If the tree is a leaf-data tree, and the siblings are leaves,
                 ** then there is no divider cell in b.apCell[]. Instead, the divider
                 ** cell consists of the integer key for the right-most cell of
                 ** the sibling-page assembled above only.
                 */
                let mut info = CellInfo::default();
                j -= 1;
                /* The two varints at the start of the cell are at most 18 bytes */
                let mut v = Vec::new();
                b.copyCell(j, 0, 24, &mut v, None);
                (newLite.xParseCell)(&newLite, &v, 0, &mut info);
                pCell = vec![0u8; 13];
                sz = 4 + putVarint(&mut pCell, 4, info.nKey as u64) as i32;
                pCell.truncate(sz as usize);
                fromCellArray = false;
            } else {
                srcDelta = -4;
                b.copyCell(j, -4, sz as usize, &mut pCell, None);
                /* Obscure case for non-leaf-data trees: If the cell at pCell was
                 ** previously stored on a leaf node, and its reported size was 4
                 ** bytes, then it may actually be smaller than this
                 ** (see btreeParseCellPtr(), 4 bytes is the minimum size of
                 ** any cell). But it is important to pass the correct size to
                 ** insertCell(), so reparse the cell now.
                 **
                 ** This can only happen for b-trees used to evaluate "IN (SELECT ...)"
                 ** and WITHOUT ROWID tables with exactly one column which is the
                 ** primary key.
                 */
                if b.szCell[j] == 4 {
                    debug_assert!(leafCorrection == 4);
                    let m = mp(pParent);
                    sz = (m.xCellSize)(&m, &pCell, 0) as i32;
                    drop(m);
                    if pCell.len() < sz as usize {
                        b.copyCell(j, -4, sz as usize, &mut pCell, None);
                    }
                }
            }
            debug_assert!(sz <= pBt.borrow().maxLocal as i32 + 23);
            if fromCellArray && b.straddlesEnd(j, srcDelta, sz as usize) {
                rc = SQLITE_CORRUPT_BKPT();
                break 'balance_cleanup;
            }
            rc = insertCell(pParent, nxDiv as usize + iu, &pCell, sz, pgnoOf(&pNew));
            if rc != SQLITE_OK {
                break 'balance_cleanup;
            }
        }

        /* Now update the actual sibling pages. The order in which they are updated
         ** is important, as this code needs to avoid disrupting any page from which
         ** cells may still to be read. In practice, this means:
         **
         **  (1) If cells are moving left (from apNew[iPg] to apNew[iPg-1])
         **      then it is not safe to update page apNew[iPg] until after
         **      the left-hand sibling apNew[iPg-1] has been updated.
         **
         **  (2) If cells are moving right (from apNew[iPg] to apNew[iPg+1])
         **      then it is not safe to update page apNew[iPg] until after
         **      the right-hand sibling apNew[iPg+1] has been updated.
         **
         ** If neither of the above apply, the page is safe to update.
         **
         ** The iPg value in the following loop starts at nNew-1 goes down
         ** to 0, then back up to nNew-1 again, thus making two passes over
         ** the pages.  On the initial downward pass, only condition (1) above
         ** needs to be tested because (2) will always be true from the previous
         ** step.  On the upward pass, both conditions are always true, so the
         ** upwards pass simply processes pages that were missed on the downward
         ** pass.
         */
        let mut ii = 1 - nNew as i32;
        while ii < nNew as i32 {
            let iPg = ii.unsigned_abs() as usize;
            debug_assert!(iPg < nNew);
            if abDone[iPg] != 0 {
                ii += 1;
                continue; /* Skip pages already processed */
            }
            if ii >= 0 /* On the upwards pass, or... */
                || cntOld[iPg - 1] >= cntNew[iPg - 1]
            /* Condition (1) is true */
            {
                let iNew: i32;
                let iOld: i32;
                let nNewCell: i32;

                if iPg == 0 {
                    iNew = 0;
                    iOld = 0;
                    nNewCell = cntNew[0];
                } else {
                    iOld = if iPg < nOld {
                        cntOld[iPg - 1] + (leafData == 0) as i32
                    } else {
                        b.nCell
                    };
                    iNew = cntNew[iPg - 1] + (leafData == 0) as i32;
                    nNewCell = cntNew[iPg] - iNew;
                }

                let pg = apNew[iPg].clone().unwrap();
                rc = editPage(&pg, iOld, iNew, nNewCell, &mut b);
                if rc != 0 {
                    break 'balance_cleanup;
                }
                abDone[iPg] += 1;
                mpm(&pg).nFree = usableSpace - szNew[iPg];
                debug_assert!(mp(&pg).nOverflow == 0);
                debug_assert!(mp(&pg).nCell as i32 == nNewCell);
            }
            ii += 1;
        }

        /* All pages have been processed exactly once */
        debug_assert!(abDone[..nNew].iter().all(|&x| x == 1));

        debug_assert!(nOld > 0);
        debug_assert!(nNew > 0);

        let new0 = apNew[0].clone().unwrap();
        let (pNcell2, pHdr2) = {
            let m = mp(pParent);
            (m.nCell, m.hdrOffset as i32)
        };
        if isRoot != 0 && pNcell2 == 0 && pHdr2 <= mp(&new0).nFree {
            /* The root page of the b-tree now contains no cells. The only sibling
             ** page is the right-child of the parent. Copy the contents of the
             ** child page into the parent, decreasing the overall height of the
             ** b-tree structure by one. This is described as the "balance-shallower"
             ** sub-algorithm in some documentation.
             **
             ** If this is an auto-vacuum database, the call to copyNodeContent()
             ** sets all pointer-map entries corresponding to database image pages
             ** for which the pointer is stored within the content being copied.
             **
             ** It is critical that the child page be defragmented before being
             ** copied into the parent, because if the parent is page 1 then it will
             ** by smaller than the child due to the database header, and so all the
             ** free space needs to be up front.
             */
            debug_assert!(nNew == 1);
            rc = defragmentPage(&mut new0.borrow_mut(), -1);
            copyNodeContent(&new0, pParent, &mut rc);
            freePage(&new0, &mut rc);
        } else if autoVacuum != 0 && leafCorrection == 0 {
            /* Fix the pointer map entries associated with the right-child of each
             ** sibling page. All other pointer map entries have already been taken
             ** care of.  */
            for iu in 0..nNew {
                let pg = apNew[iu].clone().unwrap();
                let key = get4byte(&pg.borrow().aData, 8);
                ptrmapPut(&pBt, key, PTRMAP_BTREE as u8, pgnoOf(&pg), &mut rc);
            }
        }

        debug_assert!(mp(pParent).isInit != 0);

        /* Free any old pages that were not reused as new pages.
         */
        for iu in nNew..nOld {
            let pg = apOld[iu].clone().unwrap();
            freePage(&pg, &mut rc);
        }
    }

    /*
     ** Cleanup before returning.
     */
    /* balance_cleanup: */
    for x in apOld.iter().take(nOld) {
        releasePage(x.as_ref());
    }
    for x in apNew.iter().take(nNew) {
        releasePage(x.as_ref());
    }

    rc
}

/// This function is called when the root page of a b-tree structure is
/// overfull (has one or more overflow pages).
///
/// A new child page is allocated and the contents of the current root
/// page, including overflow cells, are copied into the child. The root
/// page is then overwritten to make it an empty page with the right-child
/// pointer pointing to the new page.
///
/// Before returning, all pointer-map entries corresponding to pages
/// that the new child-page now contains pointers to are updated. The
/// entry corresponding to the new right-child pointer of the root
/// page is also updated.
///
/// If successful, *ppChild is set to contain a reference to the child
/// page and SQLITE_OK is returned. In this case the caller is required
/// to call releasePage() on *ppChild exactly once. If an error occurs,
/// an error code is returned and *ppChild is set to 0.
fn balance_deeper(pRoot: &DbPage, ppChild: &mut Option<DbPage>) -> i32 {
    let mut pChild: Option<DbPage> = None; /* Pointer to a new child page */
    let mut pgnoChild: Pgno = 0; /* Page number of the new child page */
    let pBt = mp(pRoot).pBt.upgrade().expect("page without BtShared");
    let rootPgno = pgnoOf(pRoot);

    debug_assert!(mp(pRoot).nOverflow > 0);

    /* Make pRoot, the root page of the b-tree, writable. Allocate a new
     ** page that will become the new right-child of pPage. Copy the contents
     ** of the node stored on pRoot into the new child page.
     */
    let mut rc = sqlite3PagerWrite(pRoot);
    if rc == SQLITE_OK {
        rc = allocateBtreePage(&pBt, &mut pChild, &mut pgnoChild, rootPgno, 0);
        if rc == SQLITE_OK {
            copyNodeContent(pRoot, pChild.as_ref().unwrap(), &mut rc);
        }
        if pBt.borrow().autoVacuum != 0 {
            ptrmapPut(&pBt, pgnoChild, PTRMAP_BTREE as u8, rootPgno, &mut rc);
        }
    }
    if rc != 0 {
        *ppChild = None;
        releasePage(pChild.as_ref());
        return rc;
    }
    let pChild = pChild.unwrap();

    /* Copy the overflow cells from pRoot to pChild */
    let childFlag;
    {
        let mut r = pRoot.borrow_mut();
        let mut c = pChild.borrow_mut();
        let n = r.pExtra.nOverflow as usize;
        for x in 0..n {
            c.pExtra.aiOvfl[x] = r.pExtra.aiOvfl[x];
            c.pExtra.apOvfl[x] = std::mem::take(&mut r.pExtra.apOvfl[x]);
        }
        c.pExtra.nOverflow = r.pExtra.nOverflow;
        childFlag = c.aData[0] as i32;
    }

    /* Zero the contents of pRoot. Then install pChild as the right-child. */
    {
        let mut r = pRoot.borrow_mut();
        zeroPage(&mut r, childFlag & !PTF_LEAF);
        let h = r.pExtra.hdrOffset as usize;
        put4byte(&mut r.aData, h + 8, pgnoChild);
    }

    *ppChild = Some(pChild);
    SQLITE_OK
}

/// Return SQLITE_CORRUPT if any cursor other than pCur is currently valid
/// on the same B-tree as pCur.
///
/// This can occur if a database is corrupt with two or more SQL tables
/// pointing to the same b-tree.  If an insert occurs on one SQL table
/// and causes a BEFORE TRIGGER to do a secondary insert on the other SQL
/// table linked to the same b-tree.  If the secondary insert causes a
/// rebalance, that can change content out from under the cursor on the
/// first SQL table, violating invariants on the first insert.
fn anotherValidCursor(pCur: &BtCursor) -> i32 {
    let me = pCur as *const BtCursor;
    for pOther in cursorList(pCur.bt()) {
        if !isSameCursor(&pOther, me) {
            let o = pOther.borrow();
            if o.eState == CURSOR_VALID as u8 {
                if let (Some(a), Some(b)) = (&o.pPage, &pCur.pPage) {
                    if Rc::ptr_eq(a, b) {
                        return SQLITE_CORRUPT_PAGE(&mp(b));
                    }
                }
            }
        }
    }
    SQLITE_OK
}

/// The page that pCur currently points to has just been modified in
/// some way. This function figures out if this modification means the
/// tree needs to be balanced, and if so calls the appropriate balancing
/// routine. Balancing routines are:
///
///   balance_quick()
///   balance_deeper()
///   balance_nonroot()
fn balance(pCur: &mut BtCursor) -> i32 {
    let mut rc = SQLITE_OK;
    let usableSize = pCur.bt().borrow().usableSize as i32;

    loop {
        let pPage = pCur.page().clone();

        if mp(&pPage).nFree < 0 && computeFreeSpace(&pPage) != 0 {
            break;
        }
        let (nOverflow, nFree) = {
            let m = mp(&pPage);
            (m.nOverflow, m.nFree)
        };
        if nOverflow == 0 && nFree * 3 <= usableSize * 2 {
            /* No rebalance required as long as:
             **   (1) There are no overflow cells
             **   (2) The amount of free space on the page is less than 2/3rds of
             **       the total usable space on the page. */
            break;
        } else if pCur.iPage == 0 {
            if nOverflow != 0 && {
                rc = anotherValidCursor(pCur);
                rc == SQLITE_OK
            } {
                /* The root page of the b-tree is overfull. In this case call the
                 ** balance_deeper() function to create a new child for the root-page
                 ** and copy the current contents of the root-page to it. The
                 ** next iteration of the do-loop will balance the child page.
                 */
                rc = balance_deeper(&pPage, &mut pCur.apPage[1]);
                if rc == SQLITE_OK {
                    pCur.iPage = 1;
                    pCur.ix = 0;
                    pCur.aiIdx[0] = 0;
                    pCur.apPage[0] = pCur.pPage.take();
                    pCur.pPage = pCur.apPage[1].take();
                    debug_assert!(mp(pCur.page()).nOverflow != 0);
                }
            } else {
                break;
            }
        } else if sqlite3PagerPageRefcount(&pPage) > 1 {
            /* The page being written is not a root page, and there is currently
             ** more than one reference to it. This only happens if the page is one
             ** of its own ancestor pages. Corruption. */
            rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
        } else {
            let iPage = pCur.iPage as usize;
            let pParent = pCur.apPage[iPage - 1].clone().unwrap();
            let iIdx = pCur.aiIdx[iPage - 1] as i32;

            rc = sqlite3PagerWrite(&pParent);
            if rc == SQLITE_OK && mp(&pParent).nFree < 0 {
                rc = computeFreeSpace(&pParent);
            }
            if rc == SQLITE_OK {
                let quick = {
                    let m = mp(&pPage);
                    let pm = mp(&pParent);
                    m.intKeyLeaf != 0
                        && m.nOverflow == 1
                        && m.aiOvfl[0] == m.nCell
                        && pm.pgno != 1
                        && pm.nCell as i32 == iIdx
                };
                if quick {
                    /* Call balance_quick() to create a new sibling of pPage on which
                     ** to store the overflow cell. balance_quick() inserts a new cell
                     ** into pParent, which may cause pParent overflow. If this
                     ** happens, the next iteration of the do-loop will balance pParent
                     ** use either balance_nonroot() or balance_deeper(). Until this
                     ** happens, the overflow cell is stored in the aBalanceQuickSpace[]
                     ** buffer.
                     */
                    rc = balance_quick(&pParent, &pPage);
                } else {
                    /* In this case, call balance_nonroot() to redistribute cells
                     ** between pPage and up to 2 of its sibling pages. This involves
                     ** modifying the contents of pParent, which may cause pParent to
                     ** become overfull or underfull. The next iteration of the do-loop
                     ** will balance the parent page to correct this.
                     */
                    rc = balance_nonroot(
                        &pParent,
                        iIdx,
                        (iPage == 1) as i32,
                        (pCur.hints as i32) & BTREE_BULKLOAD,
                    );
                }
            }

            mpm(&pPage).nOverflow = 0;

            /* The next iteration of the do-loop balances the parent page. */
            pCur.pPage = None;
            releasePage(Some(&pPage));
            pCur.iPage -= 1;
            debug_assert!(pCur.iPage >= 0);
            pCur.pPage = pCur.apPage[pCur.iPage as usize].take();
        }
        if rc != SQLITE_OK {
            break;
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Insert / overwrite
// ---------------------------------------------------------------------------

/// Overwrite content from pX into pDest.  Only do the write if the
/// content is different from what is already there.
fn btreeOverwriteContent(
    pPage: &DbPage,
    pDest: usize,
    pX: &BtreePayload,
    iOffset: i32,
    iAmt: i32,
) -> i32 {
    let mut iAmt = iAmt;
    let nData = pX.nData - iOffset;
    if nData <= 0 {
        /* Overwriting with zeros */
        let i = {
            let g = pPage.borrow();
            let mut i = 0i32;
            while i < iAmt && g.aData[pDest + i as usize] == 0 {
                i += 1;
            }
            i
        };
        if i < iAmt {
            let rc = sqlite3PagerWrite(pPage);
            if rc != 0 {
                return rc;
            }
            for b in &mut pPage.borrow_mut().aData[pDest + i as usize..pDest + iAmt as usize] {
                *b = 0;
            }
        }
    } else {
        if nData < iAmt {
            /* Mixed read data and zeros at the end.  Make a recursive call
             ** to write the zeros then fall through to write the real data */
            let rc = btreeOverwriteContent(
                pPage,
                pDest + nData as usize,
                pX,
                iOffset + nData,
                iAmt - nData,
            );
            if rc != 0 {
                return rc;
            }
            iAmt = nData;
        }
        let src = &pX.pData.unwrap_or(&[])[iOffset as usize..(iOffset + iAmt) as usize];
        let differs = pPage.borrow().aData[pDest..pDest + iAmt as usize] != *src;
        if differs {
            let rc = sqlite3PagerWrite(pPage);
            if rc != 0 {
                return rc;
            }
            /* In a corrupt database, it is possible for the source and destination
             ** buffers to overlap.  This is harmless since the database is already
             ** corrupt but it does cause valgrind and ASAN warnings.  So use
             ** memmove(). */
            pPage.borrow_mut().aData[pDest..pDest + iAmt as usize].copy_from_slice(src);
        }
    }
    SQLITE_OK
}

/// Overwrite the cell that cursor pCur is pointing to with fresh content
/// contained in pX.  In this variant, pCur is pointing to an overflow
/// cell.
fn btreeOverwriteOverflowCell(pCur: &mut BtCursor, pX: &BtreePayload) -> i32 {
    let nTotal = pX.nData + pX.nZero; /* Total bytes of to write */
    let pPage0 = pCur.page().clone(); /* Page being written */

    debug_assert!((pCur.info.nLocal as i32) < nTotal); /* pCur is an overflow cell */

    /* Overwrite the local portion first */
    let mut rc = btreeOverwriteContent(&pPage0, pCur.info.pPayload, pX, 0, pCur.info.nLocal as i32);
    if rc != 0 {
        return rc;
    }

    /* Now overwrite the overflow pages */
    let mut iOffset = pCur.info.nLocal as i32;
    debug_assert!(nTotal >= 0);
    debug_assert!(iOffset >= 0);
    let mut ovflPgno = get4byte(
        &pPage0.borrow().aData,
        pCur.info.pPayload + iOffset as usize,
    );
    let pBt = pCur.bt().clone();
    let mut ovflPageSize = pBt.borrow().usableSize - 4;
    loop {
        let mut pPage = None;
        rc = btreeGetPage(&pBt, ovflPgno, &mut pPage, 0);
        if rc != 0 {
            return rc;
        }
        let pPage = pPage.unwrap();
        if sqlite3PagerPageRefcount(&pPage) != 1 || mp(&pPage).isInit != 0 {
            rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
        } else {
            if (iOffset as u32 + ovflPageSize) < nTotal as u32 {
                ovflPgno = get4byte(&pPage.borrow().aData, 0);
            } else {
                ovflPageSize = (nTotal - iOffset) as u32;
            }
            rc = btreeOverwriteContent(&pPage, 4, pX, iOffset, ovflPageSize as i32);
        }
        sqlite3PagerUnref(&pPage);
        if rc != 0 {
            return rc;
        }
        iOffset += ovflPageSize as i32;
        if !(iOffset < nTotal) {
            break;
        }
    }
    SQLITE_OK
}

/// Overwrite the cell that cursor pCur is pointing to with fresh content
/// contained in pX.
fn btreeOverwriteCell(pCur: &mut BtCursor, pX: &BtreePayload) -> i32 {
    let nTotal = pX.nData + pX.nZero; /* Total bytes of to write */
    let pPage = pCur.page().clone(); /* Page being written */

    {
        let m = mp(&pPage);
        if pCur.info.pPayload + pCur.info.nLocal as usize > m.aDataEnd()
            || pCur.info.pPayload < m.cellOffset as usize
        {
            return SQLITE_CORRUPT_PAGE(&m);
        }
    }
    if pCur.info.nLocal as i32 == nTotal {
        /* The entire cell is local */
        btreeOverwriteContent(&pPage, pCur.info.pPayload, pX, 0, pCur.info.nLocal as i32)
    } else {
        /* The cell contains overflow content */
        btreeOverwriteOverflowCell(pCur, pX)
    }
}

/// C: `int sqlite3BtreeInsert( BtCursor *pCur, const BtreePayload *pX, int flags, int seekResult )`
///
/// Insert a new record into the BTree.  The content of the new record
/// is described by the pX object.  The pCur cursor is used only to
/// define what table the record should be inserted into, and is left
/// pointing at a random location.
///
/// For a table btree (used for rowid tables), only the pX.nKey value of
/// the key is used. The pX.pKey value must be NULL.  The pX.nKey is the
/// rowid or INTEGER PRIMARY KEY of the row.  The pX.nData,pData,nZero fields
/// hold the content of the row.
///
/// For an index btree (used for indexes and WITHOUT ROWID tables), the
/// key is an arbitrary byte sequence stored in pX.pKey,nKey.  The
/// pX.pData,nData,nZero fields must be zero.
///
/// If the seekResult parameter is non-zero, then a successful call to
/// sqlite3BtreeIndexMoveto() to seek cursor pCur to (pKey,nKey) has already
/// been performed.  In other words, if seekResult!=0 then the cursor
/// is currently pointing to a cell that will be adjacent to the cell
/// to be inserted.  If seekResult<0 then pCur points to a cell that is
/// smaller then (pKey,nKey).  If seekResult>0 then pCur points to a cell
/// that is larger than (pKey,nKey).
///
/// If seekResult==0, that means pCur is pointing at some unknown location.
/// In that case, this routine must seek the cursor to the correct insertion
/// point for (pKey,nKey) before doing the insertion.  For index btrees,
/// if pX->nMem is non-zero, then pX->aMem contains pointers to the unpacked
/// key values and pX->aMem can be used instead of pX->pKey to avoid having
/// to decode the key.
pub fn sqlite3BtreeInsert(
    pCur: &BtCursorP,
    pX: &crate::btree::BtreePayload,
    flags: i32,
    seekResult: i32,
) -> i32 {
    btreeInsert(&mut pCur.borrow_mut(), pX, flags, seekResult)
}

fn btreeInsert(pCur: &mut BtCursor, pX: &BtreePayload, flags: i32, seekResult: i32) -> i32 {
    let mut rc;
    let mut loc = seekResult; /* -1: before desired location  +1: after */
    let p = pCur.btree().clone();
    let pBt = pCur.bt().clone();

    debug_assert!((flags & (BTREE_SAVEPOSITION | BTREE_APPEND | BTREE_PREFORMAT)) == flags);
    debug_assert!((flags & BTREE_PREFORMAT) == 0 || seekResult != 0 || pCur.pKeyInfo.is_none());

    /* Save the positions of any other cursors open on this table.
     **
     ** In some cases, the call to btreeMoveto() below is a no-op. For
     ** example, when inserting data into a table with auto-generated integer
     ** keys, the VDBE layer invokes sqlite3BtreeLast() to figure out the
     ** integer key to use. It then calls this function to actually insert the
     ** data into the intkey B-Tree. In this case btreeMoveto() recognizes
     ** that the cursor is already where it needs to be and returns without
     ** doing any work. To avoid thwarting these optimizations, it is important
     ** not to clear the cursor here.
     */
    if pCur.curFlags as i32 & BTCF_Multiple != 0 {
        let root = pCur.pgnoRoot;
        rc = saveAllCursors(&pBt, root, Some(&mut *pCur));
        if rc != 0 {
            return rc;
        }
        if loc != 0 && pCur.iPage < 0 {
            /* This can only happen if the schema is corrupt such that there is more
             ** than one table or index with the same root page as used by the cursor.
             ** Which can only happen if the SQLITE_NoSchemaError flag was set when
             ** the schema was loaded. This cannot be asserted though, as a user might
             ** set the flag, load the schema, and then unset the flag.  */
            return SQLITE_CORRUPT_PGNO(pCur.pgnoRoot);
        }
    }

    /* Ensure that the cursor is not in the CURSOR_FAULT state and that it
     ** points to a valid cell.
     */
    if pCur.eState >= CURSOR_REQUIRESEEK as u8 {
        rc = moveToRoot(pCur);
        if rc != 0 && rc != SQLITE_EMPTY {
            return rc;
        }
    }

    debug_assert!(
        (pCur.curFlags as i32 & BTCF_WriteFlag) != 0
            && pBt.borrow().inTransaction == TRANS_WRITE as u8
            && (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0
    );

    /* Assert that the caller has been consistent. If this cursor was opened
     ** expecting an index b-tree, then the caller should be inserting blob
     ** keys with no associated data. If the cursor was opened expecting an
     ** intkey table, the caller should be inserting integer keys with a
     ** blob of associated data.  */
    debug_assert!((flags & BTREE_PREFORMAT) != 0 || pX.pKey.is_none() == pCur.pKeyInfo.is_none());

    if pCur.pKeyInfo.is_none() {
        debug_assert!(pX.pKey.is_none());
        /* If this is an insert into a table b-tree, invalidate any incrblob
         ** cursors open on the row being replaced */
        if p.borrow().hasIncrblobCur != 0 {
            let root = pCur.pgnoRoot;
            invalidateIncrblobCursors(&p, root, pX.nKey, 0, Some(&mut *pCur));
        }

        /* On the other hand, BTREE_SAVEPOSITION==0 does not imply
         ** that the cursor is not pointing to a row to be overwritten.
         ** So do a complete check.
         */
        if (pCur.curFlags as i32 & BTCF_ValidNKey) != 0 && pX.nKey == pCur.info.nKey {
            /* The cursor is pointing to the entry that is to be
             ** overwritten */
            debug_assert!(pX.nData >= 0 && pX.nZero >= 0);
            if pCur.info.nSize != 0 && pCur.info.nPayload == (pX.nData + pX.nZero) as u32 {
                /* New entry is the same size as the old.  Do an overwrite */
                return btreeOverwriteCell(pCur, pX);
            }
            debug_assert!(loc == 0);
        } else if loc == 0 {
            /* The cursor is *not* pointing to the cell to be overwritten, nor
             ** to an adjacent cell.  Move the cursor so that it is pointing either
             ** to the cell to be overwritten or an adjacent cell.
             */
            rc = btreeTableMoveto(
                pCur,
                pX.nKey,
                ((flags & BTREE_APPEND) != 0) as i32,
                &mut loc,
            );
            if rc != 0 {
                return rc;
            }
        }
    } else {
        /* This is an index or a WITHOUT ROWID table */

        /* If BTREE_SAVEPOSITION is set, the cursor must already be pointing
         ** to a row with the same key as the new entry being inserted.
         */
        debug_assert!((flags & BTREE_SAVEPOSITION) == 0 || loc == 0);

        /* If the cursor is not already pointing either to the cell to be
         ** overwritten, or if a new cell is being inserted, if the cursor is
         ** not pointing to an immediately adjacent cell, then move the cursor
         ** so that it does.
         */
        if loc == 0 && (flags & BTREE_SAVEPOSITION) == 0 {
            if pX.nMem != 0 {
                let aMem = pX.aMem.unwrap_or(&[]);
                let mut r = UnpackedRecord {
                    pKeyInfo: pCur.pKeyInfo.clone(),
                    aMem: aMem[..std::cmp::min(pX.nMem as usize, aMem.len())].to_vec(),
                    nField: pX.nMem,
                    default_rc: 0,
                    eqSeen: 0,
                    ..Default::default()
                };
                rc = btreeIndexMoveto(pCur, &mut r, &mut loc);
            } else {
                rc = btreeMoveto(
                    pCur,
                    pX.pKey,
                    pX.nKey,
                    ((flags & BTREE_APPEND) != 0) as i32,
                    &mut loc,
                );
            }
            if rc != 0 {
                return rc;
            }
        }

        /* If the cursor is currently pointing to an entry to be overwritten
         ** and the new content is the same as as the old, then use the
         ** overwrite optimization.
         */
        if loc == 0 {
            getCellInfo(pCur);
            if pCur.info.nKey == pX.nKey {
                let x2 = BtreePayload {
                    pKey: None,
                    nKey: 0,
                    pData: pX.pKey,
                    aMem: None,
                    nMem: 0,
                    nData: pX.nKey as i32,
                    nZero: 0,
                };
                debug_assert!(pX.nKey <= 0x7fffffff);
                return btreeOverwriteCell(pCur, &x2);
            }
        }
    }
    debug_assert!(
        pCur.eState == CURSOR_VALID as u8 || (pCur.eState == CURSOR_INVALID as u8 && loc != 0)
    );

    let pPage = pCur.page().clone();
    if mp(&pPage).nFree < 0 {
        if pCur.eState > CURSOR_INVALID as u8 {
            /* ^^^^^--- due to the moveToRoot() call above */
            rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
        } else {
            rc = computeFreeSpace(&pPage);
        }
        if rc != 0 {
            return rc;
        }
    }

    let mut newCell = std::mem::take(&mut pBt.borrow_mut().pTmpSpace);
    let pageSize = pBt.borrow().pageSize as usize;
    if newCell.len() < pageSize {
        newCell.resize(pageSize, 0);
    }
    rc = btreeInsertCell(pCur, &pPage, pX, flags, loc, &mut newCell);
    pBt.borrow_mut().pTmpSpace = newCell;
    rc
}

/// Second half of sqlite3BtreeInsert(): build the cell in newCell (the
/// BtShared temp space) and insert it into pPage.
fn btreeInsertCell(
    pCur: &mut BtCursor,
    pPage: &DbPage,
    pX: &BtreePayload,
    flags: i32,
    loc: i32,
    newCell: &mut Vec<u8>,
) -> i32 {
    let mut rc;
    let mut szNew: i32 = 0;
    let pBt = pCur.bt().clone();
    let autoVacuum = pBt.borrow().autoVacuum;
    let lite = mp(pPage).lite();

    debug_assert!(lite.isInit != 0);
    if flags & BTREE_PREFORMAT != 0 {
        rc = SQLITE_OK;
        szNew = pBt.borrow().nPreformatSize;
        if szNew < 4 {
            szNew = 4;
            newCell[3] = 0;
        }
        if autoVacuum != 0 && szNew > lite.maxLocal as i32 {
            let mut info = CellInfo::default();
            (lite.xParseCell)(&lite, newCell, 0, &mut info);
            if info.nPayload != info.nLocal as u32 {
                let ovfl = get4byte(newCell, szNew as usize - 4);
                ptrmapPut(&pBt, ovfl, PTRMAP_OVERFLOW1 as u8, lite.pgno, &mut rc);
                if rc != 0 {
                    return rc;
                }
            }
        }
    } else {
        rc = fillInCell(&lite, newCell, pX, &mut szNew);
        if rc != 0 {
            return rc;
        }
    }
    debug_assert!(szNew == (lite.xCellSize)(&lite, newCell, 0) as i32);
    debug_assert!(szNew <= MX_CELL_SIZE(pBt.borrow().pageSize));
    let mut idx = pCur.ix as usize;
    pCur.info.nSize = 0;
    if loc == 0 {
        let mut info = CellInfo::default();
        if idx >= mp(pPage).nCell as usize {
            return SQLITE_CORRUPT_PAGE(&mp(pPage));
        }
        rc = sqlite3PagerWrite(pPage);
        if rc != 0 {
            return rc;
        }
        let oldCell = {
            let g = pPage.borrow();
            let off = findCell(&g.pExtra, &g.aData, idx);
            if g.pExtra.leaf == 0 {
                newCell[0..4].copy_from_slice(&g.aData[off..off + 4]);
            }
            off
        };
        rc = BTREE_CLEAR_CELL(pPage, oldCell, &mut info);
        invalidateOverflowCache(pCur);
        if info.nSize as i32 == szNew
            && info.nLocal as u32 == info.nPayload
            && (autoVacuum == 0 || szNew < lite.minLocal as i32)
        {
            /* Overwrite the old cell with the new if they are the same size.
             ** We could also try to do this if the old cell is smaller, then add
             ** the leftover space to the free list.  But experiments show that
             ** doing that is no faster then skipping this optimization and just
             ** calling dropCell() and insertCell().
             **
             ** This optimization cannot be used on an autovacuum database if the
             ** new entry uses overflow pages, as the insertCell() call below is
             ** necessary to add the PTRMAP_OVERFLOW1 pointer-map entry.  */
            debug_assert!(rc == SQLITE_OK); /* clearCell never fails when nLocal==nPayload */
            let mut g = pPage.borrow_mut();
            if oldCell < g.pExtra.hdrOffset as usize + 10 {
                return SQLITE_CORRUPT_PAGE(&g.pExtra);
            }
            if oldCell + szNew as usize > g.pExtra.aDataEnd() {
                return SQLITE_CORRUPT_PAGE(&g.pExtra);
            }
            g.aData[oldCell..oldCell + szNew as usize].copy_from_slice(&newCell[..szNew as usize]);
            return SQLITE_OK;
        }
        dropCellPg(pPage, idx, info.nSize as i32, &mut rc);
        if rc != 0 {
            return rc;
        }
    } else if loc < 0 && mp(pPage).nCell > 0 {
        debug_assert!(mp(pPage).leaf != 0);
        pCur.ix += 1;
        idx = pCur.ix as usize;
        pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
    } else {
        debug_assert!(mp(pPage).leaf != 0);
    }
    rc = insertCellFast(pPage, idx, &newCell[..], szNew);
    debug_assert!(mp(pPage).nOverflow == 0 || rc == SQLITE_OK);

    /* If no error has occurred and pPage has an overflow cell, call balance()
     ** to redistribute the cells within the tree. Since balance() may move
     ** the cursor, zero the BtCursor.info.nSize and BTCF_ValidNKey
     ** variables.
     **
     ** Previous versions of SQLite called moveToRoot() to move the cursor
     ** back to the root page as balance() used to invalidate the contents
     ** of BtCursor.apPage[] and BtCursor.aiIdx[]. Instead of doing that,
     ** set the cursor state to "invalid". This makes common insert operations
     ** slightly faster.
     **
     ** There is a subtle but important optimization here too. When inserting
     ** multiple records into an intkey b-tree using a single cursor (as can
     ** happen while processing an "INSERT INTO ... SELECT" statement), it
     ** is advantageous to leave the cursor pointing to the last entry in
     ** the b-tree if possible. If the cursor is left pointing to the last
     ** entry in the table, and the next row inserted has an integer key
     ** larger than the largest existing key, it is possible to insert the
     ** row without seeking the cursor. This can be a big performance boost.
     */
    if mp(pPage).nOverflow != 0 {
        debug_assert!(rc == SQLITE_OK);
        pCur.curFlags &= !((BTCF_ValidNKey | BTCF_ValidOvfl) as u8);
        rc = balance(pCur);

        /* Must make sure nOverflow is reset to zero even if the balance()
         ** fails. Internal data structure corruption will result otherwise.
         ** Also, set the cursor state to invalid. This stops saveCursorPosition()
         ** from trying to save the current position of the cursor.  */
        if let Some(pg) = &pCur.pPage {
            mpm(pg).nOverflow = 0;
        }
        pCur.eState = CURSOR_INVALID as u8;
        if (flags & BTREE_SAVEPOSITION) != 0 && rc == SQLITE_OK {
            btreeReleaseAllCursorPages(pCur);
            if pCur.pKeyInfo.is_some() {
                debug_assert!(pCur.pKey.is_none());
                let k = pX.pKey.unwrap_or(&[]);
                pCur.pKey = Some(k[..std::cmp::min(pX.nKey as usize, k.len())].to_vec());
            }
            pCur.eState = CURSOR_REQUIRESEEK as u8;
            pCur.nKey = pX.nKey;
        }
    }
    debug_assert!(pCur.iPage < 0 || mp(pCur.page()).nOverflow == 0);
    rc
}

/// C: `int sqlite3BtreeTransferRow(BtCursor *pDest, BtCursor *pSrc, i64 iKey)`
///
/// This function is used as part of copying the current row from cursor
/// pSrc into cursor pDest. If the cursors are open on intkey tables, then
/// parameter iKey is used as the rowid value when the record is copied
/// into pDest. Otherwise, the record is copied verbatim.
///
/// This function does not actually write the new value to cursor pDest.
/// Instead, it creates and populates any required overflow pages and
/// writes the data for the new cell into the BtShared.pTmpSpace buffer
/// for the destination database. The size of the cell, in bytes, is left
/// in BtShared.nPreformatSize. The caller completes the insertion by
/// calling sqlite3BtreeInsert() with the BTREE_PREFORMAT flag specified.
///
/// SQLITE_OK is returned if successful, or an SQLite error code otherwise.
pub fn sqlite3BtreeTransferRow(pDest: &BtCursorP, pSrc: &BtCursorP, iKey: i64) -> i32 {
    let mut d = pDest.borrow_mut();
    let mut s = pSrc.borrow_mut();
    let pBt = d.bt().clone();
    let mut tmp = std::mem::take(&mut pBt.borrow_mut().pTmpSpace);
    let pageSize = pBt.borrow().pageSize as usize;
    if tmp.len() < pageSize {
        tmp.resize(pageSize, 0);
    }
    let rc = btreeTransferRow(&mut d, &mut s, iKey, &mut tmp);
    pBt.borrow_mut().pTmpSpace = tmp;
    rc
}

/// Where sqlite3BtreeTransferRow() reads from / writes to.
#[derive(Clone, Copy, PartialEq)]
enum XferBuf {
    /// The source cursor's page (input) / the BtShared temp space (output)
    Main,
    /// The current overflow page (pPageIn / pPageOut)
    Ovfl,
}

fn btreeTransferRow(
    pDest: &mut BtCursor,
    pSrc: &mut BtCursor,
    iKey: i64,
    tmp: &mut Vec<u8>,
) -> i32 {
    let pBt = pDest.bt().clone();
    let mut aOut: usize = 0; /* Offset of next output byte (in tmp or pPageOut) */
    let mut outBuf = XferBuf::Main;

    getCellInfo(pSrc);
    if pSrc.info.nPayload < 0x80 {
        tmp[aOut] = pSrc.info.nPayload as u8;
        aOut += 1;
    } else {
        aOut += putVarint(tmp, aOut, pSrc.info.nPayload as u64);
    }
    if pDest.pKeyInfo.is_none() {
        aOut += putVarint(tmp, aOut, iKey as u64);
    }
    let srcPage = pSrc.page().clone();
    let mut nIn = pSrc.info.nLocal as u32; /* Size of input buffer aIn[] */
    let mut aIn = pSrc.info.pPayload; /* Offset of next input byte */
    let mut inBuf = XferBuf::Main;
    if aIn + nIn as usize > mp(&srcPage).aDataEnd() {
        return SQLITE_CORRUPT_PAGE(&mp(&srcPage));
    }
    let mut nRem = pSrc.info.nPayload; /* Bytes of data still to copy */
    let destPage = pDest.page().clone();
    let destLite = mp(&destPage).lite();
    if nIn == nRem && nIn < destLite.maxLocal as u32 {
        let g = srcPage.borrow();
        tmp[aOut..aOut + nIn as usize].copy_from_slice(&g.aData[aIn..aIn + nIn as usize]);
        pBt.borrow_mut().nPreformatSize = nIn as i32 + aOut as i32;
        SQLITE_OK
    } else {
        let mut rc = SQLITE_OK;
        let srcBt = pSrc.bt().clone();
        let pSrcPager = pagerOf(&srcBt);
        let srcUsable = srcBt.borrow().usableSize;
        let mut pPgnoOut: Option<(XferBuf, usize)> = None;
        let mut ovflIn: Pgno = 0;
        let mut pPageIn: Option<DbPage> = None;
        let mut pPageOut: Option<DbPage> = None;

        let mut nOut = btreePayloadToLocal(&destLite, pSrc.info.nPayload as i64) as u32; /* Size of output buffer aOut[] */
        pBt.borrow_mut().nPreformatSize = nOut as i32 + aOut as i32;
        if nOut < pSrc.info.nPayload {
            pPgnoOut = Some((XferBuf::Main, aOut + nOut as usize));
            pBt.borrow_mut().nPreformatSize += 4;
        }

        if nRem > nIn {
            if aIn + nIn as usize + 4 > mp(&srcPage).aDataEnd() {
                return SQLITE_CORRUPT_PAGE(&mp(&srcPage));
            }
            ovflIn = get4byte(&srcPage.borrow().aData, pSrc.info.pPayload + nIn as usize);
        }

        let mut chunk: Vec<u8> = Vec::new();
        loop {
            nRem = nRem.wrapping_sub(nOut);
            loop {
                debug_assert!(nOut > 0);
                if nIn > 0 {
                    let nCopy = std::cmp::min(nOut, nIn) as usize;
                    {
                        let src = match inBuf {
                            XferBuf::Main => srcPage.borrow(),
                            XferBuf::Ovfl => pPageIn.as_ref().unwrap().borrow(),
                        };
                        copyOut(&src.aData, aIn, nCopy, &mut chunk);
                    }
                    match outBuf {
                        XferBuf::Main => tmp[aOut..aOut + nCopy].copy_from_slice(&chunk),
                        XferBuf::Ovfl => pPageOut.as_ref().unwrap().borrow_mut().aData
                            [aOut..aOut + nCopy]
                            .copy_from_slice(&chunk),
                    }
                    nOut -= nCopy as u32;
                    nIn -= nCopy as u32;
                    aOut += nCopy;
                    aIn += nCopy;
                }
                if nOut > 0 {
                    if let Some(pg) = pPageIn.take() {
                        sqlite3PagerUnref(&pg);
                    }
                    rc = sqlite3PagerGet(
                        &mut pSrcPager.borrow_mut(),
                        ovflIn,
                        &mut pPageIn,
                        PAGER_GET_READONLY,
                    );
                    if rc == SQLITE_OK {
                        inBuf = XferBuf::Ovfl;
                        ovflIn = get4byte(&pPageIn.as_ref().unwrap().borrow().aData, 0);
                        aIn = 4;
                        nIn = srcUsable - 4;
                    }
                }
                if !(rc == SQLITE_OK && nOut > 0) {
                    break;
                }
            }

            if rc == SQLITE_OK && nRem > 0 && pPgnoOut.is_some() {
                let mut pgnoNew: Pgno = 0; /* Prevent harmless static-analyzer warning */
                let mut pNew: Option<DbPage> = None;
                rc = allocateBtreePage(&pBt, &mut pNew, &mut pgnoNew, 0, 0);
                match pPgnoOut.unwrap() {
                    (XferBuf::Main, off) => put4byte(tmp, off, pgnoNew),
                    (XferBuf::Ovfl, off) => put4byte(
                        &mut pPageOut.as_ref().unwrap().borrow_mut().aData,
                        off,
                        pgnoNew,
                    ),
                }
                if pBt.borrow().autoVacuum != 0 {
                    if let Some(po) = &pPageOut {
                        let parent = pgnoOf(po);
                        ptrmapPut(&pBt, pgnoNew, PTRMAP_OVERFLOW2 as u8, parent, &mut rc);
                    }
                }
                releasePage(pPageOut.as_ref());
                pPageOut = pNew;
                if let Some(po) = &pPageOut {
                    pPgnoOut = Some((XferBuf::Ovfl, 0));
                    put4byte(&mut po.borrow_mut().aData, 0, 0);
                    outBuf = XferBuf::Ovfl;
                    aOut = 4;
                    nOut = std::cmp::min(pBt.borrow().usableSize - 4, nRem);
                }
            }
            if !(nRem > 0 && rc == SQLITE_OK) {
                break;
            }
        }

        releasePage(pPageOut.as_ref());
        if let Some(pg) = &pPageIn {
            sqlite3PagerUnref(pg);
        }
        rc
    }
}

/// C: `int sqlite3BtreeDelete(BtCursor *pCur, u8 flags)`
///
/// Delete the entry that the cursor is pointing to.
///
/// If the BTREE_SAVEPOSITION bit of the flags parameter is zero, then
/// the cursor is left pointing at an arbitrary location after the delete.
/// But if that bit is set, then the cursor is left in a state such that
/// the next call to BtreeNext() or BtreePrev() moves it to the same row
/// as it would have been on if the call to BtreeDelete() had been omitted.
///
/// The BTREE_AUXDELETE bit of flags indicates that is one of several deletes
/// associated with a single table entry and its indexes.  Only one of those
/// deletes is considered the "primary" delete.  The primary delete occurs
/// on a cursor that is not a BTREE_FORDELETE cursor.  All but one delete
/// operation on non-FORDELETE cursors is tagged with the AUXDELETE flag.
/// The BTREE_AUXDELETE bit is a hint that is not used by this implementation,
/// but which might be used by alternative storage engines.
pub fn sqlite3BtreeDelete(pCur: &BtCursorP, flags: u8) -> i32 {
    btreeDelete(&mut pCur.borrow_mut(), flags)
}
fn btreeDelete(pCur: &mut BtCursor, flags: u8) -> i32 {
    let p = pCur.btree().clone();
    let pBt = pCur.bt().clone();
    let mut rc;
    let mut info = CellInfo::default(); /* Size of the cell being deleted */
    let mut bPreserve: u8; /* Keep cursor valid.  2 for CURSOR_SKIPNEXT */
    let usableSize = pBt.borrow().usableSize as i32;

    debug_assert!(pBt.borrow().inTransaction == TRANS_WRITE as u8);
    debug_assert!((pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0);
    debug_assert!(pCur.curFlags as i32 & BTCF_WriteFlag != 0);
    debug_assert!((flags as i32 & !(BTREE_SAVEPOSITION | BTREE_AUXDELETE)) == 0);
    if pCur.eState != CURSOR_VALID as u8 {
        if pCur.eState >= CURSOR_REQUIRESEEK as u8 {
            rc = btreeRestoreCursorPosition(pCur);
            if rc != 0 || pCur.eState != CURSOR_VALID as u8 {
                return rc;
            }
        } else {
            return SQLITE_CORRUPT_PGNO(pCur.pgnoRoot);
        }
    }
    debug_assert!(pCur.eState == CURSOR_VALID as u8);

    let iCellDepth = pCur.iPage; /* Depth of node containing pCell */
    let iCellIdx = pCur.ix as usize; /* Index of cell to delete */
    let pPage = pCur.page().clone(); /* Page to delete cell from */
    if mp(&pPage).nCell as usize <= iCellIdx {
        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
    }
    let pCell = {
        let g = pPage.borrow();
        findCell(&g.pExtra, &g.aData, iCellIdx)
    };
    if mp(&pPage).nFree < 0 && computeFreeSpace(&pPage) != 0 {
        return SQLITE_CORRUPT_PAGE(&mp(&pPage));
    }
    {
        let m = mp(&pPage);
        if pCell < m.aCellIdx() + m.nCell as usize {
            return SQLITE_CORRUPT_PAGE(&m);
        }
    }

    /* If the BTREE_SAVEPOSITION bit is on, then the cursor position must
     ** be preserved following this delete operation. If the current delete
     ** will cause a b-tree rebalance, then this is done by saving the cursor
     ** key and leaving the cursor in CURSOR_REQUIRESEEK state before
     ** returning.
     **
     ** If the current delete will not cause a rebalance, then the cursor
     ** will be left in CURSOR_SKIPNEXT state pointing to the entry immediately
     ** before or after the deleted entry.
     **
     ** The bPreserve value records which path is required:
     **
     **    bPreserve==0         Not necessary to save the cursor position
     **    bPreserve==1         Use CURSOR_REQUIRESEEK to save the cursor position
     **    bPreserve==2         Cursor won't move.  Set CURSOR_SKIPNEXT.
     */
    bPreserve = ((flags as i32 & BTREE_SAVEPOSITION) != 0) as u8;
    if bPreserve != 0 {
        let (leaf, nFree, nCell, sz) = {
            let g = pPage.borrow();
            let m = &g.pExtra;
            (
                m.leaf,
                m.nFree,
                m.nCell,
                (m.xCellSize)(m, &g.aData, pCell) as i32,
            )
        };
        if leaf == 0 || (nFree + sz + 2) > usableSize * 2 / 3 || nCell == 1 {
            /* A b-tree rebalance will be required after deleting this entry.
             ** Save the cursor key.  */
            rc = saveCursorKey(pCur);
            if rc != 0 {
                return rc;
            }
        } else {
            bPreserve = 2;
        }
    }

    /* If the page containing the entry to delete is not a leaf page, move
     ** the cursor to the largest entry in the tree that is smaller than
     ** the entry being deleted. This cell will replace the cell being deleted
     ** from the internal node. The 'previous' entry is used for this instead
     ** of the 'next' entry, as the previous entry is always a part of the
     ** sub-tree headed by the child page of the cell being deleted. This makes
     ** balancing the tree following the delete operation easier.  */
    let pageLeaf = mp(&pPage).leaf;
    if pageLeaf == 0 {
        rc = cursorPrevious(pCur, 0);
        debug_assert!(rc != SQLITE_DONE);
        if rc != 0 {
            return rc;
        }
    }

    /* Save the positions of any other cursors open on this table before
     ** making any modifications.  */
    if pCur.curFlags as i32 & BTCF_Multiple != 0 {
        let root = pCur.pgnoRoot;
        rc = saveAllCursors(&pBt, root, Some(&mut *pCur));
        if rc != 0 {
            return rc;
        }
    }

    /* If this is a delete operation to remove a row from a table b-tree,
     ** invalidate any incrblob cursors open on the row being deleted.  */
    if pCur.pKeyInfo.is_none() && p.borrow().hasIncrblobCur != 0 {
        let (root, nKey) = (pCur.pgnoRoot, pCur.info.nKey);
        invalidateIncrblobCursors(&p, root, nKey, 0, Some(&mut *pCur));
    }

    /* Make the page containing the entry to be deleted writable. Then free any
     ** overflow pages associated with the entry and finally remove the cell
     ** itself from within the page.  */
    rc = sqlite3PagerWrite(&pPage);
    if rc != 0 {
        return rc;
    }
    rc = BTREE_CLEAR_CELL(&pPage, pCell, &mut info);
    dropCellPg(&pPage, iCellIdx, info.nSize as i32, &mut rc);
    if rc != 0 {
        return rc;
    }

    /* If the cell deleted was not located on a leaf page, then the cursor
     ** is currently pointing to the largest entry in the sub-tree headed
     ** by the child-page of the cell that was just deleted from an internal
     ** node. The cell from the leaf node needs to be moved to the internal
     ** node to replace the deleted cell.  */
    if pageLeaf == 0 {
        let pLeaf = pCur.page().clone();
        if mp(&pLeaf).nFree < 0 {
            rc = computeFreeSpace(&pLeaf);
            if rc != 0 {
                return rc;
            }
        }
        let n: Pgno = if (iCellDepth as i32) < pCur.iPage as i32 - 1 {
            pgnoOf(pCur.apPage[iCellDepth as usize + 1].as_ref().unwrap())
        } else {
            pgnoOf(pCur.page())
        };
        let (pCellL, nCell, nLeafCell) = {
            let g = pLeaf.borrow();
            let m = &g.pExtra;
            let off = findCell(m, &g.aData, m.nCell as usize - 1);
            if off < 4 {
                return SQLITE_CORRUPT_PAGE(m);
            }
            (
                off,
                (m.xCellSize)(m, &g.aData, off) as i32,
                m.nCell as usize,
            )
        };
        debug_assert!(MX_CELL_SIZE(pBt.borrow().pageSize) >= nCell);
        rc = sqlite3PagerWrite(&pLeaf);
        if rc == SQLITE_OK {
            let (cell, sz) = {
                let g = pLeaf.borrow();
                let sz = nCell + 4 - (g.aData[pCellL] == 2) as i32;
                let mut v = Vec::new();
                copyOut(&g.aData, pCellL - 4, sz as usize, &mut v);
                (v, sz)
            };
            rc = insertCell(&pPage, iCellIdx, &cell, sz, n);
        }
        dropCellPg(&pLeaf, nLeafCell - 1, nCell, &mut rc);
        if rc != 0 {
            return rc;
        }
    }

    /* Balance the tree. If the entry deleted was located on a leaf page,
     ** then the cursor still points to that page. In this case the first
     ** call to balance() repairs the tree, and the if(...) condition is
     ** never true.
     **
     ** Otherwise, if the entry deleted was on an internal node page, then
     ** pCur is pointing to the leaf page from which a cell was removed to
     ** replace the cell deleted from the internal node. This is slightly
     ** tricky as the leaf node may be underfull, and the internal node may
     ** be either under or overfull. In this case run the balancing algorithm
     ** on the leaf node first. If the balance proceeds far enough up the
     ** tree that we can be sure that any problem in the internal node has
     ** been corrected, so be it. Otherwise, after balancing the leaf node,
     ** walk the cursor up the tree to the internal node and balance it as
     ** well.  */
    debug_assert!(mp(pCur.page()).nOverflow == 0);
    debug_assert!(mp(pCur.page()).nFree >= 0);
    if mp(pCur.page()).nFree * 3 <= usableSize * 2 {
        /* Optimization: If the free space is less than 2/3rds of the page,
         ** then balance() will always be a no-op.  No need to invoke it. */
        rc = SQLITE_OK;
    } else {
        rc = balance(pCur);
    }
    if rc == SQLITE_OK && pCur.iPage > iCellDepth {
        let pg = pCur.pPage.take().unwrap();
        releasePageNotNull(&pg);
        pCur.iPage -= 1;
        while pCur.iPage > iCellDepth {
            let pg = pCur.apPage[pCur.iPage as usize].take();
            releasePage(pg.as_ref());
            pCur.iPage -= 1;
        }
        pCur.pPage = pCur.apPage[pCur.iPage as usize].take();
        rc = balance(pCur);
    }

    if rc == SQLITE_OK {
        if bPreserve > 1 {
            debug_assert!(pCur.iPage == iCellDepth);
            pCur.eState = CURSOR_SKIPNEXT as u8;
            let nCell = mp(&pPage).nCell as usize;
            if iCellIdx >= nCell {
                pCur.skipNext = -1;
                pCur.ix = (nCell as u16).wrapping_sub(1);
            } else {
                pCur.skipNext = 1;
            }
        } else {
            rc = moveToRoot(pCur);
            if bPreserve != 0 {
                btreeReleaseAllCursorPages(pCur);
                pCur.eState = CURSOR_REQUIRESEEK as u8;
            }
            if rc == SQLITE_EMPTY {
                rc = SQLITE_OK;
            }
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// Create a new BTree table.  Write into *piTable the page
/// number for the root page of the new table.
///
/// The type of type is determined by the flags parameter.  Only the
/// following values of flags are currently in use.  Other values for
/// flags might not work:
///
///     BTREE_INTKEY|BTREE_LEAFDATA     Used for SQL tables with rowid keys
///     BTREE_ZERODATA                  Used for SQL indices
fn btreeCreateTable(p: &BtreeP, piTable: &mut Pgno, createTabFlags: i32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let mut pRoot: Option<DbPage> = None;
    let mut pgnoRoot: Pgno = 0;
    let mut rc;

    debug_assert!(pBt.borrow().inTransaction == TRANS_WRITE as u8);
    debug_assert!((pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0);

    if pBt.borrow().autoVacuum != 0 {
        let mut pgnoMove: Pgno = 0; /* Move a page here to make room for the root-page */
        let mut pPageMove: Option<DbPage> = None; /* The page to move to. */

        /* Creating a new table may probably require moving an existing database
         ** to make room for the new tables root page. In case this page turns
         ** out to be an overflow page, delete all overflow page-map caches
         ** held by open cursors.
         */
        invalidateAllOverflowCache(&pBt);

        /* Read the value of meta[3] from the database to determine where the
         ** root page of the new table should go. meta[3] is the largest root-page
         ** created so far, so the new root-page is (meta[3]+1).
         */
        sqlite3BtreeGetMeta(p, BTREE_LARGEST_ROOT_PAGE, &mut pgnoRoot);
        if pgnoRoot > btreePagecount(&pBt) {
            return SQLITE_CORRUPT_PGNO(pgnoRoot);
        }
        pgnoRoot += 1;

        /* The new root-page may not be allocated on a pointer-map page, or the
         ** PENDING_BYTE page.
         */
        while pgnoRoot == PTRMAP_PAGENO(&pBt, pgnoRoot) || pgnoRoot == pendingBytePage(&pBt) {
            pgnoRoot += 1;
        }
        debug_assert!(pgnoRoot >= 3);

        /* Allocate a page. The page that currently resides at pgnoRoot will
         ** be moved to the allocated page (unless the allocated page happens
         ** to reside at pgnoRoot).
         */
        rc = allocateBtreePage(&pBt, &mut pPageMove, &mut pgnoMove, pgnoRoot, BTALLOC_EXACT);
        if rc != SQLITE_OK {
            return rc;
        }

        if pgnoMove != pgnoRoot {
            /* pgnoRoot is the page that will be used for the root-page of
             ** the new table (assuming an error did not occur). But we were
             ** allocated pgnoMove. If required (i.e. if it was not allocated
             ** by extending the file), the current page at position pgnoMove
             ** is already journaled.
             */
            let mut eType: u8 = 0;
            let mut iPtrPage: Pgno = 0;

            /* Save the positions of any open cursors. This is required in
             ** case they are holding a reference to an xFetch reference
             ** corresponding to page pgnoRoot.  */
            rc = saveAllCursors(&pBt, 0, None);
            releasePage(pPageMove.as_ref());
            if rc != SQLITE_OK {
                return rc;
            }

            /* Move the page currently at pgnoRoot to pgnoMove. */
            rc = btreeGetPage(&pBt, pgnoRoot, &mut pRoot, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = ptrmapGet(&pBt, pgnoRoot, &mut eType, Some(&mut iPtrPage));
            if eType == PTRMAP_ROOTPAGE as u8 || eType == PTRMAP_FREEPAGE as u8 {
                rc = SQLITE_CORRUPT_PGNO(pgnoRoot);
            }
            if rc != SQLITE_OK {
                releasePage(pRoot.as_ref());
                return rc;
            }
            debug_assert!(eType != PTRMAP_ROOTPAGE as u8);
            debug_assert!(eType != PTRMAP_FREEPAGE as u8);
            rc = relocatePage(&pBt, pRoot.as_ref().unwrap(), eType, iPtrPage, pgnoMove, 0);
            releasePage(pRoot.as_ref());

            /* Obtain the page at pgnoRoot */
            if rc != SQLITE_OK {
                return rc;
            }
            pRoot = None;
            rc = btreeGetPage(&pBt, pgnoRoot, &mut pRoot, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = sqlite3PagerWrite(pRoot.as_ref().unwrap());
            if rc != SQLITE_OK {
                releasePage(pRoot.as_ref());
                return rc;
            }
        } else {
            pRoot = pPageMove;
        }

        /* Update the pointer-map and meta-data with the new root-page number. */
        ptrmapPut(&pBt, pgnoRoot, PTRMAP_ROOTPAGE as u8, 0, &mut rc);
        if rc != 0 {
            releasePage(pRoot.as_ref());
            return rc;
        }

        /* When the new root page was allocated, page 1 was made writable in
         ** order either to increase the database filesize, or to decrement the
         ** freelist count.  Hence, the sqlite3BtreeUpdateMeta() call cannot fail.
         */
        rc = sqlite3BtreeUpdateMeta(p, 4, pgnoRoot);
        if rc != 0 {
            releasePage(pRoot.as_ref());
            return rc;
        }
    } else {
        rc = allocateBtreePage(&pBt, &mut pRoot, &mut pgnoRoot, 1, 0);
        if rc != 0 {
            return rc;
        }
    }
    let ptfFlags = if createTabFlags & BTREE_INTKEY != 0 {
        PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF
    } else {
        PTF_ZERODATA | PTF_LEAF
    };
    let pRoot = pRoot.unwrap();
    zeroPage(&mut pRoot.borrow_mut(), ptfFlags);
    sqlite3PagerUnref(&pRoot);
    debug_assert!((pBt.borrow().openFlags as i32 & BTREE_SINGLE) == 0 || pgnoRoot == 2);
    *piTable = pgnoRoot;
    SQLITE_OK
}

/// C: `int sqlite3BtreeCreateTable(Btree *p, Pgno *piTable, int flags)`
pub fn sqlite3BtreeCreateTable(p: &BtreeP, piTable: &mut Pgno, flags: i32) -> i32 {
    btreeCreateTable(p, piTable, flags)
}

/// Erase the given database page and all its children.  Return
/// the page to the freelist.
fn clearDatabasePage(
    pBt: &BtSharedP,
    pgno: Pgno,
    freePageFlag: i32,
    pnChange: Option<&mut i64>,
) -> i32 {
    let mut pnChange = pnChange;
    let mut rc;
    let mut info = CellInfo::default();

    if pgno > btreePagecount(pBt) {
        return SQLITE_CORRUPT_PGNO(pgno);
    }
    if (freePageFlag >> 1) > BTCURSOR_MAX_DEPTH {
        return SQLITE_CORRUPT_PGNO(pgno);
    }
    let mut pPage = None;
    rc = getAndInitPage(pBt, pgno, &mut pPage, 0);
    if rc != 0 {
        return rc;
    }
    let pPage = pPage.unwrap();
    'cleardatabasepage_out: {
        if (pBt.borrow().openFlags as i32 & BTREE_SINGLE) == 0
            && sqlite3PagerPageRefcount(&pPage) != (1 + (pgno == 1) as i32)
        {
            rc = SQLITE_CORRUPT_PAGE(&mp(&pPage));
            break 'cleardatabasepage_out;
        }
        let hdr = mp(&pPage).hdrOffset as usize;
        let mut i = 0usize;
        while i < mp(&pPage).nCell as usize {
            let (pCell, leaf) = {
                let g = pPage.borrow();
                (findCell(&g.pExtra, &g.aData, i), g.pExtra.leaf)
            };
            if leaf == 0 {
                let child = get4byte(&pPage.borrow().aData, pCell);
                rc = clearDatabasePage(pBt, child, (freePageFlag + 2) | 1, pnChange.as_deref_mut());
                if rc != 0 {
                    break 'cleardatabasepage_out;
                }
            }
            rc = BTREE_CLEAR_CELL(&pPage, pCell, &mut info);
            if rc != 0 {
                break 'cleardatabasepage_out;
            }
            i += 1;
        }
        let (leaf, intKey, nCell) = {
            let m = mp(&pPage);
            (m.leaf, m.intKey, m.nCell)
        };
        if leaf == 0 {
            let child = get4byte(&pPage.borrow().aData, hdr + 8);
            rc = clearDatabasePage(pBt, child, (freePageFlag + 2) | 1, pnChange.as_deref_mut());
            if rc != 0 {
                break 'cleardatabasepage_out;
            }
            if intKey != 0 {
                pnChange = None;
            }
        }
        if let Some(c) = pnChange {
            *c += nCell as i64;
        }
        if (freePageFlag & 1) != 0 {
            freePage(&pPage, &mut rc);
        } else {
            rc = sqlite3PagerWrite(&pPage);
            if rc == 0 {
                let mut g = pPage.borrow_mut();
                let f = g.aData[hdr] as i32 | PTF_LEAF;
                zeroPage(&mut g, f);
            }
        }
    }

    /* cleardatabasepage_out: */
    releasePage(Some(&pPage));
    rc
}

/// C: `int sqlite3BtreeClearTable(Btree *p, int iTable, i64 *pnChange)`
///
/// Delete all information from a single table in the database.  iTable is
/// the page number of the root of the table.  After this routine returns,
/// the root page is empty, but still exists.
///
/// This routine will fail with SQLITE_LOCKED if there are any open
/// read cursors on the table.  Open write cursors are moved to the
/// root of the table.
///
/// If pnChange is not NULL, then the integer value pointed to by pnChange
/// is incremented by the number of entries in the table.
pub fn sqlite3BtreeClearTable(p: &BtreeP, iTable: i32, pnChange: &mut i64) -> i32 {
    btreeClearTable(p, iTable, Some(pnChange))
}
fn btreeClearTable(p: &BtreeP, iTable: i32, pnChange: Option<&mut i64>) -> i32 {
    let pBt = p.borrow().pBt.clone();
    debug_assert!(p.borrow().inTrans == TRANS_WRITE as u8);

    let mut rc = saveAllCursors(&pBt, iTable as Pgno, None);

    if SQLITE_OK == rc {
        /* Invalidate all incrblob cursors open on table iTable (assuming iTable
         ** is the root of a table b-tree - if it is not, the following call is
         ** a no-op).  */
        if p.borrow().hasIncrblobCur != 0 {
            invalidateIncrblobCursors(p, iTable as Pgno, 0, 1, None);
        }
        rc = clearDatabasePage(&pBt, iTable as Pgno, 0, pnChange);
    }
    rc
}

/// C: `int sqlite3BtreeClearTableOfCursor(BtCursor *pCur)`
///
/// Delete all information from the single table that pCur is open on.
///
/// This routine only work for pCur on an ephemeral table.
pub fn sqlite3BtreeClearTableOfCursor(pCur: &BtCursorP) -> i32 {
    let (p, root) = {
        let c = pCur.borrow();
        (c.btree().clone(), c.pgnoRoot)
    };
    btreeClearTable(&p, root as i32, None)
}

/// Erase all information in a table and add the root of the table to
/// the freelist.  Except, the root of the principle table (the one on
/// page 1) is never added to the freelist.
///
/// This routine will fail with SQLITE_LOCKED if there are any open
/// cursors on the table.
///
/// If AUTOVACUUM is enabled and the page at iTable is not the last
/// root page in the database file, then the last root page
/// in the database file is moved into the slot formerly occupied by
/// iTable and that last slot formerly occupied by the last root page
/// is added to the freelist instead of iTable.  In this say, all
/// root pages are kept at the beginning of the database file, which
/// is necessary for AUTOVACUUM to work right.  *piMoved is set to the
/// page number that used to be the last root page in the file before
/// the move.  If no page gets moved, *piMoved is set to 0.
/// The last root page is recorded in meta[3] and the value of
/// meta[3] is updated by this procedure.
fn btreeDropTable(p: &BtreeP, iTable: Pgno, piMoved: &mut i32) -> i32 {
    let mut rc;
    let mut pPage: Option<DbPage> = None;
    let pBt = p.borrow().pBt.clone();

    debug_assert!(p.borrow().inTrans == TRANS_WRITE as u8);
    debug_assert!(iTable >= 2);
    if iTable > btreePagecount(&pBt) {
        return SQLITE_CORRUPT_PGNO(iTable);
    }

    rc = btreeClearTable(p, iTable as i32, None);
    if rc != 0 {
        return rc;
    }
    rc = btreeGetPage(&pBt, iTable, &mut pPage, 0);
    if rc != 0 {
        releasePage(pPage.as_ref());
        return rc;
    }
    let pPage = pPage.unwrap();

    *piMoved = 0;

    if pBt.borrow().autoVacuum != 0 {
        let mut maxRootPgno: Pgno = 0;
        sqlite3BtreeGetMeta(p, BTREE_LARGEST_ROOT_PAGE, &mut maxRootPgno);

        if iTable == maxRootPgno {
            /* If the table being dropped is the table with the largest root-page
             ** number in the database, put the root page on the free list.
             */
            freePage(&pPage, &mut rc);
            releasePage(Some(&pPage));
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            /* The table being dropped does not have the largest root-page
             ** number in the database. So move the page that does into the
             ** gap left by the deleted root-page.
             */
            releasePage(Some(&pPage));
            let mut pMove: Option<DbPage> = None;
            rc = btreeGetPage(&pBt, maxRootPgno, &mut pMove, 0);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = relocatePage(
                &pBt,
                pMove.as_ref().unwrap(),
                PTRMAP_ROOTPAGE as u8,
                0,
                iTable,
                0,
            );
            releasePage(pMove.as_ref());
            if rc != SQLITE_OK {
                return rc;
            }
            pMove = None;
            rc = btreeGetPage(&pBt, maxRootPgno, &mut pMove, 0);
            if rc == SQLITE_OK {
                rc = sqlite3PagerWrite(pMove.as_ref().unwrap());
            }
            if let Some(pm) = &pMove {
                freePage(pm, &mut rc);
            }
            releasePage(pMove.as_ref());
            if rc != SQLITE_OK {
                return rc;
            }
            *piMoved = maxRootPgno as i32;
        }

        /* Set the new 'max-root-page' value in the database header. This
         ** is the old value less one, less one more if that happens to
         ** be a root-page number, less one again if that is the
         ** PENDING_BYTE_PAGE.
         */
        maxRootPgno -= 1;
        while maxRootPgno == pendingBytePage(&pBt) || PTRMAP_ISPAGE(&pBt, maxRootPgno) {
            maxRootPgno -= 1;
        }
        debug_assert!(maxRootPgno != pendingBytePage(&pBt));

        rc = sqlite3BtreeUpdateMeta(p, 4, maxRootPgno);
    } else {
        freePage(&pPage, &mut rc);
        releasePage(Some(&pPage));
    }
    rc
}

/// C: `int sqlite3BtreeDropTable(Btree *p, int iTable, int *piMoved)`
pub fn sqlite3BtreeDropTable(p: &BtreeP, iTable: i32, piMoved: &mut i32) -> i32 {
    btreeDropTable(p, iTable as Pgno, piMoved)
}

/// C: `void sqlite3BtreeGetMeta(Btree *p, int idx, u32 *pMeta)`
///
/// This function may only be called if the b-tree connection already
/// has a read or write transaction open on the database.
///
/// Read the meta-information out of a database file.  Meta[0]
/// is the number of free pages currently in the database.  Meta[1]
/// through meta[15] are available for use by higher layers.  Meta[0]
/// is read-only, the others are read/write.
///
/// The schema layer numbers meta values differently.  At the schema
/// layer (and the SetCookie and ReadCookie opcodes) the number of
/// free pages is not visible.  So Cookie[0] is the same as Meta[1].
///
/// This routine treats Meta[BTREE_DATA_VERSION] as a special case.  Instead
/// of reading the value out of the header, it instead loads the "DataVersion"
/// from the pager.  The BTREE_DATA_VERSION value is not actually stored in the
/// database file.  It is a number computed by the pager.  But its access
/// pattern is the same as header meta values, and so it is convenient to
/// read it from this routine.
pub fn sqlite3BtreeGetMeta(p: &BtreeP, idx: i32, pMeta: &mut u32) {
    let (pBt, iBDataVersion) = {
        let b = p.borrow();
        (b.pBt.clone(), b.iBDataVersion)
    };
    debug_assert!(p.borrow().inTrans > TRANS_NONE as u8);
    debug_assert!(pBt.borrow().pPage1.is_some());
    debug_assert!(idx >= 0 && idx <= 15);

    if idx == BTREE_DATA_VERSION {
        let pager = pagerOf(&pBt);
        let v = sqlite3PagerDataVersion(&mut pager.borrow_mut());
        *pMeta = v.wrapping_add(iBDataVersion);
    } else {
        *pMeta = page1Get4(&pBt, 36 + idx as usize * 4);
    }
}

/// C: `int sqlite3BtreeUpdateMeta(Btree *p, int idx, u32 iMeta)`
///
/// Write meta-information back into the database.  Meta[0] is
/// read-only and may not be written.
pub fn sqlite3BtreeUpdateMeta(p: &BtreeP, idx: i32, iMeta: u32) -> i32 {
    let pBt = p.borrow().pBt.clone();
    debug_assert!(idx >= 1 && idx <= 15);
    debug_assert!(p.borrow().inTrans == TRANS_WRITE as u8);
    let pP1 = page1Of(&pBt);
    let rc = sqlite3PagerWrite(&pP1);
    if rc == SQLITE_OK {
        put4byte(&mut pP1.borrow_mut().aData, 36 + idx as usize * 4, iMeta);
        if idx == BTREE_INCR_VACUUM {
            debug_assert!(pBt.borrow().autoVacuum != 0 || iMeta == 0);
            debug_assert!(iMeta == 0 || iMeta == 1);
            pBt.borrow_mut().incrVacuum = iMeta as u8;
        }
    }
    rc
}

/// C: `int sqlite3BtreeCount(sqlite3 *db, BtCursor *pCur, i64 *pnEntry)`
///
/// The first argument, pCur, is a cursor opened on some b-tree. Count the
/// number of entries in the b-tree and write the result to *pnEntry.
///
/// SQLITE_OK is returned if the operation is successfully executed.
/// Otherwise, if an error is encountered (i.e. an IO error or database
/// corruption) an SQLite error code is returned.
pub fn sqlite3BtreeCount(db: &mut Sqlite3, pCur: &BtCursorP, pnEntry: &mut i64) -> i32 {
    let isInterrupted = db.isInterrupted.clone();
    btreeCount(pCur, pnEntry, &isInterrupted)
}

/// sqlite3BtreeCount() body (`isInterrupted` is db->u1.isInterrupted).
pub fn btreeCount(
    pCur: &BtCursorP,
    pnEntry: &mut i64,
    isInterrupted: &std::cell::Cell<i32>,
) -> i32 {
    let mut c = pCur.borrow_mut();
    let pCur = &mut *c;
    let mut nEntry: i64 = 0; /* Value to return in *pnEntry */

    let mut rc = moveToRoot(pCur);
    if rc == SQLITE_EMPTY {
        *pnEntry = 0;
        return SQLITE_OK;
    }

    /* Unless an error occurs, the following loop runs one iteration for each
     ** page in the B-Tree structure (not including overflow pages).
     */
    while rc == SQLITE_OK && isInterrupted.get() == 0 {
        /* If this is a leaf page or the tree is not an int-key tree, then
         ** this page contains countable entries. Increment the entry counter
         ** accordingly.
         */
        let mut pPage = pCur.page().clone();
        {
            let m = mp(&pPage);
            if m.leaf != 0 || m.intKey == 0 {
                nEntry += m.nCell as i64;
            }
        }

        /* pPage is a leaf node. This loop navigates the cursor so that it
         ** points to the first interior cell that it points to the parent of
         ** the next page in the tree that has not yet been visited. The
         ** pCur->aiIdx[pCur->iPage] value is set to the index of the parent cell
         ** of the page, or to the number of cells in the page if the next page
         ** to visit is the right-child of its parent.
         **
         ** If all pages in the tree have been visited, return SQLITE_OK to the
         ** caller.
         */
        if mp(&pPage).leaf != 0 {
            loop {
                if pCur.iPage == 0 {
                    /* All pages of the b-tree have been visited. Return successfully. */
                    *pnEntry = nEntry;
                    return moveToRoot(pCur);
                }
                moveToParent(pCur);
                if pCur.ix < mp(pCur.page()).nCell {
                    break;
                }
            }

            pCur.ix += 1;
            pPage = pCur.page().clone();
        }

        /* Descend to the child node of the cell that the cursor currently
         ** points at. This is the right-child if (iIdx==pPage->nCell).
         */
        let iIdx = pCur.ix as usize;
        let child = {
            let g = pPage.borrow();
            if iIdx == g.pExtra.nCell as usize {
                get4byte(&g.aData, g.pExtra.hdrOffset as usize + 8)
            } else {
                get4byte(&g.aData, findCell(&g.pExtra, &g.aData, iIdx))
            }
        };
        rc = moveToChild(pCur, child);
    }

    /* An error has occurred. Return an error code. */
    rc
}

/// C: `Pager * sqlite3BtreePager(Btree *p)`
///
/// Return the pager associated with a BTree.  This routine is used for
/// testing and debugging only.
pub fn sqlite3BtreePager(p: &BtreeP) -> crate::pager::PagerP {
    let pBt = p.borrow().pBt.clone();
    pagerOf(&pBt)
}

// ---------------------------------------------------------------------------
// Integrity check
// ---------------------------------------------------------------------------

/// This structure is passed around through all the PRAGMA integrity_check
/// checking routines in order to keep track of some global state information.
///
/// The aRef[] array is allocated so that there is 1 bit for each page in
/// the database. As the integrity-check proceeds, for each page used in
/// the database the corresponding bit is set. This allows integrity-check to
/// detect pages that are used twice and orphaned pages (both of which
/// indicate corruption).
struct IntegrityCk {
    /// The tree being checked out
    pBt: BtSharedP,
    /// The associated pager.  Also accessible by pBt->pPager
    pPager: PagerP,
    /// 1 bit per page in the db (see above)
    aPgRef: Vec<u8>,
    /// Pages in the database.  0 for partial check
    nCkPage: Pgno,
    /// Stop accumulating errors when this reaches zero
    mxErr: i32,
    /// Number of messages written to zErrMsg so far
    nErr: i32,
    /// SQLITE_OK, SQLITE_NOMEM, or SQLITE_INTERRUPT
    rc: i32,
    /// Number of steps into the integrity_check process
    nStep: u32,
    /// Current btree recursion depth
    nAbove: u8,
    /// Error message prefix
    zPfx: Option<&'static [u8]>,
    /// Value for first %u substitution in zPfx (root page)
    v0: Pgno,
    /// Value for second %u substitution in zPfx (current pg)
    v1: Pgno,
    /// Value for third %d substitution in zPfx
    v2: i32,
    /// Accumulate the error message text here
    errMsg: StrAccum,
    /// Min-heap used for analyzing cell coverage
    heap: Vec<u32>,
    /// db->u1.isInterrupted
    isInterrupted: Rc<std::cell::Cell<i32>>,
    /// db->xProgress
    xProgress: Option<Rc<dyn Fn() -> i32>>,
    /// db->nProgressOps
    nProgressOps: u32,
    /// Number of rows visited in current tree
    nRow: i64,
}

/// Record an OOM error during integrity_check
fn checkOom(pCheck: &mut IntegrityCk) {
    pCheck.rc = SQLITE_NOMEM;
    pCheck.mxErr = 0; /* Causes integrity_check processing to stop */
    if pCheck.nErr == 0 {
        pCheck.nErr += 1;
    }
}

/// Invoke the progress handler, if appropriate.  Also check for an
/// interrupt.
fn checkProgress(pCheck: &mut IntegrityCk) {
    if pCheck.isInterrupted.get() != 0 {
        pCheck.rc = SQLITE_INTERRUPT;
        pCheck.nErr += 1;
        pCheck.mxErr = 0;
    }
    if let Some(x) = pCheck.xProgress.clone() {
        debug_assert!(pCheck.nProgressOps > 0);
        pCheck.nStep = pCheck.nStep.wrapping_add(1);
        if pCheck.nProgressOps > 0 && (pCheck.nStep % pCheck.nProgressOps) == 0 && x() != 0 {
            pCheck.rc = SQLITE_INTERRUPT;
            pCheck.nErr += 1;
            pCheck.mxErr = 0;
        }
    }
}

/// Append a message to the error message string.
fn checkAppendMsg(pCheck: &mut IntegrityCk, zFormat: &[u8], args: &[crate::printf::PfArg]) {
    use crate::printf::*;
    checkProgress(pCheck);
    if pCheck.mxErr == 0 {
        return;
    }
    pCheck.mxErr -= 1;
    pCheck.nErr += 1;
    if sqlite3_str_length(&pCheck.errMsg) != 0 {
        sqlite3_str_append(&mut pCheck.errMsg, b"\n", 1);
    }
    if let Some(zPfx) = pCheck.zPfx {
        let (v0, v1, v2) = (pCheck.v0, pCheck.v1, pCheck.v2);
        sqlite3_str_appendf(
            &mut pCheck.errMsg,
            zPfx,
            &[PfArg::from(v0), PfArg::from(v1), PfArg::from(v2)],
        );
    }
    sqlite3_str_appendf(&mut pCheck.errMsg, zFormat, args);
    if pCheck.errMsg.accError == SQLITE_NOMEM as u8 {
        checkOom(pCheck);
    }
}

/// Return non-zero if the bit in the IntegrityCk.aPgRef[] array that
/// corresponds to page iPg is already set.
fn getPageReferenced(pCheck: &IntegrityCk, iPg: Pgno) -> bool {
    debug_assert!(iPg <= pCheck.nCkPage);
    (pCheck.aPgRef[(iPg / 8) as usize] & (1 << (iPg & 0x07))) != 0
}

/// Set the bit in the IntegrityCk.aPgRef[] array that corresponds to page iPg.
fn setPageReferenced(pCheck: &mut IntegrityCk, iPg: Pgno) {
    debug_assert!(iPg <= pCheck.nCkPage);
    pCheck.aPgRef[(iPg / 8) as usize] |= 1 << (iPg & 0x07);
}

/// Add 1 to the reference count for page iPage.  If this is the second
/// reference to the page, add an error message to pCheck->zErrMsg.
/// Return 1 if there are 2 or more references to the page and 0 if
/// if this is the first reference to the page.
///
/// Also check that the page number is in bounds.
fn checkRef(pCheck: &mut IntegrityCk, iPage: Pgno) -> i32 {
    if iPage > pCheck.nCkPage || iPage == 0 {
        checkAppendMsg(pCheck, b"invalid page number %u", &[iPage.into()]);
        return 1;
    }
    if getPageReferenced(pCheck, iPage) {
        checkAppendMsg(pCheck, b"2nd reference to page %u", &[iPage.into()]);
        return 1;
    }
    setPageReferenced(pCheck, iPage);
    0
}

/// Check that the entry in the pointer-map for page iChild maps to
/// page iParent, pointer type ptrType. If not, append an error message
/// to pCheck.
fn checkPtrmap(pCheck: &mut IntegrityCk, iChild: Pgno, eType: u8, iParent: Pgno) {
    let mut ePtrmapType = 0u8;
    let mut iPtrmapParent: Pgno = 0;

    let pBt = pCheck.pBt.clone();
    let rc = ptrmapGet(&pBt, iChild, &mut ePtrmapType, Some(&mut iPtrmapParent));
    if rc != SQLITE_OK {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            checkOom(pCheck);
        }
        checkAppendMsg(pCheck, b"Failed to read ptrmap key=%u", &[iChild.into()]);
        return;
    }

    if ePtrmapType != eType || iPtrmapParent != iParent {
        checkAppendMsg(
            pCheck,
            b"Bad ptr map entry key=%u expected=(%u,%u) got=(%u,%u)",
            &[
                iChild.into(),
                (eType as u32).into(),
                iParent.into(),
                (ePtrmapType as u32).into(),
                iPtrmapParent.into(),
            ],
        );
    }
}

/// Check the integrity of the freelist or of an overflow page list.
/// Verify that the number of pages on the list is N.
fn checkList(pCheck: &mut IntegrityCk, isFreeList: i32, iPage: Pgno, N: u32) {
    let mut iPage = iPage;
    let mut N = N;
    let expected = N;
    let nErrAtStart = pCheck.nErr;
    let (autoVacuum, usableSize) = {
        let b = pCheck.pBt.borrow();
        (b.autoVacuum, b.usableSize)
    };
    while iPage != 0 && pCheck.mxErr != 0 {
        if checkRef(pCheck, iPage) != 0 {
            break;
        }
        N = N.wrapping_sub(1);
        let mut pOvflPage = None;
        let pager = pCheck.pPager.clone();
        if sqlite3PagerGet(&mut pager.borrow_mut(), iPage, &mut pOvflPage, 0) != 0 {
            checkAppendMsg(pCheck, b"failed to get page %u", &[iPage.into()]);
            break;
        }
        let pOvflPage = pOvflPage.unwrap();
        let pOvflData: Vec<u8> = pOvflPage.borrow().aData.clone();
        if isFreeList != 0 {
            let n = get4byte(&pOvflData, 4);
            if autoVacuum != 0 {
                checkPtrmap(pCheck, iPage, PTRMAP_FREEPAGE as u8, 0);
            }
            if n > usableSize / 4 - 2 {
                checkAppendMsg(
                    pCheck,
                    b"freelist leaf count too big on page %u",
                    &[iPage.into()],
                );
                N = N.wrapping_sub(1);
            } else {
                for i in 0..n as usize {
                    let iFreePage = get4byte(&pOvflData, 8 + i * 4);
                    if autoVacuum != 0 {
                        checkPtrmap(pCheck, iFreePage, PTRMAP_FREEPAGE as u8, 0);
                    }
                    checkRef(pCheck, iFreePage);
                }
                N = N.wrapping_sub(n);
            }
        } else {
            /* If this database supports auto-vacuum and iPage is not the last
             ** page in this overflow list, check that the pointer-map entry for
             ** the following page matches iPage.
             */
            if autoVacuum != 0 && N > 0 {
                let i = get4byte(&pOvflData, 0);
                checkPtrmap(pCheck, i, PTRMAP_OVERFLOW2 as u8, iPage);
            }
        }
        iPage = get4byte(&pOvflData, 0);
        sqlite3PagerUnref(&pOvflPage);
    }
    if N != 0 && nErrAtStart == pCheck.nErr {
        let what: &[u8] = if isFreeList != 0 {
            b"size"
        } else {
            b"overflow list length"
        };
        checkAppendMsg(
            pCheck,
            b"%s is %u but should be %u",
            &[
                what.into(),
                expected.wrapping_sub(N).into(),
                expected.into(),
            ],
        );
    }
}

/// An implementation of a min-heap.
///
/// aHeap[0] is the number of elements on the heap.  aHeap[1] is the
/// root element.  The daughter nodes of aHeap[N] are aHeap[N*2]
/// and aHeap[N*2+1].
///
/// The heap property is this:  Every node is less than or equal to both
/// of its daughter nodes.  A consequence of the heap property is that the
/// root node aHeap[1] is always the minimum value currently in the heap.
///
/// The btreeHeapInsert() routine inserts an unsigned 32-bit number onto
/// the heap, preserving the heap property.  The btreeHeapPull() routine
/// removes the root element from the heap (the minimum value in the heap)
/// and then moves other nodes around as necessary to preserve the heap
/// property.
///
/// This heap is used for cell overlap and coverage testing.  Each u32
/// entry represents the span of a cell or freeblock on a btree page.
/// The upper 16 bits are the index of the first byte of a range and the
/// lower 16 bits are the index of the last byte of that range.
fn btreeHeapInsert(aHeap: &mut Vec<u32>, x: u32) {
    let mut x = x;
    aHeap[0] += 1;
    let mut i = aHeap[0] as usize;
    if aHeap.len() <= i + 1 {
        aHeap.resize(i + 2, 0);
    }
    aHeap[i] = x;
    loop {
        let j = i / 2;
        if !(j > 0 && aHeap[j] > aHeap[i]) {
            break;
        }
        x = aHeap[j];
        aHeap[j] = aHeap[i];
        aHeap[i] = x;
        i = j;
    }
}
fn btreeHeapPull(aHeap: &mut Vec<u32>, pOut: &mut u32) -> bool {
    let x = aHeap[0] as usize;
    if x == 0 {
        return false;
    }
    *pOut = aHeap[1];
    aHeap[1] = aHeap[x];
    aHeap[x] = 0xffffffff;
    aHeap[0] -= 1;
    let mut i = 1usize;
    loop {
        let mut j = i * 2;
        if !(j <= aHeap[0] as usize) {
            break;
        }
        if aHeap[j] > aHeap[j + 1] {
            j += 1;
        }
        if aHeap[i] < aHeap[j] {
            break;
        }
        let t = aHeap[i];
        aHeap[i] = aHeap[j];
        aHeap[j] = t;
        i = j;
    }
    true
}

/// Do various sanity checks on a single page of a tree.  Return
/// the tree depth.  Root pages return 0.  Parents of root pages
/// return 1, and so forth.
///
/// These checks are done:
///
///      1.  Make sure that cells and freeblocks do not overlap
///          but combine to completely cover the page.
///      2.  Make sure integer cell keys are in order.
///      3.  Check the integrity of overflow pages.
///      4.  Recursively call checkTreePage on all children.
///      5.  Verify that the depth of all children is the same.
fn checkTreePage(pCheck: &mut IntegrityCk, iPage: Pgno, piMinKey: &mut i64, maxKey: i64) -> i32 {
    let mut maxKey = maxKey;
    let mut depth: i32 = -1;
    let mut doCoverageCheck = true; /* True if cell coverage checking should be done */
    let mut keyCanBeEqual = true; /* True if IPK can be equal to maxKey
     ** False if IPK must be strictly less than maxKey */
    let saved_zPfx = pCheck.zPfx;
    let saved_v1 = pCheck.v1;
    let saved_v2 = pCheck.v2;
    let mut savedIsInit: u8 = 0;

    /* Check that the page exists
     */
    checkProgress(pCheck);
    if pCheck.mxErr == 0 {
        return 0;
    }
    let pBt = pCheck.pBt.clone();
    let (usableSize, autoVacuum) = {
        let b = pBt.borrow();
        (b.usableSize, b.autoVacuum)
    };
    if iPage == 0 {
        return 0;
    }
    if checkRef(pCheck, iPage) != 0 {
        return 0;
    }
    pCheck.zPfx = Some(b"Tree %u page %u: ");
    pCheck.v1 = iPage;
    pCheck.nAbove += 1;
    let mut pPage: Option<DbPage> = None;
    'end_of_check: {
        let rc = btreeGetPage(&pBt, iPage, &mut pPage, 0);
        if rc != 0 {
            checkAppendMsg(
                pCheck,
                b"unable to get the page. error code=%d",
                &[rc.into()],
            );
            if rc == SQLITE_IOERR_NOMEM {
                pCheck.rc = SQLITE_NOMEM;
            }
            break 'end_of_check;
        }
        let pg = pPage.clone().unwrap();
        if pCheck.nAbove as i32 > BTCURSOR_MAX_DEPTH {
            checkAppendMsg(
                pCheck,
                b"btree depth exceeds %d",
                &[BTCURSOR_MAX_DEPTH.into()],
            );
            break 'end_of_check;
        }

        /* Clear MemPage.isInit to make sure the corruption detection code in
         ** btreeInitPage() is executed.  */
        {
            let mut g = pg.borrow_mut();
            savedIsInit = g.pExtra.isInit;
            g.pExtra.isInit = 0;
        }
        let rc = btreeInitPage(&mut pg.borrow_mut());
        if rc != 0 {
            debug_assert!(rc == SQLITE_CORRUPT); /* The only possible error from InitPage */
            checkAppendMsg(
                pCheck,
                b"btreeInitPage() returns error code %d",
                &[rc.into()],
            );
            break 'end_of_check;
        }
        let rc = computeFreeSpace(&pg);
        if rc != 0 {
            debug_assert!(rc == SQLITE_CORRUPT);
            checkAppendMsg(pCheck, b"free space corruption", &[rc.into()]);
            break 'end_of_check;
        }
        let (data, m): (Vec<u8>, MemPage) = {
            let g = pg.borrow();
            (g.aData.clone(), g.pExtra.lite())
        };
        let hdr = m.hdrOffset as usize;

        /* Set up for cell analysis */
        pCheck.zPfx = Some(b"Tree %u page %u cell %u: ");
        let contentOffset = get2byteNotZero(&data, hdr + 5) as u32;
        debug_assert!(contentOffset <= usableSize); /* Enforced by btreeInitPage() */

        /* EVIDENCE-OF: R-37002-32774 The two-byte integer at offset 3 gives the
         ** number of cells on the page. */
        let nCell = get2byte(&data, hdr + 3) as i32;
        debug_assert!(m.nCell as i32 == nCell);
        if m.leaf != 0 || m.intKey == 0 {
            pCheck.nRow += nCell as i64;
        }

        /* EVIDENCE-OF: R-23882-45353 The cell pointer array of a b-tree page
         ** immediately follows the b-tree page header. */
        let cellStart = hdr + 12 - 4 * m.leaf as usize;
        debug_assert!(m.cellOffset as usize == cellStart);
        let mut pCellIdx = cellStart as isize + 2 * (nCell as isize - 1);

        if m.leaf == 0 {
            /* Analyze the right-child page of internal pages */
            let pgno = get4byte(&data, hdr + 8);
            if autoVacuum != 0 {
                pCheck.zPfx = Some(b"Tree %u page %u right child: ");
                checkPtrmap(pCheck, pgno, PTRMAP_BTREE as u8, iPage);
            }
            let mk = maxKey;
            depth = checkTreePage(pCheck, pgno, &mut maxKey, mk);
            keyCanBeEqual = false;
        } else {
            /* For leaf pages, the coverage check will occur in the same loop
             ** as the other cell checks, so initialize the heap.  */
            pCheck.heap[0] = 0;
        }

        /* EVIDENCE-OF: R-02776-14802 The cell pointer array consists of K 2-byte
         ** integer offsets to the cell contents. */
        let mut i = nCell - 1;
        while i >= 0 && pCheck.mxErr != 0 {
            let mut info = CellInfo::default();

            /* Check cell size */
            pCheck.v2 = i;
            debug_assert!(pCellIdx == (cellStart + i as usize * 2) as isize);
            let pc = get2byteAligned(&data, pCellIdx as usize) as u32;
            pCellIdx -= 2;
            if pc < contentOffset || pc > usableSize - 4 {
                checkAppendMsg(
                    pCheck,
                    b"Offset %u out of range %u..%u",
                    &[pc.into(), contentOffset.into(), (usableSize - 4).into()],
                );
                doCoverageCheck = false;
                i -= 1;
                continue;
            }
            let pCell = pc as usize;
            (m.xParseCell)(&m, &data, pCell, &mut info);
            if pc + info.nSize as u32 > usableSize {
                checkAppendMsg(pCheck, b"Extends off end of page", &[]);
                doCoverageCheck = false;
                i -= 1;
                continue;
            }
            if info.nPayload != 0 && at(&data, info.pPayload) < 2 {
                checkAppendMsg(pCheck, b"Bad cell header size", &[]);
                doCoverageCheck = false;
                i -= 1;
                continue;
            }

            /* Check for integer primary key out of range */
            if m.intKey != 0 {
                if if keyCanBeEqual {
                    info.nKey > maxKey
                } else {
                    info.nKey >= maxKey
                } {
                    checkAppendMsg(pCheck, b"Rowid %lld out of order", &[info.nKey.into()]);
                }
                maxKey = info.nKey;
                keyCanBeEqual = false; /* Only the first key on the page may ==maxKey */
            }

            /* Check the content overflow list */
            if info.nPayload > info.nLocal as u32 {
                debug_assert!(pc + info.nSize as u32 - 4 <= usableSize);
                let nPage =
                    (info.nPayload - info.nLocal as u32 + usableSize - 5) / (usableSize - 4); /* Number of pages on the overflow chain */
                let pgnoOvfl = get4byte(&data, pCell + info.nSize as usize - 4); /* First page of the overflow chain */
                if autoVacuum != 0 {
                    checkPtrmap(pCheck, pgnoOvfl, PTRMAP_OVERFLOW1 as u8, iPage);
                }
                checkList(pCheck, 0, pgnoOvfl, nPage);
            }

            if m.leaf == 0 {
                /* Check sanity of left child page for internal pages */
                let pgno = get4byte(&data, pCell);
                if autoVacuum != 0 {
                    checkPtrmap(pCheck, pgno, PTRMAP_BTREE as u8, iPage);
                }
                let mk = maxKey;
                let d2 = checkTreePage(pCheck, pgno, &mut maxKey, mk);
                keyCanBeEqual = false;
                if d2 != depth {
                    checkAppendMsg(pCheck, b"Child page depth differs", &[]);
                    depth = d2;
                }
            } else {
                /* Populate the coverage-checking heap for leaf pages */
                btreeHeapInsert(&mut pCheck.heap, (pc << 16) | (pc + info.nSize as u32 - 1));
            }
            i -= 1;
        }
        *piMinKey = maxKey;

        /* Check for complete coverage of the page
         */
        pCheck.zPfx = None;
        if doCoverageCheck && pCheck.mxErr > 0 {
            /* For leaf pages, the min-heap has already been initialized and the
             ** cells have already been inserted.  But for internal pages, that has
             ** not yet been done, so do it now */
            if m.leaf == 0 {
                pCheck.heap[0] = 0;
                let mut i = nCell - 1;
                while i >= 0 {
                    let pc = get2byteAligned(&data, cellStart + i as usize * 2) as u32;
                    let size = (m.xCellSize)(&m, &data, pc as usize) as u32;
                    btreeHeapInsert(&mut pCheck.heap, (pc << 16) | (pc + size).wrapping_sub(1));
                    i -= 1;
                }
            }
            /* Add the freeblocks to the min-heap
             **
             ** EVIDENCE-OF: R-20690-50594 The second field of the b-tree page header
             ** is the offset of the first freeblock, or zero if there are no
             ** freeblocks on the page.
             */
            let mut i = get2byte(&data, hdr + 1) as u32;
            while i > 0 {
                debug_assert!(i <= usableSize - 4); /* Enforced by btreeComputeFreeSpace() */
                let size = get2byte(&data, i as usize + 2) as u32;
                debug_assert!(i + size <= usableSize); /* due to btreeComputeFreeSpace() */
                btreeHeapInsert(&mut pCheck.heap, (i << 16) | (i + size - 1));
                /* EVIDENCE-OF: R-58208-19414 The first 2 bytes of a freeblock are a
                 ** big-endian integer which is the offset in the b-tree page of the next
                 ** freeblock in the chain, or zero if the freeblock is the last on the
                 ** chain. */
                let j = get2byte(&data, i as usize) as u32;
                /* EVIDENCE-OF: R-06866-39125 Freeblocks are always connected in order of
                 ** increasing offset. */
                debug_assert!(j == 0 || j > i + size); /* Enforced by btreeComputeFreeSpace() */
                debug_assert!(j <= usableSize - 4); /* Enforced by btreeComputeFreeSpace() */
                i = j;
            }
            /* Analyze the min-heap looking for overlap between cells and/or
             ** freeblocks, and counting the number of untracked bytes in nFrag.
             **
             ** Each min-heap entry is of the form:    (start_address<<16)|end_address.
             ** There is an implied first entry the covers the page header, the cell
             ** pointer index, and the gap between the cell pointer index and the start
             ** of cell content.
             **
             ** The loop below pulls entries from the min-heap in order and compares
             ** the start_address against the previous end_address.  If there is an
             ** overlap, that means bytes are used multiple times.  If there is a gap,
             ** that gap is added to the fragmentation count.
             */
            let mut nFrag: u32 = 0;
            let mut prev: u32 = contentOffset.wrapping_sub(1); /* Implied first min-heap entry */
            let mut x: u32 = 0;
            while btreeHeapPull(&mut pCheck.heap, &mut x) {
                if (prev & 0xffff) >= (x >> 16) {
                    checkAppendMsg(
                        pCheck,
                        b"Multiple uses for byte %u of page %u",
                        &[(x >> 16).into(), iPage.into()],
                    );
                    break;
                } else {
                    nFrag = nFrag.wrapping_add((x >> 16) - (prev & 0xffff) - 1);
                    prev = x;
                }
            }
            nFrag = nFrag.wrapping_add(usableSize.wrapping_sub(prev & 0xffff).wrapping_sub(1));
            /* EVIDENCE-OF: R-43263-13491 The total number of bytes in all fragments
             ** is stored in the fifth field of the b-tree page header.
             ** EVIDENCE-OF: R-07161-27322 The one-byte integer at offset 7 gives the
             ** number of fragmented free bytes within the cell content area.
             */
            if pCheck.heap[0] == 0 && nFrag != data[hdr + 7] as u32 {
                checkAppendMsg(
                    pCheck,
                    b"Fragmentation of %u bytes reported as %u on page %u",
                    &[nFrag.into(), (data[hdr + 7] as u32).into(), iPage.into()],
                );
            }
        }
    }

    /* end_of_check: */
    if let Some(pg) = &pPage {
        if !doCoverageCheck {
            pg.borrow_mut().pExtra.isInit = savedIsInit;
        }
    }
    releasePage(pPage.as_ref());
    pCheck.zPfx = saved_zPfx;
    pCheck.v1 = saved_v1;
    pCheck.v2 = saved_v2;
    pCheck.nAbove -= 1;
    depth + 1
}

/// C: `int sqlite3BtreeIntegrityCheck( sqlite3 *db, Btree *p, Pgno *aRoot, Mem *aCnt, int nRoot, int mxErr, int *pnErr, char **pzOut )`
///
/// This routine does a complete check of the given BTree file.  aRoot[] is
/// an array of pages numbers were each page number is the root page of
/// a table.  nRoot is the number of entries in aRoot.
///
/// A read-only or read-write transaction must be opened before calling
/// this function.
///
/// Write the number of error seen in *pnErr.  Except for some memory
/// allocation errors,  an error message held in memory obtained from
/// malloc is returned if *pnErr is non-zero.  If *pnErr==0 then NULL is
/// returned.  If a memory allocation error occurs, NULL is returned.
///
/// If the first entry in aRoot[] is 0, that indicates that the list of
/// root pages is incomplete.  This is a "partial integrity-check".  This
/// happens when performing an integrity check on a single table.  The
/// zero is skipped, of course.  But in addition, the freelist checks
/// and the checks to make sure every page is referenced are also skipped,
/// since obviously it is not possible to know which pages are covered by
/// the unverified btrees.  Except, if aRoot[1] is 1, then the freelist
/// checks are still performed.
pub fn sqlite3BtreeIntegrityCheck(
    db: &mut Sqlite3,
    p: &BtreeP,
    aRoot: &[Pgno],
    aCnt: &mut [Mem],
    nRoot: i32,
    mxErr: i32,
    pnErr: &mut i32,
    pzOut: &mut Option<ZStr>,
) -> i32 {
    let ctx = (
        db.isInterrupted.clone(),
        db.xProgress.clone(),
        db.nProgressOps,
    );
    btreeIntegrityCheck(&ctx, p, aRoot, aCnt, nRoot, mxErr, pnErr, pzOut)
}

/// The connection state used by the integrity check: (db->u1.isInterrupted,
/// db->xProgress, db->nProgressOps).
pub type BtCheckCtx = (Rc<std::cell::Cell<i32>>, Option<Rc<dyn Fn() -> i32>>, u32);

/// sqlite3BtreeIntegrityCheck() body.
pub fn btreeIntegrityCheck(
    db: &BtCheckCtx,
    p: &BtreeP,
    aRoot: &[Pgno],
    aCnt: &mut [Mem],
    nRoot: i32,
    mxErr: i32,
    pnErr: &mut i32,
    pzOut: &mut Option<ZStr>,
) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let savedBtFlags = pBt.borrow().dbFlags;
    let mut bPartial = false; /* True if not checking all btrees */
    let mut bCkFreelist = true; /* True to scan the freelist */

    debug_assert!(nRoot > 0);

    /* aRoot[0]==0 means this is a partial check */
    if aRoot[0] == 0 {
        debug_assert!(nRoot > 1);
        bPartial = true;
        if aRoot[1] != 1 {
            bCkFreelist = false;
        }
    }

    debug_assert!(p.borrow().inTrans > TRANS_NONE as u8);
    let mut sCheck = IntegrityCk {
        pBt: pBt.clone(),
        pPager: pagerOf(&pBt),
        aPgRef: Vec::new(),
        nCkPage: btreePagecount(&pBt),
        mxErr,
        nErr: 0,
        rc: SQLITE_OK,
        nStep: 0,
        nAbove: 0,
        zPfx: None,
        v0: 0,
        v1: 0,
        v2: 0,
        errMsg: StrAccum::default(),
        heap: Vec::new(),
        isInterrupted: db.0.clone(),
        xProgress: db.1.clone(),
        nProgressOps: db.2,
        nRow: 0,
    };
    crate::printf::sqlite3StrAccumInit(&mut sCheck.errMsg, 100, SQLITE_MAX_LENGTH);
    sCheck.errMsg.printfFlags = SQLITE_PRINTF_INTERNAL;
    'integrity_ck_cleanup: {
        if sCheck.nCkPage == 0 {
            break 'integrity_ck_cleanup;
        }

        sCheck.aPgRef = vec![0u8; (sCheck.nCkPage / 8) as usize + 1];
        let pageSize = pBt.borrow().pageSize as usize;
        sCheck.heap = vec![0u32; pageSize / 2];

        let i = pendingBytePage(&pBt);
        if i <= sCheck.nCkPage {
            setPageReferenced(&mut sCheck, i);
        }

        /* Check the integrity of the freelist
         */
        if bCkFreelist {
            sCheck.zPfx = Some(b"Freelist: ");
            let (first, n) = (page1Get4(&pBt, 32), page1Get4(&pBt, 36));
            checkList(&mut sCheck, 1, first, n);
            sCheck.zPfx = None;
        }

        /* Check all the tables.
         */
        let autoVacuum = pBt.borrow().autoVacuum;
        if !bPartial {
            if autoVacuum != 0 {
                let mut mx: Pgno = 0;
                for &r in aRoot.iter().take(nRoot as usize) {
                    if mx < r {
                        mx = r;
                    }
                }
                let mxInHdr = page1Get4(&pBt, 52);
                if mx != mxInHdr {
                    checkAppendMsg(
                        &mut sCheck,
                        b"max rootpage (%u) disagrees with header (%u)",
                        &[mx.into(), mxInHdr.into()],
                    );
                }
            } else if page1Get4(&pBt, 64) != 0 {
                checkAppendMsg(
                    &mut sCheck,
                    b"incremental_vacuum enabled with a max rootpage of zero",
                    &[],
                );
            }
        }
        /* pBt->db->flags &= ~(u64)SQLITE_CellSizeCk; */
        pBt.borrow_mut().dbFlags = savedBtFlags & !SQLITE_CellSizeCk;
        let mut i = 0usize;
        while (i as i32) < nRoot && sCheck.mxErr != 0 {
            sCheck.nRow = 0;
            if aRoot[i] != 0 {
                let mut notUsed: i64 = 0;
                if autoVacuum != 0 && aRoot[i] > 1 && !bPartial {
                    checkPtrmap(&mut sCheck, aRoot[i], PTRMAP_ROOTPAGE as u8, 0);
                }
                sCheck.v0 = aRoot[i];
                debug_assert!(sCheck.nAbove == 0);
                checkTreePage(&mut sCheck, aRoot[i], &mut notUsed, LARGEST_INT64);
                debug_assert!(sCheck.nAbove == 0);
            }
            if i < aCnt.len() {
                crate::vdbemem::sqlite3VdbeMemSetInt64(&mut aCnt[i], sCheck.nRow);
            }
            i += 1;
        }
        /* pBt->db->flags = savedDbFlags; */
        pBt.borrow_mut().dbFlags = savedBtFlags;

        /* Make sure every page in the file is referenced
         */
        if !bPartial {
            let mut i: Pgno = 1;
            while i <= sCheck.nCkPage && sCheck.mxErr != 0 {
                /* If the database supports auto-vacuum, make sure no tables contain
                 ** references to pointer-map pages.
                 */
                if !getPageReferenced(&sCheck, i)
                    && (PTRMAP_PAGENO(&pBt, i) != i || autoVacuum == 0)
                {
                    checkAppendMsg(&mut sCheck, b"Page %u: never used", &[i.into()]);
                }
                if getPageReferenced(&sCheck, i) && (PTRMAP_PAGENO(&pBt, i) == i && autoVacuum != 0)
                {
                    checkAppendMsg(&mut sCheck, b"Page %u: pointer map referenced", &[i.into()]);
                }
                i += 1;
            }
        }
    }

    /* Clean  up and report errors.
     */
    /* integrity_ck_cleanup: */
    *pnErr = sCheck.nErr;
    if sCheck.nErr == 0 {
        crate::printf::sqlite3_str_reset(&mut sCheck.errMsg);
        *pzOut = None;
    } else {
        *pzOut = crate::printf::sqlite3StrAccumFinish(&mut sCheck.errMsg);
    }
    sCheck.rc
}

// ---------------------------------------------------------------------------
// Miscellaneous interfaces
// ---------------------------------------------------------------------------

/// C: `const char * sqlite3BtreeGetFilename(Btree *p)`
///
/// Return the full pathname of the underlying database file.  Return
/// an empty string if the database is in-memory or a TEMP database.
///
/// The pager filename is invariant as long as the pager is
/// open so it is safe to access without the BtShared mutex.
pub fn sqlite3BtreeGetFilename(p: &BtreeP) -> Option<ZStr> {
    let pager = sqlite3BtreePager(p);
    let z = sqlite3PagerFilename(&pager.borrow(), 1);
    z
}

/// C: `const char * sqlite3BtreeGetJournalname(Btree *p)`
///
/// Return the pathname of the journal file for this database. The return
/// value of this routine is the same regardless of whether the journal file
/// has been created or not.
///
/// The pager journal filename is invariant as long as the pager is
/// open so it is safe to access without the BtShared mutex.
pub fn sqlite3BtreeGetJournalname(p: &BtreeP) -> Option<ZStr> {
    let pager = sqlite3BtreePager(p);
    let z = sqlite3PagerJournalname(&mut pager.borrow_mut());
    z
}

/// C: `int sqlite3BtreeTxnState(Btree *p)`
///
/// Return one of SQLITE_TXN_NONE, SQLITE_TXN_READ, or SQLITE_TXN_WRITE
/// to describe the current transaction state of Btree p.
pub fn sqlite3BtreeTxnState(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    p.borrow().inTrans as i32
}

/// C: `int sqlite3BtreeCheckpoint(Btree *p, int eMode, int *pnLog, int *pnCkpt)`
///
/// Run a checkpoint on the Btree passed as the first argument.
///
/// Return SQLITE_LOCKED if this or any other connection has an open
/// transaction on the shared-cache the argument Btree is connected to.
///
/// Parameter eMode is one of SQLITE_CHECKPOINT_PASSIVE, FULL or RESTART.
pub fn sqlite3BtreeCheckpoint(
    db: &mut Sqlite3,
    p: &BtreeP,
    eMode: i32,
    pnLog: &mut i32,
    pnCkpt: &mut i32,
) -> i32 {
    let pBt = p.borrow().pBt.clone();
    if pBt.borrow().inTransaction != TRANS_NONE as u8 {
        SQLITE_LOCKED
    } else {
        let pager = pagerOf(&pBt);
        let rc = sqlite3PagerCheckpoint(db, &mut pager.borrow_mut(), eMode, pnLog, pnCkpt);
        rc
    }
}

/// C: `int sqlite3BtreeIsInBackup(Btree *p)`
///
/// Return true if there is currently a backup running on Btree p.
pub fn sqlite3BtreeIsInBackup(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    (p.borrow().nBackup != 0) as i32
}

/// C: `void * sqlite3BtreeSchema(Btree *p, int nBytes, void(*xFree)(void *))`
///
/// This function returns a pointer to a blob of memory associated with
/// a single shared-btree. The memory is used by client code for its own
/// purposes (for example, to store a high-level schema associated with
/// the shared-btree). The btree layer manages reference counting issues.
///
/// Rust: without shared cache the Schema object is owned by the Db entry
/// of the connection (Db::pSchema), so no blob is ever attached to the
/// BtShared and this always returns None.
pub fn sqlite3BtreeSchema(
    p: &BtreeP,
    nBytes: i32,
    xFree: /*fnptr: void(*xFree)(void *)*/ (),
) -> Option<Vec<u8>> {
    None
}

/// C: `int sqlite3BtreeSchemaLocked(Btree *p)`
///
/// Return SQLITE_LOCKED_SHAREDCACHE if another user of the same shared
/// btree as the argument handle holds an exclusive lock on the
/// sqlite_schema table. Otherwise SQLITE_OK.  Always SQLITE_OK without
/// shared cache.
pub fn sqlite3BtreeSchemaLocked(db: &mut Sqlite3, p: &BtreeP) -> i32 {
    SQLITE_OK
}

/// C: `int sqlite3BtreeLockTable(Btree *p, int iTab, u8 isWriteLock)`
///
/// Obtain a lock on the table whose root page is iTab.  The
/// lock is a write lock if isWritelock is true or a read lock
/// if it is false.  Without shared cache this is a no-op.
pub fn sqlite3BtreeLockTable(p: &BtreeP, iTab: i32, isWriteLock: u8) -> i32 {
    debug_assert!(p.borrow().inTrans != TRANS_NONE as u8);
    SQLITE_OK
}

/// C: `int sqlite3BtreePutData(BtCursor *pCsr, u32 offset, u32 amt, void *z)`
///
/// Argument pCsr must be a cursor opened for writing on an
/// INTKEY table currently pointing at a valid table entry.
/// This function modifies the data stored as part of that entry.
///
/// Only the data content may only be modified, it is not possible to
/// change the length of the data stored. If this function is called with
/// parameters that attempt to write past the end of the existing data,
/// no modifications are made and SQLITE_CORRUPT is returned.
pub fn sqlite3BtreePutData(
    db: &mut Sqlite3,
    pCsr: &BtCursorP,
    offset: u32,
    amt: u32,
    z: &mut [u8],
) -> i32 {
    btreePutData(pCsr, offset, amt, z)
}

/// sqlite3BtreePutData() body.
pub fn btreePutData(pCsr: &BtCursorP, offset: u32, amt: u32, z: &mut [u8]) -> i32 {
    let mut c = pCsr.borrow_mut();
    let pCsr = &mut *c;
    debug_assert!(pCsr.curFlags as i32 & BTCF_Incrblob != 0);

    let rc = restoreCursorPosition(pCsr);
    if rc != SQLITE_OK {
        return rc;
    }
    debug_assert!(pCsr.eState != CURSOR_REQUIRESEEK as u8);
    if pCsr.eState != CURSOR_VALID as u8 {
        return SQLITE_ABORT;
    }

    /* Save the positions of all other cursors open on this table. This is
     ** required in case any of them are holding references to an xFetch
     ** version of the b-tree page modified by the accessPayload call below.
     **
     ** Note that pCsr must be open on a INTKEY table and saveCursorPosition()
     ** and hence saveAllCursors() cannot fail on a BTREE_INTKEY table, hence
     ** saveAllCursors can only return SQLITE_OK.
     */
    let pBt = pCsr.bt().clone();
    let root = pCsr.pgnoRoot;
    let _ = saveAllCursors(&pBt, root, Some(&mut *pCsr));

    /* Check some assumptions:
     **   (a) the cursor is open for writing,
     **   (b) there is a read/write transaction open,
     **   (c) the connection holds a write-lock on the table (if required),
     **   (d) there are no conflicting read-locks, and
     **   (e) the cursor points at a valid row of an intKey table.
     */
    if (pCsr.curFlags as i32 & BTCF_WriteFlag) == 0 {
        return SQLITE_READONLY;
    }
    debug_assert!(
        (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) == 0
            && pBt.borrow().inTransaction == TRANS_WRITE as u8
    );
    debug_assert!(mp(pCsr.page()).intKey != 0);

    accessPayload(pCsr, offset, amt, z, 1)
}

/// C: `void sqlite3BtreeIncrblobCursor(BtCursor *pCur)`
///
/// Mark this cursor as an incremental blob cursor.
pub fn sqlite3BtreeIncrblobCursor(pCur: &BtCursorP) {
    let mut c = pCur.borrow_mut();
    c.curFlags |= BTCF_Incrblob as u8;
    c.btree().borrow_mut().hasIncrblobCur = 1;
}

/// C: `int sqlite3BtreeSetVersion(Btree *pBtree, int iVersion)`
///
/// Set both the "read version" (single byte at byte offset 18) and
/// "write version" (single byte at byte offset 19) fields in the database
/// header to iVersion.
pub fn sqlite3BtreeSetVersion(db: &mut Sqlite3, pBtree: &BtreeP, iVersion: i32) -> i32 {
    let pBt = pBtree.borrow().pBt.clone();
    let mut rc; /* Return code */

    debug_assert!(iVersion == 1 || iVersion == 2);

    /* If setting the version fields to 1, do not automatically open the
     ** WAL connection, even if the version fields are currently set to 2.
     */
    {
        let mut bt = pBt.borrow_mut();
        bt.btsFlags &= !(BTS_NO_WAL as u16);
        if iVersion == 1 {
            bt.btsFlags |= BTS_NO_WAL as u16;
        }
    }

    let mut iSchema = 0;
    rc = sqlite3BtreeBeginTrans(db, pBtree, 0, &mut iSchema);
    if rc == SQLITE_OK {
        let pPage1 = page1Of(&pBt);
        let differs = {
            let g = pPage1.borrow();
            g.aData[18] != iVersion as u8 || g.aData[19] != iVersion as u8
        };
        if differs {
            rc = sqlite3BtreeBeginTrans(db, pBtree, 2, &mut iSchema);
            if rc == SQLITE_OK {
                rc = sqlite3PagerWrite(&pPage1);
                if rc == SQLITE_OK {
                    let mut g = pPage1.borrow_mut();
                    g.aData[18] = iVersion as u8;
                    g.aData[19] = iVersion as u8;
                }
            }
        }
    }

    pBt.borrow_mut().btsFlags &= !(BTS_NO_WAL as u16);
    rc
}

/// C: `int sqlite3BtreeCursorHasHint(BtCursor *pCsr, unsigned int mask)`
///
/// Return true if the cursor has a hint specified.  This routine is
/// only used from within assert() statements
pub fn sqlite3BtreeCursorHasHint(pCsr: &BtCursorP, mask: u32) -> i32 {
    ((pCsr.borrow().hints as u32 & mask) != 0) as i32
}

/// C: `int sqlite3BtreeIsReadonly(Btree *p)`
///
/// Return true if the given Btree is read-only.
pub fn sqlite3BtreeIsReadonly(p: &BtreeP) -> i32 {
    let pBt = p.borrow().pBt.clone();
    let r = (pBt.borrow().btsFlags as i32 & BTS_READ_ONLY) != 0;
    r as i32
}

/// C: `int sqlite3HeaderSizeBtree(void)`
///
/// Return the size of the header added to each page by this module
/// (ROUND8(sizeof(MemPage)) on a 64-bit host).
pub fn sqlite3HeaderSizeBtree() -> i32 {
    SIZEOF_MEMPAGE
}

/// C: `void sqlite3BtreeClearCache(Btree *p)`
///
/// If no transaction is active and the database is not a temp-db, clear
/// the in-memory pager cache.
pub fn sqlite3BtreeClearCache(p: &BtreeP) {
    let pBt = p.borrow().pBt.clone();
    if pBt.borrow().inTransaction == TRANS_NONE as u8 {
        let pager = pagerOf(&pBt);
        sqlite3PagerClearCache(&mut pager.borrow_mut());
    }
}

/// C: `int sqlite3BtreeSharable(Btree *p)`
///
/// Return true if the Btree passed as the only argument is sharable.
/// (Always false: no shared cache.)
pub fn sqlite3BtreeSharable(p: &BtreeP) -> i32 {
    0
}

/// C: `int sqlite3BtreeConnectionCount(Btree *p)`
///
/// Return the number of connections to the BtShared object accessed by
/// the Btree handle passed as the only argument. (Always 1: no shared
/// cache.)
pub fn sqlite3BtreeConnectionCount(p: &BtreeP) -> i32 {
    1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! The scripts below were run through C SQLite 3.54.1's own btree layer
    //! (amalgamation built with SQLITE_OMIT_SHARED_CACHE, driving
    //! sqlite3BtreeOpen/Insert/Delete/... directly with the same op
    //! interpreter).  The expected outputs and the FNV-1a hashes of the
    //! resulting database files come from that run: the port must produce
    //! byte-identical files.  They need the pager, os and (for index
    //! b-trees) vdbeaux ports, hence #[ignore] until those exist.
    use super::*;
    use std::fmt::Write as _;

    fn fnv(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn payload(key: i64, n: usize) -> Vec<u8> {
        (0..n)
            .map(|j| (key.wrapping_mul(131).wrapping_add(j as i64 * 7 + 13) & 0xff) as u8)
            .collect()
    }

    /// An index record (INTEGER, BLOB) as built by the C test harness.
    fn mkrec(v: i64, len: usize) -> Vec<u8> {
        let st = 12 + 2 * len as u64;
        let mut t = [0u8; 10];
        let nt = putVarint(&mut t, 0, st);
        let mut out = vec![(2 + nt) as u8, 6];
        out.extend_from_slice(&t[..nt]);
        out.extend_from_slice(&v.to_be_bytes());
        for j in 0..len {
            out.push((v.wrapping_mul(17).wrapping_add(j as i64 * 5 + 3) & 0xff) as u8);
        }
        out
    }

    fn cksum(buf: &[u8], sum: i64) -> i64 {
        let mut s = sum;
        for (j, &x) in buf.iter().enumerate() {
            s = s.wrapping_add(x as i64 * (j as i64 + 1));
        }
        s
    }

    fn chk(rc: i32, what: &str, line: usize) {
        assert_eq!(rc, SQLITE_OK, "line {}: {}", line, what);
    }

    /// Run an op script on a file database; return (output, file content).
    fn run_script(name: &str, script: &str) -> (String, Vec<u8>) {
        let path =
            std::env::temp_dir().join(format!("goro-btree-{}-{}.db", name, std::process::id()));
        let file = path.to_str().unwrap().to_string();
        let st = BtDbState {
            flags: 0,
            tempInMemory: 0,
            writableSchema: 0,
            nSavepoint: 0,
            nVdbeRead: 0,
            szMmap: 0,
            busyHandler: Rc::new(RefCell::new(BusyHandler {
                xBusyHandler: None,
                nBusy: 0,
            })),
        };
        let ki: KeyInfoRef = Rc::new(RefCell::new(KeyInfo {
            enc: SQLITE_UTF8 as u8,
            nKeyField: 2,
            nAllField: 2,
            aSortFlags: vec![0; 2],
            aColl: vec![None, None],
        }));
        let vfs: Rc<crate::os::Vfs> =
            Rc::from(crate::os::sqlite3_vfs_find(None).expect("default vfs"));
        let mut out = String::new();
        let mut p: Option<BtreeP> = None;
        let mut roots: Vec<Pgno> = Vec::new();
        let mut types: Vec<u8> = Vec::new();
        let mut saved: Vec<Pgno> = Vec::new();
        let mut savedT: Vec<u8> = Vec::new();
        let mut curT = 0usize;
        for (ln, line) in script.lines().enumerate() {
            let lineno = ln + 1;
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.is_empty() {
                continue;
            }
            let mut a: i64 = 0;
            if let Some(x) = w.get(1) {
                if let Some(t) = x.strip_prefix('T') {
                    let ti: usize = t.parse().unwrap();
                    if ti >= roots.len() || roots[ti] == 0 {
                        continue;
                    }
                    a = roots[ti] as i64;
                    curT = ti;
                } else {
                    a = x.parse().unwrap();
                }
            }
            let b: i64 = w.get(2).map_or(0, |x| x.parse().unwrap());
            let c: i64 = w.get(3).map_or(0, |x| x.parse().unwrap());
            let isIdx = types.get(curT).copied().unwrap_or(0) != 0;
            let kinfo = if isIdx { Some(ki.clone()) } else { None };
            let pp = p.clone();
            match w[0] {
                "open" => {
                    let _ = std::fs::remove_file(&path);
                    chk(
                        btreeOpen(
                            &st,
                            &vfs,
                            Some(file.as_bytes()),
                            &mut p,
                            0,
                            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB,
                        ),
                        "open",
                        lineno,
                    );
                    let pp = p.as_ref().unwrap();
                    chk(
                        sqlite3BtreeSetPageSize(pp, a as i32, 0, 0),
                        "pagesize",
                        lineno,
                    );
                    chk(sqlite3BtreeSetAutoVacuum(pp, b as i32), "autovac", lineno);
                    if c != 0 {
                        sqlite3BtreeSecureDelete(pp, c as i32);
                    }
                }
                "begin" => {
                    let mut sv = 0;
                    chk(
                        btreeBeginTransFull(&st, pp.as_ref().unwrap(), 1, &mut sv),
                        "begin",
                        lineno,
                    );
                    saved = roots.clone();
                    savedT = types.clone();
                }
                "commit" => {
                    let pp = pp.as_ref().unwrap();
                    chk(btreeCommitPhaseOne(None, pp, None), "commit", lineno);
                    chk(btreeCommitPhaseTwo(pp, 0, 0), "commit", lineno);
                }
                "rollback" => {
                    chk(
                        btreeRollback(pp.as_ref().unwrap(), SQLITE_OK, 0, 0),
                        "rollback",
                        lineno,
                    );
                    roots = saved.clone();
                    types = savedT.clone();
                }
                "stmt" => {
                    let pager = sqlite3BtreePager(pp.as_ref().unwrap());
                    chk(
                        sqlite3PagerOpenSavepoint(&mut pager.borrow_mut(), a as i32),
                        "stmt",
                        lineno,
                    );
                }
                "sprollback" => chk(
                    sqlite3BtreeSavepoint(pp.as_ref().unwrap(), SAVEPOINT_ROLLBACK, a as i32),
                    "sprollback",
                    lineno,
                ),
                "sprelease" => chk(
                    sqlite3BtreeSavepoint(pp.as_ref().unwrap(), SAVEPOINT_RELEASE, a as i32),
                    "sprelease",
                    lineno,
                ),
                "create" | "icreate" => {
                    let mut t: Pgno = 0;
                    let f = if w[0] == "create" {
                        BTREE_INTKEY
                    } else {
                        BTREE_BLOBKEY
                    };
                    chk(
                        sqlite3BtreeCreateTable(pp.as_ref().unwrap(), &mut t, f),
                        "create",
                        lineno,
                    );
                    writeln!(out, "{} {}", w[0], t).unwrap();
                    roots.push(t);
                    types.push((w[0] == "icreate") as u8);
                }
                "insert" | "delete" | "putdata" => {
                    let mut cur = None;
                    chk(
                        sqlite3BtreeCursor(
                            pp.as_ref().unwrap(),
                            a as Pgno,
                            BTREE_WRCSR,
                            None,
                            &mut cur,
                        ),
                        "cursor",
                        lineno,
                    );
                    let cur = cur.unwrap();
                    if w[0] == "insert" {
                        let buf = payload(b, c as usize);
                        let x = BtreePayload {
                            pKey: None,
                            nKey: b,
                            pData: Some(&buf),
                            aMem: None,
                            nMem: 0,
                            nData: c as i32,
                            nZero: 0,
                        };
                        chk(sqlite3BtreeInsert(&cur, &x, 0, 0), "insert", lineno);
                    } else {
                        if w[0] == "putdata" {
                            sqlite3BtreeIncrblobCursor(&cur);
                        }
                        let mut res = 0;
                        chk(
                            btreeTableMoveto(&mut cur.borrow_mut(), b, 0, &mut res),
                            "moveto",
                            lineno,
                        );
                        if res == 0 && w[0] == "delete" {
                            chk(sqlite3BtreeDelete(&cur, 0), "delete", lineno);
                        } else if res == 0 {
                            let sz = sqlite3BtreePayloadSize(&cur);
                            let off = (c as u64 % (sz as u64 + 1)) as u32;
                            let amt = std::cmp::min(sz - off, 3000);
                            let mut v: Vec<u8> = (0..amt)
                                .map(|j| (j.wrapping_mul(3).wrapping_add(c as u32)) as u8)
                                .collect();
                            chk(btreePutData(&cur, off, amt, &mut v), "putdata", lineno);
                            writeln!(out, "putdata {} {}", off, amt).unwrap();
                        }
                    }
                    sqlite3BtreeCloseCursor(&cur);
                }
                "iinsert" | "idelete" | "minsert" => {
                    let mut cur = None;
                    let rec = mkrec(b, c as usize);
                    chk(
                        sqlite3BtreeCursor(
                            pp.as_ref().unwrap(),
                            a as Pgno,
                            BTREE_WRCSR,
                            Some(ki.clone()),
                            &mut cur,
                        ),
                        "cursor",
                        lineno,
                    );
                    let cur = cur.unwrap();
                    let mut r = allocUnpackedRecord(&ki);
                    crate::vdbeaux::sqlite3VdbeRecordUnpack(rec.len() as i32, &rec, &mut r);
                    if w[0] == "idelete" {
                        let mut res = 0;
                        chk(
                            btreeIndexMoveto(&mut cur.borrow_mut(), &mut r, &mut res),
                            "imoveto",
                            lineno,
                        );
                        if res == 0 {
                            chk(sqlite3BtreeDelete(&cur, 0), "idelete", lineno);
                        }
                    } else {
                        let m = w[0] == "minsert";
                        let x = BtreePayload {
                            pKey: Some(&rec),
                            nKey: rec.len() as i64,
                            pData: None,
                            aMem: if m { Some(&r.aMem) } else { None },
                            nMem: if m { r.nField } else { 0 },
                            nData: 0,
                            nZero: 0,
                        };
                        chk(sqlite3BtreeInsert(&cur, &x, 0, 0), "iinsert", lineno);
                    }
                    sqlite3BtreeCloseCursor(&cur);
                }
                "drop" => {
                    let mut moved = 0;
                    chk(
                        sqlite3BtreeDropTable(pp.as_ref().unwrap(), a as i32, &mut moved),
                        "drop",
                        lineno,
                    );
                    writeln!(out, "drop {}", moved).unwrap();
                    roots[curT] = 0;
                    if moved != 0 {
                        for r in roots.iter_mut() {
                            if *r == moved as Pgno {
                                *r = a as Pgno;
                            }
                        }
                    }
                }
                "clear" => {
                    let mut nc = 0i64;
                    chk(
                        sqlite3BtreeClearTable(pp.as_ref().unwrap(), a as i32, &mut nc),
                        "clear",
                        lineno,
                    );
                    writeln!(out, "clear {}", nc).unwrap();
                }
                "incrvac" => loop {
                    let r = sqlite3BtreeIncrVacuum(pp.as_ref().unwrap());
                    if r != SQLITE_OK {
                        assert_eq!(r, SQLITE_DONE, "line {}: incrvac", lineno);
                        break;
                    }
                },
                "scan" | "rscan" => {
                    let mut cur = None;
                    chk(
                        sqlite3BtreeCursor(pp.as_ref().unwrap(), a as Pgno, 0, kinfo, &mut cur),
                        "cursor",
                        lineno,
                    );
                    let cur = cur.unwrap();
                    let (mut res, mut cnt, mut sum) = (0, 0i64, 0i64);
                    let fwd = w[0] == "scan";
                    if fwd {
                        chk(
                            cursorFirst(&mut cur.borrow_mut(), &mut res),
                            "first",
                            lineno,
                        );
                    } else {
                        chk(cursorLast(&mut cur.borrow_mut(), &mut res), "last", lineno);
                    }
                    while res == 0 && sqlite3BtreeEof(&cur) == 0 {
                        let sz = sqlite3BtreePayloadSize(&cur);
                        let mut v = vec![0u8; sz as usize];
                        chk(sqlite3BtreePayload(&cur, 0, sz, &mut v), "payload", lineno);
                        cnt += 1;
                        if !isIdx {
                            sum = sum.wrapping_add(sqlite3BtreeIntegerKey(&cur));
                        }
                        sum = cksum(&v, sum);
                        let rc = if fwd {
                            sqlite3BtreeNext(&cur, 0)
                        } else {
                            sqlite3BtreePrevious(&cur, 0)
                        };
                        if rc == SQLITE_DONE {
                            break;
                        }
                        chk(rc, "next", lineno);
                    }
                    writeln!(out, "{} {} {}", w[0], cnt, sum).unwrap();
                    sqlite3BtreeCloseCursor(&cur);
                }
                "count" => {
                    let mut cur = None;
                    chk(
                        sqlite3BtreeCursor(pp.as_ref().unwrap(), a as Pgno, 0, kinfo, &mut cur),
                        "cursor",
                        lineno,
                    );
                    let cur = cur.unwrap();
                    let mut n = 0i64;
                    chk(
                        btreeCount(&cur, &mut n, &std::cell::Cell::new(0)),
                        "count",
                        lineno,
                    );
                    writeln!(out, "count {}", n).unwrap();
                    sqlite3BtreeCloseCursor(&cur);
                }
                "delscan" => {
                    let mut cur = None;
                    chk(
                        sqlite3BtreeCursor(
                            pp.as_ref().unwrap(),
                            a as Pgno,
                            BTREE_WRCSR,
                            kinfo,
                            &mut cur,
                        ),
                        "cursor",
                        lineno,
                    );
                    let cur = cur.unwrap();
                    let (mut res, mut cnt, mut nd, mut i) = (0, 0i64, 0i64, 0i64);
                    chk(
                        cursorFirst(&mut cur.borrow_mut(), &mut res),
                        "first",
                        lineno,
                    );
                    while res == 0 && sqlite3BtreeEof(&cur) == 0 {
                        let k = i;
                        i += 1;
                        if k % b == 0 {
                            chk(
                                sqlite3BtreeDelete(&cur, BTREE_SAVEPOSITION as u8),
                                "delete",
                                lineno,
                            );
                            nd += 1;
                        } else {
                            cnt += 1;
                        }
                        let rc = sqlite3BtreeNext(&cur, 0);
                        if rc == SQLITE_DONE {
                            break;
                        }
                        chk(rc, "next", lineno);
                    }
                    writeln!(out, "delscan {} {}", cnt, nd).unwrap();
                    sqlite3BtreeCloseCursor(&cur);
                }
                "multi" => {
                    let (mut ca, mut cb) = (None, None);
                    chk(
                        sqlite3BtreeCursor(pp.as_ref().unwrap(), a as Pgno, 0, None, &mut ca),
                        "cursorA",
                        lineno,
                    );
                    chk(
                        sqlite3BtreeCursor(
                            pp.as_ref().unwrap(),
                            a as Pgno,
                            BTREE_WRCSR,
                            None,
                            &mut cb,
                        ),
                        "cursorB",
                        lineno,
                    );
                    let (ca, cb) = (ca.unwrap(), cb.unwrap());
                    let mut res = 0;
                    chk(cursorFirst(&mut ca.borrow_mut(), &mut res), "first", lineno);
                    for _ in 0..3 {
                        if res != 0 {
                            break;
                        }
                        let rc = sqlite3BtreeNext(&ca, 0);
                        if rc == SQLITE_DONE {
                            res = 1;
                            break;
                        }
                        chk(rc, "next", lineno);
                    }
                    let buf = payload(b, c as usize);
                    let x = BtreePayload {
                        pKey: None,
                        nKey: b,
                        pData: Some(&buf),
                        aMem: None,
                        nMem: 0,
                        nData: c as i32,
                        nZero: 0,
                    };
                    chk(sqlite3BtreeInsert(&cb, &x, 0, 0), "insert", lineno);
                    if res == 0 {
                        let mut dr = 0;
                        if sqlite3BtreeCursorHasMoved(&ca) != 0 {
                            chk(sqlite3BtreeCursorRestore(&ca, &mut dr), "restore", lineno);
                        }
                        let (mut cnt, mut sum) = (0i64, 0i64);
                        while sqlite3BtreeEof(&ca) == 0 {
                            let sz = sqlite3BtreePayloadSize(&ca);
                            let mut v = vec![0u8; sz as usize];
                            chk(sqlite3BtreePayload(&ca, 0, sz, &mut v), "payload", lineno);
                            cnt += 1;
                            sum = sum.wrapping_add(sqlite3BtreeIntegerKey(&ca));
                            sum = cksum(&v, sum);
                            let rc = sqlite3BtreeNext(&ca, 0);
                            if rc == SQLITE_DONE {
                                break;
                            }
                            chk(rc, "next", lineno);
                        }
                        writeln!(out, "multi {} {} {}", cnt, sum, dr).unwrap();
                    }
                    sqlite3BtreeCloseCursor(&ca);
                    sqlite3BtreeCloseCursor(&cb);
                }
                "xfer" => {
                    let bi = b as usize;
                    if bi < roots.len() && roots[bi] != 0 {
                        let dst = roots[bi];
                        let (mut cs, mut cd) = (None, None);
                        chk(
                            sqlite3BtreeCursor(pp.as_ref().unwrap(), a as Pgno, 0, None, &mut cs),
                            "cursorS",
                            lineno,
                        );
                        chk(
                            sqlite3BtreeCursor(
                                pp.as_ref().unwrap(),
                                dst,
                                BTREE_WRCSR,
                                None,
                                &mut cd,
                            ),
                            "cursorD",
                            lineno,
                        );
                        let (cs, cd) = (cs.unwrap(), cd.unwrap());
                        let (mut res, mut n) = (0, 0i64);
                        chk(cursorFirst(&mut cs.borrow_mut(), &mut res), "first", lineno);
                        while res == 0 && sqlite3BtreeEof(&cs) == 0 {
                            let k = sqlite3BtreeIntegerKey(&cs) + 1000000000;
                            let mut r2 = 0;
                            chk(
                                btreeTableMoveto(&mut cd.borrow_mut(), k, 0, &mut r2),
                                "moveto",
                                lineno,
                            );
                            chk(sqlite3BtreeTransferRow(&cd, &cs, k), "xfer", lineno);
                            let x = BtreePayload {
                                pKey: None,
                                nKey: k,
                                pData: None,
                                aMem: None,
                                nMem: 0,
                                nData: 0,
                                nZero: 0,
                            };
                            chk(
                                sqlite3BtreeInsert(&cd, &x, BTREE_PREFORMAT, r2),
                                "xinsert",
                                lineno,
                            );
                            n += 1;
                            let rc = sqlite3BtreeNext(&cs, 0);
                            if rc == SQLITE_DONE {
                                break;
                            }
                            chk(rc, "next", lineno);
                        }
                        sqlite3BtreeCloseCursor(&cs);
                        sqlite3BtreeCloseCursor(&cd);
                        writeln!(out, "xfer {}", n).unwrap();
                    }
                }
                "check" => {
                    let mut aRoot = vec![1 as Pgno];
                    aRoot.extend(roots.iter().copied().filter(|&r| r != 0));
                    let mut aCnt: Vec<Mem> = (0..aRoot.len()).map(|_| Mem::default()).collect();
                    let (mut nErr, mut zErr) = (0, None);
                    let ctx: BtCheckCtx = (Rc::new(std::cell::Cell::new(0)), None, 0);
                    let n = aRoot.len() as i32;
                    btreeIntegrityCheck(
                        &ctx,
                        pp.as_ref().unwrap(),
                        &aRoot,
                        &mut aCnt,
                        n,
                        100,
                        &mut nErr,
                        &mut zErr,
                    );
                    writeln!(
                        out,
                        "check {} {}",
                        nErr,
                        String::from_utf8_lossy(&zErr.unwrap_or_default())
                    )
                    .unwrap();
                }
                "close" => {
                    btreeClose(None, pp.as_ref().unwrap(), 0);
                    p = None;
                }
                _ => panic!("bad op {}", w[0]),
            }
        }
        let data = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        (out, data)
    }

    fn check_golden(name: &str, script: &str, output: &str, size: usize, hash: u64) {
        let (out, data) = run_script(name, script);
        assert_eq!(out, output, "{}: output", name);
        assert_eq!(data.len(), size, "{}: file size", name);
        assert_eq!(fnv(&data), hash, "{}: file content", name);
    }

    #[test]
    fn varints_and_cell_parsing() {
        let mut a = [0u8; 16];
        for &v in &[
            0u64,
            1,
            127,
            128,
            0x3fff,
            0x4000,
            0x1fffff,
            0xffffffff,
            1 << 56,
            u64::MAX,
        ] {
            let n = putVarint(&mut a, 0, v);
            let mut x = 0u64;
            assert_eq!(getVarint(&a, 0, &mut x) as usize, n);
            assert_eq!(x, v);
        }
        /* A table-leaf cell: payload size 3, rowid 300, payload "abc" */
        let mut p = MemPage::default();
        p.leaf = 1;
        p.intKey = 1;
        p.intKeyLeaf = 1;
        p.maxLocal = 4061;
        p.minLocal = 489;
        p.usableSize = 4096;
        let cell = [3u8, 0x82, 0x2c, b'a', b'b', b'c'];
        let mut info = CellInfo::default();
        btreeParseCellPtr(&p, &cell, 0, &mut info);
        assert_eq!(info.nKey, 300);
        assert_eq!(info.nPayload, 3);
        assert_eq!(info.pPayload, 3);
        assert_eq!(info.nLocal, 3);
        assert_eq!(info.nSize, 6);
        assert_eq!(cellSizePtrTableLeaf(&p, &cell, 0), 6);
    }

    #[test]
    #[ignore = "needs the pager and os ports"]
    fn golden_p512() {
        check_golden("p512", SCRIPT_P512, OUTPUT_P512, SIZE_P512, HASH_P512);
    }

    #[test]
    #[ignore = "needs the pager and os ports"]
    fn golden_p1024_autovacuum() {
        check_golden(
            "p1024av",
            SCRIPT_P1024AV,
            OUTPUT_P1024AV,
            SIZE_P1024AV,
            HASH_P1024AV,
        );
    }

    #[test]
    #[ignore = "needs the pager and os ports"]
    fn golden_p4096_incremental_vacuum_secure_delete() {
        check_golden(
            "p4096incr",
            SCRIPT_P4096INCR,
            OUTPUT_P4096INCR,
            SIZE_P4096INCR,
            HASH_P4096INCR,
        );
    }

    #[test]
    #[ignore = "needs the pager and os ports"]
    fn golden_p65536_fast_secure_delete() {
        check_golden(
            "p65536sd",
            SCRIPT_P65536SD,
            OUTPUT_P65536SD,
            SIZE_P65536SD,
            HASH_P65536SD,
        );
    }

    #[test]
    #[ignore = "needs the pager, os and vdbeaux (record compare) ports"]
    fn golden_index_btrees() {
        check_golden(
            "idx1024",
            SCRIPT_IDX1024,
            OUTPUT_IDX1024,
            SIZE_IDX1024,
            HASH_IDX1024,
        );
    }

    const SCRIPT_P512: &str = r#"open 512 0 0
begin
create
insert T0 1243 10
insert T0 10002 40
insert T0 780 784
commit
begin
insert T0 10005 42
putdata T0 10006 10905
insert T0 10007 10
insert T0 70 0
insert T0 2904 4
insert T0 1275 26
delete T0 10000
rscan T0
insert T0 -2195769750095698722 24
insert T0 2428 56
create
insert T0 3533320345221537307 49
insert T1 370 10
create
insert T2 1582 10
insert T2 778 10
insert T1 10021 6
insert T1 2886 26
delete T1 2951
delete T1 2523
insert T0 10025 100
insert T0 2175 8
insert T1 4146697044969699026 887
insert T2 10028 46
insert T0 2237 2
delete T2 1400
insert T0 -4053309385737220568 756
create
create
insert T3 10034 47
insert T3 1111139375684537956 123
commit
begin
insert T3 47 9
insert T1 10038 59
insert T2 1027 100
delete T1 10024
insert T1 2034 57
delete T2 10007
insert T1 4498706519392328243 159
insert T0 2489 100
insert T4 557 12
commit
begin
insert T0 3624430482483355179 100
delete T1 10016
insert T3 10049 10
insert T4 10050 1164
insert T1 2959 28
insert T0 2330 128
delete T3 840
insert T0 1599 1781
insert T2 2943 526
insert T2 10056 124
delete T0 144
insert T1 2116 700
delete T3 2823
insert T4 10060 143
insert T4 2750 54
delete T2 10061
insert T3 2196 10
insert T0 10064 51
delete T0 270
delete T3 1939
insert T3 10067 100
insert T1 10068 657
insert T2 1456 100
insert T0 10212919511836003 135
delete T3 10020
insert T4 10072 54
insert T1 2572 46
insert T1 797 307
delete T0 398
insert T4 10076 100
insert T0 2796 52
delete T4 1407
insert T1 -2653133826474562342 100
insert T1 648 902
insert T3 397 29
insert T1 10082 10
delete T3 1517
insert T1 64 5
delete T3 783
insert T2 477 40
delete T2 10047
delete T3 758
delscan T3 1
create
insert T5 885 145
insert T4 -678494333385867497 101
delete T4 971
insert T0 10094 2
insert T2 10095 33
insert T5 2641 10
insert T3 693 724
insert T3 2158 19
insert T5 1162 75
commit
begin
insert T3 10101 57
insert T4 1277249992859841740 10
insert T1 404 910
delete T1 472
scan T0
insert T4 1489 42
insert T5 10107 49
insert T1 -2234424305587186379 10
insert T4 10109 17
insert T5 10110 61
insert T5 -4436888648879719889 27
commit
begin
insert T0 2272336289925885868 22
insert T5 305 114
delete T1 1058
insert T1 10116 1844
insert T1 1513 29
delete T0 2327
insert T2 -3208270682364247911 58
insert T2 2843 56
insert T5 481 28
insert T5 3842011658201664651 25
delete T4 1482
delete T5 1588
delete T2 10069
insert T1 1862 458
delete T0 2325
insert T3 2240 34
putdata T5 1286 11101
commit
begin
insert T3 903 58
insert T3 1364 126
insert T0 10133 590
delete T2 10096
delete T4 2146
insert T0 1723 421
insert T3 10137 24
insert T0 2994 10
delete T0 10004
insert T0 810 10
insert T3 10141 10
delete T2 2255
insert T3 -2906854039578789817 14
insert T1 -3542927408799629966 60
delete T2 1010
insert T1 1319 10
insert T2 10147 10
create
delete T2 2163
insert T3 2403 32
insert T3 2795 100
insert T4 1276 25
xfer T3 6
insert T5 2735 29
insert T0 10155 100
insert T2 10156 0
delete T5 2524
insert T4 10158 943
insert T2 1543 84
commit
begin
check
scan T0
rscan T0
scan T1
rscan T1
scan T2
rscan T2
scan T3
rscan T3
scan T4
rscan T4
scan T5
rscan T5
scan T6
rscan T6
commit
close"#;
    const OUTPUT_P512: &str = r#"create 2
rscan 8 39244086
create 6
create 7
create 11
create 12
delscan 0 7
create 35
scan 23 918884611667965077
create 51
xfer 11
check 2 Tree 12 page 28 cell 3: Bad cell header size
Tree 12 page 28 cell 2: Bad cell header size
scan 29 3191220901627895277
rscan 29 3191220901627895277
scan 23 214918023955613537
rscan 23 214918023955613537
scan 14 -3208270682343248547
rscan 14 -3208270682343248547
scan 11 -2906854039543242222
rscan 11 -2906854039543242222
scan 12 598755659620248058
rscan 12 598755659620248058
scan 10 -594876990674849335
rscan 10 -594876990674849335
scan 11 -2906854028543242222
rscan 11 -2906854028543242222
"#;
    const SIZE_P512: usize = 28672;
    const HASH_P512: u64 = 0x7acf49a0d2e31215;

    const SCRIPT_P1024AV: &str = r#"open 1024 1 0
begin
create
insert T0 10001 285
create
insert T0 818 100
insert T0 249 338
insert T1 -3069486679110611024 26
insert T1 10006 42
delete T1 1488
delete T1 963
insert T0 1762 10
insert T1 2488 10
insert T0 2042 40
insert T1 1155 44
delete T0 2249
insert T0 -1299757299593672633 10
insert T0 -1756007392670390953 100
delete T1 941
delete T1 1076
insert T0 2527 910
insert T1 10019 100
insert T0 10020 100
insert T1 241 39
insert T1 10022 37
insert T0 10023 23
insert T0 2909 10
delete T1 1555
delete T0 2823
insert T0 2454 201
insert T1 2807 43
insert T0 10029 223
insert T1 10030 3
insert T0 -4469195735775185019 1091
insert T1 2116 1367
delete T1 2052
delete T1 10003
delete T1 1802
insert T1 3000 100
delete T0 925
delete T1 1495
insert T1 2488 8
insert T1 2531 173
insert T1 -1478935987177012004 130
insert T0 2006899102207833489 3526
delete T1 10013
insert T1 701 10
insert T0 1142997230077341999 1
insert T1 10046 33
create
insert T1 10048 26
delete T1 636
insert T2 2796 280
insert T0 1815442656069318030 10
insert T0 -2523991961481855998 289
insert T2 10053 398
insert T1 10054 21
insert T1 -3003577186553123886 172
delete T2 10023
insert T1 10057 6
insert T1 10058 162
insert T1 2558 317
insert T1 2312 100
insert T1 -1423659391674976236 9
insert T1 1534 58
insert T1 2559 13
delete T2 582
insert T2 10065 100
insert T2 10066 676
delete T2 1796
insert T2 804 7
insert T1 10069 8
delete T1 1119
insert T0 10071 248
insert T2 1942017980295630602 8
commit
begin
delete T1 10019
insert T2 1062 53
scan T0
insert T0 2260 31
insert T0 935 33
delete T2 10024
insert T2 980 34
insert T1 10081 10
insert T1 1639 100
insert T2 34 190
insert T1 10084 39
insert T1 2111 268
putdata T0 10083 47616
insert T2 175 10
drop T2
insert T1 -4251884941181356382 144
insert T1 10090 5
insert T1 85 1068
insert T1 1457 36
insert T2 -4434311643402445938 55
insert T0 -840526775322499542 519
delete T0 10086
insert T0 1532 3
insert T1 10097 13
delete T0 2022
delete T2 1917
insert T1 -2125463588279896555 10
insert T0 2663 11
insert T1 3702765534444604375 27
insert T0 -1307053700381147876 73
putdata T2 2405 86883
insert T2 -1674591861025705194 1283
insert T2 2030 74
delete T2 2584
insert T0 10108 1457
delete T1 1846
delete T0 2281
insert T0 2184 100
insert T2 -3337613612664637568 207
commit
begin
delete T0 1520
insert T0 -1492987447700262993 51
insert T1 2882936894600135753 17
delete T0 944
insert T1 10118 21
create
insert T3 -2903316506471507460 662
insert T3 1221 824
check
insert T2 10123 2133
delete T0 1562
create
insert T0 968 3
insert T4 735490155159062276 10
commit
begin
delete T1 10030
insert T4 1958 10
insert T0 617 48
insert T3 2104 162
insert T1 10133 37
insert T3 -2766325652914516689 1515
insert T3 10135 41
insert T3 10136 25
insert T0 -1450582549486992755 119
delete T4 10018
insert T4 2020 56
insert T4 10140 83
insert T2 4547342767836042217 11
delete T0 10138
insert T4 2930 18
insert T3 272 2
insert T4 -4034154184854606917 10
insert T0 3363666770117590758 39
delete T3 1433
rscan T0
insert T4 2285 100
insert T1 2002 98
delete T4 2352
insert T3 2068 53
insert T3 908 100
insert T2 840 100
insert T0 1661 1394
delete T1 42
insert T3 10157 22
insert T0 2422 314
insert T3 2308 2
commit
begin
check
scan T0
rscan T0
scan T1
rscan T1
scan T2
rscan T2
scan T3
rscan T3
scan T4
rscan T4
commit
close"#;
    const OUTPUT_P1024AV: &str = r#"create 3
create 4
create 5
scan 19 -5083613400215300012
drop 0
create 5
check 2 Tree 3 page 16 cell 2: Bad cell header size
Tree 5 page 23 cell 0: Bad cell header size
create 6
rscan 32 -6811097102833942575
check 2 Tree 3 page 15 cell 4: Bad cell header size
Tree 5 page 23 cell 0: Bad cell header size
scan 34 -6811097102702960635
rscan 34 -6811097102702960635
scan 39 -8767305344717553854
rscan 39 -8767305344717553854
scan 11 -5669642159166179652
rscan 11 -5669642159166179652
scan 7 -3298664029694124451
rscan 7 -3298664029694124451
"#;
    const SIZE_P1024AV: usize = 30720;
    const HASH_P1024AV: u64 = 0x6851d658ef76bcda;

    const SCRIPT_P4096INCR: &str = r#"open 4096 2 1
begin
create
insert T0 10001 100
commit
begin
insert T0 123740369501555184 14
insert T0 10004 6596
insert T0 10005 47
delete T0 1202
insert T0 1073 810
delete T0 797
insert T0 10009 4557
create
insert T1 10011 100
insert T1 2583 45
insert T0 10013 10
insert T1 10014 11
insert T0 -3336205731524901083 38
insert T0 1716636528392441784 1078
create
insert T2 10018 5641
insert T0 636 34
insert T2 131 45
delete T1 882
insert T1 10022 27
insert T0 1887 10
stmt 1
delete T1 10012
insert T1 -2546859123187183812 10
insert T1 1424 100
insert T2 1863895620581311346 8
sprelease 0
delete T0 10028
insert T0 1523 100
delete T2 978
insert T1 1611 1120
delete T0 10023
insert T2 356 59
commit
begin
insert T0 -1650258722588318818 594
delete T0 1588
insert T0 16 34
insert T0 3064831266093199012 100
insert T2 3932147775606908476 100
insert T0 10042 10
stmt 1
insert T1 10044 113
insert T2 10045 10
delete T1 10009
insert T1 -447951206532930082 38
insert T2 1841 4149
insert T2 10049 58
stmt 2
insert T1 -2152424342878698559 464
insert T1 10052 10
insert T2 2096 100
delete T2 2343
insert T1 10055 24
insert T2 2376 100
delete T0 10028
insert T2 1026 10
insert T2 2300 13
insert T2 527 5908
insert T0 -3702587899453462672 100
insert T2 10062 5
delete T0 1429
insert T2 10064 187
insert T1 1075 37
insert T1 2932 10
insert T1 900 205
insert T0 2358 8
insert T1 -4574173567369268021 1003
insert T1 1727 7150
insert T1 1484 22
insert T2 694 116
delete T0 2260
insert T1 10075 25
insert T1 993 53
delete T0 10056
insert T2 10078 1142
insert T0 10082 54
insert T0 2325 25
insert T2 1733 10
insert T2 10085 58
delete T1 2006
insert T2 10087 3
insert T0 466 10
sprelease 0
insert T1 10090 10
delete T0 274
insert T0 10092 38
insert T0 2438 10
insert T2 2560 100
insert T0 2459 359
delete T2 2877
insert T0 10097 51
insert T0 10098 49
delete T1 2921
insert T1 1901 5953
insert T2 10101 10
insert T1 1638 8
commit
begin
commit
begin
xfer T0 1
insert T2 2928 10
delete T2 10010
delete T0 1397
insert T0 1198 584
insert T0 424 24
insert T1 -4463289866915123144 873
insert T1 501 661
create
insert T2 10114 880
create
insert T1 10116 785
insert T4 95 10
scan T2
delete T3 10080
insert T1 10120 58
drop T1
insert T3 1207 16
drop T1
delete T1 846
insert T0 2744362156624691840 100
insert T3 1728 4
insert T1 2829 34
clear T0
insert T4 1362 37
delete T3 10027
insert T3 -805805345624754868 100
insert T1 296 5041
stmt 1
delete T3 10046
insert T2 228861299931971886 89
insert T1 162634158900633989 6173
insert T1 10137 22
delete T2 10061
delete T0 1698
insert T3 10140 7125
insert T3 10141 100
delete T2 2576
insert T0 2470 10
delete T3 2036
delete T4 1919
insert T2 -1787497324637780077 40
insert T1 788363133608641697 39
delete T0 10104
insert T0 10149 55
delete T4 2258
insert T0 10152 100
insert T0 749 5509
insert T0 10154 28
delete T2 148
delete T3 586
sprollback 0
sprollback 0
delete T4 930
sprelease 0
commit
begin
check
scan T0
rscan T0
scan T1
rscan T1
scan T2
rscan T2
scan T3
rscan T3
scan T4
rscan T4
commit
begin
incrvac
check
commit
close"#;
    const OUTPUT_P4096INCR: &str = r#"create 3
create 4
create 5
xfer 26
create 6
create 7
scan 24 5796043401674280927
drop 7
clear 29
check 1 Tree 5 page 14 cell 8: Bad cell header size
scan 0 0
rscan 0 0
scan 24 5796043401674280927
rscan 24 5796043401674280927
scan 3 -805805345624110121
rscan 3 -805805345624110121
scan 2 133330
rscan 2 133330
check 1 Tree 5 page 8 cell 8: Bad cell header size
"#;
    const SIZE_P4096INCR: usize = 49152;
    const HASH_P4096INCR: u64 = 0x88dbdb5e1a59125c;

    const SCRIPT_P65536SD: &str = r#"open 65536 0 2
begin
create
insert T0 485 55
delete T0 2592
insert T0 2670 100
commit
begin
delete T0 10000
delete T0 2611
insert T0 10007 43
insert T0 1153 6
insert T0 115 100
insert T0 2808 100
insert T0 389 25
insert T0 587 17
insert T0 -4228083177231419956 36
insert T0 773 100
delete T0 1280
stmt 1
insert T0 -3963472476836344746 123419
insert T0 852 10879
insert T0 2441 5185
insert T0 1003 93402
insert T0 -4029627602550644387 100
insert T0 -910478404145308778 10
insert T0 10023 50
insert T0 1823 10
delete T0 910
delete T0 10019
insert T0 10027 30
insert T0 2786 21231
insert T0 472 130487
insert T0 10030 8
delete T0 1842
insert T0 1112 10
insert T0 -2553240508937412288 73979
insert T0 561 36
delete T0 1814
delete T0 10027
insert T0 -4053076113214713844 45
insert T0 10039 6394
insert T0 10040 42
sprollback 0
delete T0 10004
insert T0 10043 12
insert T0 2792 31
insert T0 2608 57
delete T0 632
sprollback 0
insert T0 10048 4305
sprollback 0
delete T0 10019
delete T0 1010
delete T0 2138
delete T0 1116
insert T0 927 58715
insert T0 2473 2516
insert T0 2226 100
insert T0 4187194156778175775 2644
delete T0 436
insert T0 265 11828
insert T0 1983 117328
sprelease 0
insert T0 2213 10
delete T0 10008
insert T0 10064 113750
delete T0 1000
delete T0 2097
insert T0 462 10
insert T0 1130 14
insert T0 -1277679228622946377 100
insert T0 990 16212
delete T0 10025
insert T0 -4085565480840562857 34
insert T0 10073 2274
delete T0 661
delete T0 2911
insert T0 10076 31991
insert T0 10077 15768
delete T0 10058
insert T0 10079 17
insert T0 2973 106925
delete T0 93
scan T0
insert T0 862 59
delete T0 844
delete T0 50
delete T0 10042
insert T0 10087 535
insert T0 388 6301
insert T0 1325 10
delete T0 1162
delete T0 1992
insert T0 10092 17
delete T0 1372
insert T0 1163 34
insert T0 -57364420750638267 10
delete T0 10077
insert T0 785 100
insert T0 1946831635235336435 19487
insert T0 668863226569972217 53951
rollback
begin
insert T0 2575477205097754425 60
insert T0 886 10532
insert T0 1196 60
insert T0 1968 100
insert T0 1561 20
insert T0 2529 43798
delete T0 10103
insert T0 408 100
insert T0 -4056229114568036630 118063
delete T0 10097
insert T0 1639 16
insert T0 863 46
insert T0 165 59499
insert T0 90 44364
insert T0 10115 128524
insert T0 10116 7063
commit
begin
insert T0 475 100
insert T0 1398 100
commit
begin
check
scan T0
rscan T0
commit
close"#;
    const OUTPUT_P65536SD: &str = r#"create 2
scan 28 -5404130970761615386
check 0 
scan 18 -1480749484095900093
rscan 18 -1480749484095900093
"#;
    const SIZE_P65536SD: usize = 655360;
    const HASH_P65536SD: u64 = 0xe77c781802035442;

    const SCRIPT_IDX1024: &str = r#"open 1024 1 0
begin
create
create
delete T0 1911
insert T1 846 1974
insert T1 1866 250
xfer T1 0
insert T1 270 60
delete T0 2829
delete T1 410
delete T0 2274
insert T0 2049 15
insert T1 1952 28
delete T1 473
insert T0 -2908279437072457117 2
insert T1 2048 55
delete T0 335
insert T1 10016 5
insert T1 230 154
insert T0 2492403601972925207 100
insert T0 1022 32
insert T1 11 115
insert T0 10021 60
insert T1 528 20
insert T1 10023 10
insert T1 -996781380243332415 35
insert T1 2282 321
insert T1 10026 173
insert T1 1381 100
insert T1 -688751500659392546 100
insert T1 10029 810
insert T0 926 56
insert T0 10031 30
delete T0 1733
insert T0 10033 4726
insert T0 2393 6037
insert T1 1901 16
insert T1 10036 7
insert T1 -671506389630243545 87
delete T0 2795
insert T1 1111 587
insert T0 2356 18
insert T0 2675163459765986180 2833
insert T1 10042 240
delete T1 1404
insert T0 10044 43
insert T0 2169 131
insert T1 -287367891578533893 248
delete T0 10035
delete T1 2449
rollback
begin
create
insert T0 1728 321
stmt 1
delete T0 268
insert T0 2560 100
insert T0 803 58
delete T0 2142
delete T0 120
insert T0 592 259
insert T0 3254125973428168104 335
sprelease 0
insert T0 -2610563093556768273 658
insert T0 1627 4
insert T0 10063 253
insert T0 863309068548142173 11
insert T0 547 15
insert T0 3250770862872898234 100
insert T0 10067 100
scan T0
insert T0 1499 37
delete T0 591
insert T0 10071 10
delete T0 895
rollback
begin
create
create
delete T0 10061
delete T1 10059
insert T0 10078 58
delete T0 1235
insert T1 1116817108218388158 34
delete T1 1269
insert T0 2698 810
insert T1 818 479
insert T1 908424022639456427 10
delete T0 2492
insert T1 10086 41
insert T0 -4183794895167397398 100
delete T0 521
insert T0 10089 5154
delete T0 594
delete T1 541
insert T0 277 10
delete T1 10066
insert T0 697 48
scan T1
insert T1 2379 45
insert T1 1043 19
insert T0 10098 59
delete T1 2018
delete T0 2534
insert T1 1735 16
insert T1 10102 10
insert T0 4298611475401193455 217
delete T1 984
delete T0 865
insert T1 2094 560
insert T0 10107 10
insert T0 3062402838980344433 10
insert T0 1881 1396
delete T0 10055
insert T1 2323 142
delete T1 154
insert T0 149 474
stmt 1
insert T0 1887380253184074101 56
delete T0 2621
insert T1 10117 264
insert T1 1487 27
delete T0 2327
insert T1 1277 1
delete T0 68
insert T0 4442473597279002014 10
insert T1 476 6011
sprollback 0
insert T0 865 99
insert T0 3359634254048269255 100
insert T1 10128 100
delete T0 1208
delete T0 1397
insert T1 10131 100
insert T0 2063 5
insert T0 -2132312756823990375 20
delete T0 10125
delete T1 2881
insert T1 1188 100
insert T1 2831 10
delete T1 832
insert T0 4168185255365264280 48
insert T1 463561005270027734 10
insert T0 1819 13
insert T0 2605 44
delete T1 2221
insert T0 917 334
delete T0 2718
insert T1 306 50
insert T1 1489 64
insert T0 468 9
insert T1 2213 2048
insert T0 524 2222
insert T1 2434 22
insert T1 -767939057675720500 64
insert T0 2874 217
delete T1 2482
insert T0 10157 102
insert T0 362 48
insert T1 91 142
insert T1 2786 29
insert T1 1075 170
insert T1 298 10
stmt 2
delete T0 2293
insert T1 -3661069011066581913 882
insert T0 10168 17
delete T1 1726
insert T0 10170 365
insert T0 -1093596909843520771 36
insert T1 10172 47
delete T0 227
insert T0 10174 1782
delete T1 2180
insert T1 10176 100
insert T0 -983813907346198463 7
insert T0 984113397119800219 100
insert T1 1801 10
insert T0 10180 23
insert T1 1158157478891518588 18
delete T0 10065
insert T0 1779 100
sprollback 1
insert T1 1801878635395945824 55
insert T1 10187 840
delete T1 288
insert T1 10189 39
insert T0 10190 1706
sprollback 1
delete T1 1426
insert T1 887743080280324193 27
delete T0 2420
delete T0 1842
insert T1 518 29
insert T1 10197 100
insert T0 10198 10
insert T1 10199 46
sprelease 0
commit
begin
check
scan T0
rscan T0
scan T1
rscan T1
commit
close"#;
    const OUTPUT_IDX1024: &str = r#"create 3
create 4
xfer 2
create 3
scan 12 4757642811344573478
create 3
create 4
scan 4 2025241130872642847
check 1 Tree 4 page 13 cell 1: Bad cell header size
scan 26 8572726174009144122
rscan 26 8572726174009144122
scan 28 2608606159042550215
rscan 28 2608606159042550215
"#;
    const SIZE_IDX1024: usize = 22528;
    const HASH_IDX1024: u64 = 0x6fc66a3ff96027eb;
}
