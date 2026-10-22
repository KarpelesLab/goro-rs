//! Port of pager.c: the page cache subsystem / rollback-journal pager.
//!
//! The pager is used to access a database disk file.  It implements
//! atomic commit and rollback through the use of a journal file that is
//! separate from the database file.  The pager also implements file
//! locking to prevent two processes from writing the same database file
//! simultaneously, or one process from reading the database while
//! another is writing.
//!
//! Rust-port notes (see PORTING.md §7):
//! * `PagerP = Rc<RefCell<Pager>>`; pages are `DbPage = Rc<RefCell<PgHdr>>`
//!   and reach their pager through `PgHdr::pPager` (Weak).  Page-level
//!   functions (`sqlite3PagerWrite(&DbPage)` ...) borrow the pager
//!   mutably: never call them while holding a borrow of the pager, nor
//!   while holding a borrow of the page itself.
//! * The xReiniter callback and the busy handler are invoked while the
//!   pager is mutably borrowed: they must not call back into the pager
//!   (the btree's pageReinit() only touches the page).
//! * Memory-mapped I/O is not supported (SQLITE_MAX_MMAP_SIZE==0) and the
//!   VFS has no shared memory, so WAL mode is never entered
//!   (sqlite3PagerWalSupported() returns 0): the WAL code paths are kept
//!   but only run when `pWal` is Some.
//! * The sqlite3_backup objects that must be told about page changes are
//!   registered (as Weak references) in the Vec returned by
//!   sqlite3PagerBackupPtr(); sqlite3BackupUpdate()/sqlite3BackupRestart()
//!   are called once per live registered backup.

use crate::bitvec::*;
use crate::int::*;
use crate::memjournal::*;
use crate::os::*;
use crate::pcache::*;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

// The Pager.eState variable stores the current 'state' of a pager.  A
// pager may be in any one of the seven states shown in the following
// state diagram (see pager.c for the full description).
pub const PAGER_OPEN: u8 = 0;
pub const PAGER_READER: u8 = 1;
pub const PAGER_WRITER_LOCKED: u8 = 2;
pub const PAGER_WRITER_CACHEMOD: u8 = 3;
pub const PAGER_WRITER_DBMOD: u8 = 4;
pub const PAGER_WRITER_FINISHED: u8 = 5;
pub const PAGER_ERROR: u8 = 6;

/// The Pager.eLock variable is almost always set to one of the following
/// locking-states, according to the lock currently held on the database
/// file: NO_LOCK, SHARED_LOCK, RESERVED_LOCK or EXCLUSIVE_LOCK.  It may
/// also be set to UNKNOWN_LOCK.
const UNKNOWN_LOCK: u8 = (EXCLUSIVE_LOCK + 1) as u8;

/// The maximum legal sector size
const MAX_SECTOR_SIZE: u32 = 0x10000;

/// Number of u32 of WAL savepoint data (C WAL_SAVEPOINT_NDATA)
const WAL_SAVEPOINT_NDATA: usize = 4;

// Bits of the Pager.doNotSpill flag.
const SPILLFLAG_OFF: u8 = 0x01; // Never spill cache.  Set via pragma
const SPILLFLAG_ROLLBACK: u8 = 0x02; // Current rolling back, so do not spill
const SPILLFLAG_NOSYNC: u8 = 0x04; // Spill is ok, but do not sync

// Indexes for use with Pager.aStat[].
const PAGER_STAT_HIT: usize = 0;
const PAGER_STAT_MISS: usize = 1;
const PAGER_STAT_WRITE: usize = 2;
const PAGER_STAT_SPILL: usize = 3;

/// Journal files begin with the following magic string.  The data was
/// obtained from /dev/random.
const aJournalMagic: [u8; 8] = [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];

/// The pager getter method (C Pager.xGet)
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum PagerGetter {
    Normal,
    Error,
}

/// An instance of the following structure is allocated for each active
/// savepoint and statement transaction in the system.
#[derive(Default)]
struct PagerSavepoint {
    /// Starting offset in main journal
    iOffset: i64,
    /// See above
    iHdrOffset: i64,
    /// Set of pages in this savepoint
    pInSavepoint: Option<Box<Bitvec>>,
    /// Original number of pages in file
    nOrig: Pgno,
    /// Index of first record in sub-journal
    iSubRec: Pgno,
    /// If stmt journal may be truncated on RELEASE
    bTruncateOnRelease: i32,
    /// WAL savepoint context
    aWalData: [u32; WAL_SAVEPOINT_NDATA],
}

/// A list of the sqlite3_backup objects reading from this pager (C
/// Pager.pBackup).
pub type BackupList = Vec<Weak<RefCell<crate::backup::Backup>>>;

/// A open page cache is an instance of struct Pager.
pub struct Pager {
    /// The Rc holding this pager (set by sqlite3PagerOpen()); stored in
    /// PgHdr::pPager of the pages.
    pSelf: Weak<RefCell<Pager>>,
    /// OS functions to use for IO
    pVfs: Rc<Vfs>,
    /// Boolean. True if locking_mode==EXCLUSIVE
    exclusiveMode: u8,
    /// One of the PAGER_JOURNALMODE_* values
    journalMode: u8,
    /// Use a rollback journal on this file
    useJournal: u8,
    /// Do not sync the journal if true
    noSync: u8,
    /// Do extra syncs of the journal for robustness
    fullSync: u8,
    /// sync directory after journal delete
    extraSync: u8,
    /// SYNC_NORMAL or SYNC_FULL otherwise
    syncFlags: u8,
    /// See description above
    walSyncFlags: u8,
    /// zFilename is a temporary or immutable file
    tempFile: u8,
    /// Do not lock (except in WAL mode)
    noLock: u8,
    /// True for a read-only database
    readOnly: u8,
    /// True to inhibit all file I/O
    memDb: u8,
    /// VFS-implemented memory database
    memVfs: u8,
    /// Pager state (OPEN, READER, WRITER_LOCKED..)
    eState: u8,
    /// Current lock held on database file
    eLock: u8,
    /// Set after incrementing the change-counter
    changeCountDone: u8,
    /// Super-jrnl name is written into jrnl
    setSuper: u8,
    /// Do not spill the cache when non-zero
    doNotSpill: u8,
    /// True to use in-memory sub-journals
    subjInMemory: u8,
    /// True to use xFetch() (never: no mmap)
    #[allow(dead_code)]
    bUseFetch: u8,
    /// True if a shared lock has ever been held
    hasHeldSharedLock: u8,
    /// Number of pages in the database
    dbSize: Pgno,
    /// dbSize before the current transaction
    dbOrigSize: Pgno,
    /// Number of pages in the database file
    dbFileSize: Pgno,
    /// Value passed to FCNTL_SIZE_HINT call
    dbHintSize: Pgno,
    /// One of several kinds of errors
    errCode: i32,
    /// Pages journalled since last j-header written
    nRec: i32,
    /// Quasi-random value added to every checksum
    cksumInit: u32,
    /// Number of records written to sub-journal
    nSubRec: u32,
    /// One bit for each page in the database file
    pInJournal: Option<Box<Bitvec>>,
    /// File descriptor for database
    fd: SqliteFile,
    /// File descriptor for main journal
    jfd: SqliteFile,
    /// File descriptor for sub-journal
    sjfd: SqliteFile,
    /// Current write offset in the journal file
    journalOff: i64,
    /// Byte offset to previous journal header
    journalHdr: i64,
    /// Pointer to list of ongoing backup processes
    pBackup: BackupList,
    /// Array of active savepoints
    aSavepoint: Vec<PagerSavepoint>,
    /// Number of elements in aSavepoint[]
    nSavepoint: i32,
    /// Changes whenever database content changes
    iDataVersion: u32,
    /// Changes whenever database file changes
    dbFileVers: [u8; 16],
    /// Number of mmap pages currently outstanding
    nMmapOut: i32,
    /// Desired maximum mmap size
    szMmap: i64,
    /// Add this many bytes to each in-memory page
    nExtra: u16,
    /// Number of unused bytes at end of each page
    nReserve: i16,
    /// Flags for sqlite3_vfs.xOpen()
    vfsFlags: u32,
    /// Assumed sector size during rollback
    sectorSize: u32,
    /// Maximum allowed size of the database
    mxPgno: Pgno,
    /// Page number for the locking page
    lckPgno: Pgno,
    /// Number of bytes in a page
    pageSize: i64,
    /// Size limit for persistent journal files
    journalSizeLimit: i64,
    /// Name of the database file (C layout: name, then NUL and the URI
    /// parameter block if there are URI parameters)
    zFilename: ZStr,
    /// Name of the journal file (None for temp/in-memory databases)
    zJournal: Option<ZStr>,
    /// Function to call when busy
    xBusyHandler: Option<Rc<dyn Fn() -> i32>>,
    /// Total cache hits, misses, writes, spills
    aStat: [u32; 4],
    /// Database pages read
    nRead: i32,
    /// Call this routine when reloading pages
    xReiniter: Option<fn(&DbPage)>,
    /// Routine used to fetch pages
    xGet: PagerGetter,
    /// Pager.pageSize bytes of space for tmp use
    pTmpSpace: Vec<u8>,
    /// Pointer to page cache object
    pub(crate) pPCache: PCache,
    /// Write-ahead log used by "journal_mode=wal"
    pWal: Option<Box<crate::wal::Wal>>,
    /// File name for write-ahead log
    zWal: Option<ZStr>,
}

pub type PagerP = Rc<RefCell<Pager>>;

/// A page in the page cache (C PgHdr, a.k.a. DbPage).  The btree layer
/// reads and writes `aData` and `pExtra` directly (C
/// sqlite3PagerGetData() / sqlite3PagerGetExtra()).
pub struct PgHdr {
    pub pgno: Pgno,
    /// Page content (page-size bytes)
    pub aData: Vec<u8>,
    /// The btree layer's per-page state (C pExtra)
    pub pExtra: crate::btree::MemPage,
    pub flags: u16,
    pub nRef: i64,
    pub pPager: Weak<RefCell<Pager>>,
    // --- private to pcache.c / pcache1.c ---
    /// The PgHdr has been initialized by sqlite3PcacheFetchFinish() (C
    /// `pPage!=0`).
    pub(crate) bInit: bool,
    /// pcache1 key (C PgHdr1.iKey)
    pub(crate) iKey: u32,
    /// Position in the pcache1 LRU list; 0 when pinned
    pub(crate) lruSeq: u64,
    /// Position in the pcache dirty list; 0 when not dirty
    pub(crate) dirtySeq: u64,
}

pub type DbPage = Rc<RefCell<PgHdr>>;

impl PgHdr {
    /// A new, uninitialized page-cache entry (pcache1 allocation).
    pub(crate) fn new_raw(aData: Vec<u8>) -> DbPage {
        Rc::new(RefCell::new(PgHdr {
            pgno: 0,
            aData,
            pExtra: Default::default(),
            flags: 0,
            nRef: 0,
            pPager: Weak::new(),
            bInit: false,
            iKey: 0,
            lruSeq: 0,
            dirtySeq: 0,
        }))
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

#[inline]
fn get4byte(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}
#[inline]
fn put32bits(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_be_bytes());
}

#[inline]
fn JOURNAL_PG_SZ(p: &Pager) -> i64 {
    p.pageSize + 8
}
#[inline]
fn JOURNAL_HDR_SZ(p: &Pager) -> i64 {
    p.sectorSize as i64
}
#[inline]
fn MEMDB(p: &Pager) -> bool {
    p.memDb != 0
}
#[inline]
fn pagerUseWal(p: &Pager) -> bool {
    p.pWal.is_some()
}
#[inline]
fn PAGER_SJ_PGNO(p: &Pager) -> Pgno {
    p.lckPgno
}
#[inline]
fn pgflags(p: &DbPage) -> i32 {
    p.borrow().flags as i32
}
#[inline]
fn pgpgno(p: &DbPage) -> Pgno {
    p.borrow().pgno
}
#[inline]
fn PENDING_BYTE() -> i64 {
    crate::global::sqlite3PendingByte
}

/// The pager that owns page pPg (None if it is gone).
fn page_pager(pPg: &DbPage) -> Option<PagerP> {
    pPg.borrow().pPager.upgrade()
}

/// Call sqlite3BackupUpdate() for each registered backup.
fn backup_update(pPager: &mut Pager, pgno: Pgno, aData: &[u8]) {
    if pPager.pBackup.is_empty() {
        return;
    }
    pPager.pBackup.retain(|w| w.strong_count() > 0);
    let list: Vec<crate::backup::BackupP> = pPager.pBackup.iter().filter_map(|w| w.upgrade()).collect();
    crate::backup::sqlite3BackupUpdate(&list, pgno, aData);
}

/// Call sqlite3BackupRestart() for each registered backup.
fn backup_restart(pPager: &mut Pager) {
    if pPager.pBackup.is_empty() {
        return;
    }
    pPager.pBackup.retain(|w| w.strong_count() > 0);
    let list: Vec<crate::backup::BackupP> = pPager.pBackup.iter().filter_map(|w| w.upgrade()).collect();
    crate::backup::sqlite3BackupRestart(&list);
}

/// Check the internal consistency of the pager state (C
/// assert_pager_state()).
fn assert_pager_state(p: &Pager) -> bool {
    // State must be valid.
    debug_assert!(p.eState <= PAGER_ERROR);
    // Regardless of the current state, a temp-file connection always
    // behaves as if it has an exclusive lock on the database file.
    debug_assert!(p.tempFile == 0 || p.eLock as i32 == EXCLUSIVE_LOCK);
    debug_assert!(p.tempFile == 0 || p.changeCountDone != 0);
    // If the useJournal flag is clear, the journal-mode must be "OFF".
    debug_assert!(p.journalMode as i32 == PAGER_JOURNALMODE_OFF || p.useJournal != 0);
    debug_assert!(p.journalMode as i32 != PAGER_JOURNALMODE_OFF || !isOpen(&p.jfd));
    if MEMDB(p) {
        debug_assert!(!isOpen(&p.fd));
        debug_assert!(p.noSync != 0);
        debug_assert!(
            p.journalMode as i32 == PAGER_JOURNALMODE_OFF
                || p.journalMode as i32 == PAGER_JOURNALMODE_MEMORY
        );
        debug_assert!(p.eState != PAGER_ERROR && p.eState != PAGER_OPEN);
        debug_assert!(!pagerUseWal(p));
    }
    debug_assert!(p.changeCountDone == 0 || p.eLock as i32 >= RESERVED_LOCK);
    debug_assert!(p.eLock as i32 != PENDING_LOCK);
    match p.eState {
        PAGER_OPEN => {
            debug_assert!(!MEMDB(p));
            debug_assert!(p.errCode == SQLITE_OK);
            debug_assert!(p.pPCache.nRefSum == 0 || p.tempFile != 0);
        }
        PAGER_READER => {
            debug_assert!(p.errCode == SQLITE_OK);
            debug_assert!(p.eLock != UNKNOWN_LOCK);
            debug_assert!(p.eLock as i32 >= SHARED_LOCK);
        }
        PAGER_WRITER_LOCKED => {
            debug_assert!(p.eLock != UNKNOWN_LOCK);
            debug_assert!(p.errCode == SQLITE_OK);
            if !pagerUseWal(p) {
                debug_assert!(p.eLock as i32 >= RESERVED_LOCK);
            }
            debug_assert!(p.dbSize == p.dbOrigSize);
            debug_assert!(p.dbOrigSize == p.dbFileSize);
            debug_assert!(p.dbOrigSize == p.dbHintSize);
            debug_assert!(p.setSuper == 0);
        }
        PAGER_WRITER_CACHEMOD => {
            debug_assert!(p.eLock != UNKNOWN_LOCK);
            debug_assert!(p.errCode == SQLITE_OK);
            if !pagerUseWal(p) {
                debug_assert!(p.eLock as i32 >= RESERVED_LOCK);
                debug_assert!(
                    isOpen(&p.jfd)
                        || p.journalMode as i32 == PAGER_JOURNALMODE_OFF
                        || p.journalMode as i32 == PAGER_JOURNALMODE_WAL
                );
            }
            debug_assert!(p.dbOrigSize == p.dbFileSize);
            debug_assert!(p.dbOrigSize == p.dbHintSize);
        }
        PAGER_WRITER_DBMOD => {
            debug_assert!(p.eLock as i32 == EXCLUSIVE_LOCK);
            debug_assert!(p.errCode == SQLITE_OK);
            debug_assert!(!pagerUseWal(p));
            debug_assert!(
                isOpen(&p.jfd)
                    || p.journalMode as i32 == PAGER_JOURNALMODE_OFF
                    || p.journalMode as i32 == PAGER_JOURNALMODE_WAL
            );
            debug_assert!(p.dbOrigSize <= p.dbHintSize);
        }
        PAGER_WRITER_FINISHED => {
            debug_assert!(p.eLock as i32 == EXCLUSIVE_LOCK);
            debug_assert!(p.errCode == SQLITE_OK);
            debug_assert!(!pagerUseWal(p));
            debug_assert!(
                isOpen(&p.jfd)
                    || p.journalMode as i32 == PAGER_JOURNALMODE_OFF
                    || p.journalMode as i32 == PAGER_JOURNALMODE_WAL
            );
        }
        _ => {
            // There must be at least one outstanding reference to the
            // pager if in ERROR state. Otherwise the pager should have
            // already dropped back to OPEN state.
            debug_assert!(p.errCode != SQLITE_OK);
            debug_assert!(p.pPCache.nRefSum > 0 || p.tempFile != 0);
        }
    }
    true
}

/// Set the Pager.xGet method for the appropriate routine used to fetch
/// content from the pager.
fn setGetterMethod(pPager: &mut Pager) {
    if pPager.errCode != 0 {
        pPager.xGet = PagerGetter::Error;
    } else {
        pPager.xGet = PagerGetter::Normal;
    }
}

/// Return true if it is necessary to write page *pPg into the sub-journal.
/// A page needs to be written into the sub-journal if there exists one
/// or more open savepoints for which:
///
///   * The page-number is less than or equal to PagerSavepoint.nOrig, and
///   * The bit corresponding to the page-number is not set in
///     PagerSavepoint.pInSavepoint.
fn subjRequiresPage(pPager: &mut Pager, pgno: Pgno) -> bool {
    let n = pPager.nSavepoint as usize;
    for i in 0..n {
        let p = &mut pPager.aSavepoint[i];
        if p.nOrig >= pgno && 0 == sqlite3BitvecTestNotNull(p.pInSavepoint.as_mut().unwrap(), pgno)
        {
            for j in i + 1..n {
                pPager.aSavepoint[j].bTruncateOnRelease = 0;
            }
            return true;
        }
    }
    false
}

/// Return true if the page is already in the journal file.
fn pageInJournal(pPager: &mut Pager, pgno: Pgno) -> bool {
    sqlite3BitvecTestOpt(pPager.pInJournal.as_deref_mut(), pgno) != 0
}

/// Read a 32-bit integer from the given file descriptor.  Store the
/// integer that is read in *pRes.  Return SQLITE_OK if everything worked,
/// or the error code returned by sqlite3OsRead() otherwise.
fn read32bits(fd: &mut SqliteFile, offset: i64, pRes: &mut u32) -> i32 {
    let mut ac = [0u8; 4];
    let rc = sqlite3OsRead(fd, &mut ac, 4, offset);
    if rc == SQLITE_OK {
        *pRes = get4byte(&ac);
    }
    rc
}

/// Write a 32-bit integer into the given file descriptor.  Return
/// SQLITE_OK on success or an error code is something goes wrong.
fn write32bits(fd: &mut SqliteFile, offset: i64, val: u32) -> i32 {
    let ac = val.to_be_bytes();
    sqlite3OsWrite(fd, &ac, 4, offset)
}

/// Unlock the database file to level eLock, which must be either
/// NO_LOCK or SHARED_LOCK. Regardless of whether or not the call to
/// xUnlock() succeeds, the Pager.eLock variable is updated to reflect the
/// new lock state.
fn pagerUnlockDb(pPager: &mut Pager, eLock: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pPager.exclusiveMode == 0 || pPager.eLock as i32 == eLock);
    debug_assert!(eLock == NO_LOCK || eLock == SHARED_LOCK);
    debug_assert!(eLock != NO_LOCK || !pagerUseWal(pPager));
    if isOpen(&pPager.fd) {
        debug_assert!(pPager.eLock as i32 >= eLock);
        rc = if pPager.noLock != 0 {
            SQLITE_OK
        } else {
            sqlite3OsUnlock(&mut pPager.fd, eLock)
        };
        if pPager.eLock != UNKNOWN_LOCK {
            pPager.eLock = eLock as u8;
        }
    }
    pPager.changeCountDone = pPager.tempFile; // ticket fb3b3024ea238d5c
    rc
}

/// Lock the database file to level eLock, which must be either
/// SHARED_LOCK, RESERVED_LOCK or EXCLUSIVE_LOCK. If the caller is
/// successful, set the Pager.eLock variable to match the (attempted) new
/// lock.
fn pagerLockDb(pPager: &mut Pager, eLock: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(eLock == SHARED_LOCK || eLock == RESERVED_LOCK || eLock == EXCLUSIVE_LOCK);
    if (pPager.eLock as i32) < eLock || pPager.eLock == UNKNOWN_LOCK {
        rc = if pPager.noLock != 0 {
            SQLITE_OK
        } else {
            sqlite3OsLock(&mut pPager.fd, eLock)
        };
        if rc == SQLITE_OK && (pPager.eLock != UNKNOWN_LOCK || eLock == EXCLUSIVE_LOCK) {
            pPager.eLock = eLock as u8;
        }
    }
    rc
}

/// This function determines whether or not the atomic-write or
/// batch-atomic-write optimizations can be used with this pager (neither
/// is compiled in: always 0).
fn jrnlBufferSize(_pPager: &Pager) -> i32 {
    0
}

/// When this is called the journal file for pager pPager must be open.
/// This function attempts to read a super-journal file name from the end
/// of the file and, if successful, copies it into memory supplied by the
/// caller.
///
/// If a super-journal file name is present at the end of the journal
/// file, then it is read into *pzSuper.  Otherwise *pzSuper is None.
fn pagerIsSuperJrnlName(zSuper: &[u8]) -> bool {
    let nSuper = zSuper.len();
    if nSuper < 12 {
        return false;
    }
    if &zSuper[nSuper - 12..nSuper - 9] != b"-mj" {
        return false;
    }
    if zSuper[nSuper - 3] != b'9' {
        return false;
    }
    for ii in nSuper - 9..nSuper {
        if !crate::global::sqlite3Isxdigit(zSuper[ii]) {
            return false;
        }
    }
    true
}

/// Read the super-journal name stored at the end of journal pJrnl.
fn readSuperJournal(pJrnl: &mut SqliteFile, nSuper: u64, pzSuper: &mut Option<ZStr>) -> i32 {
    let mut rc;
    let mut len: u32 = 0;
    let mut szJ: i64 = 0;
    let mut cksum: u32 = 0;
    let mut aMagic = [0u8; 8];
    *pzSuper = None;

    rc = sqlite3OsFileSize(pJrnl, &mut szJ);
    if rc != SQLITE_OK || szJ < 16 {
        return rc;
    }
    rc = read32bits(pJrnl, szJ - 16, &mut len);
    if rc != SQLITE_OK || len as u64 >= nSuper || len as i64 > szJ - 16 || len == 0 {
        return rc;
    }
    rc = read32bits(pJrnl, szJ - 12, &mut cksum);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = sqlite3OsRead(pJrnl, &mut aMagic, 8, szJ - 8);
    if rc != SQLITE_OK {
        return rc;
    }
    let mut zOut = vec![0u8; len as usize];
    rc = sqlite3OsRead(pJrnl, &mut zOut, len as i32, szJ - 16 - len as i64);
    if rc == SQLITE_OK {
        // See if the checksum matches the super-journal name
        for &c in zOut.iter() {
            cksum = cksum.wrapping_sub(c as i8 as i32 as u32);
        }
    }
    // The C string stops at the first NUL
    let zName = zpath_name(&zOut).to_vec();
    if rc != SQLITE_OK || !pagerIsSuperJrnlName(&zName) || cksum != 0 || aMagic != aJournalMagic {
        // If the checksum doesn't add up, then one or more of the disk
        // sectors containing the super-journal filename is corrupted.
        // This means definitely roll back, so just return SQLITE_OK and
        // report a (nul) super-journal name.
        *pzSuper = None;
    } else {
        *pzSuper = Some(zName);
    }
    rc
}

/// Return the offset of the sector boundary at or immediately following
/// the value in pPager->journalOff, assuming a sector size of
/// pPager->sectorSize bytes.
fn journalHdrOffset(pPager: &Pager) -> i64 {
    let mut offset = 0;
    let c = pPager.journalOff;
    if c != 0 {
        offset = ((c - 1) / JOURNAL_HDR_SZ(pPager) + 1) * JOURNAL_HDR_SZ(pPager);
    }
    debug_assert!(offset % JOURNAL_HDR_SZ(pPager) == 0);
    debug_assert!(offset >= c);
    debug_assert!((offset - c) < JOURNAL_HDR_SZ(pPager));
    offset
}

/// The journal file must be open when this function is called.
///
/// This function is a no-op if the journal file has not been written to
/// within the current transaction (i.e. if Pager.journalOff==0).
///
/// If doTruncate is non-zero or the Pager.journalSizeLimit variable is
/// set to 0, then truncate the journal file to zero bytes in size.
/// Otherwise, zero the 28-byte header at the start of the journal file.
/// In either case, if the pager is not in no-sync mode, sync the journal
/// file immediately after writing or truncating it.
fn zeroJournalHdr(pPager: &mut Pager, doTruncate: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(isOpen(&pPager.jfd));
    debug_assert!(sqlite3JournalIsInMemory(&pPager.jfd) == 0);
    if pPager.journalOff != 0 {
        let iLimit = pPager.journalSizeLimit; // Local cache of jsl
        if doTruncate != 0 || iLimit == 0 {
            rc = sqlite3OsTruncate(&mut pPager.jfd, 0);
        } else {
            let zeroHdr = [0u8; 28];
            rc = sqlite3OsWrite(&mut pPager.jfd, &zeroHdr, 28, 0);
        }
        if rc == SQLITE_OK && pPager.noSync == 0 {
            rc = sqlite3OsSync(
                &mut pPager.jfd,
                SQLITE_SYNC_DATAONLY | pPager.syncFlags as i32,
            );
        }

        // At this point the transaction is committed but the write lock
        // is still held on the file. If there is a size limit configured
        // for the persistent journal and the journal file currently
        // consumes more space than that limit allows for, truncate it now.
        if rc == SQLITE_OK && iLimit > 0 {
            let mut sz = 0i64;
            rc = sqlite3OsFileSize(&mut pPager.jfd, &mut sz);
            if rc == SQLITE_OK && sz > iLimit {
                rc = sqlite3OsTruncate(&mut pPager.jfd, iLimit);
            }
        }
    }
    rc
}

/// The journal file must be open when this routine is called. A journal
/// header (JOURNAL_HDR_SZ bytes) is written into the journal file at the
/// current location.
///
/// The format for the journal header is as follows:
/// - 8 bytes: Magic identifying journal format.
/// - 4 bytes: Number of records in journal, or -1 no-sync mode is on.
/// - 4 bytes: Random number used for page hash.
/// - 4 bytes: Initial database page count.
/// - 4 bytes: Sector size used by the process that wrote this journal.
/// - 4 bytes: Database page size.
///
/// Followed by (JOURNAL_HDR_SZ - 28) bytes of unused space.
fn writeJournalHdr(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    let mut nHeader = pPager.pageSize as u32;
    debug_assert!(isOpen(&pPager.jfd));
    if nHeader as i64 > JOURNAL_HDR_SZ(pPager) {
        nHeader = JOURNAL_HDR_SZ(pPager) as u32;
    }
    let mut zHeader = vec![0u8; nHeader as usize];

    // If there are active savepoints and any of them were created since
    // the most recent journal header was written, update the
    // PagerSavepoint.iHdrOffset fields now.
    let journalOff = pPager.journalOff;
    for ii in 0..pPager.nSavepoint as usize {
        if pPager.aSavepoint[ii].iHdrOffset == 0 {
            pPager.aSavepoint[ii].iHdrOffset = journalOff;
        }
    }

    pPager.journalOff = journalHdrOffset(pPager);
    pPager.journalHdr = pPager.journalOff;

    // Write the nRec Field - the number of page records that follow this
    // journal header. Normally, zero is written to this value at this
    // time.  After the records are added to the journal (and the journal
    // synced, if in full-sync mode), the zero is overwritten with the
    // true number of records (see syncJournal()).
    //
    // A faster alternative is to write 0xFFFFFFFF to the nRec field. When
    // reading the journal this value tells SQLite to assume that the rest
    // of the journal file contains valid page records. This assumption is
    // dangerous, as if a failure occurred whilst writing to the journal
    // file it may contain some garbage data. There are two scenarios
    // where this risk can be ignored:
    //
    //   * When the pager is in no-sync mode. Corruption can follow a
    //     power failure in this case anyway.
    //
    //   * When the SQLITE_IOCAP_SAFE_APPEND flag is set. This guarantees
    //     that garbage data is never appended to the journal file.
    debug_assert!(isOpen(&pPager.fd) || pPager.noSync != 0);
    if pPager.noSync != 0
        || pPager.journalMode as i32 == PAGER_JOURNALMODE_MEMORY
        || (sqlite3OsDeviceCharacteristics(&mut pPager.fd) & SQLITE_IOCAP_SAFE_APPEND) != 0
    {
        zHeader[..8].copy_from_slice(&aJournalMagic);
        put32bits(&mut zHeader[8..], 0xffffffff);
    } else {
        for b in zHeader[..12].iter_mut() {
            *b = 0;
        }
    }

    // The random check-hash initializer
    if pPager.journalMode as i32 != PAGER_JOURNALMODE_MEMORY {
        let mut r = [0u8; 4];
        os_randomness(&mut r);
        pPager.cksumInit = u32::from_ne_bytes(r);
    }
    put32bits(&mut zHeader[12..], pPager.cksumInit);
    // The initial database size
    put32bits(&mut zHeader[16..], pPager.dbOrigSize);
    // The assumed sector size for this process
    put32bits(&mut zHeader[20..], pPager.sectorSize);

    // The page size
    put32bits(&mut zHeader[24..], pPager.pageSize as u32);

    // Initializing the tail of the buffer is not necessary.  Everything
    // works find if the following memset() is omitted.  But initializing
    // the memory prevents valgrind from complaining, so we are willing to
    // take the performance hit.
    for b in zHeader[28..].iter_mut() {
        *b = 0;
    }

    // In theory, it is only necessary to write the 28 bytes that the
    // journal header consumes to the journal file here. Then increment
    // the Pager.journalOff variable by JOURNAL_HDR_SZ so that the next
    // record is written to the following sector (leaving a gap in the
    // file that will be implicitly filled in by the OS).
    //
    // However it has been discovered that on some systems this pattern
    // can be significantly slower than contiguously writing data to the
    // file, even if that means explicitly writing data to the block of
    // (JOURNAL_HDR_SZ - 28) bytes that will not be used. So that is what
    // is done.
    let mut nWrite: u32 = 0;
    while rc == SQLITE_OK && (nWrite as i64) < JOURNAL_HDR_SZ(pPager) {
        rc = sqlite3OsWrite(&mut pPager.jfd, &zHeader, nHeader as i32, pPager.journalOff);
        debug_assert!(pPager.journalHdr <= pPager.journalOff);
        pPager.journalOff += nHeader as i64;
        nWrite += nHeader;
    }
    rc
}

/// The journal file must be open when this is called. A journal header
/// file (JOURNAL_HDR_SZ bytes) is read from the current location in the
/// journal file. See comments above function writeJournalHdr() for a
/// description of the journal header format.
///
/// If the header is read successfully, *pNRec is set to the number of
/// page records following this header and *pDbSize is set to the size of
/// the database before the transaction began, in pages. Also, pPager->
/// cksumInit is set to the value stored in the journal header. SQLITE_OK
/// is returned in this case.
///
/// If the journal header file appears to be corrupted, SQLITE_DONE is
/// returned and *pNRec and *PDbSize are undefined.  If JOURNAL_HDR_SZ
/// bytes cannot be read from the journal file an error code is returned.
fn readJournalHdr(
    pPager: &mut Pager,
    isHot: i32,
    journalSize: i64,
    pNRec: &mut u32,
    pDbSize: &mut u32,
) -> i32 {
    let mut rc;
    let mut aMagic = [0u8; 8];
    debug_assert!(isOpen(&pPager.jfd));

    // Advance Pager.journalOff to the start of the next sector. If the
    // journal file is too small for there to be a header stored at this
    // point, return SQLITE_DONE.
    pPager.journalOff = journalHdrOffset(pPager);
    if pPager.journalOff + JOURNAL_HDR_SZ(pPager) > journalSize {
        return SQLITE_DONE;
    }
    let iHdrOff = pPager.journalOff;

    // Read in the first 8 bytes of the journal header. If they do not
    // match the  magic string found at the start of each journal header,
    // return SQLITE_DONE. If an IO error occurs, return an error code.
    // Otherwise, proceed.
    if isHot != 0 || iHdrOff != pPager.journalHdr {
        rc = sqlite3OsRead(&mut pPager.jfd, &mut aMagic, 8, iHdrOff);
        if rc != 0 {
            return rc;
        }
        if aMagic != aJournalMagic {
            return SQLITE_DONE;
        }
    }

    // Read the first three 32-bit fields of the journal header: The nRec
    // field, the checksum-initializer and the database size at the start
    // of the transaction. Return an error code if anything goes wrong.
    rc = read32bits(&mut pPager.jfd, iHdrOff + 8, pNRec);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = read32bits(&mut pPager.jfd, iHdrOff + 12, &mut pPager.cksumInit);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = read32bits(&mut pPager.jfd, iHdrOff + 16, pDbSize);
    if rc != SQLITE_OK {
        return rc;
    }

    if pPager.journalOff == 0 {
        let mut iPageSize: u32 = 0; // Page-size field of journal header
        let mut iSectorSize: u32 = 0; // Sector-size field of journal header

        // Read the page-size and sector-size journal header fields.
        rc = read32bits(&mut pPager.jfd, iHdrOff + 20, &mut iSectorSize);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = read32bits(&mut pPager.jfd, iHdrOff + 24, &mut iPageSize);
        if rc != SQLITE_OK {
            return rc;
        }

        // Versions of SQLite prior to 3.5.8 set the page-size field of the
        // journal header to zero. In this case, assume that the
        // Pager.pageSize variable is already set to the correct page size.
        if iPageSize == 0 {
            iPageSize = pPager.pageSize as u32;
        }

        // Check that the values read from the page-size and sector-size
        // fields are within range. To be 'in range', both values need to
        // be a power of two greater than or equal to 512 or 32, and not
        // greater than their respective compile time maximums.
        //
        // If either of these values are invalid, return SQLITE_DONE.
        if iPageSize < 512
            || iSectorSize < 32
            || iPageSize > SQLITE_MAX_PAGE_SIZE as u32
            || iSectorSize > MAX_SECTOR_SIZE
            || (iPageSize.wrapping_sub(1) & iPageSize) != 0
            || (iSectorSize.wrapping_sub(1) & iSectorSize) != 0
        {
            // If the either the page-size or sector-size in the journal-
            // header is invalid, then the process that wrote the journal-
            // header must have crashed before the header was synced. In
            // this case stop reading the journal file here.
            return SQLITE_DONE;
        }

        // Update the page-size to match the value read from the journal.
        // Use a testcase() macro to make sure that malloc failure within
        // PagerSetPagesize() is tested.
        rc = sqlite3PagerSetPagesize(pPager, &mut iPageSize, -1);

        // Update the assumed sector-size to match the value used by the
        // process that created this journal. If this journal was created
        // by a process other than this one, then this routine is being
        // called from within pager_playback(). The local value of
        // Pager.sectorSize is restored at the end of that routine.
        pPager.sectorSize = iSectorSize;
    }

    pPager.journalOff += JOURNAL_HDR_SZ(pPager);
    rc
}

/// Write the supplied super-journal name into the journal file for pager
/// pPager at the current location. The super-journal name must be the
/// last thing written to a journal file. If the pager is in full-sync
/// mode, the journal file descriptor is advanced to the next sector
/// boundary before anything is written. The format is:
///
///   + 4 bytes: PAGER_SJ_PGNO.
///   + N bytes: super-journal filename in utf-8.
///   + 4 bytes: N (length of super-journal name in bytes, no nul-terminator).
///   + 4 bytes: super-journal name checksum.
///   + 8 bytes: aJournalMagic[].
///
/// The super-journal page checksum is the sum of the bytes in the
/// super-journal name, where each byte is interpreted as a signed 8-bit
/// integer.
///
/// If zSuper is a NULL pointer (occurs for a single database
/// transaction), this call is a no-op.
fn writeSuperJournal(pPager: &mut Pager, zSuper: Option<&[u8]>) -> i32 {
    let mut rc;
    let mut cksum: u32 = 0;

    debug_assert!(pPager.setSuper == 0);
    debug_assert!(!pagerUseWal(pPager));

    let zSuper = match zSuper {
        Some(z) if pPager.journalMode as i32 != PAGER_JOURNALMODE_MEMORY && isOpen(&pPager.jfd) => {
            zpath_name(z)
        }
        _ => return SQLITE_OK,
    };
    pPager.setSuper = 1;
    debug_assert!(pPager.journalHdr <= pPager.journalOff);

    // Calculate the length in bytes and the checksum of zSuper
    let nSuper = zSuper.len();
    for &c in zSuper {
        cksum = cksum.wrapping_add(c as i8 as i32 as u32);
    }

    // If in full-sync mode, advance to the next disk sector before writing
    // the super-journal name. This is in case the previous page written to
    // the journal has already been synced.
    if pPager.fullSync != 0 {
        pPager.journalOff = journalHdrOffset(pPager);
    }
    let iHdrOff = pPager.journalOff;

    // Write the super-journal data to the end of the journal file. If an
    // error occurs, return the error code to the caller.
    let lck = PAGER_SJ_PGNO(pPager);
    rc = write32bits(&mut pPager.jfd, iHdrOff, lck);
    if rc != 0 {
        return rc;
    }
    rc = sqlite3OsWrite(&mut pPager.jfd, zSuper, nSuper as i32, iHdrOff + 4);
    if rc != 0 {
        return rc;
    }
    rc = write32bits(&mut pPager.jfd, iHdrOff + 4 + nSuper as i64, nSuper as u32);
    if rc != 0 {
        return rc;
    }
    rc = write32bits(&mut pPager.jfd, iHdrOff + 4 + nSuper as i64 + 4, cksum);
    if rc != 0 {
        return rc;
    }
    rc = sqlite3OsWrite(
        &mut pPager.jfd,
        &aJournalMagic,
        8,
        iHdrOff + 4 + nSuper as i64 + 8,
    );
    if rc != 0 {
        return rc;
    }
    pPager.journalOff += nSuper as i64 + 20;

    // If the pager is in persistent-journal mode, then the physical
    // journal-file may extend past the end of the super-journal name and 8
    // bytes of magic data just written to the file. This is dangerous
    // because the code to rollback a hot-journal file will not be able to
    // find the super-journal name to determine whether or not the journal
    // is hot.
    //
    // Easiest thing to do in this scenario is to truncate the journal file
    // to the required size.
    let mut jrnlSize = 0i64;
    rc = sqlite3OsFileSize(&mut pPager.jfd, &mut jrnlSize);
    if rc == SQLITE_OK && jrnlSize > pPager.journalOff {
        rc = sqlite3OsTruncate(&mut pPager.jfd, pPager.journalOff);
    }
    rc
}

/// Discard the entire contents of the in-memory page-cache.
fn pager_reset(pPager: &mut Pager) {
    pPager.iDataVersion = pPager.iDataVersion.wrapping_add(1);
    backup_restart(pPager);
    sqlite3PcacheClear(&mut pPager.pPCache);
}

/// C: `u32 sqlite3PagerDataVersion(Pager *pPager)`
///
/// Return the pPager->iDataVersion value
pub fn sqlite3PagerDataVersion(pPager: &mut crate::pager::Pager) -> u32 {
    pPager.iDataVersion
}

/// Free all structures in the Pager.aSavepoint[] array and set both
/// Pager.aSavepoint and Pager.nSavepoint to zero. Close the sub-journal
/// if it is open and the pager is not in exclusive mode.
fn releaseAllSavepoints(pPager: &mut Pager) {
    pPager.aSavepoint.clear();
    if pPager.exclusiveMode == 0 || sqlite3JournalIsInMemory(&pPager.sjfd) != 0 {
        sqlite3OsClose(&mut pPager.sjfd);
    }
    pPager.nSavepoint = 0;
    pPager.nSubRec = 0;
}

/// Set the bit number pgno in the PagerSavepoint.pInSavepoint bitvecs of
/// all open savepoints. Or, if the pgno is greater than the savepoint's
/// nOrig value, do nothing.
fn addToSavepointBitvecs(pPager: &mut Pager, pgno: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    for ii in 0..pPager.nSavepoint as usize {
        let p = &mut pPager.aSavepoint[ii];
        if pgno <= p.nOrig {
            rc |= sqlite3BitvecSetOpt(p.pInSavepoint.as_deref_mut(), pgno);
        }
    }
    rc
}

/// This function is a no-op if the pager is in exclusive mode and not in
/// the ERROR state. Otherwise, it switches the pager to PAGER_OPEN state.
///
/// If the pager is not in exclusive-access mode, the database file is
/// completely unlocked. If the file is unlocked and the file-system does
/// not exhibit the UNDELETABLE_WHEN_OPEN property, the journal file is
/// closed (if it is open).
///
/// If the pager is in ERROR state when this function is called, the
/// contents of the pager cache are discarded before switching back to
/// the OPEN state. Regardless of whether the pager is in exclusive-mode
/// or not, any journal file left in the file-system will be treated as
/// a hot-journal and rolled back the next time a read-transaction is
/// opened (by this or by any other connection).
fn pager_unlock(pPager: &mut Pager) {
    debug_assert!(
        pPager.eState == PAGER_READER
            || pPager.eState == PAGER_OPEN
            || pPager.eState == PAGER_ERROR
    );

    pPager.pInJournal = None;
    releaseAllSavepoints(pPager);

    if pagerUseWal(pPager) {
        debug_assert!(!isOpen(&pPager.jfd));
        let eState = pPager.eState;
        if let Some(w) = pPager.pWal.as_mut() {
            if eState == PAGER_ERROR {
                // If an IO error occurs in wal.c while attempting to wrap
                // the wal file after a commit has occurred, the commit may
                // have been successful.
                let _ = crate::wal::sqlite3WalEndWriteTransaction(w);
            }
            crate::wal::sqlite3WalEndReadTransaction(w);
        }
        pPager.eState = PAGER_OPEN;
    } else if pPager.exclusiveMode == 0 {
        let iDc = if isOpen(&pPager.fd) {
            sqlite3OsDeviceCharacteristics(&mut pPager.fd)
        } else {
            0
        };

        // If the operating system support deletion of open files, then
        // close the journal file when dropping the database lock.
        // Otherwise another connection with journal_mode=delete might
        // delete the file out from under us.
        if 0 == (iDc & SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN) || 1 != (pPager.journalMode & 5) {
            sqlite3OsClose(&mut pPager.jfd);
        }

        // If the pager is in the ERROR state and the call to unlock the
        // database file fails, set the current lock to UNKNOWN_LOCK. See
        // the comment above the #define for UNKNOWN_LOCK for an
        // explanation of why this is necessary.
        let rc = pagerUnlockDb(pPager, NO_LOCK);
        if rc != SQLITE_OK && pPager.eState == PAGER_ERROR {
            pPager.eLock = UNKNOWN_LOCK;
        }

        // The pager state may be changed from PAGER_ERROR to PAGER_OPEN
        // here without clearing the error code. This is intentional - the
        // error code is cleared and the cache reset in the block below.
        debug_assert!(pPager.errCode != 0 || pPager.eState != PAGER_ERROR);
        pPager.eState = PAGER_OPEN;
    }

    // If Pager.errCode is set, the contents of the pager cache cannot be
    // trusted. Now that there are no outstanding references to the pager,
    // it can safely move back to PAGER_OPEN state. This happens in both
    // normal and exclusive-locking mode.
    debug_assert!(pPager.errCode == SQLITE_OK || !MEMDB(pPager));
    if pPager.errCode != 0 {
        if pPager.tempFile == 0 {
            pager_reset(pPager);
            pPager.changeCountDone = 0;
            pPager.eState = PAGER_OPEN;
        } else {
            pPager.eState = if isOpen(&pPager.jfd) {
                PAGER_OPEN
            } else {
                PAGER_READER
            };
        }
        pPager.errCode = SQLITE_OK;
        setGetterMethod(pPager);
    }

    pPager.journalOff = 0;
    pPager.journalHdr = 0;
    pPager.setSuper = 0;
}

/// This function is called whenever an IOERR or FULL error that requires
/// the pager to transition into the ERROR state may have occurred.  The
/// first argument is a pointer to the pager structure, the second the
/// error-code about to be returned by a pager API function. The value
/// returned is a copy of the second argument to this function.
///
/// If the second argument is SQLITE_FULL, SQLITE_IOERR or one of the
/// IOERR sub-codes, the pager enters the ERROR state and the error code
/// is stored in Pager.errCode. While the pager remains in the ERROR
/// state, all major API calls on the Pager will immediately return
/// Pager.errCode.
fn pager_error(pPager: &mut Pager, rc: i32) -> i32 {
    let rc2 = rc & 0xff;
    debug_assert!(rc == SQLITE_OK || !MEMDB(pPager));
    debug_assert!(
        pPager.errCode == SQLITE_FULL
            || pPager.errCode == SQLITE_OK
            || (pPager.errCode & 0xff) == SQLITE_IOERR
    );
    if rc2 == SQLITE_FULL || rc2 == SQLITE_IOERR {
        pPager.errCode = rc;
        pPager.eState = PAGER_ERROR;
        setGetterMethod(pPager);
    }
    rc
}

/// The write transaction open on pPager is being committed (bCommit==1)
/// or rolled back (bCommit==0).
///
/// Return TRUE if and only if all dirty pages should be flushed to disk.
///
/// Rules:
///
///   *  For non-TEMP databases, always sync to disk.  This is necessary
///      for transactions to be durable.
///
///   *  Sync TEMP database only on a COMMIT (not a ROLLBACK) when the
///      file has been created already (via a spill on pagerStress()) and
///      when the number of dirty pages in memory exceeds 25% of the total
///      cache size.
fn pagerFlushOnCommit(pPager: &mut Pager, bCommit: i32) -> bool {
    if pPager.tempFile == 0 {
        return true;
    }
    if bCommit == 0 {
        return false;
    }
    if !isOpen(&pPager.fd) {
        return false;
    }
    sqlite3PCachePercentDirty(&mut pPager.pPCache) >= 25
}

/// This routine ends a transaction. A transaction is usually ended by
/// either a COMMIT or a ROLLBACK operation. This routine may be called
/// after rollback of a hot-journal, or if an error occurs while opening
/// the journal file or writing the very first journal-header of a
/// database transaction.
///
/// This routine is never called in PAGER_ERROR state. If it is called in
/// PAGER_NONE state, it is a no-op.  See pager.c for the effects on the
/// journal file in each journal mode.
fn pager_end_transaction(pPager: &mut Pager, hasSuper: i32, bCommit: i32) -> i32 {
    let mut rc = SQLITE_OK; // Error code from journal finalization operation
    let mut rc2 = SQLITE_OK; // Error code from db file unlock operation

    // Do nothing if the pager does not have an open write transaction or
    // at least a RESERVED lock. This function may be called when there is
    // no write-transaction active but a RESERVED or greater lock is held
    // under two circumstances:
    //
    //   1. After a successful hot-journal rollback, it is called with
    //      eState==PAGER_NONE and eLock==EXCLUSIVE_LOCK.
    //
    //   2. If a connection with locking_mode=exclusive holding an
    //      EXCLUSIVE lock switches back to locking_mode=normal and then
    //      executes a read-transaction, this function is called with
    //      eState==PAGER_READER and eLock==EXCLUSIVE_LOCK.
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.eState != PAGER_ERROR);
    if pPager.eState < PAGER_WRITER_LOCKED && (pPager.eLock as i32) < RESERVED_LOCK {
        return SQLITE_OK;
    }

    releaseAllSavepoints(pPager);
    debug_assert!(isOpen(&pPager.jfd) || pPager.pInJournal.is_none());
    if isOpen(&pPager.jfd) {
        debug_assert!(!pagerUseWal(pPager));

        // Finalize the journal file.
        if sqlite3JournalIsInMemory(&pPager.jfd) != 0 {
            sqlite3OsClose(&mut pPager.jfd);
        } else if pPager.journalMode as i32 == PAGER_JOURNALMODE_TRUNCATE {
            if pPager.journalOff == 0 {
                rc = SQLITE_OK;
            } else {
                rc = sqlite3OsTruncate(&mut pPager.jfd, 0);
                if rc == SQLITE_OK && pPager.fullSync != 0 {
                    // Make sure the new file size is written into the inode
                    // right away.  Otherwise the journal might resurrect
                    // following a power loss and cause the last
                    // transaction to roll back.  See
                    // https://bugzilla.mozilla.org/show_bug.cgi?id=1072773
                    rc = sqlite3OsSync(&mut pPager.jfd, pPager.syncFlags as i32);
                }
            }
            pPager.journalOff = 0;
        } else if pPager.journalMode as i32 == PAGER_JOURNALMODE_PERSIST
            || (pPager.exclusiveMode != 0 && (pPager.journalMode as i32) < PAGER_JOURNALMODE_WAL)
        {
            rc = zeroJournalHdr(pPager, (hasSuper != 0 || pPager.tempFile != 0) as i32);
            pPager.journalOff = 0;
        } else {
            // This branch may be executed with Pager.journalMode==MEMORY
            // if a hot-journal was just rolled back. In this case the
            // journal file should be closed and deleted. If this
            // connection writes to the database file, it will do so using
            // an in-memory journal.
            let bDelete = pPager.tempFile == 0;
            debug_assert!(sqlite3JournalIsInMemory(&pPager.jfd) == 0);
            debug_assert!(
                pPager.journalMode as i32 == PAGER_JOURNALMODE_DELETE
                    || pPager.journalMode as i32 == PAGER_JOURNALMODE_MEMORY
                    || pPager.journalMode as i32 == PAGER_JOURNALMODE_WAL
            );
            sqlite3OsClose(&mut pPager.jfd);
            if bDelete {
                let zJournal = pPager.zJournal.clone().unwrap_or_default();
                rc = sqlite3OsDelete(&pPager.pVfs, &zJournal, pPager.extraSync as i32);
            }
        }
    }

    pPager.pInJournal = None;
    pPager.nRec = 0;
    if rc == SQLITE_OK {
        if MEMDB(pPager) || pagerFlushOnCommit(pPager, bCommit) {
            sqlite3PcacheCleanAll(&mut pPager.pPCache);
        } else {
            sqlite3PcacheClearWritable(&mut pPager.pPCache);
        }
        let dbSize = pPager.dbSize;
        sqlite3PcacheTruncate(&mut pPager.pPCache, dbSize);
    }

    if pagerUseWal(pPager) {
        // Drop the WAL write-lock, if any. Also, if the connection was in
        // locking_mode=exclusive mode but is no longer, drop the EXCLUSIVE
        // lock held on the database file.
        rc2 = crate::wal::sqlite3WalEndWriteTransaction(pPager.pWal.as_mut().unwrap());
        debug_assert!(rc2 == SQLITE_OK);
    } else if rc == SQLITE_OK && bCommit != 0 && pPager.dbFileSize > pPager.dbSize {
        // This branch is taken when committing a transaction in rollback-
        // journal mode if the database file on disk is larger than the
        // database image.  At this point the journal has been finalized
        // and the transaction committed but the database file still has
        // a size larger than the transaction.  Truncate it now.
        debug_assert!(pPager.eLock as i32 == EXCLUSIVE_LOCK);
        let dbSize = pPager.dbSize;
        rc = pager_truncate(pPager, dbSize);
    }

    if rc == SQLITE_OK && bCommit != 0 {
        rc = sqlite3OsFileControl(&mut pPager.fd, SQLITE_FCNTL_COMMIT_PHASETWO, FcntlArg::None);
        if rc == SQLITE_NOTFOUND {
            rc = SQLITE_OK;
        }
    }

    if pPager.exclusiveMode == 0
        && (!pagerUseWal(pPager)
            || crate::wal::sqlite3WalExclusiveMode(pPager.pWal.as_mut().unwrap(), 0) != 0)
    {
        rc2 = pagerUnlockDb(pPager, SHARED_LOCK);
    }
    pPager.eState = PAGER_READER;
    pPager.setSuper = 0;

    if rc == SQLITE_OK { rc2 } else { rc }
}

/// Execute a rollback if a transaction is active and unlock the database
/// file.
///
/// If the pager has already entered the ERROR state, do not attempt the
/// rollback at this time. Instead, pager_unlock() is called. The call to
/// pager_unlock() will discard all in-memory pages, unlock the database
/// file and move the pager back to OPEN state. If this means that there
/// is a hot-journal left in the file-system, the next connection to
/// obtain a shared lock on the pager (which may be this one) will roll it
/// back.
///
/// If the pager has not already entered the ERROR state, but an IO or
/// malloc error occurs during a rollback, then this will itself cause
/// the pager to enter the ERROR state. Which will be cleared by the call
/// to pager_unlock(), as described above.
fn pagerUnlockAndRollback(pPager: &mut Pager) {
    if pPager.eState != PAGER_ERROR && pPager.eState != PAGER_OPEN {
        debug_assert!(assert_pager_state(pPager));
        if pPager.eState >= PAGER_WRITER_LOCKED {
            sqlite3PagerRollback(pPager);
        } else if pPager.exclusiveMode == 0 {
            debug_assert!(pPager.eState == PAGER_READER);
            pager_end_transaction(pPager, 0, 0);
        }
    } else if pPager.eState == PAGER_ERROR
        && pPager.journalMode as i32 == PAGER_JOURNALMODE_MEMORY
        && isOpen(&pPager.jfd)
    {
        // Special case:  If we hit an error while in the middle of a
        // rollback of an in-memory journal, attempt to finish the rollback.
        let errCode = pPager.errCode;
        let eLock = pPager.eLock;
        pPager.eState = PAGER_OPEN;
        pPager.errCode = SQLITE_OK;
        pPager.eLock = EXCLUSIVE_LOCK as u8;
        pager_playback(pPager, 1);
        pPager.errCode = errCode;
        pPager.eLock = eLock;
    }
    pager_unlock(pPager);
}

/// Parameter aData must point to a buffer of pPager->pageSize bytes of
/// data. Compute and return a checksum for the page of data.
///
/// This is not a real checksum. It is really just the sum of the random
/// initial value (pPager->cksumInit) and every 200th byte of the page
/// data, starting with byte offset (pPager->pageSize%200).
fn pager_cksum(pPager: &Pager, aData: &[u8]) -> u32 {
    let mut cksum = pPager.cksumInit; // Checksum value to return
    let mut i = pPager.pageSize as i32 - 200; // Loop counter
    while i > 0 {
        cksum = cksum.wrapping_add(aData[i as usize] as u32);
        i -= 200;
    }
    cksum
}

/// Read a single page from either the journal file (if isMainJrnl==1) or
/// from the sub-journal (if isMainJrnl==0) and playback that page.  The
/// page begins at offset *pOffset into the file. The *pOffset value is
/// increased to the next journal record (see pager.c for the details).
fn pager_playback_one_page(
    pPager: &mut Pager,
    pOffset: &mut i64,
    mut pDone: Option<&mut Bitvec>,
    isMainJrnl: i32,
    isSavepnt: i32,
) -> i32 {
    let mut rc;
    let mut pgno: Pgno = 0; // The page number of a page in journal
    let mut cksum: u32 = 0; // Checksum used for sanity checking
    let pageSize = pPager.pageSize as usize;
    let mut aData = vec![0u8; pageSize]; // Temporary storage for the page

    debug_assert!((isMainJrnl & !1) == 0); // isMainJrnl is 0 or 1
    debug_assert!((isSavepnt & !1) == 0); // isSavepnt is 0 or 1
    debug_assert!(isMainJrnl != 0 || pDone.is_some()); // pDone always used on sub-journals
    debug_assert!(isSavepnt != 0 || pDone.is_none()); // pDone never used on non-savepoint
    debug_assert!(!pagerUseWal(pPager) || (isMainJrnl == 0 && isSavepnt != 0));

    // Either the state is greater than PAGER_WRITER_CACHEMOD (a
    // transaction or savepoint rollback done at the request of the
    // caller) or this is a hot-journal rollback. If it is a hot-journal
    // rollback, the pager is in state OPEN and holds an EXCLUSIVE lock on
    // the database file.
    debug_assert!(
        pPager.eState >= PAGER_WRITER_CACHEMOD
            || (pPager.eState == PAGER_OPEN && pPager.eLock as i32 == EXCLUSIVE_LOCK)
    );
    debug_assert!(pPager.eState >= PAGER_WRITER_CACHEMOD || isMainJrnl != 0);

    // Read the page number and page data from the journal or sub-journal
    // file. Return an error code to the caller if an IO error occurs.
    {
        let jfd = if isMainJrnl != 0 {
            &mut pPager.jfd
        } else {
            &mut pPager.sjfd
        };
        rc = read32bits(jfd, *pOffset, &mut pgno);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = sqlite3OsRead(jfd, &mut aData, pageSize as i32, (*pOffset) + 4);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    *pOffset += pPager.pageSize + 4 + isMainJrnl as i64 * 4;

    // Sanity checking on the page.  This is more important that I
    // originally thought.  If a power failure occurs while the journal is
    // being written, it could cause invalid data to be written into the
    // journal.  We need to detect this invalid data (with high
    // probability) and ignore it.
    if pgno == 0 || pgno == PAGER_SJ_PGNO(pPager) {
        debug_assert!(isSavepnt == 0);
        return SQLITE_DONE;
    }
    if pgno > pPager.dbSize || sqlite3BitvecTestOpt(pDone.as_deref_mut(), pgno) != 0 {
        return SQLITE_OK;
    }
    if isMainJrnl != 0 {
        rc = read32bits(&mut pPager.jfd, (*pOffset) - 4, &mut cksum);
        if rc != 0 {
            return rc;
        }
        if isSavepnt == 0 && pager_cksum(pPager, &aData) != cksum {
            return SQLITE_DONE;
        }
    }

    // If this page has already been played back before during the current
    // rollback, then don't bother to play it back again.
    if let Some(d) = pDone.as_deref_mut() {
        rc = sqlite3BitvecSet(d, pgno);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // When playing back page 1, restore the nReserve setting
    if pgno == 1 && pPager.nReserve != aData[20] as i16 {
        pPager.nReserve = aData[20] as i16;
    }

    // If the pager is in CACHEMOD state, then there must be a copy of this
    // page in the pager cache. In this case just update the pager cache,
    // not the database file. The page is left marked dirty in this case.
    //
    // An exception to the above rule: If the database is in no-sync mode
    // and a page is moved during an incremental vacuum then the page may
    // not be in the pager cache. Later: if a malloc() or IO error occurs
    // during a Movepage() call, then the page may not be in the cache
    // either. So the condition described in the above paragraph is not
    // assert()able.
    //
    // If in WRITER_DBMOD, WRITER_FINISHED or OPEN state, then we update
    // the pager cache if it exists and the main file. The page is then
    // marked not dirty. Since the page cache is not cleared, it is
    // possible that a page in the main file and the page cache will not be
    // the same.
    //
    // Ticket #1171:  The statement journal might contain page content that
    // is different from the page content at the start of the transaction.
    // This occurs when a page is changed prior to the start of a statement
    // then changed again within the statement.  When rolling back such a
    // statement we must not write to the original database unless we know
    // for certain that original page contents are synced into the main
    // rollback journal.  Otherwise, a power loss might leave modified data
    // in the database file without an entry in the rollback journal that
    // can restore the database to its original form.  Two conditions must
    // be met before writing to the database files. (1) the database must
    // be locked.  (2) we know that the original page content is fully
    // synced in the main journal either because the page is not in cache
    // or else the page is marked as needSync==0.
    //
    // 2008-04-14:  When attempting to vacuum a corrupt database file, it
    // is possible to fail a statement on a database that does not yet
    // exist. Do not attempt to write if database file has never been
    // opened.
    let mut pPg: Option<DbPage> = if pagerUseWal(pPager) {
        None
    } else {
        sqlite3PagerLookup(pPager, pgno)
    };
    debug_assert!(pPg.is_some() || !MEMDB(pPager));
    debug_assert!(pPager.eState != PAGER_OPEN || pPg.is_none() || pPager.tempFile != 0);
    let isSynced = if isMainJrnl != 0 {
        pPager.noSync != 0 || (*pOffset <= pPager.journalHdr)
    } else {
        match &pPg {
            None => true,
            Some(p) => 0 == (pgflags(p) & PGHDR_NEED_SYNC),
        }
    };
    if isOpen(&pPager.fd)
        && (pPager.eState >= PAGER_WRITER_DBMOD || pPager.eState == PAGER_OPEN)
        && isSynced
    {
        let ofst = (pgno as i64 - 1) * pPager.pageSize;
        debug_assert!(!pagerUseWal(pPager));

        // Write the data read from the journal back into the database
        // file.  This is usually safe even for an encrypted database - as
        // the data was encrypted before it was written to the journal
        // file. The exception is when the database was opened with
        // SQLITE_OPEN_READONLY and is not a temp file.
        rc = sqlite3OsWrite(&mut pPager.fd, &aData, pageSize as i32, ofst);

        if pgno > pPager.dbFileSize {
            pPager.dbFileSize = pgno;
        }
        backup_update(pPager, pgno, &aData);
    } else if isMainJrnl == 0 && pPg.is_none() {
        // If this is a rollback of a savepoint and data was not written to
        // the database and the page is not in-memory, there is a potential
        // problem. When the page is next fetched by the b-tree layer, it
        // will be read from the database file, which may or may not be
        // current.
        //
        // There are a couple of different ways this can happen. All are
        // quite obscure. When running in synchronous mode, this can only
        // happen if the page is on the free-list at the start of the
        // transaction, then populated, then moved using
        // sqlite3PagerMovepage().
        //
        // The solution is to add an in-memory page to the cache containing
        // the data just read from the sub-journal. Mark the page as dirty
        // and if the pager requires a journal-sync, then mark the page as
        // requiring a journal-sync before it is written.
        debug_assert!(isSavepnt != 0);
        debug_assert!((pPager.doNotSpill & SPILLFLAG_ROLLBACK) == 0);
        pPager.doNotSpill |= SPILLFLAG_ROLLBACK;
        let mut p = None;
        rc = sqlite3PagerGet(pPager, pgno, &mut p, 1);
        debug_assert!((pPager.doNotSpill & SPILLFLAG_ROLLBACK) != 0);
        pPager.doNotSpill &= !SPILLFLAG_ROLLBACK;
        if rc != SQLITE_OK {
            return rc;
        }
        let p = p.unwrap();
        sqlite3PcacheMakeDirty(&mut pPager.pPCache, &p);
        pPg = Some(p);
    }
    if let Some(pPg) = pPg {
        // No page should ever be explicitly rolled back that is in use,
        // except for page 1 which is held in use in order to keep the lock
        // on the database active. However such a page may be rolled back
        // as a result of an internal error resulting in an automatic call
        // to sqlite3PagerRollback().
        {
            let mut pg = pPg.borrow_mut();
            pg.aData[..pageSize].copy_from_slice(&aData);
        }
        if let Some(x) = pPager.xReiniter {
            x(&pPg);
        }
        // It used to be that sqlite3PcacheMakeClean(pPg) was called here.
        // But that is not correct: the page might be marked as needing to
        // be journaled even if it is being rolled back.

        // If this was page 1, then restore the value of
        // Pager.dbFileVers.  Do this before any decoding.
        if pgno == 1 {
            let pg = pPg.borrow();
            pPager.dbFileVers.copy_from_slice(&pg.aData[24..40]);
        }
        sqlite3PcacheRelease(&mut pPager.pPCache, &pPg);
    }
    rc
}

/// Parameter zSuper is the name of a super-journal file. A single
/// journal file that referred to the super-journal file has just been
/// rolled back. This routine checks if it is possible to delete the
/// super-journal file, and does so if it is.
///
/// Argument zSuper may point to Pager.pTmpSpace. So that buffer is not
/// available for use within this function.
///
/// When a super-journal file is created, it is populated with the names
/// of all of its child journals, one after another, formatted as utf-8
/// encoded text. The end of each child journal file is marked with a
/// nul-terminator byte (0x00). i.e. the entire contents of a super-journal
/// file for a transaction involving two databases might be:
///
///   "/home/bill/a.db-journal\x00/home/bill/b.db-journal\x00"
///
/// A super-journal file may only be deleted once all of its child
/// journals have been rolled back.
///
/// This function reads the contents of the super-journal file into
/// memory and loops through each of the child journal names. For each
/// child journal, it checks if:
///
///   * if the child journal exists, and if so
///   * if the child journal contains a reference to super-journal
///     file zSuper
///
/// If a child journal can be found that matches both of the criteria
/// above, this function returns without doing anything. Otherwise, if
/// no such child journal can be found, file zSuper is deleted from the
/// file-system and SQLITE_OK is returned.
///
/// If an IO error within this function, an error code is returned. This
/// function allocates memory by calling sqlite3Malloc(). If an allocation
/// fails, SQLITE_NOMEM is returned. Otherwise, if no IO or malloc errors
/// occur, SQLITE_OK is returned.
///
/// TODO: This function allocates a single block of memory to load the
/// entire contents of the super-journal file. This could be a couple of
/// kilobytes or so - potentially larger than the page size.
fn pager_delsuper(pPager: &mut Pager, zSuper: &[u8]) -> i32 {
    let pVfs = pPager.pVfs.clone();
    let mut rc;
    let mut pSuper = SqliteFile::Closed; // Malloc'd super-journal file descriptor
    let mut pJournal = SqliteFile::Closed; // Malloc'd child-journal file descriptor
    let mut nSuperJournal: i64 = 0; // Size of super-journal file
    let mut bSeen = false; // True if a journal matching zJournal has been seen

    if !pagerIsSuperJrnlName(zSuper) {
        return SQLITE_OK;
    }

    'delsuper_out: {
        let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_SUPER_JOURNAL;
        rc = sqlite3OsOpen(&pVfs, Some(zSuper), &mut pSuper, flags, None);
        if rc != SQLITE_OK {
            break 'delsuper_out;
        }

        // Load the entire super-journal file into space obtained from
        // sqlite3_malloc() and pointed to by zSuperJournal.   Also obtain
        // sufficient space (in zSuperPtr) to hold the names of super-journal
        // files extracted from regular rollback-journals.
        rc = sqlite3OsFileSize(&mut pSuper, &mut nSuperJournal);
        if rc != SQLITE_OK {
            break 'delsuper_out;
        }
        debug_assert!(nSuperJournal >= 0);
        let mut zSuperJournal = vec![0u8; nSuperJournal as usize];
        if nSuperJournal > 0 {
            rc = sqlite3OsRead(&mut pSuper, &mut zSuperJournal, nSuperJournal as i32, 0);
            if rc != SQLITE_OK {
                break 'delsuper_out;
            }
        }
        let zMyJournal = pPager.zJournal.clone().unwrap_or_default();
        let mut off = 0usize;
        while off < zSuperJournal.len() {
            let zJournal = zpath_name(&zSuperJournal[off..]).to_vec();
            if zJournal == zMyJournal {
                bSeen = true;
            } else {
                let mut exists = 0;
                rc = sqlite3OsAccess(&pVfs, &zJournal, SQLITE_ACCESS_EXISTS, &mut exists);
                if rc != SQLITE_OK {
                    break 'delsuper_out;
                }
                if exists != 0 {
                    // One of the journals pointed to by the super-journal
                    // exists.  Open it and check if it points at the
                    // super-journal. If so, return without deleting the
                    // super-journal file.  NB: zJournal is really a
                    // MAIN_JOURNAL.  But SUPER_JOURNAL here is a
                    // terminology hack used to avoid calling
                    // findCreateFileMode() in os_unix.c.
                    let mut zSuperPtr: Option<ZStr> = None;
                    let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_SUPER_JOURNAL;
                    rc = sqlite3OsOpen(&pVfs, Some(&zJournal), &mut pJournal, flags, None);
                    if rc != SQLITE_OK {
                        break 'delsuper_out;
                    }

                    rc =
                        readSuperJournal(&mut pJournal, 1 + pVfs.mxPathname as u64, &mut zSuperPtr);
                    sqlite3OsClose(&mut pJournal);
                    if rc != SQLITE_OK {
                        break 'delsuper_out;
                    }

                    let c = zSuperPtr.as_deref() == Some(zpath_name(zSuper));
                    if c {
                        // We have a match. Do not delete the super-journal.
                        break 'delsuper_out;
                    }
                }
            }
            off += zJournal.len() + 1;
        }

        sqlite3OsClose(&mut pSuper);
        if bSeen {
            rc = sqlite3OsDelete(&pVfs, zSuper, 0);
        }
    }
    sqlite3OsClose(&mut pSuper);
    debug_assert!(!isOpen(&pJournal));
    rc
}

/// This function is used to change the actual size of the database file
/// in the file-system. This only happens when committing a transaction,
/// or rolling back a transaction (including rolling back a hot-journal).
///
/// If the main database file is not open, or the pager is not in either
/// DBMOD or OPEN state, this function is a no-op. Otherwise, the size of
/// the file is changed to nPage pages (nPage*pPager->pageSize bytes). If
/// the file on disk is currently larger than nPage pages, then use the
/// VFS xTruncate() method to truncate it.
///
/// Or, it might be the case that the file on disk is smaller than
/// nPage pages. Some operating system implementations can get confused
/// if you try to truncate a file to some size that is larger than it
/// currently is, so detect this case and write a single zero byte to
/// the end of the new file instead.
///
/// If successful, return SQLITE_OK. If an IO error occurs while modifying
/// the database file, return the error code to the caller.
fn pager_truncate(pPager: &mut Pager, nPage: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pPager.eState != PAGER_ERROR);
    debug_assert!(pPager.eState != PAGER_READER);

    if isOpen(&pPager.fd) && (pPager.eState >= PAGER_WRITER_DBMOD || pPager.eState == PAGER_OPEN) {
        let mut currentSize = 0i64;
        let szPage = pPager.pageSize as i32;
        debug_assert!(pPager.eLock as i32 == EXCLUSIVE_LOCK);
        // TODO: Is it safe to use Pager.dbFileSize here?
        rc = sqlite3OsFileSize(&mut pPager.fd, &mut currentSize);
        let mut newSize = szPage as i64 * nPage as i64;
        if rc == SQLITE_OK && currentSize != newSize {
            if currentSize > newSize {
                rc = sqlite3OsTruncate(&mut pPager.fd, newSize);
            } else if (currentSize + szPage as i64) <= newSize {
                let pTmp = vec![0u8; szPage as usize];
                sqlite3OsFileControlHint(
                    &mut pPager.fd,
                    SQLITE_FCNTL_SIZE_HINT,
                    FcntlArg::I64(&mut newSize),
                );
                rc = sqlite3OsWrite(&mut pPager.fd, &pTmp, szPage, newSize - szPage as i64);
            }
            if rc == SQLITE_OK {
                pPager.dbFileSize = nPage;
            }
        }
    }
    rc
}

/// C: `int sqlite3SectorSize(sqlite3_file *pFile)`
///
/// Return a sanitized version of the sector-size of OS file pFile. The
/// return value is guaranteed to lie between 32 and MAX_SECTOR_SIZE.
pub fn sqlite3SectorSize(pFile: &mut crate::os::SqliteFile) -> i32 {
    let mut iRet = sqlite3OsSectorSize(pFile);
    if iRet < 32 {
        iRet = 512;
    } else if iRet > MAX_SECTOR_SIZE as i32 {
        iRet = MAX_SECTOR_SIZE as i32;
    }
    iRet
}

/// Set the value of the Pager.sectorSize variable for the given pager
/// based on the value returned by the xSectorSize method of the open
/// database file. The sector size will be used to determine the size and
/// alignment of journal header and super-journal pointers within created
/// journal files.
///
/// For temporary files the effective sector size is always 512 bytes.
///
/// Otherwise, for non-temporary files, the effective sector size is the
/// value returned by the xSectorSize() method rounded up to 32 if it is
/// less than 32, or rounded down to MAX_SECTOR_SIZE if it is greater than
/// MAX_SECTOR_SIZE.
///
/// If the file has the SQLITE_IOCAP_POWERSAFE_OVERWRITE property, then
/// the effective sector size is always 512.
fn setSectorSize(pPager: &mut Pager) {
    debug_assert!(isOpen(&pPager.fd) || pPager.tempFile != 0);
    if pPager.tempFile != 0
        || (sqlite3OsDeviceCharacteristics(&mut pPager.fd) & SQLITE_IOCAP_POWERSAFE_OVERWRITE) != 0
    {
        // Sector size doesn't matter for temporary files. Also, the file
        // may not have been opened yet, in which case the OsSectorSize()
        // call will segfault.
        pPager.sectorSize = 512;
    } else {
        pPager.sectorSize = sqlite3SectorSize(&mut pPager.fd) as u32;
    }
}

/// Playback the journal and thus restore the database file to the state
/// it was in before we started making changes.
///
/// The journal file format is as follows:
///
///  (1)  8 byte prefix.  A copy of aJournalMagic[].
///  (2)  4 byte big-endian integer which is the number of valid page
///       records in the journal.  If this value is 0xffffffff, then
///       compute the number of page records from the journal size.
///  (3)  4 byte big-endian integer which is the initial value for the
///       sanity checksum.
///  (4)  4 byte integer which is the number of pages to truncate the
///       database to during a rollback.
///  (5)  4 byte big-endian integer which is the sector size.  The header
///       is this many bytes in size.
///  (6)  4 byte big-endian integer which is the page size.
///  (7)  zero padding out to the next sector size.
///  (8)  Zero or more pages instances, each as follows:
///        +  4 byte page number.
///        +  pPager->pageSize bytes of data.
///        +  4 byte checksum
///
/// When we speak of the journal header, we mean the first 7 items above.
/// Each entry in the journal is an instance of the 8th item.
///
/// See pager.c for the full description of hot-journal handling.
fn pager_playback(pPager: &mut Pager, isHot: i32) -> i32 {
    let pVfs = pPager.pVfs.clone();
    let mut szJ: i64 = 0; // Size of the journal file in bytes
    let mut nRec: u32 = 0; // Number of Records in the journal
    let mut mxPg: Pgno = 0; // Size of the original file in pages
    let mut rc; // Result code of a subroutine
    let mut res = 1; // Value returned by sqlite3OsAccess()
    let mut zSuper: Option<ZStr> = None; // Name of super-journal file if any
    let mut nPlayback = 0; // Total number of pages restored from journal
    let mut savedPageSize = pPager.pageSize as u32;

    // Figure out how many records are in the journal.  Abort early if the
    // journal is empty.
    debug_assert!(isOpen(&pPager.jfd));
    'end_playback: {
        rc = sqlite3OsFileSize(&mut pPager.jfd, &mut szJ);
        if rc != SQLITE_OK {
            break 'end_playback;
        }

        // Read the super-journal name from the journal, if it is present.
        // If a super-journal file name is specified, but the file is not
        // present on disk, then the journal is not hot and does not need
        // to be played back.
        //
        // TODO: Technically the following is an error because it assumes
        // that buffer Pager.pTmpSpace is (mxPathname+1) bytes or larger.
        // i.e. that (pPager->pageSize >= pPager->pVfs->mxPathname+1).
        // mxPathname is 512, which is the same as the minimum allowable
        // value for pageSize.
        rc = readSuperJournal(&mut pPager.jfd, 1 + pVfs.mxPathname as u64, &mut zSuper);
        if rc == SQLITE_OK {
            if let Some(z) = &zSuper {
                rc = sqlite3OsAccess(&pVfs, z, SQLITE_ACCESS_EXISTS, &mut res);
            }
        }
        if rc != SQLITE_OK || res == 0 {
            break 'end_playback;
        }
        pPager.journalOff = 0;
        let mut needPagerReset = isHot != 0;

        // This loop terminates either when a readJournalHdr() or
        // pager_playback_one_page() call returns SQLITE_DONE or an IO
        // error occurs.
        loop {
            // Read the next journal header from the journal file.  If there
            // are not enough bytes left in the journal file for a complete
            // header, or it is corrupted, then a process must have failed
            // while writing it. This indicates nothing more needs to be
            // rolled back.
            rc = readJournalHdr(pPager, isHot, szJ, &mut nRec, &mut mxPg);
            if rc != SQLITE_OK {
                if rc == SQLITE_DONE {
                    rc = SQLITE_OK;
                }
                break 'end_playback;
            }

            // If nRec is 0xffffffff, then this journal was created by a
            // process working in no-sync mode. This means that the rest of
            // the journal file consists of pages, there are no more journal
            // headers. Compute the value of nRec based on this assumption.
            if nRec == 0xffffffff {
                debug_assert!(pPager.journalOff == JOURNAL_HDR_SZ(pPager));
                nRec = ((szJ - JOURNAL_HDR_SZ(pPager)) / JOURNAL_PG_SZ(pPager)) as u32;
            }

            // If nRec is 0 and this rollback is of a transaction created by
            // this process and if this is the final header in the journal,
            // then it means that this part of the journal was being filled
            // but has not yet been synced to disk.  Compute the number of
            // pages based on the remaining size of the file.
            //
            // The third term of the test was added to fix ticket #2565.
            // When rolling back a hot journal, nRec==0 always means that
            // the next chunk of the journal contains zero pages to be
            // rolled back.  But when doing a ROLLBACK and the nRec==0 chunk
            // is the last chunk in the journal, it means that the journal
            // might contain additional pages that need to be rolled back
            // and that the number of pages should be computed based on the
            // journal file size.
            if nRec == 0
                && isHot == 0
                && pPager.journalHdr + JOURNAL_HDR_SZ(pPager) == pPager.journalOff
            {
                nRec = ((szJ - pPager.journalOff) / JOURNAL_PG_SZ(pPager)) as u32;
            }

            // If this is the first header read from the journal, truncate
            // the database file back to its original size.
            if pPager.journalOff == JOURNAL_HDR_SZ(pPager) {
                rc = pager_truncate(pPager, mxPg);
                if rc != SQLITE_OK {
                    break 'end_playback;
                }
                pPager.dbSize = mxPg;
                if pPager.mxPgno < mxPg {
                    pPager.mxPgno = mxPg;
                }
            }

            // Copy original pages out of the journal and back into the
            // database file and/or page cache.
            let mut u = 0u32;
            while u < nRec {
                if needPagerReset {
                    pager_reset(pPager);
                    needPagerReset = false;
                }
                let mut off = pPager.journalOff;
                rc = pager_playback_one_page(pPager, &mut off, None, 1, 0);
                pPager.journalOff = off;
                if rc == SQLITE_OK {
                    nPlayback += 1;
                } else if rc == SQLITE_DONE {
                    pPager.journalOff = szJ;
                    break;
                } else if rc == SQLITE_IOERR_SHORT_READ {
                    // If the journal has been truncated, simply stop
                    // reading and processing the journal. This might
                    // happen if the journal was not fully synced, in
                    // which case the remainder can be ignored.
                    rc = SQLITE_OK;
                    break 'end_playback;
                } else {
                    // If we are unable to rollback, quit and return the
                    // error code.  This will cause the pager to enter the
                    // error state.
                    break 'end_playback;
                }
                u += 1;
            }
        }
    }
    // Following a rollback, the database file should be back in its
    // original state prior to the start of the transaction, so invoke the
    // SQLITE_FCNTL_DB_UNCHANGED file-control method to disable the
    // assertion that the transaction counter was modified (debug only).
    if rc == SQLITE_OK {
        rc = sqlite3PagerSetPagesize(pPager, &mut savedPageSize, -1);
    }

    // If this playback is happening automatically as a result of an IO or
    // malloc error that occurred after the change-counter was updated but
    // before the transaction was committed, then the change-counter
    // modification may just have been reverted. If this happens in
    // exclusive mode, then subsequent transactions performed by the
    // connection will not update the change-counter at all. This may lead
    // to cache inconsistency problems for other processes at some point.
    // To prevent this, unset pPager->changeCountDone here (via
    // pager_end_transaction()).
    pPager.changeCountDone = pPager.tempFile;

    if rc == SQLITE_OK && (pPager.eState >= PAGER_WRITER_DBMOD || pPager.eState == PAGER_OPEN) {
        rc = sqlite3PagerSync(pPager, None);
    }
    if rc == SQLITE_OK {
        rc = pager_end_transaction(pPager, zSuper.is_some() as i32, 0);
    }
    if rc == SQLITE_OK && res != 0 {
        if let Some(z) = &zSuper {
            // If there was a super-journal and this routine will return
            // success, see if it is possible to delete the super-journal.
            rc = pager_delsuper(pPager, z);
        }
    }
    if isHot != 0 && nPlayback != 0 {
        let zJournal = pPager.zJournal.clone().unwrap_or_default();
        crate::printf::sqlite3_log(
            SQLITE_NOTICE_RECOVER_ROLLBACK,
            b"recovered %d pages from %s",
            &[
                crate::printf::PfArg::from(nPlayback as i32),
                crate::printf::PfArg::from(&zJournal[..]),
            ],
        );
    }

    // The Pager.sectorSize variable may have been updated while rolling
    // back a journal created by a process with a different sector size
    // value. Reset it to the correct value for this process.
    setSectorSize(pPager);
    rc
}

/// Read the content for page pPg out of the database file (or out of the
/// WAL if that is where the most recent copy if found) into pPg->pData.
/// A shared lock or greater must be held on the database file before this
/// function is called.
///
/// If page 1 is read, then the value of Pager.dbFileVers[] is set to the
/// value read from the database file.
///
/// If an IO error occurs, then the IO error is returned to the caller.
/// Otherwise, SQLITE_OK is returned.
fn readDbPage(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc;
    let mut iFrame: u32 = 0; // Frame of WAL containing pgno
    let pgno = pgpgno(pPg);

    debug_assert!(pPager.eState >= PAGER_READER && !MEMDB(pPager));
    debug_assert!(isOpen(&pPager.fd));

    if let Some(w) = pPager.pWal.as_mut() {
        rc = crate::wal::sqlite3WalFindFrame(w, pgno, &mut iFrame);
        if rc != 0 {
            return rc;
        }
    }
    let pageSize = pPager.pageSize as i32;
    if iFrame != 0 {
        let mut pg = pPg.borrow_mut();
        rc = crate::wal::sqlite3WalReadFrame(
            pPager.pWal.as_mut().unwrap(),
            iFrame,
            pageSize,
            &mut pg.aData,
        );
    } else {
        let iOffset = (pgno as i64 - 1) * pPager.pageSize;
        let mut pg = pPg.borrow_mut();
        rc = sqlite3OsRead(&mut pPager.fd, &mut pg.aData, pageSize, iOffset);
        if rc == SQLITE_IOERR_SHORT_READ {
            rc = SQLITE_OK;
        }
    }

    if pgno == 1 {
        if rc != 0 {
            // If the read is unsuccessful, set the dbFileVers[] to
            // something that will never be a valid file version.
            // dbFileVers[] is a copy of bytes 24..39 of the database.
            // Bytes 27..31 hold the change counter, and bytes 92..95 hold
            // the version number.  The 0xff bytes will never match a valid
            // file version.
            pPager.dbFileVers = [0xff; 16];
        } else {
            let pg = pPg.borrow();
            pPager.dbFileVers.copy_from_slice(&pg.aData[24..40]);
        }
    }
    pPager.nRead += 1;
    rc
}

/// Update the value of the change-counter at offsets 24 and 92 in the
/// header and the sqlite version number at offset 96.
///
/// This is an unconditional update.  See also the
/// pager_incr_changecounter() routine which only updates the
/// change-counter if the update is actually needed, as determined by the
/// pPager->changeCountDone state variable.
fn pager_write_changecounter(dbFileVers: &[u8; 16], pPg: &DbPage) {
    // Increment the value just read and write it back to byte 24.
    let change_counter = get4byte(dbFileVers).wrapping_add(1);
    let mut pg = pPg.borrow_mut();
    put32bits(&mut pg.aData[24..], change_counter);

    // Also store the SQLite version number in bytes 96..99 and in bytes
    // 92..95 store the change counter for which the version number is
    // valid.
    put32bits(&mut pg.aData[92..], change_counter);
    put32bits(&mut pg.aData[96..], SQLITE_VERSION_NUMBER as u32);
}

/// This function is invoked once for each page that has already been
/// written into the log file when a WAL transaction is rolled back.
/// Parameter iPg is the page number of said page. The pCtx argument is
/// actually a pointer to the Pager structure.
///
/// If page iPg is present in the cache, and has no outstanding
/// references, it is discarded. Otherwise, if there are one or more
/// outstanding references, the page content is reloaded from the
/// database. If the attempt to reload content from the database is
/// required and fails, return an SQLite error code. Otherwise, SQLITE_OK.
fn pagerUndoCallback(pPager: &mut Pager, iPg: Pgno) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pagerUseWal(pPager));
    if let Some(pPg) = sqlite3PagerLookup(pPager, iPg) {
        if sqlite3PcachePageRefcount(&pPg) == 1 {
            sqlite3PcacheDrop(&mut pPager.pPCache, &pPg);
        } else {
            rc = readDbPage(pPager, &pPg);
            if rc == SQLITE_OK {
                if let Some(x) = pPager.xReiniter {
                    x(&pPg);
                }
            }
            pagerUnrefNotNull(pPager, &pPg);
        }
    }

    // Normally, if a transaction is rolled back, any backup processes are
    // updated as data is copied out of the rollback journal and into the
    // database. This is not generally possible with a WAL database, as
    // rollback involves simply truncating the log file. Therefore, if one
    // or more frames have already been written to the log (and therefore
    // also copied into the backup databases) as part of this transaction,
    // the backups must be restarted.
    backup_restart(pPager);
    rc
}

/// This function is called to rollback a transaction on a WAL database.
fn pagerRollbackWal(pPager: &mut Pager) -> i32 {
    // For all pages in the cache that are currently dirty or have already
    // been written (but not committed) to the log file, do one of the
    // following:
    //
    //   + Discard the cached page (if refcount==0), or
    //   + Reload page content from the database (if refcount>0).
    pPager.dbSize = pPager.dbOrigSize;
    let mut undone: Vec<Pgno> = Vec::new();
    let mut rc = crate::wal::sqlite3WalUndo(pPager.pWal.as_mut().unwrap(), &mut |pg| {
        undone.push(pg);
        SQLITE_OK
    });
    for pg in undone {
        if rc != SQLITE_OK {
            break;
        }
        rc = pagerUndoCallback(pPager, pg);
    }
    let pList = sqlite3PcacheDirtyList(&mut pPager.pPCache);
    for p in pList {
        if rc != SQLITE_OK {
            break;
        }
        let pgno = pgpgno(&p);
        rc = pagerUndoCallback(pPager, pgno);
    }
    rc
}

/// This function is a wrapper around sqlite3WalFrames(). As well as
/// logging the contents of the list of pages headed by pList (connected
/// by pDirty), this function notifies any active backup processes that
/// the pages have changed.
///
/// The list of pages passed into this routine is always sorted by page
/// number.  Hence, if page 1 appears anywhere on the list, it will be the
/// first page.
fn pagerWalFrames(pPager: &mut Pager, pList: &[DbPage], nTruncate: Pgno, isCommit: i32) -> i32 {
    debug_assert!(pPager.pWal.is_some());
    debug_assert!(!pList.is_empty());
    debug_assert!(pList.len() == 1 || isCommit != 0);
    let list: Vec<DbPage> = if isCommit != 0 {
        // If a WAL transaction is being committed, there is no point in
        // writing any pages with page numbers greater than nTruncate into
        // the WAL file. They will never be read by any client. So remove
        // them from the pDirty list here.
        pList
            .iter()
            .filter(|p| pgpgno(p) <= nTruncate)
            .cloned()
            .collect()
    } else {
        pList[..1].to_vec()
    };
    pPager.aStat[PAGER_STAT_WRITE] = pPager.aStat[PAGER_STAT_WRITE].wrapping_add(list.len() as u32);

    if let Some(first) = list.first() {
        if pgpgno(first) == 1 {
            let v = pPager.dbFileVers;
            pager_write_changecounter(&v, first);
        }
    }
    let pageSize = pPager.pageSize as i32;
    let walSyncFlags = pPager.walSyncFlags as i32;
    let rc = crate::wal::sqlite3WalFrames(
        pPager.pWal.as_mut().unwrap(),
        pageSize,
        &list,
        nTruncate,
        isCommit,
        walSyncFlags,
    );
    if rc == SQLITE_OK && !pPager.pBackup.is_empty() {
        for p in &list {
            let (pgno, data) = {
                let b = p.borrow();
                (b.pgno, b.aData.clone())
            };
            backup_update(pPager, pgno, &data);
        }
    }
    rc
}

/// Begin a read transaction on the WAL.
///
/// This routine used to be called "pagerOpenSnapshot()" because it
/// essentially makes a snapshot of the database at the current point in
/// time and assumes that the content of the database file will not
/// change for the duration of the read transaction.
fn pagerBeginReadTransaction(pPager: &mut Pager) -> i32 {
    let mut changed = 0; // True if cache must be reset

    debug_assert!(pagerUseWal(pPager));
    debug_assert!(pPager.eState == PAGER_OPEN || pPager.eState == PAGER_READER);

    // sqlite3WalEndReadTransaction() was not called for the previous
    // transaction in locking_mode=EXCLUSIVE.  So call it now.  If we are
    // in locking_mode=NORMAL and EndRead() was previously called, the
    // duplicate call is harmless.
    let w = pPager.pWal.as_mut().unwrap();
    crate::wal::sqlite3WalEndReadTransaction(w);

    let rc = crate::wal::sqlite3WalBeginReadTransaction(w, &mut changed);
    if rc != SQLITE_OK || changed != 0 {
        pager_reset(pPager);
    }
    rc
}

/// This function is called as part of the transition from PAGER_OPEN to
/// PAGER_READER state to determine the size of the database file in
/// pages, assuming there is no super-journal. If there is a WAL file,
/// the size of the database is determined from it.
///
/// Return SQLITE_OK or an error code.
fn pagerPagecount(pPager: &mut Pager, pnPage: &mut Pgno) -> i32 {
    // Query the WAL sub-system for the database size. The WalDbsize()
    // function returns zero if the WAL is not open (i.e. Pager.pWal==0),
    // or if the database size is not available. The database size is not
    // available from the WAL sub-system if the log file is empty or
    // contains no valid committed transactions.
    debug_assert!(pPager.eState == PAGER_OPEN);
    debug_assert!(pPager.eLock as i32 >= SHARED_LOCK);
    debug_assert!(isOpen(&pPager.fd));
    debug_assert!(pPager.tempFile == 0);
    let mut nPage = match pPager.pWal.as_mut() {
        Some(w) => crate::wal::sqlite3WalDbsize(w),
        None => 0,
    };

    // If the number of pages in the database is not available from the
    // WAL sub-system, determine the page count based on the size of the
    // file on disk.
    if nPage == 0 && isOpen(&pPager.fd) {
        let mut n: i64 = 0; // Size of db file in bytes
        let rc = sqlite3OsFileSize(&mut pPager.fd, &mut n);
        if rc != SQLITE_OK {
            return rc;
        }
        nPage = ((n + pPager.pageSize - 1) / pPager.pageSize) as Pgno;
    }

    // If the current number of pages in the file is greater than the
    // configured maximum pager number, increase the allowed limit so that
    // the file can be read.
    if nPage > pPager.mxPgno {
        pPager.mxPgno = nPage;
    }

    *pnPage = nPage;
    SQLITE_OK
}

/// Check if the *-wal file that corresponds to the database opened by
/// pPager exists if the database is not empty, or verify that the *-wal
/// file does not exist (by deleting it) if the database file is empty.
///
/// If the database is not empty and the *-wal file exists, open the pager
/// in WAL mode.  If the database is empty or if no *-wal file exists and
/// if no error occurs, make sure Pager.journalMode is not set to
/// PAGER_JOURNALMODE_WAL.
///
/// Return SQLITE_OK or an error code.
///
/// The caller must hold a SHARED lock on the database file to call this
/// function. Because an EXCLUSIVE lock on the db file is required to
/// delete a WAL while it is in use, the existence of the WAL is not
/// changed as long as the SHARED lock is held.
fn pagerOpenWalIfPresent(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(pPager.eState == PAGER_OPEN);
    debug_assert!(pPager.eLock as i32 >= SHARED_LOCK);

    if pPager.tempFile == 0 {
        let mut isWal = 0; // True if WAL file exists
        let zWal = pPager.zWal.clone().unwrap_or_default();
        rc = sqlite3OsAccess(&pPager.pVfs, &zWal, SQLITE_ACCESS_EXISTS, &mut isWal);
        if rc == SQLITE_OK {
            if isWal != 0 {
                let mut nPage: Pgno = 0; // Size of the database file
                rc = pagerPagecount(pPager, &mut nPage);
                if rc != 0 {
                    return rc;
                }
                if nPage == 0 {
                    rc = sqlite3OsDelete(&pPager.pVfs, &zWal, 0);
                } else {
                    rc = sqlite3PagerOpenWal(pPager, None);
                }
            } else if pPager.journalMode as i32 == PAGER_JOURNALMODE_WAL {
                pPager.journalMode = PAGER_JOURNALMODE_DELETE as u8;
            }
        }
    }
    rc
}

/// Playback savepoint pSavepoint. Or, if pSavepoint==NULL, then playback
/// the entire super-journal file. The case pSavepoint==NULL occurs when
/// a ROLLBACK TO command is invoked on a SAVEPOINT that is a transaction
/// savepoint.
///
/// When pSavepoint is not NULL (meaning a non-transaction savepoint is
/// being rolled back), then the rollback consists of up to three stages,
/// performed in the order specified:
///
///   * Pages are played back from the main journal starting at byte
///     offset PagerSavepoint.iOffset and continuing to
///     PagerSavepoint.iHdrOffset, or to the end of the main journal file
///     if PagerSavepoint.iHdrOffset is zero.
///
///   * If PagerSavepoint.iHdrOffset is not zero, then pages are played
///     back starting from the journal header immediately following
///     PagerSavepoint.iHdrOffset to the end of the main journal file.
///
///   * Pages are then played back from the sub-journal file, starting
///     with the PagerSavepoint.iSubRec and continuing to the end of the
///     journal file.
///
/// Throughout the rollback process, each time a page is rolled back, the
/// corresponding bit is set in a bitvec structure (variable pDone in the
/// implementation below). This is used to ensure that a page is only
/// rolled back the first time it is encountered in either journal.
///
/// If pSavepoint is NULL, then pages are only played back from the main
/// journal file. There is no need for a bitvec in this case.
///
/// In either case, before playback commences the Pager.dbSize variable
/// is reset to the value that it held at the start of the savepoint (or
/// transaction). No page with a page-number greater than this value is
/// played back. If one is encountered it is simply skipped.
fn pagerPlaybackSavepoint(pPager: &mut Pager, iSavepoint: Option<usize>) -> i32 {
    let mut rc = SQLITE_OK; // Return code

    debug_assert!(pPager.eState != PAGER_ERROR);
    debug_assert!(pPager.eState >= PAGER_WRITER_LOCKED);

    // Copy the fields of the savepoint (it is not modified here).
    let sp = iSavepoint.map(|i| {
        let s = &pPager.aSavepoint[i];
        (s.nOrig, s.iOffset, s.iHdrOffset, s.iSubRec, s.aWalData)
    });

    // Allocate a bitvec to use to store the set of pages rolled back
    let mut pDone: Option<Box<Bitvec>> = match &sp {
        Some(s) => sqlite3BitvecCreate(s.0),
        None => None,
    };

    // Set the database size back to the value it was before the savepoint
    // being reverted was opened.
    pPager.dbSize = match &sp {
        Some(s) => s.0,
        None => pPager.dbOrigSize,
    };
    pPager.changeCountDone = pPager.tempFile;

    if sp.is_none() && pagerUseWal(pPager) {
        return pagerRollbackWal(pPager);
    }

    // Use pPager->journalOff as the effective size of the main rollback
    // journal.  The actual file might be larger than this in
    // PAGER_JOURNALMODE_TRUNCATE or PAGER_JOURNALMODE_PERSIST.  But
    // anything past pPager->journalOff is off-limits to us.
    let szJ = pPager.journalOff; // Effective size of the main journal
    debug_assert!(!pagerUseWal(pPager) || szJ == 0);

    // Begin by rolling back records from the main journal starting at
    // PagerSavepoint.iOffset and continuing to the next journal header.
    // There might be records in the main journal that have a page number
    // greater than the current database size (pPager->dbSize) but those
    // will be skipped automatically.  Pages are added to pDone as they are
    // played back.
    if let (Some(s), false) = (&sp, pagerUseWal(pPager)) {
        let iHdrOff = if s.2 != 0 { s.2 } else { szJ };
        pPager.journalOff = s.1;
        while rc == SQLITE_OK && pPager.journalOff < iHdrOff {
            let mut off = pPager.journalOff;
            rc = pager_playback_one_page(pPager, &mut off, pDone.as_deref_mut(), 1, 1);
            pPager.journalOff = off;
        }
        debug_assert!(rc != SQLITE_DONE);
    } else {
        pPager.journalOff = 0;
    }

    // Continue rolling back records out of the main journal starting at
    // the first journal header seen and continuing until the effective
    // end of the main journal file.  Continue to skip out-of-range pages
    // and continue adding pages rolled back to pDone.
    while rc == SQLITE_OK && pPager.journalOff < szJ {
        let mut nJRec: u32 = 0; // Number of Journal Records
        let mut dummy: u32 = 0;
        rc = readJournalHdr(pPager, 0, szJ, &mut nJRec, &mut dummy);
        debug_assert!(rc != SQLITE_DONE);

        // The "pPager->journalHdr+JOURNAL_HDR_SZ(pPager)==pPager->journalOff"
        // test is related to ticket #2565.  See the discussion in the
        // pager_playback() function for additional information.
        if nJRec == 0 && pPager.journalHdr + JOURNAL_HDR_SZ(pPager) == pPager.journalOff {
            nJRec = ((szJ - pPager.journalOff) / JOURNAL_PG_SZ(pPager)) as u32;
        }
        let mut ii = 0u32;
        while rc == SQLITE_OK && ii < nJRec && pPager.journalOff < szJ {
            let mut off = pPager.journalOff;
            rc = pager_playback_one_page(pPager, &mut off, pDone.as_deref_mut(), 1, 1);
            pPager.journalOff = off;
            ii += 1;
        }
        debug_assert!(rc != SQLITE_DONE);
    }
    debug_assert!(rc != SQLITE_OK || pPager.journalOff >= szJ);

    // Finally,  rollback pages from the sub-journal.  Page that were
    // previously rolled back out of the main journal (and are hence in
    // pDone) will be skipped.  Out-of-range pages are also skipped.
    if let Some(s) = &sp {
        let mut offset = s.3 as i64 * (4 + pPager.pageSize);
        if let Some(w) = pPager.pWal.as_mut() {
            let mut a = s.4;
            rc = crate::wal::sqlite3WalSavepointUndo(w, &mut a);
        }
        let mut ii = s.3;
        while rc == SQLITE_OK && ii < pPager.nSubRec {
            debug_assert!(offset == ii as i64 * (4 + pPager.pageSize));
            rc = pager_playback_one_page(pPager, &mut offset, pDone.as_deref_mut(), 0, 1);
            ii += 1;
        }
        debug_assert!(rc != SQLITE_DONE);
    }

    drop(pDone);
    if rc == SQLITE_OK {
        pPager.journalOff = szJ;
    }
    rc
}

/// C: `void sqlite3PagerSetCachesize(Pager *pPager, int mxPage)`
///
/// Change the maximum number of in-memory pages that are allowed before
/// attempting to recycle clean and unused pages.
pub fn sqlite3PagerSetCachesize(pPager: &mut crate::pager::Pager, mxPage: i32) {
    sqlite3PcacheSetCachesize(&mut pPager.pPCache, mxPage);
}

/// C: `int sqlite3PagerSetSpillsize(Pager *pPager, int mxPage)`
///
/// Change the maximum number of in-memory pages that are allowed before
/// attempting to spill the cache.
pub fn sqlite3PagerSetSpillsize(pPager: &mut crate::pager::Pager, mxPage: i32) -> i32 {
    sqlite3PcacheSetSpillsize(&mut pPager.pPCache, mxPage)
}

/// Invoke SQLITE_FCNTL_MMAP_SIZE based on the current value of szMmap
/// (no-op: SQLITE_MAX_MMAP_SIZE==0).
fn pagerFixMaplimit(_pPager: &mut Pager) {}

/// C: `void sqlite3PagerSetMmapLimit(Pager *pPager, sqlite3_int64 szMmap)`
///
/// Change the maximum size of any memory mapping made of the database
/// file.
pub fn sqlite3PagerSetMmapLimit(pPager: &mut crate::pager::Pager, szMmap: i64) {
    pPager.szMmap = szMmap;
    pagerFixMaplimit(pPager);
}

/// C: `void sqlite3PagerShrink(Pager *pPager)`
///
/// Free as much memory as possible from the pager.
pub fn sqlite3PagerShrink(pPager: &mut crate::pager::Pager) {
    sqlite3PcacheShrink(&mut pPager.pPCache);
}

/// C: `void sqlite3PagerSetFlags( Pager *pPager, unsigned pgFlags )`
///
/// Adjust settings of the pager to those specified in the pgFlags
/// parameter.
///
/// The "level" in pgFlags & PAGER_SYNCHRONOUS_MASK sets the robustness of
/// the database to damage due to OS crashes or power failures by changing
/// the number of syncs()s when writing the journals.  There are four
/// levels:
///
///    OFF       sqlite3OsSync() is never called.  This is the default for
///              temporary and transient files.
///
///    NORMAL    The journal is synced once before writes begin on the
///              database.  This is normally adequate protection, but it
///              is theoretically possible, though very unlikely, that an
///              inopportune power failure could leave the journal in a
///              state which would cause damage to the database when it is
///              rolled back.
///
///    FULL      The journal is synced twice before writes begin on the
///              database (with some additional information - the nRec
///              field of the journal header - being written in between
///              the two syncs).  If we assume that writing a single disk
///              sector is atomic, then this mode provides assurance that
///              the journal will not be corrupted to the point of causing
///              damage to the database during rollback.
///
///    EXTRA     This is like FULL except that is also syncs the directory
///              that contains the rollback journal after the rollback
///              journal is unlinked.
///
/// The above is for a rollback-journal mode.  For WAL mode, OFF continues
/// to mean that no syncs ever occur.  NORMAL means that the WAL is synced
/// prior to the start of checkpoint and that the database file is synced
/// at the conclusion of the checkpoint if the entire content of the WAL
/// was written back into the database.  But no sync operations occur for
/// an ordinary commit in NORMAL mode with WAL.  FULL means that the WAL
/// file is synced following each commit operation, in addition to the
/// syncs associated with NORMAL.  There is no difference between FULL
/// and EXTRA for WAL mode.
///
/// Do not confuse synchronous=FULL with SQLITE_SYNC_FULL.  The
/// SQLITE_SYNC_FULL flag determines whether or not the F_FULLFSYNC
/// option is used by the sync operations (macOS only).
///
/// The cache_spill=OFF flag (PAGER_CACHESPILL clear) prevents the pager
/// from spilling dirty pages into the journal.
pub fn sqlite3PagerSetFlags(pPager: &mut crate::pager::Pager, pgFlags: u32) {
    let level = pgFlags & PAGER_SYNCHRONOUS_MASK as u32;
    if pPager.tempFile != 0 || level == PAGER_SYNCHRONOUS_OFF as u32 {
        pPager.noSync = 1;
        pPager.fullSync = 0;
        pPager.extraSync = 0;
    } else {
        pPager.noSync = 0;
        pPager.fullSync = (level >= PAGER_SYNCHRONOUS_FULL as u32) as u8;
        if level == PAGER_SYNCHRONOUS_EXTRA as u32 {
            pPager.extraSync = 1;
        } else {
            pPager.extraSync = 0;
        }
    }
    if pPager.noSync != 0 {
        pPager.syncFlags = 0;
    } else if (pgFlags & PAGER_FULLFSYNC as u32) != 0 {
        pPager.syncFlags = SQLITE_SYNC_FULL as u8;
    } else {
        pPager.syncFlags = SQLITE_SYNC_NORMAL as u8;
    }
    pPager.walSyncFlags = pPager.syncFlags << 2;
    if pPager.fullSync != 0 {
        pPager.walSyncFlags |= pPager.syncFlags;
    }
    if (pgFlags & PAGER_CKPT_FULLFSYNC as u32) != 0 && pPager.noSync == 0 {
        pPager.walSyncFlags |= (SQLITE_SYNC_FULL << 2) as u8;
    }
    if (pgFlags & PAGER_CACHESPILL as u32) != 0 {
        pPager.doNotSpill &= !SPILLFLAG_OFF;
    } else {
        pPager.doNotSpill |= SPILLFLAG_OFF;
    }
}

/// Open a temporary file.
///
/// Write the file descriptor into *pFile. Return SQLITE_OK on success or
/// some other error code if we fail. The OS will automatically delete the
/// temporary file when it is closed.
///
/// The flags passed to the VFS layer xOpen() call are those specified by
/// parameter vfsFlags ORed with the following:
///
///     SQLITE_OPEN_READWRITE
///     SQLITE_OPEN_CREATE
///     SQLITE_OPEN_EXCLUSIVE
///     SQLITE_OPEN_DELETEONCLOSE
fn pagerOpentemp(pVfs: &Vfs, pFile: &mut SqliteFile, vfsFlags: i32) -> i32 {
    let vfsFlags = vfsFlags
        | SQLITE_OPEN_READWRITE
        | SQLITE_OPEN_CREATE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_DELETEONCLOSE;
    let rc = sqlite3OsOpen(pVfs, None, pFile, vfsFlags, None);
    debug_assert!(rc != SQLITE_OK || isOpen(pFile));
    rc
}

/// C: `void sqlite3PagerSetBusyHandler( Pager *pPager, int (*xBusyHandler)(void *), void *pBusyHandlerArg )`
///
/// Set the busy handler function.
///
/// The pager invokes the busy-handler if sqlite3OsLock() returns
/// SQLITE_BUSY when trying to upgrade from no-lock to a SHARED lock, or
/// when trying to upgrade from a RESERVED lock to an EXCLUSIVE lock. It
/// does not invoke the busy handler when upgrading from SHARED to
/// RESERVED, or when upgrading from SHARED to EXCLUSIVE (which occurs
/// during hot-journal rollback). Summary:
///
///   Transition                        | Invokes xBusyHandler
///   --------------------------------------------------------
///   NO_LOCK       -> SHARED_LOCK      | Yes
///   SHARED_LOCK   -> RESERVED_LOCK    | No
///   SHARED_LOCK   -> EXCLUSIVE_LOCK   | No
///   RESERVED_LOCK -> EXCLUSIVE_LOCK   | Yes
///
/// If the busy-handler callback returns non-zero, the lock is retried.
/// If it returns zero, then the SQLITE_BUSY error is returned to the
/// caller of the pager API function.  (The SQLITE_FCNTL_BUSYHANDLER file
/// control is not forwarded: the unix VFS ignores it.)
pub fn sqlite3PagerSetBusyHandler(
    pPager: &mut crate::pager::Pager,
    xBusyHandler: Option<Rc<dyn Fn() -> i32>>,
) {
    pPager.xBusyHandler = xBusyHandler;
}

/// C: `int sqlite3PagerSetPagesize(Pager *pPager, u32 *pPageSize, int nReserve)`
///
/// Change the page size used by the Pager object. The new page size is
/// passed in *pPageSize.
///
/// If the pager is in the error state when this function is called, it
/// is a no-op. The value returned is the error state error code (i.e.
/// one of SQLITE_IOERR, an SQLITE_IOERR_xxx sub-code or SQLITE_FULL).
///
/// Otherwise, if all of the following are true:
///
///   * the new page size (value of *pPageSize) is valid (a power of two
///     between 512 and SQLITE_MAX_PAGE_SIZE, inclusive), and
///
///   * there are no outstanding page references, and
///
///   * the database is either not an in-memory database or it is an
///     in-memory database that currently consists of zero pages.
///
/// then the pager object page size is set to *pPageSize.
///
/// If the page size is changed, then this function uses
/// sqlite3PagerMalloc() to obtain a new Pager.pTmpSpace buffer. If this
/// allocation fails, SQLITE_NOMEM is returned and the page size remains
/// unchanged. SQLITE_NOMEM may also be returned if an error occurs after
/// the page size has been changed (or not).
///
/// In all other cases, SQLITE_OK is returned.
///
/// If the page size is not changed, either because one of the
/// enumerated conditions above is not true, the pager was in error
/// state when this function was called, or because the memory
/// allocation attempt failed, then *pPageSize is set to the old, retained
/// page size before returning.
pub fn sqlite3PagerSetPagesize(
    pPager: &mut crate::pager::Pager,
    pPageSize: &mut u32,
    nReserve: i32,
) -> i32 {
    let mut rc = SQLITE_OK;

    // It is not possible to do a full assert_pager_state() here, as this
    // function may be called from within PagerOpen(), before the state of
    // the Pager object is internally consistent.
    //
    // At one point this function returned an error if the pager was in
    // PAGER_ERROR state. But since PAGER_ERROR state guarantees that there
    // is no outstanding page references, there is no point in returning an
    // error in that case.
    let pageSize = *pPageSize;
    debug_assert!(pageSize == 0 || (pageSize >= 512 && pageSize <= SQLITE_MAX_PAGE_SIZE as u32));
    if (pPager.memDb == 0 || pPager.dbSize == 0)
        && sqlite3PcacheRefCount(&mut pPager.pPCache) == 0
        && pageSize != 0
        && pageSize != pPager.pageSize as u32
    {
        let mut nByte: i64 = 0;

        if pPager.eState > PAGER_OPEN && isOpen(&pPager.fd) {
            rc = sqlite3OsFileSize(&mut pPager.fd, &mut nByte);
        }
        if rc == SQLITE_OK {
            // 8 bytes of zeroed overrun space is sufficient so that the
            // b-tree cell header parser will never run off the end of the
            // allocation
            pager_reset(pPager);
            rc = sqlite3PcacheSetPageSize(&mut pPager.pPCache, pageSize as i32);
        }
        if rc == SQLITE_OK {
            pPager.pTmpSpace = vec![0u8; pageSize as usize + 8];
            pPager.dbSize = ((nByte + pageSize as i64 - 1) / pageSize as i64) as Pgno;
            pPager.pageSize = pageSize as i64;
            pPager.lckPgno = (PENDING_BYTE() / pageSize as i64) as Pgno + 1;
        }
    }

    *pPageSize = pPager.pageSize as u32;
    if rc == SQLITE_OK {
        let nReserve = if nReserve < 0 {
            pPager.nReserve as i32
        } else {
            nReserve
        };
        debug_assert!(nReserve >= 0 && nReserve < 1000);
        pPager.nReserve = nReserve as i16;
        pagerFixMaplimit(pPager);
    }
    rc
}

/// C: `void * sqlite3PagerTempSpace(Pager *pPager)`
///
/// Return a pointer to the "temporary page" buffer held internally by the
/// pager.  This is a buffer that is big enough to hold the entire content
/// of a database page.  This buffer is used internally during rollback
/// and will be overwritten whenever a rollback occurs.  But other modules
/// are free to use it too, as long as no rollbacks are happening.
///
/// Rust: returns a copy of the buffer (pageSize+8 bytes); its content is
/// scratch.
pub fn sqlite3PagerTempSpace(pPager: &mut crate::pager::Pager) -> Option<Vec<u8>> {
    Some(pPager.pTmpSpace.clone())
}

/// C: `Pgno sqlite3PagerMaxPageCount(Pager *pPager, Pgno mxPage)`
///
/// Attempt to set the maximum database page count if mxPage is positive.
/// Make no changes if mxPage is zero or negative.  And never reduce the
/// maximum page count below the current size of the database.
///
/// Regardless of mxPage, return the current maximum page count.
pub fn sqlite3PagerMaxPageCount(pPager: &mut crate::pager::Pager, mxPage: Pgno) -> Pgno {
    if mxPage > 0 {
        pPager.mxPgno = mxPage;
    }
    debug_assert!(pPager.eState != PAGER_OPEN); // Called only by OP_MaxPgcnt
    // assert( pPager->mxPgno>=pPager->dbSize ); Called only by
    // OP_MaxPgcnt, which guarantees this
    pPager.mxPgno
}

/// C: `int sqlite3PagerReadFileheader(Pager *pPager, int N, unsigned char *pDest)`
///
/// Read the first N bytes from the beginning of the file into memory that
/// pDest points to.
///
/// If the pager was opened on a transient file (zFilename==""), or opened
/// on a file less than N bytes in size, the output buffer is zeroed and
/// SQLITE_OK returned. The rationale for this is that this function is
/// used to read database headers, and a new transient or zero sized
/// database has a header than consists entirely of zeroes.
///
/// If any IO error apart from SQLITE_IOERR_SHORT_READ is encountered, the
/// error code is returned to the caller and the contents of the output
/// buffer undefined.
pub fn sqlite3PagerReadFileheader(
    pPager: &mut crate::pager::Pager,
    N: i32,
    pDest: &mut [u8],
) -> i32 {
    let mut rc = SQLITE_OK;
    for b in pDest[..N as usize].iter_mut() {
        *b = 0;
    }
    debug_assert!(isOpen(&pPager.fd) || pPager.tempFile != 0);

    // This routine is only called by btree immediately after creating the
    // Pager object.  There has not been an opportunity to transition to
    // WAL mode yet.
    debug_assert!(!pagerUseWal(pPager));

    if isOpen(&pPager.fd) {
        rc = sqlite3OsRead(&mut pPager.fd, pDest, N, 0);
        if rc == SQLITE_IOERR_SHORT_READ {
            rc = SQLITE_OK;
        }
    }
    rc
}

/// C: `void sqlite3PagerPagecount(Pager *pPager, int *pnPage)`
///
/// This function may only be called when a read-transaction is open on
/// the pager. It returns the total number of pages in the database.
///
/// However, if the file is between 1 and <page-size> bytes in size, then
/// this is considered a 1 page file.
pub fn sqlite3PagerPagecount(pPager: &mut crate::pager::Pager, pnPage: &mut i32) {
    debug_assert!(pPager.eState >= PAGER_READER);
    debug_assert!(pPager.eState != PAGER_WRITER_FINISHED);
    *pnPage = pPager.dbSize as i32;
}

/// Try to obtain a lock of type locktype on the database file. If a lock
/// of type locktype cannot be obtained, it is because some other process
/// holds a lock that prevents it, and the busy-handler is invoked.  If
/// the busy-handler returns non-zero, another attempt to obtain the lock
/// is made, and so on until either the lock is obtained or the
/// busy-handler returns 0.
///
/// The function returns SQLITE_OK if the lock is obtained, SQLITE_BUSY if
/// the busy-handler gives up, or an error code if an IO error occurs.
fn pager_wait_on_lock(pPager: &mut Pager, locktype: i32) -> i32 {
    // Check that this is either a no-op (because the requested lock is
    // already held), or one of the transitions that the busy-handler may
    // be invoked during, according to the comment above
    // sqlite3PagerSetBusyhandler().
    debug_assert!(
        (pPager.eLock as i32 >= locktype)
            || (pPager.eLock as i32 == NO_LOCK && locktype == SHARED_LOCK)
            || (pPager.eLock as i32 == RESERVED_LOCK && locktype == EXCLUSIVE_LOCK)
    );

    loop {
        let rc = pagerLockDb(pPager, locktype);
        if rc == SQLITE_BUSY {
            if let Some(h) = pPager.xBusyHandler.clone() {
                if h() != 0 {
                    continue;
                }
            }
        }
        return rc;
    }
}

/// C: `void sqlite3PagerTruncateImage(Pager *pPager, Pgno nPage)`
///
/// Truncate the in-memory database file image to nPage pages. This
/// function does not actually modify the database file on disk. It just
/// sets the internal state of the pager object so that the truncation
/// will be done when the current transaction is committed.
///
/// This function is only called right before committing a transaction.
/// Once this function has been called, the transaction must either be
/// rolled back or committed. It is not safe to call this function and
/// then continue writing to the database.
pub fn sqlite3PagerTruncateImage(pPager: &mut crate::pager::Pager, nPage: Pgno) {
    debug_assert!(pPager.eState >= PAGER_WRITER_CACHEMOD);
    pPager.dbSize = nPage;

    // At one point the code here called assertTruncateConstraint() to
    // ensure that all pages being truncated away by this operation are,
    // if one or more savepoints are open, present in the savepoint
    // journal so that they can be restored if the savepoint is rolled
    // back. This is no longer necessary as this function is now only
    // called right before committing a transaction. So although the
    // Pager object may still have open savepoints (Pager.nSavepoint!=0),
    // they cannot be rolled back. So the assertTruncateConstraint() call
    // is no longer correct.
}

/// This function is called before attempting a hot-journal rollback. It
/// syncs the journal file to disk, then sets pPager->journalHdr to the
/// size of the journal file so that the pager_playback() routine knows
/// that the entire journal file has been synced.
///
/// Syncing a hot-journal to disk before attempting to roll it back
/// ensures that if a power-failure occurs during the rollback, the
/// process that attempts rollback following system recovery sees the
/// same journal content as this process.
///
/// If everything goes as planned, SQLITE_OK is returned. Otherwise, an
/// SQLite error code.
fn pagerSyncHotJournal(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    if pPager.noSync == 0 {
        rc = sqlite3OsSync(&mut pPager.jfd, SQLITE_SYNC_NORMAL);
    }
    if rc == SQLITE_OK {
        rc = sqlite3OsFileSize(&mut pPager.jfd, &mut pPager.journalHdr);
    }
    rc
}

/// Check that the database file has not been renamed or deleted since it
/// was opened (the SQLITE_FCNTL_HAS_MOVED file control).
fn databaseIsUnmoved(pPager: &mut Pager) -> i32 {
    let mut bHasMoved = 0;
    if pPager.tempFile != 0 {
        return SQLITE_OK;
    }
    if pPager.dbSize == 0 {
        return SQLITE_OK;
    }
    debug_assert!(!pPager.zFilename.is_empty());
    let mut rc = sqlite3OsFileControl(
        &mut pPager.fd,
        SQLITE_FCNTL_HAS_MOVED,
        FcntlArg::Int(&mut bHasMoved),
    );
    if rc == SQLITE_NOTFOUND {
        // If the HAS_MOVED file-control is unimplemented, assume that the
        // file has not been moved.  That is the historical behavior of
        // SQLite: prior to version 3.8.3, it never checked if the file had
        // been moved, as there was no file-control to check.
        rc = SQLITE_OK;
    } else if rc == SQLITE_OK && bHasMoved != 0 {
        rc = SQLITE_READONLY_DBMOVED;
    }
    rc
}

/// C: `int sqlite3PagerClose(Pager *pPager, sqlite3 *db)`
///
/// Shutdown the page cache.  Free all memory and close all files.
///
/// If a transaction was in progress when this routine is called, that
/// transaction is rolled back.  All outstanding pages are invalidated and
/// their memory is freed.  Any attempt to use a page associated with
/// this page cache after this function returns will likely result in a
/// coredump.
///
/// This function always succeeds. If a transaction is active an attempt
/// is made to roll it back. If an error occurs during the rollback a hot
/// journal may be left in the filesystem but no error is returned to the
/// caller.
///
/// Signature change: `db: Option<&mut Sqlite3>` (C allows NULL when the
/// pager is not in WAL mode).
pub fn sqlite3PagerClose(db: Option<&mut Sqlite3>, pPager: &mut crate::pager::Pager) -> i32 {
    debug_assert!(assert_pager_state(pPager));
    pPager.exclusiveMode = 0;
    if let Some(mut w) = pPager.pWal.take() {
        let mut a: Vec<u8> = Vec::new();
        if let Some(db) = db {
            let bCkpt =
                (db.flags & SQLITE_NoCkptOnClose) == 0 && SQLITE_OK == databaseIsUnmoved(pPager);
            if bCkpt {
                a = pPager.pTmpSpace.clone();
            }
            let walSyncFlags = pPager.walSyncFlags as i32;
            let pageSize = pPager.pageSize as i32;
            crate::wal::sqlite3WalClose(db, &mut w, walSyncFlags, pageSize, &mut a);
        }
    }
    pager_reset(pPager);
    if MEMDB(pPager) {
        pager_unlock(pPager);
    } else {
        // If it is open, sync the journal file before calling
        // UnlockAndRollback.  If this is not done, then an unsynced
        // portion of the open journal file may be played back into the
        // database. If a power failure occurs while this is happening,
        // the database could become corrupt.
        //
        // If an error occurs while trying to sync the journal, shift the
        // pager into the error state. This causes UnlockAndRollback to
        // unlock the database file without first attempting to rollback
        // the journal.
        if isOpen(&pPager.jfd) {
            let rc = pagerSyncHotJournal(pPager);
            pager_error(pPager, rc);
        }
        pagerUnlockAndRollback(pPager);
    }
    sqlite3OsClose(&mut pPager.jfd);
    sqlite3OsClose(&mut pPager.fd);
    pPager.pTmpSpace = Vec::new();
    sqlite3PcacheClose(&mut pPager.pPCache);
    debug_assert!(pPager.aSavepoint.is_empty() && pPager.pInJournal.is_none());
    debug_assert!(!isOpen(&pPager.jfd) && !isOpen(&pPager.sjfd));
    SQLITE_OK
}

/// C: `Pgno sqlite3PagerPagenumber(DbPage *pPg)`
///
/// Return the page number for page pPg.
pub fn sqlite3PagerPagenumber(pPg: &DbPage) -> Pgno {
    pPg.borrow().pgno
}

/// C: `void sqlite3PagerRef(DbPage *pPg)`
///
/// Increment the reference count for page pPg.
pub fn sqlite3PagerRef(pPg: &DbPage) {
    match page_pager(pPg) {
        Some(p) => sqlite3PcacheRef(&mut p.borrow_mut().pPCache, pPg),
        None => pPg.borrow_mut().nRef += 1,
    }
}

/// sqlite3PagerRef() for callers that already hold the pager.
pub fn sqlite3PagerRefWith(pPager: &mut Pager, pPg: &DbPage) {
    sqlite3PcacheRef(&mut pPager.pPCache, pPg);
}

/// Sync the journal. In other words, make sure all the pages that have
/// been written to the journal have actually reached the surface of the
/// disk and can be restored in the event of a hot-journal rollback.
///
/// If the Pager.noSync flag is set, then this function is a no-op.
/// Otherwise, the actions required depend on the journal-mode and the
/// device characteristics of the file-system, as follows:
///
///  * If the journal file is an in-memory journal file, no action need
///    be taken.
///
///  * Otherwise, if the device does not support the SAFE_APPEND property,
///    then the nRec field of the most recently written journal header
///    is updated to contain the number of journal records that have been
///    written following it. If the pager is operating in full-sync mode,
///    then the journal file is synced before this field is updated.
///
///  * If the device does not support the SEQUENTIAL property, then
///    journal file is synced.
///
/// Or, in pseudo-code:
///
///   if( NOT <in-memory journal> ){
///     if( NOT SAFE_APPEND ){
///       if( <full-sync mode> ) xSync(<journal file>);
///       <update nRec field>
///     }
///     if( NOT SEQUENTIAL ) xSync(<journal file>);
///   }
///
/// If successful, this routine clears the PGHDR_NEED_SYNC flag of every
/// page currently held in memory before returning SQLITE_OK. If an IO
/// error is encountered, then the IO error code is returned to the
/// caller.
fn syncJournal(pPager: &mut Pager, newHdr: i32) -> i32 {
    let mut rc;

    debug_assert!(pPager.eState == PAGER_WRITER_CACHEMOD || pPager.eState == PAGER_WRITER_DBMOD);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(!pagerUseWal(pPager));

    rc = sqlite3PagerExclusiveLock(pPager);
    if rc != SQLITE_OK {
        return rc;
    }

    if pPager.noSync == 0 {
        debug_assert!(pPager.tempFile == 0);
        if isOpen(&pPager.jfd) && pPager.journalMode as i32 != PAGER_JOURNALMODE_MEMORY {
            let iDc = sqlite3OsDeviceCharacteristics(&mut pPager.fd);
            debug_assert!(isOpen(&pPager.jfd));

            if 0 == (iDc & SQLITE_IOCAP_SAFE_APPEND) {
                // This block deals with an obscure problem. If the last
                // connection that wrote to this database was operating in
                // persistent-journal mode, then the journal file may at
                // this point actually be larger than Pager.journalOff
                // bytes. If the next thing in the journal file happens to
                // be a journal-header (written as part of the previous
                // connection's transaction), and a crash or power-failure
                // occurs after nRec is updated but before this connection
                // writes anything else to the journal file (or commits
                // the transaction), then the next time the journal is
                // played back the transaction will be rolled back even
                // though it was committed.  To work around this, if the
                // journal file does appear to contain a valid header
                // following Pager.journalOff, then write a 0x00 byte to
                // the start of it to prevent it from being recognized.
                //
                // Variable iNextHdrOffset is set to the offset at which
                // this problematic header will occur, if it exists.
                // aMagic is used to hold the 8-byte header magic number.
                let mut aMagic = [0u8; 8];
                let mut zHeader = [0u8; 12];
                zHeader[..8].copy_from_slice(&aJournalMagic);
                put32bits(&mut zHeader[8..], pPager.nRec as u32);

                let iNextHdrOffset = journalHdrOffset(pPager);
                rc = sqlite3OsRead(&mut pPager.jfd, &mut aMagic, 8, iNextHdrOffset);
                if rc == SQLITE_OK && aMagic == aJournalMagic {
                    let zerobyte = [0u8; 1];
                    rc = sqlite3OsWrite(&mut pPager.jfd, &zerobyte, 1, iNextHdrOffset);
                }
                if rc != SQLITE_OK && rc != SQLITE_IOERR_SHORT_READ {
                    return rc;
                }

                // Write the nRec value into the journal file header. If in
                // full-synchronous mode, sync the journal first. This
                // ensures that all data has really hit the disk before nRec
                // is updated to mark it as a candidate for rollback.
                //
                // This is not required if the persistent media supports
                // the SAFE_APPEND property. Because in this case it is not
                // possible for garbage data to be appended to the file,
                // the nRec field is populated with 0xFFFFFFFF when the
                // journal header is written and never needs to be updated.
                if pPager.fullSync != 0 && 0 == (iDc & SQLITE_IOCAP_SEQUENTIAL) {
                    rc = sqlite3OsSync(&mut pPager.jfd, pPager.syncFlags as i32);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                }
                rc = sqlite3OsWrite(&mut pPager.jfd, &zHeader, 12, pPager.journalHdr);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
            if 0 == (iDc & SQLITE_IOCAP_SEQUENTIAL) {
                let f = pPager.syncFlags as i32
                    | if pPager.syncFlags as i32 == SQLITE_SYNC_FULL {
                        SQLITE_SYNC_DATAONLY
                    } else {
                        0
                    };
                rc = sqlite3OsSync(&mut pPager.jfd, f);
                if rc != SQLITE_OK {
                    return rc;
                }
            }

            pPager.journalHdr = pPager.journalOff;
            if newHdr != 0 && 0 == (iDc & SQLITE_IOCAP_SAFE_APPEND) {
                pPager.nRec = 0;
                rc = writeJournalHdr(pPager);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        } else {
            pPager.journalHdr = pPager.journalOff;
        }
    }

    // Unless the pager is in noSync mode, the journal file was just
    // successfully synced. Either way, clear the PGHDR_NEED_SYNC flag on
    // all pages.
    sqlite3PcacheClearSyncFlags(&mut pPager.pPCache);
    pPager.eState = PAGER_WRITER_DBMOD;
    debug_assert!(assert_pager_state(pPager));
    SQLITE_OK
}

/// The argument is the first in a linked list of dirty pages connected by
/// the PgHdr.pDirty pointer. This function writes each one of the
/// in-memory pages in the list to the database file. The argument may be
/// NULL, representing an empty list. In this case this function is a
/// no-op.
///
/// The pager must hold at least a RESERVED lock when this function is
/// called. Before writing anything to the database file, this lock is
/// upgraded to an EXCLUSIVE lock. If the lock cannot be obtained,
/// SQLITE_BUSY is returned and no data is written to the database file.
///
/// If the pager is a temp-file pager and the actual file-system file is
/// not yet open, it is created and opened before any data is written to
/// it.
///
/// Once the lock has been upgraded and, if necessary, the file opened,
/// the pages are written out to the database file in list order. Writing
/// a page is skipped if it meets either of the following criteria:
///
///   * The page number is greater than Pager.dbSize, or
///   * The PGHDR_DONT_WRITE flag is set on the page.
///
/// If writing out a page causes the database file to grow,
/// Pager.dbFileSize is updated accordingly. If page 1 is written out,
/// then the value cached in Pager.dbFileVers[] is updated to match the
/// new value stored in the database file.
///
/// If everything is successful, SQLITE_OK is returned. If an IO error
/// occurs, an IO error code is returned. Or, if the EXCLUSIVE lock cannot
/// be obtained, SQLITE_BUSY is returned.
fn pager_write_pagelist(pPager: &mut Pager, pList: &[DbPage]) -> i32 {
    let mut rc = SQLITE_OK; // Return code

    // This function is only called for rollback pagers in WRITER_DBMOD
    // state.
    debug_assert!(!pagerUseWal(pPager));
    debug_assert!(pPager.tempFile != 0 || pPager.eState == PAGER_WRITER_DBMOD);
    debug_assert!(pPager.eLock as i32 == EXCLUSIVE_LOCK);
    debug_assert!(isOpen(&pPager.fd) || pList.len() <= 1);

    // If the file is a temp-file has not yet been opened, open it now. It
    // is not possible for rc to be other than SQLITE_OK if this branch is
    // taken, as pager_wait_on_lock() is a no-op for temp-files.
    if !isOpen(&pPager.fd) {
        debug_assert!(pPager.tempFile != 0 && rc == SQLITE_OK);
        let vfs = pPager.pVfs.clone();
        rc = pagerOpentemp(&vfs, &mut pPager.fd, pPager.vfsFlags as i32);
    }

    // Before the first write, give the VFS a hint of what the final file
    // size will be.
    debug_assert!(rc != SQLITE_OK || isOpen(&pPager.fd));
    if rc == SQLITE_OK
        && pPager.dbHintSize < pPager.dbSize
        && !pList.is_empty()
        && (pList.len() > 1 || pgpgno(&pList[0]) > pPager.dbHintSize)
    {
        let mut szFile = pPager.pageSize * pPager.dbSize as i64;
        sqlite3OsFileControlHint(
            &mut pPager.fd,
            SQLITE_FCNTL_SIZE_HINT,
            FcntlArg::I64(&mut szFile),
        );
        pPager.dbHintSize = pPager.dbSize;
    }

    for p in pList {
        if rc != SQLITE_OK {
            break;
        }
        let (pgno, flags) = {
            let b = p.borrow();
            (b.pgno, b.flags as i32)
        };

        // If there are dirty pages in the page cache with page numbers
        // greater than Pager.dbSize, this means sqlite3PagerTruncateImage()
        // was called to make the file smaller (presumably by auto-vacuum
        // code). Do not write any data for such pages to the file.
        if pgno <= pPager.dbSize && 0 == (flags & PGHDR_DONT_WRITE) {
            let offset = (pgno as i64 - 1) * pPager.pageSize; // Offset to write

            debug_assert!((flags & PGHDR_NEED_SYNC) == 0);
            if pgno == 1 {
                let v = pPager.dbFileVers;
                pager_write_changecounter(&v, p);
            }

            // Write out the page data.
            let pageSize = pPager.pageSize as i32;
            {
                let b = p.borrow();
                rc = sqlite3OsWrite(&mut pPager.fd, &b.aData, pageSize, offset);
            }

            // If page 1 was just written, update Pager.dbFileVers to match
            // the value now stored in the database file. If writing this
            // page caused the database file to grow, update dbFileSize.
            if pgno == 1 {
                let b = p.borrow();
                pPager.dbFileVers.copy_from_slice(&b.aData[24..40]);
            }
            if pgno > pPager.dbFileSize {
                pPager.dbFileSize = pgno;
            }
            pPager.aStat[PAGER_STAT_WRITE] = pPager.aStat[PAGER_STAT_WRITE].wrapping_add(1);

            // Update any backup objects copying the contents of this pager.
            if !pPager.pBackup.is_empty() {
                let data = p.borrow().aData.clone();
                backup_update(pPager, pgno, &data);
            }
        }
    }
    rc
}

/// Ensure that the sub-journal file is open. If it is already open, this
/// function is a no-op.
///
/// SQLITE_OK is returned if everything goes according to plan. An
/// SQLITE_IOERR_XXX error code is returned if a call to sqlite3OsOpen()
/// fails.
fn openSubJournal(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;
    if !isOpen(&pPager.sjfd) {
        let flags = SQLITE_OPEN_SUBJOURNAL
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE;
        let mut nStmtSpill = crate::global::sqlite3Config.nStmtSpill;
        if pPager.journalMode as i32 == PAGER_JOURNALMODE_MEMORY || pPager.subjInMemory != 0 {
            nStmtSpill = -1;
        }
        let vfs = pPager.pVfs.clone();
        rc = sqlite3JournalOpen(Some(&vfs), None, &mut pPager.sjfd, flags, nStmtSpill);
    }
    rc
}

/// Append a record of the current state of page pPg to the sub-journal.
///
/// If successful, set the bit corresponding to pPg->pgno in the bitvecs
/// for all open savepoints before returning.
///
/// This function returns SQLITE_OK if everything is successful, an IO
/// error code if the attempt to write to the sub-journal fails, or
/// SQLITE_NOMEM if a malloc fails while setting a bit in a savepoint
/// bitvec.
fn subjournalPage(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc = SQLITE_OK;
    let pgno = pgpgno(pPg);

    // Open the sub-journal, if it has not already been opened
    debug_assert!(pPager.useJournal != 0);
    debug_assert!(
        isOpen(&pPager.jfd)
            || pagerUseWal(pPager)
            || pPager.journalMode as i32 == PAGER_JOURNALMODE_OFF
    );
    debug_assert!(isOpen(&pPager.sjfd) || pPager.nSubRec == 0);
    if pPager.journalMode as i32 != PAGER_JOURNALMODE_OFF {
        rc = openSubJournal(pPager);

        // If the sub-journal was opened successfully (or was already open),
        // write the journal record into the file.
        if rc == SQLITE_OK {
            let offset = pPager.nSubRec as i64 * (4 + pPager.pageSize);
            rc = write32bits(&mut pPager.sjfd, offset, pgno);
            if rc == SQLITE_OK {
                let pageSize = pPager.pageSize as i32;
                let b = pPg.borrow();
                rc = sqlite3OsWrite(&mut pPager.sjfd, &b.aData, pageSize, offset + 4);
            }
        }
    }
    if rc == SQLITE_OK {
        pPager.nSubRec += 1;
        debug_assert!(pPager.nSavepoint > 0);
        rc = addToSavepointBitvecs(pPager, pgno);
    }
    rc
}

fn subjournalPageIfRequired(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let pgno = pgpgno(pPg);
    if subjRequiresPage(pPager, pgno) {
        subjournalPage(pPager, pPg)
    } else {
        SQLITE_OK
    }
}

/// This function is called by the pcache layer when it has reached some
/// soft memory limit. The first argument is a pointer to a Pager object
/// (cast as a void*). The pager is always 'purgeable' (not an in-memory
/// database). The second argument is a reference to a page that is
/// currently dirty but has no outstanding references. The page is always
/// associated with the Pager object passed as the first argument.
///
/// The job of this function is to make pPg clean by writing its contents
/// out to the database file, if possible. This may involve syncing the
/// journal file.
///
/// If successful, sqlite3PcacheMakeClean() is called on the page and
/// SQLITE_OK returned. If an IO error occurs while trying to make the
/// page clean, the IO error code is returned. If the page cannot be made
/// clean for some other reason, but no error occurs, then SQLITE_OK is
/// returned and sqlite3PcacheMakeClean() is not called.
pub(crate) fn pagerStress(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!((pgflags(pPg) & PGHDR_DIRTY) != 0);

    // The doNotSpill NOSYNC bit is set during times when doing a sync of
    // journal (and adding a new header) is not allowed.  This occurs
    // during calls to sqlite3PagerWrite() while trying to journal multiple
    // pages belonging to the same sector.
    //
    // The doNotSpill ROLLBACK and OFF bits inhibits all cache spilling
    // regardless of whether or not a sync is required.  This is set
    // during a rollback or by user request, respectively.
    //
    // Spilling is also prohibited when in an error state since that could
    // lead to database corruption.   In the current implementation it is
    // impossible for sqlite3PcacheFetch() to be called with createFlag==3
    // while in the error state, hence it is impossible for this routine
    // to be called in the error state.  Nevertheless, we include a
    // NEVER() test for the error state as a safeguard against future
    // changes.
    if pPager.errCode != 0 {
        return SQLITE_OK;
    }
    if pPager.doNotSpill != 0
        && ((pPager.doNotSpill & (SPILLFLAG_ROLLBACK | SPILLFLAG_OFF)) != 0
            || (pgflags(pPg) & PGHDR_NEED_SYNC) != 0)
    {
        return SQLITE_OK;
    }

    pPager.aStat[PAGER_STAT_SPILL] = pPager.aStat[PAGER_STAT_SPILL].wrapping_add(1);
    if pagerUseWal(pPager) {
        // Write a single frame for this page to the log.
        rc = subjournalPageIfRequired(pPager, pPg);
        if rc == SQLITE_OK {
            rc = pagerWalFrames(pPager, std::slice::from_ref(pPg), 0, 0);
        }
    } else {
        // Sync the journal file if required.
        if (pgflags(pPg) & PGHDR_NEED_SYNC) != 0 || pPager.eState == PAGER_WRITER_CACHEMOD {
            rc = syncJournal(pPager, 1);
        }

        // Write the contents of the page out to the database file.
        if rc == SQLITE_OK {
            debug_assert!((pgflags(pPg) & PGHDR_NEED_SYNC) == 0);
            rc = pager_write_pagelist(pPager, std::slice::from_ref(pPg));
        }
    }

    // Mark the page as clean.
    if rc == SQLITE_OK {
        sqlite3PcacheMakeClean(&mut pPager.pPCache, pPg);
    }

    pager_error(pPager, rc)
}

/// C: `int sqlite3PagerFlush(Pager *pPager)`
///
/// Flush all unreferenced dirty pages to disk.
pub fn sqlite3PagerFlush(pPager: &mut crate::pager::Pager) -> i32 {
    let mut rc = pPager.errCode;
    if !MEMDB(pPager) {
        let pList = sqlite3PcacheDirtyList(&mut pPager.pPCache);
        debug_assert!(assert_pager_state(pPager));
        for p in pList {
            if rc != SQLITE_OK {
                break;
            }
            if p.borrow().nRef == 0 {
                rc = pagerStress(pPager, &p);
            }
        }
    }
    rc
}

/// C: `int sqlite3PagerOpen( sqlite3_vfs *pVfs, Pager **ppPager, const char *zFilename, int nExtra, int flags, int vfsFlags, void (*xReinit)(DbPage*) )`
///
/// Allocate and initialize a new Pager object and put a pointer to it in
/// *ppPager. The pager should eventually be freed by passing it to
/// sqlite3PagerClose().
///
/// The zFilename argument is the path to the database file to open.  If
/// zFilename is NULL or "" then a randomly-named temporary file is
/// created and used as the file to be cached. Temporary files are be
/// deleted automatically when they are closed. If zFilename is ":memory:"
/// then all information is held in cache. It is never written to disk.
/// This can be used to implement an in-memory database.  (The URI
/// parameter block, if any, follows the name after a NUL byte.)
///
/// The nExtra parameter specifies the number of bytes of space allocated
/// along with each page reference. This space is available to the user
/// via the sqlite3PagerGetExtra() API.  When a new page is allocated, the
/// first 8 bytes of this space are zeroed but the remainder is
/// uninitialized.  (The extra space is used by btree as the MemPage
/// object; here the value only matters for cache-size computations: pass
/// the C sizeof(MemPage), 136, for identical behaviour.)
///
/// The flags argument is used to specify properties that affect the
/// operation of the pager. It should be passed some bitwise combination
/// of the PAGER_* flags.
///
/// The vfsFlags parameter is a bitmask to pass to the flags parameter of
/// the xOpen() method of the supplied VFS when opening files.
///
/// If the pager object is allocated and the specified file opened
/// successfully, SQLITE_OK is returned and *ppPager set to point to the
/// new pager object. If an error occurs, *ppPager is set to NULL and an
/// error code returned. This function may return SQLITE_NOMEM
/// (sqlite3Malloc() is used to allocate memory), SQLITE_CANTOPEN or one
/// of the various IO error codes.
pub fn sqlite3PagerOpen(
    pVfs: &Rc<crate::os::Vfs>,
    ppPager: &mut Option<PagerP>,
    zFilename: Option<&[u8]>,
    nExtra: i32,
    flags: i32,
    vfsFlags: i32,
    xReinit: Option<fn(&DbPage)>,
) -> i32 {
    let mut rc = SQLITE_OK; // Return code
    let mut tempFile = 0; // True for temp files (incl. in-memory files)
    let mut memDb = 0; // True if this is an in-memory file
    let mut memJM = 0; // Memory journal mode
    let mut readOnly = 0; // True if this is a read-only file
    let mut zPathname: ZStr = ZStr::new(); // Full path to database file
    let useJournal = (flags & PAGER_OMIT_JOURNAL) == 0; // False to omit journal
    let mut szPageDflt: u32 = SQLITE_DEFAULT_PAGE_SIZE as u32; // Default page size
    let mut vfsFlags = vfsFlags;

    *ppPager = None;

    // Split the C-layout file name into the name and the URI block.
    let zUri: Option<ZStr> = zFilename.and_then(uri_block); // URI args to copy
    let mut zFilename: Option<ZStr> = zFilename.map(|z| zpath_name(z).to_vec());

    if (flags & PAGER_MEMORY) != 0 {
        memDb = 1;
        if let Some(z) = &zFilename {
            if !z.is_empty() {
                zPathname = z.clone();
                zFilename = None;
            }
        }
    }

    // Compute and store the full pathname in an allocated buffer pointed
    // to by zPathname, length nPathname. Or, if this is a temporary file,
    // leave both nPathname and zPathname set to 0.
    let zOrig = zFilename.clone();
    if let Some(z) = &zOrig {
        if !z.is_empty() {
            let nPathname = pVfs.mxPathname + 1;
            rc = sqlite3OsFullPathname(pVfs, z, nPathname, &mut zPathname);
            if rc != SQLITE_OK {
                if rc == SQLITE_OK_SYMLINK {
                    if (vfsFlags & SQLITE_OPEN_NOFOLLOW) != 0 {
                        rc = SQLITE_CANTOPEN_SYMLINK;
                    } else {
                        rc = SQLITE_OK;
                    }
                }
            }
            if rc == SQLITE_OK && zPathname.len() as i32 + 8 > pVfs.mxPathname {
                // This branch is taken when the journal path required by
                // the database being opened will be more than
                // pVfs->mxPathname bytes in length. This means the
                // database cannot be opened, as it will not be possible to
                // open the journal file or WAL file.
                rc = SQLITE_CANTOPEN;
            }
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }
    let nPathname = zPathname.len();
    let mut zFull: ZStr = zPathname.clone();
    if nPathname > 0 && memDb == 0 {
        if let Some(u) = &zUri {
            zFull.push(0);
            zFull.extend_from_slice(u);
        }
    }
    let zJournal = if nPathname > 0 {
        let mut j = zPathname.clone();
        j.extend_from_slice(b"-journal");
        Some(j)
    } else {
        None
    };
    let zWal = if nPathname > 0 {
        let mut w = zPathname.clone();
        w.extend_from_slice(b"-wal");
        Some(w)
    } else {
        None
    };

    let mut pPager = Pager {
        pSelf: Weak::new(),
        pVfs: pVfs.clone(),
        exclusiveMode: 0,
        journalMode: 0,
        useJournal: 0,
        noSync: 0,
        fullSync: 0,
        extraSync: 0,
        syncFlags: 0,
        walSyncFlags: 0,
        tempFile: 0,
        noLock: 0,
        readOnly: 0,
        memDb: 0,
        memVfs: 0,
        eState: PAGER_OPEN,
        eLock: NO_LOCK as u8,
        changeCountDone: 0,
        setSuper: 0,
        doNotSpill: 0,
        subjInMemory: 0,
        bUseFetch: 0,
        hasHeldSharedLock: 0,
        dbSize: 0,
        dbOrigSize: 0,
        dbFileSize: 0,
        dbHintSize: 0,
        errCode: SQLITE_OK,
        nRec: 0,
        cksumInit: 0,
        nSubRec: 0,
        pInJournal: None,
        fd: SqliteFile::Closed,
        jfd: SqliteFile::Closed,
        sjfd: SqliteFile::Closed,
        journalOff: 0,
        journalHdr: 0,
        pBackup: Vec::new(),
        aSavepoint: Vec::new(),
        nSavepoint: 0,
        iDataVersion: 0,
        dbFileVers: [0; 16],
        nMmapOut: 0,
        szMmap: 0,
        nExtra: 0,
        nReserve: 0,
        vfsFlags: vfsFlags as u32,
        sectorSize: 0,
        mxPgno: 0,
        lckPgno: 0,
        pageSize: 0,
        journalSizeLimit: 0,
        zFilename: zFull,
        zJournal,
        xBusyHandler: None,
        aStat: [0; 4],
        nRead: 0,
        xReiniter: None,
        xGet: PagerGetter::Normal,
        pTmpSpace: Vec::new(),
        pPCache: PCache::default(),
        pWal: None,
        zWal,
    };

    // Open the pager file.
    let mut act_like_temp_file = true;
    if let Some(z) = &zOrig {
        if !z.is_empty() {
            act_like_temp_file = false;
            let mut fout = 0; // VFS flags returned by xOpen()
            let bImmutable = uri_boolean(Some(&pPager.zFilename), b"immutable", 0);
            if bImmutable != 0 {
                vfsFlags |= SQLITE_OPEN_READONLY;
                vfsFlags &= !(SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
            }
            let zF = pPager.zFilename.clone();
            rc = sqlite3OsOpen(pVfs, Some(&zF), &mut pPager.fd, vfsFlags, Some(&mut fout));
            if bImmutable != 0 {
                act_like_temp_file = true;
            } else {
                debug_assert!(memDb == 0);
                memJM = ((fout & SQLITE_OPEN_MEMORY) != 0) as i32;
                pPager.memVfs = memJM as u8;
                readOnly = ((fout & SQLITE_OPEN_READONLY) != 0) as i32;

                // If the file was successfully opened for read/write
                // access, choose a default page size in case we have to
                // create the database file. The default page size is the
                // maximum of:
                //
                //    + SQLITE_DEFAULT_PAGE_SIZE,
                //    + The value returned by sqlite3OsSectorSize()
                //    + The largest page size that can be written atomically.
                if rc == SQLITE_OK {
                    let iDc = sqlite3OsDeviceCharacteristics(&mut pPager.fd);
                    if readOnly == 0 {
                        setSectorSize(&mut pPager);
                        debug_assert!(SQLITE_DEFAULT_PAGE_SIZE <= SQLITE_MAX_DEFAULT_PAGE_SIZE);
                        if szPageDflt < pPager.sectorSize {
                            if pPager.sectorSize > SQLITE_MAX_DEFAULT_PAGE_SIZE as u32 {
                                szPageDflt = SQLITE_MAX_DEFAULT_PAGE_SIZE as u32;
                            } else {
                                szPageDflt = pPager.sectorSize;
                            }
                        }
                    }
                    pPager.noLock = uri_boolean(Some(&pPager.zFilename), b"nolock", 0) as u8;
                    if (iDc & SQLITE_IOCAP_IMMUTABLE) != 0 {
                        vfsFlags |= SQLITE_OPEN_READONLY;
                        act_like_temp_file = true;
                    }
                }
            }
        }
    }
    if act_like_temp_file {
        // If a temporary file is requested, it is not opened immediately.
        // In this case we accept the default page size and delay actually
        // opening the file until the first call to OsWrite().
        //
        // This branch is also run for an in-memory database. An in-memory
        // database is the same as a temp-file that is never written out to
        // disk and uses an in-memory rollback journal.
        //
        // This branch also runs for files marked as immutable.
        tempFile = 1;
        pPager.eState = PAGER_READER; // Pretend we already have a lock
        pPager.eLock = EXCLUSIVE_LOCK as u8; // Pretend we are in EXCLUSIVE mode
        pPager.noLock = 1; // Do no locking
        readOnly = vfsFlags & SQLITE_OPEN_READONLY;
    }

    // The following call to PagerSetPagesize() serves to set the value of
    // Pager.pageSize and to allocate the Pager.pTmpSpace buffer.
    if rc == SQLITE_OK {
        debug_assert!(pPager.memDb == 0);
        rc = sqlite3PagerSetPagesize(&mut pPager, &mut szPageDflt, -1);
    }

    // Initialize the PCache object.
    let mut nExtra = nExtra;
    if rc == SQLITE_OK {
        nExtra = (nExtra + 7) & !7;
        debug_assert!(nExtra >= 8 && nExtra < 1000);
        rc = sqlite3PcacheOpen(
            szPageDflt as i32,
            nExtra,
            (memDb == 0) as i32,
            if memDb == 0 { Some(pagerStress) } else { None },
            &mut pPager.pPCache,
        );
    }

    // If an error occurred above, free the  Pager structure and close the
    // file.
    if rc != SQLITE_OK {
        sqlite3OsClose(&mut pPager.fd);
        return rc;
    }

    pPager.useJournal = useJournal as u8;
    pPager.mxPgno = SQLITE_MAX_PAGE_COUNT;
    pPager.tempFile = tempFile as u8;
    debug_assert!(tempFile == PAGER_LOCKINGMODE_NORMAL || tempFile == PAGER_LOCKINGMODE_EXCLUSIVE);
    pPager.exclusiveMode = tempFile as u8;
    pPager.changeCountDone = pPager.tempFile;
    pPager.memDb = memDb as u8;
    pPager.readOnly = readOnly as u8;
    debug_assert!(useJournal || pPager.tempFile != 0);
    sqlite3PagerSetFlags(
        &mut pPager,
        ((SQLITE_DEFAULT_SYNCHRONOUS + 1) | PAGER_CACHESPILL) as u32,
    );
    pPager.nExtra = nExtra as u16;
    pPager.journalSizeLimit = SQLITE_DEFAULT_JOURNAL_SIZE_LIMIT as i64;
    debug_assert!(isOpen(&pPager.fd) || tempFile != 0);
    setSectorSize(&mut pPager);
    if !useJournal {
        pPager.journalMode = PAGER_JOURNALMODE_OFF as u8;
    } else if memDb != 0 || memJM != 0 {
        pPager.journalMode = PAGER_JOURNALMODE_MEMORY as u8;
    }
    pPager.xReiniter = xReinit;
    setGetterMethod(&mut pPager);

    let p = Rc::new(RefCell::new(pPager));
    p.borrow_mut().pSelf = Rc::downgrade(&p);
    *ppPager = Some(p);
    SQLITE_OK
}

/// The URI parameter block that follows the name in a C-layout file name
/// buffer (`name\0key\0value\0...\0`), including its terminating empty
/// key, or None if there are no parameters.
fn uri_block(z: &[u8]) -> Option<ZStr> {
    let n = z.iter().position(|&c| c == 0)?;
    let rest = &z[n + 1..];
    if rest.first().copied().unwrap_or(0) == 0 {
        return None;
    }
    let mut i = 0usize;
    while i < rest.len() && rest[i] != 0 {
        // key
        while i < rest.len() && rest[i] != 0 {
            i += 1;
        }
        i += 1;
        // value
        while i < rest.len() && rest[i] != 0 {
            i += 1;
        }
        i += 1;
    }
    let mut b = rest[..i.min(rest.len())].to_vec();
    b.push(0);
    Some(b)
}

/// C: `sqlite3_file * sqlite3_database_file_object(const char *zName)`
///
/// Return the sqlite3_file for the main database given the name of the
/// corresponding WAL or Journal name as passed into xOpen.  Not
/// supported in this port (Rust cannot recover the Pager from a name
/// pointer): always None.
pub fn sqlite3_database_file_object(zName: Option<&[u8]>) -> Option<Box<crate::os::SqliteFile>> {
    let _ = zName;
    None
}

/// This function is called after transitioning from PAGER_UNLOCK to
/// PAGER_SHARED state. It tests if there is a hot journal present in the
/// file-system for the given pager. A hot journal is one that needs to
/// be played back. According to this function, a hot-journal exists if
/// the following criteria are met:
///
///   * The journal file exists in the file system, and
///   * No process holds a RESERVED or greater lock on the database file,
///     and
///   * The database file itself is greater than 0 bytes in size, and
///   * The first byte of the journal file exists and is not 0x00.
///
/// If the current size of the database file is 0 but a journal file
/// exists, that is probably an old journal left over from a prior
/// database with the same name. In this case the journal file is just
/// deleted using the underlying VFS. If the journal file is not deleted,
/// it remains as a hot-journal.
///
/// This routine does not check if there is a super-journal filename at
/// the end of the file. If there is, and that super-journal file does not
/// exist, then this is not a hot-journal. pager_playback() detects this
/// case (in which case the journal is simply discarded).
///
/// If a hot-journal file is found to exist, *pExists is set to 1 and
/// SQLITE_OK returned. If no hot-journal file is present, *pExists is
/// set to 0 and SQLITE_OK returned. If an IO error occurs while trying
/// to determine whether or not a hot-journal file exists, the IO error
/// code is returned and the value of *pExists is undefined.
fn hasHotJournal(pPager: &mut Pager, pExists: &mut i32) -> i32 {
    let pVfs = pPager.pVfs.clone();
    let mut rc = SQLITE_OK; // Return code
    let mut exists = 1; // True if a journal file is present
    let jrnlOpen = isOpen(&pPager.jfd);
    let zJournal = pPager.zJournal.clone().unwrap_or_default();

    debug_assert!(pPager.useJournal != 0);
    debug_assert!(isOpen(&pPager.fd));
    debug_assert!(pPager.eState == PAGER_OPEN);

    *pExists = 0;
    if !jrnlOpen {
        rc = sqlite3OsAccess(&pVfs, &zJournal, SQLITE_ACCESS_EXISTS, &mut exists);
    }
    if rc == SQLITE_OK && exists != 0 {
        let mut locked = 0; // True if some process holds a RESERVED lock

        // Race condition here:  Another process might have been holding
        // the the RESERVED lock and have a journal open at the
        // sqlite3OsAccess() call above.  If that is the case, then the
        // journal might have been closed and deleted by the time we get
        // to the next statement.
        rc = sqlite3OsCheckReservedLock(&mut pPager.fd, &mut locked);
        if rc == SQLITE_OK && locked == 0 {
            let mut nPage: Pgno = 0; // Number of pages in database file

            debug_assert!(pPager.tempFile == 0);
            rc = pagerPagecount(pPager, &mut nPage);
            if rc == SQLITE_OK {
                // If the database is zero pages in size, that means that
                // either (1) the journal is a leftover from a prior
                // database with the same name, or (2) the database file is
                // being written by another process and the journal will be
                // closed and deleted soon.  Either way, the journal is not
                // hot and can be ignored.
                if nPage == 0 && !jrnlOpen {
                    if pagerLockDb(pPager, RESERVED_LOCK) == SQLITE_OK {
                        sqlite3OsDelete(&pVfs, &zJournal, 0);
                        if pPager.exclusiveMode == 0 {
                            pagerUnlockDb(pPager, SHARED_LOCK);
                        }
                    }
                } else {
                    // The journal file exists and no other connection has
                    // a reserved or greater lock on the database file. Now
                    // check that there is at least one non-zero bytes at
                    // the start of the journal file.  If there is, then it
                    // is a hot journal.
                    if !jrnlOpen {
                        let mut f = SQLITE_OPEN_READONLY | SQLITE_OPEN_MAIN_JOURNAL;
                        rc =
                            sqlite3OsOpen(&pVfs, Some(&zJournal), &mut pPager.jfd, f, Some(&mut f));
                    }
                    if rc == SQLITE_OK {
                        let mut first = [0u8; 1];
                        rc = sqlite3OsRead(&mut pPager.jfd, &mut first, 1, 0);
                        if rc == SQLITE_IOERR_SHORT_READ {
                            rc = SQLITE_OK;
                        }
                        if !jrnlOpen {
                            sqlite3OsClose(&mut pPager.jfd);
                        }
                        *pExists = (first[0] != 0) as i32;
                    } else if rc == SQLITE_CANTOPEN {
                        // If we cannot open the rollback journal file in
                        // order to see if its has a zero header, that
                        // might be due to an I/O error, or it might be due
                        // to a race condition with another process.  Since
                        // we cannot be sure, it is safest to assume that
                        // the journal is hot.
                        *pExists = 1;
                        rc = SQLITE_OK;
                    }
                }
            }
        }
    }
    rc
}

/// C: `int sqlite3PagerSharedLock(Pager *pPager)`
///
/// This function is called to obtain a shared lock on the database file.
/// It is illegal to call sqlite3PagerGet() until after this function has
/// been successfully called. If a shared-lock is already held when this
/// function is called, it is a no-op.
///
/// The following operations are also performed by this function.
///
///   1) If the pager is currently in PAGER_OPEN state (no lock held on
///      the database file), then an attempt is made to obtain a SHARED
///      lock on the database file. Immediately after obtaining the SHARED
///      lock, the file-system is checked for a hot-journal, which is
///      played back if present. Following any hot-journal rollback, the
///      contents of the cache are validated by checking the 'change-
///      counter' field of the database file header and discarded if they
///      are found to be invalid.
///
///   2) If the pager is running in exclusive-mode, and there are
///      currently no outstanding references to any pages, and is in the
///      error state, then an attempt is made to clear the error state by
///      discarding the contents of the page cache and rolling back any
///      open journal file.
///
/// If everything is successful, SQLITE_OK is returned. If an IO error
/// occurs while locking the database, checking for a hot-journal file or
/// rolling back a journal file, the IO error code is returned.
pub fn sqlite3PagerSharedLock(pPager: &mut crate::pager::Pager) -> i32 {
    let mut rc = SQLITE_OK; // Return code

    // This routine is only called from b-tree and only when there are no
    // outstanding pages. This implies that the pager state should either
    // be OPEN or READER. READER is only possible if the pager is or was
    // in exclusive access mode.
    debug_assert!(sqlite3PcacheRefCount(&mut pPager.pPCache) == 0);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.eState == PAGER_OPEN || pPager.eState == PAGER_READER);
    debug_assert!(pPager.errCode == SQLITE_OK);

    'failed: {
        if !pagerUseWal(pPager) && pPager.eState == PAGER_OPEN {
            let mut bHotJournal = 1; // True if there exists a hot journal-file

            debug_assert!(!MEMDB(pPager));
            debug_assert!(pPager.tempFile == 0 || pPager.eLock as i32 == EXCLUSIVE_LOCK);

            rc = pager_wait_on_lock(pPager, SHARED_LOCK);
            if rc != SQLITE_OK {
                debug_assert!(pPager.eLock as i32 == NO_LOCK || pPager.eLock == UNKNOWN_LOCK);
                break 'failed;
            }

            // If a journal file exists, and there is no RESERVED lock on
            // the database file, then it either needs to be played back or
            // deleted.
            if pPager.eLock as i32 <= SHARED_LOCK {
                rc = hasHotJournal(pPager, &mut bHotJournal);
            }
            if rc != SQLITE_OK {
                break 'failed;
            }
            if bHotJournal != 0 {
                if pPager.readOnly != 0 {
                    rc = SQLITE_READONLY_ROLLBACK;
                    break 'failed;
                }

                // Get an EXCLUSIVE lock on the database file. At this
                // point it is important that a RESERVED lock is not
                // obtained on the way to the EXCLUSIVE lock. If it were,
                // another process might open and play back the journal
                // file between the time the RESERVED lock was obtained
                // and the EXCLUSIVE lock (the playback would fail since
                // the EXCLUSIVE lock is held, but the journal would still
                // be considered hot).
                //
                // Because the intermediate RESERVED lock is not requested,
                // any other process attempting to access the database
                // file will get to this point in the code and fail to
                // obtain its own EXCLUSIVE lock on the database file.
                //
                // Unless the pager is in locking_mode=exclusive mode, the
                // lock is downgraded to SHARED_LOCK before this function
                // returns.
                rc = pagerLockDb(pPager, EXCLUSIVE_LOCK);
                if rc != SQLITE_OK {
                    break 'failed;
                }

                // If it is not already open and the file exists on disk,
                // open the journal for read/write access. Write access is
                // required because in exclusive-access mode the file
                // descriptor will be kept open and possibly used for a
                // transaction later on. Also, write-access is usually
                // required to finalize the journal in journal_mode=persist
                // mode (and also for journal_mode=truncate on some
                // systems).
                //
                // If the journal does not exist, it usually means that
                // some other connection managed to get in and roll it back
                // before this connection obtained the exclusive lock above.
                // Or, it may mean that the pager was in the error-state
                // when this function was called and the journal file does
                // not exist because it was rolled back by a previous call
                // to this function.  In either case, the call to
                // sqlite3OsAccess() below will find the journal does not
                // exist.
                if !isOpen(&pPager.jfd) && pPager.journalMode as i32 != PAGER_JOURNALMODE_OFF {
                    let pVfs = pPager.pVfs.clone();
                    let mut bExists = 0; // True if journal file exists
                    let zJournal = pPager.zJournal.clone().unwrap_or_default();
                    rc = sqlite3OsAccess(&pVfs, &zJournal, SQLITE_ACCESS_EXISTS, &mut bExists);
                    if rc == SQLITE_OK && bExists != 0 {
                        let mut fout = 0;
                        let f = SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_JOURNAL;
                        debug_assert!(pPager.tempFile == 0);
                        rc = sqlite3OsOpen(
                            &pVfs,
                            Some(&zJournal),
                            &mut pPager.jfd,
                            f,
                            Some(&mut fout),
                        );
                        debug_assert!(rc != SQLITE_OK || isOpen(&pPager.jfd));
                        if rc == SQLITE_OK && (fout & SQLITE_OPEN_READONLY) != 0 {
                            rc = SQLITE_CANTOPEN;
                            sqlite3OsClose(&mut pPager.jfd);
                        }
                    }
                }

                // Playback and delete the journal.  Drop the database
                // write lock and reacquire the read lock. Purge the cache
                // before playing back the hot-journal so that we don't end
                // up with an inconsistent cache.  Sync the hot journal
                // before playing it back.
                if isOpen(&pPager.jfd) {
                    debug_assert!(rc == SQLITE_OK);
                    rc = pagerSyncHotJournal(pPager);
                    if rc == SQLITE_OK {
                        let isHot = (pPager.tempFile == 0) as i32;
                        rc = pager_playback(pPager, isHot);
                        pPager.eState = PAGER_OPEN;
                    }
                } else if pPager.exclusiveMode == 0 {
                    pagerUnlockDb(pPager, SHARED_LOCK);
                }

                if rc != SQLITE_OK {
                    // This branch is taken if an error occurs while trying
                    // to open or roll back a hot-journal while holding an
                    // EXCLUSIVE lock. The pager_unlock() routine will be
                    // called before returning to unlock the file. If the
                    // unlock attempt fails, then Pager.eLock must be set
                    // to UNKNOWN_LOCK (see the comment above the #define
                    // for UNKNOWN_LOCK above for an explanation).
                    //
                    // In order to get pager_unlock() to do this, set
                    // Pager.eState to PAGER_ERROR now. This is not
                    // actually used to signal an error, but it is the
                    // easiest way to ensure that the appropriate behavior
                    // is achieved.
                    pager_error(pPager, rc);
                    break 'failed;
                }

                debug_assert!(pPager.eState == PAGER_OPEN);
                debug_assert!(
                    (pPager.eLock as i32 == SHARED_LOCK)
                        || (pPager.exclusiveMode != 0 && pPager.eLock as i32 > SHARED_LOCK)
                );
            }

            if pPager.tempFile == 0 && pPager.hasHeldSharedLock != 0 {
                // The shared-lock has just been acquired then check to see
                // if the database has been modified.  If the database has
                // changed, flush the cache.  The hasHeldSharedLock flag
                // prevents this from happening on the very first
                // transaction.
                //
                // There is a vanishingly small chance that a change will
                // not be detected.  The chance of an undetected change is
                // so small that it can be neglected.
                let mut dbFileVers = [0u8; 16];
                rc = sqlite3OsRead(&mut pPager.fd, &mut dbFileVers, 16, 24);
                if rc != SQLITE_OK {
                    if rc != SQLITE_IOERR_SHORT_READ {
                        break 'failed;
                    }
                    dbFileVers = [0u8; 16];
                }

                if pPager.dbFileVers != dbFileVers {
                    pager_reset(pPager);
                }
            }

            // If there is a WAL file in the file-system, open this database
            // in WAL mode. Otherwise, the following function call is a
            // no-op.
            rc = pagerOpenWalIfPresent(pPager);
            debug_assert!(!pagerUseWal(pPager) || rc == SQLITE_OK);
        }

        if pagerUseWal(pPager) {
            debug_assert!(rc == SQLITE_OK);
            rc = pagerBeginReadTransaction(pPager);
        }

        if pPager.tempFile == 0 && pPager.eState == PAGER_OPEN && rc == SQLITE_OK {
            let mut n: Pgno = 0;
            rc = pagerPagecount(pPager, &mut n);
            pPager.dbSize = n;
        }
    }

    if rc != SQLITE_OK {
        debug_assert!(!MEMDB(pPager));
        pager_unlock(pPager);
        debug_assert!(pPager.eState == PAGER_OPEN);
    } else {
        pPager.eState = PAGER_READER;
        pPager.hasHeldSharedLock = 1;
    }
    rc
}

/// If the reference count has reached zero, rollback any active
/// transaction and unlock the pager.
///
/// Except, in locking_mode=EXCLUSIVE when there is nothing to in the
/// rollback journal, the unlock is not performed and there is nothing to
/// rollback, so this routine is a no-op.
fn pagerUnlockIfUnused(pPager: &mut Pager) {
    if sqlite3PcacheRefCount(&mut pPager.pPCache) == 0 {
        debug_assert!(pPager.nMmapOut == 0); // because page1 is never memory mapped
        pagerUnlockAndRollback(pPager);
    }
}

/// The page getter methods each try to acquire a reference to a page with
/// page number pgno. If the requested reference is successfully
/// obtained, it is copied to *ppPage and SQLITE_OK returned.
///
/// There are different implementations of the getter method depending on
/// the current state of the pager.
///
///     getPageNormal()         --  The normal getter
///     getPageError()          --  Used if the pager is in an error state
///     getPageMmap()           --  Used if memory-mapped I/O is enabled
///
/// If the requested page is already in the cache, it is returned.
/// Otherwise, a new page object is allocated and populated with data
/// read from the database file. In some cases, the pcache module may
/// choose not to allocate a new page object and may reuse an existing
/// object with no outstanding references.
///
/// The extra data appended to a page is always initialized to zeros the
/// first time a page is loaded into memory. If the page requested is
/// already in the cache when this function is called, then the extra
/// data is left as it was when the page object was last used.
///
/// If the database image is smaller than the requested page or if the
/// flags parameter contains the PAGER_GET_NOCONTENT bit and the
/// requested page is not already stored in the cache, then no actual
/// disk read occurs. In this case the memory image of the page is
/// initialized to all zeros.
///
/// If PAGER_GET_NOCONTENT is true, it means that we do not care about
/// the contents of the page. This occurs in two scenarios:
///
///   a) When reading a free-list leaf page from the database, and
///
///   b) When a savepoint is being rolled back and we need to load a new
///      page into the cache to be filled with the data read from the
///      savepoint journal.
///
/// If PAGER_GET_NOCONTENT is true, then the data returned is zeroed
/// instead of being read from the database. Additionally, the bits
/// corresponding to pgno in Pager.pInJournal (bitvec of pages already
/// written to the journal file) and the PagerSavepoint.pInSavepoint
/// bitvecs of any open savepoints are set to 1. This is done for two
/// reasons:
///
///   1) The data on the page will not be written to the journal.
///   2) A page is being moved from the free-list to the main data
///      structures, its content is not needed (or stored in the journal).
///
/// The acquisition might fail for several reasons.  In all cases, an
/// appropriate error code is returned and *ppPage is set to NULL.
///
/// See also sqlite3PagerLookup().  Both this routine and Lookup() attempt
/// to find a page in the in-memory cache first.  If the page is not
/// already in memory, this routine goes to disk to read it in whereas
/// Lookup() just returns 0.  This routine acquires a read-lock the first
/// time it has to go to disk, and could also playback an old journal if
/// necessary.  Since Lookup() never goes to disk, it never has to deal
/// with locks or journal files.
fn getPageNormal(pPager: &mut Pager, pgno: Pgno, ppPage: &mut Option<DbPage>, flags: i32) -> i32 {
    let mut rc;
    let mut pPg: Option<DbPage> = None;

    debug_assert!(pPager.errCode == SQLITE_OK);
    debug_assert!(pPager.eState >= PAGER_READER);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.hasHeldSharedLock == 1);

    'pager_acquire_err: {
        if pgno == 0 {
            *ppPage = None;
            return SQLITE_CORRUPT;
        }
        let mut pBase = sqlite3PcacheFetch(&mut pPager.pPCache, pgno, 3);
        if pBase.is_none() {
            rc = sqlite3PcacheFetchStress(pPager, pgno, &mut pBase);
            if rc != SQLITE_OK {
                break 'pager_acquire_err;
            }
            if pBase.is_none() {
                rc = SQLITE_NOMEM;
                break 'pager_acquire_err;
            }
        }
        let p = sqlite3PcacheFetchFinish(&mut pPager.pPCache, pgno, pBase.as_ref().unwrap());
        *ppPage = Some(p.clone());
        pPg = Some(p.clone());
        debug_assert!(pgpgno(&p) == pgno);

        let noContent = (flags & PAGER_GET_NOCONTENT) != 0;
        let hasPager = p.borrow().pPager.strong_count() > 0;
        if hasPager && !noContent {
            // In this case the pcache already contains an initialized copy
            // of the page. Return without further ado.
            debug_assert!(pgno != PAGER_SJ_PGNO(pPager));
            pPager.aStat[PAGER_STAT_HIT] = pPager.aStat[PAGER_STAT_HIT].wrapping_add(1);
            return SQLITE_OK;
        } else {
            // The pager cache has created a new page. Its content needs to
            // be initialized. But first some error checks:
            //
            // (*) obsolete.  Was: maximum page number is 2^31
            // (2) Never try to fetch the locking page
            if pgno == PAGER_SJ_PGNO(pPager) {
                rc = SQLITE_CORRUPT;
                break 'pager_acquire_err;
            }

            p.borrow_mut().pPager = pPager.pSelf.clone();

            debug_assert!(!isOpen(&pPager.fd) || !MEMDB(pPager));
            if !isOpen(&pPager.fd) || pPager.dbSize < pgno || noContent {
                if pgno > pPager.mxPgno {
                    rc = SQLITE_FULL;
                    if pgno <= pPager.dbSize {
                        sqlite3PcacheRelease(&mut pPager.pPCache, &p);
                        pPg = None;
                    }
                    break 'pager_acquire_err;
                }
                if noContent {
                    // Failure to set the bits in the InJournal bit-vectors
                    // is benign.  It merely means that we might do some
                    // extra work to journal a page that does not need to
                    // be journaled.  Nevertheless, be sure to test the
                    // case where a malloc error occurs while trying to set
                    // a bit in a bit vector.
                    if pgno <= pPager.dbOrigSize {
                        sqlite3BitvecSetOpt(pPager.pInJournal.as_deref_mut(), pgno);
                    }
                    addToSavepointBitvecs(pPager, pgno);
                }
                let mut pg = p.borrow_mut();
                let n = pPager.pageSize as usize;
                for b in pg.aData[..n].iter_mut() {
                    *b = 0;
                }
            } else {
                debug_assert!(p.borrow().pPager.ptr_eq(&pPager.pSelf));
                pPager.aStat[PAGER_STAT_MISS] = pPager.aStat[PAGER_STAT_MISS].wrapping_add(1);
                rc = readDbPage(pPager, &p);
                if rc != SQLITE_OK {
                    break 'pager_acquire_err;
                }
            }
        }
        return SQLITE_OK;
    }

    debug_assert!(rc != SQLITE_OK);
    if let Some(p) = pPg {
        sqlite3PcacheDrop(&mut pPager.pPCache, &p);
    }
    pagerUnlockIfUnused(pPager);
    *ppPage = None;
    rc
}

/// The page getter for when the pager is in the error state
fn getPageError(pPager: &mut Pager, _pgno: Pgno, ppPage: &mut Option<DbPage>, _flags: i32) -> i32 {
    debug_assert!(pPager.errCode != SQLITE_OK);
    *ppPage = None;
    pPager.errCode
}

/// C: `int sqlite3PagerGet( Pager *pPager, Pgno pgno, DbPage **ppPage, int flags )`
///
/// Dispatch all page fetch requests to the appropriate getter method.
pub fn sqlite3PagerGet(
    pPager: &mut crate::pager::Pager,
    pgno: Pgno,
    ppPage: &mut Option<DbPage>,
    flags: i32,
) -> i32 {
    match pPager.xGet {
        PagerGetter::Normal => getPageNormal(pPager, pgno, ppPage, flags),
        PagerGetter::Error => getPageError(pPager, pgno, ppPage, flags),
    }
}

/// C: `DbPage * sqlite3PagerLookup(Pager *pPager, Pgno pgno)`
///
/// Acquire a page if it is already in the in-memory cache.  Do not read
/// the page from disk.  Return a pointer to the page, or 0 if the page is
/// not in cache.
///
/// See also sqlite3PagerGet().  The difference between this routine and
/// sqlite3PagerGet() is that _get() will go to the disk and read in the
/// page if the page is not already in cache.  This routine returns NULL
/// if the page is not in cache or if a disk I/O error has ever happened.
pub fn sqlite3PagerLookup(pPager: &mut crate::pager::Pager, pgno: Pgno) -> Option<DbPage> {
    debug_assert!(pgno != 0);
    let pPage = sqlite3PcacheFetch(&mut pPager.pPCache, pgno, 0)?;
    debug_assert!(pPager.hasHeldSharedLock != 0);
    Some(sqlite3PcacheFetchFinish(&mut pPager.pPCache, pgno, &pPage))
}

/// sqlite3PagerUnrefNotNull() with the pager already borrowed.
fn pagerUnrefNotNull(pPager: &mut Pager, pPg: &DbPage) {
    sqlite3PcacheRelease(&mut pPager.pPCache, pPg);
}

/// C: `void sqlite3PagerUnrefNotNull(DbPage *pPg)`
///
/// Release a page reference.
///
/// The sqlite3PagerUnref() and sqlite3PagerUnrefNotNull() may only be
/// used if we know that the page being released is not the last reference
/// to page1, and that there are already at least two references to the
/// page as well (or the page is not page 1).  For page 1, use
/// sqlite3PagerUnrefPageOne() for the last reference.
pub fn sqlite3PagerUnrefNotNull(pPg: &DbPage) {
    match page_pager(pPg) {
        Some(p) => pagerUnrefNotNull(&mut p.borrow_mut(), pPg),
        None => {
            let mut pg = pPg.borrow_mut();
            if pg.nRef > 0 {
                pg.nRef -= 1;
            }
        }
    }
}

/// sqlite3PagerUnrefNotNull() for callers that already hold the pager.
pub fn sqlite3PagerUnrefWith(pPager: &mut Pager, pPg: &DbPage) {
    pagerUnrefNotNull(pPager, pPg);
}

/// C: `void sqlite3PagerUnref(DbPage *pPg)`
pub fn sqlite3PagerUnref(pPg: &DbPage) {
    sqlite3PagerUnrefNotNull(pPg);
}

/// C: `void sqlite3PagerUnrefPageOne(DbPage *pPg)`
///
/// Release the last reference to page 1 (and unlock the pager if it is
/// no longer used).
pub fn sqlite3PagerUnrefPageOne(pPg: &DbPage) {
    debug_assert!(pPg.borrow().pgno == 1);
    debug_assert!((pgflags(pPg) & PGHDR_MMAP) == 0); // Page1 is never memory mapped
    if let Some(p) = page_pager(pPg) {
        let mut pPager = p.borrow_mut();
        sqlite3PcacheRelease(&mut pPager.pPCache, pPg);
        pagerUnlockIfUnused(&mut pPager);
    }
}

/// This function is called at the start of every write transaction.
/// There must already be a RESERVED or EXCLUSIVE lock on the database
/// file when this routine is called.
///
/// Open the journal file for pager pPager and write a journal header
/// to the start of it. If there are active savepoints, open the sub-
/// journal as well. This function is only used when the journal file is
/// being opened to write a rollback log for a transaction. It is not used
/// when opening a hot journal file to roll it back.
///
/// If the journal file is already open (as it may be in exclusive mode),
/// then this function just writes a journal header to the start of the
/// already open file.
///
/// Whether or not the journal file is opened by this function, the
/// Pager.pInJournal bitvec structure is allocated.
///
/// Return SQLITE_OK if everything is successful. Otherwise, return
/// SQLITE_NOMEM if the attempt to allocate Pager.pInJournal fails, or an
/// IO error code if opening or writing the journal file fails.
fn pager_open_journal(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK; // Return code
    let pVfs = pPager.pVfs.clone(); // Local cache of vfs pointer

    debug_assert!(pPager.eState == PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.pInJournal.is_none());

    // If already in the error state, this function is a no-op.  But on
    // the other hand, this routine is never called if we are already in
    // an error state.
    if pPager.errCode != 0 {
        return pPager.errCode;
    }

    if !pagerUseWal(pPager) && pPager.journalMode as i32 != PAGER_JOURNALMODE_OFF {
        pPager.pInJournal = sqlite3BitvecCreate(pPager.dbSize);

        // Open the journal file if it is not already open.
        if !isOpen(&pPager.jfd) {
            if pPager.journalMode as i32 == PAGER_JOURNALMODE_MEMORY {
                sqlite3MemJournalOpen(&mut pPager.jfd);
            } else {
                let mut flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
                let nSpill;

                if pPager.tempFile != 0 {
                    flags |= SQLITE_OPEN_DELETEONCLOSE | SQLITE_OPEN_TEMP_JOURNAL;
                    flags |= SQLITE_OPEN_EXCLUSIVE;
                    nSpill = crate::global::sqlite3Config.nStmtSpill;
                } else {
                    flags |= SQLITE_OPEN_MAIN_JOURNAL;
                    nSpill = jrnlBufferSize(pPager);
                }

                // Verify that the database still has the same name as it
                // did when it was originally opened.
                rc = databaseIsUnmoved(pPager);
                if rc == SQLITE_OK {
                    let zJournal = pPager.zJournal.clone();
                    rc = sqlite3JournalOpen(
                        Some(&pVfs),
                        zJournal.as_deref(),
                        &mut pPager.jfd,
                        flags,
                        nSpill,
                    );
                }
            }
            debug_assert!(rc != SQLITE_OK || isOpen(&pPager.jfd));
        }

        // Write the first journal header to the journal file and open the
        // sub-journal if necessary.
        if rc == SQLITE_OK {
            // TODO: Check if all of these are really required.
            pPager.nRec = 0;
            pPager.journalOff = 0;
            pPager.setSuper = 0;
            pPager.journalHdr = 0;
            rc = writeJournalHdr(pPager);
        }
    }

    if rc != SQLITE_OK {
        pPager.pInJournal = None;
        pPager.journalOff = 0;
    } else {
        debug_assert!(pPager.eState == PAGER_WRITER_LOCKED);
        pPager.eState = PAGER_WRITER_CACHEMOD;
    }
    rc
}

/// C: `int sqlite3PagerBegin(Pager *pPager, int exFlag, int subjInMemory)`
///
/// Begin a write-transaction on the specified pager object. If a
/// write-transaction has already been opened, this function is a no-op.
///
/// If the exFlag argument is false, then acquire at least a RESERVED lock
/// on the database file. If exFlag is true, then acquire at least an
/// EXCLUSIVE lock. If such a lock is already held, no locking functions
/// are invoked.
///
/// If the subjInMemory argument is non-zero, then any sub-journal opened
/// within this transaction will be opened as an in-memory file. This has
/// no effect if the sub-journal is already opened (as it may be when
/// running in exclusive mode) or if the transaction does not require a
/// sub-journal. If the subjInMemory argument is zero, then any required
/// sub-journal is implemented in-memory if pPager is an in-memory
/// database, or using a temporary file otherwise.
pub fn sqlite3PagerBegin(pPager: &mut crate::pager::Pager, exFlag: i32, subjInMemory: i32) -> i32 {
    let mut rc = SQLITE_OK;

    if pPager.errCode != 0 {
        return pPager.errCode;
    }
    debug_assert!(pPager.eState >= PAGER_READER && pPager.eState < PAGER_ERROR);
    pPager.subjInMemory = subjInMemory as u8;

    if pPager.eState == PAGER_READER {
        debug_assert!(pPager.pInJournal.is_none());

        if pagerUseWal(pPager) {
            // If the pager is configured to use locking_mode=exclusive,
            // and an exclusive lock on the database is not already held,
            // obtain it now.
            if pPager.exclusiveMode != 0
                && crate::wal::sqlite3WalExclusiveMode(pPager.pWal.as_mut().unwrap(), -1) != 0
            {
                rc = pagerLockDb(pPager, EXCLUSIVE_LOCK);
                if rc != SQLITE_OK {
                    return rc;
                }
                let _ = crate::wal::sqlite3WalExclusiveMode(pPager.pWal.as_mut().unwrap(), 1);
            }

            // Grab the write lock on the log file. If successful, upgrade
            // to PAGER_RESERVED state. Otherwise, return an error code to
            // the caller. The busy-handler is not invoked if another
            // connection already holds the write-lock. If possible, the
            // upper layer will call it.
            rc = crate::wal::sqlite3WalBeginWriteTransaction(pPager.pWal.as_mut().unwrap());
        } else {
            // Obtain a RESERVED lock on the database file. If the exFlag
            // parameter is true, then immediately upgrade this to an
            // EXCLUSIVE lock. The busy-handler is not invoked if the
            // request for the RESERVED lock fails (the upper layer will
            // invoke it).
            rc = pagerLockDb(pPager, RESERVED_LOCK);
            if rc == SQLITE_OK && exFlag != 0 {
                rc = pager_wait_on_lock(pPager, EXCLUSIVE_LOCK);
            }
        }

        if rc == SQLITE_OK {
            // Change to WRITER_LOCKED state.
            //
            // WAL mode sets Pager.eState to PAGER_WRITER_LOCKED or CACHEMOD
            // when it has an open transaction, but never to DBMOD or
            // FINISHED. This is because in those states the code to roll
            // back savepoint transactions may copy data from the
            // sub-journal into the database file as well as into the page
            // cache. Which would be incorrect in WAL mode.
            pPager.eState = PAGER_WRITER_LOCKED;
            pPager.dbHintSize = pPager.dbSize;
            pPager.dbFileSize = pPager.dbSize;
            pPager.dbOrigSize = pPager.dbSize;
            pPager.journalOff = 0;
        }

        debug_assert!(rc == SQLITE_OK || pPager.eState == PAGER_READER);
        debug_assert!(rc != SQLITE_OK || pPager.eState == PAGER_WRITER_LOCKED);
        debug_assert!(assert_pager_state(pPager));
    }
    rc
}

/// Write page pPg onto the end of the rollback journal.
fn pagerAddPageToRollbackJournal(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc;
    let iOff = pPager.journalOff;
    let pgno = pgpgno(pPg);

    // We should never write to the journal file the page that contains
    // the database locks.  The following assert verifies that we do not.
    debug_assert!(pgno != PAGER_SJ_PGNO(pPager));

    debug_assert!(pPager.journalHdr <= pPager.journalOff);
    let pageSize = pPager.pageSize as i32;
    let cksum = {
        let b = pPg.borrow();
        pager_cksum(pPager, &b.aData)
    };

    // Even if an IO or diskfull error occurs while journalling the page in
    // the block above, set the need-sync flag for the page.  Otherwise,
    // when the transaction is rolled back, the logic in playback_one_page()
    // will think that the page needs to be restored in the database file.
    // And if an IO error occurs while doing so, then corruption may
    // follow.
    pPg.borrow_mut().flags |= PGHDR_NEED_SYNC as u16;

    rc = write32bits(&mut pPager.jfd, iOff, pgno);
    if rc != SQLITE_OK {
        return rc;
    }
    {
        let b = pPg.borrow();
        rc = sqlite3OsWrite(&mut pPager.jfd, &b.aData, pageSize, iOff + 4);
    }
    if rc != SQLITE_OK {
        return rc;
    }
    rc = write32bits(&mut pPager.jfd, iOff + pPager.pageSize + 4, cksum);
    if rc != SQLITE_OK {
        return rc;
    }

    pPager.journalOff += 8 + pPager.pageSize;
    pPager.nRec += 1;
    debug_assert!(pPager.pInJournal.is_some());
    rc = sqlite3BitvecSetOpt(pPager.pInJournal.as_deref_mut(), pgno);
    rc |= addToSavepointBitvecs(pPager, pgno);
    rc
}

/// Mark a single data page as writeable. The page is written into the
/// main journal or sub-journal as required. If the page is written into
/// one of the journals, the corresponding bit is set in the
/// Pager.pInJournal bitvec and the PagerSavepoint.pInSavepoint bitvecs of
/// all open savepoints as appropriate.
fn pager_write(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc = SQLITE_OK;

    // This routine is not called unless a write-transaction has already
    // been started. The journal file may or may not be open at this point.
    // It is never called in the ERROR state.
    debug_assert!(
        pPager.eState == PAGER_WRITER_LOCKED
            || pPager.eState == PAGER_WRITER_CACHEMOD
            || pPager.eState == PAGER_WRITER_DBMOD
    );
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.errCode == 0);
    debug_assert!(pPager.readOnly == 0);

    // The journal file needs to be opened. Higher level routines have
    // already obtained the necessary locks to begin the write-transaction,
    // but the rollback journal might not yet be open. Open it now if this
    // is the case.
    //
    // This is done before calling sqlite3PcacheMakeDirty() on the page.
    // Otherwise, if it were done after calling sqlite3PcacheMakeDirty(),
    // then attempting to open the journal file below may fail and cause
    // the page to be left on the dirty list, even though it is not
    // journalled.
    if pPager.eState == PAGER_WRITER_LOCKED {
        rc = pager_open_journal(pPager);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    debug_assert!(pPager.eState >= PAGER_WRITER_CACHEMOD);
    debug_assert!(assert_pager_state(pPager));

    // Mark the page that is about to be modified as dirty.
    sqlite3PcacheMakeDirty(&mut pPager.pPCache, pPg);

    // If a rollback journal is in use, them make sure the page that is
    // about to change is in the rollback journal, or if the page is a new
    // page off the end of the file, make sure it is marked as dirty in the
    // rollback journal.
    debug_assert!(pPager.pInJournal.is_some() == isOpen(&pPager.jfd));
    let pgno = pgpgno(pPg);
    if pPager.pInJournal.is_some()
        && sqlite3BitvecTestNotNull(pPager.pInJournal.as_mut().unwrap(), pgno) == 0
    {
        debug_assert!(!pagerUseWal(pPager));
        if pgno <= pPager.dbOrigSize {
            rc = pagerAddPageToRollbackJournal(pPager, pPg);
            if rc != SQLITE_OK {
                return rc;
            }
        } else if pPager.eState != PAGER_WRITER_DBMOD {
            pPg.borrow_mut().flags |= PGHDR_NEED_SYNC as u16;
        }
    }

    // The PGHDR_DIRTY bit is set above when the page was added to the
    // dirty-list and the PGHDR_WRITEABLE bit is also set here.
    pPg.borrow_mut().flags |= PGHDR_WRITEABLE as u16;

    // If the statement journal is open and the page is not in it, then
    // write the page into the statement journal.
    if pPager.nSavepoint > 0 {
        rc = subjournalPageIfRequired(pPager, pPg);
    }

    // Update the database size and return.
    if pPager.dbSize < pgno {
        pPager.dbSize = pgno;
    }
    rc
}

/// This is a variant of sqlite3PagerWrite() that runs when the sector
/// size is larger than the page size.  SQLite makes the (reasonable)
/// assumption that all writes to a database file will be atomic, so
/// pages are written to the journal in full sectors.  See pager.c.
fn pagerWriteLargeSector(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    let mut rc = SQLITE_OK; // Return code
    let mut needSync = false; // True if any page has PGHDR_NEED_SYNC
    let nPagePerSector = pPager.sectorSize as Pgno / pPager.pageSize as Pgno;
    let pPgno = pgpgno(pPg);

    // Set the doNotSpill NOSYNC bit to 1. This is because we cannot allow
    // a journal header to be written between the pages journaled by this
    // function.
    debug_assert!(!MEMDB(pPager));
    debug_assert!((pPager.doNotSpill & SPILLFLAG_NOSYNC) == 0);
    pPager.doNotSpill |= SPILLFLAG_NOSYNC;

    // This trick assumes that both the page-size and sector-size are an
    // integer power of 2. It sets variable pg1 to the identifier of the
    // first page of the sector pPg is located on.
    let pg1 = ((pPgno - 1) & !(nPagePerSector - 1)) + 1;

    let nPageCount = pPager.dbSize;
    let nPage: Pgno = if pPgno > nPageCount {
        (pPgno - pg1) + 1
    } else if (pg1 + nPagePerSector - 1) > nPageCount {
        nPageCount + 1 - pg1
    } else {
        nPagePerSector
    };
    debug_assert!(nPage > 0);
    debug_assert!(pg1 <= pPgno);
    debug_assert!((pg1 + nPage) > pPgno);

    let mut ii = 0;
    while ii < nPage && rc == SQLITE_OK {
        let pg = pg1 + ii;
        if pg == pPgno || !pageInJournal(pPager, pg) {
            if pg != PAGER_SJ_PGNO(pPager) {
                let mut pPage = None;
                rc = sqlite3PagerGet(pPager, pg, &mut pPage, 0);
                if rc == SQLITE_OK {
                    let pPage = pPage.unwrap();
                    rc = pager_write(pPager, &pPage);
                    if (pgflags(&pPage) & PGHDR_NEED_SYNC) != 0 {
                        needSync = true;
                    }
                    pagerUnrefNotNull(pPager, &pPage);
                }
            }
        } else if let Some(pPage) = sqlite3PagerLookup(pPager, pg) {
            if (pgflags(&pPage) & PGHDR_NEED_SYNC) != 0 {
                needSync = true;
            }
            pagerUnrefNotNull(pPager, &pPage);
        }
        ii += 1;
    }

    // If the PGHDR_NEED_SYNC flag is set for any of the nPage pages
    // starting at pg1, then it needs to be set for all of them. Because
    // writing to any of these nPage pages may damage the others, the
    // journal file must contain sync()ed copies of all of them before any
    // of them can be written out to the database file.
    if rc == SQLITE_OK && needSync {
        debug_assert!(!MEMDB(pPager));
        for ii in 0..nPage {
            if let Some(pPage) = sqlite3PagerLookup(pPager, pg1 + ii) {
                pPage.borrow_mut().flags |= PGHDR_NEED_SYNC as u16;
                pagerUnrefNotNull(pPager, &pPage);
            }
        }
    }

    debug_assert!((pPager.doNotSpill & SPILLFLAG_NOSYNC) != 0);
    pPager.doNotSpill &= !SPILLFLAG_NOSYNC;
    rc
}

/// sqlite3PagerWrite() for callers that already hold the pager.
pub fn sqlite3PagerWriteWith(pPager: &mut Pager, pPg: &DbPage) -> i32 {
    debug_assert!((pgflags(pPg) & PGHDR_MMAP) == 0);
    debug_assert!(pPager.eState >= PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(pPager));
    let (flags, pgno) = {
        let b = pPg.borrow();
        (b.flags as i32, b.pgno)
    };
    if (flags & PGHDR_WRITEABLE) != 0 && pPager.dbSize >= pgno {
        if pPager.nSavepoint != 0 {
            return subjournalPageIfRequired(pPager, pPg);
        }
        SQLITE_OK
    } else if pPager.errCode != 0 {
        pPager.errCode
    } else if pPager.sectorSize > pPager.pageSize as u32 {
        debug_assert!(pPager.tempFile == 0);
        pagerWriteLargeSector(pPager, pPg)
    } else {
        pager_write(pPager, pPg)
    }
}

/// C: `int sqlite3PagerWrite(PgHdr *pPg)`
///
/// Mark a data page as writeable. This routine must be called before
/// making changes to a page. The caller must check the return value of
/// this function and be careful not to change any page data unless this
/// routine returns SQLITE_OK.
///
/// The difference between this function and pager_write() is that this
/// function also deals with the special case where 2 or more pages fit
/// on a single disk sector. In this case all co-resident pages must have
/// been written to the journal before returning.
///
/// If an error occurs, SQLITE_NOMEM or an IO error code is returned as
/// appropriate. Otherwise, SQLITE_OK.
pub fn sqlite3PagerWrite(pPg: &DbPage) -> i32 {
    match page_pager(pPg) {
        Some(p) => sqlite3PagerWriteWith(&mut p.borrow_mut(), pPg),
        None => SQLITE_MISUSE,
    }
}

/// C: `int sqlite3PagerIswriteable(DbPage *pPg)`
///
/// Return TRUE if the page given in the argument was previously passed to
/// sqlite3PagerWrite().  In other words, return TRUE if it is ok to
/// change the content of the page.
pub fn sqlite3PagerIswriteable(pPg: &DbPage) -> i32 {
    pgflags(pPg) & PGHDR_WRITEABLE
}

/// C: `void sqlite3PagerDontWrite(PgHdr *pPg)`
///
/// A call to this routine tells the pager that it is not necessary to
/// write the information on page pPg back to the disk, even though that
/// page might be marked as dirty.  This happens, for example, when the
/// page has been added as a leaf of the freelist and so its content no
/// longer matters.
///
/// The overlying software layer calls this routine when all of the data
/// on the given page is unused. The pager marks the page as clean so that
/// it does not get written to disk.
///
/// Tests show that this optimization can quadruple the speed of large
/// DELETE operations.
///
/// This optimization cannot be used with a temp-file, as the page may
/// have been dirty at the start of the transaction. In that case, if
/// memory pressure forces page pPg out of the cache, the data does need
/// to be written out to disk so that it may be read back in if the
/// current transaction is rolled back.
pub fn sqlite3PagerDontWrite(pPg: &DbPage) {
    let p = match page_pager(pPg) {
        Some(p) => p,
        None => return,
    };
    let pPager = p.borrow();
    let mut pg = pPg.borrow_mut();
    if pPager.tempFile == 0 && (pg.flags as i32 & PGHDR_DIRTY) != 0 && pPager.nSavepoint == 0 {
        pg.flags |= PGHDR_DONT_WRITE as u16;
        pg.flags &= !(PGHDR_WRITEABLE as u16);
    }
}

/// This routine is called to increment the value of the database file
/// change-counter, stored as a 4-byte big-endian integer starting at
/// byte offset 24 of the pager file.  The secondary change counter at
/// 92 is also updated, as is the SQLite version number at offset 96.
///
/// But this only happens if the pPager->changeCountDone flag is false.
/// To avoid excess churning of page 1, the update only happens once.
/// See also the pager_write_changecounter() routine that does an
/// unconditional update of the change counters.
///
/// The isDirectMode flag may only be non-zero if the library was
/// compiled with the SQLITE_ENABLE_ATOMIC_WRITE macro defined (it is
/// not here).
fn pager_incr_changecounter(pPager: &mut Pager, isDirectMode: i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pPager.eState == PAGER_WRITER_CACHEMOD || pPager.eState == PAGER_WRITER_DBMOD);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(isDirectMode == 0);

    if pPager.changeCountDone == 0 && pPager.dbSize > 0 {
        let mut pPgHdr: Option<DbPage> = None; // Reference to page 1

        debug_assert!(pPager.tempFile == 0 && isOpen(&pPager.fd));

        // Open page 1 of the file for writing.
        rc = sqlite3PagerGet(pPager, 1, &mut pPgHdr, 0);
        debug_assert!(pPgHdr.is_none() || rc == SQLITE_OK);

        // If page one was fetched successfully, and this function is not
        // operating in direct-mode, make page 1 writable.  When not in
        // direct mode, page 1 is always held in cache and hence the
        // PagerGet() above is always successful - hence the ALWAYS on
        // rc==SQLITE_OK.
        if rc == SQLITE_OK {
            rc = sqlite3PagerWriteWith(pPager, pPgHdr.as_ref().unwrap());
        }

        if rc == SQLITE_OK {
            // Actually do the update of the change counter
            let v = pPager.dbFileVers;
            pager_write_changecounter(&v, pPgHdr.as_ref().unwrap());
            pPager.changeCountDone = 1;
        }

        // Release the page reference.
        if let Some(p) = pPgHdr {
            pagerUnrefNotNull(pPager, &p);
        }
    }
    rc
}

/// C: `int sqlite3PagerSync(Pager *pPager, const char *zSuper)`
///
/// Sync the database file to disk. This is a no-op for in-memory
/// databases or pages with the Pager.noSync flag set.
///
/// If successful, or if called on a pager for which it is a no-op, this
/// function returns SQLITE_OK. Otherwise, an IO error code is returned.
pub fn sqlite3PagerSync(pPager: &mut crate::pager::Pager, zSuper: Option<&[u8]>) -> i32 {
    let mut rc = sqlite3OsFileControl(&mut pPager.fd, SQLITE_FCNTL_SYNC, FcntlArg::Str(zSuper));
    if rc == SQLITE_NOTFOUND {
        rc = SQLITE_OK;
    }
    if rc == SQLITE_OK && pPager.noSync == 0 {
        debug_assert!(!MEMDB(pPager));
        rc = sqlite3OsSync(&mut pPager.fd, pPager.syncFlags as i32);
    }
    rc
}

/// C: `int sqlite3PagerExclusiveLock(Pager *pPager)`
///
/// This function may only be called while a write-transaction is active
/// in rollback. If the connection is in WAL mode, this call is a no-op.
/// Otherwise, if the connection does not already have an EXCLUSIVE lock
/// on the database file, an attempt is made to obtain one.
///
/// If the EXCLUSIVE lock is already held or the attempt to obtain it is
/// successful, or the connection is in WAL mode, SQLITE_OK is returned.
/// Otherwise, either SQLITE_BUSY or an SQLITE_IOERR_XXX error code is
/// returned.
pub fn sqlite3PagerExclusiveLock(pPager: &mut crate::pager::Pager) -> i32 {
    let mut rc = pPager.errCode;
    debug_assert!(assert_pager_state(pPager));
    if rc == SQLITE_OK {
        debug_assert!(
            pPager.eState == PAGER_WRITER_CACHEMOD
                || pPager.eState == PAGER_WRITER_DBMOD
                || pPager.eState == PAGER_WRITER_LOCKED
        );
        debug_assert!(assert_pager_state(pPager));
        if !pagerUseWal(pPager) {
            rc = pager_wait_on_lock(pPager, EXCLUSIVE_LOCK);
        }
    }
    rc
}

/// C: `int sqlite3PagerCommitPhaseOne( Pager *pPager, const char *zSuper, int noSync )`
///
/// Sync the database file for the pager pPager. zSuper points to the name
/// of a super-journal file that should be written into the individual
/// journal file. zSuper may be NULL, which is interpreted as no
/// super-journal (a single database transaction).
///
/// This routine ensures that:
///
///   * The database file change-counter is updated,
///   * the journal is synced (unless the atomic-write optimization is
///     used),
///   * all dirty pages are written to the database file,
///   * the database file is truncated (if required), and
///   * the database file synced.
///
/// The only thing that remains to commit the transaction is to finalize
/// (delete, truncate or zero the first part of) the journal file (or
/// delete the super-journal file if specified).
///
/// Note that if zSuper==NULL, this does not overwrite a previous value
/// passed to an sqlite3PagerCommitPhaseOne() call.
///
/// If the final parameter - noSync - is true, then the database file
/// itself is not synced. The caller must call sqlite3PagerSync() directly
/// to sync the database file before calling CommitPhaseTwo() to delete
/// the journal file in this case.
pub fn sqlite3PagerCommitPhaseOne(
    pPager: &mut crate::pager::Pager,
    zSuper: Option<&[u8]>,
    noSync: i32,
) -> i32 {
    let mut rc = SQLITE_OK; // Return code

    debug_assert!(
        pPager.eState == PAGER_WRITER_LOCKED
            || pPager.eState == PAGER_WRITER_CACHEMOD
            || pPager.eState == PAGER_WRITER_DBMOD
            || pPager.eState == PAGER_ERROR
    );
    debug_assert!(assert_pager_state(pPager));

    // If a prior error occurred, report that error again.
    if pPager.errCode != 0 {
        return pPager.errCode;
    }

    // Provide the ability to easily simulate an I/O error during testing
    // (sqlite3FaultSim(400)): not ported.

    // If no database changes have been made, return early.
    if pPager.eState < PAGER_WRITER_CACHEMOD {
        return SQLITE_OK;
    }

    debug_assert!(!MEMDB(pPager) || pPager.tempFile != 0);
    debug_assert!(isOpen(&pPager.fd) || pPager.tempFile != 0);
    'commit_phase_one_exit: {
        if !pagerFlushOnCommit(pPager, 1) {
            // If this is an in-memory db, or no pages have been written to,
            // or this function has already been called, it is mostly a
            // no-op. However, any backup operation in progress needs to be
            // restarted.
            backup_restart(pPager);
        } else if pagerUseWal(pPager) {
            let mut pList = sqlite3PcacheDirtyList(&mut pPager.pPCache);
            let mut pPageOne: Option<DbPage> = None;
            if pList.is_empty() {
                // Must have at least one page for the WAL commit flag.
                // Ticket [2d1a5c67dfc2363e44f29d9bbd57f] 2011-05-18
                rc = sqlite3PagerGet(pPager, 1, &mut pPageOne, 0);
                if let Some(p) = &pPageOne {
                    pList = vec![p.clone()];
                }
            }
            debug_assert!(rc == SQLITE_OK);
            if !pList.is_empty() {
                let dbSize = pPager.dbSize;
                rc = pagerWalFrames(pPager, &pList, dbSize, 1);
            }
            if let Some(p) = pPageOne {
                pagerUnrefNotNull(pPager, &p);
            }
            if rc == SQLITE_OK {
                sqlite3PcacheCleanAll(&mut pPager.pPCache);
            }
        } else {
            // The bBatch boolean is true if the batch-atomic-write commit
            // method should be used (never: not compiled in).

            // The following block updates the change-counter. Exactly how
            // it does this depends on whether or not the atomic-update
            // optimization was enabled at compile time, and if this
            // transaction meets the runtime criteria to use the operation:
            rc = pager_incr_changecounter(pPager, 0);
            if rc != SQLITE_OK {
                break 'commit_phase_one_exit;
            }

            // Write the super-journal name into the journal file. If a
            // super-journal file name has already been written to the
            // journal file, or if zSuper is NULL (no super-journal), then
            // this call is a no-op.
            rc = writeSuperJournal(pPager, zSuper);
            if rc != SQLITE_OK {
                break 'commit_phase_one_exit;
            }

            // Sync the journal file and write all dirty pages to the
            // database.  If the atomic-update optimization is being used,
            // this call will not create the journal file or perform any
            // real IO.
            //
            // Because the change-counter page was just modified, unless
            // the atomic-update optimization is used it is almost certain
            // that the journal requires a sync here. However, in locking
            // mode exclusive on certain file-systems, it is possible that
            // no sync is required.
            rc = syncJournal(pPager, 0);
            if rc != SQLITE_OK {
                break 'commit_phase_one_exit;
            }

            let pList = sqlite3PcacheDirtyList(&mut pPager.pPCache);
            rc = pager_write_pagelist(pPager, &pList);
            if rc != SQLITE_OK {
                debug_assert!(rc != SQLITE_IOERR_BLOCKED);
                break 'commit_phase_one_exit;
            }
            sqlite3PcacheCleanAll(&mut pPager.pPCache);

            // If the file on disk is smaller than the database image, use
            // pager_truncate to grow the file here. This can happen if in
            // the course of an auto-vacuum the database file was shrunk,
            // and then extended again by a page that was written to the
            // journal.
            if pPager.dbSize > pPager.dbFileSize {
                let nNew = pPager.dbSize - (pPager.dbSize == PAGER_SJ_PGNO(pPager)) as Pgno;
                debug_assert!(pPager.eState == PAGER_WRITER_DBMOD);
                rc = pager_truncate(pPager, nNew);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }
            }

            // Finally, sync the database file.
            if noSync == 0 {
                rc = sqlite3PagerSync(pPager, zSuper);
            }
        }
    }

    if rc == SQLITE_OK && !pagerUseWal(pPager) {
        pPager.eState = PAGER_WRITER_FINISHED;
    }
    rc
}

/// C: `int sqlite3PagerCommitPhaseTwo(Pager *pPager)`
///
/// When this function is called, the database file has been completely
/// updated to reflect the changes made by the current transaction and
/// synced to disk. The journal file still exists in the file-system
/// though, and if a failure occurs at this point it will eventually be
/// used as a hot-journal and the current transaction rolled back.
///
/// This function finalizes the journal file, either by deleting,
/// truncating or partially zeroing it, so that it cannot be used for
/// hot-journal rollback. Once this is done the transaction is
/// irrevocably committed.
///
/// If an error occurs, an IO error code is returned and the pager moves
/// into the error state. Otherwise, SQLITE_OK is returned.
pub fn sqlite3PagerCommitPhaseTwo(pPager: &mut crate::pager::Pager) -> i32 {
    // This routine should not be called if a prior error has occurred.
    // But if (due to a coding error elsewhere in the system) it does get
    // called, just return the same error code without doing anything.
    if pPager.errCode != 0 {
        return pPager.errCode;
    }
    pPager.iDataVersion = pPager.iDataVersion.wrapping_add(1);

    debug_assert!(
        pPager.eState == PAGER_WRITER_LOCKED
            || pPager.eState == PAGER_WRITER_FINISHED
            || (pagerUseWal(pPager) && pPager.eState == PAGER_WRITER_CACHEMOD)
    );
    debug_assert!(assert_pager_state(pPager));

    // An optimization. If the database was not actually modified during
    // this transaction, the pager is running in exclusive-mode and is
    // using persistent journals, then this function is a no-op.
    //
    // The start of the journal file currently contains a single journal
    // header with the nRec field set to 0. If such a journal is used as a
    // hot-journal during hot-journal rollback, 0 changes will be made to
    // the database file. So there is no need to zero the journal header.
    // Since the pager is in exclusive-mode, there is no need to drop any
    // locks either.
    if pPager.eState == PAGER_WRITER_LOCKED
        && pPager.exclusiveMode != 0
        && pPager.journalMode as i32 == PAGER_JOURNALMODE_PERSIST
    {
        debug_assert!(pPager.journalOff == JOURNAL_HDR_SZ(pPager) || pPager.journalOff == 0);
        pPager.eState = PAGER_READER;
        return SQLITE_OK;
    }

    let setSuper = pPager.setSuper as i32;
    let rc = pager_end_transaction(pPager, setSuper, 1);
    pager_error(pPager, rc)
}

/// C: `int sqlite3PagerRollback(Pager *pPager)`
///
/// If a write transaction is open, then all changes made within the
/// transaction are reverted and the current write-transaction is closed.
/// The pager falls back to PAGER_READER state if successful, or PAGER_ERROR
/// state if an error occurs.
///
/// If the pager is already in PAGER_ERROR state when this function is
/// called, it returns Pager.errCode immediately. No work is performed in
/// this case.
///
/// Otherwise, in rollback mode, this function performs two functions:
///
///   1) It rolls back the journal file, restoring all database file and
///      in-memory cache pages to the state they were in when the
///      transaction was opened, and
///
///   2) It finalizes the journal file, so that it is not used for
///      hot-journal rollback at any point in the future.
///
/// Finalization of the journal file (task 2) is only performed if the
/// rollback is successful.
///
/// In WAL mode, all cache-entries containing data modified within the
/// current transaction are either expelled from the cache or reverted to
/// their pre-transaction state by re-reading data from the database or
/// WAL files. The WAL transaction is then closed.
pub fn sqlite3PagerRollback(pPager: &mut crate::pager::Pager) -> i32 {
    let mut rc; // Return code

    // PagerRollback() is a no-op if called in READER or OPEN state. If the
    // pager is already in the ERROR state, the rollback is not attempted
    // here. Instead, the error code is returned to the caller.
    debug_assert!(assert_pager_state(pPager));
    if pPager.eState == PAGER_ERROR {
        return pPager.errCode;
    }
    if pPager.eState <= PAGER_READER {
        return SQLITE_OK;
    }

    if pagerUseWal(pPager) {
        rc = sqlite3PagerSavepoint(pPager, SAVEPOINT_ROLLBACK, -1);
        let setSuper = pPager.setSuper as i32;
        let rc2 = pager_end_transaction(pPager, setSuper, 0);
        if rc == SQLITE_OK {
            rc = rc2;
        }
    } else if !isOpen(&pPager.jfd) || pPager.eState == PAGER_WRITER_LOCKED {
        let eState = pPager.eState;
        rc = pager_end_transaction(pPager, 0, 0);
        if !MEMDB(pPager) && eState > PAGER_WRITER_LOCKED {
            // This can happen using journal_mode=off. Move the pager to
            // the error state to indicate that the contents of the cache
            // may not be trusted.  Any active readers will get
            // SQLITE_ABORT.
            pPager.errCode = SQLITE_ABORT;
            pPager.eState = PAGER_ERROR;
            setGetterMethod(pPager);
            return rc;
        }
    } else {
        rc = pager_playback(pPager, 0);
    }

    debug_assert!(pPager.eState == PAGER_READER || rc != SQLITE_OK);
    debug_assert!(
        rc == SQLITE_OK
            || rc == SQLITE_FULL
            || rc == SQLITE_CORRUPT
            || rc == SQLITE_NOMEM
            || (rc & 0xFF) == SQLITE_IOERR
            || rc == SQLITE_CANTOPEN
    );

    // If an error occurs during a ROLLBACK, we can no longer trust the
    // pager cache. So call pager_error() on the way out to make any error
    // persistent.
    pager_error(pPager, rc)
}

/// C: `u8 sqlite3PagerIsreadonly(Pager *pPager)`
///
/// Return TRUE if the database file is opened read-only.  Return FALSE
/// if the database is (in theory) writable.
pub fn sqlite3PagerIsreadonly(pPager: &mut crate::pager::Pager) -> u8 {
    pPager.readOnly
}

/// C: `int sqlite3PagerRefcount(Pager *pPager)`
///
/// Return the sum of the reference counts for all pages held by pPager.
pub fn sqlite3PagerRefcount(pPager: &mut crate::pager::Pager) -> i32 {
    sqlite3PcacheRefCount(&mut pPager.pPCache) as i32
}

/// C: `void sqlite3PagerWalStat(Pager *pPager, sqlite3_str *pStr)`
pub fn sqlite3PagerWalStat(pPager: &mut crate::pager::Pager, pStr: &mut StrAccum) {
    if let Some(w) = pPager.pWal.as_mut() {
        crate::wal::sqlite3WalStat(w, pStr);
    }
}

/// C: `int sqlite3PagerMemUsed(Pager *pPager)`
///
/// Return the approximate number of bytes of memory currently used by
/// the pager and its associated cache (computed with the sizes of the
/// 64-bit C build).
pub fn sqlite3PagerMemUsed(pPager: &mut crate::pager::Pager) -> i32 {
    let perPageSize = pPager.pageSize as i32 + pPager.nExtra as i32 + (80 + 5 * 8);
    let n = pPager.zFilename.len() as i32;
    let szPager = 304 + 80 + 120 + 2 * 120 + 8 + 4 + 3 * (n + 1) + 8 + 4 + 3;
    perPageSize * sqlite3PcachePagecount(&mut pPager.pPCache) + szPager + pPager.pageSize as i32
}

/// C: `int sqlite3PagerPageRefcount(DbPage *pPage)`
///
/// Return the number of references to the specified page.
pub fn sqlite3PagerPageRefcount(pPage: &DbPage) -> i32 {
    sqlite3PcachePageRefcount(pPage) as i32
}

/// C: `int * sqlite3PagerStats(Pager *pPager)`
///
/// This routine is used for testing and analysis only.  Signature
/// change: returns the 11 statistics.
pub fn sqlite3PagerStats(pPager: &mut crate::pager::Pager) -> [i32; 11] {
    let mut a = [0i32; 11];
    a[0] = sqlite3PcacheRefCount(&mut pPager.pPCache) as i32;
    a[1] = sqlite3PcachePagecount(&mut pPager.pPCache);
    a[2] = sqlite3PcacheGetCachesize(&mut pPager.pPCache);
    a[3] = if pPager.eState == PAGER_OPEN {
        -1
    } else {
        pPager.dbSize as i32
    };
    a[4] = pPager.eState as i32;
    a[5] = pPager.errCode;
    a[6] = (pPager.aStat[PAGER_STAT_HIT] & 0x7fffffff) as i32;
    a[7] = (pPager.aStat[PAGER_STAT_MISS] & 0x7fffffff) as i32;
    a[8] = 0; // Used to be pPager->nOvfl
    a[9] = pPager.nRead;
    a[10] = (pPager.aStat[PAGER_STAT_WRITE] & 0x7fffffff) as i32;
    a
}

/// C: `void sqlite3PagerCacheStat(Pager *pPager, int eStat, int reset, u64 *pnVal)`
///
/// Parameter eStat must be one of SQLITE_DBSTATUS_CACHE_HIT, _MISS,
/// _WRITE, or _CACHE_WRITE+1.  The SQLITE_DBSTATUS_CACHE_WRITE+1 case is
/// a translation of SQLITE_DBSTATUS_CACHE_SPILL.  The _SPILL case is not
/// contiguous because it was added later.
///
/// Before returning, *pnVal is incremented by the current value of the
/// specified cache statistic. If the resetFlag is true, then the counter
/// is reset to zero.
pub fn sqlite3PagerCacheStat(
    pPager: &mut crate::pager::Pager,
    eStat: i32,
    reset: i32,
    pnVal: &mut u64,
) {
    debug_assert!(
        eStat == SQLITE_DBSTATUS_CACHE_HIT
            || eStat == SQLITE_DBSTATUS_CACHE_MISS
            || eStat == SQLITE_DBSTATUS_CACHE_WRITE
            || eStat == SQLITE_DBSTATUS_CACHE_WRITE + 1
    );
    let e = (eStat - SQLITE_DBSTATUS_CACHE_HIT) as usize;
    *pnVal = pnVal.wrapping_add(pPager.aStat[e] as u64);
    if reset != 0 {
        pPager.aStat[e] = 0;
    }
}

/// C: `int sqlite3PagerIsMemdb(Pager *pPager)`
///
/// Return true if this is an in-memory or temp-file backed pager.
pub fn sqlite3PagerIsMemdb(pPager: &mut crate::pager::Pager) -> i32 {
    (pPager.tempFile != 0 || pPager.memVfs != 0) as i32
}

/// Check that there are at least nSavepoint savepoints open. If there are
/// currently less than nSavepoints open, then open one or more savepoints
/// to make up the difference. If the number of savepoints is already
/// equal to nSavepoint, then this function is a no-op.
///
/// If a memory allocation fails, SQLITE_NOMEM is returned. If an error
/// occurs while opening the sub-journal file, then an IO error code is
/// returned. Otherwise, SQLITE_OK.
fn pagerOpenSavepoint(pPager: &mut Pager, nSavepoint: i32) -> i32 {
    let rc = SQLITE_OK; // Return code
    let nCurrent = pPager.nSavepoint; // Current number of savepoints

    debug_assert!(pPager.eState >= PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(pPager));
    debug_assert!(nSavepoint > nCurrent && pPager.useJournal != 0);

    // Grow the Pager.aSavepoint array using realloc(). Return SQLITE_NOMEM
    // if the allocation fails. Otherwise, zero the new portion in case a
    // malloc failure occurs while populating it in the for(...) loop
    // below.
    pPager.aSavepoint.truncate(nCurrent as usize);
    pPager
        .aSavepoint
        .resize_with(nSavepoint as usize, PagerSavepoint::default);

    // Populate the PagerSavepoint structures just allocated.
    for ii in nCurrent as usize..nSavepoint as usize {
        let iOffset = if isOpen(&pPager.jfd) && pPager.journalOff > 0 {
            pPager.journalOff
        } else {
            JOURNAL_HDR_SZ(pPager)
        };
        let dbSize = pPager.dbSize;
        let nSubRec = pPager.nSubRec;
        {
            let a = &mut pPager.aSavepoint[ii];
            a.nOrig = dbSize;
            a.iOffset = iOffset;
            a.iSubRec = nSubRec;
            a.pInSavepoint = sqlite3BitvecCreate(dbSize);
            a.bTruncateOnRelease = 1;
        }
        if let Some(w) = pPager.pWal.as_mut() {
            crate::wal::sqlite3WalSavepoint(w, &mut pPager.aSavepoint[ii].aWalData);
        }
        pPager.nSavepoint = ii as i32 + 1;
    }
    debug_assert!(pPager.nSavepoint == nSavepoint);
    rc
}

/// C: `int sqlite3PagerOpenSavepoint(Pager *pPager, int nSavepoint)`
pub fn sqlite3PagerOpenSavepoint(pPager: &mut crate::pager::Pager, nSavepoint: i32) -> i32 {
    debug_assert!(pPager.eState >= PAGER_WRITER_LOCKED);
    debug_assert!(assert_pager_state(pPager));

    if nSavepoint > pPager.nSavepoint && pPager.useJournal != 0 {
        pagerOpenSavepoint(pPager, nSavepoint)
    } else {
        SQLITE_OK
    }
}

/// C: `int sqlite3PagerSavepoint(Pager *pPager, int op, int iSavepoint)`
///
/// This function is called to rollback or release (commit) a savepoint.
/// The savepoint to release or rollback need not be the most recently
/// created savepoint.
///
/// Parameter op is always either SAVEPOINT_ROLLBACK or SAVEPOINT_RELEASE.
/// If it is SAVEPOINT_RELEASE, then release and destroy the savepoint
/// with index iSavepoint. If it is SAVEPOINT_ROLLBACK, then rollback all
/// changes that have occurred since the specified savepoint was created.
///
/// The savepoint to rollback or release is identified by parameter
/// iSavepoint. A value of 0 means to operate on the outermost savepoint
/// (the first created). A value of (Pager.nSavepoint-1) means operate on
/// the most recently created savepoint. If iSavepoint is greater than
/// (Pager.nSavepoint-1), then this function is a no-op.
///
/// If a negative value is passed to this function, then the current
/// transaction is rolled back. This is different to calling
/// sqlite3PagerRollback() because this function does not terminate the
/// transaction or unlock the database, it just restores the contents of
/// the database to its original state.
///
/// In any case, all savepoints with an index greater than iSavepoint are
/// destroyed. If this is a release operation (op==SAVEPOINT_RELEASE),
/// then savepoint iSavepoint is also destroyed.
///
/// This function may return SQLITE_NOMEM if a memory allocation fails,
/// or an IO error code if an IO error occurs while rolling back a
/// savepoint. If no errors occur, SQLITE_OK is returned.
pub fn sqlite3PagerSavepoint(pPager: &mut crate::pager::Pager, op: i32, iSavepoint: i32) -> i32 {
    let mut rc = pPager.errCode;

    debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK);
    debug_assert!(iSavepoint >= 0 || op == SAVEPOINT_ROLLBACK);

    if rc == SQLITE_OK && iSavepoint < pPager.nSavepoint {
        // Figure out how many savepoints will still be active after this
        // operation. Store this value in nNew. Then free resources
        // associated with any savepoints that are destroyed by this
        // operation.
        let nNew = iSavepoint + if op == SAVEPOINT_RELEASE { 0 } else { 1 }; // Number of remaining savepoints after this op.
        for ii in nNew.max(0) as usize..pPager.nSavepoint as usize {
            pPager.aSavepoint[ii].pInSavepoint = None;
        }
        pPager.nSavepoint = nNew;

        // Truncate the sub-journal so that it only includes the parts that
        // are still in use.
        if op == SAVEPOINT_RELEASE {
            let (bTruncateOnRelease, iSubRec) = {
                let pRel = &pPager.aSavepoint[nNew as usize];
                (pRel.bTruncateOnRelease, pRel.iSubRec)
            };
            if bTruncateOnRelease != 0 && isOpen(&pPager.sjfd) {
                // Only truncate if it is an in-memory sub-journal.
                if sqlite3JournalIsInMemory(&pPager.sjfd) != 0 {
                    let sz = (pPager.pageSize + 4) * iSubRec as i64;
                    rc = sqlite3OsTruncate(&mut pPager.sjfd, sz);
                    debug_assert!(rc == SQLITE_OK);
                }
                pPager.nSubRec = iSubRec;
            }
        }
        // Else this is a rollback operation, playback the specified
        // savepoint.  If this is a temp-file, it is possible that the
        // journal file has not yet been opened. In this case there have
        // been no changes to the database file, so the playback operation
        // can be skipped.
        else if pagerUseWal(pPager) || isOpen(&pPager.jfd) {
            let pSavepoint = if nNew == 0 {
                None
            } else {
                Some(nNew as usize - 1)
            };
            rc = pagerPlaybackSavepoint(pPager, pSavepoint);
            debug_assert!(rc != SQLITE_DONE);
        }
    }
    rc
}

/// C: `const char * sqlite3PagerFilename(const Pager *pPager, int nullIfMemDb)`
///
/// Return the full pathname of the database file.
///
/// Except, if the pager is in-memory only, then return an empty string if
/// nullIfMemDb is true.  This routine is called with nullIfMemDb==1 when
/// used to report the filename to the user, for compatibility with
/// legacy behavior.  But when the Btree needs to know the filename for
/// matching to shared cache, it uses nullIfMemDb==0 so that in-memory
/// databases can participate in shared-cache.
///
/// The return value is the name only (no URI parameters); see
/// sqlite3PagerFilenameUri() for the C buffer with the parameters.
pub fn sqlite3PagerFilename(pPager: &crate::pager::Pager, nullIfMemDb: i32) -> Option<ZStr> {
    if nullIfMemDb != 0 && (pPager.memDb != 0 || pPager.pVfs.zName == b"memdb") {
        Some(ZStr::new())
    } else {
        Some(zpath_name(&pPager.zFilename).to_vec())
    }
}

/// The database file name in the C buffer layout (name, then NUL and the
/// URI parameter block if any), usable with sqlite3_uri_parameter().
pub fn sqlite3PagerFilenameUri(pPager: &crate::pager::Pager) -> ZStr {
    pPager.zFilename.clone()
}

/// C: `sqlite3_vfs * sqlite3PagerVfs(Pager *pPager)`
///
/// Return the VFS structure for the pager.
pub fn sqlite3PagerVfs(pPager: &mut crate::pager::Pager) -> Rc<crate::os::Vfs> {
    pPager.pVfs.clone()
}

/// C: `sqlite3_file * sqlite3PagerFile(Pager *pPager)`
///
/// Return the file handle for the database file associated with the
/// pager.  This might return NULL if the file has not yet been opened.
pub fn sqlite3PagerFile(pPager: &mut crate::pager::Pager) -> &mut crate::os::SqliteFile {
    &mut pPager.fd
}

/// C: `sqlite3_file * sqlite3PagerJrnlFile(Pager *pPager)`
///
/// Return the file handle for the journal file (if it exists).
/// This will be either the rollback journal or the WAL file.
pub fn sqlite3PagerJrnlFile(pPager: &mut crate::pager::Pager) -> &mut crate::os::SqliteFile {
    match pPager.pWal.as_mut() {
        Some(w) => crate::wal::sqlite3WalFile(w),
        None => &mut pPager.jfd,
    }
}

/// C: `const char * sqlite3PagerJournalname(Pager *pPager)`
///
/// Return the full pathname of the journal file.
pub fn sqlite3PagerJournalname(pPager: &mut crate::pager::Pager) -> Option<ZStr> {
    pPager.zJournal.clone()
}

/// C: `int sqlite3PagerMovepage(Pager *pPager, DbPage *pPg, Pgno pgno, int isCommit)`
///
/// Move the page pPg to location pgno in the file.
///
/// There must be no references to the page previously located at pgno
/// (which we call pPgOld) though that page is allowed to be in cache. If
/// the page previously located at pgno is not already in the rollback
/// journal, it is not put there by by this routine.
///
/// References to the page pPg remain valid. Updating any meta-data
/// associated with pPg (i.e. data stored in the nExtra bytes allocated
/// along with the page) is the responsibility of the caller.
///
/// A transaction must be active when this routine is called. It used to
/// be required that a statement transaction was not active, but this
/// restriction has been removed (CREATE INDEX needs to move a page when a
/// statement transaction is active).
///
/// If the fourth argument, isCommit, is non-zero, then this page is being
/// moved as part of a database reorganization just before the
/// transaction is being committed. In this case, it is guaranteed that
/// the database page pPg refers to will not be written to again within
/// this transaction.
///
/// This function may return SQLITE_NOMEM or an IO error code if an error
/// occurs. Otherwise, it returns SQLITE_OK.
pub fn sqlite3PagerMovepage(
    pPager: &mut crate::pager::Pager,
    pPg: &DbPage,
    pgno: Pgno,
    isCommit: i32,
) -> i32 {
    let mut rc;
    let mut needSyncPgno: Pgno = 0; // Old value of pPg->pgno, if sync is required

    debug_assert!(pPg.borrow().nRef > 0);
    debug_assert!(pPager.eState == PAGER_WRITER_CACHEMOD || pPager.eState == PAGER_WRITER_DBMOD);
    debug_assert!(assert_pager_state(pPager));

    // In order to be able to rollback, an in-memory database must journal
    // the page we are moving from.
    debug_assert!(pPager.tempFile != 0 || !MEMDB(pPager));
    if pPager.tempFile != 0 {
        rc = sqlite3PagerWriteWith(pPager, pPg);
        if rc != 0 {
            return rc;
        }
    }

    // If the page being moved is dirty and has not been saved by the
    // latest savepoint, then save the current contents of the page into
    // the sub-journal now. This is required to handle the following
    // scenario:
    //
    //   BEGIN;
    //     <journal page X, then modify it in memory>
    //     SAVEPOINT one;
    //       <Move page X to location Y>
    //     ROLLBACK TO one;
    //
    // If page X were not written to the sub-journal here, it would not be
    // possible to restore its contents when the "ROLLBACK TO one"
    // statement were processed.
    //
    // subjournalPage() may need to allocate space to store pPg->pgno into
    // one or more savepoint bitvecs. This is the reason this function may
    // fail.
    if (pgflags(pPg) & PGHDR_DIRTY) != 0 {
        rc = subjournalPageIfRequired(pPager, pPg);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // If the journal needs to be sync()ed before page pPg->pgno can be
    // written to, store pPg->pgno in local variable needSyncPgno.
    //
    // If the isCommit flag is set, there is no need to remember that the
    // journal needs to be sync()ed before database page pPg->pgno can be
    // written to. The caller has already promised not to write to it.
    if (pgflags(pPg) & PGHDR_NEED_SYNC) != 0 && isCommit == 0 {
        needSyncPgno = pgpgno(pPg);
        debug_assert!((pgflags(pPg) & PGHDR_DIRTY) != 0);
    }

    // If the cache contains a page with page-number pgno, remove it from
    // its hash chain. Also, if the PGHDR_NEED_SYNC flag was set for page
    // pgno, then the journal must be synced before the page is written.
    pPg.borrow_mut().flags &= !(PGHDR_NEED_SYNC as u16);
    let pPgOld = sqlite3PagerLookup(pPager, pgno);
    if let Some(old) = &pPgOld {
        if old.borrow().nRef > 1 {
            pagerUnrefNotNull(pPager, old);
            return SQLITE_CORRUPT;
        }
        let f = pgflags(old) & PGHDR_NEED_SYNC;
        pPg.borrow_mut().flags |= f as u16;
        if pPager.tempFile != 0 {
            // Do not discard pages from an in-memory database since we
            // might need to rollback later.  Just move the page out of the
            // way.
            let n = pPager.dbSize + 1;
            sqlite3PcacheMove(&mut pPager.pPCache, old, n);
        } else {
            sqlite3PcacheDrop(&mut pPager.pPCache, old);
        }
    }

    let origPgno = pgpgno(pPg);
    sqlite3PcacheMove(&mut pPager.pPCache, pPg, pgno);
    sqlite3PcacheMakeDirty(&mut pPager.pPCache, pPg);

    // For an in-memory database, make sure the original page continues to
    // exist, in case the transaction needs to roll back.  Use pPgOld as
    // the page number.
    if pPager.tempFile != 0 {
        if let Some(old) = &pPgOld {
            sqlite3PcacheMove(&mut pPager.pPCache, old, origPgno);
            pagerUnrefNotNull(pPager, old);
        }
    }

    if needSyncPgno != 0 {
        // If needSyncPgno is non-zero, then the journal file needs to be
        // sync()ed before any data is written to database file page
        // needSyncPgno.  Currently, no such page exists in the page-cache
        // and the "is journaled" bitvec flag has been set. This needs to
        // be remedied by loading the page into the pager-cache and setting
        // the PGHDR_NEED_SYNC flag.
        //
        // If the attempt to load the page into the page-cache fails,
        // (due to a malloc() or IO failure), clear the bit in the
        // pInJournal[] array. Otherwise, if the page is loaded and
        // written again in this transaction, it may be written to the
        // database file before it is synced to the journal file. This is
        // not allowed.
        let mut pPgHdr = None;
        rc = sqlite3PagerGet(pPager, needSyncPgno, &mut pPgHdr, 0);
        if rc != SQLITE_OK {
            if needSyncPgno <= pPager.dbOrigSize {
                if let Some(b) = pPager.pInJournal.as_mut() {
                    sqlite3BitvecClear(b, needSyncPgno, &mut []);
                }
            }
            return rc;
        }
        let pPgHdr = pPgHdr.unwrap();
        pPgHdr.borrow_mut().flags |= PGHDR_NEED_SYNC as u16;
        sqlite3PcacheMakeDirty(&mut pPager.pPCache, &pPgHdr);
        pagerUnrefNotNull(pPager, &pPgHdr);
    }

    SQLITE_OK
}

/// C: `void sqlite3PagerRekey(DbPage *pPg, Pgno iNew, u16 flags)`
///
/// The page handle passed as the first argument refers to a dirty page
/// with a page number other than iNew. This function changes the page's
/// page number to iNew and sets the value of the PgHdr.flags field to
/// the value passed as the third parameter.
pub fn sqlite3PagerRekey(pPg: &DbPage, iNew: Pgno, flags: u16) {
    debug_assert!(pPg.borrow().pgno != iNew);
    pPg.borrow_mut().flags = flags;
    if let Some(p) = page_pager(pPg) {
        sqlite3PcacheMove(&mut p.borrow_mut().pPCache, pPg, iNew);
    }
}

/// C: `int sqlite3PagerLockingMode(Pager *pPager, int eMode)`
///
/// Get/set the locking-mode for this pager. Parameter eMode must be one
/// of PAGER_LOCKINGMODE_QUERY, PAGER_LOCKINGMODE_NORMAL or
/// PAGER_LOCKINGMODE_EXCLUSIVE. If the parameter is not _QUERY, then
/// the locking-mode is set to the value specified.
///
/// The returned value is either PAGER_LOCKINGMODE_NORMAL or
/// PAGER_LOCKINGMODE_EXCLUSIVE, indicating the current (possibly updated)
/// locking-mode.
pub fn sqlite3PagerLockingMode(pPager: &mut crate::pager::Pager, eMode: i32) -> i32 {
    debug_assert!(
        eMode == PAGER_LOCKINGMODE_QUERY
            || eMode == PAGER_LOCKINGMODE_NORMAL
            || eMode == PAGER_LOCKINGMODE_EXCLUSIVE
    );
    let heap = match pPager.pWal.as_mut() {
        Some(w) => crate::wal::sqlite3WalHeapMemory(w) != 0,
        None => false,
    };
    if eMode >= 0 && pPager.tempFile == 0 && !heap {
        pPager.exclusiveMode = eMode as u8;
    }
    pPager.exclusiveMode as i32
}

/// C: `int sqlite3PagerSetJournalMode(Pager *pPager, int eMode)`
///
/// Set the journal-mode for this pager. Parameter eMode must be one of:
///
///    PAGER_JOURNALMODE_DELETE
///    PAGER_JOURNALMODE_TRUNCATE
///    PAGER_JOURNALMODE_PERSIST
///    PAGER_JOURNALMODE_OFF
///    PAGER_JOURNALMODE_MEMORY
///    PAGER_JOURNALMODE_WAL
///
/// The journalmode is set to the value specified if the change is
/// allowed.  The change may be disallowed for the following reasons:
///
///   *  An in-memory database can only have its journal_mode set to
///      _OFF or _MEMORY.
///
///   *  Temporary databases cannot have _WAL journalmode.
///
/// The returned indicate the current (possibly updated) journal-mode.
pub fn sqlite3PagerSetJournalMode(pPager: &mut crate::pager::Pager, eMode: i32) -> i32 {
    let eOld = pPager.journalMode; // Prior journalmode
    let mut eMode = eMode;

    // The eMode parameter is always valid
    debug_assert!(
        eMode == PAGER_JOURNALMODE_DELETE
            || eMode == PAGER_JOURNALMODE_PERSIST
            || eMode == PAGER_JOURNALMODE_OFF
            || eMode == PAGER_JOURNALMODE_TRUNCATE
            || eMode == PAGER_JOURNALMODE_MEMORY
            || eMode == PAGER_JOURNALMODE_WAL
    );

    // This routine is only called from the OP_JournalMode opcode, and the
    // logic there will never allow a temporary file to be changed to WAL
    // mode.
    debug_assert!(pPager.tempFile == 0 || eMode != PAGER_JOURNALMODE_WAL);

    // Do allow the journalmode of an in-memory database to be set to
    // anything other than MEMORY or OFF
    if MEMDB(pPager) {
        debug_assert!(
            eOld as i32 == PAGER_JOURNALMODE_MEMORY || eOld as i32 == PAGER_JOURNALMODE_OFF
        );
        if eMode != PAGER_JOURNALMODE_MEMORY && eMode != PAGER_JOURNALMODE_OFF {
            eMode = eOld as i32;
        }
    }

    if eMode != eOld as i32 {
        // Change the journal mode.
        debug_assert!(pPager.eState != PAGER_ERROR);
        pPager.journalMode = eMode as u8;

        // When transitioning from TRUNCATE or PERSIST to any other journal
        // mode except WAL, unless the pager is in locking_mode=exclusive
        // mode, delete the journal file.
        debug_assert!(isOpen(&pPager.fd) || pPager.exclusiveMode != 0);
        if pPager.exclusiveMode == 0 && (eOld & 5) == 1 && (eMode & 1) == 0 {
            // In this case we would like to delete the journal file. If it
            // is not possible, then that is not a problem. Deleting the
            // journal file is an optimization.
            //
            // Before deleting the journal file, obtain a RESERVED lock on
            // the database file. This ensures that the journal file is not
            // deleted while it is in use by some other client.
            sqlite3OsClose(&mut pPager.jfd);
            let zJournal = pPager.zJournal.clone().unwrap_or_default();
            if pPager.eLock as i32 >= RESERVED_LOCK {
                sqlite3OsDelete(&pPager.pVfs, &zJournal, 0);
            } else {
                let mut rc = SQLITE_OK;
                let state = pPager.eState;
                debug_assert!(state == PAGER_OPEN || state == PAGER_READER);
                if state == PAGER_OPEN {
                    rc = sqlite3PagerSharedLock(pPager);
                }
                if pPager.eState == PAGER_READER {
                    debug_assert!(rc == SQLITE_OK);
                    rc = pagerLockDb(pPager, RESERVED_LOCK);
                }
                if rc == SQLITE_OK {
                    sqlite3OsDelete(&pPager.pVfs, &zJournal, 0);
                }
                if rc == SQLITE_OK && state == PAGER_READER {
                    pagerUnlockDb(pPager, SHARED_LOCK);
                } else if state == PAGER_OPEN {
                    pager_unlock(pPager);
                }
                debug_assert!(state == pPager.eState);
            }
        } else if eMode == PAGER_JOURNALMODE_OFF || eMode == PAGER_JOURNALMODE_MEMORY {
            sqlite3OsClose(&mut pPager.jfd);
        }
    }

    // Return the new journal mode
    pPager.journalMode as i32
}

/// C: `int sqlite3PagerGetJournalMode(Pager *pPager)`
///
/// Return the current journal mode.
pub fn sqlite3PagerGetJournalMode(pPager: &mut crate::pager::Pager) -> i32 {
    pPager.journalMode as i32
}

/// C: `int sqlite3PagerOkToChangeJournalMode(Pager *pPager)`
///
/// Return TRUE if the pager is in a state where it is OK to change the
/// journalmode.  Journalmode changes can only happen when the database
/// is unmodified.
pub fn sqlite3PagerOkToChangeJournalMode(pPager: &mut crate::pager::Pager) -> i32 {
    debug_assert!(assert_pager_state(pPager));
    if pPager.eState >= PAGER_WRITER_CACHEMOD {
        return 0;
    }
    if isOpen(&pPager.jfd) && pPager.journalOff > 0 {
        return 0;
    }
    1
}

/// C: `i64 sqlite3PagerJournalSizeLimit(Pager *pPager, i64 iLimit)`
///
/// Get/set the size-limit used for persistent journal files.
///
/// Setting the size limit to -1 means no limit is enforced.  An attempt
/// to set a limit smaller than -1 is a no-op.
pub fn sqlite3PagerJournalSizeLimit(pPager: &mut crate::pager::Pager, iLimit: i64) -> i64 {
    if iLimit >= -1 {
        pPager.journalSizeLimit = iLimit;
        if let Some(w) = pPager.pWal.as_mut() {
            crate::wal::sqlite3WalLimit(w, iLimit);
        }
    }
    pPager.journalSizeLimit
}

/// C: `sqlite3_backup ** sqlite3PagerBackupPtr(Pager *pPager)`
///
/// Return a pointer to the pPager->pBackup variable. The backup module in
/// backup.c maintains the content of this variable. This module uses it
/// opaquely as an argument to sqlite3BackupRestart() and
/// sqlite3BackupUpdate() only.  (Rust: the list of registered backups;
/// C prepends new backups to the list.)
pub fn sqlite3PagerBackupPtr(pPager: &mut crate::pager::Pager) -> &mut BackupList {
    &mut pPager.pBackup
}

/// C: `void sqlite3PagerClearCache(Pager *pPager)`
///
/// Unless this is an in-memory or temporary database, clear the pager
/// cache.
pub fn sqlite3PagerClearCache(pPager: &mut crate::pager::Pager) {
    debug_assert!(!MEMDB(pPager) || pPager.tempFile != 0);
    if pPager.tempFile == 0 {
        pager_reset(pPager);
    }
}

/// C: `int sqlite3PagerCheckpoint( Pager *pPager, sqlite3 *db, int eMode, int *pnLog, int *pnCkpt )`
///
/// This function is called when the user invokes "PRAGMA wal_checkpoint",
/// "PRAGMA wal_blocking_checkpoint" or calls the
/// sqlite3_wal_checkpoint() or sqlite3_wal_checkpoint_v2() functions.
///
/// Parameter eMode is one of SQLITE_CHECKPOINT_PASSIVE, FULL, RESTART
/// or TRUNCATE.
pub fn sqlite3PagerCheckpoint(
    db: &mut Sqlite3,
    pPager: &mut crate::pager::Pager,
    eMode: i32,
    pnLog: &mut i32,
    pnCkpt: &mut i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    if pPager.pWal.is_none() && pPager.journalMode as i32 == PAGER_JOURNALMODE_WAL {
        // This only happens when a database file is zero bytes in size
        // opened and then "PRAGMA journal_mode=WAL" is run and then
        // sqlite3_wal_checkpoint() is called.  In that case, the
        // sqlite3_exec() below forces the WAL file into existence.
        let mut zErr = None;
        crate::legacy::sqlite3_exec(db, Some(b"PRAGMA table_list"), None, &mut zErr);
    }
    if pPager.pWal.is_some() {
        let xBusy = if eMode <= SQLITE_CHECKPOINT_PASSIVE {
            None
        } else {
            pPager.xBusyHandler.clone()
        };
        let walSyncFlags = pPager.walSyncFlags as i32;
        let pageSize = pPager.pageSize as i32;
        let mut tmp = pPager.pTmpSpace.clone();
        rc = crate::wal::sqlite3WalCheckpoint(
            db,
            pPager.pWal.as_mut().unwrap(),
            eMode,
            xBusy,
            walSyncFlags,
            pageSize,
            &mut tmp,
            pnLog,
            pnCkpt,
        );
    }
    rc
}

/// C: `int sqlite3PagerWalCallback(Pager *pPager)`
pub fn sqlite3PagerWalCallback(pPager: &mut crate::pager::Pager) -> i32 {
    match pPager.pWal.as_mut() {
        Some(w) => crate::wal::sqlite3WalCallback(w),
        None => 0,
    }
}

/// C: `int sqlite3PagerWalSupported(Pager *pPager)`
///
/// Return true if the underlying VFS for the given pager supports the
/// primitives necessary for write-ahead logging.  The VFS of this port
/// has no shared memory and the heap-memory WAL is not ported, so this
/// always returns 0 (journal_mode=WAL falls back like with a VFS without
/// xShmMap).
pub fn sqlite3PagerWalSupported(pPager: &mut crate::pager::Pager) -> i32 {
    let _ = pPager;
    0
}

/// Attempt to take an exclusive lock on the database file. If a PENDING
/// lock is obtained instead, immediately release it.
fn pagerExclusiveLock(pPager: &mut Pager) -> i32 {
    debug_assert!(pPager.eLock as i32 >= SHARED_LOCK);
    let eOrigLock = pPager.eLock; // Original lock
    let rc = pagerLockDb(pPager, EXCLUSIVE_LOCK);
    if rc != SQLITE_OK {
        // If the attempt to grab the exclusive lock failed, release the
        // pending lock that may have been obtained instead.
        pagerUnlockDb(pPager, eOrigLock as i32);
    }
    rc
}

/// Call sqlite3WalOpen() to open the WAL handle. If the pager is in
/// exclusive-locking mode when this function is called, take an
/// EXCLUSIVE lock on the database file and use heap-memory to store the
/// wal-index instead of the VFS shared-memory implementation.
///
/// If an error occurs, an error code is returned and the pager is left
/// in the same state. Otherwise, SQLITE_OK is returned.
fn pagerOpenWal(pPager: &mut Pager) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pPager.pWal.is_none() && pPager.tempFile == 0);
    debug_assert!(pPager.eLock as i32 == SHARED_LOCK || pPager.eLock as i32 == EXCLUSIVE_LOCK);

    // If the pager is already in exclusive-mode, the WAL module will use
    // heap-memory for the wal-index instead of the VFS shared-memory
    // implementation. Take the exclusive lock now, before opening the WAL
    // file, to make sure this is safe.
    if pPager.exclusiveMode != 0 {
        rc = pagerExclusiveLock(pPager);
    }

    // Open the connection to the log file. If this operation fails, (e.g.
    // due to malloc() failure), return an error code.
    if rc == SQLITE_OK {
        let vfs = pPager.pVfs.clone();
        let zWal = pPager.zWal.clone().unwrap_or_default();
        let excl = pPager.exclusiveMode as i32;
        let lim = pPager.journalSizeLimit;
        rc = crate::wal::sqlite3WalOpen(&vfs, &mut pPager.fd, &zWal, excl, lim, &mut pPager.pWal);
    }
    pagerFixMaplimit(pPager);
    rc
}

/// C: `int sqlite3PagerOpenWal( Pager *pPager, int *pbOpen )`
///
/// The caller must be holding a SHARED lock on the database file to call
/// this function.
///
/// If the pager passed as the first argument is open on a real database
/// file (not a temp file or an in-memory database), and the WAL file is
/// not already open, make an attempt to open it now. If successful,
/// return SQLITE_OK. If an error occurs or the VFS used by the pager
/// does not support the xShmXXX() methods, return an error code. *pbOpen
/// is not modified in either case.
///
/// If the pager is open on a temp-file (or in-memory database), or if the
/// WAL file is already open, set *pbOpen to 1 and return SQLITE_OK
/// without doing anything.
///
/// Signature change: `pbOpen: Option<&mut i32>` (C NULL).
pub fn sqlite3PagerOpenWal(pPager: &mut crate::pager::Pager, pbOpen: Option<&mut i32>) -> i32 {
    let mut rc = SQLITE_OK; // Return code

    debug_assert!(assert_pager_state(pPager));
    debug_assert!(pPager.eState == PAGER_OPEN || pbOpen.is_some());
    debug_assert!(pPager.eState == PAGER_READER || pbOpen.is_none());

    if pPager.tempFile == 0 && pPager.pWal.is_none() {
        if sqlite3PagerWalSupported(pPager) == 0 {
            return SQLITE_CANTOPEN;
        }

        // Close any rollback journal previously open
        sqlite3OsClose(&mut pPager.jfd);

        rc = pagerOpenWal(pPager);
        if rc == SQLITE_OK {
            pPager.journalMode = PAGER_JOURNALMODE_WAL as u8;
            pPager.eState = PAGER_OPEN;
        }
    } else if let Some(b) = pbOpen {
        *b = 1;
    }
    rc
}

/// C: `int sqlite3PagerCloseWal(Pager *pPager, sqlite3 *db)`
///
/// This function is called to close the connection to the log file prior
/// to switching from WAL to rollback mode.
///
/// Before closing the log file, this function attempts to take an
/// EXCLUSIVE lock on the database file. If this cannot be obtained, an
/// error (SQLITE_BUSY) is returned and the log connection is not closed.
/// If successful, the EXCLUSIVE lock is not released before returning.
pub fn sqlite3PagerCloseWal(db: &mut Sqlite3, pPager: &mut crate::pager::Pager) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pPager.journalMode as i32 == PAGER_JOURNALMODE_WAL);

    // If the log file is not already open, but does exist in the
    // file-system, it may need to be checkpointed before the connection
    // can switch to rollback mode. Open it now so this can happen.
    if pPager.pWal.is_none() {
        let mut logexists = 0;
        rc = pagerLockDb(pPager, SHARED_LOCK);
        if rc == SQLITE_OK {
            let zWal = pPager.zWal.clone().unwrap_or_default();
            rc = sqlite3OsAccess(&pPager.pVfs, &zWal, SQLITE_ACCESS_EXISTS, &mut logexists);
        }
        if rc == SQLITE_OK && logexists != 0 {
            rc = pagerOpenWal(pPager);
        }
    }

    // Checkpoint and close the log. Because an EXCLUSIVE lock is held on
    // the database file, the log and log-summary files will be deleted.
    if rc == SQLITE_OK && pPager.pWal.is_some() {
        rc = pagerExclusiveLock(pPager);
        if rc == SQLITE_OK {
            let walSyncFlags = pPager.walSyncFlags as i32;
            let pageSize = pPager.pageSize as i32;
            let mut tmp = pPager.pTmpSpace.clone();
            let mut w = pPager.pWal.take().unwrap();
            rc = crate::wal::sqlite3WalClose(db, &mut w, walSyncFlags, pageSize, &mut tmp);
            pagerFixMaplimit(pPager);
            if rc != 0 && pPager.exclusiveMode == 0 {
                pagerUnlockDb(pPager, SHARED_LOCK);
            }
        }
    }
    rc
}

/// C: `int sqlite3PagerWalWriteLock(Pager *pPager, int bLock)`
/// (SQLITE_ENABLE_SETLK_TIMEOUT only)
pub fn sqlite3PagerWalWriteLock(pPager: &mut crate::pager::Pager, bLock: i32) -> i32 {
    let _ = (pPager, bLock);
    SQLITE_OK
}

/// C: `void sqlite3PagerWalDb(Pager *pPager, sqlite3 *db)`
/// (SQLITE_ENABLE_SETLK_TIMEOUT only)
pub fn sqlite3PagerWalDb(db: &mut Sqlite3, pPager: &mut crate::pager::Pager) {
    let _ = (db, pPager);
}

/// C: `int sqlite3PagerSnapshotGet(Pager *pPager, sqlite3_snapshot **ppSnapshot)`
/// (SQLITE_ENABLE_SNAPSHOT is not enabled)
pub fn sqlite3PagerSnapshotGet(
    pPager: &mut crate::pager::Pager,
    ppSnapshot: /*sqlite3_snapshot***/ (),
) -> i32 {
    let _ = (pPager, ppSnapshot);
    SQLITE_ERROR
}

/// C: `int sqlite3PagerSnapshotOpen( Pager *pPager, sqlite3_snapshot *pSnapshot )`
pub fn sqlite3PagerSnapshotOpen(
    pPager: &mut crate::pager::Pager,
    pSnapshot: /*sqlite3_snapshot**/ (),
) -> i32 {
    let _ = (pPager, pSnapshot);
    SQLITE_ERROR
}

/// C: `int sqlite3PagerSnapshotRecover(Pager *pPager)`
pub fn sqlite3PagerSnapshotRecover(pPager: &mut crate::pager::Pager) -> i32 {
    let _ = pPager;
    SQLITE_ERROR
}

/// C: `int sqlite3PagerSnapshotCheck(Pager *pPager, sqlite3_snapshot *pSnapshot)`
pub fn sqlite3PagerSnapshotCheck(
    pPager: &mut crate::pager::Pager,
    pSnapshot: /*sqlite3_snapshot**/ (),
) -> i32 {
    let _ = (pPager, pSnapshot);
    SQLITE_ERROR
}

/// C: `void sqlite3PagerSnapshotUnlock(Pager *pPager)`
pub fn sqlite3PagerSnapshotUnlock(pPager: &mut crate::pager::Pager) {
    let _ = pPager;
}

/// C: `int sqlite3PagerWalFramesize(Pager *pPager)` (ZIPVFS only)
pub fn sqlite3PagerWalFramesize(pPager: &mut crate::pager::Pager) -> i32 {
    debug_assert!(pPager.eState >= PAGER_READER);
    match pPager.pWal.as_mut() {
        Some(w) => crate::wal::sqlite3WalFramesize(w),
        None => 0,
    }
}

/// C: `int sqlite3PagerWalSystemErrno(Pager *pPager)` (SQLITE_USE_SEH only)
pub fn sqlite3PagerWalSystemErrno(pPager: &mut crate::pager::Pager) -> i32 {
    let _ = pPager;
    0
}

/// C: `int sqlite3PagerDirectReadOk(Pager *pPager, Pgno pgno)`
///
/// Return true if it is ok for the btree to read page pgno directly from
/// the database file (SQLITE_DIRECT_OVERFLOW_READ): the file is open,
/// there are no dirty pages in the cache, the page is not in the WAL and
/// the VFS supports sub-page reads.
pub fn sqlite3PagerDirectReadOk(pPager: &mut crate::pager::Pager, pgno: Pgno) -> i32 {
    if !isOpen(&pPager.fd) {
        return 0;
    }
    if sqlite3PCacheIsDirty(&mut pPager.pPCache) != 0 {
        return 0;
    }
    if let Some(w) = pPager.pWal.as_mut() {
        let mut iRead: u32 = 0;
        let _ = crate::wal::sqlite3WalFindFrame(w, pgno, &mut iRead);
        if iRead != 0 {
            return 0;
        }
    }
    if (sqlite3OsDeviceCharacteristics(&mut pPager.fd) & SQLITE_IOCAP_SUBPAGE_READ) == 0 {
        return 0;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    const VFSFLAGS: i32 = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("goro-sqlite-pager-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zpath(p: &std::path::Path) -> Vec<u8> {
        p.as_os_str().as_bytes().to_vec()
    }

    fn open(z: &[u8], flags: i32) -> PagerP {
        let vfs = crate::os_unix::sqlite3UnixVfs();
        let mut pp = None;
        let rc = sqlite3PagerOpen(&vfs, &mut pp, Some(z), 136, flags, VFSFLAGS, None);
        assert_eq!(rc, SQLITE_OK);
        let p = pp.unwrap();
        let mut sz = 1024u32;
        assert_eq!(
            sqlite3PagerSetPagesize(&mut p.borrow_mut(), &mut sz, -1),
            SQLITE_OK
        );
        p
    }

    fn get(p: &PagerP, pgno: Pgno) -> DbPage {
        let mut pg = None;
        let rc = sqlite3PagerGet(&mut p.borrow_mut(), pgno, &mut pg, 0);
        assert_eq!(rc, SQLITE_OK, "get page {}", pgno);
        pg.unwrap()
    }

    /// Fill page with a recognizable pattern (keeping the header bytes of
    /// page 1 that the pager maintains).
    fn fill(pg: &DbPage, v: u8) {
        assert_eq!(sqlite3PagerWrite(pg), SQLITE_OK);
        let mut b = pg.borrow_mut();
        let start = if b.pgno == 1 { 100 } else { 0 };
        for x in b.aData[start..].iter_mut() {
            *x = v;
        }
    }

    fn page_byte(pg: &DbPage) -> u8 {
        pg.borrow().aData[200]
    }

    /// Run one write transaction setting pages 1..=n to value v.
    fn write_txn(p: &PagerP, n: Pgno, v: u8, commit: bool) {
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let p1 = get(p, 1);
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0), SQLITE_OK);
        fill(&p1, v);
        for i in 2..=n {
            let pg = get(p, i);
            fill(&pg, v);
            sqlite3PagerUnref(&pg);
        }
        if commit {
            assert_eq!(
                sqlite3PagerCommitPhaseOne(&mut p.borrow_mut(), None, 0),
                SQLITE_OK
            );
            assert_eq!(sqlite3PagerCommitPhaseTwo(&mut p.borrow_mut()), SQLITE_OK);
        } else {
            assert_eq!(sqlite3PagerRollback(&mut p.borrow_mut()), SQLITE_OK);
        }
        sqlite3PagerUnrefPageOne(&p1);
    }

    fn read_txn_check(p: &PagerP, n: Pgno, v: u8) {
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let mut cnt = 0;
        sqlite3PagerPagecount(&mut p.borrow_mut(), &mut cnt);
        assert_eq!(cnt as Pgno, n);
        let p1 = get(p, 1);
        for i in 2..=n {
            let pg = get(p, i);
            assert_eq!(page_byte(&pg), v, "page {}", i);
            sqlite3PagerUnref(&pg);
        }
        assert_eq!(page_byte(&p1), v);
        sqlite3PagerUnrefPageOne(&p1);
    }

    fn file_byte(path: &std::path::Path, pgno: u64) -> u8 {
        let d = std::fs::read(path).unwrap();
        d[((pgno - 1) * 1024 + 200) as usize]
    }

    fn close(p: PagerP) {
        sqlite3PagerClose(None, &mut p.borrow_mut());
    }

    #[test]
    fn pager_commit_rollback_reopen() {
        let d = tmpdir("crr");
        let path = d.join("a.db");
        let z = zpath(&path);
        let p = open(&z, 0);
        write_txn(&p, 5, 0x11, true);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 5 * 1024);
        assert_eq!(file_byte(&path, 3), 0x11);
        // The journal is deleted after commit (journal_mode=DELETE).
        assert!(!d.join("a.db-journal").exists());
        // The change counter was incremented.
        let f = std::fs::read(&path).unwrap();
        assert_eq!(&f[24..28], &[0, 0, 0, 1]);
        assert_eq!(&f[92..96], &[0, 0, 0, 1]);

        // Rollback leaves the file unchanged.
        write_txn(&p, 8, 0x22, false);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 5 * 1024);
        read_txn_check(&p, 5, 0x11);

        // Second commit grows the file
        write_txn(&p, 7, 0x33, true);
        close(p);
        let p = open(&z, 0);
        read_txn_check(&p, 7, 0x33);
        let f = std::fs::read(&path).unwrap();
        assert_eq!(&f[24..28], &[0, 0, 0, 2]);
        close(p);
    }

    #[test]
    fn pager_hot_journal_recovery() {
        let d = tmpdir("hot");
        let path = d.join("b.db");
        let z = zpath(&path);
        let p = open(&z, 0);
        write_txn(&p, 4, 0x44, true);

        // Start a transaction that rewrites the pages and adds two, and run
        // commit phase one: the database file is modified and the journal
        // is still in place.  Copy both files: this is the state after a
        // crash before phase two.
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let p1 = get(&p, 1);
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0), SQLITE_OK);
        fill(&p1, 0x55);
        for i in 2..=6 {
            let pg = get(&p, i);
            fill(&pg, 0x55);
            sqlite3PagerUnref(&pg);
        }
        assert_eq!(
            sqlite3PagerCommitPhaseOne(&mut p.borrow_mut(), None, 0),
            SQLITE_OK
        );
        assert_eq!(file_byte(&path, 2), 0x55);
        let jpath = d.join("b.db-journal");
        assert!(jpath.exists());
        let path2 = d.join("c.db");
        std::fs::copy(&path, &path2).unwrap();
        std::fs::copy(&jpath, d.join("c.db-journal")).unwrap();
        assert_eq!(sqlite3PagerCommitPhaseTwo(&mut p.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&p1);
        close(p);

        // Opening the copy plays the hot journal back.
        let p2 = open(&zpath(&path2), 0);
        read_txn_check(&p2, 4, 0x44);
        assert!(!d.join("c.db-journal").exists());
        assert_eq!(std::fs::metadata(&path2).unwrap().len(), 4 * 1024);
        assert_eq!(file_byte(&path2, 3), 0x44);
        close(p2);

        // The original was committed.
        let p = open(&z, 0);
        read_txn_check(&p, 6, 0x55);
        close(p);
    }

    #[test]
    fn pager_savepoints() {
        let d = tmpdir("sp");
        let path = d.join("s.db");
        let p = open(&zpath(&path), 0);
        write_txn(&p, 3, 0x01, true);

        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let p1 = get(&p, 1);
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0), SQLITE_OK);
        fill(&p1, 0x02);
        let pg2 = get(&p, 2);
        fill(&pg2, 0x02);
        assert_eq!(sqlite3PagerOpenSavepoint(&mut p.borrow_mut(), 1), SQLITE_OK);
        fill(&pg2, 0x03);
        let pg3 = get(&p, 3);
        fill(&pg3, 0x03);
        let pg4 = get(&p, 4);
        fill(&pg4, 0x03);
        sqlite3PagerUnref(&pg4);
        assert_eq!(sqlite3PagerOpenSavepoint(&mut p.borrow_mut(), 2), SQLITE_OK);
        fill(&pg3, 0x04);
        // Roll back the inner savepoint
        assert_eq!(
            sqlite3PagerSavepoint(&mut p.borrow_mut(), SAVEPOINT_ROLLBACK, 1),
            SQLITE_OK
        );
        assert_eq!(page_byte(&pg3), 0x03);
        // Roll back the outer savepoint: page 4 is gone, pages 2,3 restored
        assert_eq!(
            sqlite3PagerSavepoint(&mut p.borrow_mut(), SAVEPOINT_ROLLBACK, 0),
            SQLITE_OK
        );
        assert_eq!(page_byte(&pg2), 0x02);
        assert_eq!(page_byte(&pg3), 0x01);
        let mut n = 0;
        sqlite3PagerPagecount(&mut p.borrow_mut(), &mut n);
        assert_eq!(n, 3);
        assert_eq!(
            sqlite3PagerSavepoint(&mut p.borrow_mut(), SAVEPOINT_RELEASE, 0),
            SQLITE_OK
        );
        sqlite3PagerUnref(&pg2);
        sqlite3PagerUnref(&pg3);
        assert_eq!(
            sqlite3PagerCommitPhaseOne(&mut p.borrow_mut(), None, 0),
            SQLITE_OK
        );
        assert_eq!(sqlite3PagerCommitPhaseTwo(&mut p.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&p1);
        close(p);
        assert_eq!(file_byte(&path, 2), 0x02);
        assert_eq!(file_byte(&path, 3), 0x01);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 3 * 1024);
    }

    #[test]
    fn pager_cache_spill() {
        let d = tmpdir("spill");
        let path = d.join("sp.db");
        let p = open(&zpath(&path), 0);
        sqlite3PagerSetCachesize(&mut p.borrow_mut(), 10);
        write_txn(&p, 200, 0x66, true);
        // A large transaction spills: the pager wrote pages before commit.
        assert!(p.borrow().aStat[PAGER_STAT_SPILL] > 0);
        read_txn_check(&p, 200, 0x66);
        // Rollback of a spilled transaction restores the file.
        write_txn(&p, 300, 0x77, false);
        read_txn_check(&p, 200, 0x66);
        close(p);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 200 * 1024);
        assert_eq!(file_byte(&path, 150), 0x66);
    }

    #[test]
    fn pager_journal_modes() {
        let d = tmpdir("jm");
        for (mode, exists) in [
            (PAGER_JOURNALMODE_PERSIST, true),
            (PAGER_JOURNALMODE_TRUNCATE, true),
            (PAGER_JOURNALMODE_MEMORY, false),
            (PAGER_JOURNALMODE_OFF, false),
            (PAGER_JOURNALMODE_DELETE, false),
        ] {
            let path = d.join(format!("m{}.db", mode));
            let p = open(&zpath(&path), 0);
            assert_eq!(sqlite3PagerSetJournalMode(&mut p.borrow_mut(), mode), mode);
            write_txn(&p, 3, 0x10, true);
            write_txn(&p, 4, 0x20, true);
            if mode != PAGER_JOURNALMODE_OFF {
                write_txn(&p, 5, 0x30, false);
            }
            read_txn_check(&p, 4, 0x20);
            close(p);
            let j = d.join(format!("m{}.db-journal", mode));
            assert_eq!(j.exists(), exists, "mode {}", mode);
            if mode == PAGER_JOURNALMODE_TRUNCATE {
                assert_eq!(std::fs::metadata(&j).unwrap().len(), 0);
            }
            if mode == PAGER_JOURNALMODE_PERSIST {
                // The header is zeroed so the journal is not hot.
                let f = std::fs::read(&j).unwrap();
                assert!(f[..28].iter().all(|&b| b == 0));
            }
            let p = open(&zpath(&path), 0);
            read_txn_check(&p, 4, 0x20);
            close(p);
        }
    }

    #[test]
    fn pager_memdb_and_temp() {
        let vfs = crate::os_unix::sqlite3UnixVfs();
        for (z, flags) in [(&b":memory:"[..], PAGER_MEMORY), (&b""[..], 0)] {
            let mut pp = None;
            let vf = (VFSFLAGS & !SQLITE_OPEN_MAIN_DB) | SQLITE_OPEN_TEMP_DB;
            let rc = sqlite3PagerOpen(&vfs, &mut pp, Some(z), 136, flags, vf, None);
            assert_eq!(rc, SQLITE_OK);
            let p = pp.unwrap();
            sqlite3PagerSetCachesize(&mut p.borrow_mut(), 5);
            write_txn(&p, 50, 0x5a, true);
            read_txn_check(&p, 50, 0x5a);
            write_txn(&p, 60, 0x6b, false);
            read_txn_check(&p, 50, 0x5a);
            close(p);
        }
    }

    #[test]
    fn pager_locking_between_pagers() {
        let d = tmpdir("lock");
        let path = d.join("l.db");
        let z = zpath(&path);
        let a = open(&z, 0);
        let b = open(&z, 0);
        write_txn(&a, 2, 0x01, true);
        // a holds an exclusive write transaction
        assert_eq!(sqlite3PagerSharedLock(&mut a.borrow_mut()), SQLITE_OK);
        let a1 = get(&a, 1);
        assert_eq!(sqlite3PagerBegin(&mut a.borrow_mut(), 1, 0), SQLITE_OK);
        fill(&a1, 0x09);
        // b cannot get a shared lock while a holds EXCLUSIVE
        assert_eq!(sqlite3PagerSharedLock(&mut b.borrow_mut()), SQLITE_BUSY);
        assert_eq!(sqlite3PagerRollback(&mut a.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&a1);
        read_txn_check(&b, 2, 0x01);
        close(a);
        close(b);
    }
}

/// Interoperability with the C library (needs the reference CLI: set
/// SQLITE3REF to its path; run with --ignored).
#[cfg(test)]
mod interop_tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    fn sqlite3ref() -> Option<String> {
        std::env::var("SQLITE3REF").ok()
    }

    fn run(cli: &str, db: &std::path::Path, sql: &str) -> (bool, String) {
        let o = Command::new(cli).arg(db).arg(sql).output().unwrap();
        let mut s = String::from_utf8_lossy(&o.stdout).to_string();
        s.push_str(&String::from_utf8_lossy(&o.stderr));
        (o.status.success(), s.trim().to_string())
    }

    fn open(z: &[u8]) -> PagerP {
        let vfs = crate::os_unix::sqlite3UnixVfs();
        let mut pp = None;
        let vf = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB;
        assert_eq!(
            sqlite3PagerOpen(&vfs, &mut pp, Some(z), 136, 0, vf, None),
            SQLITE_OK
        );
        pp.unwrap()
    }

    fn setup(name: &str) -> (String, std::path::PathBuf) {
        let cli = sqlite3ref().expect("SQLITE3REF");
        let d = std::env::temp_dir().join(format!(
            "goro-sqlite-interop-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("x.db");
        let (ok, out) = run(
            &cli,
            &db,
            "pragma page_size=1024; create table t(a,b); \
             with recursive c(i) as (select 1 union all select i+1 from c where i<40) \
             insert into t select i, randomblob(600) from c;",
        );
        assert!(ok, "{}", out);
        (cli, d)
    }

    #[test]
    #[ignore]
    fn interop_c_recovers_rust_hot_journal() {
        let (cli, d) = setup("c-recovers");
        let db = d.join("x.db");
        let before = run(&cli, &db, "select count(*), total(a), hex(substr(group_concat(hex(b)),1,64)), sum(length(b)) from t").1;
        let p = open(db.as_os_str().as_bytes());
        let mut sz = 1024u32;
        sqlite3PagerSetPagesize(&mut p.borrow_mut(), &mut sz, -1);
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let mut n = 0;
        sqlite3PagerPagecount(&mut p.borrow_mut(), &mut n);
        assert!(n > 20);
        let mut p1 = None;
        assert_eq!(
            sqlite3PagerGet(&mut p.borrow_mut(), 1, &mut p1, 0),
            SQLITE_OK
        );
        let p1 = p1.unwrap();
        assert_eq!(&p1.borrow().aData[..15], b"SQLite format 3");
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0), SQLITE_OK);
        for i in 2..=n as Pgno {
            let mut pg = None;
            assert_eq!(
                sqlite3PagerGet(&mut p.borrow_mut(), i, &mut pg, 0),
                SQLITE_OK
            );
            let pg = pg.unwrap();
            assert_eq!(sqlite3PagerWrite(&pg), SQLITE_OK);
            for b in pg.borrow_mut().aData.iter_mut() {
                *b = 0xEE;
            }
            sqlite3PagerUnref(&pg);
        }
        // While we hold RESERVED/EXCLUSIVE, the C library sees the lock.
        let (ok, out) = run(&cli, &db, "insert into t values(0,0)");
        assert!(!ok && out.contains("locked"), "{}", out);
        assert_eq!(
            sqlite3PagerCommitPhaseOne(&mut p.borrow_mut(), None, 0),
            SQLITE_OK
        );
        let (ok, out) = run(&cli, &db, "select count(*) from t");
        assert!(!ok && out.contains("locked"), "{}", out);
        let y = d.join("y.db");
        std::fs::copy(&db, &y).unwrap();
        std::fs::copy(d.join("x.db-journal"), d.join("y.db-journal")).unwrap();
        assert_eq!(sqlite3PagerRollback(&mut p.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&p1);
        sqlite3PagerClose(None, &mut p.borrow_mut());
        // The rollback restored the original
        let (ok, out) = run(&cli, &db, "pragma integrity_check");
        assert!(ok && out == "ok", "{}", out);
        // The C library recovers the crashed copy from our journal.
        let (ok, out) = run(&cli, &y, "pragma integrity_check");
        assert!(ok && out == "ok", "{}", out);
        assert!(!d.join("y.db-journal").exists());
        let after = run(&cli, &y, "select count(*), total(a), hex(substr(group_concat(hex(b)),1,64)), sum(length(b)) from t").1;
        assert_eq!(before, after);
    }

    #[test]
    #[ignore]
    fn interop_rust_recovers_c_hot_journal() {
        let (cli, d) = setup("rust-recovers");
        let db = d.join("x.db");
        let orig = std::fs::read(&db).unwrap();
        let z = d.join("z.db");
        // Make the C library spill a transaction to the database file and
        // copy the crash state (database + hot journal) mid-transaction.
        let sql = format!(
            "pragma cache_size=2;\nbegin;\nupdate t set b=zeroblob(600);\n.system cp {db} {z}; cp {db}-journal {z}-journal\nrollback;\n",
            db = db.display(),
            z = z.display()
        );
        let o = Command::new(&cli)
            .arg(&db)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
                c.wait_with_output()
            })
            .unwrap();
        assert!(o.status.success());
        assert!(d.join("z.db-journal").exists());
        let crashed = std::fs::read(&z).unwrap();
        assert_ne!(crashed, orig);
        let p = open(z.as_os_str().as_bytes());
        let mut sz = 1024u32;
        sqlite3PagerSetPagesize(&mut p.borrow_mut(), &mut sz, -1);
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let mut p1 = None;
        assert_eq!(
            sqlite3PagerGet(&mut p.borrow_mut(), 1, &mut p1, 0),
            SQLITE_OK
        );
        sqlite3PagerUnrefPageOne(&p1.unwrap());
        sqlite3PagerClose(None, &mut p.borrow_mut());
        assert!(!d.join("z.db-journal").exists());
        let recovered = std::fs::read(&z).unwrap();
        assert_eq!(recovered, orig);
        let (ok, out) = run(&cli, &z, "pragma integrity_check");
        assert!(ok && out == "ok", "{}", out);
    }
}

#[cfg(test)]
mod super_tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    fn open(z: &[u8]) -> PagerP {
        let vfs = crate::os_unix::sqlite3UnixVfs();
        let mut pp = None;
        let vf = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB;
        assert_eq!(
            sqlite3PagerOpen(&vfs, &mut pp, Some(z), 136, 0, vf, None),
            SQLITE_OK
        );
        let p = pp.unwrap();
        let mut sz = 1024u32;
        sqlite3PagerSetPagesize(&mut p.borrow_mut(), &mut sz, -1);
        p
    }

    fn txn(p: &PagerP, v: u8, zSuper: Option<&[u8]>, phase_two: bool) -> DbPage {
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let mut p1 = None;
        assert_eq!(
            sqlite3PagerGet(&mut p.borrow_mut(), 1, &mut p1, 0),
            SQLITE_OK
        );
        let p1 = p1.unwrap();
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0), SQLITE_OK);
        for i in 1..=3 {
            let mut pg = None;
            assert_eq!(
                sqlite3PagerGet(&mut p.borrow_mut(), i, &mut pg, 0),
                SQLITE_OK
            );
            let pg = pg.unwrap();
            assert_eq!(sqlite3PagerWrite(&pg), SQLITE_OK);
            for b in pg.borrow_mut().aData[100..].iter_mut() {
                *b = v;
            }
            sqlite3PagerUnref(&pg);
        }
        assert_eq!(
            sqlite3PagerCommitPhaseOne(&mut p.borrow_mut(), zSuper, 0),
            SQLITE_OK
        );
        if phase_two {
            assert_eq!(sqlite3PagerCommitPhaseTwo(&mut p.borrow_mut()), SQLITE_OK);
        }
        p1
    }

    fn byte(path: &std::path::Path) -> u8 {
        std::fs::read(path).unwrap()[2 * 1024 + 200]
    }

    #[test]
    fn pager_super_journal() {
        let d = std::env::temp_dir().join(format!("goro-sqlite-super-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let a = d.join("a.db");
        let p = open(a.as_os_str().as_bytes());
        let p1 = txn(&p, 1, None, true);
        sqlite3PagerUnrefPageOne(&p1);

        let zSuper = d.join("a.db-mj012345978");
        let zSuperB = zSuper.as_os_str().as_bytes().to_vec();
        let p1 = txn(&p, 2, Some(&zSuperB), false);
        // Crash copies: b (super journal missing => committed) and c
        // (super journal present => rolled back).
        for n in ["b", "c"] {
            std::fs::copy(&a, d.join(format!("{}.db", n))).unwrap();
            std::fs::copy(d.join("a.db-journal"), d.join(format!("{}.db-journal", n))).unwrap();
        }
        assert_eq!(sqlite3PagerCommitPhaseTwo(&mut p.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&p1);
        sqlite3PagerClose(None, &mut p.borrow_mut());

        let b = d.join("b.db");
        let pb = open(b.as_os_str().as_bytes());
        assert_eq!(sqlite3PagerSharedLock(&mut pb.borrow_mut()), SQLITE_OK);
        sqlite3PagerClose(None, &mut pb.borrow_mut());
        assert_eq!(byte(&b), 2);
        assert!(!d.join("b.db-journal").exists());

        // The super-journal lists the child journals.
        let c = d.join("c.db");
        let mut sj = d.join("c.db-journal").as_os_str().as_bytes().to_vec();
        sj.push(0);
        std::fs::write(&zSuper, &sj).unwrap();
        let pc = open(c.as_os_str().as_bytes());
        assert_eq!(sqlite3PagerSharedLock(&mut pc.borrow_mut()), SQLITE_OK);
        sqlite3PagerClose(None, &mut pc.borrow_mut());
        assert_eq!(byte(&c), 1);
        assert!(!d.join("c.db-journal").exists());
        // ... and is deleted once all its children are rolled back.
        assert!(!zSuper.exists());
    }
}

#[cfg(test)]
mod interop_lock_tests {
    use super::*;
    use std::io::{BufRead, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Stdio};

    /// The C library holds a lock; the Rust pager must see it.
    #[test]
    #[ignore]
    fn interop_rust_sees_c_locks() {
        let cli = std::env::var("SQLITE3REF").expect("SQLITE3REF");
        let d =
            std::env::temp_dir().join(format!("goro-sqlite-interop-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("x.db");
        let st = Command::new(&cli)
            .arg(&db)
            .arg("pragma page_size=1024; create table t(a); insert into t values(1);")
            .status()
            .unwrap();
        assert!(st.success());

        let vfs = crate::os_unix::sqlite3UnixVfs();
        let mut pp = None;
        let vf = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB;
        let z = db.as_os_str().as_bytes().to_vec();
        assert_eq!(
            sqlite3PagerOpen(&vfs, &mut pp, Some(&z), 136, 0, vf, None),
            SQLITE_OK
        );
        let p = pp.unwrap();
        let mut sz = 1024u32;
        sqlite3PagerSetPagesize(&mut p.borrow_mut(), &mut sz, -1);

        for (stmt, shared_ok, reserved_ok) in [
            ("begin immediate;", true, false),
            ("begin exclusive;", false, false),
        ] {
            let mut c = Command::new(&cli)
                .arg(&db)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = c.stdin.take().unwrap();
            let mut out = std::io::BufReader::new(c.stdout.take().unwrap());
            writeln!(stdin, "{} select 'ready';", stmt).unwrap();
            stdin.flush().unwrap();
            let mut line = String::new();
            out.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "ready");

            let rc = sqlite3PagerSharedLock(&mut p.borrow_mut());
            assert_eq!(rc == SQLITE_OK, shared_ok, "{}", stmt);
            if rc == SQLITE_OK {
                let mut p1 = None;
                assert_eq!(
                    sqlite3PagerGet(&mut p.borrow_mut(), 1, &mut p1, 0),
                    SQLITE_OK
                );
                let p1 = p1.unwrap();
                let rc = sqlite3PagerBegin(&mut p.borrow_mut(), 0, 0);
                assert_eq!(rc == SQLITE_OK, reserved_ok, "{}", stmt);
                assert_eq!(rc, SQLITE_BUSY);
                sqlite3PagerUnrefPageOne(&p1);
            } else {
                assert_eq!(rc, SQLITE_BUSY);
            }
            writeln!(stdin, "commit;").unwrap();
            drop(stdin);
            assert!(c.wait().unwrap().success());
        }
        // Once the C connection is gone, everything works.
        assert_eq!(sqlite3PagerSharedLock(&mut p.borrow_mut()), SQLITE_OK);
        let mut p1 = None;
        assert_eq!(
            sqlite3PagerGet(&mut p.borrow_mut(), 1, &mut p1, 0),
            SQLITE_OK
        );
        let p1 = p1.unwrap();
        assert_eq!(sqlite3PagerBegin(&mut p.borrow_mut(), 1, 0), SQLITE_OK);
        assert_eq!(sqlite3PagerRollback(&mut p.borrow_mut()), SQLITE_OK);
        sqlite3PagerUnrefPageOne(&p1);
        sqlite3PagerClose(None, &mut p.borrow_mut());
    }
}
