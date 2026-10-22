//! Port of wal.c: the write-ahead log.
//!
//! The on-disk formats (the WAL file and the -shm wal-index) are those of
//! C SQLite, so that goro and the C library can share a WAL database,
//! including concurrently from different processes.  See wal.c for the
//! full description of the file formats and of the locking protocol.
//!
//! Porting notes:
//!
//! * The wal-index pages (C `volatile u32 **apWiData`) are
//!   `crate::os::ShmPage`s: reference-counted regions seen as
//!   `[AtomicU32]` (native byte order), either mappings of the -shm file
//!   (shared with other processes) or heap memory (heap-memory mode,
//!   unreliable read-only shm).  `ht_slot` (u16) hash-table entries are
//!   accessed as halves of those words.  C `memcpy()`s of the wal-index
//!   header become word-by-word copies (see `WalIndexHdr::to_words`).
//! * The Wal cannot keep a pointer to the pager's database file (C
//!   `pDbFd`): the shared-memory methods are reached through an
//!   `crate::os::ShmHandle` obtained at open time, and the functions that
//!   do I/O on the database file (open, close, checkpoint) take it as a
//!   parameter.
//! * The WalIterator of the checkpoint copies the page numbers of the
//!   wal-index instead of pointing into it; the sort of WALITER-1 is
//!   replaced by an equivalent one (ascending page numbers, keeping the
//!   most recent frame of each page), which visits the same frames in the
//!   same order.
//! * SQLITE_ENABLE_SNAPSHOT, SQLITE_ENABLE_SETLK_TIMEOUT and SQLITE_USE_SEH
//!   are not enabled in the configuration we mirror: the snapshot entry
//!   points return errors and the blocking-lock helpers are no-ops.

use crate::int::*;
use crate::os::{
    FcntlArg, ShmHandle, ShmPage, ShmRegion, SqliteFile, Vfs, sqlite3OsClose, sqlite3OsDelete,
    sqlite3OsDeviceCharacteristics, sqlite3OsFileControl, sqlite3OsFileControlHint,
    sqlite3OsFileSize, sqlite3OsLock, sqlite3OsOpen, sqlite3OsRead, sqlite3OsShmBarrier,
    sqlite3OsShmHandle, sqlite3OsShmLock, sqlite3OsShmMap, sqlite3OsShmUnmap, sqlite3OsSleep,
    sqlite3OsSync, sqlite3OsTruncate, sqlite3OsWrite,
};
use crate::util::{sqlite3Get4byte, sqlite3Put4byte};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The maximum (and only) versions of the wal and wal-index formats that
/// may be interpreted by this version of SQLite.
const WAL_MAX_VERSION: u32 = 3007000;
const WALINDEX_MAX_VERSION: u32 = 3007000;

// Index numbers for various locking bytes.   WAL_NREADER is the number of
// available reader locks and should be at least 3.  The default is
// SQLITE_SHM_NLOCK==8 and  WAL_NREADER==5.
const WAL_WRITE_LOCK: i32 = 0;
const WAL_ALL_BUT_WRITE: i32 = 1;
const WAL_CKPT_LOCK: i32 = 1;
const WAL_RECOVER_LOCK: i32 = 2;
#[inline]
const fn WAL_READ_LOCK(i: i32) -> i32 {
    3 + i
}
const WAL_NREADER: i32 = SQLITE_SHM_NLOCK - 3;

const READMARK_NOT_USED: u32 = 0xffffffff;

/// Size of the wal-index header in bytes: two copies of WalIndexHdr (48
/// bytes each) and one WalCkptInfo (40 bytes).
const WALINDEX_HDR_SIZE: usize = 136;
/// Word offsets of the WalCkptInfo fields in wal-index page 0.
const CKPT_NBACKFILL: usize = 24;
const CKPT_READMARK: usize = 25;
const CKPT_NBACKFILLATTEMPTED: usize = 32;

/// Size of header before each frame in wal
const WAL_FRAME_HDRSIZE: usize = 24;
/// Size of write ahead log header, including checksum.
const WAL_HDRSIZE: usize = 32;
/// WAL magic value. Either this value, or the same value with the least
/// significant bit also set (WAL_MAGIC | 0x00000001) is stored in 32-bit
/// big-endian format in the first 4 bytes of a WAL file.
const WAL_MAGIC: u32 = 0x377f0682;

/// Return the offset of frame iFrame in the write-ahead log file, assuming
/// a database page size of szPage bytes. The offset returned is to the
/// start of the write-ahead log frame-header.
#[inline]
fn walFrameOffset(iFrame: u32, szPage: i64) -> i64 {
    WAL_HDRSIZE as i64 + (iFrame.wrapping_sub(1) as i64) * (szPage + WAL_FRAME_HDRSIZE as i64)
}

// Candidate values for Wal.exclusiveMode.
const WAL_NORMAL_MODE: u8 = 0;
const WAL_EXCLUSIVE_MODE: u8 = 1;
const WAL_HEAPMEMORY_MODE: u8 = 2;

// Possible values for WAL.readOnly
const WAL_RDWR: u8 = 0; // Normal read/write connection
const WAL_RDONLY: u8 = 1; // The WAL file is readonly
const WAL_SHM_RDONLY: u8 = 2; // The SHM file is readonly

// Parameters of the hash tables in the wal-index file. There is a
// hash-table following every HASHTABLE_NPAGE page numbers in the
// wal-index.  Changing any of these constants will alter the wal-index
// format and create incompatibilities.
const HASHTABLE_NPAGE: usize = 4096; // Must be power of 2
const HASHTABLE_HASH_1: u32 = 383; // Should be prime
const HASHTABLE_NSLOT: usize = HASHTABLE_NPAGE * 2; // Must be a power of 2
/// The block of page numbers associated with the first hash-table in a
/// wal-index is smaller than usual. This is so that there is a complete
/// hash-table on each aligned 32KB page of the wal-index.
const HASHTABLE_NPAGE_ONE: usize = HASHTABLE_NPAGE - (WALINDEX_HDR_SIZE / 4);
/// The wal-index is divided into pages of WALINDEX_PGSZ bytes each.
const WALINDEX_PGSZ: usize = 2 * HASHTABLE_NSLOT + HASHTABLE_NPAGE * 4;

/// This is the value that walTryBeginRead returns when it needs to be
/// retried.
const WAL_RETRY: i32 = -1;
const WAL_RETRY_PROTOCOL_LIMIT: i32 = 100;
const WAL_RETRY_BLOCKED_MASK: i32 = 0;

/// C SQLITE_BIGENDIAN
const SQLITE_BIGENDIAN: u8 = cfg!(target_endian = "big") as u8;

#[inline]
fn WAL_SYNC_FLAGS(x: i32) -> i32 {
    x & 0x03
}
#[inline]
fn CKPT_SYNC_FLAGS(x: i32) -> i32 {
    (x >> 2) & 0x03
}

/// A copy of the wal-index header content (C WalIndexHdr).  The actual
/// header in the wal-index consists of two copies of this object followed
/// by one instance of the WalCkptInfo object.
///
/// The szPage value can be any power of 2 between 512 and 32768,
/// inclusive.  Or it can be 1 to represent a 65536-byte page.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct WalIndexHdr {
    /// Wal-index version
    iVersion: u32,
    /// Unused (padding) field
    unused: u32,
    /// Counter incremented each transaction
    iChange: u32,
    /// 1 when initialized
    isInit: u8,
    /// True if checksums in WAL are big-endian
    bigEndCksum: u8,
    /// Database page size in bytes. 1==64K
    szPage: u16,
    /// Index of last valid frame in the WAL
    mxFrame: u32,
    /// Size of database in pages
    nPage: u32,
    /// Checksum of last frame in log
    aFrameCksum: [u32; 2],
    /// Two salt values copied from WAL header (raw bytes, as C memcpy()s
    /// them)
    aSalt: [u8; 8],
    /// Checksum over all prior fields
    aCksum: [u32; 2],
}

/// Number of 32-bit words of a WalIndexHdr.
const HDR_NWORD: usize = 12;

impl WalIndexHdr {
    /// The in-memory image of the C struct, as native-endian words.
    fn to_words(&self) -> [u32; HDR_NWORD] {
        let b = [
            self.isInit,
            self.bigEndCksum,
            self.szPage.to_ne_bytes()[0],
            self.szPage.to_ne_bytes()[1],
        ];
        [
            self.iVersion,
            self.unused,
            self.iChange,
            u32::from_ne_bytes(b),
            self.mxFrame,
            self.nPage,
            self.aFrameCksum[0],
            self.aFrameCksum[1],
            u32::from_ne_bytes(self.aSalt[0..4].try_into().unwrap()),
            u32::from_ne_bytes(self.aSalt[4..8].try_into().unwrap()),
            self.aCksum[0],
            self.aCksum[1],
        ]
    }

    fn from_words(w: &[u32; HDR_NWORD]) -> WalIndexHdr {
        let b = w[3].to_ne_bytes();
        let mut aSalt = [0u8; 8];
        aSalt[0..4].copy_from_slice(&w[8].to_ne_bytes());
        aSalt[4..8].copy_from_slice(&w[9].to_ne_bytes());
        WalIndexHdr {
            iVersion: w[0],
            unused: w[1],
            iChange: w[2],
            isInit: b[0],
            bigEndCksum: b[1],
            szPage: u16::from_ne_bytes([b[2], b[3]]),
            mxFrame: w[4],
            nPage: w[5],
            aFrameCksum: [w[6], w[7]],
            aSalt,
            aCksum: [w[10], w[11]],
        }
    }
}

/// An open write-ahead log file (C Wal).
pub struct Wal {
    /// The VFS used to create pDbFd
    pVfs: Rc<Vfs>,
    /// The shared-memory methods of the database file (C pDbFd->xShm*);
    /// None if the VFS has none (heap-memory mode only)
    pShm: Option<ShmHandle>,
    /// File handle for WAL file
    pWalFd: SqliteFile,
    /// Value to pass to log callback (or 0)
    iCallback: u32,
    /// Truncate WAL to this size upon reset
    mxWalSize: i64,
    /// Pointer to wal-index content in memory (nWiData is the length)
    apWiData: Vec<Option<ShmPage>>,
    /// Database page size
    szPage: u32,
    /// Which read lock is being held.  -1 for none
    readLock: i16,
    /// Non-zero if connection is in exclusive mode
    exclusiveMode: u8,
    /// True if in a write transaction
    writeLock: u8,
    /// True if holding a checkpoint lock
    ckptLock: u8,
    /// WAL_RDWR, WAL_RDONLY, or WAL_SHM_RDONLY
    readOnly: u8,
    /// True to truncate WAL file on commit
    truncateOnCommit: u8,
    /// Fsync the WAL header if true
    syncHeader: u8,
    /// Pad transactions out to the next sector
    padToSectorBoundary: u8,
    /// SHM content is read-only and unreliable
    bShmUnreliable: u8,
    /// Wal-index header for current transaction
    hdr: WalIndexHdr,
    /// Ignore wal frames before this one
    minFrame: u32,
    /// On commit, recalculate checksums from here
    iReCksum: u32,
    /// Name of WAL file
    zWalName: ZStr,
    /// Checkpoint sequence counter in the wal-header
    nCkpt: u32,
    /// True if a locking error has occurred (C SQLITE_DEBUG lockError)
    lockError: u8,
}

impl Wal {
    /// Page iPage of the wal-index, which must be mapped.
    #[inline]
    fn wi(&self, iPage: usize) -> &[AtomicU32] {
        self.apWiData[iPage].as_deref().unwrap().w()
    }

    /// C walCkptInfo(pWal)->field: word iWord of page 0.
    #[inline]
    fn ckpt_get(&self, iWord: usize) -> u32 {
        self.wi(0)[iWord].load(Ordering::Relaxed)
    }
    #[inline]
    fn ckpt_set(&self, iWord: usize, v: u32) {
        self.wi(0)[iWord].store(v, Ordering::Relaxed)
    }

    /// Copy i of the wal-index header in shared memory (C
    /// `walIndexHdr(pWal)[i]`).
    fn live_hdr(&self, i: usize) -> WalIndexHdr {
        let p = self.wi(0);
        let mut w = [0u32; HDR_NWORD];
        for (k, x) in w.iter_mut().enumerate() {
            *x = p[i * HDR_NWORD + k].load(Ordering::Relaxed);
        }
        WalIndexHdr::from_words(&w)
    }
}

/// Obtain the iPage'th page of the wal-index. The wal-index is broken into
/// pages of WALINDEX_PGSZ bytes. Wal-index pages are numbered from zero.
///
/// If the wal-index is currently smaller the iPage pages then the size of
/// the wal-index might be increased, but only if it is safe to do so.  It
/// is safe to enlarge the wal-index if pWal->writeLock is true or
/// pWal->exclusiveMode==WAL_HEAPMEMORY_MODE.
///
/// Three possible result scenarios:
///
///   (1)  rc==SQLITE_OK    and *ppPage==Requested-Wal-Index-Page
///   (2)  rc>=SQLITE_ERROR and *ppPage==NULL
///   (3)  rc==SQLITE_OK    and *ppPage==NULL  // only if iPage==0
///
/// Scenario (3) can only occur when pWal->writeLock is false and iPage==0
fn walIndexPageRealloc(pWal: &mut Wal, iPage: usize, ppPage: &mut Option<ShmPage>) -> i32 {
    let mut rc = SQLITE_OK;

    // Enlarge the pWal->apWiData[] array if required
    if pWal.apWiData.len() <= iPage {
        pWal.apWiData.resize(iPage + 1, None);
    }

    // Request a pointer to the required page from the VFS
    debug_assert!(pWal.apWiData[iPage].is_none());
    if pWal.exclusiveMode == WAL_HEAPMEMORY_MODE {
        pWal.apWiData[iPage] = Some(Arc::new(ShmRegion::new_heap(WALINDEX_PGSZ)));
    } else {
        let mut pp: Option<ShmPage> = None;
        rc = match &pWal.pShm {
            Some(h) => sqlite3OsShmMap(
                h,
                iPage as i32,
                WALINDEX_PGSZ as i32,
                pWal.writeLock as i32,
                &mut pp,
            ),
            None => SQLITE_IOERR_SHMMAP,
        };
        pWal.apWiData[iPage] = pp;
        debug_assert!(
            pWal.apWiData[iPage].is_some()
                || rc != SQLITE_OK
                || (pWal.writeLock == 0 && iPage == 0)
        );
        if rc == SQLITE_OK {
            // sqlite3FaultSim(600) is never set
        } else if (rc & 0xff) == SQLITE_READONLY {
            pWal.readOnly |= WAL_SHM_RDONLY;
            if rc == SQLITE_READONLY {
                rc = SQLITE_OK;
            }
        }
    }

    *ppPage = pWal.apWiData[iPage].clone();
    debug_assert!(iPage == 0 || ppPage.is_some() || rc != SQLITE_OK);
    rc
}

fn walIndexPage(pWal: &mut Wal, iPage: usize, ppPage: &mut Option<ShmPage>) -> i32 {
    match pWal.apWiData.get(iPage) {
        Some(Some(p)) => {
            *ppPage = Some(p.clone());
            SQLITE_OK
        }
        _ => walIndexPageRealloc(pWal, iPage, ppPage),
    }
}

/// Generate or extend an 8 byte checksum based on the data in array
/// aByte[] and the initial values of aIn[0] and aIn[1] (or initial values
/// of 0 and 0 if aIn==NULL).
///
/// The checksum is returned.  nByte must be a positive multiple of 8.
/// `nativeCksum` is true for native byte-order, false for non-native.
fn walChecksumBytes(nativeCksum: bool, a: &[u8], aIn: Option<&[u32; 2]>) -> [u32; 2] {
    let (mut s1, mut s2) = match aIn {
        Some(x) => (x[0], x[1]),
        None => (0u32, 0u32),
    };
    // nByte is a multiple of 8 between 8 and 65536
    debug_assert!(a.len() >= 8 && (a.len() & 7) == 0 && a.len() <= 65536);
    // Word i is read in native order (native checksums) or byte-swapped.
    let bigEnd = nativeCksum == cfg!(target_endian = "big");
    let rd = |c: &[u8]| -> u32 {
        let b: [u8; 4] = c.try_into().unwrap();
        if bigEnd {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    };
    for c in a.chunks_exact(8) {
        s1 = s1.wrapping_add(rd(&c[0..4]).wrapping_add(s2));
        s2 = s2.wrapping_add(rd(&c[4..8]).wrapping_add(s1));
    }
    [s1, s2]
}

/// walChecksumBytes(1, ...) over native words.
fn walChecksumWords(a: &[u32]) -> [u32; 2] {
    let (mut s1, mut s2) = (0u32, 0u32);
    for c in a.chunks_exact(2) {
        s1 = s1.wrapping_add(c[0].wrapping_add(s2));
        s2 = s2.wrapping_add(c[1].wrapping_add(s1));
    }
    [s1, s2]
}

/// If there is the possibility of concurrent access to the SHM file from
/// multiple threads and/or processes, then do a memory barrier.
fn walShmBarrier(pWal: &Wal) {
    if pWal.exclusiveMode != WAL_HEAPMEMORY_MODE {
        match &pWal.pShm {
            Some(h) => sqlite3OsShmBarrier(h),
            None => std::sync::atomic::fence(Ordering::SeqCst),
        }
    }
}

/// Write the header information in pWal->hdr into the wal-index.
///
/// The checksum on pWal->hdr is updated before it is written.
fn walIndexWriteHdr(pWal: &mut Wal) {
    debug_assert!(pWal.writeLock != 0);
    pWal.hdr.isInit = 1;
    pWal.hdr.iVersion = WALINDEX_MAX_VERSION;
    let w = pWal.hdr.to_words();
    pWal.hdr.aCksum = walChecksumWords(&w[..HDR_NWORD - 2]);
    let w = pWal.hdr.to_words();
    let p = pWal.wi(0);
    for (k, x) in w.iter().enumerate() {
        p[HDR_NWORD + k].store(*x, Ordering::Relaxed);
    }
    walShmBarrier(pWal);
    let p = pWal.wi(0);
    for (k, x) in w.iter().enumerate() {
        p[k].store(*x, Ordering::Relaxed);
    }
}

/// This function encodes a single frame header and writes it to a buffer
/// supplied by the caller. A frame-header is made up of a series of 4-byte
/// big-endian integers, as follows:
///
///     0: Page number.
///     4: For commit records, the size of the database image in pages
///        after the commit. For all other records, zero.
///     8: Salt-1 (copied from the wal-header)
///    12: Salt-2 (copied from the wal-header)
///    16: Checksum-1.
///    20: Checksum-2.
fn walEncodeFrame(
    pWal: &mut Wal,
    iPage: u32,
    nTruncate: u32,
    aData: &[u8],
    aFrame: &mut [u8; WAL_FRAME_HDRSIZE],
) {
    sqlite3Put4byte(&mut aFrame[0..], iPage);
    sqlite3Put4byte(&mut aFrame[4..], nTruncate);
    if pWal.iReCksum == 0 {
        aFrame[8..16].copy_from_slice(&pWal.hdr.aSalt);

        let nativeCksum = pWal.hdr.bigEndCksum == SQLITE_BIGENDIAN;
        let mut aCksum = pWal.hdr.aFrameCksum;
        aCksum = walChecksumBytes(nativeCksum, &aFrame[..8], Some(&aCksum));
        aCksum = walChecksumBytes(nativeCksum, &aData[..pWal.szPage as usize], Some(&aCksum));
        pWal.hdr.aFrameCksum = aCksum;

        sqlite3Put4byte(&mut aFrame[16..], aCksum[0]);
        sqlite3Put4byte(&mut aFrame[20..], aCksum[1]);
    } else {
        aFrame[8..24].fill(0);
    }
}

/// Check to see if the frame with header in aFrame[] and content in
/// aData[] is valid.  If it is a valid frame, fill *piPage and *pnTruncate
/// and return true.  Return false if the frame is not valid.
fn walDecodeFrame(
    pWal: &mut Wal,
    piPage: &mut u32,
    pnTruncate: &mut u32,
    aData: &[u8],
    aFrame: &[u8],
) -> bool {
    // A frame is only valid if the salt values in the frame-header match
    // the salt values in the wal-header.
    if pWal.hdr.aSalt[..] != aFrame[8..16] {
        return false;
    }

    // A frame is only valid if the page number is greater than zero.
    let pgno = sqlite3Get4byte(&aFrame[0..]);
    if pgno == 0 {
        return false;
    }

    // Need a valid page size
    if pWal.szPage == 0 {
        return false;
    }

    // A frame is only valid if a checksum of the WAL header, all prior
    // frames, the first 16 bytes of this frame-header, and the frame-data
    // matches the checksum in the last 8 bytes of this frame-header.
    let nativeCksum = pWal.hdr.bigEndCksum == SQLITE_BIGENDIAN;
    let mut aCksum = pWal.hdr.aFrameCksum;
    aCksum = walChecksumBytes(nativeCksum, &aFrame[..8], Some(&aCksum));
    aCksum = walChecksumBytes(nativeCksum, &aData[..pWal.szPage as usize], Some(&aCksum));
    pWal.hdr.aFrameCksum = aCksum;
    if aCksum[0] != sqlite3Get4byte(&aFrame[16..]) || aCksum[1] != sqlite3Get4byte(&aFrame[20..]) {
        // Checksum failed.
        return false;
    }

    // If we reach this point, the frame is valid.  Return the page number
    // and the new database size.
    *piPage = pgno;
    *pnTruncate = sqlite3Get4byte(&aFrame[4..]);
    true
}

// Set or release locks on the WAL.  Locks are either shared or exclusive.
// A lock cannot be moved directly between shared and exclusive - it must
// go through the unlocked state first.
//
// In locking_mode=EXCLUSIVE, all of these routines become no-ops.

fn walShmLock(pWal: &Wal, ofst: i32, n: i32, flags: i32) -> i32 {
    match &pWal.pShm {
        Some(h) => sqlite3OsShmLock(h, ofst, n, flags),
        None => SQLITE_IOERR_SHMLOCK,
    }
}

fn walLockShared(pWal: &mut Wal, lockIdx: i32) -> i32 {
    if pWal.exclusiveMode != 0 {
        return SQLITE_OK;
    }
    let rc = walShmLock(pWal, lockIdx, 1, SQLITE_SHM_LOCK | SQLITE_SHM_SHARED);
    pWal.lockError = (rc != SQLITE_OK && (rc & 0xFF) != SQLITE_BUSY) as u8;
    rc
}

fn walUnlockShared(pWal: &mut Wal, lockIdx: i32) {
    if pWal.exclusiveMode != 0 {
        return;
    }
    let _ = walShmLock(pWal, lockIdx, 1, SQLITE_SHM_UNLOCK | SQLITE_SHM_SHARED);
}

fn walLockExclusive(pWal: &mut Wal, lockIdx: i32, n: i32) -> i32 {
    if pWal.exclusiveMode != 0 {
        return SQLITE_OK;
    }
    let rc = walShmLock(pWal, lockIdx, n, SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE);
    pWal.lockError = (rc != SQLITE_OK && (rc & 0xFF) != SQLITE_BUSY) as u8;
    rc
}

fn walUnlockExclusive(pWal: &mut Wal, lockIdx: i32, n: i32) {
    if pWal.exclusiveMode != 0 {
        return;
    }
    let _ = walShmLock(pWal, lockIdx, n, SQLITE_SHM_UNLOCK | SQLITE_SHM_EXCLUSIVE);
}

/// Compute a hash on a page number.  The resulting hash value must land
/// between 0 and (HASHTABLE_NSLOT-1).  The walNextHash() function
/// advances the hash to the next value in the event of a collision.
#[inline]
fn walHash(iPage: u32) -> usize {
    debug_assert!(iPage > 0);
    (iPage.wrapping_mul(HASHTABLE_HASH_1) as usize) & (HASHTABLE_NSLOT - 1)
}
#[inline]
fn walNextHash(iPriorHash: usize) -> usize {
    (iPriorHash + 1) & (HASHTABLE_NSLOT - 1)
}

/// The location of a page hash table in the wal-index (C WalHashLoc).
struct WalHashLoc {
    /// The wal-index page holding the hash table
    page: ShmPage,
    /// Word offset of aPgno[0] (C: aPgno[0] is the page number of the
    /// first frame indexed by the hash table, frame iZero+1)
    off: usize,
    /// One less than the frame number of first indexed
    iZero: u32,
}

/// Word offset of the hash table (C aHash) in a wal-index page.
const HASH_OFF: usize = HASHTABLE_NPAGE;

impl WalHashLoc {
    /// C aPgno[k]
    #[inline]
    fn pgno(&self, k: usize) -> u32 {
        self.page.w()[self.off + k].load(Ordering::Relaxed)
    }
    #[inline]
    fn set_pgno(&self, k: usize, v: u32) {
        self.page.w()[self.off + k].store(v, Ordering::Relaxed)
    }
    /// C aHash[slot] (an ht_slot, i.e. u16)
    #[inline]
    fn hash(&self, slot: usize) -> u32 {
        let w = self.page.w()[HASH_OFF + slot / 2].load(Ordering::Relaxed);
        ((w >> half_shift(slot)) & 0xffff) as u32
    }
    /// C aHash[slot] = v (an atomic read-modify-write of the word, so
    /// that the other half is never disturbed)
    #[inline]
    fn set_hash(&self, slot: usize, v: u32) {
        let sh = half_shift(slot);
        let a = &self.page.w()[HASH_OFF + slot / 2];
        let mut w = a.load(Ordering::Relaxed);
        loop {
            let n = (w & !(0xffff << sh)) | ((v & 0xffff) << sh);
            match a.compare_exchange_weak(w, n, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(x) => w = x,
            }
        }
    }
    /// Zero words [from, to) of the page (C memset()).
    fn zero_words(&self, from: usize, to: usize) {
        for x in &self.page.w()[from..to] {
            x.store(0, Ordering::Relaxed);
        }
    }
}

/// Bit offset of u16 slot `slot` within its 32-bit word (the slot at byte
/// offset 2*slot of native memory).
#[inline]
fn half_shift(slot: usize) -> u32 {
    if cfg!(target_endian = "little") {
        if slot & 1 == 0 { 0 } else { 16 }
    } else if slot & 1 == 0 {
        16
    } else {
        0
    }
}

/// Return the hash table and page number array stored on page iHash of
/// the wal-index. The wal-index is broken into 32KB pages numbered
/// starting from 0.
///
/// pLoc->iZero is one less than the frame number of the first frame
/// indexed by this hash table. If a slot in the hash table is set to N, it
/// refers to frame number (pLoc->iZero+N) in the log.
fn walHashGet(pWal: &mut Wal, iHash: usize, pLoc: &mut Option<WalHashLoc>) -> i32 {
    let mut page: Option<ShmPage> = None;
    let mut rc = walIndexPage(pWal, iHash, &mut page);
    debug_assert!(rc == SQLITE_OK || iHash > 0);

    if let Some(page) = page {
        let (off, iZero) = if iHash == 0 {
            (WALINDEX_HDR_SIZE / 4, 0u32)
        } else {
            (
                0,
                (HASHTABLE_NPAGE_ONE + (iHash - 1) * HASHTABLE_NPAGE) as u32,
            )
        };
        *pLoc = Some(WalHashLoc { page, off, iZero });
    } else if rc == SQLITE_OK {
        rc = SQLITE_ERROR;
    }
    rc
}

/// Return the number of the wal-index page that contains the hash-table
/// and page-number array that contain entries corresponding to WAL frame
/// iFrame. The wal-index is broken up into 32KB pages. Wal-index pages are
/// numbered starting from 0.
#[inline]
fn walFramePage(iFrame: u32) -> usize {
    let iHash = (iFrame as usize + HASHTABLE_NPAGE - HASHTABLE_NPAGE_ONE - 1) / HASHTABLE_NPAGE;
    debug_assert!(
        (iHash == 0 || iFrame as usize > HASHTABLE_NPAGE_ONE)
            && (iHash >= 1 || iFrame as usize <= HASHTABLE_NPAGE_ONE)
    );
    iHash
}

/// Return the page number associated with frame iFrame in this WAL.
fn walFramePgno(pWal: &Wal, iFrame: u32) -> u32 {
    let iHash = walFramePage(iFrame);
    if iHash == 0 {
        return pWal.wi(0)[WALINDEX_HDR_SIZE / 4 + iFrame as usize - 1].load(Ordering::Relaxed);
    }
    pWal.wi(iHash)[(iFrame as usize - 1 - HASHTABLE_NPAGE_ONE) % HASHTABLE_NPAGE]
        .load(Ordering::Relaxed)
}

/// Remove entries from the hash table that point to WAL slots greater
/// than pWal->hdr.mxFrame.
///
/// This function is called whenever pWal->hdr.mxFrame is decreased due to
/// a rollback or savepoint.
///
/// At most only the hash table containing pWal->hdr.mxFrame needs to be
/// updated.  Any later hash tables will be automatically cleared when
/// pWal->hdr.mxFrame advances to the point where those hash tables are
/// actually needed.
fn walCleanupHash(pWal: &mut Wal) {
    debug_assert!(pWal.writeLock != 0);
    if pWal.hdr.mxFrame == 0 {
        return;
    }

    // Obtain pointers to the hash-table and page-number array containing
    // the entry that corresponds to frame pWal->hdr.mxFrame. It is
    // guaranteed that the page said hash-table and array reside on is
    // already mapped.
    let mut sLoc = None;
    let i = walHashGet(pWal, walFramePage(pWal.hdr.mxFrame), &mut sLoc);
    if i != 0 {
        return; // Defense-in-depth
    }
    let sLoc = sLoc.unwrap();

    // Zero all hash-table entries that correspond to frame numbers greater
    // than pWal->hdr.mxFrame.
    let iLimit = (pWal.hdr.mxFrame - sLoc.iZero) as usize;
    debug_assert!(iLimit > 0);
    for i in 0..HASHTABLE_NSLOT {
        if sLoc.hash(i) as usize > iLimit {
            sLoc.set_hash(i, 0);
        }
    }

    // Zero the entries in the aPgno array that correspond to frames with
    // frame numbers greater than pWal->hdr.mxFrame.
    sLoc.zero_words(sLoc.off + iLimit, HASH_OFF);
}

/// Set an entry in the wal-index that will map database page number pPage
/// into WAL frame iFrame.
fn walIndexAppend(pWal: &mut Wal, iFrame: u32, iPage: u32) -> i32 {
    let mut sLoc = None;
    let rc = walHashGet(pWal, walFramePage(iFrame), &mut sLoc);

    // Assuming the wal-index file was successfully mapped, populate the
    // page number array and hash table entry.
    if rc == SQLITE_OK {
        let sLoc = sLoc.unwrap();
        let idx = (iFrame - sLoc.iZero) as usize;
        debug_assert!(idx <= HASHTABLE_NSLOT / 2 + 1);

        // If this is the first entry to be added to this hash-table, zero
        // the entire hash table and aPgno[] array before proceeding.
        if idx == 1 {
            sLoc.zero_words(sLoc.off, HASH_OFF + HASHTABLE_NSLOT / 2);
        }

        // If the entry in aPgno[] is already set, then the previous writer
        // must have exited unexpectedly in the middle of a transaction
        // (after writing one or more dirty pages to the WAL to free up
        // memory).  Remove the remnants of that writers uncommitted
        // transaction from the hash-table before writing any new entries.
        if sLoc.pgno(idx - 1) != 0 {
            walCleanupHash(pWal);
            debug_assert!(sLoc.pgno(idx - 1) == 0);
        }

        // Write the aPgno[] array entry and the hash-table slot.
        let mut nCollide = idx;
        let mut iKey = walHash(iPage);
        while sLoc.hash(iKey) != 0 {
            if nCollide == 0 {
                return SQLITE_CORRUPT_BKPT();
            }
            nCollide -= 1;
            iKey = walNextHash(iKey);
        }
        sLoc.set_pgno((idx - 1) & (HASHTABLE_NPAGE - 1), iPage);
        sLoc.set_hash(iKey, idx as u32);
    }
    rc
}

/// C SQLITE_CORRUPT_BKPT
#[allow(non_snake_case)]
#[inline]
fn SQLITE_CORRUPT_BKPT() -> i32 {
    SQLITE_CORRUPT
}

/// Recover the wal-index by reading the write-ahead log file.
///
/// This routine first tries to establish an exclusive lock on the
/// wal-index to prevent other threads/processes from doing anything with
/// the WAL or wal-index while recovery is running.  The WAL_RECOVER_LOCK
/// is also held so that other threads will know that this thread is
/// running recovery.  If unable to establish the necessary locks, this
/// routine returns SQLITE_BUSY.
fn walIndexRecover(pWal: &mut Wal) -> i32 {
    let mut rc;
    let mut nSize: i64 = 0; // Size of log file
    let mut aFrameCksum = [0u32; 2];

    // Obtain an exclusive lock on all byte in the locking range not
    // already locked by the caller. The caller is guaranteed to have
    // locked the WAL_WRITE_LOCK byte, and may have also locked the
    // WAL_CKPT_LOCK byte.  If successful, the same bytes that are locked
    // here are unlocked before this function returns.
    debug_assert!(pWal.ckptLock == 1 || pWal.ckptLock == 0);
    debug_assert!(pWal.writeLock != 0);
    let iLock = WAL_ALL_BUT_WRITE + pWal.ckptLock as i32;
    rc = walLockExclusive(pWal, iLock, WAL_READ_LOCK(0) - iLock);
    if rc != 0 {
        return rc;
    }

    pWal.hdr = WalIndexHdr::default();

    'recovery_error: {
        rc = sqlite3OsFileSize(&mut pWal.pWalFd, &mut nSize);
        if rc != SQLITE_OK {
            break 'recovery_error;
        }

        'finished: {
            if nSize > WAL_HDRSIZE as i64 {
                let mut aBuf = [0u8; WAL_HDRSIZE]; // Buffer to load WAL header into

                // Read in the WAL header.
                rc = sqlite3OsRead(&mut pWal.pWalFd, &mut aBuf, WAL_HDRSIZE as i32, 0);
                if rc != SQLITE_OK {
                    break 'recovery_error;
                }

                // If the database page size is not a power of two, or is
                // greater than SQLITE_MAX_PAGE_SIZE, conclude that the WAL
                // file contains no valid data. Similarly, if the 'magic'
                // value is invalid, ignore the whole WAL file.
                let magic = sqlite3Get4byte(&aBuf[0..]);
                let szPage = sqlite3Get4byte(&aBuf[8..]) as i32;
                if (magic & 0xFFFFFFFE) != WAL_MAGIC
                    || (szPage & (szPage.wrapping_sub(1))) != 0
                    || szPage > SQLITE_MAX_PAGE_SIZE
                    || szPage < 512
                {
                    break 'finished;
                }
                pWal.hdr.bigEndCksum = (magic & 0x00000001) as u8;
                pWal.szPage = szPage as u32;
                pWal.nCkpt = sqlite3Get4byte(&aBuf[12..]);
                pWal.hdr.aSalt.copy_from_slice(&aBuf[16..24]);

                // Verify that the WAL header checksum is correct
                pWal.hdr.aFrameCksum = walChecksumBytes(
                    pWal.hdr.bigEndCksum == SQLITE_BIGENDIAN,
                    &aBuf[..WAL_HDRSIZE - 2 * 4],
                    None,
                );
                if pWal.hdr.aFrameCksum[0] != sqlite3Get4byte(&aBuf[24..])
                    || pWal.hdr.aFrameCksum[1] != sqlite3Get4byte(&aBuf[28..])
                {
                    break 'finished;
                }

                // Verify that the version number on the WAL format is one
                // that are able to understand
                let version = sqlite3Get4byte(&aBuf[4..]);
                if version != WAL_MAX_VERSION {
                    rc = SQLITE_CANTOPEN;
                    break 'finished;
                }

                // Malloc a buffer to read frames into.
                let szFrame = szPage as usize + WAL_FRAME_HDRSIZE;
                let mut aFrame = vec![0u8; szFrame];
                // Heap copy of *-shm hash being populated (C aPrivate; one
                // buffer reused for every page, as in C)
                let aPrivate: ShmPage = Arc::new(ShmRegion::new_heap(WALINDEX_PGSZ));

                // Read all frames from the log file.
                let iLastFrame = ((nSize - WAL_HDRSIZE as i64) / szFrame as i64) as u32;
                let mut iPg: u32 = 0;
                while iPg <= walFramePage(iLastFrame) as u32 {
                    let iLast = iLastFrame
                        .min((HASHTABLE_NPAGE_ONE + iPg as usize * HASHTABLE_NPAGE) as u32);
                    let iFirst = 1 + if iPg == 0 {
                        0
                    } else {
                        (HASHTABLE_NPAGE_ONE + (iPg as usize - 1) * HASHTABLE_NPAGE) as u32
                    };
                    let mut aShare: Option<ShmPage> = None;
                    rc = walIndexPage(pWal, iPg as usize, &mut aShare);
                    debug_assert!(aShare.is_some() || rc != SQLITE_OK);
                    let Some(aShare) = aShare else {
                        break;
                    };
                    pWal.apWiData[iPg as usize] = Some(aPrivate.clone());

                    let mut iFrame = iFirst;
                    while iFrame <= iLast {
                        let iOffset = walFrameOffset(iFrame, szPage as i64);
                        let mut pgno: u32 = 0; // Database page number for frame
                        let mut nTruncate: u32 = 0; // dbsize field from frame header

                        // Read and decode the next log frame.
                        rc = sqlite3OsRead(&mut pWal.pWalFd, &mut aFrame, szFrame as i32, iOffset);
                        if rc != SQLITE_OK {
                            break;
                        }
                        let (hdrPart, dataPart) = aFrame.split_at(WAL_FRAME_HDRSIZE);
                        let isValid =
                            walDecodeFrame(pWal, &mut pgno, &mut nTruncate, dataPart, hdrPart);
                        if !isValid {
                            break;
                        }
                        rc = walIndexAppend(pWal, iFrame, pgno);
                        if rc != SQLITE_OK {
                            break;
                        }

                        // If nTruncate is non-zero, this is a commit record.
                        if nTruncate != 0 {
                            pWal.hdr.mxFrame = iFrame;
                            pWal.hdr.nPage = nTruncate;
                            pWal.hdr.szPage = ((szPage & 0xff00) | (szPage >> 16)) as u16;
                            aFrameCksum[0] = pWal.hdr.aFrameCksum[0];
                            aFrameCksum[1] = pWal.hdr.aFrameCksum[1];
                        }
                        iFrame += 1;
                    }
                    pWal.apWiData[iPg as usize] = Some(aShare.clone());
                    let nHdr32 = if iPg == 0 { WALINDEX_HDR_SIZE / 4 } else { 0 };
                    let src = aPrivate.w();
                    for (d, s) in aShare.w()[nHdr32..].iter().zip(&src[nHdr32..]) {
                        d.store(s.load(Ordering::Relaxed), Ordering::Relaxed);
                    }
                    if iFrame <= iLast {
                        break;
                    }
                    iPg += 1;
                }
            }
        }

        // finished:
        if rc == SQLITE_OK {
            pWal.hdr.aFrameCksum[0] = aFrameCksum[0];
            pWal.hdr.aFrameCksum[1] = aFrameCksum[1];
            walIndexWriteHdr(pWal);

            // Reset the checkpoint-header. This is safe because this
            // thread is currently holding locks that exclude all other
            // writers and checkpointers. Then set the values of read-mark
            // slots 1 through N.
            pWal.ckpt_set(CKPT_NBACKFILL, 0);
            pWal.ckpt_set(CKPT_NBACKFILLATTEMPTED, pWal.hdr.mxFrame);
            pWal.ckpt_set(CKPT_READMARK, 0);
            for i in 1..WAL_NREADER {
                rc = walLockExclusive(pWal, WAL_READ_LOCK(i), 1);
                if rc == SQLITE_OK {
                    if i == 1 && pWal.hdr.mxFrame != 0 {
                        pWal.ckpt_set(CKPT_READMARK + i as usize, pWal.hdr.mxFrame);
                    } else {
                        pWal.ckpt_set(CKPT_READMARK + i as usize, READMARK_NOT_USED);
                    }
                    walUnlockExclusive(pWal, WAL_READ_LOCK(i), 1);
                } else if rc != SQLITE_BUSY {
                    break 'recovery_error;
                }
            }

            // If more than one frame was recovered from the log file,
            // report an event via sqlite3_log(). This is to help with
            // identifying performance problems caused by applications
            // routinely shutting down without checkpointing the log file.
            if pWal.hdr.nPage != 0 {
                crate::printf::sqlite3_log(
                    SQLITE_NOTICE_RECOVER_WAL,
                    b"recovered %d frames from WAL file %s",
                    &[
                        crate::printf::PfArg::from(pWal.hdr.mxFrame as i64),
                        crate::printf::PfArg::from(&pWal.zWalName[..]),
                    ],
                );
            }
        }
    }

    // recovery_error:
    walUnlockExclusive(pWal, iLock, WAL_READ_LOCK(0) - iLock);
    rc
}

/// Close an open wal-index.
fn walIndexClose(pWal: &mut Wal, isDelete: i32) {
    if pWal.exclusiveMode == WAL_HEAPMEMORY_MODE || pWal.bShmUnreliable != 0 {
        for p in pWal.apWiData.iter_mut() {
            *p = None;
        }
    }
    if pWal.exclusiveMode != WAL_HEAPMEMORY_MODE {
        if let Some(h) = &pWal.pShm {
            sqlite3OsShmUnmap(h, isDelete);
        }
    }
}

/// C: `int sqlite3WalOpen( sqlite3_vfs *pVfs, sqlite3_file *pDbFd, const char *zWalName, int bNoShm, i64 mxWalSize, Wal **ppWal )`
///
/// Open a connection to the WAL file zWalName. The database file must
/// already be opened on connection pDbFd.
///
/// A SHARED lock should be held on the database file when this function
/// is called. The purpose of this SHARED lock is to prevent any other
/// client from unlinking the WAL or wal-index file. If another process
/// were to do this just after this client opened one of these files, the
/// system would be badly broken.
///
/// If the log file is successfully opened, SQLITE_OK is returned and
/// *ppWal is set to point to a new WAL handle. If an error occurs, an
/// SQLite error code is returned and *ppWal is left unmodified.
pub fn sqlite3WalOpen(
    pVfs: &Rc<crate::os::Vfs>,
    pDbFd: &mut crate::os::SqliteFile,
    zWalName: &[u8],
    bNoShm: i32,
    mxWalSize: i64,
    ppWal: &mut Option<Box<crate::wal::Wal>>,
) -> i32 {
    debug_assert!(!zWalName.is_empty());
    const _: () = assert!(WALINDEX_PGSZ == 32768 && HASHTABLE_NPAGE_ONE == 4062);
    const _: () = assert!(WAL_NREADER == 5 && WALINDEX_HDR_SIZE == 136);

    // Allocate an instance of struct Wal to return.
    *ppWal = None;
    let mut pRet = Box::new(Wal {
        pVfs: pVfs.clone(),
        pShm: sqlite3OsShmHandle(pDbFd),
        pWalFd: SqliteFile::Closed,
        iCallback: 0,
        mxWalSize,
        apWiData: Vec::new(),
        szPage: 0,
        readLock: -1,
        exclusiveMode: if bNoShm != 0 {
            WAL_HEAPMEMORY_MODE
        } else {
            WAL_NORMAL_MODE
        },
        writeLock: 0,
        ckptLock: 0,
        readOnly: WAL_RDWR,
        truncateOnCommit: 0,
        syncHeader: 1,
        padToSectorBoundary: 1,
        bShmUnreliable: 0,
        hdr: WalIndexHdr::default(),
        minFrame: 0,
        iReCksum: 0,
        zWalName: zWalName.to_vec(),
        nCkpt: 0,
        lockError: 0,
    });

    // Open file handle on the write-ahead log file.
    let mut flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_WAL;
    let mut outFlags = flags;
    let rc = sqlite3OsOpen(
        pVfs,
        Some(zWalName),
        &mut pRet.pWalFd,
        flags,
        Some(&mut outFlags),
    );
    flags = outFlags;
    if rc == SQLITE_OK && (flags & SQLITE_OPEN_READONLY) != 0 {
        pRet.readOnly = WAL_RDONLY;
    }

    if rc != SQLITE_OK {
        walIndexClose(&mut pRet, 0);
        sqlite3OsClose(&mut pRet.pWalFd);
    } else {
        let iDC = sqlite3OsDeviceCharacteristics(pDbFd);
        if (iDC & SQLITE_IOCAP_SEQUENTIAL) != 0 {
            pRet.syncHeader = 0;
        }
        if (iDC & SQLITE_IOCAP_POWERSAFE_OVERWRITE) != 0 {
            pRet.padToSectorBoundary = 0;
        }
        *ppWal = Some(pRet);
    }
    rc
}

/// C: `void sqlite3WalLimit(Wal *pWal, i64 iLimit)`
///
/// Change the size to which the WAL file is truncated on each reset.
pub fn sqlite3WalLimit(pWal: &mut crate::wal::Wal, iLimit: i64) {
    pWal.mxWalSize = iLimit;
}

/// One segment of a WalIterator (C struct WalSegment): the frames indexed
/// by one 32KB page of the wal-index.
struct WalSegment {
    /// Next slot in aIndex[] not yet returned
    iNext: usize,
    /// i0, i1, i2... such that aPgno[iN] ascend
    aIndex: Vec<u16>,
    /// Array of page numbers (a copy of the wal-index entries).
    aPgno: Vec<u32>,
    /// Frame number associated with aPgno[0]
    iZero: u32,
}

/// An iterator that loops through all frames in the WAL in database page
/// order (C WalIterator). Where two or more frames correspond to the same
/// database page, the iterator visits only the frame most recently
/// written to the WAL (in other words, the frame with the largest index).
///
/// There are two possible algorithms:
///
///   WALITER-1     Load all pages mentioned in the WAL file and sort them,
///                 then hand them back out in sorted order as requested by
///                 walIteratorNext().  nPagemap==0 for this algorithm.
///
///   WALITER-2     Create a map of all database file pages and record the
///                 last WAL file frame that updates that page.  Then walk
///                 through the map in acsending order and return all
///                 non-zero entries. nPagemap is positive for this
///                 algorithm.
struct WalIterator {
    /// Last result returned from the iterator
    iPrior: u32,
    /// Non-zero in "page-map" mode
    nPagemap: u32,
    /// WALITER-2: aPagemap[PGNO] is the frame number of the last entry for
    /// page PGNO in the WAL file, or 0.
    aPagemap: Vec<u32>,
    /// WALITER-1: one for every 32KB page in the wal-index
    aSegment: Vec<WalSegment>,
}

/// Find the smallest page number out of all pages held in the WAL that
/// has not been returned by any prior invocation of this method on the
/// same WalIterator object.   Write into *piFrame the frame index where
/// that page was last written into the WAL.  Write into *piPage the page
/// number.
///
/// Return false on success.  If there are no pages in the WAL with a page
/// number larger than *piPage, then return true.
fn walIteratorNext(p: &mut WalIterator, piPage: &mut u32, piFrame: &mut u32) -> bool {
    let mut iRet: u32 = 0xFFFFFFFF; // 0xffffffff is never a valid page number

    if p.nPagemap != 0 {
        // Algorithm WALITER-2
        while p.iPrior < p.nPagemap {
            p.iPrior += 1;
            let f = p.aPagemap[p.iPrior as usize];
            if f != 0 {
                iRet = p.iPrior;
                *piFrame = f;
                break;
            }
        }
    } else {
        // Algorithm WALITER-1
        let iMin = p.iPrior; // Result pgno must be greater than iMin
        debug_assert!(iMin < 0xffffffff);
        for pSegment in p.aSegment.iter_mut().rev() {
            while pSegment.iNext < pSegment.aIndex.len() {
                let ix = pSegment.aIndex[pSegment.iNext] as usize;
                let iPg = pSegment.aPgno[ix];
                if iPg > iMin {
                    if iPg < iRet {
                        iRet = iPg;
                        *piFrame = pSegment.iZero + ix as u32;
                    }
                    break;
                }
                pSegment.iNext += 1;
            }
        }
    }

    p.iPrior = iRet;
    *piPage = iRet;
    iRet == 0xFFFFFFFF
}

/// Sort the elements in list aList using aContent[] as the sort key.
/// Remove elements with duplicate keys, preferring to keep the larger
/// aList[] values (C walMergesort()).
///
/// The aList[] entries are indices into aContent[].  The values in aList[]
/// are to be sorted so that for all J<K:
///
///      aContent[aList[J]] < aContent[aList[K]]
///
/// For any X and Y such that
///
///      aContent[aList[X]] == aContent[aList[Y]]
///
/// Keep the larger of the two values aList[X] and aList[Y] and discard the
/// smaller.  The C merge sort and this stable sort produce the same list.
fn walMergesort(aContent: &[u32], aList: &mut Vec<u16>) {
    aList.sort_by_key(|&i| aContent[i as usize]);
    // Equal keys are adjacent and in increasing aList order (the input is
    // ascending and the sort is stable): keep the last of each run.
    let mut out: Vec<u16> = Vec::with_capacity(aList.len());
    for &i in aList.iter() {
        match out.last_mut() {
            Some(l) if aContent[*l as usize] == aContent[i as usize] => *l = i,
            _ => out.push(i),
        }
    }
    *aList = out;
}

/// Construct a WalInterator object that can be used to loop over all
/// pages in the WAL following frame nBackfill in ascending order. Frames
/// nBackfill or earlier may be included - excluding them is an
/// optimization only. The caller must hold the checkpoint lock.
fn walIteratorInit(pWal: &mut Wal, nBackfill: u32, pp: &mut Option<WalIterator>) -> i32 {
    let mut rc = SQLITE_OK;

    // This routine only runs while holding the checkpoint lock. And it
    // only runs if there is actually content in the log (mxFrame>0).
    debug_assert!(pWal.ckptLock != 0 && pWal.hdr.mxFrame > 0);

    let iLast = pWal.hdr.mxFrame;
    let iFirstPage = walFramePage(nBackfill + 1);
    let iLastPage = walFramePage(iLast);
    let iZero = if iFirstPage != 0 {
        (HASHTABLE_NPAGE_ONE + (iFirstPage - 1) * HASHTABLE_NPAGE) as u32
    } else {
        0
    };

    let eAlg = if (iLast - iZero) as u64 * 2 > pWal.hdr.nPage as u64 * 4 {
        2 // Algorithm WALITER-2
    } else {
        1 // Algorithm WALITER-1
    };
    let mut p = WalIterator {
        iPrior: 0,
        nPagemap: 0,
        aPagemap: Vec::new(),
        aSegment: Vec::new(),
    };
    if eAlg == 2 {
        p.nPagemap = pWal.hdr.nPage;
        p.aPagemap = vec![0u32; pWal.hdr.nPage as usize + 1];
    }
    let mut i = iFirstPage;
    while rc == SQLITE_OK && i <= iLastPage {
        let mut sLoc = None;
        rc = walHashGet(pWal, i, &mut sLoc);
        if rc == SQLITE_OK {
            let sLoc = sLoc.unwrap();
            let nEntry = if i == iLastPage {
                (iLast - sLoc.iZero) as usize
            } else {
                HASH_OFF - sLoc.off
            };

            if eAlg == 2 {
                for j in 0..nEntry {
                    let pgno = sLoc.pgno(j);
                    if pgno <= p.nPagemap {
                        p.aPagemap[pgno as usize] = sLoc.iZero + j as u32 + 1;
                    }
                }
            } else {
                debug_assert!(i != iFirstPage || iZero == sLoc.iZero);
                let aPgno: Vec<u32> = (0..nEntry).map(|j| sLoc.pgno(j)).collect();
                let mut aIndex: Vec<u16> = (0..nEntry as u16).collect();
                walMergesort(&aPgno, &mut aIndex);
                p.aSegment.push(WalSegment {
                    iNext: 0,
                    aIndex,
                    aPgno,
                    iZero: sLoc.iZero + 1,
                });
            }
        }
        i += 1;
    }
    *pp = if rc != SQLITE_OK { None } else { Some(p) };
    rc
}

/// C: `int sqlite3WalWriteLock(Wal *pWal, int bLock)`
/// (SQLITE_ENABLE_SETLK_TIMEOUT only: blocking locks are not available,
/// so only the release of the WRITER lock does anything.)
pub fn sqlite3WalWriteLock(db: &mut Sqlite3, pWal: &mut crate::wal::Wal, bLock: i32) -> i32 {
    let _ = db;
    if bLock == 0 && pWal.writeLock != 0 {
        walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
        pWal.writeLock = 0;
    }
    SQLITE_OK
}

/// C: `void sqlite3WalDb(Wal *pWal, sqlite3 *db)` (SQLITE_ENABLE_SETLK_TIMEOUT only)
pub fn sqlite3WalDb(db: &mut Sqlite3, pWal: &mut crate::wal::Wal) {
    let _ = (db, pWal);
}

/// Attempt to obtain the exclusive WAL lock defined by parameters lockIdx
/// and n. If the attempt fails and parameter xBusy is not NULL, then it is
/// a busy-handler function. Invoke it and retry the lock until either the
/// lock is successfully obtained or the busy-handler returns 0.
fn walBusyLock(pWal: &mut Wal, xBusy: &Option<Rc<dyn Fn() -> i32>>, lockIdx: i32, n: i32) -> i32 {
    loop {
        let rc = walLockExclusive(pWal, lockIdx, n);
        match xBusy {
            Some(x) if rc == SQLITE_BUSY && x() != 0 => {}
            _ => return rc,
        }
    }
}

/// The cache of the wal-index header must be valid to call this function.
/// Return the page-size in bytes used by the database.
fn walPagesize(pWal: &Wal) -> i32 {
    (pWal.hdr.szPage as i32 & 0xfe00) + ((pWal.hdr.szPage as i32 & 0x0001) << 16)
}

/// The following is guaranteed when this function is called:
///
///   a) the WRITER lock is held,
///   b) the entire log file has been checkpointed, and
///   c) any existing readers are reading exclusively from the database
///      file - there are no readers that may attempt to read a frame from
///      the log file.
///
/// This function updates the shared-memory structures so that the next
/// client to write to the database (which may be this one) does so by
/// writing frames into the start of the log file.
///
/// The value of parameter salt1 is used as the aSalt[1] value in the new
/// wal-index header. It should be passed a pseudo-random value (i.e. one
/// obtained from sqlite3_randomness()).
fn walRestartHdr(pWal: &mut Wal, salt1: [u8; 4]) {
    pWal.nCkpt = pWal.nCkpt.wrapping_add(1);
    pWal.hdr.mxFrame = 0;
    let s0 = 1u32.wrapping_add(sqlite3Get4byte(&pWal.hdr.aSalt[0..4]));
    sqlite3Put4byte(&mut pWal.hdr.aSalt[0..4], s0);
    pWal.hdr.aSalt[4..8].copy_from_slice(&salt1);
    walIndexWriteHdr(pWal);
    pWal.ckpt_set(CKPT_NBACKFILL, 0);
    pWal.ckpt_set(CKPT_NBACKFILLATTEMPTED, 0);
    pWal.ckpt_set(CKPT_READMARK + 1, 0);
    for i in 2..WAL_NREADER as usize {
        pWal.ckpt_set(CKPT_READMARK + i, READMARK_NOT_USED);
    }
    debug_assert!(pWal.ckpt_get(CKPT_READMARK) == 0);
}

/// Copy as much content as we can from the WAL back into the database file
/// in response to an sqlite3_wal_checkpoint() request or the equivalent.
///
/// The amount of information copies from WAL to database might be limited
/// by active readers.  This routine will never overwrite a database page
/// that a concurrent reader might be using.
///
/// All I/O barrier operations (a.k.a fsyncs) occur in this routine when
/// SQLite is in WAL-mode in synchronous=NORMAL.  That means that if
/// checkpoints are always run by a background thread or background
/// process, foreground threads will never block on a lengthy fsync call.
///
/// Fsync is called on the WAL before writing content out of the WAL and
/// into the database.  This ensures that if the new content is persistent
/// in the WAL and can be recovered following a power-loss or hard reset.
///
/// Fsync is also called on the database file if (and only if) the entire
/// WAL content is copied into the database file.  This second fsync makes
/// it safe to delete the WAL since the new content will persist in the
/// database file.
///
/// This routine uses and updates the nBackfill field of the wal-index
/// header.  This is the only routine that will increase the value of
/// nBackfill.  (A WAL reset or recovery will revert nBackfill to zero, but
/// not increase its value.)
///
/// The caller must be holding sufficient locks to ensure that no other
/// checkpoint is running (in any other thread or process) at the same
/// time.
fn walCheckpoint(
    pWal: &mut Wal,
    db: &mut Sqlite3,
    pDbFd: &mut SqliteFile,
    eMode: i32,
    xBusy: Option<Rc<dyn Fn() -> i32>>,
    sync_flags: i32,
    zBuf: &mut [u8],
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut xBusy = xBusy;
    let mut pIter: Option<WalIterator> = None; // Wal iterator context
    let mut iDbpage: u32 = 0; // Next database page to write
    let mut iFrame: u32 = 0; // Wal frame containing data for iDbpage

    let szPage = walPagesize(pWal); // Database page-size
    'walcheckpoint_out: {
        if pWal.ckpt_get(CKPT_NBACKFILL) < pWal.hdr.mxFrame {
            // EVIDENCE-OF: R-62920-47450 The busy-handler callback is never
            // invoked in the SQLITE_CHECKPOINT_PASSIVE mode.
            debug_assert!(eMode != SQLITE_CHECKPOINT_PASSIVE || xBusy.is_none());

            // Compute in mxSafeFrame the index of the last frame of the WAL
            // that is safe to write into the database.  Frames beyond
            // mxSafeFrame might overwrite database pages that are in use by
            // active readers and thus cannot be backfilled from the WAL.
            let mut mxSafeFrame = pWal.hdr.mxFrame; // Max frame that can be backfilled
            let mxPage = pWal.hdr.nPage; // Max database page to write
            for i in 1..WAL_NREADER {
                let y = pWal.ckpt_get(CKPT_READMARK + i as usize);
                if mxSafeFrame > y {
                    debug_assert!(y <= pWal.hdr.mxFrame);
                    rc = walBusyLock(pWal, &xBusy, WAL_READ_LOCK(i), 1);
                    if rc == SQLITE_OK {
                        let iMark = if i == 1 {
                            mxSafeFrame
                        } else {
                            READMARK_NOT_USED
                        };
                        pWal.ckpt_set(CKPT_READMARK + i as usize, iMark);
                        walUnlockExclusive(pWal, WAL_READ_LOCK(i), 1);
                    } else if rc == SQLITE_BUSY {
                        mxSafeFrame = y;
                        xBusy = None;
                    } else {
                        break 'walcheckpoint_out;
                    }
                }
            }

            // Allocate the iterator
            if pWal.ckpt_get(CKPT_NBACKFILL) < mxSafeFrame {
                let nBackfill = pWal.ckpt_get(CKPT_NBACKFILL);
                rc = walIteratorInit(pWal, nBackfill, &mut pIter);
                debug_assert!(rc == SQLITE_OK || pIter.is_none());
            }

            if pIter.is_some() && {
                rc = walBusyLock(pWal, &xBusy, WAL_READ_LOCK(0), 1);
                rc == SQLITE_OK
            } {
                let nBackfill = pWal.ckpt_get(CKPT_NBACKFILL);
                let pLive = pWal.live_hdr(0);

                // Now that read-lock slot 0 is locked, check that the wal
                // has not been wrapped since the header was read for this
                // checkpoint. If it was, then there was no work to do
                // anyway.  In this case the (pInfo->nBackfill <
                // pWal->hdr.mxFrame) test above only passed because
                // pInfo->nBackfill had already been set to 0 by the writer
                // that wrapped the wal file. It would also be dangerous to
                // proceed, as there may be fewer than pWal->hdr.mxFrame
                // valid frames in the wal file.
                let bChg = pLive.aSalt != pWal.hdr.aSalt;
                if !bChg {
                    pWal.ckpt_set(CKPT_NBACKFILLATTEMPTED, mxSafeFrame);

                    // Sync the WAL to disk
                    rc = sqlite3OsSync(&mut pWal.pWalFd, CKPT_SYNC_FLAGS(sync_flags));

                    // If the database may grow as a result of this
                    // checkpoint, hint about the eventual size of the db
                    // file to the VFS layer.
                    if rc == SQLITE_OK {
                        let mut nReq = mxPage as i64 * szPage as i64;
                        let mut nSize: i64 = 0; // Current size of database file
                        sqlite3OsFileControl(pDbFd, SQLITE_FCNTL_CKPT_START, FcntlArg::None);
                        rc = sqlite3OsFileSize(pDbFd, &mut nSize);
                        if rc == SQLITE_OK && nSize < nReq {
                            if (nSize + 65536 + pWal.hdr.mxFrame as i64 * szPage as i64) < nReq {
                                // If the size of the final database is
                                // larger than the current database plus the
                                // amount of data in the wal file, plus the
                                // maximum size of the pending-byte page
                                // (65536 bytes), then must be corruption
                                // somewhere.
                                rc = SQLITE_CORRUPT_BKPT();
                            } else {
                                sqlite3OsFileControlHint(
                                    pDbFd,
                                    SQLITE_FCNTL_SIZE_HINT,
                                    FcntlArg::I64(&mut nReq),
                                );
                            }
                        }
                    }

                    // Iterate through the contents of the WAL, copying data
                    // to the db file
                    let it = pIter.as_mut().unwrap();
                    while rc == SQLITE_OK && !walIteratorNext(it, &mut iDbpage, &mut iFrame) {
                        debug_assert!(walFramePgno(pWal, iFrame) == iDbpage);
                        if db.isInterrupted.get() != 0 {
                            rc = if db.mallocFailed != 0 {
                                SQLITE_NOMEM
                            } else {
                                SQLITE_INTERRUPT
                            };
                            break;
                        }
                        if iFrame <= nBackfill || iFrame > mxSafeFrame || iDbpage > mxPage {
                            continue;
                        }
                        let mut iOffset =
                            walFrameOffset(iFrame, szPage as i64) + WAL_FRAME_HDRSIZE as i64;
                        rc = sqlite3OsRead(&mut pWal.pWalFd, zBuf, szPage, iOffset);
                        if rc != SQLITE_OK {
                            break;
                        }
                        iOffset = (iDbpage as i64 - 1) * szPage as i64;
                        rc = sqlite3OsWrite(pDbFd, zBuf, szPage, iOffset);
                        if rc != SQLITE_OK {
                            break;
                        }
                    }
                    sqlite3OsFileControl(pDbFd, SQLITE_FCNTL_CKPT_DONE, FcntlArg::None);

                    // If work was actually accomplished...
                    if rc == SQLITE_OK {
                        if mxSafeFrame == pWal.live_hdr(0).mxFrame {
                            let szDb = pWal.hdr.nPage as i64 * szPage as i64;
                            rc = sqlite3OsTruncate(pDbFd, szDb);
                            if rc == SQLITE_OK {
                                rc = sqlite3OsSync(pDbFd, CKPT_SYNC_FLAGS(sync_flags));
                            }
                        }
                        if rc == SQLITE_OK {
                            pWal.ckpt_set(CKPT_NBACKFILL, mxSafeFrame);
                        }
                    }
                }

                // Release the reader lock held while backfilling
                walUnlockExclusive(pWal, WAL_READ_LOCK(0), 1);
            }

            if rc == SQLITE_BUSY {
                // Reset the return code so as not to report a checkpoint
                // failure just because there are active readers.
                rc = SQLITE_OK;
            }
        }

        // If this is an SQLITE_CHECKPOINT_RESTART or TRUNCATE operation,
        // and the entire wal file has been copied into the database file,
        // then block until all readers have finished using the wal file.
        // This ensures that the next process to write to the database
        // restarts the wal file.
        if rc == SQLITE_OK && eMode != SQLITE_CHECKPOINT_PASSIVE {
            debug_assert!(pWal.writeLock != 0);
            if pWal.ckpt_get(CKPT_NBACKFILL) < pWal.hdr.mxFrame {
                rc = SQLITE_BUSY;
            } else if eMode >= SQLITE_CHECKPOINT_RESTART {
                let mut salt1 = [0u8; 4];
                crate::os::os_randomness(&mut salt1);
                debug_assert!(pWal.ckpt_get(CKPT_NBACKFILL) == pWal.hdr.mxFrame);
                rc = walBusyLock(pWal, &xBusy, WAL_READ_LOCK(1), WAL_NREADER - 1);
                if rc == SQLITE_OK {
                    if eMode == SQLITE_CHECKPOINT_TRUNCATE {
                        // IMPLEMENTATION-OF: R-44699-57140 This mode works
                        // the same way as SQLITE_CHECKPOINT_RESTART with the
                        // addition that it also truncates the log file to
                        // zero bytes just prior to a successful return.
                        //
                        // In theory, it might be safe to do this without
                        // updating the wal-index header in shared memory,
                        // as all subsequent reader or writer clients should
                        // see that the entire log file has been checkpointed
                        // and behave accordingly. This seems unsafe though,
                        // as it would leave the system in a state where the
                        // contents of the wal-index header do not match the
                        // contents of the file-system. To avoid this, update
                        // the wal-index header to indicate that the log file
                        // contains zero valid frames.
                        walRestartHdr(pWal, salt1);
                        rc = sqlite3OsTruncate(&mut pWal.pWalFd, 0);
                    }
                    walUnlockExclusive(pWal, WAL_READ_LOCK(1), WAL_NREADER - 1);
                }
            }
        }
    }

    // walcheckpoint_out:
    rc
}

/// If the WAL file is currently larger than nMax bytes in size, truncate
/// it to exactly nMax bytes. If an error occurs while doing so, ignore it.
fn walLimitSize(pWal: &mut Wal, nMax: i64) {
    let mut sz: i64 = 0;
    let mut rx = sqlite3OsFileSize(&mut pWal.pWalFd, &mut sz);
    if rx == SQLITE_OK && sz > nMax {
        rx = sqlite3OsTruncate(&mut pWal.pWalFd, nMax);
    }
    if rx != 0 {
        crate::printf::sqlite3_log(
            rx,
            b"cannot limit WAL size: %s",
            &[crate::printf::PfArg::from(&pWal.zWalName[..])],
        );
    }
}

/// C: `int sqlite3WalSystemErrno(Wal *pWal)` (SQLITE_USE_SEH only)
pub fn sqlite3WalSystemErrno(pWal: &mut crate::wal::Wal) -> i32 {
    let _ = pWal;
    0
}

/// C: `int sqlite3WalClose( Wal *pWal, sqlite3 *db, int sync_flags, int nBuf, u8 *zBuf )`
///
/// Close a connection to a log file.
///
/// Signature change: `db` is optional (C passes it only for the interrupt
/// flag of the checkpoint, which is not run without it), the database
/// file is passed as `pDbFd`, an empty zBuf is C `zBuf==0`.
pub fn sqlite3WalClose(
    db: Option<&mut Sqlite3>,
    pWal: &mut crate::wal::Wal,
    pDbFd: &mut SqliteFile,
    sync_flags: i32,
    nBuf: i32,
    zBuf: &mut [u8],
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut isDelete = 0; // True to unlink wal and wal-index files

    // If an EXCLUSIVE lock can be obtained on the database file (using the
    // ordinary, rollback-mode locking methods, this guarantees that the
    // connection associated with this log file is the only connection to
    // the database. In this case checkpoint the database and unlink both
    // the wal and wal-index files.
    //
    // The EXCLUSIVE lock is not released before returning.
    if let Some(db) = db {
        if !zBuf.is_empty() && {
            rc = sqlite3OsLock(pDbFd, SQLITE_LOCK_EXCLUSIVE);
            rc == SQLITE_OK
        } {
            if pWal.exclusiveMode == WAL_NORMAL_MODE {
                pWal.exclusiveMode = WAL_EXCLUSIVE_MODE;
            }
            let (mut nLog, mut nCkpt) = (0, 0);
            rc = sqlite3WalCheckpoint(
                db,
                pWal,
                pDbFd,
                SQLITE_CHECKPOINT_PASSIVE,
                None,
                sync_flags,
                nBuf,
                zBuf,
                &mut nLog,
                &mut nCkpt,
            );
            if rc == SQLITE_OK {
                let mut bPersist = -1;
                sqlite3OsFileControlHint(
                    pDbFd,
                    SQLITE_FCNTL_PERSIST_WAL,
                    FcntlArg::Int(&mut bPersist),
                );
                if bPersist != 1 {
                    // Try to delete the WAL file if the checkpoint
                    // completed and fsynced (rc==SQLITE_OK) and if we are
                    // not in persistent-wal mode (!bPersist)
                    isDelete = 1;
                } else if pWal.mxWalSize >= 0 {
                    // Try to truncate the WAL file to zero bytes if the
                    // checkpoint completed and fsynced (rc==SQLITE_OK) and
                    // we are in persistent WAL mode (bPersist) and if the
                    // PRAGMA journal_size_limit is a non-negative value
                    // (pWal->mxWalSize>=0).  Note that we truncate to zero
                    // bytes as truncating to the journal_size_limit might
                    // leave a corrupt WAL file on disk.
                    walLimitSize(pWal, 0);
                }
            }
        }
    }

    walIndexClose(pWal, isDelete);
    sqlite3OsClose(&mut pWal.pWalFd);
    if isDelete != 0 {
        sqlite3OsDelete(&pWal.pVfs, &pWal.zWalName, 0);
    }
    pWal.apWiData.clear();
    pWal.pShm = None;
    rc
}

/// Try to read the wal-index header.  Return 0 on success and 1 if there
/// is a problem.
///
/// The wal-index is in shared memory.  Another thread or process might be
/// writing the header at the same time this procedure is trying to read
/// it, which might result in inconsistency.  A dirty read is detected by
/// verifying that both copies of the header are the same and also by a
/// checksum on the header.
///
/// If and only if the read is consistent and the header is different from
/// pWal->hdr, then pWal->hdr is updated to the content of the new header
/// and *pChanged is set to 1.
///
/// If the checksum cannot be verified return non-zero. If the header is
/// read successfully and the checksum verified, return zero.
fn walIndexTryHdr(pWal: &mut Wal, pChanged: &mut i32) -> i32 {
    // The first page of the wal-index must be mapped at this point.
    debug_assert!(!pWal.apWiData.is_empty() && pWal.apWiData[0].is_some());

    // Read the header. This might happen concurrently with a write to the
    // same area of shared memory on a different CPU in a SMP, meaning it
    // is possible that an inconsistent snapshot is read from the file. If
    // this happens, return non-zero.
    //
    // There are two copies of the header at the beginning of the
    // wal-index.  When reading, read [0] first then [1].  Writes are in
    // the reverse order.  Memory barriers are used to prevent the compiler
    // or the hardware from reordering the reads and writes.
    let h1 = pWal.live_hdr(0);
    walShmBarrier(pWal);
    let h2 = pWal.live_hdr(1);

    if h1 != h2 {
        return 1; // Dirty read
    }
    if h1.isInit == 0 {
        return 1; // Malformed header - probably all zeros
    }
    let w = h1.to_words();
    let aCksum = walChecksumWords(&w[..HDR_NWORD - 2]);
    if aCksum[0] != h1.aCksum[0] || aCksum[1] != h1.aCksum[1] {
        return 1; // Checksum does not match
    }

    if pWal.hdr != h1 {
        *pChanged = 1;
        pWal.hdr = h1;
        pWal.szPage =
            ((pWal.hdr.szPage as u32) & 0xfe00) + (((pWal.hdr.szPage as u32) & 0x0001) << 16);
    }

    // The header was successfully read. Return zero.
    0
}

/// Read the wal-index header from the wal-index and into pWal->hdr.  If
/// the wal-header appears to be corrupt, try to reconstruct the wal-index
/// from the WAL before returning.
///
/// Set *pChanged to 1 if the wal-index header value in pWal->hdr is
/// changed by this operation.  If pWal->hdr is unchanged, set *pChanged to
/// 0.
///
/// If the wal-index header is successfully read, return SQLITE_OK.
/// Otherwise an SQLite error code.
fn walIndexReadHdr(pWal: &mut Wal, pChanged: &mut i32) -> i32 {
    let mut page0: Option<ShmPage> = None; // Chunk of wal-index containing header

    // Ensure that page 0 of the wal-index (the page that contains the
    // wal-index header) is mapped. Return early if an error occurs here.
    let mut rc = walIndexPage(pWal, 0, &mut page0);
    if rc != SQLITE_OK {
        debug_assert!(rc != SQLITE_READONLY); // READONLY changed to OK in walIndexPage
        if rc == SQLITE_READONLY_CANTINIT {
            // The SQLITE_READONLY_CANTINIT return means that the
            // shared-memory was openable but is not writable, and this
            // thread is unable to confirm that another write-capable
            // connection has the shared-memory open, and hence the content
            // of the shared-memory is unreliable, since the shared-memory
            // might be inconsistent with the WAL file and there is no
            // writer on hand to fix it.
            debug_assert!(page0.is_none());
            debug_assert!(pWal.writeLock == 0);
            debug_assert!((pWal.readOnly & WAL_SHM_RDONLY) != 0);
            pWal.bShmUnreliable = 1;
            pWal.exclusiveMode = WAL_HEAPMEMORY_MODE;
            *pChanged = 1;
        } else {
            return rc; // Any other non-OK return is just an error
        }
    }
    // page0 can be NULL if the SHM is zero bytes in size and
    // pWal->writeLock is zero, which prevents the SHM from growing
    debug_assert!(page0.is_some() || pWal.writeLock == 0);

    // If the first page of the wal-index has been mapped, try to read the
    // wal-index header immediately, without holding any lock. This usually
    // works, but may fail if the wal-index header is corrupt or currently
    // being modified by another thread or process.
    let mut badHdr = if page0.is_some() {
        walIndexTryHdr(pWal, pChanged)
    } else {
        1
    };

    // If the first attempt failed, it might have been due to a race with a
    // writer.  So get a WRITE lock and try again.
    if badHdr != 0 {
        if pWal.bShmUnreliable == 0 && (pWal.readOnly & WAL_SHM_RDONLY) != 0 {
            rc = walLockShared(pWal, WAL_WRITE_LOCK);
            if rc == SQLITE_OK {
                walUnlockShared(pWal, WAL_WRITE_LOCK);
                rc = SQLITE_READONLY_RECOVERY;
            }
        } else {
            let bWriteLock = pWal.writeLock;
            if bWriteLock != 0 || {
                rc = walLockExclusive(pWal, WAL_WRITE_LOCK, 1);
                rc == SQLITE_OK
            } {
                // If the write-lock was just obtained, set writeLock to 2
                // instead of the usual 1. This causes walIndexPage() to
                // behave as if the write-lock were held (so that it
                // allocates new pages as required).
                if bWriteLock == 0 {
                    pWal.writeLock = 2;
                }
                rc = walIndexPage(pWal, 0, &mut page0);
                if rc == SQLITE_OK {
                    badHdr = walIndexTryHdr(pWal, pChanged);
                    if badHdr != 0 {
                        // If the wal-index header is still malformed even
                        // while holding a WRITE lock, it can only mean that
                        // the header is corrupted and needs to be
                        // reconstructed.  So run recovery to do exactly
                        // that.
                        rc = walIndexRecover(pWal);
                        *pChanged = 1;
                    }
                }
                if bWriteLock == 0 {
                    pWal.writeLock = 0;
                    walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
                }
            }
        }
    }

    // If the header is read successfully, check the version number to make
    // sure the wal-index was not constructed with some future format that
    // this version of SQLite cannot understand.
    if badHdr == 0 && pWal.hdr.iVersion != WALINDEX_MAX_VERSION {
        rc = SQLITE_CANTOPEN;
    }
    if pWal.bShmUnreliable != 0 {
        if rc != SQLITE_OK {
            walIndexClose(pWal, 0);
            pWal.bShmUnreliable = 0;
            debug_assert!(!pWal.apWiData.is_empty() && pWal.apWiData[0].is_none());
            // walIndexRecover() might have returned SHORT_READ if a
            // concurrent writer truncated the WAL out from under it.  If
            // that happens, it indicates that a writer has fixed the SHM
            // file for us, so retry
            if rc == SQLITE_IOERR_SHORT_READ {
                rc = WAL_RETRY;
            }
        }
        pWal.exclusiveMode = WAL_NORMAL_MODE;
    }

    rc
}

/// Open a transaction in a connection where the shared-memory is
/// read-only and where we cannot verify that there is a separate
/// write-capable connection on hand to keep the shared-memory up-to-date
/// with the WAL file.
///
/// This can happen, for example, when the shared-memory is implemented by
/// memory-mapping a *-shm file, where a prior writer has shut down and
/// left the *-shm file on disk, and now the present connection is trying
/// to use that database but lacks write permission on the *-shm file.
///
/// Precondition:
///
///    The *-wal file has been read and an appropriate wal-index has been
///    constructed in pWal->apWiData[] using heap memory instead of shared
///    memory.
///
/// If this function returns SQLITE_OK, then the read transaction has been
/// successfully opened. In this case output variable (*pChanged) is set
/// to true before returning if the caller should discard the contents of
/// the page cache before proceeding. Or, if it returns WAL_RETRY, then the
/// heap memory wal-index has been discarded and the caller should retry
/// opening the read transaction from the beginning (including attempting
/// to map the *-shm file).
///
/// If an error occurs, an SQLite error code is returned.
fn walBeginShmUnreliable(pWal: &mut Wal, pChanged: &mut i32) -> i32 {
    let mut rc;
    let mut szWal: i64 = 0; // Size of wal file on disk in bytes
    let mut aBuf = [0u8; WAL_HDRSIZE]; // Buffer to load WAL header into

    debug_assert!(pWal.bShmUnreliable != 0);
    debug_assert!((pWal.readOnly & WAL_SHM_RDONLY) != 0);
    debug_assert!(!pWal.apWiData.is_empty() && pWal.apWiData[0].is_some());

    'begin_unreliable_shm_out: {
        // Take WAL_READ_LOCK(0). This has the effect of preventing any
        // writers from running a checkpoint, but does not stop them from
        // running recovery.
        rc = walLockShared(pWal, WAL_READ_LOCK(0));
        if rc != SQLITE_OK {
            if rc == SQLITE_BUSY {
                rc = WAL_RETRY;
            }
            break 'begin_unreliable_shm_out;
        }
        pWal.readLock = 0;

        // Check to see if a separate writer has attached to the
        // shared-memory area, thus making the shared-memory "reliable"
        // again.  Do this by invoking the xShmMap() routine of the VFS and
        // looking to see if the return is SQLITE_READONLY instead of
        // SQLITE_READONLY_CANTINIT.
        //
        // If the shared-memory is now "reliable" return WAL_RETRY, which
        // will cause the heap-memory WAL-index to be discarded and the
        // actual shared memory to be used in its place.
        //
        // This step is important because, even though this connection is
        // holding the WAL_READ_LOCK(0) which prevents a checkpoint, a
        // writer might have already checkpointed the WAL file and, while
        // the current is active, wrap the WAL and start overwriting frames
        // that this process wants to use.
        let mut pDummy: Option<ShmPage> = None;
        rc = match &pWal.pShm {
            Some(h) => sqlite3OsShmMap(h, 0, WALINDEX_PGSZ as i32, 0, &mut pDummy),
            None => SQLITE_IOERR_SHMMAP,
        };
        debug_assert!(rc != SQLITE_OK); // SQLITE_OK not possible for read-only connection
        if rc != SQLITE_READONLY_CANTINIT {
            rc = if rc == SQLITE_READONLY { WAL_RETRY } else { rc };
            break 'begin_unreliable_shm_out;
        }

        // We reach this point only if the real shared-memory is still
        // unreliable.  Assume the in-memory WAL-index substitute is correct
        // and load it into pWal->hdr.
        pWal.hdr = pWal.live_hdr(0);

        // Make sure some writer hasn't come in and changed the WAL file out
        // from under us, then disconnected, while we were not looking.
        rc = sqlite3OsFileSize(&mut pWal.pWalFd, &mut szWal);
        if rc != SQLITE_OK {
            break 'begin_unreliable_shm_out;
        }
        if szWal < WAL_HDRSIZE as i64 {
            // If the wal file is too small to contain a wal-header and the
            // wal-index header has mxFrame==0, then it must be safe to
            // proceed reading the database file only. However, the page
            // cache cannot be trusted, as a read/write connection may have
            // connected, written the db, run a checkpoint, truncated the
            // wal file and disconnected since this client's last read
            // transaction.
            *pChanged = 1;
            rc = if pWal.hdr.mxFrame == 0 {
                SQLITE_OK
            } else {
                WAL_RETRY
            };
            break 'begin_unreliable_shm_out;
        }

        // Check the salt keys at the start of the wal file still match.
        rc = sqlite3OsRead(&mut pWal.pWalFd, &mut aBuf, WAL_HDRSIZE as i32, 0);
        if rc != SQLITE_OK {
            break 'begin_unreliable_shm_out;
        }
        if pWal.hdr.aSalt[..] != aBuf[16..24] {
            // Some writer has wrapped the WAL file while we were not
            // looking.  Return WAL_RETRY which will cause the in-memory
            // WAL-index to be rebuilt.
            rc = WAL_RETRY;
            break 'begin_unreliable_shm_out;
        }

        // Allocate a buffer to read frames into
        debug_assert!((pWal.szPage & (pWal.szPage.wrapping_sub(1))) == 0);
        let szFrame = pWal.szPage as usize + WAL_FRAME_HDRSIZE;
        let mut aFrame = vec![0u8; szFrame];

        // Check to see if a complete transaction has been appended to the
        // wal file since the heap-memory wal-index was created. If so, the
        // heap-memory wal-index is discarded and WAL_RETRY returned to the
        // caller.
        let aSaveCksum = pWal.hdr.aFrameCksum;
        let mut iOffset = walFrameOffset(pWal.hdr.mxFrame + 1, pWal.szPage as i64);
        while iOffset + szFrame as i64 <= szWal {
            let mut pgno: u32 = 0; // Database page number for frame
            let mut nTruncate: u32 = 0; // dbsize field from frame header

            // Read and decode the next log frame.
            rc = sqlite3OsRead(&mut pWal.pWalFd, &mut aFrame, szFrame as i32, iOffset);
            if rc != SQLITE_OK {
                break;
            }
            let (h, d) = aFrame.split_at(WAL_FRAME_HDRSIZE);
            if !walDecodeFrame(pWal, &mut pgno, &mut nTruncate, d, h) {
                break;
            }

            // If nTruncate is non-zero, then a complete transaction has
            // been appended to this wal file. Set rc to WAL_RETRY and break
            // out of the loop.
            if nTruncate != 0 {
                rc = WAL_RETRY;
                break;
            }
            iOffset += szFrame as i64;
        }
        pWal.hdr.aFrameCksum = aSaveCksum;
    }

    // begin_unreliable_shm_out:
    if rc != SQLITE_OK {
        for p in pWal.apWiData.iter_mut() {
            *p = None;
        }
        pWal.bShmUnreliable = 0;
        sqlite3WalEndReadTransaction(pWal);
        *pChanged = 1;
    }
    rc
}

/// Attempt to start a read transaction.  This might fail due to a race or
/// other transient condition.  When that happens, it returns WAL_RETRY to
/// indicate to the caller that it is safe to retry immediately.
///
/// On success return SQLITE_OK.  On a permanent failure (such an I/O error
/// or an SQLITE_BUSY because another process is running recovery) return
/// a positive error code.
///
/// The useWal parameter is true to force the use of the WAL and disable
/// the case where the WAL is bypassed because it has been completely
/// checkpointed.  If useWal==0 then this routine calls walIndexReadHdr()
/// to make a copy of the wal-index header into pWal->hdr.  If the
/// wal-index header has changed, *pChanged is set to 1 (as an indication
/// to the caller that the local page cache is obsolete and needs to be
/// flushed.)  When useWal==1, the wal-index header is assumed to already
/// be loaded and the pChanged parameter is unused.
///
/// The caller must set the cnt parameter to the number of prior calls to
/// this routine during the current read attempt that returned WAL_RETRY.
/// This routine will start taking more aggressive measures to clear the
/// race conditions after multiple WAL_RETRY returns, and after an
/// excessive number of errors will ultimately return SQLITE_PROTOCOL.  The
/// SQLITE_PROTOCOL return indicates that some other process has gone rogue
/// and is not honoring the locking protocol.
///
/// On success, this routine obtains a read lock on
/// WAL_READ_LOCK(pWal->readLock).  The pWal->readLock integer is in the
/// range 0 <= pWal->readLock < WAL_NREADER.  If pWal->readLock==(-1) that
/// means the Wal does not hold any read lock.  The reader must not access
/// any database page that is modified by a WAL frame up to and including
/// frame number aReadMark[pWal->readLock].  The reader will use WAL frames
/// up to and including pWal->hdr.mxFrame if pWal->readLock>0 Or if
/// pWal->readLock==0, then the reader will ignore the WAL completely and
/// get all content directly from the database file.
///
/// If the useWal parameter is 1 then the WAL will never be ignored and
/// this routine will always set pWal->readLock>0 on success.
///
/// When the read transaction is completed, the caller must release the
/// lock on WAL_READ_LOCK(pWal->readLock) and set pWal->readLock to -1.
///
/// This routine uses the nBackfill and aReadMark[] fields of the header to
/// select a particular WAL_READ_LOCK() that strives to let the checkpoint
/// process do as much work as possible.  This routine might update values
/// of the aReadMark[] array in the header, but if it does so it takes care
/// to hold an exclusive lock on the corresponding WAL_READ_LOCK() while
/// changing values.
fn walTryBeginRead(pWal: &mut Wal, pChanged: &mut i32, useWal: i32, pCnt: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;

    debug_assert!(pWal.readLock < 0); // Not currently locked

    // useWal may only be set for read/write connections
    debug_assert!((pWal.readOnly & WAL_SHM_RDONLY) == 0 || useWal == 0);

    // Take steps to avoid spinning forever if there is a protocol error.
    //
    // Circumstances that cause a RETRY should only last for the briefest
    // instances of time.  No I/O or other system calls are done while the
    // locks are held, so the locks should not be held for very long. But
    // if we are unlucky, another process that is holding a lock might get
    // paged out or take a page-fault that is time-consuming to resolve,
    // during the few nanoseconds that it is holding the lock.  In that
    // case, it might take longer than normal for the lock to free.
    //
    // After 5 RETRYs, we begin calling sqlite3OsSleep().  The first few
    // calls to sqlite3OsSleep() have a delay of 1 microsecond.  Really this
    // is more of a scheduler yield than an actual delay.  But on the 10th
    // an subsequent retries, the delays start becoming longer and longer,
    // so that on the 100th (and last) RETRY we delay for 323 milliseconds.
    // The total delay time before giving up is less than 10 seconds.
    *pCnt += 1;
    if *pCnt > 5 {
        let mut nDelay = 1; // Pause time in microseconds
        let cnt = *pCnt & !WAL_RETRY_BLOCKED_MASK;
        if cnt > WAL_RETRY_PROTOCOL_LIMIT {
            pWal.lockError = 1;
            return SQLITE_PROTOCOL;
        }
        if *pCnt >= 10 {
            nDelay = (cnt - 9) * (cnt - 9) * 39;
        }
        sqlite3OsSleep(&pWal.pVfs, nDelay);
        *pCnt &= !WAL_RETRY_BLOCKED_MASK;
    }

    if useWal == 0 {
        debug_assert!(rc == SQLITE_OK);
        if pWal.bShmUnreliable == 0 {
            rc = walIndexReadHdr(pWal, pChanged);
        }
        if rc == SQLITE_BUSY {
            // If there is not a recovery running in another thread or
            // process then convert BUSY errors to WAL_RETRY.  If recovery
            // is known to be running, convert BUSY to BUSY_RECOVERY.  There
            // is a race here which might cause WAL_RETRY to be returned
            // even if BUSY_RECOVERY would be technically correct.  But the
            // race is benign since with WAL_RETRY this routine will be
            // called again and will probably be right on the second
            // iteration.
            if pWal.apWiData.first().is_none_or(|p| p.is_none()) {
                // This branch is taken when the xShmMap() method returns
                // SQLITE_BUSY.  We assume this is a transient condition, so
                // return WAL_RETRY. The xShmMap() implementation used by the
                // default unix and win32 VFS modules may return SQLITE_BUSY
                // due to a race condition in the code that determines
                // whether or not the shared-memory region must be zeroed
                // before the requested page is returned.
                rc = WAL_RETRY;
            } else {
                rc = walLockShared(pWal, WAL_RECOVER_LOCK);
                if rc == SQLITE_OK {
                    walUnlockShared(pWal, WAL_RECOVER_LOCK);
                    rc = WAL_RETRY;
                } else if rc == SQLITE_BUSY {
                    rc = SQLITE_BUSY_RECOVERY;
                }
            }
        }
        if rc != SQLITE_OK {
            return rc;
        } else if pWal.bShmUnreliable != 0 {
            return walBeginShmUnreliable(pWal, pChanged);
        }
    }

    debug_assert!(!pWal.apWiData.is_empty());
    debug_assert!(pWal.apWiData[0].is_some());
    {
        if useWal == 0 && pWal.ckpt_get(CKPT_NBACKFILL) == pWal.hdr.mxFrame {
            // The WAL has been completely backfilled (or it is empty).  and
            // can be safely ignored.
            rc = walLockShared(pWal, WAL_READ_LOCK(0));
            walShmBarrier(pWal);
            if rc == SQLITE_OK {
                if pWal.live_hdr(0) != pWal.hdr {
                    // It is not safe to allow the reader to continue here if
                    // frames may have been appended to the log before
                    // READ_LOCK(0) was obtained.  When holding READ_LOCK(0),
                    // the reader ignores the entire log file, which implies
                    // that the database file contains a trustworthy
                    // snapshot. Since holding READ_LOCK(0) prevents a
                    // checkpoint from happening, this is usually correct.
                    //
                    // However, if frames have been appended to the log (or
                    // if the log is wrapped and written for that matter)
                    // before the READ_LOCK(0) is obtained, that is not
                    // necessarily true. A checkpointer may have started to
                    // backfill the appended frames but crashed before it
                    // finished. Leaving a corrupt image in the database
                    // file.
                    walUnlockShared(pWal, WAL_READ_LOCK(0));
                    return WAL_RETRY;
                }
                pWal.readLock = 0;
                return SQLITE_OK;
            } else if rc != SQLITE_BUSY {
                return rc;
            }
        }

        // If we get this far, it means that the reader will want to use the
        // WAL to get at content from recent commits.  The job now is to
        // select one of the aReadMark[] entries that is closest to but not
        // exceeding pWal->hdr.mxFrame and lock that entry.
        let mut mxReadMark: u32 = 0; // Largest aReadMark[] value
        let mut mxI: i32 = 0; // Index of largest aReadMark[] value
        let mxFrame = pWal.hdr.mxFrame; // Wal frame to lock to
        for i in 1..WAL_NREADER {
            let thisMark = pWal.ckpt_get(CKPT_READMARK + i as usize);
            if mxReadMark <= thisMark && thisMark <= mxFrame {
                debug_assert!(thisMark != READMARK_NOT_USED);
                mxReadMark = thisMark;
                mxI = i;
            }
        }
        if (pWal.readOnly & WAL_SHM_RDONLY) == 0 && (mxReadMark < mxFrame || mxI == 0) {
            for i in 1..WAL_NREADER {
                rc = walLockExclusive(pWal, WAL_READ_LOCK(i), 1);
                if rc == SQLITE_OK {
                    pWal.ckpt_set(CKPT_READMARK + i as usize, mxFrame);
                    mxReadMark = mxFrame;
                    mxI = i;
                    walUnlockExclusive(pWal, WAL_READ_LOCK(i), 1);
                    break;
                } else if rc != SQLITE_BUSY {
                    return rc;
                }
            }
        }
        if mxI == 0 {
            debug_assert!(rc == SQLITE_BUSY || (pWal.readOnly & WAL_SHM_RDONLY) != 0);
            return if rc == SQLITE_BUSY {
                WAL_RETRY
            } else {
                SQLITE_READONLY_CANTINIT
            };
        }

        rc = walLockShared(pWal, WAL_READ_LOCK(mxI));
        if rc != 0 {
            return if (rc & 0xFF) == SQLITE_BUSY {
                WAL_RETRY
            } else {
                rc
            };
        }
        // Now that the read-lock has been obtained, check that neither the
        // value in the aReadMark[] array or the contents of the wal-index
        // header have changed.
        //
        // It is necessary to check that the wal-index header did not
        // change between the time it was read and when the shared-lock was
        // obtained on WAL_READ_LOCK(mxI) was obtained to account for the
        // possibility that the log file may have been wrapped by a writer,
        // or that frames that occur later in the log than
        // pWal->hdr.mxFrame may have been copied into the database by a
        // checkpointer. If either of these things happened, then reading
        // the database with the current value of pWal->hdr.mxFrame risks
        // reading a corrupted snapshot. So, retry instead.
        //
        // Before checking that the live wal-index header has not changed
        // since it was read, set Wal.minFrame to the first frame in the wal
        // file that has not yet been checkpointed. This client will not
        // need to read any frames earlier than minFrame from the wal file -
        // they can be safely read directly from the database file.
        //
        // Because a ShmBarrier() call is made between taking the copy of
        // nBackfill and checking that the wal-header in shared-memory still
        // matches the one cached in pWal->hdr, it is guaranteed that the
        // checkpointer that set nBackfill was not working with a wal-index
        // header newer than that cached in pWal->hdr.
        pWal.minFrame = pWal.ckpt_get(CKPT_NBACKFILL).wrapping_add(1);
        walShmBarrier(pWal);
        if pWal.ckpt_get(CKPT_READMARK + mxI as usize) != mxReadMark || pWal.live_hdr(0) != pWal.hdr
        {
            walUnlockShared(pWal, WAL_READ_LOCK(mxI));
            return WAL_RETRY;
        } else {
            debug_assert!(mxReadMark <= pWal.hdr.mxFrame);
            pWal.readLock = mxI as i16;
        }
    }
    rc
}

/// C: `int sqlite3WalSnapshotRecover(Wal *pWal)` (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3WalSnapshotRecover(pWal: &mut crate::wal::Wal) -> i32 {
    let _ = pWal;
    SQLITE_ERROR
}

/// C: `int sqlite3WalBeginReadTransaction(Wal *pWal, int *pChanged)`
///
/// Begin a read transaction on the database.
///
/// This routine used to be called sqlite3OpenSnapshot() and with good
/// reason: it takes a snapshot of the state of the WAL and wal-index for
/// the current instant in time.  The current thread will continue to use
/// this snapshot.  Other threads might append new content to the WAL and
/// wal-index but that extra content is ignored by the current thread.
///
/// If the database contents have changes since the previous read
/// transaction, then *pChanged is set to 1 before returning.  The Pager
/// layer will use this to know that its cache is stale and needs to be
/// flushed.
pub fn sqlite3WalBeginReadTransaction(pWal: &mut crate::wal::Wal, pChanged: &mut i32) -> i32 {
    let mut cnt = 0; // Number of TryBeginRead attempts
    debug_assert!(pWal.ckptLock == 0);
    loop {
        let rc = walTryBeginRead(pWal, pChanged, 0, &mut cnt);
        if rc != WAL_RETRY {
            return rc;
        }
    }
}

/// C: `void sqlite3WalEndReadTransaction(Wal *pWal)`
///
/// Finish with a read transaction.  All this does is release the
/// read-lock.
pub fn sqlite3WalEndReadTransaction(pWal: &mut crate::wal::Wal) {
    debug_assert!(pWal.writeLock == 0 || pWal.readLock < 0);
    if pWal.readLock >= 0 {
        let _ = sqlite3WalEndWriteTransaction(pWal);
        walUnlockShared(pWal, WAL_READ_LOCK(pWal.readLock as i32));
        pWal.readLock = -1;
    }
}

/// Search the wal file for page pgno. If found, set *piRead to the frame
/// that contains the page. Otherwise, if pgno is not in the wal file, set
/// *piRead to zero.
///
/// Return SQLITE_OK if successful, or an error code if an error occurs. If
/// an error does occur, the final value of *piRead is undefined.
fn walFindFrame(pWal: &mut Wal, pgno: Pgno, piRead: &mut u32) -> i32 {
    let mut iRead: u32 = 0; // If !=0, WAL frame to return data from
    let iLast = pWal.hdr.mxFrame; // Last page in WAL for this reader

    // This routine is only be called from within a read transaction.
    debug_assert!(pWal.readLock >= 0 || pWal.lockError != 0);

    // If the "last page" field of the wal-index header snapshot is 0, then
    // no data will be read from the wal under any circumstances. Return
    // early in this case as an optimization.  Likewise, if
    // pWal->readLock==0, then the WAL is ignored by the reader so return
    // early, as if the WAL were empty.
    if iLast == 0 || (pWal.readLock == 0 && pWal.bShmUnreliable == 0) {
        *piRead = 0;
        return SQLITE_OK;
    }

    // Search the hash table or tables for an entry matching page number
    // pgno. Each iteration of the following for() loop searches one hash
    // table (each hash table indexes up to HASHTABLE_NPAGE frames).
    //
    // This code might run concurrently to the code in walIndexAppend()
    // that adds entries to the wal-index (and possibly to this hash
    // table). This means the value just read from the hash slot
    // (aHash[iKey]) may have been added before or after the current read
    // transaction was opened. Values added after the read transaction was
    // opened may have been written incorrectly - i.e. these slots may
    // contain garbage data. However, we assume that any slots written
    // before the current read transaction was opened remain unmodified.
    //
    // For the reasons above, the if(...) condition featured in the inner
    // loop of the following block is more stringent that would be required
    // if we had exclusive access to the hash-table:
    //
    //   (aPgno[iFrame]==pgno):
    //     This condition filters out normal hash-table collisions.
    //
    //   (iFrame<=iLast):
    //     This condition filters out entries that were added to the hash
    //     table after the current read-transaction had started.
    let iMinHash = walFramePage(pWal.minFrame);
    let mut iHash = walFramePage(iLast) as isize;
    while iHash >= iMinHash as isize {
        let mut sLoc = None;
        let rc = walHashGet(pWal, iHash as usize, &mut sLoc);
        if rc != SQLITE_OK {
            return rc;
        }
        let sLoc = sLoc.unwrap();
        let mut nCollide = HASHTABLE_NSLOT; // Number of hash collisions remaining
        let mut iKey = walHash(pgno);
        loop {
            let iH = sLoc.hash(iKey);
            if iH == 0 {
                break;
            }
            let iFrame = iH + sLoc.iZero;
            if iFrame <= iLast
                && iFrame >= pWal.minFrame
                && sLoc.pgno(((iH - 1) as usize) & (HASHTABLE_NPAGE - 1)) == pgno
            {
                iRead = iFrame;
            }
            if nCollide == 0 {
                *piRead = 0;
                return SQLITE_CORRUPT_BKPT();
            }
            nCollide -= 1;
            iKey = walNextHash(iKey);
        }
        if iRead != 0 {
            break;
        }
        iHash -= 1;
    }

    *piRead = iRead;
    SQLITE_OK
}

/// C: `int sqlite3WalFindFrame( Wal *pWal, Pgno pgno, u32 *piRead )`
pub fn sqlite3WalFindFrame(pWal: &mut crate::wal::Wal, pgno: Pgno, piRead: &mut u32) -> i32 {
    walFindFrame(pWal, pgno, piRead)
}

/// C: `int sqlite3WalReadFrame( Wal *pWal, u32 iRead, int nOut, u8 *pOut )`
///
/// Read the contents of frame iRead from the wal file into buffer pOut
/// (which is nOut bytes in size). Return SQLITE_OK if successful, or an
/// error code otherwise.
pub fn sqlite3WalReadFrame(
    pWal: &mut crate::wal::Wal,
    iRead: u32,
    nOut: i32,
    pOut: &mut [u8],
) -> i32 {
    let mut nOut = nOut;
    let mut sz = pWal.hdr.szPage as i32;
    sz = (sz & 0xfe00) + ((sz & 0x0001) << 16);
    if nOut > sz {
        pOut[sz as usize..nOut as usize].fill(0);
        nOut = sz;
    }
    let iOffset = walFrameOffset(iRead, sz as i64) + WAL_FRAME_HDRSIZE as i64;
    sqlite3OsRead(&mut pWal.pWalFd, pOut, nOut, iOffset)
}

/// C: `Pgno sqlite3WalDbsize(Wal *pWal)`
///
/// Return the size of the database in pages (or zero, if unknown).
pub fn sqlite3WalDbsize(pWal: &mut crate::wal::Wal) -> Pgno {
    if pWal.readLock >= 0 {
        return pWal.hdr.nPage;
    }
    0
}

/// C: `int sqlite3WalBeginWriteTransaction(Wal *pWal)`
///
/// This function starts a write transaction on the WAL.
///
/// A read transaction must have already been started by a prior call to
/// sqlite3WalBeginReadTransaction().
///
/// If another thread or process has written into the database since the
/// read transaction was started, then it is not possible for this thread
/// to write as doing so would cause a fork.  So this routine returns
/// SQLITE_BUSY in that case and no write transaction is started.
///
/// There can only be a single writer active at a time.
pub fn sqlite3WalBeginWriteTransaction(pWal: &mut crate::wal::Wal) -> i32 {
    // Cannot start a write transaction without first holding a read
    // transaction.
    debug_assert!(pWal.readLock >= 0);
    debug_assert!(pWal.writeLock == 0 && pWal.iReCksum == 0);

    if pWal.readOnly != 0 {
        return SQLITE_READONLY;
    }

    // Only one writer allowed at a time.  Get the write lock.  Return
    // SQLITE_BUSY if unable.
    let mut rc = walLockExclusive(pWal, WAL_WRITE_LOCK, 1);
    if rc != 0 {
        return rc;
    }
    pWal.writeLock = 1;

    // If another connection has written to the database file since the
    // time the read transaction on this connection was started, then the
    // write is disallowed.
    if pWal.hdr != pWal.live_hdr(0) {
        rc = SQLITE_BUSY_SNAPSHOT;
    }

    if rc != SQLITE_OK {
        walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
        pWal.writeLock = 0;
    }
    rc
}

/// C: `int sqlite3WalEndWriteTransaction(Wal *pWal)`
///
/// End a write transaction.  The commit has already been done.  This
/// routine merely releases the lock.
pub fn sqlite3WalEndWriteTransaction(pWal: &mut crate::wal::Wal) -> i32 {
    if pWal.writeLock != 0 {
        walUnlockExclusive(pWal, WAL_WRITE_LOCK, 1);
        pWal.writeLock = 0;
        pWal.iReCksum = 0;
        pWal.truncateOnCommit = 0;
    }
    SQLITE_OK
}

/// C: `int sqlite3WalUndo(Wal *pWal, int (*xUndo)(void *, Pgno), void *pUndoCtx)`
///
/// If any data has been written (but not committed) to the log file, this
/// function moves the write-pointer back to the start of the transaction.
///
/// Additionally, the callback function is invoked for each frame written
/// to the WAL since the start of the transaction. If the callback returns
/// other than SQLITE_OK, it is not invoked again and the error code is
/// returned to the caller.
///
/// Otherwise, if the callback function does not return an error, this
/// function returns SQLITE_OK.
pub fn sqlite3WalUndo(pWal: &mut crate::wal::Wal, xUndo: &mut dyn FnMut(Pgno) -> i32) -> i32 {
    let mut rc = SQLITE_OK;
    if pWal.writeLock != 0 {
        let iMax = pWal.hdr.mxFrame;

        // Restore the clients cache of the wal-index header to the state it
        // was in before the client began writing to the database.
        pWal.hdr = pWal.live_hdr(0);

        let mut iFrame = pWal.hdr.mxFrame + 1;
        while rc == SQLITE_OK && iFrame <= iMax {
            // This call cannot fail. Unless the page for which the page
            // number is passed as the second argument is (a) in the cache
            // and (b) has an outstanding reference, then xUndo is either a
            // no-op (if (a) is false) or simply expels the page from the
            // cache (if (b) is false).
            //
            // If the upper layer is doing a rollback, it is guaranteed that
            // there are no outstanding references to any page other than
            // page 1. And page 1 is never written to the log until the
            // transaction is committed. As a result, the call to xUndo may
            // not fail.
            debug_assert!(walFramePgno(pWal, iFrame) != 1);
            rc = xUndo(walFramePgno(pWal, iFrame));
            iFrame += 1;
        }
        if iMax != pWal.hdr.mxFrame {
            walCleanupHash(pWal);
        }
        pWal.iReCksum = 0;
    }
    rc
}

/// C: `void sqlite3WalSavepoint(Wal *pWal, u32 *aWalData)`
///
/// Argument aWalData must point to an array of WAL_SAVEPOINT_NDATA u32
/// values. This function populates the array with values required to
/// "rollback" the write position of the WAL handle back to the current
/// point in the event of a savepoint rollback (via WalSavepointUndo()).
pub fn sqlite3WalSavepoint(pWal: &mut crate::wal::Wal, aWalData: &mut [u32]) {
    debug_assert!(pWal.writeLock != 0);
    aWalData[0] = pWal.hdr.mxFrame;
    aWalData[1] = pWal.hdr.aFrameCksum[0];
    aWalData[2] = pWal.hdr.aFrameCksum[1];
    aWalData[3] = pWal.nCkpt;
}

/// C: `int sqlite3WalSavepointUndo(Wal *pWal, u32 *aWalData)`
///
/// Move the write position of the WAL back to the point identified by the
/// values in the aWalData[] array. aWalData must point to an array of
/// WAL_SAVEPOINT_NDATA u32 values that has been previously populated by a
/// call to WalSavepoint().
pub fn sqlite3WalSavepointUndo(pWal: &mut crate::wal::Wal, aWalData: &mut [u32]) -> i32 {
    debug_assert!(pWal.writeLock != 0);
    debug_assert!(aWalData[3] != pWal.nCkpt || aWalData[0] <= pWal.hdr.mxFrame);

    if aWalData[3] != pWal.nCkpt {
        // This savepoint was opened immediately after the write-transaction
        // was started. Right after that, the writer decided to wrap around
        // to the start of the log. Update the savepoint values to match.
        aWalData[0] = 0;
        aWalData[3] = pWal.nCkpt;
    }

    if aWalData[0] < pWal.hdr.mxFrame {
        pWal.hdr.mxFrame = aWalData[0];
        pWal.hdr.aFrameCksum[0] = aWalData[1];
        pWal.hdr.aFrameCksum[1] = aWalData[2];
        walCleanupHash(pWal);
        if pWal.iReCksum > pWal.hdr.mxFrame {
            pWal.iReCksum = 0;
        }
    }
    SQLITE_OK
}

/// This function is called just before writing a set of frames to the log
/// file (see sqlite3WalFrames()). It checks to see if, instead of
/// appending to the current log file, it is possible to overwrite the
/// start of the existing log file with the new frames (i.e. "reset" the
/// log). If so, it sets pWal->hdr.mxFrame to 0. Otherwise,
/// pWal->hdr.mxFrame is left unchanged.
///
/// SQLITE_OK is returned if no error is encountered (regardless of whether
/// or not pWal->hdr.mxFrame is modified). An SQLite error code is returned
/// if an error occurs.
fn walRestartLog(pWal: &mut Wal) -> i32 {
    let mut rc = SQLITE_OK;

    if pWal.readLock == 0 {
        debug_assert!(pWal.ckpt_get(CKPT_NBACKFILL) == pWal.hdr.mxFrame);
        if pWal.ckpt_get(CKPT_NBACKFILL) > 0 {
            let mut salt1 = [0u8; 4];
            crate::os::os_randomness(&mut salt1);
            rc = walLockExclusive(pWal, WAL_READ_LOCK(1), WAL_NREADER - 1);
            if rc == SQLITE_OK {
                // If all readers are using WAL_READ_LOCK(0) (in other words
                // if no readers are currently using the WAL), then the
                // transactions frames will overwrite the start of the
                // existing log. Update the wal-index header to reflect
                // this.
                //
                // In theory it would be Ok to update the cache of the
                // header only at this point. But updating the actual
                // wal-index header is also safe and means there is no
                // special case for sqlite3WalUndo() to handle if this
                // transaction is rolled back.
                walRestartHdr(pWal, salt1);
                walUnlockExclusive(pWal, WAL_READ_LOCK(1), WAL_NREADER - 1);
            } else if rc != SQLITE_BUSY {
                return rc;
            }
        }
        walUnlockShared(pWal, WAL_READ_LOCK(0));
        pWal.readLock = -1;
        let mut cnt = 0;
        loop {
            let mut notUsed = 0;
            rc = walTryBeginRead(pWal, &mut notUsed, 1, &mut cnt);
            if rc != WAL_RETRY {
                break;
            }
        }
        debug_assert!((rc & 0xff) != SQLITE_BUSY); // BUSY not possible when useWal==1
    }
    rc
}

/// Information about the current state of the WAL file and where the next
/// fsync should occur - passed from sqlite3WalFrames() into
/// walWriteToLog() (C WalWriter; the Wal and its file are passed
/// separately).
struct WalWriter {
    /// Fsync at this offset
    iSyncPoint: i64,
    /// Flags for the fsync
    syncFlags: i32,
    /// Size of one page
    szPage: i32,
}

/// Write iAmt bytes of content into the WAL file beginning at iOffset.
/// Do a sync when crossing the p->iSyncPoint boundary.
///
/// In other words, if iSyncPoint is in between iOffset and iOffset+iAmt,
/// first write the part before iSyncPoint, then sync, then write the rest.
fn walWriteToLog(p: &WalWriter, pFd: &mut SqliteFile, pContent: &[u8], iOffset: i64) -> i32 {
    let mut rc;
    let mut iOffset = iOffset;
    let mut pContent = pContent;
    let iAmt = pContent.len() as i64;
    if iOffset < p.iSyncPoint && iOffset + iAmt >= p.iSyncPoint {
        let iFirstAmt = (p.iSyncPoint - iOffset) as usize;
        rc = sqlite3OsWrite(pFd, pContent, iFirstAmt as i32, iOffset);
        if rc != 0 {
            return rc;
        }
        iOffset += iFirstAmt as i64;
        pContent = &pContent[iFirstAmt..];
        debug_assert!(WAL_SYNC_FLAGS(p.syncFlags) != 0);
        rc = sqlite3OsSync(pFd, WAL_SYNC_FLAGS(p.syncFlags));
        if pContent.is_empty() || rc != 0 {
            return rc;
        }
    }
    sqlite3OsWrite(pFd, pContent, pContent.len() as i32, iOffset)
}

/// Write out a single frame of the WAL
fn walWriteOneFrame(
    p: &WalWriter,
    pWal: &mut Wal,
    pPage: &DbPage,
    nTruncate: u32,
    iOffset: i64,
) -> i32 {
    let mut aFrame = [0u8; WAL_FRAME_HDRSIZE]; // Buffer to assemble frame-header in
    let pg = pPage.borrow();
    let pData = &pg.aData[..p.szPage as usize];
    walEncodeFrame(pWal, pg.pgno, nTruncate, pData, &mut aFrame);
    let rc = walWriteToLog(p, &mut pWal.pWalFd, &aFrame, iOffset);
    if rc != 0 {
        return rc;
    }
    // Write the page data
    walWriteToLog(
        p,
        &mut pWal.pWalFd,
        pData,
        iOffset + WAL_FRAME_HDRSIZE as i64,
    )
}

/// This function is called as part of committing a transaction within
/// which one or more frames have been overwritten. It updates the
/// checksums for all frames written to the wal file by the current
/// transaction starting with the earliest to have been overwritten.
///
/// SQLITE_OK is returned if successful, or an SQLite error code otherwise.
fn walRewriteChecksums(pWal: &mut Wal, iLast: u32) -> i32 {
    let szPage = pWal.szPage as usize; // Database page size
    let mut aBuf = vec![0u8; szPage + WAL_FRAME_HDRSIZE]; // Buffer to load data from wal file into
    let mut aFrame = [0u8; WAL_FRAME_HDRSIZE]; // Buffer to assemble frame-headers in

    // Find the checksum values to use as input for the recalculating the
    // first checksum. If the first frame is frame 1 (implying that the
    // current transaction restarted the wal file), these values must be
    // read from the wal-file header. Otherwise, read them from the frame
    // header of the previous frame.
    debug_assert!(pWal.iReCksum > 0);
    let iCksumOff = if pWal.iReCksum == 1 {
        24
    } else {
        walFrameOffset(pWal.iReCksum - 1, szPage as i64) + 16
    };
    let mut rc = sqlite3OsRead(&mut pWal.pWalFd, &mut aBuf, 8, iCksumOff);
    pWal.hdr.aFrameCksum[0] = sqlite3Get4byte(&aBuf[0..]);
    pWal.hdr.aFrameCksum[1] = sqlite3Get4byte(&aBuf[4..]);

    let mut iRead = pWal.iReCksum; // Next frame to read from wal file
    pWal.iReCksum = 0;
    while rc == SQLITE_OK && iRead <= iLast {
        let iOff = walFrameOffset(iRead, szPage as i64);
        rc = sqlite3OsRead(
            &mut pWal.pWalFd,
            &mut aBuf,
            (szPage + WAL_FRAME_HDRSIZE) as i32,
            iOff,
        );
        if rc == SQLITE_OK {
            let iPgno = sqlite3Get4byte(&aBuf[0..]);
            let nDbSize = sqlite3Get4byte(&aBuf[4..]);
            walEncodeFrame(
                pWal,
                iPgno,
                nDbSize,
                &aBuf[WAL_FRAME_HDRSIZE..],
                &mut aFrame,
            );
            rc = sqlite3OsWrite(&mut pWal.pWalFd, &aFrame, WAL_FRAME_HDRSIZE as i32, iOff);
        }
        iRead += 1;
    }
    rc
}

/// Write a set of frames to the log. The caller must hold the write-lock
/// on the log file (obtained using sqlite3WalBeginWriteTransaction()).
fn walFrames(
    pWal: &mut Wal,
    szPage: i32,
    pList: &[DbPage],
    nTruncate: Pgno,
    isCommit: i32,
    sync_flags: i32,
) -> i32 {
    let mut rc;
    let mut pLast: Option<usize> = None; // Last frame in list
    let mut nExtra = 0; // Number of extra copies of last page
    let mut iFirst: u32 = 0; // First frame that may be overwritten

    debug_assert!(!pList.is_empty());
    debug_assert!(pWal.writeLock != 0);

    // If this frame set completes a transaction, then nTruncate>0.  If
    // nTruncate==0 then this frame set does not complete the transaction.
    debug_assert!((isCommit != 0) == (nTruncate != 0));

    let pLive = pWal.live_hdr(0);
    if pWal.hdr != pLive {
        iFirst = pLive.mxFrame + 1;
    }

    // See if it is possible to write these frames into the start of the
    // log file, instead of appending to it at pWal->hdr.mxFrame.
    rc = walRestartLog(pWal);
    if rc != SQLITE_OK {
        return rc;
    }

    // If this is the first frame written into the log, write the WAL header
    // to the start of the WAL file. See comments at the top of this source
    // file for a description of the WAL header format.
    let mut iFrame = pWal.hdr.mxFrame; // Next frame address
    if iFrame == 0 {
        let mut aWalHdr = [0u8; WAL_HDRSIZE]; // Buffer to assemble wal-header in

        sqlite3Put4byte(&mut aWalHdr[0..], WAL_MAGIC | SQLITE_BIGENDIAN as u32);
        sqlite3Put4byte(&mut aWalHdr[4..], WAL_MAX_VERSION);
        sqlite3Put4byte(&mut aWalHdr[8..], szPage as u32);
        sqlite3Put4byte(&mut aWalHdr[12..], pWal.nCkpt);
        if pWal.nCkpt == 0 {
            crate::os::os_randomness(&mut pWal.hdr.aSalt);
        }
        aWalHdr[16..24].copy_from_slice(&pWal.hdr.aSalt);
        let aCksum = walChecksumBytes(true, &aWalHdr[..WAL_HDRSIZE - 2 * 4], None);
        sqlite3Put4byte(&mut aWalHdr[24..], aCksum[0]);
        sqlite3Put4byte(&mut aWalHdr[28..], aCksum[1]);

        pWal.szPage = szPage as u32;
        pWal.hdr.bigEndCksum = SQLITE_BIGENDIAN;
        pWal.hdr.aFrameCksum = aCksum;
        pWal.truncateOnCommit = 1;

        rc = sqlite3OsWrite(&mut pWal.pWalFd, &aWalHdr, WAL_HDRSIZE as i32, 0);
        if rc != SQLITE_OK {
            return rc;
        }

        // Sync the header (unless SQLITE_IOCAP_SEQUENTIAL is true or unless
        // all syncing is turned off by PRAGMA synchronous=OFF).  Otherwise
        // an out-of-order write following a WAL restart could result in
        // database corruption.  See the ticket:
        //
        //     https://sqlite.org/src/info/ff5be73dee
        if pWal.syncHeader != 0 {
            rc = sqlite3OsSync(&mut pWal.pWalFd, CKPT_SYNC_FLAGS(sync_flags));
            if rc != 0 {
                return rc;
            }
        }
    }
    if pWal.szPage as i32 != szPage {
        return SQLITE_CORRUPT_BKPT(); // TH3 test case: cov1/corrupt155.test
    }

    // Setup information needed to write frames into the WAL
    let mut w = WalWriter {
        iSyncPoint: 0,
        syncFlags: sync_flags,
        szPage,
    };
    let mut iOffset = walFrameOffset(iFrame + 1, szPage as i64); // Next byte to write in WAL file
    let szFrame = szPage as i64 + WAL_FRAME_HDRSIZE as i64; // The size of a single frame

    // Write all frames into the log file exactly once
    for (ip, p) in pList.iter().enumerate() {
        let hasNext = ip + 1 < pList.len(); // C p->pDirty!=0

        // Check if this page has already been written into the wal file by
        // the current transaction. If so, overwrite the existing frame and
        // set Wal.writeLock to WAL_WRITELOCK_RECKSUM - indicating that
        // checksums must be recomputed when the transaction is committed.
        if iFirst != 0 && (hasNext || isCommit == 0) {
            let mut iWrite: u32 = 0;
            let pgno = p.borrow().pgno;
            let _ = walFindFrame(pWal, pgno, &mut iWrite);
            if iWrite >= iFirst {
                let iOff = walFrameOffset(iWrite, szPage as i64) + WAL_FRAME_HDRSIZE as i64;
                if pWal.iReCksum == 0 || iWrite < pWal.iReCksum {
                    pWal.iReCksum = iWrite;
                }
                let mut pg = p.borrow_mut();
                rc = sqlite3OsWrite(&mut pWal.pWalFd, &pg.aData, szPage, iOff);
                if rc != 0 {
                    return rc;
                }
                pg.flags &= !(PGHDR_WAL_APPEND as u16);
                continue;
            }
        }

        iFrame += 1;
        debug_assert!(iOffset == walFrameOffset(iFrame, szPage as i64));
        let nDbSize = if isCommit != 0 && !hasNext {
            nTruncate
        } else {
            0
        };
        rc = walWriteOneFrame(&w, pWal, p, nDbSize, iOffset);
        if rc != 0 {
            return rc;
        }
        pLast = Some(ip);
        iOffset += szFrame;
        p.borrow_mut().flags |= PGHDR_WAL_APPEND as u16;
    }

    // Recalculate checksums within the wal file if required.
    if isCommit != 0 && pWal.iReCksum != 0 {
        rc = walRewriteChecksums(pWal, iFrame);
        if rc != 0 {
            return rc;
        }
    }

    // If this is the end of a transaction, then we might need to pad the
    // transaction and/or sync the WAL file.
    //
    // Padding and syncing only occur if this set of frames complete a
    // transaction and if PRAGMA synchronous=FULL.  If synchronous==NORMAL
    // or synchronous==OFF, then no padding or syncing are needed.
    //
    // If SQLITE_IOCAP_POWERSAFE_OVERWRITE is defined, then padding is not
    // needed and only the sync is done.  If padding is needed, then the
    // final frame is repeated (with its commit mark) until the next sector
    // boundary is crossed.  Only the part of the WAL prior to the last
    // sector boundary is synced; the part of the last frame that extends
    // past the sector boundary is written after the sync.
    if isCommit != 0 && WAL_SYNC_FLAGS(sync_flags) != 0 {
        let mut bSync = true;
        if pWal.padToSectorBoundary != 0 {
            let sectorSize = crate::pager::sqlite3SectorSize(&mut pWal.pWalFd) as i64;
            w.iSyncPoint = ((iOffset + sectorSize - 1) / sectorSize) * sectorSize;
            bSync = w.iSyncPoint == iOffset;
            while iOffset < w.iSyncPoint {
                let last = &pList[pLast.unwrap()];
                rc = walWriteOneFrame(&w, pWal, last, nTruncate, iOffset);
                if rc != 0 {
                    return rc;
                }
                iOffset += szFrame;
                nExtra += 1;
            }
        }
        if bSync {
            debug_assert!(rc == SQLITE_OK);
            rc = sqlite3OsSync(&mut pWal.pWalFd, WAL_SYNC_FLAGS(sync_flags));
        }
    }

    // If this frame set completes the first transaction in the WAL and if
    // PRAGMA journal_size_limit is set, then truncate the WAL to the
    // journal size limit, if possible.
    if isCommit != 0 && pWal.truncateOnCommit != 0 && pWal.mxWalSize >= 0 {
        let mut sz = pWal.mxWalSize;
        if walFrameOffset(iFrame + nExtra + 1, szPage as i64) > pWal.mxWalSize {
            sz = walFrameOffset(iFrame + nExtra + 1, szPage as i64);
        }
        walLimitSize(pWal, sz);
        pWal.truncateOnCommit = 0;
    }

    // Append data to the wal-index. It is not necessary to lock the
    // wal-index to do this as the SQLITE_SHM_WRITE lock held on the
    // wal-index guarantees that there are no other writers, and no data
    // that may be in use by existing readers is being overwritten.
    iFrame = pWal.hdr.mxFrame;
    for p in pList {
        if rc != SQLITE_OK {
            break;
        }
        let (flags, pgno) = {
            let b = p.borrow();
            (b.flags, b.pgno)
        };
        if (flags & PGHDR_WAL_APPEND as u16) == 0 {
            continue;
        }
        iFrame += 1;
        rc = walIndexAppend(pWal, iFrame, pgno);
    }
    debug_assert!(pLast.is_some() || nExtra == 0);
    while rc == SQLITE_OK && nExtra > 0 {
        iFrame += 1;
        nExtra -= 1;
        let pgno = pList[pLast.unwrap()].borrow().pgno;
        rc = walIndexAppend(pWal, iFrame, pgno);
    }

    if rc == SQLITE_OK {
        // Update the private copy of the header.
        pWal.hdr.szPage = ((szPage & 0xff00) | (szPage >> 16)) as u16;
        pWal.hdr.mxFrame = iFrame;
        if isCommit != 0 {
            pWal.hdr.iChange = pWal.hdr.iChange.wrapping_add(1);
            pWal.hdr.nPage = nTruncate;
        }
        // If this is a commit, update the wal-index header too.
        if isCommit != 0 {
            walIndexWriteHdr(pWal);
            pWal.iCallback = iFrame;
        }
    }
    rc
}

/// C: `int sqlite3WalFrames( Wal *pWal, int szPage, PgHdr *pList, Pgno nTruncate, int isCommit, int sync_flags )`
///
/// Write a set of frames to the log. The caller must hold the write-lock
/// on the log file (obtained using sqlite3WalBeginWriteTransaction()).
/// `pList` is the C pDirty list, in order.
pub fn sqlite3WalFrames(
    pWal: &mut crate::wal::Wal,
    szPage: i32,
    pList: &[DbPage],
    nTruncate: Pgno,
    isCommit: i32,
    sync_flags: i32,
) -> i32 {
    walFrames(pWal, szPage, pList, nTruncate, isCommit, sync_flags)
}

/// C: `int sqlite3WalCheckpoint( Wal *pWal, sqlite3 *db, int eMode, int (*xBusy)(void*), void *pBusyArg, int sync_flags, int nBuf, u8 *zBuf, int *pnLog, int *pnCkpt )`
///
/// This routine is called to implement sqlite3_wal_checkpoint() and
/// related interfaces.
///
/// Obtain a CHECKPOINT lock and then backfill as much information as we
/// can from WAL into the database.
///
/// If parameter xBusy is not NULL, it is a pointer to a busy-handler
/// callback. In this case this function runs a blocking checkpoint.
///
/// Signature change: the database file is passed as `pDbFd`.
pub fn sqlite3WalCheckpoint(
    db: &mut Sqlite3,
    pWal: &mut crate::wal::Wal,
    pDbFd: &mut SqliteFile,
    eMode: i32,
    xBusy: Option<Rc<dyn Fn() -> i32>>,
    sync_flags: i32,
    nBuf: i32,
    zBuf: &mut [u8],
    pnLog: &mut i32,
    pnCkpt: &mut i32,
) -> i32 {
    let mut rc; // Return code
    let mut isChanged = 0; // True if a new wal-index header is loaded
    let mut eMode2 = eMode; // Mode to pass to walCheckpoint()
    let mut xBusy2 = xBusy.clone(); // Busy handler for eMode2

    debug_assert!(pWal.ckptLock == 0);
    debug_assert!(pWal.writeLock == 0);

    // EVIDENCE-OF: R-62920-47450 The busy-handler callback is never invoked
    // in the SQLITE_CHECKPOINT_PASSIVE mode.
    debug_assert!(eMode > SQLITE_CHECKPOINT_PASSIVE || xBusy.is_none());

    if pWal.readOnly != 0 {
        return SQLITE_READONLY;
    }

    // IMPLEMENTATION-OF: R-62028-47212 All calls obtain an exclusive
    // "checkpoint" lock on the database file.
    //
    // EVIDENCE-OF: R-10421-19736 If any other process is running a
    // checkpoint operation at the same time, the lock cannot be obtained
    // and SQLITE_BUSY is returned.
    //
    // EVIDENCE-OF: R-53820-33897 Even if there is a busy-handler
    // configured, it will not be invoked in this case.
    if eMode != SQLITE_CHECKPOINT_NOOP {
        rc = walLockExclusive(pWal, WAL_CKPT_LOCK, 1);
        if rc == SQLITE_OK {
            pWal.ckptLock = 1;

            // IMPLEMENTATION-OF: R-59782-36818 The SQLITE_CHECKPOINT_FULL,
            // RESTART and TRUNCATE modes also obtain the exclusive "writer"
            // lock on the database file.
            //
            // EVIDENCE-OF: R-60642-04082 If the writer lock cannot be
            // obtained immediately, and a busy-handler is configured, it is
            // invoked and the writer lock retried until either the
            // busy-handler returns 0 or the lock is successfully obtained.
            if eMode != SQLITE_CHECKPOINT_PASSIVE {
                rc = walBusyLock(pWal, &xBusy2, WAL_WRITE_LOCK, 1);
                if rc == SQLITE_OK {
                    pWal.writeLock = 1;
                } else if rc == SQLITE_BUSY {
                    eMode2 = SQLITE_CHECKPOINT_PASSIVE;
                    xBusy2 = None;
                    rc = SQLITE_OK;
                }
            }
        }
    } else {
        rc = SQLITE_OK;
    }

    // Read the wal-index header.
    if rc == SQLITE_OK {
        rc = walIndexReadHdr(pWal, &mut isChanged);
        // (sqlite3OsUnfetch() is a no-op without mmap)
    }

    // Copy data from the log to the database file.
    if rc == SQLITE_OK {
        if pWal.hdr.mxFrame != 0 && walPagesize(pWal) != nBuf {
            rc = SQLITE_CORRUPT_BKPT();
        } else if eMode2 != SQLITE_CHECKPOINT_NOOP {
            rc = walCheckpoint(pWal, db, pDbFd, eMode2, xBusy2, sync_flags, zBuf);
        }

        // If no error occurred, set the output variables.
        if rc == SQLITE_OK || rc == SQLITE_BUSY {
            *pnLog = pWal.hdr.mxFrame as i32;
            *pnCkpt = pWal.ckpt_get(CKPT_NBACKFILL) as i32;
        }
    }

    if isChanged != 0 {
        // If a new wal-index header was loaded before the checkpoint was
        // performed, then the pager-cache associated with pWal is now out
        // of date. So zero the cached wal-index header to ensure that next
        // time the pager opens a snapshot on this database it knows that
        // the cache needs to be reset.
        pWal.hdr = WalIndexHdr::default();
    }

    // Release the locks.
    let _ = sqlite3WalEndWriteTransaction(pWal);
    if pWal.ckptLock != 0 {
        walUnlockExclusive(pWal, WAL_CKPT_LOCK, 1);
        pWal.ckptLock = 0;
    }
    if rc == SQLITE_OK && eMode != eMode2 {
        SQLITE_BUSY
    } else {
        rc
    }
}

/// C: `int sqlite3WalCallback(Wal *pWal)`
///
/// Return the value to pass to a sqlite3_wal_hook callback, the number of
/// frames in the WAL at the point of the last commit since
/// sqlite3WalCallback() was called.  If no commits have occurred since the
/// last call, then return 0.
pub fn sqlite3WalCallback(pWal: &mut crate::wal::Wal) -> i32 {
    let ret = pWal.iCallback;
    pWal.iCallback = 0;
    ret as i32
}

/// C: `int sqlite3WalExclusiveMode(Wal *pWal, int op)`
///
/// This function is called to change the WAL subsystem into or out of
/// locking_mode=EXCLUSIVE.
///
/// If op is zero, then attempt to change from locking_mode=EXCLUSIVE into
/// locking_mode=NORMAL.  This means that we must acquire a lock on the
/// pWal->readLock byte.  If the WAL is already in locking_mode=NORMAL or
/// if the acquisition of the lock fails, then return 0.  If the transition
/// out of exclusive-mode is successful, return 1.  This operation must
/// occur while the pager is still holding the exclusive lock on the main
/// database file.
///
/// If op is one, then change from locking_mode=NORMAL into
/// locking_mode=EXCLUSIVE.  This means that the pWal->readLock must be
/// released.  Return 1 if the transition is made and 0 if the WAL is
/// already in exclusive-locking mode - meaning that this routine is a
/// no-op.  The pager must already hold the exclusive lock on the main
/// database file before invoking this operation.
///
/// If op is negative, then do a dry-run of the op==1 case but do not
/// actually change anything. The pager uses this to see if it should
/// acquire the database exclusive lock prior to invoking the op==1 case.
pub fn sqlite3WalExclusiveMode(pWal: &mut crate::wal::Wal, op: i32) -> i32 {
    debug_assert!(pWal.writeLock == 0);
    debug_assert!(pWal.exclusiveMode != WAL_HEAPMEMORY_MODE || op == -1);

    // pWal->readLock is usually set, but might be -1 if there was a prior
    // error while attempting to acquire are read-lock. This cannot happen
    // if the connection is actually in exclusive mode (as no xShmLock
    // locks are taken in this case). Nor should the pager attempt to
    // upgrade to exclusive-mode following such an error.
    debug_assert!(pWal.readLock >= 0 || pWal.lockError != 0);
    debug_assert!(pWal.readLock >= 0 || (op <= 0 && pWal.exclusiveMode == 0));

    if op == 0 {
        if pWal.exclusiveMode != WAL_NORMAL_MODE {
            pWal.exclusiveMode = WAL_NORMAL_MODE;
            if walLockShared(pWal, WAL_READ_LOCK(pWal.readLock as i32)) != SQLITE_OK {
                pWal.exclusiveMode = WAL_EXCLUSIVE_MODE;
            }
            (pWal.exclusiveMode == WAL_NORMAL_MODE) as i32
        } else {
            // Already in locking_mode=NORMAL
            0
        }
    } else if op > 0 {
        debug_assert!(pWal.exclusiveMode == WAL_NORMAL_MODE);
        debug_assert!(pWal.readLock >= 0);
        walUnlockShared(pWal, WAL_READ_LOCK(pWal.readLock as i32));
        pWal.exclusiveMode = WAL_EXCLUSIVE_MODE;
        1
    } else {
        (pWal.exclusiveMode == WAL_NORMAL_MODE) as i32
    }
}

/// C: `int sqlite3WalHeapMemory(Wal *pWal)`
///
/// Return true if the argument is non-NULL and the WAL module is using
/// heap-memory for the wal-index. Otherwise, if the argument is NULL or
/// the WAL module is using shared-memory, return false.
pub fn sqlite3WalHeapMemory(pWal: &mut crate::wal::Wal) -> i32 {
    (pWal.exclusiveMode == WAL_HEAPMEMORY_MODE) as i32
}

/// C: `int sqlite3WalSnapshotGet(Wal *pWal, sqlite3_snapshot **ppSnapshot)`
/// (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3WalSnapshotGet(
    pWal: &mut crate::wal::Wal,
    ppSnapshot: /*sqlite3_snapshot***/ (),
) -> i32 {
    let _ = (pWal, ppSnapshot);
    SQLITE_ERROR
}

/// C: `void sqlite3WalSnapshotOpen( Wal *pWal, sqlite3_snapshot *pSnapshot )`
/// (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3WalSnapshotOpen(pWal: &mut crate::wal::Wal, pSnapshot: /*sqlite3_snapshot**/ ()) {
    let _ = (pWal, pSnapshot);
}

/// C: `int sqlite3_snapshot_cmp(sqlite3_snapshot *p1, sqlite3_snapshot *p2)`
/// (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3_snapshot_cmp(p1: /*sqlite3_snapshot**/ (), p2: /*sqlite3_snapshot**/ ()) -> i32 {
    let _ = (p1, p2);
    0
}

/// C: `int sqlite3WalSnapshotCheck(Wal *pWal, sqlite3_snapshot *pSnapshot)`
/// (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3WalSnapshotCheck(
    pWal: &mut crate::wal::Wal,
    pSnapshot: /*sqlite3_snapshot**/ (),
) -> i32 {
    let _ = (pWal, pSnapshot);
    SQLITE_ERROR
}

/// C: `void sqlite3WalSnapshotUnlock(Wal *pWal)` (SQLITE_ENABLE_SNAPSHOT only)
pub fn sqlite3WalSnapshotUnlock(pWal: &mut crate::wal::Wal) {
    let _ = pWal;
}

/// C: `int sqlite3WalFramesize(Wal *pWal)`
///
/// This function returns the database page-size if it is known, or zero
/// if it is not.
pub fn sqlite3WalFramesize(pWal: &mut crate::wal::Wal) -> i32 {
    debug_assert!(pWal.readLock >= 0);
    pWal.szPage as i32
}

/// C: `sqlite3_file * sqlite3WalFile(Wal *pWal)`
///
/// Return the sqlite3_file object for the WAL file
pub fn sqlite3WalFile(pWal: &mut crate::wal::Wal) -> &mut crate::os::SqliteFile {
    &mut pWal.pWalFd
}

/// C: `void sqlite3WalStat(Wal *pWal, sqlite3_str *pStr)`
/// (SQLITE_DEBUG || SQLITE_ENABLE_WALSTAT)
///
/// Append JSON-5 structure elements onto describing the current state of
/// pWal onto the end of pStr.  This function is for debugging and
/// analysis only and is not included in production builds.
pub fn sqlite3WalStat(pWal: &mut crate::wal::Wal, pStr: &mut StrAccum) {
    use crate::printf::{PfArg, sqlite3_str_appendf};
    if pWal.apWiData.first().is_none_or(|p| p.is_none()) {
        return;
    }
    let h = pWal.live_hdr(0);
    sqlite3_str_appendf(pStr, b"hdr0:{", &[]);
    sqlite3_str_appendf(pStr, b"iVersion:%u", &[PfArg::from(h.iVersion as i64)]);
    sqlite3_str_appendf(pStr, b",iChange:%u", &[PfArg::from(h.iChange as i64)]);
    sqlite3_str_appendf(pStr, b",isInit:%u", &[PfArg::from(h.isInit as i64)]);
    sqlite3_str_appendf(
        pStr,
        b",bigEndCksum:%u",
        &[PfArg::from(h.bigEndCksum as i64)],
    );
    sqlite3_str_appendf(pStr, b",szPage:%u", &[PfArg::from(h.szPage as i64)]);
    sqlite3_str_appendf(pStr, b",mxFrame:%u", &[PfArg::from(h.mxFrame as i64)]);
    sqlite3_str_appendf(pStr, b",nPage:%u", &[PfArg::from(h.nPage as i64)]);
    sqlite3_str_appendf(pStr, b"},ckpt:{", &[]);
    sqlite3_str_appendf(
        pStr,
        b"nBackfill:%u",
        &[PfArg::from(pWal.ckpt_get(CKPT_NBACKFILL) as i64)],
    );
    sqlite3_str_appendf(
        pStr,
        b",aReadMark:[%u",
        &[PfArg::from(pWal.ckpt_get(CKPT_READMARK) as i64)],
    );
    for i in 1..WAL_NREADER as usize {
        sqlite3_str_appendf(
            pStr,
            b",%u",
            &[PfArg::from(pWal.ckpt_get(CKPT_READMARK + i) as i64)],
        );
    }
    sqlite3_str_appendf(
        pStr,
        b"],nBackfillAttempted:%u}",
        &[PfArg::from(pWal.ckpt_get(CKPT_NBACKFILLATTEMPTED) as i64)],
    );
    sqlite3_str_appendf(
        pStr,
        b",nWiData:%d",
        &[PfArg::from(pWal.apWiData.len() as i64)],
    );
    sqlite3_str_appendf(pStr, b",readLock:%d", &[PfArg::from(pWal.readLock as i64)]);
    sqlite3_str_appendf(pStr, b",minFrame:%u", &[PfArg::from(pWal.minFrame as i64)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_words_roundtrip() {
        let h = WalIndexHdr {
            iVersion: WALINDEX_MAX_VERSION,
            unused: 0,
            iChange: 7,
            isInit: 1,
            bigEndCksum: 0,
            szPage: 4096,
            mxFrame: 12,
            nPage: 3,
            aFrameCksum: [1, 2],
            aSalt: [1, 2, 3, 4, 5, 6, 7, 8],
            aCksum: [9, 10],
        };
        assert_eq!(WalIndexHdr::from_words(&h.to_words()), h);
    }

    #[test]
    fn checksum_matches_native_words() {
        let w: Vec<u32> = (1..=10).collect();
        let b: Vec<u8> = w.iter().flat_map(|x| x.to_ne_bytes()).collect();
        assert_eq!(walChecksumBytes(true, &b, None), walChecksumWords(&w));
    }

    #[test]
    fn mergesort_keeps_latest() {
        let aContent = [5u32, 3, 5, 1, 3, 9];
        let mut l: Vec<u16> = (0..6).collect();
        walMergesort(&aContent, &mut l);
        assert_eq!(l, vec![3, 4, 2, 5]);
    }

    #[test]
    fn hash_slots() {
        let loc = WalHashLoc {
            page: Arc::new(ShmRegion::new_heap(WALINDEX_PGSZ)),
            off: 0,
            iZero: 0,
        };
        loc.set_hash(0, 0x1234);
        loc.set_hash(1, 0x5678);
        loc.set_hash(8191, 7);
        assert_eq!(loc.hash(0), 0x1234);
        assert_eq!(loc.hash(1), 0x5678);
        assert_eq!(loc.hash(8191), 7);
        loc.set_hash(0, 0);
        assert_eq!(loc.hash(1), 0x5678);
    }
}
