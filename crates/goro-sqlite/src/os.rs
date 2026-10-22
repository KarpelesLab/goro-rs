//! Port of os.c: the OS interface (VFS and file wrappers).
//!
//! `Vfs` is the C `sqlite3_vfs` (a table of function pointers plus a few
//! fields); `SqliteFile` is the C `sqlite3_file`.  A C `sqlite3_file`
//! whose `pMethods` is NULL is `SqliteFile::Closed`; the io-methods table
//! is replaced by the enum variant (the unix file of os_unix.rs or the
//! in-memory journal of memjournal.rs).
//!
//! File names passed through the VFS (`zPath`) use the C buffer layout:
//! the name, then optionally a NUL followed by the URI parameter block
//! (`key\0value\0...\0`).  `zpath_name()` returns the name part.

use crate::int::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The argument of xFileControl() (C `void *pArg`).
pub enum FcntlArg<'a> {
    /// No argument (C NULL).
    None,
    /// `int*` argument (in/out).
    Int(&'a mut i32),
    /// `sqlite3_int64*` argument (in/out).
    I64(&'a mut i64),
    /// `const char*` input argument (e.g. SQLITE_FCNTL_SYNC).
    Str(Option<&'a [u8]>),
    /// `char**` output argument (SQLITE_FCNTL_VFSNAME, TEMPFILENAME).
    StrOut(&'a mut Option<ZStr>),
    /// `char*[3]` argument of SQLITE_FCNTL_PRAGMA: [0] receives the
    /// result/error message, [1] is the pragma name, [2] the argument.
    Pragma(&'a mut [Option<ZStr>; 3]),
}

/// An open file (C sqlite3_file).
#[derive(Default)]
pub enum SqliteFile {
    /// Not open (C `pMethods==0`).
    #[default]
    Closed,
    /// A file of the unix VFS.
    Unix(crate::os_unix::UnixFile),
    /// An in-memory journal (memjournal.c).
    Mem(crate::memjournal::MemJournal),
}

/// C isOpen(pFd)
#[inline]
pub fn isOpen(p: &SqliteFile) -> bool {
    !matches!(p, SqliteFile::Closed)
}

/// The name part (up to the first NUL) of a C-layout file name buffer.
pub fn zpath_name(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// True if the C-layout file name buffer carries URI parameters.
pub fn zpath_has_params(z: &[u8]) -> bool {
    match z.iter().position(|&c| c == 0) {
        Some(n) => z.len() > n + 1 && z[n + 1] != 0,
        None => false,
    }
}

/// sqlite3_uri_boolean() that does not call into mainapi when the name
/// carries no URI parameter (the result is then always `bDflt`).
pub(crate) fn uri_boolean(z: Option<&[u8]>, zParam: &[u8], bDflt: i32) -> i32 {
    match z {
        Some(z) if zpath_has_params(z) => {
            crate::mainapi::sqlite3_uri_boolean(Some(z), Some(zParam), bDflt)
        }
        _ => (bDflt != 0) as i32,
    }
}

/// sqlite3_uri_parameter() that does not call into mainapi when the name
/// carries no URI parameter.
pub(crate) fn uri_parameter(z: Option<&[u8]>, zParam: &[u8]) -> Option<ZStr> {
    match z {
        Some(z) if zpath_has_params(z) => {
            crate::mainapi::sqlite3_uri_parameter(Some(z), Some(zParam))
        }
        _ => None,
    }
}

/// sqlite3_randomness() as used by the OS layer and the pager.
/// Unit tests of this crate cannot call random.rs before the whole engine
/// is assembled, so they use a local generator.
pub(crate) fn os_randomness(pBuf: &mut [u8]) {
    #[cfg(not(test))]
    {
        crate::random::sqlite3_randomness(pBuf.len() as i32, pBuf);
    }
    #[cfg(test)]
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEED: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);
        for b in pBuf.iter_mut() {
            let mut x = SEED.fetch_add(0x9E3779B97F4A7C15, Ordering::Relaxed);
            x ^= x >> 33;
            x = x.wrapping_mul(0xff51afd7ed558ccd);
            x ^= x >> 33;
            *b = x as u8;
        }
    }
}

/// Signature of xOpen: (vfs, zPath, file, flags, pOutFlags).
pub type XOpen = fn(&Vfs, Option<&[u8]>, &mut SqliteFile, i32, Option<&mut i32>) -> i32;

/// A virtual file system (C sqlite3_vfs).
pub struct Vfs {
    pub iVersion: i32,
    pub szOsFile: i32,
    pub mxPathname: i32,
    pub zName: ZStr,
    /// VFS private data (the unix VFS: locking style, see os_unix.rs).
    pub pAppData: usize,
    pub xOpen: XOpen,
    pub xDelete: Option<fn(&Vfs, &[u8], i32) -> i32>,
    pub xAccess: fn(&Vfs, &[u8], i32, &mut i32) -> i32,
    /// (vfs, zPath, nOut, zOut): the result replaces the content of zOut.
    pub xFullPathname: fn(&Vfs, &[u8], i32, &mut ZStr) -> i32,
    pub xRandomness: fn(&Vfs, &mut [u8]) -> i32,
    pub xSleep: fn(&Vfs, i32) -> i32,
    pub xCurrentTime: Option<fn(&Vfs, &mut f64) -> i32>,
    pub xGetLastError: Option<fn(&Vfs) -> i32>,
    pub xCurrentTimeInt64: Option<fn(&Vfs, &mut i64) -> i32>,
}

// ---------------------------------------------------------------------------
// File methods
// ---------------------------------------------------------------------------

/// C: `void sqlite3OsClose(sqlite3_file *pId)`
pub fn sqlite3OsClose(pId: &mut crate::os::SqliteFile) {
    match pId {
        SqliteFile::Closed => {}
        SqliteFile::Unix(f) => {
            crate::os_unix::unixClose(f);
        }
        SqliteFile::Mem(m) => {
            crate::memjournal::memjrnlClose(m);
        }
    }
    *pId = SqliteFile::Closed;
}

/// C: `int sqlite3OsRead(sqlite3_file *id, void *pBuf, int amt, i64 offset)`
pub fn sqlite3OsRead(
    id: &mut crate::os::SqliteFile,
    pBuf: &mut [u8],
    amt: i32,
    offset: i64,
) -> i32 {
    let buf = &mut pBuf[..amt as usize];
    match id {
        SqliteFile::Closed => SQLITE_IOERR_READ,
        SqliteFile::Unix(f) => crate::os_unix::unixRead(f, buf, offset),
        SqliteFile::Mem(m) => crate::memjournal::memjrnlRead(m, buf, offset),
    }
}

/// C: `int sqlite3OsWrite(sqlite3_file *id, const void *pBuf, int amt, i64 offset)`
pub fn sqlite3OsWrite(id: &mut crate::os::SqliteFile, pBuf: &[u8], amt: i32, offset: i64) -> i32 {
    let buf = &pBuf[..amt as usize];
    match id {
        SqliteFile::Closed => SQLITE_IOERR_WRITE,
        SqliteFile::Unix(f) => crate::os_unix::unixWrite(f, buf, offset),
        SqliteFile::Mem(_) => crate::memjournal::memjrnlWrite(id, buf, offset),
    }
}

/// C: `int sqlite3OsTruncate(sqlite3_file *id, i64 size)`
pub fn sqlite3OsTruncate(id: &mut crate::os::SqliteFile, size: i64) -> i32 {
    match id {
        SqliteFile::Closed => SQLITE_IOERR_TRUNCATE,
        SqliteFile::Unix(f) => crate::os_unix::unixTruncate(f, size),
        SqliteFile::Mem(m) => crate::memjournal::memjrnlTruncate(m, size),
    }
}

/// C: `int sqlite3OsSync(sqlite3_file *id, int flags)`
pub fn sqlite3OsSync(id: &mut crate::os::SqliteFile, flags: i32) -> i32 {
    if flags == 0 {
        return SQLITE_OK;
    }
    match id {
        SqliteFile::Closed => SQLITE_IOERR_FSYNC,
        SqliteFile::Unix(f) => crate::os_unix::unixSync(f, flags),
        SqliteFile::Mem(_) => SQLITE_OK,
    }
}

/// C: `int sqlite3OsFileSize(sqlite3_file *id, i64 *pSize)`
pub fn sqlite3OsFileSize(id: &mut crate::os::SqliteFile, pSize: &mut i64) -> i32 {
    match id {
        SqliteFile::Closed => SQLITE_IOERR_FSTAT,
        SqliteFile::Unix(f) => crate::os_unix::unixFileSize(f, pSize),
        SqliteFile::Mem(m) => crate::memjournal::memjrnlFileSize(m, pSize),
    }
}

/// C: `int sqlite3OsLock(sqlite3_file *id, int lockType)`
pub fn sqlite3OsLock(id: &mut crate::os::SqliteFile, lockType: i32) -> i32 {
    debug_assert!(lockType >= SHARED_LOCK && lockType <= EXCLUSIVE_LOCK);
    match id {
        SqliteFile::Unix(f) => crate::os_unix::unixLockMethod(f, lockType),
        _ => SQLITE_OK,
    }
}

/// C: `int sqlite3OsUnlock(sqlite3_file *id, int lockType)`
pub fn sqlite3OsUnlock(id: &mut crate::os::SqliteFile, lockType: i32) -> i32 {
    debug_assert!(lockType == NO_LOCK || lockType == SHARED_LOCK);
    match id {
        SqliteFile::Unix(f) => crate::os_unix::unixUnlockMethod(f, lockType),
        _ => SQLITE_OK,
    }
}

/// C: `int sqlite3OsCheckReservedLock(sqlite3_file *id, int *pResOut)`
pub fn sqlite3OsCheckReservedLock(id: &mut crate::os::SqliteFile, pResOut: &mut i32) -> i32 {
    match id {
        SqliteFile::Unix(f) => crate::os_unix::unixCheckReservedLockMethod(f, pResOut),
        _ => {
            *pResOut = 0;
            SQLITE_OK
        }
    }
}

/// C: `int sqlite3OsFileControl(sqlite3_file *id, int op, void *pArg)`
///
/// Signature change: `pArg` is a typed `FcntlArg`.
pub fn sqlite3OsFileControl(id: &mut crate::os::SqliteFile, op: i32, pArg: FcntlArg) -> i32 {
    match id {
        SqliteFile::Closed => SQLITE_NOTFOUND,
        SqliteFile::Unix(f) => crate::os_unix::unixFileControl(f, op, pArg),
        SqliteFile::Mem(_) => SQLITE_NOTFOUND,
    }
}

/// C: `void sqlite3OsFileControlHint(sqlite3_file *id, int op, void *pArg)`
pub fn sqlite3OsFileControlHint(id: &mut crate::os::SqliteFile, op: i32, pArg: FcntlArg) {
    if isOpen(id) {
        let _ = sqlite3OsFileControl(id, op, pArg);
    }
}

/// C: `int sqlite3OsSectorSize(sqlite3_file *id)`
pub fn sqlite3OsSectorSize(id: &mut crate::os::SqliteFile) -> i32 {
    match id {
        SqliteFile::Unix(f) => crate::os_unix::unixSectorSize(f),
        _ => SQLITE_DEFAULT_SECTOR_SIZE,
    }
}

/// C: `int sqlite3OsDeviceCharacteristics(sqlite3_file *id)`
pub fn sqlite3OsDeviceCharacteristics(id: &mut crate::os::SqliteFile) -> i32 {
    match id {
        SqliteFile::Unix(f) => crate::os_unix::unixDeviceCharacteristics(f),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Shared memory (the wal-index)
// ---------------------------------------------------------------------------

/// One region (32KB page) of the wal-index (C `volatile u32*` returned by
/// xShmMap()).
///
/// The content is seen as an array of `AtomicU32` (native byte order, like
/// the C code which accesses it as `u32`/`u16` arrays): the memory may be
/// shared with other threads and, through a `MAP_SHARED` mapping of the
/// -shm file, with other processes (including the C library), so every
/// access is atomic.  A region is either heap memory (heap-memory WAL
/// mode, unreliable read-only shm, or the unix-excl VFS) or a mapping of
/// the -shm file owned by os_unix.rs.  Regions are reference counted: a
/// mapping is unmapped when the last user drops it.
pub struct ShmRegion(ShmRegionKind);

enum ShmRegionKind {
    Heap(Box<[std::sync::atomic::AtomicU32]>),
    Mapped(std::sync::Arc<crate::os_unix::MappedRegion>, usize, usize),
}

impl ShmRegion {
    /// A zeroed heap region of nByte bytes (C sqlite3MallocZero()).
    pub fn new_heap(nByte: usize) -> ShmRegion {
        ShmRegion(ShmRegionKind::Heap(
            (0..nByte / 4)
                .map(|_| std::sync::atomic::AtomicU32::new(0))
                .collect(),
        ))
    }

    /// `nWord` words starting at word `iOff` of a mapping.
    pub(crate) fn mapped(
        m: std::sync::Arc<crate::os_unix::MappedRegion>,
        iOff: usize,
        nWord: usize,
    ) -> ShmRegion {
        ShmRegion(ShmRegionKind::Mapped(m, iOff, nWord))
    }

    /// The content of the region as 32-bit words.
    #[inline]
    pub fn w(&self) -> &[std::sync::atomic::AtomicU32] {
        match &self.0 {
            ShmRegionKind::Heap(b) => b,
            ShmRegionKind::Mapped(m, off, n) => &m.words()[*off..*off + *n],
        }
    }
}

/// A wal-index page handed out by xShmMap() (C `void volatile*`).
pub type ShmPage = std::sync::Arc<ShmRegion>;

/// The shared-memory methods of an open database file (C xShmMap,
/// xShmLock, xShmBarrier and xShmUnmap of `pDbFd->pMethods`, with the
/// per-connection state C keeps in `unixFile.pShm`).
///
/// Signature change: the WAL module cannot keep a pointer to the pager's
/// database file, so it obtains this handle from the file once (see
/// sqlite3OsShmHandle()) and passes it to the sqlite3OsShm*() functions.
/// The database file keeps a clone of the handle, for the file-control and
/// locking code that needs to know about the shared memory.
pub type ShmHandle = Rc<RefCell<crate::os_unix::UnixShmConn>>;

/// The shared-memory handle of database file `pDbFd`, or None if its VFS
/// does not implement shared memory (C `pMethods->iVersion<2 ||
/// pMethods->xShmMap==0`).
pub fn sqlite3OsShmHandle(pDbFd: &mut crate::os::SqliteFile) -> Option<ShmHandle> {
    match pDbFd {
        SqliteFile::Unix(f) => crate::os_unix::unixShmHandle(f),
        _ => None,
    }
}

/// True if the VFS of `pDbFd` implements shared memory (xShmMap!=0).
pub fn sqlite3OsHasShm(pDbFd: &crate::os::SqliteFile) -> bool {
    match pDbFd {
        SqliteFile::Unix(f) => crate::os_unix::unixHasShm(f),
        _ => false,
    }
}

/// C: `int sqlite3OsShmLock(sqlite3_file *id, int offset, int n, int flags)`
pub fn sqlite3OsShmLock(id: &ShmHandle, offset: i32, n: i32, flags: i32) -> i32 {
    crate::os_unix::unixShmLock(&mut id.borrow_mut(), offset, n, flags)
}

/// C: `void sqlite3OsShmBarrier(sqlite3_file *id)`
pub fn sqlite3OsShmBarrier(id: &ShmHandle) {
    let _ = id;
    crate::os_unix::unixShmBarrier();
}

/// C: `int sqlite3OsShmUnmap(sqlite3_file *id, int deleteFlag)`
pub fn sqlite3OsShmUnmap(id: &ShmHandle, deleteFlag: i32) -> i32 {
    crate::os_unix::unixShmUnmap(&mut id.borrow_mut(), deleteFlag)
}

/// C: `int sqlite3OsShmMap( sqlite3_file *id, int iPage, int pgsz, int bExtend, void volatile **pp )`
///
/// Signature change: `pp` receives the region (None for C NULL).
pub fn sqlite3OsShmMap(
    id: &ShmHandle,
    iPage: i32,
    pgsz: i32,
    bExtend: i32,
    pp: &mut Option<ShmPage>,
) -> i32 {
    crate::os_unix::unixShmMap(&mut id.borrow_mut(), iPage, pgsz, bExtend, pp)
}

/// C: `int sqlite3OsFetch(sqlite3_file *id, i64 iOff, int iAmt, void **pp)`
/// (SQLITE_MAX_MMAP_SIZE==0 variant: never returns a mapping)
pub fn sqlite3OsFetch(
    id: &mut crate::os::SqliteFile,
    iOff: i64,
    iAmt: i32,
    pp: /*void***/ (),
) -> i32 {
    let _ = (id, iOff, iAmt, pp);
    SQLITE_OK
}

/// C: `int sqlite3OsUnfetch(sqlite3_file *id, i64 iOff, void *p)`
pub fn sqlite3OsUnfetch(id: &mut crate::os::SqliteFile, iOff: i64, p: &mut [u8]) -> i32 {
    let _ = (id, iOff, p);
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// VFS methods
// ---------------------------------------------------------------------------

/// C: `int sqlite3OsOpen( sqlite3_vfs *pVfs, const char *zPath, sqlite3_file *pFile, int flags, int *pFlagsOut )`
///
/// Signature change: `pVfs: &Vfs`, `pFlagsOut: Option<&mut i32>` (C NULL).
pub fn sqlite3OsOpen(
    pVfs: &crate::os::Vfs,
    zPath: Option<&[u8]>,
    pFile: &mut crate::os::SqliteFile,
    flags: i32,
    pFlagsOut: Option<&mut i32>,
) -> i32 {
    debug_assert!(zPath.is_some() || (flags & SQLITE_OPEN_EXCLUSIVE) != 0);
    // 0x87f7f is a mask of SQLITE_OPEN_ flags that are valid to be passed
    // down into the VFS layer.  Some SQLITE_OPEN_ flags (for example,
    // SQLITE_OPEN_FULLMUTEX or SQLITE_OPEN_SHAREDCACHE) are blocked before
    // reaching the VFS.
    let rc = (pVfs.xOpen)(pVfs, zPath, pFile, flags & 0x1087f7f, pFlagsOut);
    debug_assert!(rc == SQLITE_OK || !isOpen(pFile));
    rc
}

/// C: `int sqlite3OsDelete(sqlite3_vfs *pVfs, const char *zPath, int dirSync)`
pub fn sqlite3OsDelete(pVfs: &crate::os::Vfs, zPath: &[u8], dirSync: i32) -> i32 {
    debug_assert!(dirSync == 0 || dirSync == 1);
    match pVfs.xDelete {
        Some(x) => x(pVfs, zPath, dirSync),
        None => SQLITE_OK,
    }
}

/// C: `int sqlite3OsAccess( sqlite3_vfs *pVfs, const char *zPath, int flags, int *pResOut )`
pub fn sqlite3OsAccess(pVfs: &crate::os::Vfs, zPath: &[u8], flags: i32, pResOut: &mut i32) -> i32 {
    (pVfs.xAccess)(pVfs, zPath, flags, pResOut)
}

/// C: `int sqlite3OsFullPathname( sqlite3_vfs *pVfs, const char *zPath, int nPathOut, char *zPathOut )`
///
/// Signature change: the result is stored in `zPathOut` (a ZStr without
/// the terminating NUL).
pub fn sqlite3OsFullPathname(
    pVfs: &crate::os::Vfs,
    zPath: &[u8],
    nPathOut: i32,
    zPathOut: &mut ZStr,
) -> i32 {
    zPathOut.clear();
    (pVfs.xFullPathname)(pVfs, zPath, nPathOut, zPathOut)
}

/// C: `void * sqlite3OsDlOpen(sqlite3_vfs *pVfs, const char *zPath)`
/// (no load_extension in this port)
pub fn sqlite3OsDlOpen(pVfs: &crate::os::Vfs, zPath: Option<&[u8]>) -> Option<Vec<u8>> {
    let _ = (pVfs, zPath);
    None
}

/// C: `void sqlite3OsDlError(sqlite3_vfs *pVfs, int nByte, char *zBufOut)`
pub fn sqlite3OsDlError(pVfs: &crate::os::Vfs, nByte: i32, zBufOut: &mut ZStr) {
    let _ = (pVfs, nByte);
    zBufOut.clear();
    zBufOut.extend_from_slice(b"extension loading is not supported");
}

/// C: `void sqlite3OsDlClose(sqlite3_vfs *pVfs, void *pHandle)`
pub fn sqlite3OsDlClose(pVfs: &crate::os::Vfs, pHandle: &mut [u8]) {
    let _ = (pVfs, pHandle);
}

/// C: `int sqlite3OsRandomness(sqlite3_vfs *pVfs, int nByte, char *zBufOut)`
pub fn sqlite3OsRandomness(pVfs: &crate::os::Vfs, nByte: i32, zBufOut: &mut [u8]) -> i32 {
    let seed = crate::random::prng_seed();
    if seed != 0 {
        let n = nByte.max(0) as usize;
        for b in zBufOut[..n].iter_mut() {
            *b = 0;
        }
        let mut nByte = n;
        if nByte > 4 {
            nByte = 4;
        }
        zBufOut[..nByte].copy_from_slice(&seed.to_ne_bytes()[..nByte]);
        SQLITE_OK
    } else {
        (pVfs.xRandomness)(pVfs, &mut zBufOut[..nByte.max(0) as usize])
    }
}

/// C: `int sqlite3OsSleep(sqlite3_vfs *pVfs, int nMicro)`
pub fn sqlite3OsSleep(pVfs: &crate::os::Vfs, nMicro: i32) -> i32 {
    (pVfs.xSleep)(pVfs, nMicro)
}

/// C: `int sqlite3OsGetLastError(sqlite3_vfs *pVfs)`
pub fn sqlite3OsGetLastError(pVfs: &crate::os::Vfs) -> i32 {
    match pVfs.xGetLastError {
        Some(x) => x(pVfs),
        None => 0,
    }
}

/// C: `int sqlite3OsCurrentTimeInt64(sqlite3_vfs *pVfs, sqlite3_int64 *pTimeOut)`
pub fn sqlite3OsCurrentTimeInt64(pVfs: &crate::os::Vfs, pTimeOut: &mut i64) -> i32 {
    if pVfs.iVersion >= 2 && pVfs.xCurrentTimeInt64.is_some() {
        (pVfs.xCurrentTimeInt64.unwrap())(pVfs, pTimeOut)
    } else {
        let mut r = 0.0f64;
        let rc = match pVfs.xCurrentTime {
            Some(x) => x(pVfs, &mut r),
            None => SQLITE_ERROR,
        };
        *pTimeOut = crate::vdbemem::sqlite3RealToI64(r * 86400000.0);
        rc
    }
}

/// C: `int sqlite3OsOpenMalloc( sqlite3_vfs *pVfs, const char *zFile, sqlite3_file **ppFile, int flags, int *pOutFlags )`
pub fn sqlite3OsOpenMalloc(
    pVfs: &crate::os::Vfs,
    zFile: Option<&[u8]>,
    ppFile: &mut Option<Box<crate::os::SqliteFile>>,
    flags: i32,
    pOutFlags: Option<&mut i32>,
) -> i32 {
    let mut pFile = Box::new(SqliteFile::Closed);
    let rc = sqlite3OsOpen(pVfs, zFile, &mut pFile, flags, pOutFlags);
    if rc != SQLITE_OK {
        *ppFile = None;
    } else {
        *ppFile = Some(pFile);
    }
    rc
}

/// C: `void sqlite3OsCloseFree(sqlite3_file *pFile)`
pub fn sqlite3OsCloseFree(pFile: &mut crate::os::SqliteFile) {
    sqlite3OsClose(pFile);
}

/// C: `int sqlite3OsInit(void)`
pub fn sqlite3OsInit() -> i32 {
    crate::os_unix::sqlite3_os_init()
}

thread_local! {
    /// The list of all registered VFS implementations (C vfsList), the
    /// default one first.
    static VFS_LIST: RefCell<Vec<Rc<Vfs>>> = const { RefCell::new(Vec::new()) };
}

/// C: `sqlite3_vfs * sqlite3_vfs_find(const char *zVfs)`
///
/// Signature change: returns `Option<Rc<Vfs>>`.
pub fn sqlite3_vfs_find(zVfs: Option<&[u8]>) -> Option<Rc<crate::os::Vfs>> {
    let rc = crate::mainapi::sqlite3_initialize();
    if rc != 0 {
        return None;
    }
    vfs_find_noinit(zVfs)
}

/// sqlite3_vfs_find() without the sqlite3_initialize() call.
pub fn vfs_find_noinit(zVfs: Option<&[u8]>) -> Option<Rc<crate::os::Vfs>> {
    VFS_LIST.with(|l| {
        let l = l.borrow();
        match zVfs {
            None => l.first().cloned(),
            Some(z) => l.iter().find(|v| v.zName == z).cloned(),
        }
    })
}

/// Unlink a VFS from the linked list
fn vfsUnlink(l: &mut Vec<Rc<Vfs>>, pVfs: &Rc<Vfs>) {
    l.retain(|v| !Rc::ptr_eq(v, pVfs));
}

/// C: `int sqlite3_vfs_register(sqlite3_vfs *pVfs, int makeDflt)`
///
/// Signature change: takes an `Rc<Vfs>`.
pub fn sqlite3_vfs_register(pVfs: Rc<crate::os::Vfs>, makeDflt: i32) -> i32 {
    let rc = crate::mainapi::sqlite3_initialize();
    if rc != 0 {
        return rc;
    }
    vfs_register_noinit(pVfs, makeDflt)
}

/// sqlite3_vfs_register() without the sqlite3_initialize() call (used by
/// sqlite3_os_init()).
pub fn vfs_register_noinit(pVfs: Rc<crate::os::Vfs>, makeDflt: i32) -> i32 {
    VFS_LIST.with(|l| {
        let mut l = l.borrow_mut();
        vfsUnlink(&mut l, &pVfs);
        if makeDflt != 0 || l.is_empty() {
            l.insert(0, pVfs);
        } else {
            l.insert(1, pVfs);
        }
    });
    SQLITE_OK
}

/// C: `int sqlite3_vfs_unregister(sqlite3_vfs *pVfs)`
pub fn sqlite3_vfs_unregister(pVfs: &Rc<crate::os::Vfs>) -> i32 {
    let rc = crate::mainapi::sqlite3_initialize();
    if rc != 0 {
        return rc;
    }
    VFS_LIST.with(|l| vfsUnlink(&mut l.borrow_mut(), pVfs));
    SQLITE_OK
}
