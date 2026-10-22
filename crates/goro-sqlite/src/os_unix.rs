//! Port of os_unix.c: the "unix" VFS.
//!
//! Only the POSIX advisory-locking flavour ("unix", "unix-excl") and the
//! no-locking flavour ("unix-none") are implemented; the dot-file, flock,
//! AFP, proxy and VxWorks variants, mmap and the WAL shared-memory
//! methods are not.  The locking protocol is byte-for-byte that of C
//! SQLite (PENDING / RESERVED / SHARED byte ranges, F_SETLK through the
//! libc crate, per-inode lock sharing within the process with deferred
//! close of file descriptors), so that databases can be shared with the
//! real sqlite3 library.

use crate::int::*;
use crate::os::{FcntlArg, SqliteFile, Vfs};
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Maximum supported path-length.
pub const MAX_PATHNAME: i32 = 512;
/// Maximum supported symbolic links
const SQLITE_MAX_SYMLINK: i32 = 200;
/// C FILENAME_MAX on Linux
const SQLITE_MAX_PATHLEN: usize = 4096;
const SQLITE_DEFAULT_FILE_PERMISSIONS: u32 = 0o644;
const SQLITE_TEMP_FILE_PREFIX: &[u8] = b"etilqs_";
const SQLITE_POWERSAFE_OVERWRITE: i32 = 1;
const SQLITE_MINIMUM_FILE_DESCRIPTOR: i32 = 3;

// Allowed values for the unixFile.ctrlFlags bitmask:
const UNIXFILE_EXCL: u16 = 0x01;
const UNIXFILE_RDONLY: u16 = 0x02;
const UNIXFILE_PERSIST_WAL: u16 = 0x04;
const UNIXFILE_DIRSYNC: u16 = 0x08;
const UNIXFILE_PSOW: u16 = 0x10;
const UNIXFILE_DELETE: u16 = 0x20;
const UNIXFILE_URI: u16 = 0x40;
const UNIXFILE_NOLOCK: u16 = 0x80;

/// Values of Vfs::pAppData for the unix VFS (C: the "finder" function).
pub const UNIX_FINDER_POSIX: usize = 0;
pub const UNIX_FINDER_NOLOCK: usize = 1;

/// The directory used for temporary files (C sqlite3_temp_directory, set
/// by PRAGMA temp_store_directory).  None means "use the default search".
pub static sqlite3_temp_directory: Mutex<Option<ZStr>> = Mutex::new(None);

#[inline]
fn PENDING_BYTE() -> i64 {
    crate::global::sqlite3PendingByte
}
#[inline]
fn RESERVED_BYTE() -> i64 {
    PENDING_BYTE() + 1
}
#[inline]
fn SHARED_FIRST() -> i64 {
    PENDING_BYTE() + 2
}

/// The io-methods table of a unix file (C posixIoMethods / nolockIoMethods)
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum UnixMethods {
    Posix,
    Nolock,
}

/// An unused file descriptor kept open until the locks on the inode are
/// released (C UnixUnusedFd).
struct UnixUnusedFd {
    fd: File,
    flags: i32,
}

/// The mutex-protected part of unixInodeInfo.
struct InodeLock {
    /// Number of SHARED locks held
    nShared: i32,
    /// Number of outstanding file locks
    nLock: i32,
    /// One of SHARED_LOCK, RESERVED_LOCK etc.
    eFileLock: u8,
    /// An exclusive process lock is held
    bProcessLock: u8,
    /// Unused file descriptors to close (most recent last)
    pUnused: Vec<UnixUnusedFd>,
}

/// An instance of this object is created for each inode that is open by
/// the process (C unixInodeInfo).
struct UnixInodeInfo {
    /// The lookup key (st_dev, st_ino)
    fileId: (u64, u64),
    /// Number of pointers to this structure (protected by INODE_LIST)
    nRef: AtomicI32,
    /// C pLockMutex and the fields it protects
    lock: Mutex<InodeLock>,
}

/// All unixInodeInfo objects of the process (C inodeList); the mutex is
/// the C unixBigLock.
static INODE_LIST: Mutex<Vec<Arc<UnixInodeInfo>>> = Mutex::new(Vec::new());

/// PID that last reset the PRNG (C randomnessPid).
static RANDOMNESS_PID: AtomicU32 = AtomicU32::new(0);

fn unixEnterMutex() -> MutexGuard<'static, Vec<Arc<UnixInodeInfo>>> {
    INODE_LIST.lock().unwrap_or_else(|e| e.into_inner())
}

fn lockInode(p: &UnixInodeInfo) -> MutexGuard<'_, InodeLock> {
    p.lock.lock().unwrap_or_else(|e| e.into_inner())
}

/// The unixFile structure is a subclass of sqlite3_file specific to the
/// unix portability layer (C unixFile).
pub struct UnixFile {
    pMethod: UnixMethods,
    /// Name of the VFS that created this file
    zVfsName: ZStr,
    /// mxPathname of that VFS
    mxPathname: i32,
    /// Info about locks on this inode
    pInode: Option<Arc<UnixInodeInfo>>,
    /// The file descriptor (C h; None is -1)
    h: Option<File>,
    /// The type of lock held on this fd
    pub eFileLock: u8,
    /// Behavioral bits.  UNIXFILE_* flags
    ctrlFlags: u16,
    /// The unix errno from last I/O error
    lastErrno: i32,
    /// C pPreallocatedUnused: Some(open flags) for main database files
    pPreallocatedUnused: Option<i32>,
    /// Name of the file (C layout, with URI parameters if any)
    zPath: Option<ZStr>,
    /// Chunk size configured by FCNTL_CHUNK_SIZE
    szChunk: i32,
    /// Device sector size
    sectorSize: i32,
    /// Precomputed device characteristics
    deviceCharacteristics: i32,
}

fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(libc::EIO)
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn os_path(z: &[u8]) -> &std::path::Path {
    std::path::Path::new(std::ffi::OsStr::from_bytes(z))
}

fn cstr(z: &[u8]) -> std::ffi::CString {
    std::ffi::CString::new(crate::os::zpath_name(z).to_vec()).unwrap_or_default()
}

/// C osAccess()
fn osAccess(z: &[u8], mode: i32) -> i32 {
    let c = cstr(z);
    unsafe { libc::access(c.as_ptr(), mode) }
}

/// C osGetpid()
fn osGetpid() -> u32 {
    std::process::id()
}

/// Invoke open().  Do so multiple times, until it either succeeds or
/// fails for some reason other than EINTR.
///
/// If the file creation mode "m" is 0 then set it to the default.  If
/// the file descriptor that comes back is less than
/// SQLITE_MINIMUM_FILE_DESCRIPTOR, it is closed and /dev/null opened in
/// its place (so that stray writes to stdout/stderr cannot corrupt the
/// database).
fn robust_open(z: &[u8], f: i32, m: u32) -> Result<File, i32> {
    let m2 = if m != 0 {
        m
    } else {
        SQLITE_DEFAULT_FILE_PERMISSIONS
    };
    let path = os_path(crate::os::zpath_name(z));
    let fd = loop {
        let mut oo = std::fs::OpenOptions::new();
        let acc = f & libc::O_ACCMODE;
        oo.read(acc == libc::O_RDONLY || acc == libc::O_RDWR);
        oo.write(acc == libc::O_WRONLY || acc == libc::O_RDWR);
        if (f & libc::O_CREAT) != 0 {
            oo.create(true);
        }
        oo.custom_flags(f & !(libc::O_ACCMODE | libc::O_CREAT));
        oo.mode(m2);
        match oo.open(path) {
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break Err(errno_of(&e));
            }
            Ok(file) => {
                let n = file.as_raw_fd();
                if n >= SQLITE_MINIMUM_FILE_DESCRIPTOR {
                    break Ok(file);
                }
                if (f & (libc::O_EXCL | libc::O_CREAT)) == (libc::O_EXCL | libc::O_CREAT) {
                    let _ = std::fs::remove_file(path);
                }
                drop(file);
                crate::printf::sqlite3_log(
                    SQLITE_WARNING,
                    b"attempt to open \"%s\" as file descriptor %d",
                    &[
                        crate::printf::PfArg::from(crate::os::zpath_name(z)),
                        crate::printf::PfArg::from(n),
                    ],
                );
                match File::open("/dev/null") {
                    Ok(d) => std::mem::forget(d),
                    Err(e) => break Err(errno_of(&e)),
                }
            }
        }
    };
    if let Ok(file) = &fd {
        if m != 0 {
            if let Ok(md) = file.metadata() {
                if md.len() == 0 && (md.mode() & 0o777) != m {
                    let _ = file.set_permissions(std::fs::Permissions::from_mode(m));
                }
            }
        }
    }
    fd
}

/// This function translates a POSIX error into a SQLite error code.
fn sqliteErrorFromPosixError(posixError: i32, sqliteIOErr: i32) -> i32 {
    match posixError {
        libc::EACCES
        | libc::EAGAIN
        | libc::ETIMEDOUT
        | libc::EBUSY
        | libc::EINTR
        | libc::ENOLCK => SQLITE_BUSY,
        libc::EPERM => SQLITE_PERM,
        _ => sqliteIOErr,
    }
}

/// Log an I/O error (C unixLogErrorAtLine).  Returns errcode.
fn unixLogError(errcode: i32, zFunc: &str, zPath: Option<&[u8]>, iErrno: i32) -> i32 {
    let zErr = std::io::Error::from_raw_os_error(iErrno).to_string();
    let zPath = zPath.map(crate::os::zpath_name).unwrap_or(b"");
    crate::printf::sqlite3_log(
        errcode,
        b"os_unix.c:%d: (%d) %s(%s) - %s",
        &[
            crate::printf::PfArg::from(0i32),
            crate::printf::PfArg::from(iErrno),
            crate::printf::PfArg::from(zFunc.as_bytes()),
            crate::printf::PfArg::from(zPath),
            crate::printf::PfArg::from(zErr.as_bytes()),
        ],
    );
    errcode
}

fn storeLastErrno(pFile: &mut UnixFile, error: i32) {
    pFile.lastErrno = error;
}

/// Close all file descriptors accumulated in the unixInodeInfo->pUnused
/// list.
fn closePendingFds(st: &mut InodeLock) {
    st.pUnused.clear();
}

/// Release a unixInodeInfo structure previously allocated by
/// findInodeInfo().  The big lock must be held.
fn releaseInodeInfo(list: &mut Vec<Arc<UnixInodeInfo>>, pFile: &mut UnixFile) {
    if let Some(pInode) = pFile.pInode.take() {
        let n = pInode.nRef.fetch_sub(1, Ordering::SeqCst) - 1;
        if n == 0 {
            closePendingFds(&mut lockInode(&pInode));
            list.retain(|p| !Arc::ptr_eq(p, &pInode));
        }
    }
}

/// Given a file descriptor, locate the unixInodeInfo object that
/// describes that file descriptor.  Create a new one if necessary.  The
/// big lock must be held.
fn findInodeInfo(list: &mut Vec<Arc<UnixInodeInfo>>, pFile: &mut UnixFile) -> i32 {
    let md = match pFile.h.as_ref().unwrap().metadata() {
        Ok(m) => m,
        Err(e) => {
            storeLastErrno(pFile, errno_of(&e));
            return SQLITE_IOERR;
        }
    };
    let fileId = (md.dev(), md.ino());
    let pInode = match list.iter().find(|p| p.fileId == fileId) {
        Some(p) => {
            p.nRef.fetch_add(1, Ordering::SeqCst);
            p.clone()
        }
        None => {
            let p = Arc::new(UnixInodeInfo {
                fileId,
                nRef: AtomicI32::new(1),
                lock: Mutex::new(InodeLock {
                    nShared: 0,
                    nLock: 0,
                    eFileLock: 0,
                    bProcessLock: 0,
                    pUnused: Vec::new(),
                }),
            });
            list.insert(0, p.clone());
            p
        }
    };
    pFile.pInode = Some(pInode);
    SQLITE_OK
}

/// Return TRUE if pFile has been renamed or unlinked since it was first
/// opened.
fn fileHasMoved(pFile: &UnixFile) -> bool {
    match (&pFile.pInode, &pFile.zPath) {
        (Some(pInode), Some(z)) => match std::fs::metadata(os_path(crate::os::zpath_name(z))) {
            Err(_) => true,
            Ok(m) => m.ino() != pInode.fileId.1,
        },
        (Some(_), None) => true,
        _ => false,
    }
}

/// Check a unixFile that is a database.  Verify the following:
/// (1) There is exactly one hard link on the file
/// (2) The file is not a symbolic link
/// (3) The file has not been renamed or unlinked
/// Issue sqlite3_log(SQLITE_WARNING,...) messages if anything is not right.
fn verifyDbFile(pFile: &UnixFile) {
    if (pFile.ctrlFlags & UNIXFILE_NOLOCK) != 0 {
        return;
    }
    let zPath = pFile
        .zPath
        .as_deref()
        .map(crate::os::zpath_name)
        .unwrap_or(b"");
    let warn = |m: &[u8]| {
        crate::printf::sqlite3_log(SQLITE_WARNING, m, &[crate::printf::PfArg::from(zPath)]);
    };
    let md = match pFile.h.as_ref().map(|h| h.metadata()) {
        Some(Ok(m)) => m,
        _ => {
            warn(b"cannot fstat db file %s");
            return;
        }
    };
    if md.nlink() == 0 {
        warn(b"file unlinked while open: %s");
        return;
    }
    if md.nlink() > 1 {
        warn(b"multiple links to file: %s");
        return;
    }
    if fileHasMoved(pFile) {
        warn(b"file renamed while open: %s");
    }
}

/// A POSIX advisory lock request (C struct flock, l_whence==SEEK_SET).
#[derive(Copy, Clone)]
struct Flock {
    l_type: i32,
    l_start: i64,
    l_len: i64,
}

fn to_libc_flock(l: &Flock) -> libc::flock {
    let mut f: libc::flock = unsafe { std::mem::zeroed() };
    f.l_type = l.l_type as libc::c_short;
    f.l_whence = libc::SEEK_SET as libc::c_short;
    f.l_start = l.l_start as libc::off_t;
    f.l_len = l.l_len as libc::off_t;
    f
}

/// C osFcntl(h, F_SETLK, pLock).  Returns 0 on success or -1 (errno set).
fn osSetPosixAdvisoryLock(h: &File, pLock: &Flock) -> i32 {
    let f = to_libc_flock(pLock);
    unsafe { libc::fcntl(h.as_raw_fd(), libc::F_SETLK, &f as *const libc::flock) }
}

/// This routine checks if there is a RESERVED lock held on the specified
/// file by this or any other process.
fn unixCheckReservedLock(pFile: &mut UnixFile, pResOut: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mut reserved = 0;
    debug_assert!(pFile.eFileLock as i32 <= SHARED_LOCK);
    let pInode = pFile.pInode.clone().unwrap();
    let st = lockInode(&pInode);
    // Check if a thread in this process holds such a lock
    if st.eFileLock as i32 > SHARED_LOCK {
        reserved = 1;
    }
    // Otherwise see if some other process holds it.
    if reserved == 0 && st.bProcessLock == 0 {
        let lock = Flock {
            l_type: libc::F_WRLCK as i32,
            l_start: RESERVED_BYTE(),
            l_len: 1,
        };
        let mut f = to_libc_flock(&lock);
        let h = pFile.h.as_ref().map(|h| h.as_raw_fd()).unwrap_or(-1);
        if unsafe { libc::fcntl(h, libc::F_GETLK, &mut f as *mut libc::flock) } != 0 {
            rc = SQLITE_IOERR_CHECKRESERVEDLOCK;
            pFile.lastErrno = last_errno();
        } else if f.l_type as i32 != libc::F_UNLCK as i32 {
            reserved = 1;
        }
    }
    drop(st);
    *pResOut = reserved;
    rc
}

/// Attempt to set a system-lock on the file pFile.  The lock is described
/// by pLock.  If the pFile was opened read/write from unix-excl, then the
/// only lock ever obtained is an exclusive lock, and it is obtained
/// exactly once the first time any lock is attempted.
///
/// Returns 0 on success, or -1 with the errno in *pErrno.
fn unixFileLock(pFile: &UnixFile, st: &mut InodeLock, pLock: &Flock, pErrno: &mut i32) -> i32 {
    let h = match &pFile.h {
        Some(h) => h,
        None => {
            *pErrno = libc::EBADF;
            return -1;
        }
    };
    let rc;
    if (pFile.ctrlFlags & (UNIXFILE_EXCL | UNIXFILE_RDONLY)) == UNIXFILE_EXCL {
        if st.bProcessLock == 0 {
            let lock = Flock {
                l_type: libc::F_WRLCK as i32,
                l_start: SHARED_FIRST(),
                l_len: SHARED_SIZE as i64,
            };
            let r = osSetPosixAdvisoryLock(h, &lock);
            if r < 0 {
                *pErrno = last_errno();
                return r;
            }
            st.bProcessLock = 1;
            st.nLock += 1;
        }
        rc = 0;
    } else {
        rc = osSetPosixAdvisoryLock(h, pLock);
        if rc != 0 {
            *pErrno = last_errno();
        }
    }
    rc
}

/// Lock the file with the lock specified by parameter eFileLock - one of
/// SHARED_LOCK, RESERVED_LOCK, PENDING_LOCK or EXCLUSIVE_LOCK (C
/// unixLock(); see os_unix.c for the full description of the protocol).
fn unixLock(pFile: &mut UnixFile, eFileLock: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mut tErrno = 0;

    // If there is already a lock of this type or more restrictive on the
    // unixFile, do nothing.
    if pFile.eFileLock as i32 >= eFileLock {
        return SQLITE_OK;
    }

    // Make sure the locking sequence is correct.
    debug_assert!(pFile.eFileLock as i32 != NO_LOCK || eFileLock == SHARED_LOCK);
    debug_assert!(eFileLock != PENDING_LOCK);
    debug_assert!(eFileLock != RESERVED_LOCK || pFile.eFileLock as i32 == SHARED_LOCK);

    // This mutex is needed because pFile->pInode is shared across threads
    let pInode = pFile.pInode.clone().unwrap();
    let mut st = lockInode(&pInode);

    'end_lock: {
        // If some thread using this PID has a lock via a different
        // unixFile* handle that precludes the requested lock, return BUSY.
        if pFile.eFileLock != st.eFileLock
            && (st.eFileLock as i32 >= PENDING_LOCK || eFileLock > SHARED_LOCK)
        {
            rc = SQLITE_BUSY;
            break 'end_lock;
        }

        // If a SHARED lock is requested, and some thread using this PID
        // already has a SHARED or RESERVED lock, then increment reference
        // counts and return SQLITE_OK.
        if eFileLock == SHARED_LOCK
            && (st.eFileLock as i32 == SHARED_LOCK || st.eFileLock as i32 == RESERVED_LOCK)
        {
            debug_assert!(pFile.eFileLock == 0);
            debug_assert!(st.nShared > 0);
            pFile.eFileLock = SHARED_LOCK as u8;
            st.nShared += 1;
            st.nLock += 1;
            break 'end_lock;
        }

        // A PENDING lock is needed before acquiring a SHARED lock and
        // before acquiring an EXCLUSIVE lock.  For the SHARED lock, the
        // PENDING will be released.
        let mut lock = Flock {
            l_type: 0,
            l_start: 0,
            l_len: 1,
        };
        if eFileLock == SHARED_LOCK
            || (eFileLock == EXCLUSIVE_LOCK && pFile.eFileLock as i32 == RESERVED_LOCK)
        {
            lock.l_type = if eFileLock == SHARED_LOCK {
                libc::F_RDLCK as i32
            } else {
                libc::F_WRLCK as i32
            };
            lock.l_start = PENDING_BYTE();
            if unixFileLock(pFile, &mut st, &lock, &mut tErrno) != 0 {
                rc = sqliteErrorFromPosixError(tErrno, SQLITE_IOERR_LOCK);
                if rc != SQLITE_BUSY {
                    storeLastErrno(pFile, tErrno);
                }
                break 'end_lock;
            } else if eFileLock == EXCLUSIVE_LOCK {
                pFile.eFileLock = PENDING_LOCK as u8;
                st.eFileLock = PENDING_LOCK as u8;
            }
        }

        // If control gets to this point, then actually go ahead and make
        // operating system calls for the specified lock.
        if eFileLock == SHARED_LOCK {
            debug_assert!(st.nShared == 0);
            debug_assert!(st.eFileLock == 0);
            debug_assert!(rc == SQLITE_OK);

            // Now get the read-lock
            lock.l_start = SHARED_FIRST();
            lock.l_len = SHARED_SIZE as i64;
            if unixFileLock(pFile, &mut st, &lock, &mut tErrno) != 0 {
                rc = sqliteErrorFromPosixError(tErrno, SQLITE_IOERR_LOCK);
            }

            // Drop the temporary PENDING lock
            lock.l_start = PENDING_BYTE();
            lock.l_len = 1;
            lock.l_type = libc::F_UNLCK as i32;
            let mut e2 = 0;
            if unixFileLock(pFile, &mut st, &lock, &mut e2) != 0 && rc == SQLITE_OK {
                // This could happen with a network mount
                tErrno = e2;
                rc = SQLITE_IOERR_UNLOCK;
            }

            if rc != 0 {
                if rc != SQLITE_BUSY {
                    storeLastErrno(pFile, tErrno);
                }
                break 'end_lock;
            } else {
                pFile.eFileLock = SHARED_LOCK as u8;
                st.nLock += 1;
                st.nShared = 1;
            }
        } else if eFileLock == EXCLUSIVE_LOCK && st.nShared > 1 {
            // We are trying for an exclusive lock but another thread in
            // this same process is still holding a shared lock.
            rc = SQLITE_BUSY;
        } else {
            // The request was for a RESERVED or EXCLUSIVE lock.  It is
            // assumed that there is a SHARED or greater lock on the file
            // already.
            debug_assert!(pFile.eFileLock != 0);
            lock.l_type = libc::F_WRLCK as i32;
            if eFileLock == RESERVED_LOCK {
                lock.l_start = RESERVED_BYTE();
                lock.l_len = 1;
            } else {
                lock.l_start = SHARED_FIRST();
                lock.l_len = SHARED_SIZE as i64;
            }
            if unixFileLock(pFile, &mut st, &lock, &mut tErrno) != 0 {
                rc = sqliteErrorFromPosixError(tErrno, SQLITE_IOERR_LOCK);
                if rc != SQLITE_BUSY {
                    storeLastErrno(pFile, tErrno);
                }
            }
        }

        if rc == SQLITE_OK {
            pFile.eFileLock = eFileLock as u8;
            st.eFileLock = eFileLock as u8;
        }
    }
    drop(st);
    rc
}

/// Add the file descriptor used by file handle pFile to the corresponding
/// pUnused list.
fn setPendingFd(pFile: &mut UnixFile, st: &mut InodeLock) {
    let flags = pFile.pPreallocatedUnused.take().unwrap_or(0);
    if let Some(fd) = pFile.h.take() {
        st.pUnused.push(UnixUnusedFd { fd, flags });
    }
}

/// Lower the locking level on file descriptor pFile to eFileLock.
/// eFileLock must be either NO_LOCK or SHARED_LOCK.
fn posixUnlock(pFile: &mut UnixFile, eFileLock: i32) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(eFileLock <= SHARED_LOCK);
    if pFile.eFileLock as i32 <= eFileLock {
        return SQLITE_OK;
    }
    let pInode = pFile.pInode.clone().unwrap();
    let mut st = lockInode(&pInode);
    debug_assert!(st.nShared != 0);
    let mut e = 0;
    'end_unlock: {
        if pFile.eFileLock as i32 > SHARED_LOCK {
            debug_assert!(st.eFileLock == pFile.eFileLock);

            // downgrading to a shared lock on NFS involves clearing the
            // write lock before establishing the readlock - to avoid a race
            // condition we downgrade the lock in 2 steps (NFS only; not
            // ported).
            if eFileLock == SHARED_LOCK {
                let lock = Flock {
                    l_type: libc::F_RDLCK as i32,
                    l_start: SHARED_FIRST(),
                    l_len: SHARED_SIZE as i64,
                };
                if unixFileLock(pFile, &mut st, &lock, &mut e) != 0 {
                    // In theory, the call to unixFileLock() cannot fail
                    // because another process is holding an incompatible
                    // lock.
                    rc = SQLITE_IOERR_RDLOCK;
                    storeLastErrno(pFile, e);
                    break 'end_unlock;
                }
            }
            let lock = Flock {
                l_type: libc::F_UNLCK as i32,
                l_start: PENDING_BYTE(),
                l_len: 2,
            };
            if unixFileLock(pFile, &mut st, &lock, &mut e) == 0 {
                st.eFileLock = SHARED_LOCK as u8;
            } else {
                rc = SQLITE_IOERR_UNLOCK;
                storeLastErrno(pFile, e);
                break 'end_unlock;
            }
        }
        if eFileLock == NO_LOCK {
            // Decrement the shared lock counter.  Release the lock using an
            // OS call only when all threads in this same process have
            // released the lock.
            st.nShared -= 1;
            if st.nShared == 0 {
                let lock = Flock {
                    l_type: libc::F_UNLCK as i32,
                    l_start: 0,
                    l_len: 0,
                };
                if unixFileLock(pFile, &mut st, &lock, &mut e) == 0 {
                    st.eFileLock = NO_LOCK as u8;
                } else {
                    rc = SQLITE_IOERR_UNLOCK;
                    storeLastErrno(pFile, e);
                    st.eFileLock = NO_LOCK as u8;
                    pFile.eFileLock = NO_LOCK as u8;
                }
            }

            // Decrement the count of locks against this same file.  When
            // the count reaches zero, close any other file descriptors
            // whose close was deferred because of outstanding locks.
            st.nLock -= 1;
            debug_assert!(st.nLock >= 0);
            if st.nLock == 0 {
                closePendingFds(&mut st);
            }
        }
    }
    drop(st);
    if rc == SQLITE_OK {
        pFile.eFileLock = eFileLock as u8;
    }
    rc
}

/// Lower the locking level on file descriptor pFile to eFileLock.
fn unixUnlock(pFile: &mut UnixFile, eFileLock: i32) -> i32 {
    posixUnlock(pFile, eFileLock)
}

/// This function performs the parts of the "close file" operation common
/// to all locking schemes.
fn closeUnixFile(pFile: &mut UnixFile) -> i32 {
    // Dropping the File closes the descriptor (errors are ignored, as in
    // robust_close() which only logs them).
    pFile.h = None;
    pFile.pPreallocatedUnused = None;
    pFile.pInode = None;
    pFile.eFileLock = 0;
    pFile.ctrlFlags = 0;
    pFile.zPath = None;
    SQLITE_OK
}

/// Close a file.
pub(crate) fn unixClose(pFile: &mut UnixFile) -> i32 {
    if pFile.pMethod == UnixMethods::Nolock {
        return closeUnixFile(pFile);
    }
    verifyDbFile(pFile);
    unixUnlock(pFile, NO_LOCK);
    let mut list = unixEnterMutex();
    if let Some(pInode) = pFile.pInode.clone() {
        let mut st = lockInode(&pInode);
        debug_assert!(st.nLock > 0 || st.bProcessLock == 0);
        if st.nLock != 0 {
            // If there are outstanding locks, do not actually close the
            // file just yet because that would clear those locks.
            // Instead, add the file descriptor to pInode->pUnused list.
            // It will be automatically closed when the last lock is
            // cleared.
            setPendingFd(pFile, &mut st);
        }
    }
    releaseInodeInfo(&mut list, pFile);
    let rc = closeUnixFile(pFile);
    drop(list);
    rc
}

/// xLock method (dispatch on the locking style)
pub(crate) fn unixLockMethod(pFile: &mut UnixFile, eFileLock: i32) -> i32 {
    match pFile.pMethod {
        UnixMethods::Posix => unixLock(pFile, eFileLock),
        UnixMethods::Nolock => SQLITE_OK,
    }
}

/// xUnlock method
pub(crate) fn unixUnlockMethod(pFile: &mut UnixFile, eFileLock: i32) -> i32 {
    match pFile.pMethod {
        UnixMethods::Posix => unixUnlock(pFile, eFileLock),
        UnixMethods::Nolock => SQLITE_OK,
    }
}

/// xCheckReservedLock method
pub(crate) fn unixCheckReservedLockMethod(pFile: &mut UnixFile, pResOut: &mut i32) -> i32 {
    match pFile.pMethod {
        UnixMethods::Posix => unixCheckReservedLock(pFile, pResOut),
        UnixMethods::Nolock => {
            *pResOut = 0;
            SQLITE_OK
        }
    }
}

/// Seek to the offset passed as the second argument, then read cnt bytes
/// into pBuf.  Return the number of bytes actually read, or -1 on error.
fn seekAndRead(id: &mut UnixFile, mut offset: i64, pBuf: &mut [u8]) -> i32 {
    let mut prior: i32 = 0;
    let mut got: i32;
    let mut pos = 0usize;
    let mut cnt = pBuf.len();
    let h = match &id.h {
        Some(h) => h,
        None => {
            id.lastErrno = libc::EBADF;
            return -1;
        }
    };
    loop {
        match h.read_at(&mut pBuf[pos..pos + cnt], offset as u64) {
            Ok(n) => {
                got = n as i32;
                if n == cnt {
                    break;
                }
                if n > 0 {
                    cnt -= n;
                    offset += n as i64;
                    prior += got;
                    pos += n;
                } else {
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                got = -1;
                prior = 0;
                id.lastErrno = errno_of(&e);
                break;
            }
        }
    }
    got + prior
}

/// Read data from a file into a buffer.  Return SQLITE_OK if all bytes
/// were read successfully and SQLITE_IOERR if anything goes wrong.
pub(crate) fn unixRead(pFile: &mut UnixFile, pBuf: &mut [u8], offset: i64) -> i32 {
    debug_assert!(offset >= 0);
    let amt = pBuf.len() as i32;
    debug_assert!(amt > 0);
    let got = seekAndRead(pFile, offset, pBuf);
    if got == amt {
        SQLITE_OK
    } else if got < 0 {
        // pFile->lastErrno has been set by seekAndRead().
        match pFile.lastErrno {
            libc::ERANGE | libc::EIO | libc::ENXIO => SQLITE_IOERR_CORRUPTFS,
            _ => SQLITE_IOERR_READ,
        }
    } else {
        storeLastErrno(pFile, 0); // not a system error
        // Unread parts of the buffer must be zero-filled
        for b in pBuf[got as usize..].iter_mut() {
            *b = 0;
        }
        SQLITE_IOERR_SHORT_READ
    }
}

/// Seek to the offset in id->offset then read cnt bytes into pBuf.
/// Return the number of bytes actually written, or -1 on error.
fn seekAndWrite(id: &mut UnixFile, iOff: i64, pBuf: &[u8]) -> i32 {
    let nBuf = pBuf.len() & 0x1ffff;
    let h = match &id.h {
        Some(h) => h,
        None => {
            id.lastErrno = libc::EBADF;
            return -1;
        }
    };
    loop {
        match h.write_at(&pBuf[..nBuf], iOff as u64) {
            Ok(n) => return n as i32,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                id.lastErrno = errno_of(&e);
                return -1;
            }
        }
    }
}

/// Write data from a buffer into a file.  Return SQLITE_OK on success or
/// some other error code on failure.
pub(crate) fn unixWrite(pFile: &mut UnixFile, pBuf: &[u8], mut offset: i64) -> i32 {
    let mut amt = pBuf.len() as i32;
    debug_assert!(amt > 0);
    let mut pos = 0usize;
    let mut wrote;
    loop {
        wrote = seekAndWrite(pFile, offset, &pBuf[pos..]);
        if !(wrote < amt && wrote > 0) {
            break;
        }
        amt -= wrote;
        offset += wrote as i64;
        pos += wrote as usize;
    }
    if amt > wrote {
        if wrote < 0 && pFile.lastErrno != libc::ENOSPC {
            // lastErrno set by seekAndWrite
            return SQLITE_IOERR_WRITE;
        } else {
            storeLastErrno(pFile, 0); // not a system error
            return SQLITE_FULL;
        }
    }
    SQLITE_OK
}

/// The fsync() system call does not work as advertised on many unix
/// systems.  On Linux, fdatasync() is used (C full_fsync()).
fn full_fsync(fd: &File, _fullSync: i32, _dataOnly: i32) -> i32 {
    match fd.sync_data() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Open a file descriptor to the directory containing file zFilename.
/// If successful, *pFd is set to the opened file descriptor and
/// SQLITE_OK is returned.  If an error occurs, either SQLITE_NOMEM or
/// SQLITE_CANTOPEN is returned and *pFd is set to an undefined value.
fn openDirectory(zFilename: &[u8], pFd: &mut Option<File>) -> i32 {
    let mut zDirname: ZStr = crate::os::zpath_name(zFilename).to_vec();
    if zDirname.len() > MAX_PATHNAME as usize - 1 {
        zDirname.truncate(MAX_PATHNAME as usize - 1);
    }
    // for(ii=strlen(z); ii>0 && z[ii]!='/'; ii--);  (z[strlen] is the NUL)
    let mut ii = zDirname.len();
    while ii > 0 && (ii >= zDirname.len() || zDirname[ii] != b'/') {
        ii -= 1;
    }
    if ii > 0 {
        zDirname.truncate(ii);
    } else if zDirname.first() != Some(&b'/') {
        zDirname = b".".to_vec();
    } else {
        zDirname = b"/".to_vec();
    }
    match robust_open(&zDirname, libc::O_RDONLY, 0) {
        Ok(f) => {
            *pFd = Some(f);
            SQLITE_OK
        }
        Err(e) => {
            *pFd = None;
            unixLogError(SQLITE_CANTOPEN, "openDirectory", Some(&zDirname), e)
        }
    }
}

/// Make sure all writes to a particular file are committed to disk.
///
/// If dirsync is pending (the file was newly created journal), the
/// directory containing the file is also fsynced.
pub(crate) fn unixSync(pFile: &mut UnixFile, flags: i32) -> i32 {
    let isDataOnly = flags & SQLITE_SYNC_DATAONLY;
    let isFullsync = ((flags & 0x0F) == SQLITE_SYNC_FULL) as i32;
    debug_assert!((flags & 0x0F) == SQLITE_SYNC_NORMAL || (flags & 0x0F) == SQLITE_SYNC_FULL);
    let mut rc = match &pFile.h {
        Some(h) => full_fsync(h, isFullsync, isDataOnly),
        None => -1,
    };
    if rc != 0 {
        let e = last_errno();
        storeLastErrno(pFile, e);
        return unixLogError(SQLITE_IOERR_FSYNC, "full_fsync", pFile.zPath.as_deref(), e);
    }

    // Also fsync the directory containing the file if the DIRSYNC flag is
    // set.  This is a one-time occurrence.  Many systems (examples: AIX)
    // are unable to fsync a directory, so ignore errors on the fsync.
    if (pFile.ctrlFlags & UNIXFILE_DIRSYNC) != 0 {
        let mut dirfd = None;
        let zPath = pFile.zPath.clone().unwrap_or_default();
        rc = openDirectory(&zPath, &mut dirfd);
        if rc == SQLITE_OK {
            if let Some(d) = &dirfd {
                full_fsync(d, 0, 0);
            }
        } else {
            debug_assert!(rc == SQLITE_CANTOPEN);
            rc = SQLITE_OK;
        }
        pFile.ctrlFlags &= !UNIXFILE_DIRSYNC;
    }
    rc
}

/// Truncate an open file to a specified size
pub(crate) fn unixTruncate(pFile: &mut UnixFile, mut nByte: i64) -> i32 {
    // If the user has configured a chunk-size for this file, truncate the
    // file so that it consists of an integer number of chunks (i.e. the
    // actual file size after the operation may be larger than the
    // requested size).
    if pFile.szChunk > 0 {
        let c = pFile.szChunk as i64;
        nByte = ((nByte + c - 1) / c) * c;
    }
    let r = match &pFile.h {
        Some(h) => loop {
            match h.set_len(nByte as u64) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                r => break r,
            }
        },
        None => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    };
    match r {
        Err(e) => {
            let e = errno_of(&e);
            storeLastErrno(pFile, e);
            unixLogError(
                SQLITE_IOERR_TRUNCATE,
                "ftruncate",
                pFile.zPath.as_deref(),
                e,
            )
        }
        Ok(()) => SQLITE_OK,
    }
}

/// Determine the current size of a file in bytes
pub(crate) fn unixFileSize(pFile: &mut UnixFile, pSize: &mut i64) -> i32 {
    let md = match &pFile.h {
        Some(h) => h.metadata(),
        None => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
    };
    match md {
        Err(e) => {
            storeLastErrno(pFile, errno_of(&e));
            SQLITE_IOERR_FSTAT
        }
        Ok(m) => {
            *pSize = m.len() as i64;
            // When opening a zero-size database, the findInodeInfo()
            // procedure writes a single byte into that file in order to
            // work around a bug in the OS-X msdos filesystem.  In order to
            // avoid problems with upper layers, we need to report this
            // file size as zero even though it is really 1.
            if *pSize == 1 {
                *pSize = 0;
            }
            SQLITE_OK
        }
    }
}

/// This function is called to handle the SQLITE_FCNTL_SIZE_HINT
/// file-control operation.  Enlarge the database to nBytes in size
/// (rounded up to the next chunk-size).  If the database is already
/// nBytes or larger, this routine is a no-op.
fn fcntlSizeHint(pFile: &mut UnixFile, nByte: i64) -> i32 {
    if pFile.szChunk > 0 {
        let md = match &pFile.h {
            Some(h) => match h.metadata() {
                Ok(m) => m,
                Err(_) => return SQLITE_IOERR_FSTAT,
            },
            None => return SQLITE_IOERR_FSTAT,
        };
        let c = pFile.szChunk as i64;
        let nSize = ((nByte + c - 1) / c) * c;
        let st_size = md.len() as i64;
        if nSize > st_size {
            // If the OS does not have posix_fallocate(), fake it.  Write a
            // single byte to the last byte in each block that falls
            // entirely within the extended region.  Then, if required, a
            // single byte at offset (nSize-1), to set the size of the file
            // correctly.
            let nBlk = (md.blksize() as i64).max(1);
            let mut iWrite = (st_size / nBlk) * nBlk + nBlk - 1;
            while iWrite < nSize + nBlk - 1 {
                if iWrite >= nSize {
                    iWrite = nSize - 1;
                }
                let nWrite = seekAndWrite(pFile, iWrite, b"\0");
                if nWrite != 1 {
                    return SQLITE_IOERR_WRITE;
                }
                iWrite += nBlk;
            }
        }
    }
    SQLITE_OK
}

/// If *pArg is initially negative then this is a query.  Set *pArg to 1
/// or 0 depending on whether or not bit mask of pFile->ctrlFlags is set.
///
/// If *pArg is 0 or 1, then clear or set the mask bit of pFile->ctrlFlags.
fn unixModeBit(pFile: &mut UnixFile, mask: u16, pArg: &mut i32) {
    if *pArg < 0 {
        *pArg = ((pFile.ctrlFlags & mask) != 0) as i32;
    } else if *pArg == 0 {
        pFile.ctrlFlags &= !mask;
    } else {
        pFile.ctrlFlags |= mask;
    }
}

/// Information and control of an open file handle.
pub(crate) fn unixFileControl(pFile: &mut UnixFile, op: i32, pArg: FcntlArg) -> i32 {
    match (op, pArg) {
        (SQLITE_FCNTL_NULL_IO, _) => {
            pFile.h = None;
            SQLITE_OK
        }
        (SQLITE_FCNTL_LOCKSTATE, FcntlArg::Int(p)) => {
            *p = pFile.eFileLock as i32;
            SQLITE_OK
        }
        (SQLITE_FCNTL_LAST_ERRNO, FcntlArg::Int(p)) => {
            *p = pFile.lastErrno;
            SQLITE_OK
        }
        (SQLITE_FCNTL_CHUNK_SIZE, FcntlArg::Int(p)) => {
            pFile.szChunk = *p;
            SQLITE_OK
        }
        (SQLITE_FCNTL_SIZE_HINT, FcntlArg::I64(p)) => fcntlSizeHint(pFile, *p),
        (SQLITE_FCNTL_PERSIST_WAL, FcntlArg::Int(p)) => {
            unixModeBit(pFile, UNIXFILE_PERSIST_WAL, p);
            SQLITE_OK
        }
        (SQLITE_FCNTL_POWERSAFE_OVERWRITE, FcntlArg::Int(p)) => {
            unixModeBit(pFile, UNIXFILE_PSOW, p);
            SQLITE_OK
        }
        (SQLITE_FCNTL_VFSNAME, FcntlArg::StrOut(p)) => {
            *p = Some(pFile.zVfsName.clone());
            SQLITE_OK
        }
        (SQLITE_FCNTL_TEMPFILENAME, FcntlArg::StrOut(p)) => {
            let mut z = ZStr::new();
            unixGetTempname(pFile.mxPathname, &mut z);
            *p = Some(z);
            SQLITE_OK
        }
        (SQLITE_FCNTL_HAS_MOVED, FcntlArg::Int(p)) => {
            *p = fileHasMoved(pFile) as i32;
            SQLITE_OK
        }
        (SQLITE_FCNTL_EXTERNAL_READER, FcntlArg::Int(p)) => {
            // No shared memory: there is never an external reader.
            *p = 0;
            SQLITE_OK
        }
        _ => SQLITE_NOTFOUND,
    }
}

/// Initialize sectorSize and deviceCharacteristics of the file.
fn setDeviceCharacteristics(pFd: &mut UnixFile) {
    debug_assert!(pFd.deviceCharacteristics == 0 || pFd.sectorSize != 0);
    if pFd.sectorSize == 0 {
        // Set the POWERSAFE_OVERWRITE flag if requested.
        if (pFd.ctrlFlags & UNIXFILE_PSOW) != 0 {
            pFd.deviceCharacteristics |= SQLITE_IOCAP_POWERSAFE_OVERWRITE;
        }
        pFd.deviceCharacteristics |= SQLITE_IOCAP_SUBPAGE_READ;
        pFd.sectorSize = SQLITE_DEFAULT_SECTOR_SIZE;
    }
}

/// Return the sector size in bytes of the underlying block device for the
/// specified file.
pub(crate) fn unixSectorSize(pFd: &mut UnixFile) -> i32 {
    setDeviceCharacteristics(pFd);
    pFd.sectorSize
}

/// Return the device characteristics for the file.
pub(crate) fn unixDeviceCharacteristics(pFd: &mut UnixFile) -> i32 {
    setDeviceCharacteristics(pFd);
    pFd.deviceCharacteristics
}

/// Initialize the contents of the unixFile structure pointed to by pId.
fn fillInUnixFile(
    pVfs: &Vfs,
    h: File,
    pId: &mut SqliteFile,
    zFilename: Option<&[u8]>,
    ctrlFlags: u16,
) -> i32 {
    let mut ctrlFlags = ctrlFlags;
    let mut pNew = UnixFile {
        pMethod: UnixMethods::Posix,
        zVfsName: pVfs.zName.clone(),
        mxPathname: pVfs.mxPathname,
        pInode: None,
        h: Some(h),
        eFileLock: 0,
        ctrlFlags,
        lastErrno: 0,
        pPreallocatedUnused: None,
        zPath: zFilename.map(|z| z.to_vec()),
        szChunk: 0,
        sectorSize: 0,
        deviceCharacteristics: 0,
    };
    if crate::os::uri_boolean(
        if (ctrlFlags & UNIXFILE_URI) != 0 {
            zFilename
        } else {
            None
        },
        b"psow",
        SQLITE_POWERSAFE_OVERWRITE,
    ) != 0
    {
        pNew.ctrlFlags |= UNIXFILE_PSOW;
    }
    if pVfs.zName == b"unix-excl" {
        pNew.ctrlFlags |= UNIXFILE_EXCL;
    }
    if pVfs.pAppData == UNIX_FINDER_NOLOCK {
        ctrlFlags |= UNIXFILE_NOLOCK;
    }
    let mut rc = SQLITE_OK;
    if (ctrlFlags & UNIXFILE_NOLOCK) != 0 {
        pNew.pMethod = UnixMethods::Nolock;
    } else {
        pNew.pMethod = UnixMethods::Posix;
        let mut list = unixEnterMutex();
        rc = findInodeInfo(&mut list, &mut pNew);
        if rc != SQLITE_OK {
            pNew.h = None;
        }
    }
    storeLastErrno(&mut pNew, 0);
    if rc == SQLITE_OK {
        verifyDbFile(&pNew);
        *pId = SqliteFile::Unix(pNew);
    }
    rc
}

/// Return the name of a directory in which to put temporary files.  If
/// no suitable temporary file directory can be found, return None.
fn unixTempFileDir() -> Option<ZStr> {
    for i in 0..7 {
        let zDir: Option<ZStr> = match i {
            0 => sqlite3_temp_directory
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            1 => std::env::var_os("SQLITE_TMPDIR").map(|s| s.as_bytes().to_vec()),
            2 => std::env::var_os("TMPDIR").map(|s| s.as_bytes().to_vec()),
            3 => Some(b"/var/tmp".to_vec()),
            4 => Some(b"/usr/tmp".to_vec()),
            5 => Some(b"/tmp".to_vec()),
            _ => Some(b".".to_vec()),
        };
        if let Some(z) = zDir {
            if let Ok(m) = std::fs::metadata(os_path(&z)) {
                if m.is_dir() && osAccess(&z, 0o3) == 0 {
                    return Some(z);
                }
            }
        }
    }
    None
}

/// Create a temporary file name in zBuf.  zBuf must be allocated by the
/// calling process and must be at least nBuf bytes in size.
fn unixGetTempname(nBuf: i32, zBuf: &mut ZStr) -> i32 {
    let mut iLimit = 0;
    let mut rc = SQLITE_OK;
    zBuf.clear();
    match unixTempFileDir() {
        None => {
            rc = SQLITE_IOERR_GETTEMPPATH;
        }
        Some(zDir) => loop {
            let mut r = [0u8; 8];
            crate::os::os_randomness(&mut r);
            let r = u64::from_ne_bytes(r);
            let mut z = zDir.clone();
            z.push(b'/');
            z.extend_from_slice(SQLITE_TEMP_FILE_PREFIX);
            z.extend_from_slice(format!("{:x}", r).as_bytes());
            // sqlite3_snprintf(nBuf, ...) followed by the zBuf[nBuf-2]
            // overflow check
            if z.len() > (nBuf - 2) as usize || {
                let l = iLimit;
                iLimit += 1;
                l > 10
            } {
                *zBuf = z;
                rc = SQLITE_ERROR;
                break;
            }
            *zBuf = z;
            if osAccess(zBuf, 0) != 0 {
                break;
            }
        },
    }
    rc
}

/// Search for an unused file descriptor that was opened on the database
/// file (not a journal or super-journal file) identified by pathname
/// zPath with SQLITE_OPEN_XXX flags matching those passed as the second
/// argument to this function.
fn findReusableFd(zPath: &[u8], flags: i32) -> Option<UnixUnusedFd> {
    let list = unixEnterMutex();
    if list.is_empty() {
        return None;
    }
    let sStat = std::fs::metadata(os_path(crate::os::zpath_name(zPath))).ok()?;
    let pInode = list
        .iter()
        .find(|p| p.fileId == (sStat.dev(), sStat.ino()))?
        .clone();
    let mut st = lockInode(&pInode);
    let flags = flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE);
    let pos = st.pUnused.iter().rposition(|u| u.flags == flags)?;
    let r = st.pUnused.remove(pos);
    drop(st);
    drop(list);
    Some(r)
}

/// Find the mode, uid and gid of file zFile.
fn getFileMode(zFile: &[u8], pMode: &mut u32, pUid: &mut u32, pGid: &mut u32) -> i32 {
    match std::fs::metadata(os_path(zFile)) {
        Ok(m) => {
            *pMode = m.mode() & 0o777;
            *pUid = m.uid();
            *pGid = m.gid();
            SQLITE_OK
        }
        Err(_) => SQLITE_IOERR_FSTAT,
    }
}

/// This function is called by unixOpen() to determine the unix
/// permissions to create new files with.
fn findCreateFileMode(
    zPath: &[u8],
    flags: i32,
    pMode: &mut u32,
    pUid: &mut u32,
    pGid: &mut u32,
) -> i32 {
    let mut rc = SQLITE_OK;
    *pMode = 0;
    *pUid = 0;
    *pGid = 0;
    if (flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL)) != 0 {
        // If this is a main database journal or WAL, the name of the
        // database is the text up to the last '-' that follows any '.'.
        let z = crate::os::zpath_name(zPath);
        let mut nDb = z.len() as isize - 1;
        while nDb > 0 && z[nDb as usize] != b'.' {
            if z[nDb as usize] == b'-' {
                rc = getFileMode(&z[..nDb as usize], pMode, pUid, pGid);
                break;
            }
            nDb -= 1;
        }
    } else if (flags & SQLITE_OPEN_DELETEONCLOSE) != 0 {
        *pMode = 0o600;
    } else if (flags & SQLITE_OPEN_URI) != 0 {
        // If this is a main database file and the file was opened using a
        // URI filename, check for the "modeof" parameter.
        if let Some(z) = crate::os::uri_parameter(Some(zPath), b"modeof") {
            rc = getFileMode(&z, pMode, pUid, pGid);
        }
    }
    rc
}

/// Open the file zPath (C unixOpen()).
fn unixOpen(
    pVfs: &Vfs,
    zPath: Option<&[u8]>,
    pFile: &mut SqliteFile,
    flags: i32,
    pOutFlags: Option<&mut i32>,
) -> i32 {
    let mut flags = flags;
    let mut fd: Option<File> = None;
    let mut openFlags: i32 = 0;
    let eType = flags & 0x0FFF00;
    let mut rc;
    let mut ctrlFlags: u16 = 0;

    let isExclusive = flags & SQLITE_OPEN_EXCLUSIVE;
    let isDelete = flags & SQLITE_OPEN_DELETEONCLOSE;
    let isCreate = flags & SQLITE_OPEN_CREATE;
    let mut isReadonly = flags & SQLITE_OPEN_READONLY;
    let isReadWrite = flags & SQLITE_OPEN_READWRITE;

    // If creating a super- or main-file journal, this function will open a
    // file descriptor on the directory too.  The first time unixSync() is
    // called the directory file descriptor will be fsync()ed and close()d.
    let isNewJrnl = isCreate != 0
        && (eType == SQLITE_OPEN_SUPER_JOURNAL
            || eType == SQLITE_OPEN_MAIN_JOURNAL
            || eType == SQLITE_OPEN_WAL);

    let mut zTmpname: ZStr = ZStr::new();
    let mut zName: Option<ZStr> = zPath.map(|z| z.to_vec());
    let mut pPreallocatedUnused: Option<i32> = None;

    debug_assert!((isReadonly == 0 || isReadWrite == 0) && (isReadWrite != 0 || isReadonly != 0));
    debug_assert!(isCreate == 0 || isReadWrite != 0);
    debug_assert!(isExclusive == 0 || isCreate != 0);
    debug_assert!(isDelete == 0 || isCreate != 0);

    // Detect a pid change and reset the PRNG.  There is a race condition
    // here such that two or more threads all trying to open databases at
    // the same instant might all reset the PRNG.  But multiple resets are
    // harmless.
    if RANDOMNESS_PID.load(Ordering::Relaxed) != osGetpid() {
        RANDOMNESS_PID.store(osGetpid(), Ordering::Relaxed);
        #[cfg(not(test))]
        crate::random::sqlite3_randomness(0, &mut []);
    }
    *pFile = SqliteFile::Closed;

    if eType == SQLITE_OPEN_MAIN_DB {
        let z = zName.as_deref().unwrap_or(b"");
        if let Some(u) = findReusableFd(z, flags) {
            fd = Some(u.fd);
        }
        pPreallocatedUnused = Some(0);
    } else if zName.is_none() {
        // If zName is NULL, the upper layer is requesting a temp file.
        debug_assert!(isDelete != 0 && !isNewJrnl);
        // On Linux, use O_TMPFILE when available: the file has no name.
        if let Some(zDir) = unixTempFileDir() {
            if let Ok(f) = robust_open(&zDir, libc::O_RDWR | libc::O_EXCL | libc::O_TMPFILE, 0o600)
            {
                rc = fillInUnixFile(pVfs, f, pFile, zPath, ctrlFlags | UNIXFILE_NOLOCK);
                return rc;
            }
        }
        rc = unixGetTempname(pVfs.mxPathname, &mut zTmpname);
        if rc != SQLITE_OK {
            return rc;
        }
        zName = Some(zTmpname.clone());
    }
    let zName = zName.unwrap_or_default();

    // Determine the value of the flags parameter passed to POSIX function
    // open().  These must be calculated even if open() is not called, as
    // they may be stored as part of the file handle and used by the
    // 'conch file' locking functions later on.
    if isReadonly != 0 {
        openFlags |= libc::O_RDONLY;
    }
    if isReadWrite != 0 {
        openFlags |= libc::O_RDWR;
    }
    if isCreate != 0 {
        openFlags |= libc::O_CREAT;
    }
    if isExclusive != 0 {
        openFlags |= libc::O_EXCL | libc::O_NOFOLLOW;
    }
    openFlags |= libc::O_LARGEFILE | libc::O_NOFOLLOW;

    if fd.is_none() {
        let mut openMode = 0u32;
        let mut uid = 0u32;
        let mut gid = 0u32;
        rc = findCreateFileMode(&zName, flags, &mut openMode, &mut uid, &mut gid);
        if rc != SQLITE_OK {
            debug_assert!(eType == SQLITE_OPEN_WAL || eType == SQLITE_OPEN_MAIN_JOURNAL);
            return rc;
        }
        let mut r = robust_open(&zName, openFlags, openMode);
        if let Err(errno) = r {
            if isNewJrnl && errno == libc::EACCES && osAccess(&zName, libc::F_OK) != 0 {
                // If unable to create a journal because the directory is
                // not writable, change the error code to indicate that.
                rc = SQLITE_READONLY_DIRECTORY;
            } else if errno != libc::EISDIR && isReadWrite != 0 {
                // Failed to open the file for read/write access.  Try
                // read-only.
                flags &= !(SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
                openFlags &= !(libc::O_RDWR | libc::O_CREAT);
                flags |= SQLITE_OPEN_READONLY;
                openFlags |= libc::O_RDONLY;
                isReadonly = 1;
                if let Some(u) = findReusableFd(&zName, flags) {
                    r = Ok(u.fd);
                } else {
                    r = robust_open(&zName, openFlags, openMode);
                }
            }
        }
        match r {
            Err(errno) => {
                let rc2 = unixLogError(SQLITE_CANTOPEN, "open", Some(&zName), errno);
                if rc == SQLITE_OK {
                    rc = rc2;
                }
                return rc;
            }
            Ok(f) => {
                // The owner of the rollback journal or WAL file should
                // always be the same as the owner of the database file.
                if openMode != 0 && (flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL)) != 0 {
                    // robustFchown(): only root changes ownership
                    if unsafe { libc::geteuid() } == 0 {
                        let _ = std::os::unix::fs::fchown(&f, Some(uid), Some(gid));
                    }
                }
                fd = Some(f);
            }
        }
    }
    let fd = fd.unwrap();
    if let Some(o) = pOutFlags {
        *o = flags;
    }
    if pPreallocatedUnused.is_some() {
        pPreallocatedUnused = Some(flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE));
    }

    if isDelete != 0 {
        let _ = std::fs::remove_file(os_path(&zName));
    }

    // Set up appropriate ctrlFlags
    if isDelete != 0 {
        ctrlFlags |= UNIXFILE_DELETE;
    }
    if isReadonly != 0 {
        ctrlFlags |= UNIXFILE_RDONLY;
    }
    let noLock = eType != SQLITE_OPEN_MAIN_DB;
    if noLock {
        ctrlFlags |= UNIXFILE_NOLOCK;
    }
    if isNewJrnl {
        ctrlFlags |= UNIXFILE_DIRSYNC;
    }
    if (flags & SQLITE_OPEN_URI) != 0 {
        ctrlFlags |= UNIXFILE_URI;
    }

    rc = fillInUnixFile(pVfs, fd, pFile, zPath, ctrlFlags);
    if rc == SQLITE_OK {
        if let SqliteFile::Unix(p) = pFile {
            p.pPreallocatedUnused = pPreallocatedUnused;
        }
    }
    rc
}

/// Delete the file at zPath.  If the dirSync argument is true, fsync()
/// the directory after deleting the file.
fn unixDelete(_pVfs: &Vfs, zPath: &[u8], dirSync: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let z = crate::os::zpath_name(zPath);
    if let Err(e) = std::fs::remove_file(os_path(z)) {
        let errno = errno_of(&e);
        if errno == libc::ENOENT {
            rc = SQLITE_IOERR_DELETE_NOENT;
        } else {
            rc = unixLogError(SQLITE_IOERR_DELETE, "unlink", Some(z), errno);
        }
        return rc;
    }
    if (dirSync & 1) != 0 {
        let mut fd = None;
        rc = openDirectory(z, &mut fd);
        if rc == SQLITE_OK {
            if let Some(d) = &fd {
                if full_fsync(d, 0, 0) != 0 {
                    rc = unixLogError(SQLITE_IOERR_DIR_FSYNC, "fsync", Some(z), last_errno());
                }
            }
        } else {
            debug_assert!(rc == SQLITE_CANTOPEN);
            rc = SQLITE_OK;
        }
    }
    rc
}

/// Test the existence of or access permissions of file zPath.
fn unixAccess(_pVfs: &Vfs, zPath: &[u8], flags: i32, pResOut: &mut i32) -> i32 {
    debug_assert!(flags == SQLITE_ACCESS_EXISTS || flags == SQLITE_ACCESS_READWRITE);
    let z = crate::os::zpath_name(zPath);
    if flags == SQLITE_ACCESS_EXISTS {
        *pResOut = match std::fs::metadata(os_path(z)) {
            Ok(m) => (!m.file_type().is_file() || m.len() > 0) as i32,
            Err(_) => 0,
        };
    } else {
        *pResOut = (osAccess(z, libc::W_OK | libc::R_OK) == 0) as i32;
    }
    SQLITE_OK
}

/// A pathname under construction (C DbPath)
struct DbPath<'a> {
    rc: i32,
    nSymlink: i32,
    zOut: &'a mut ZStr,
    nOut: usize,
}

/// Append a single path element to the DbPath under construction
fn appendOnePathElement(pPath: &mut DbPath, zName: &[u8]) {
    let nName = zName.len();
    debug_assert!(nName > 0);
    if zName[0] == b'.' {
        if nName == 1 {
            return;
        }
        if zName[1] == b'.' && nName == 2 {
            if pPath.zOut.len() > 1 {
                debug_assert!(pPath.zOut[0] == b'/');
                loop {
                    let c = pPath.zOut.pop();
                    if c == Some(b'/') || c.is_none() {
                        break;
                    }
                }
            }
            return;
        }
    }
    if pPath.zOut.len() + nName + 2 >= pPath.nOut {
        pPath.rc = SQLITE_ERROR;
        return;
    }
    pPath.zOut.push(b'/');
    pPath.zOut.extend_from_slice(zName);
    if pPath.rc == SQLITE_OK {
        let zIn = pPath.zOut.clone();
        match std::fs::symlink_metadata(os_path(&zIn)) {
            Err(e) => {
                let errno = errno_of(&e);
                if errno != libc::ENOENT {
                    pPath.rc = unixLogError(SQLITE_CANTOPEN, "lstat", Some(&zIn), errno);
                }
            }
            Ok(m) => {
                if m.file_type().is_symlink() {
                    let n = pPath.nSymlink;
                    pPath.nSymlink += 1;
                    if n > SQLITE_MAX_SYMLINK {
                        pPath.rc = SQLITE_CANTOPEN;
                        return;
                    }
                    let zLnk = match std::fs::read_link(os_path(&zIn)) {
                        Ok(l) => l.as_os_str().as_bytes().to_vec(),
                        Err(e) => {
                            pPath.rc =
                                unixLogError(SQLITE_CANTOPEN, "readlink", Some(&zIn), errno_of(&e));
                            return;
                        }
                    };
                    if zLnk.is_empty() || zLnk.len() >= SQLITE_MAX_PATHLEN {
                        pPath.rc = unixLogError(SQLITE_CANTOPEN, "readlink", Some(&zIn), 0);
                        return;
                    }
                    if zLnk[0] == b'/' {
                        pPath.zOut.clear();
                    } else {
                        let l = pPath.zOut.len() - (nName + 1);
                        pPath.zOut.truncate(l);
                    }
                    appendAllPathElements(pPath, &zLnk);
                }
            }
        }
    }
}

/// Append all path elements in zPath to the DbPath under construction.
fn appendAllPathElements(pPath: &mut DbPath, zPath: &[u8]) {
    let mut i = 0usize;
    let mut j = 0usize;
    loop {
        while i < zPath.len() && zPath[i] != b'/' {
            i += 1;
        }
        if i > j {
            appendOnePathElement(pPath, &zPath[j..i]);
        }
        j = i + 1;
        let end = i >= zPath.len();
        i += 1;
        if end {
            break;
        }
    }
}

/// Turn a relative pathname into a full pathname.  The relative path is
/// stored as a nul-terminated string in the buffer pointed to by zPath.
fn unixFullPathname(_pVfs: &Vfs, zPath: &[u8], nOut: i32, zOut: &mut ZStr) -> i32 {
    let zPath = crate::os::zpath_name(zPath);
    zOut.clear();
    let mut path = DbPath {
        rc: 0,
        nSymlink: 0,
        zOut,
        nOut: nOut as usize,
    };
    if zPath.first() != Some(&b'/') {
        let zPwd = match std::env::current_dir() {
            Ok(p) => p.as_os_str().as_bytes().to_vec(),
            Err(e) => {
                return unixLogError(SQLITE_CANTOPEN, "getcwd", Some(zPath), errno_of(&e));
            }
        };
        appendAllPathElements(&mut path, &zPwd);
    }
    appendAllPathElements(&mut path, zPath);
    if path.rc != 0 || path.zOut.len() < 2 {
        return SQLITE_CANTOPEN;
    }
    if path.nSymlink != 0 {
        return SQLITE_OK_SYMLINK;
    }
    SQLITE_OK
}

/// Write nBuf bytes of random data to the supplied buffer zBuf.
fn unixRandomness(_pVfs: &Vfs, zBuf: &mut [u8]) -> i32 {
    let mut nBuf = zBuf.len();
    for b in zBuf.iter_mut() {
        *b = 0;
    }
    let pid = osGetpid();
    RANDOMNESS_PID.store(pid, Ordering::Relaxed);
    match robust_open(b"/dev/urandom", libc::O_RDONLY, 0) {
        Err(_) => {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let tb = t.to_ne_bytes();
            let pb = (pid as i32).to_ne_bytes();
            let n1 = tb.len().min(zBuf.len());
            zBuf[..n1].copy_from_slice(&tb[..n1]);
            let n2 = pb.len().min(zBuf.len() - n1);
            zBuf[n1..n1 + n2].copy_from_slice(&pb[..n2]);
            nBuf = tb.len() + pb.len();
        }
        Ok(mut f) => {
            use std::io::Read;
            loop {
                match f.read(zBuf) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    _ => break,
                }
            }
        }
    }
    nBuf as i32
}

/// Sleep for a little while.  Return the amount of time slept.  The
/// argument is the number of microseconds we want to sleep.
fn unixSleep(_pVfs: &Vfs, microseconds: i32) -> i32 {
    std::thread::sleep(std::time::Duration::from_micros(microseconds.max(0) as u64));
    microseconds
}

/// Find the current time (in Universal Coordinated Time).  Write into
/// *piNow the current time and date as a Julian Day number times 86_400_000.
fn unixCurrentTimeInt64(_pVfs: &Vfs, piNow: &mut i64) -> i32 {
    const unixEpoch: i64 = 24405875 * 8640000;
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    *piNow = unixEpoch + 1000 * (d.as_secs() as i64) + (d.subsec_micros() as i64) / 1000;
    SQLITE_OK
}

/// Find the current time (in Universal Coordinated Time) as a Julian day.
fn unixCurrentTime(pVfs: &Vfs, prNow: &mut f64) -> i32 {
    let mut i = 0i64;
    let rc = unixCurrentTimeInt64(pVfs, &mut i);
    *prNow = i as f64 / 86400000.0;
    rc
}

/// The xGetLastError() method is designed to return a better low-level
/// error message when operating-system problems come up during SQLite
/// operation.  Only the integer return code is currently used.
fn unixGetLastError(_pVfs: &Vfs) -> i32 {
    last_errno()
}

/// Build one of the unix VFS objects (C UNIXVFS macro).
fn unix_vfs(zName: &[u8], finder: usize) -> Vfs {
    Vfs {
        iVersion: 3,
        szOsFile: 120,
        mxPathname: MAX_PATHNAME,
        zName: zName.to_vec(),
        pAppData: finder,
        xOpen: unixOpen,
        xDelete: Some(unixDelete),
        xAccess: unixAccess,
        xFullPathname: unixFullPathname,
        xRandomness: unixRandomness,
        xSleep: unixSleep,
        xCurrentTime: Some(unixCurrentTime),
        xGetLastError: Some(unixGetLastError),
        xCurrentTimeInt64: Some(unixCurrentTimeInt64),
    }
}

thread_local! {
    /// The unix VFS objects (C aVfs[]), created once per thread.
    static UNIX_VFS: Vec<Rc<Vfs>> = vec![
        Rc::new(unix_vfs(b"unix", UNIX_FINDER_POSIX)),
        Rc::new(unix_vfs(b"unix-none", UNIX_FINDER_NOLOCK)),
        Rc::new(unix_vfs(b"unix-excl", UNIX_FINDER_POSIX)),
    ];
}

/// The default "unix" VFS object (without registering it).
pub fn sqlite3UnixVfs() -> Rc<Vfs> {
    UNIX_VFS.with(|v| v[0].clone())
}

/// C: `int sqlite3_os_init(void)`
///
/// Registers "unix" (the default), "unix-none" and "unix-excl".
pub fn sqlite3_os_init() -> i32 {
    UNIX_VFS.with(|v| {
        for (i, p) in v.iter().enumerate() {
            crate::os::vfs_register_noinit(p.clone(), (i == 0) as i32);
        }
    });
    SQLITE_OK
}

/// C: `int sqlite3_os_end(void)`
pub fn sqlite3_os_end() -> i32 {
    SQLITE_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::*;

    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goro-sqlite-osunix-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn fullpathname() {
        let vfs = sqlite3UnixVfs();
        let mut out = ZStr::new();
        assert_eq!(
            sqlite3OsFullPathname(&vfs, b"/a/b/../c/./d", 512, &mut out),
            SQLITE_OK
        );
        assert_eq!(out, b"/a/c/d");
        let rc = sqlite3OsFullPathname(&vfs, b"x.db", 512, &mut out);
        assert!(rc == SQLITE_OK || rc == SQLITE_OK_SYMLINK);
        assert!(out.ends_with(b"/x.db"));
    }

    #[test]
    fn open_read_write_lock() {
        let vfs = sqlite3UnixVfs();
        let d = tmpdir();
        let p = d.join("t1.db");
        let _ = std::fs::remove_file(&p);
        let z = p.as_os_str().as_bytes().to_vec();
        let mut f = SqliteFile::Closed;
        let mut fout = 0;
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB;
        assert_eq!(
            sqlite3OsOpen(&vfs, Some(&z), &mut f, flags, Some(&mut fout)),
            SQLITE_OK
        );
        assert_eq!(sqlite3OsWrite(&mut f, b"hello", 5, 10), SQLITE_OK);
        let mut sz = 0;
        assert_eq!(sqlite3OsFileSize(&mut f, &mut sz), SQLITE_OK);
        assert_eq!(sz, 15);
        let mut buf = [1u8; 20];
        assert_eq!(
            sqlite3OsRead(&mut f, &mut buf, 20, 0),
            SQLITE_IOERR_SHORT_READ
        );
        assert_eq!(&buf[10..15], b"hello");
        assert_eq!(&buf[15..], &[0u8; 5]);

        // Two handles on the same file share the inode lock state.
        let mut g = SqliteFile::Closed;
        assert_eq!(
            sqlite3OsOpen(&vfs, Some(&z), &mut g, flags, None),
            SQLITE_OK
        );
        assert_eq!(sqlite3OsLock(&mut f, SHARED_LOCK), SQLITE_OK);
        assert_eq!(sqlite3OsLock(&mut g, SHARED_LOCK), SQLITE_OK);
        assert_eq!(sqlite3OsLock(&mut f, RESERVED_LOCK), SQLITE_OK);
        let mut res = 0;
        assert_eq!(sqlite3OsCheckReservedLock(&mut g, &mut res), SQLITE_OK);
        assert_eq!(res, 1);
        assert_eq!(sqlite3OsLock(&mut g, RESERVED_LOCK), SQLITE_BUSY);
        assert_eq!(sqlite3OsLock(&mut f, EXCLUSIVE_LOCK), SQLITE_BUSY);
        assert_eq!(sqlite3OsUnlock(&mut g, NO_LOCK), SQLITE_OK);
        assert_eq!(sqlite3OsLock(&mut f, EXCLUSIVE_LOCK), SQLITE_OK);
        assert_eq!(sqlite3OsLock(&mut g, SHARED_LOCK), SQLITE_BUSY);
        // Closing g while f holds locks defers the close of g's fd.
        sqlite3OsClose(&mut g);
        assert_eq!(sqlite3OsUnlock(&mut f, SHARED_LOCK), SQLITE_OK);
        assert_eq!(sqlite3OsUnlock(&mut f, NO_LOCK), SQLITE_OK);
        sqlite3OsClose(&mut f);
        let mut ex = 0;
        assert_eq!(
            sqlite3OsAccess(&vfs, &z, SQLITE_ACCESS_EXISTS, &mut ex),
            SQLITE_OK
        );
        assert_eq!(ex, 1);
        assert_eq!(sqlite3OsDelete(&vfs, &z, 1), SQLITE_OK);
        assert_eq!(sqlite3OsDelete(&vfs, &z, 0), SQLITE_IOERR_DELETE_NOENT);
    }

    #[test]
    fn temp_file() {
        let vfs = sqlite3UnixVfs();
        let mut f = SqliteFile::Closed;
        let flags = SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE
            | SQLITE_OPEN_TEMP_DB;
        assert_eq!(sqlite3OsOpen(&vfs, None, &mut f, flags, None), SQLITE_OK);
        assert_eq!(sqlite3OsWrite(&mut f, &[7u8; 4096], 4096, 8192), SQLITE_OK);
        let mut sz = 0;
        sqlite3OsFileSize(&mut f, &mut sz);
        assert_eq!(sz, 12288);
        sqlite3OsClose(&mut f);
    }
}
