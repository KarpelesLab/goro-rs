//! Port of memjournal.c: an in-memory journal file.
//!
//! The journal content is kept in a growable buffer instead of the C
//! linked list of chunks; the observable behaviour (short reads past the
//! end, truncation on non-append writes, the "overwrite the header at
//! offset 0" special case and the spill to a real file once the journal
//! grows beyond nSpill bytes) is that of the C code.

use crate::int::*;
use crate::os::{SqliteFile, Vfs};
use std::rc::Rc;

/// Default chunk size (C MEMJOURNAL_DFLT_FILECHUNKSIZE)
const MEMJOURNAL_DFLT_FILECHUNKSIZE: i32 = 1024;
/// sizeof(FileChunk) on 64-bit (pNext + 8 bytes of data)
const SIZEOF_FILECHUNK: i32 = 16;
/// sizeof(MemJournal) in the 64-bit C build
const SIZEOF_MEMJOURNAL: i32 = 80;

/// An in-memory journal (C MemJournal).
pub struct MemJournal {
    /// In-memory chunk-size
    nChunkSize: i32,
    /// Bytes of data before flushing (<0: never, 0: not used)
    nSpill: i32,
    /// The journal content; its length is C endpoint.iOffset
    aData: Vec<u8>,
    /// xOpen flags
    flags: i32,
    /// The "real" underlying VFS
    pVfs: Option<Rc<Vfs>>,
    /// Name of the journal file
    zJournal: Option<ZStr>,
}

/// Read data from the in-memory journal file.  This is the implementation
/// of the sqlite3_vfs.xRead method.
pub(crate) fn memjrnlRead(p: &mut MemJournal, zBuf: &mut [u8], iOfst: i64) -> i32 {
    let iAmt = zBuf.len() as i64;
    if iAmt + iOfst > p.aData.len() as i64 {
        return SQLITE_IOERR_SHORT_READ;
    }
    let o = iOfst as usize;
    zBuf.copy_from_slice(&p.aData[o..o + zBuf.len()]);
    SQLITE_OK
}

/// Into a new file, write the journal content and switch the journal to
/// the real file (C memjrnlCreateFile()).
fn memjrnlCreateFile(pJfd: &mut SqliteFile) -> i32 {
    let copy = match std::mem::take(pJfd) {
        SqliteFile::Mem(m) => m,
        other => {
            *pJfd = other;
            return SQLITE_OK;
        }
    };
    let pVfs = match &copy.pVfs {
        Some(v) => v.clone(),
        None => {
            *pJfd = SqliteFile::Mem(copy);
            return SQLITE_CANTOPEN;
        }
    };
    let mut rc = crate::os::sqlite3OsOpen(&pVfs, copy.zJournal.as_deref(), pJfd, copy.flags, None);
    if rc == SQLITE_OK {
        let nChunk = copy.nChunkSize.max(1) as usize;
        let mut iOff = 0usize;
        while iOff < copy.aData.len() {
            let n = nChunk.min(copy.aData.len() - iOff);
            rc =
                crate::os::sqlite3OsWrite(pJfd, &copy.aData[iOff..iOff + n], n as i32, iOff as i64);
            if rc != 0 {
                break;
            }
            iOff += n;
        }
    }
    if rc != SQLITE_OK {
        // If an error occurred while creating or writing to the file,
        // restore the original before returning.  This way, SQLite uses
        // the in-memory journal data to roll back changes made to the
        // internal page-cache before this function was called.
        crate::os::sqlite3OsClose(pJfd);
        *pJfd = SqliteFile::Mem(copy);
    }
    rc
}

/// Write data to the file.
pub(crate) fn memjrnlWrite(pJfd: &mut SqliteFile, zBuf: &[u8], iOfst: i64) -> i32 {
    let iAmt = zBuf.len() as i64;
    let spill = match pJfd {
        SqliteFile::Mem(p) => p.nSpill > 0 && (iAmt + iOfst) > p.nSpill as i64,
        _ => false,
    };
    // If the file should be created now, create it and write the new data
    // into the file on disk.
    if spill {
        let mut rc = memjrnlCreateFile(pJfd);
        if rc == SQLITE_OK {
            rc = crate::os::sqlite3OsWrite(pJfd, zBuf, iAmt as i32, iOfst);
        }
        return rc;
    }
    let p = match pJfd {
        SqliteFile::Mem(p) => p,
        _ => return SQLITE_IOERR_WRITE,
    };
    // If the contents of this write should be stored in memory
    debug_assert!(iOfst <= p.aData.len() as i64);
    if iOfst > 0 && iOfst != p.aData.len() as i64 {
        memjrnlTruncate(p, iOfst);
    }
    if iOfst == 0 && !p.aData.is_empty() {
        // An in-memory journal file should only ever be appended to.
        // Random access writes are not required.  The only exception to
        // this is when the in-memory journal is being used by a connection
        // using SQLITE_USE_HEAP_MEMORY_WAL mode or when the header of the
        // journal is rewritten.  Bytes that would land past the current
        // end of file are not readable in C either.
        debug_assert!(p.nChunkSize as i64 > iAmt);
        let n = zBuf.len().min(p.aData.len());
        p.aData[..n].copy_from_slice(&zBuf[..n]);
    } else {
        p.aData.extend_from_slice(zBuf);
    }
    SQLITE_OK
}

/// Truncate the in-memory file.
pub(crate) fn memjrnlTruncate(p: &mut MemJournal, size: i64) -> i32 {
    if size < p.aData.len() as i64 {
        p.aData.truncate(size.max(0) as usize);
        if size == 0 {
            p.aData = Vec::new();
        }
    }
    SQLITE_OK
}

/// Close the file.
pub(crate) fn memjrnlClose(p: &mut MemJournal) -> i32 {
    p.aData = Vec::new();
    SQLITE_OK
}

/// Query the size of the file in bytes.
pub(crate) fn memjrnlFileSize(p: &mut MemJournal, pSize: &mut i64) -> i32 {
    *pSize = p.aData.len() as i64;
    SQLITE_OK
}

/// C: `int sqlite3JournalOpen( sqlite3_vfs *pVfs, const char *zName, sqlite3_file *pJfd, int flags, int nSpill )`
///
/// Open a journal file.
///
/// The behaviour of the journal file depends on the value of parameter
/// nSpill. If nSpill is 0, then the journal file is always created and
/// accessed using the underlying VFS. If nSpill is less than zero, then
/// all content is always stored in main-memory. Finally, if nSpill is a
/// positive value, then the journal file is initially created in-memory
/// but may be flushed to disk later on.  In this case the journal file
/// is flushed to disk either when it grows larger than nSpill bytes in
/// size, or when sqlite3JournalCreate() is called.
///
/// Signature change: `pVfs: Option<&Rc<Vfs>>` (the VFS is kept for a
/// later spill; C NULL from sqlite3MemJournalOpen()).
pub fn sqlite3JournalOpen(
    pVfs: Option<&Rc<crate::os::Vfs>>,
    zName: Option<&[u8]>,
    pJfd: &mut crate::os::SqliteFile,
    flags: i32,
    nSpill: i32,
) -> i32 {
    debug_assert!(zName.is_some() || nSpill < 0 || (flags & SQLITE_OPEN_EXCLUSIVE) != 0);
    *pJfd = SqliteFile::Closed;
    if nSpill == 0 {
        return crate::os::sqlite3OsOpen(pVfs.expect("journal VFS"), zName, pJfd, flags, None);
    }
    let nChunkSize = if nSpill > 0 {
        nSpill
    } else {
        8 + MEMJOURNAL_DFLT_FILECHUNKSIZE - SIZEOF_FILECHUNK
    };
    *pJfd = SqliteFile::Mem(MemJournal {
        nChunkSize,
        nSpill,
        aData: Vec::new(),
        flags,
        pVfs: pVfs.cloned(),
        zJournal: zName.map(|z| z.to_vec()),
    });
    SQLITE_OK
}

/// C: `void sqlite3MemJournalOpen(sqlite3_file *pJfd)`
///
/// Open an in-memory journal file.
pub fn sqlite3MemJournalOpen(pJfd: &mut crate::os::SqliteFile) {
    sqlite3JournalOpen(None, None, pJfd, 0, -1);
}

/// C: `int sqlite3JournalCreate(sqlite3_file *pJfd)`
///
/// Only compiled in C with SQLITE_ENABLE_ATOMIC_WRITE or
/// SQLITE_ENABLE_BATCH_ATOMIC_WRITE, neither of which is enabled in the
/// configuration we mirror: the journal is never forced to disk.
pub fn sqlite3JournalCreate(pJfd: &mut crate::os::SqliteFile) -> i32 {
    let _ = pJfd;
    SQLITE_OK
}

/// C: `int sqlite3JournalIsInMemory(sqlite3_file *p)`
///
/// The file-handle passed as the only argument is open on a journal file.
/// Return true if this "journal file" is currently stored in heap memory,
/// or false otherwise.  Signature change: takes `&SqliteFile`.
pub fn sqlite3JournalIsInMemory(p: &crate::os::SqliteFile) -> i32 {
    matches!(p, SqliteFile::Mem(_)) as i32
}

/// C: `int sqlite3JournalSize(sqlite3_vfs *pVfs)`
///
/// Return the number of bytes required to store a JournalFile that uses
/// vfs pVfs to create the underlying on-disk files.
pub fn sqlite3JournalSize(pVfs: &crate::os::Vfs) -> i32 {
    pVfs.szOsFile.max(SIZEOF_MEMJOURNAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::*;

    #[test]
    fn memjournal_basic_and_spill() {
        let mut f = SqliteFile::Closed;
        sqlite3MemJournalOpen(&mut f);
        assert_eq!(sqlite3JournalIsInMemory(&f), 1);
        assert_eq!(sqlite3OsWrite(&mut f, &[1u8; 100], 100, 0), SQLITE_OK);
        assert_eq!(sqlite3OsWrite(&mut f, &[2u8; 50], 50, 100), SQLITE_OK);
        let mut b = [0u8; 10];
        assert_eq!(sqlite3OsRead(&mut f, &mut b, 10, 95), SQLITE_OK);
        assert_eq!(b, [1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
        assert_eq!(
            sqlite3OsRead(&mut f, &mut b, 10, 145),
            SQLITE_IOERR_SHORT_READ
        );
        // non-append write truncates
        assert_eq!(sqlite3OsWrite(&mut f, &[3u8; 5], 5, 20), SQLITE_OK);
        let mut sz = 0;
        sqlite3OsFileSize(&mut f, &mut sz);
        assert_eq!(sz, 25);
        sqlite3OsClose(&mut f);

        let vfs = crate::os_unix::sqlite3UnixVfs();
        let flags = SQLITE_OPEN_SUBJOURNAL
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE;
        assert_eq!(
            sqlite3JournalOpen(Some(&vfs), None, &mut f, flags, 1000),
            SQLITE_OK
        );
        assert_eq!(sqlite3OsWrite(&mut f, &[4u8; 600], 600, 0), SQLITE_OK);
        assert_eq!(sqlite3JournalIsInMemory(&f), 1);
        assert_eq!(sqlite3OsWrite(&mut f, &[5u8; 600], 600, 600), SQLITE_OK);
        assert_eq!(sqlite3JournalIsInMemory(&f), 0);
        let mut b = [0u8; 4];
        assert_eq!(sqlite3OsRead(&mut f, &mut b, 4, 598), SQLITE_OK);
        assert_eq!(b, [4, 4, 5, 5]);
        sqlite3OsFileSize(&mut f, &mut sz);
        assert_eq!(sz, 1200);
        sqlite3OsClose(&mut f);
    }
}
