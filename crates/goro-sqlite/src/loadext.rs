//! Port of loadext.c.
//!
//! This file contains code used to dynamically load extensions into
//! the SQLite library.
//!
//! Rust: this port cannot load shared libraries (no FFI).  The behaviour
//! is that of a C build in which every dlopen() fails: the error
//! messages are the ones the C library produces in that case (the text
//! of dlerror() as reported by glibc for the last file that was tried).
//! Automatic extensions are Rust closures run on every new connection.

use crate::int::*;
use crate::util::sqlite3ErrorWithMsg;
use std::cell::RefCell;
use std::rc::Rc;

/// SQLITE_MAX_PATHLEN (FILENAME_MAX on glibc)
const SQLITE_MAX_PATHLEN: usize = 4096;

/// An automatic extension entry point: `xInit(db, &zErrMsg)` returns an
/// SQLite result code (C `sqlite3_loadext_entry` without the API thunk).
pub type XAutoExt = Rc<dyn Fn(&mut Sqlite3, &mut Option<ZStr>) -> i32>;

/// Rust: emulation of `sqlite3OsDlOpen()` + `sqlite3OsDlError()` for a
/// platform where no shared library can be loaded.  Returns the message
/// glibc's dlerror() gives after a failed dlopen(zFile).
fn dlopenError(zFile: &[u8]) -> ZStr {
    let path = String::from_utf8_lossy(zFile).into_owned();
    let mut z = zFile.to_vec();
    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => z.extend_from_slice(b": cannot read file data: Is a directory"),
        Ok(_) => z.extend_from_slice(b": invalid ELF header"),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            z.extend_from_slice(b": cannot open shared object file: Permission denied")
        }
        Err(_) => {
            z.extend_from_slice(b": cannot open shared object file: No such file or directory")
        }
    }
    z
}

/// Attempt to load an SQLite extension library contained in the file
/// zFile.  The entry point is zProc.  zProc may be 0 in which case a
/// default entry point name (sqlite3_extension_init) is used.  Use
/// of the default name is recommended.
///
/// Return SQLITE_OK on success and SQLITE_ERROR if something goes wrong.
///
/// If an error occurs and pzErrMsg is not 0, then fill *pzErrMsg with
/// error message text.
fn sqlite3LoadExtension(
    db: &mut Sqlite3,
    zFile: &[u8],
    zProc: Option<&[u8]>,
    pzErrMsg: &mut Option<ZStr>,
) -> i32 {
    let mut nMsg = zFile.len();

    // Shared library endings to try if zFile cannot be loaded as written
    const azEndings: [&[u8]; 1] = [b"so"];

    *pzErrMsg = None;

    // Ticket #1863.  To avoid a creating security problems for older
    // applications that relink against newer versions of SQLite, the
    // ability to run load_extension is turned off by default.  One
    // must call either sqlite3_enable_load_extension(db) or
    // sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, 0)
    // to turn on extension loading.
    if (db.flags & SQLITE_LoadExtension) == 0 {
        *pzErrMsg = Some(b"not authorized".to_vec());
        return SQLITE_ERROR;
    }

    let _zEntry: &[u8] = zProc.unwrap_or(b"sqlite3_extension_init");

    // tag-20210611-1.  Some dlopen() implementations will segfault if given
    // an oversize filename.  Most filesystems have a pathname limit of 4K,
    // so limit the extension filename length to about twice that.
    //
    // Do not allow sqlite3_load_extension() to link to a copy of the
    // running application, by passing in an empty filename.
    let mut zDlErr: Option<ZStr> = None;
    if nMsg <= SQLITE_MAX_PATHLEN && nMsg != 0 {
        // handle = sqlite3OsDlOpen(pVfs, zFile): always fails.
        zDlErr = Some(dlopenError(zFile));
        for e in azEndings.iter() {
            let mut zAltFile = zFile.to_vec();
            zAltFile.push(b'.');
            zAltFile.extend_from_slice(e);
            if nMsg + e.len() + 1 <= SQLITE_MAX_PATHLEN {
                zDlErr = Some(dlopenError(&zAltFile));
            }
        }
    }

    // extension_not_found:
    nMsg += 300;
    let mut zErrmsg = crate::printf::sqlite3_snprintf(
        nMsg as i32,
        b"unable to open shared library [%.*s]",
        &[
            crate::printf::PfArg::Int(SQLITE_MAX_PATHLEN as i64),
            crate::printf::PfArg::Str(Some(zFile)),
        ],
    );
    // sqlite3OsDlError(pVfs, nMsg-1, zErrmsg): overwrites the message with
    // the dlerror() text, if there is one.
    if let Some(e) = zDlErr {
        zErrmsg = e;
        zErrmsg.truncate(nMsg - 2);
    }
    *pzErrMsg = Some(zErrmsg);
    SQLITE_ERROR
}

/// C: `int sqlite3_load_extension( sqlite3 *db, const char *zFile, const char *zProc, char **pzErrMsg )`
pub fn sqlite3_load_extension(
    db: &mut Sqlite3,
    zFile: Option<&[u8]>,
    zProc: Option<&[u8]>,
    pzErrMsg: &mut Option<ZStr>,
) -> i32 {
    let rc = sqlite3LoadExtension(db, zFile.unwrap_or(b""), zProc, pzErrMsg);
    crate::mainapi::sqlite3ApiExit(db, rc)
}

/// C: `void sqlite3CloseExtensions(sqlite3 *db)`
///
/// Call this routine when the database connection is closing in order
/// to clean up loaded extensions.  Rust: no extension is ever loaded.
pub fn sqlite3CloseExtensions(db: &mut Sqlite3) {
    db.nExtension = 0;
}

/// C: `int sqlite3_enable_load_extension(sqlite3 *db, int onoff)`
///
/// Enable or disable extension loading.  Extension loading is disabled by
/// default so as not to open security holes in older applications.
pub fn sqlite3_enable_load_extension(db: &mut Sqlite3, onoff: i32) -> i32 {
    if onoff != 0 {
        db.flags |= SQLITE_LoadExtension | SQLITE_LoadExtFunc;
    } else {
        db.flags &= !(SQLITE_LoadExtension | SQLITE_LoadExtFunc);
    }
    SQLITE_OK
}

thread_local! {
    /// The list of automatically loaded extensions (C sqlite3Autoext).
    /// Rust: per thread, since the closures are not Send.
    static sqlite3Autoext: RefCell<Vec<XAutoExt>> = RefCell::new(Vec::new());
}

/// C: `int sqlite3_auto_extension( void (*xInit)(void) )`
///
/// Register a statically linked extension that is automatically
/// loaded by every new database connection.
pub fn sqlite3_auto_extension(xInit: XAutoExt) -> i32 {
    let rc = crate::mainapi::sqlite3_initialize();
    if rc != 0 {
        return rc;
    }
    sqlite3Autoext.with(|a| {
        let mut a = a.borrow_mut();
        if !a.iter().any(|x| Rc::ptr_eq(x, &xInit)) {
            a.push(xInit);
        }
    });
    SQLITE_OK
}

/// C: `int sqlite3_cancel_auto_extension( void (*xInit)(void) )`
///
/// Cancel a prior call to sqlite3_auto_extension.  Remove xInit from the
/// set of routines that is invoked for each new database connection, if it
/// is currently on the list.  If xInit is not on the list, then this
/// routine is a no-op.
///
/// Return 1 if xInit was found on the list and removed.  Return 0 if xInit
/// was not on the list.
pub fn sqlite3_cancel_auto_extension(xInit: &XAutoExt) -> i32 {
    sqlite3Autoext.with(|a| {
        let mut a = a.borrow_mut();
        for i in (0..a.len()).rev() {
            if Rc::ptr_eq(&a[i], xInit) {
                // C: aExt[i] = aExt[--nExt]
                a.swap_remove(i);
                return 1;
            }
        }
        0
    })
}

/// C: `void sqlite3_reset_auto_extension(void)`
///
/// Reset the automatic extension loading mechanism.
pub fn sqlite3_reset_auto_extension() {
    if crate::mainapi::sqlite3_initialize() == SQLITE_OK {
        sqlite3Autoext.with(|a| a.borrow_mut().clear());
    }
}

/// C: `void sqlite3AutoLoadExtensions(sqlite3 *db)`
///
/// Load all automatic extensions.
///
/// If anything goes wrong, set an error in the database connection.
pub fn sqlite3AutoLoadExtensions(db: &mut Sqlite3) {
    let mut i = 0usize;
    loop {
        let xInit = sqlite3Autoext.with(|a| a.borrow().get(i).cloned());
        let Some(xInit) = xInit else { break };
        let mut zErrmsg: Option<ZStr> = None;
        let rc = xInit(db, &mut zErrmsg);
        if rc != 0 {
            sqlite3ErrorWithMsg(
                db,
                rc,
                Some(b"automatic extension loading failed: %s"),
                &[crate::printf::PfArg::Str(zErrmsg.as_deref())],
            );
            break;
        }
        i += 1;
    }
}
