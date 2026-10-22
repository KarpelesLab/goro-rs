//! Port of main.c.
//!
//! Main file for the SQLite library.  The routines in this file
//! implement the programmer interface to the library.  Routines in
//! other files are for internal use by SQLite and should not be
//! accessed by users of the library.
//!
//! Rust notes:
//! * a connection is a `Box<Sqlite3>` returned by sqlite3_open_v2();
//!   sqlite3_close() releases its resources (the caller drops the box
//!   afterwards).  With sqlite3_close_v2() and outstanding statements the
//!   connection becomes a zombie and is torn down when the last statement
//!   is finalized (sqlite3LeaveMutexAndCloseZombie()).
//! * application callbacks are closures (`Rc<dyn Fn ...>`); the C
//!   `void *pArg` is whatever the closure captures, and the functions that
//!   return the previous pArg return the previous closure instead.
//! * filenames produced by sqlite3ParseUri()/sqlite3_create_filename() are
//!   byte buffers that start at the database name (C: the pointer just
//!   after the 4 leading zero bytes): `name \0 (key \0 value \0)* \0
//!   journal \0 wal \0 \0`.  The sqlite3_uri_*() and sqlite3_filename_*()
//!   functions take such a buffer (Rust cannot walk back from a pointer
//!   into the middle of a buffer, so `databaseName()` is the identity).

use crate::btree::*;
use crate::build::{sqlite3CollapseDatabaseArray, sqlite3FindDbName, sqlite3FindTable};
use crate::callback::{
    FuncDefHash, sqlite3FindCollSeq, sqlite3FindFunction, sqlite3FuncDefReplace,
};
use crate::global::{sqlite3GlobalConfig, sqlite3StdType, sqlite3StrBINARY};
use crate::int::*;
use crate::os::Vfs;
use crate::printf::PfArg;
use crate::util::{sqlite3Error, sqlite3ErrorWithMsg};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// If the following global variable points to a string which is the
/// name of a directory, then that directory will be used to store
/// temporary files.
///
/// See also the "PRAGMA temp_store_directory" SQL command.
pub static sqlite3_temp_directory: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// If the following global variable points to a string which is the
/// name of a directory, then that directory will be used to store
/// all database files specified with a relative pathname.
///
/// See also the "PRAGMA data_store_directory" SQL command.
pub static sqlite3_data_directory: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// mem0.alarmThreshold (soft heap limit) and mem0.hardLimit of malloc.c.
/// Rust: memory is not accounted, the limits are only recorded.
static mem0_alarmThreshold: AtomicI64 = AtomicI64::new(0);
static mem0_hardLimit: AtomicI64 = AtomicI64::new(0);

thread_local! {
    /// sqlite3GlobalConfig.isInit
    static isInit: Cell<bool> = const { Cell::new(false) };
    /// The built-in function table (C sqlite3BuiltinFunctions), built by
    /// sqlite3_initialize().
    static sqlite3BuiltinFunctionsTab: RefCell<Option<Rc<FuncDefHash>>> = const { RefCell::new(None) };
    /// The canonical BINARY comparison closure, so that sqlite3IsBinary()
    /// can recognise it (C compares the xCmp function pointer).
    static binaryCmp: XCmp = Rc::new(|a: &[u8], b: &[u8]| sqlite3BinaryCompare(a, b));
}

/// The built-in SQL functions (C global sqlite3BuiltinFunctions).
/// Initializes the library if needed.
pub fn sqlite3BuiltinFunctions() -> Rc<FuncDefHash> {
    sqlite3_initialize();
    sqlite3BuiltinFunctionsTab.with(|t| match &*t.borrow() {
        Some(h) => h.clone(),
        None => Rc::new(FuncDefHash::new()),
    })
}

/// The compile-time options of this build (C sqlite3azCompileOpt).
const azCompileOpt: &[&[u8]] = &[
    b"ATOMIC_INTRINSICS=1",
    b"COMPILER=gcc-15.3.0",
    b"DEFAULT_AUTOVACUUM",
    b"DEFAULT_CACHE_SIZE=-2000",
    b"DEFAULT_FILE_FORMAT=4",
    b"DEFAULT_JOURNAL_SIZE_LIMIT=-1",
    b"DEFAULT_MMAP_SIZE=0",
    b"DEFAULT_PAGE_SIZE=4096",
    b"DEFAULT_PCACHE_INITSZ=20",
    b"DEFAULT_RECURSIVE_TRIGGERS",
    b"DEFAULT_SECTOR_SIZE=4096",
    b"DEFAULT_SYNCHRONOUS=2",
    b"DEFAULT_WAL_AUTOCHECKPOINT=1000",
    b"DEFAULT_WAL_SYNCHRONOUS=2",
    b"DEFAULT_WORKER_THREADS=0",
    b"DIRECT_OVERFLOW_READ",
    b"ENABLE_COLUMN_METADATA",
    b"ENABLE_MATH_FUNCTIONS",
    b"MALLOC_SOFT_LIMIT=1024",
    b"MAX_ATTACHED=10",
    b"MAX_COLUMN=2000",
    b"MAX_COMPOUND_SELECT=500",
    b"MAX_DEFAULT_PAGE_SIZE=8192",
    b"MAX_EXPR_DEPTH=1000",
    b"MAX_FUNCTION_ARG=1000",
    b"MAX_LENGTH=1000000000",
    b"MAX_LIKE_PATTERN_LENGTH=50000",
    b"MAX_MMAP_SIZE=0x7fff0000",
    b"MAX_PAGE_COUNT=0xfffffffe",
    b"MAX_PAGE_SIZE=65536",
    b"MAX_SCHEMA=10000000",
    b"MAX_SQL_LENGTH=1000000000",
    b"MAX_TRIGGER_DEPTH=1000",
    b"MAX_VARIABLE_NUMBER=32766",
    b"MAX_VDBE_OP=250000000",
    b"MAX_WORKER_THREADS=8",
    b"MUTEX_PTHREADS",
    b"SYSTEM_MALLOC",
    b"TEMP_STORE=1",
    b"THREADSAFE=1",
];

/// C: `const char **sqlite3CompileOptions(int *pnOpt)`
pub fn sqlite3CompileOptions() -> &'static [&'static [u8]] {
    azCompileOpt
}

/// Length of a C string held in a byte slice (up to the first NUL).
#[inline]
fn cstrlen(z: &[u8]) -> usize {
    z.iter().position(|&c| c == 0).unwrap_or(z.len())
}

/// The C string starting at offset `off` of `z` (without its NUL).
#[inline]
fn cstr_at(z: &[u8], off: usize) -> &[u8] {
    if off >= z.len() {
        return b"";
    }
    let s = &z[off..];
    &s[..cstrlen(s)]
}

// ---------------------------------------------------------------------------
// Version, initialization, configuration
// ---------------------------------------------------------------------------

/// C: `const char * sqlite3_libversion(void)`
///
/// IMPLEMENTATION-OF: R-53536-42575 The sqlite3_libversion() function
/// returns a pointer to the to the sqlite3_version[] string constant.
pub fn sqlite3_libversion() -> Option<ZStr> {
    Some(crate::SQLITE_VERSION.as_bytes().to_vec())
}

/// C: `const char * sqlite3_sourceid(void)`
///
/// IMPLEMENTATION-OF: R-25063-23286 The sqlite3_sourceid() function
/// returns a pointer to a string constant whose value is the same as the
/// SQLITE_SOURCE_ID C preprocessor macro.
pub fn sqlite3_sourceid() -> Option<ZStr> {
    Some(crate::SQLITE_SOURCE_ID.as_bytes().to_vec())
}

/// C: `int sqlite3_libversion_number(void)`
///
/// IMPLEMENTATION-OF: R-35210-63508 The sqlite3_libversion_number()
/// function returns an integer equal to SQLITE_VERSION_NUMBER.
pub fn sqlite3_libversion_number() -> i32 {
    3054001
}

/// C: `int sqlite3_threadsafe(void)`
///
/// IMPLEMENTATION-OF: R-20790-14025 The sqlite3_threadsafe() function
/// returns zero if and only if SQLite was compiled with mutexing code
/// omitted due to the SQLITE_THREADSAFE compile-time option being set to
/// 0.
pub fn sqlite3_threadsafe() -> i32 {
    1
}

/// C: `SQLITE_API int sqlite3_initialize(void)`
///
/// Initialize SQLite.
///
/// This routine is a no-op except on its very first call for the process
/// (Rust: for the thread), or for the first call after a call to
/// sqlite3_shutdown.
pub fn sqlite3_initialize() -> i32 {
    if isInit.with(|i| i.get()) {
        // SQLite has already been initialized.  Fast early-out.
        return SQLITE_OK;
    }
    // sqlite3Initialize()
    isInit.with(|i| i.set(true));
    let mut h = FuncDefHash::new();
    crate::func::sqlite3RegisterBuiltinFunctions(&mut h);
    sqlite3BuiltinFunctionsTab.with(|t| *t.borrow_mut() = Some(Rc::new(h)));
    let rc = crate::os::sqlite3OsInit();
    if rc != SQLITE_OK {
        isInit.with(|i| i.set(false));
    }
    rc
}

/// C: `int sqlite3_shutdown(void)`
///
/// Undo the effects of sqlite3_initialize().  Must not be called while
/// there are outstanding database connections.
pub fn sqlite3_shutdown() -> i32 {
    if isInit.with(|i| i.get()) {
        crate::loadext::sqlite3_reset_auto_extension();
        isInit.with(|i| i.set(false));
        sqlite3BuiltinFunctionsTab.with(|t| *t.borrow_mut() = None);
        *sqlite3_data_directory.lock().unwrap() = None;
        *sqlite3_temp_directory.lock().unwrap() = None;
    }
    SQLITE_OK
}

/// C: `int sqlite3_config(int op, ...)`
///
/// This API allows applications to modify the global configuration of
/// the SQLite library at run-time.
///
/// Rust: the global configuration is fixed at compile time; the call is
/// validated like in C (SQLITE_MISUSE once initialized, except for the
/// "anytime" options) and otherwise ignored.
pub fn sqlite3_config(op: i32, args: &[crate::printf::PfArg]) -> i32 {
    let _ = args;
    // sqlite3_config() normally returns SQLITE_MISUSE if it is invoked while
    // the SQLite library is in use.  Except, a few selected opcodes
    // are allowed.
    if isInit.with(|i| i.get()) {
        let mAnytimeConfigOption: u64 =
            (1u64 << SQLITE_CONFIG_LOG) | (1u64 << SQLITE_CONFIG_PCACHE_HDRSZ);
        if op < 0 || op > 63 || ((1u64 << op) & mAnytimeConfigOption) == 0 {
            return SQLITE_MISUSE;
        }
    }
    match op {
        SQLITE_CONFIG_SINGLETHREAD
        | SQLITE_CONFIG_MULTITHREAD
        | SQLITE_CONFIG_SERIALIZED
        | SQLITE_CONFIG_MALLOC
        | SQLITE_CONFIG_GETMALLOC
        | SQLITE_CONFIG_MUTEX
        | SQLITE_CONFIG_GETMUTEX
        | SQLITE_CONFIG_MEMSTATUS
        | SQLITE_CONFIG_SMALL_MALLOC
        | SQLITE_CONFIG_PAGECACHE
        | SQLITE_CONFIG_PCACHE
        | SQLITE_CONFIG_GETPCACHE
        | SQLITE_CONFIG_PCACHE2
        | SQLITE_CONFIG_GETPCACHE2
        | SQLITE_CONFIG_HEAP
        | SQLITE_CONFIG_LOOKASIDE
        | SQLITE_CONFIG_LOG
        | SQLITE_CONFIG_URI
        | SQLITE_CONFIG_COVERING_INDEX_SCAN
        | SQLITE_CONFIG_MMAP_SIZE
        | SQLITE_CONFIG_PCACHE_HDRSZ
        | SQLITE_CONFIG_PMASZ
        | SQLITE_CONFIG_STMTJRNL_SPILL
        | SQLITE_CONFIG_SORTERREF_SIZE
        | SQLITE_CONFIG_MEMDB_MAXSIZE
        | SQLITE_CONFIG_ROWID_IN_VIEW => SQLITE_OK,
        _ => SQLITE_ERROR,
    }
}

/// C: `sqlite3_mutex * sqlite3_db_mutex(sqlite3 *db)`
///
/// Return the mutex associated with a database connection.  Rust: there
/// are no mutexes (a connection is owned by one thread).
pub fn sqlite3_db_mutex(db: &mut Sqlite3) -> Option<Box<()>> {
    let _ = db;
    None
}

/// C: `int sqlite3_db_release_memory(sqlite3 *db)`
///
/// Free up as much memory as we can from the given database
/// connection.
pub fn sqlite3_db_release_memory(db: &mut Sqlite3) -> i32 {
    for i in 0..db.aDb.len() {
        if let Some(pBt) = db.aDb[i].pBt.clone() {
            let pPager = sqlite3BtreePager(&pBt);
            crate::pager::sqlite3PagerShrink(&mut pPager.borrow_mut());
        }
    }
    SQLITE_OK
}

/// C: `int sqlite3_db_cacheflush(sqlite3 *db)`
///
/// Flush any dirty pages in the pager-cache for any attached database
/// to disk.
pub fn sqlite3_db_cacheflush(db: &mut Sqlite3) -> i32 {
    let mut rc = SQLITE_OK;
    let mut bSeenBusy = false;
    let mut i = 0;
    while rc == SQLITE_OK && i < db.aDb.len() {
        if let Some(pBt) = db.aDb[i].pBt.clone() {
            if sqlite3BtreeTxnState(db, &pBt) == SQLITE_TXN_WRITE {
                let pPager = sqlite3BtreePager(&pBt);
                rc = crate::pager::sqlite3PagerFlush(&mut pPager.borrow_mut());
                if rc == SQLITE_BUSY {
                    bSeenBusy = true;
                    rc = SQLITE_OK;
                }
            }
        }
        i += 1;
    }
    if rc == SQLITE_OK && bSeenBusy {
        SQLITE_BUSY
    } else {
        rc
    }
}

/// Set up the lookaside buffers for a database connection.
/// Rust: there is no lookaside allocator; like C, refuse with SQLITE_BUSY
/// while lookaside memory would be in use (never), else succeed.
fn setupLookaside(db: &mut Sqlite3, sz: i32, cnt: i32) -> i32 {
    let _ = (sz, cnt);
    let mut h = 0;
    if crate::status::sqlite3LookasideUsed(db, &mut h) > 0 {
        return SQLITE_BUSY;
    }
    SQLITE_OK
}

/// C: `int sqlite3_db_config(sqlite3 *db, int op, ...)`
///
/// Configuration settings for an individual database connection.
///
/// Rust: the varargs are (iArg, pRes): for the boolean options iArg is
/// the on/off value and *pRes receives the new setting; for
/// SQLITE_DBCONFIG_FP_DIGITS iArg is the new value and *pRes the current
/// one; for SQLITE_DBCONFIG_LOOKASIDE the arguments are ignored.  Use
/// sqlite3_db_config_maindbname() for SQLITE_DBCONFIG_MAINDBNAME.
pub fn sqlite3_db_config(db: &mut Sqlite3, op: i32, iArg: i32, pRes: Option<&mut i32>) -> i32 {
    let rc;
    match op {
        SQLITE_DBCONFIG_MAINDBNAME => {
            // Rust: needs a string argument, see sqlite3_db_config_maindbname().
            rc = SQLITE_MISUSE;
        }
        SQLITE_DBCONFIG_LOOKASIDE => {
            rc = setupLookaside(db, 0, 0);
        }
        SQLITE_DBCONFIG_FP_DIGITS => {
            let nIn = iArg;
            if nIn > 3 && nIn < 24 {
                db.nFpDigit = nIn as u8;
            }
            if let Some(pOut) = pRes {
                *pOut = db.nFpDigit as i32;
            }
            rc = SQLITE_OK;
        }
        _ => {
            const aFlagOp: [(i32, u64); 21] = [
                (SQLITE_DBCONFIG_ENABLE_FKEY, SQLITE_ForeignKeys),
                (SQLITE_DBCONFIG_ENABLE_TRIGGER, SQLITE_EnableTrigger),
                (SQLITE_DBCONFIG_ENABLE_VIEW, SQLITE_EnableView),
                (SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, SQLITE_Fts3Tokenizer),
                (SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, SQLITE_LoadExtension),
                (SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, SQLITE_NoCkptOnClose),
                (SQLITE_DBCONFIG_ENABLE_QPSG, SQLITE_EnableQPSG),
                (SQLITE_DBCONFIG_TRIGGER_EQP, SQLITE_TriggerEQP),
                (SQLITE_DBCONFIG_RESET_DATABASE, SQLITE_ResetDatabase),
                (SQLITE_DBCONFIG_DEFENSIVE, SQLITE_Defensive),
                (
                    SQLITE_DBCONFIG_WRITABLE_SCHEMA,
                    SQLITE_WriteSchema | SQLITE_NoSchemaError,
                ),
                (SQLITE_DBCONFIG_LEGACY_ALTER_TABLE, SQLITE_LegacyAlter),
                (SQLITE_DBCONFIG_DQS_DDL, SQLITE_DqsDDL),
                (SQLITE_DBCONFIG_DQS_DML, SQLITE_DqsDML),
                (SQLITE_DBCONFIG_LEGACY_FILE_FORMAT, SQLITE_LegacyFileFmt),
                (SQLITE_DBCONFIG_TRUSTED_SCHEMA, SQLITE_TrustedSchema),
                (SQLITE_DBCONFIG_STMT_SCANSTATUS, SQLITE_StmtScanStatus),
                (SQLITE_DBCONFIG_REVERSE_SCANORDER, SQLITE_ReverseOrder),
                (SQLITE_DBCONFIG_ENABLE_ATTACH_CREATE, SQLITE_AttachCreate),
                (SQLITE_DBCONFIG_ENABLE_ATTACH_WRITE, SQLITE_AttachWrite),
                (SQLITE_DBCONFIG_ENABLE_COMMENTS, SQLITE_Comments),
            ];
            let mut r = SQLITE_ERROR; // IMP: R-42790-23372
            for &(fop, mask) in aFlagOp.iter() {
                if fop == op {
                    let onoff = iArg;
                    let oldFlags = db.flags;
                    if onoff > 0 {
                        db.flags |= mask;
                    } else if onoff == 0 {
                        db.flags &= !mask;
                    }
                    if oldFlags != db.flags {
                        crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 0);
                    }
                    if let Some(pRes) = pRes {
                        *pRes = ((db.flags & mask) != 0) as i32;
                    }
                    r = SQLITE_OK;
                    break;
                }
            }
            rc = r;
        }
    }
    rc
}

/// sqlite3_db_config(db, SQLITE_DBCONFIG_MAINDBNAME, zName)
pub fn sqlite3_db_config_maindbname(db: &mut Sqlite3, zName: &[u8]) -> i32 {
    // IMP: R-06824-28531
    // IMP: R-36257-52125
    db.aDb[0].zDbSName = Some(zName.to_vec());
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Built-in collating sequences
// ---------------------------------------------------------------------------

/// C: `int sqlite3BinaryCompare( void *NotUsed, int nKey1, const void *pKey1, int nKey2, const void *pKey2 )`
///
/// This is the default collating function named "BINARY" which is always
/// available.
pub fn sqlite3BinaryCompare(pKey1: &[u8], pKey2: &[u8]) -> i32 {
    let n = pKey1.len().min(pKey2.len());
    // EVIDENCE-OF: R-65033-28449 The built-in BINARY collation compares
    // strings byte by byte using the memcmp() function from the standard C
    // library.
    for i in 0..n {
        if pKey1[i] != pKey2[i] {
            return pKey1[i] as i32 - pKey2[i] as i32;
        }
    }
    pKey1.len() as i32 - pKey2.len() as i32
}

/// This is the collating function named "RTRIM" which is always
/// available.  Ignore trailing spaces.
fn rtrimCollFunc(pKey1: &[u8], pKey2: &[u8]) -> i32 {
    let mut nKey1 = pKey1.len();
    let mut nKey2 = pKey2.len();
    while nKey1 > 0 && pKey1[nKey1 - 1] == b' ' {
        nKey1 -= 1;
    }
    while nKey2 > 0 && pKey2[nKey2 - 1] == b' ' {
        nKey2 -= 1;
    }
    sqlite3BinaryCompare(&pKey1[..nKey1], &pKey2[..nKey2])
}

/// C: `int sqlite3IsBinary(const CollSeq *p)`
///
/// Return true if CollSeq is the default built-in BINARY.
/// Rust: takes an Option (C NULL counts as BINARY).
pub fn sqlite3IsBinary(p: Option<&CollSeqRef>) -> i32 {
    match p {
        None => 1,
        Some(p) => match &*p.xCmp.borrow() {
            Some(x) => binaryCmp.with(|b| Rc::ptr_eq(x, b)) as i32,
            None => 0,
        },
    }
}

/// Another built-in collating sequence: NOCASE.
///
/// This collating sequence is intended to be used for "case independent
/// comparison". SQLite's knowledge of upper and lower case equivalents
/// extends only to the 26 characters used in the English language.
///
/// At the moment there is only a UTF-8 implementation.
fn nocaseCollatingFunc(pKey1: &[u8], pKey2: &[u8]) -> i32 {
    let nKey1 = pKey1.len() as i32;
    let nKey2 = pKey2.len() as i32;
    let mut r = crate::util::sqlite3_strnicmp(Some(pKey1), Some(pKey2), nKey1.min(nKey2));
    if 0 == r {
        r = nKey1 - nKey2;
    }
    r
}

// ---------------------------------------------------------------------------
// Simple accessors
// ---------------------------------------------------------------------------

/// C: `sqlite_int64 sqlite3_last_insert_rowid(sqlite3 *db)`
///
/// Return the ROWID of the most recent insert
pub fn sqlite3_last_insert_rowid(db: &mut Sqlite3) -> i64 {
    db.lastRowid
}

/// C: `void sqlite3_set_last_insert_rowid(sqlite3 *db, sqlite3_int64 iRowid)`
///
/// Set the value returned by the sqlite3_last_insert_rowid() API function.
pub fn sqlite3_set_last_insert_rowid(db: &mut Sqlite3, iRowid: i64) {
    db.lastRowid = iRowid;
}

/// C: `sqlite3_int64 sqlite3_changes64(sqlite3 *db)`
///
/// Return the number of changes in the most recently executed DML
/// statement.
pub fn sqlite3_changes64(db: &mut Sqlite3) -> i64 {
    db.nChange
}

/// C: `int sqlite3_changes(sqlite3 *db)`
pub fn sqlite3_changes(db: &mut Sqlite3) -> i32 {
    sqlite3_changes64(db) as i32
}

/// C: `sqlite3_int64 sqlite3_total_changes64(sqlite3 *db)`
///
/// Return the number of changes since the database handle was opened.
pub fn sqlite3_total_changes64(db: &mut Sqlite3) -> i64 {
    db.nTotalChange
}

/// C: `int sqlite3_total_changes(sqlite3 *db)`
pub fn sqlite3_total_changes(db: &mut Sqlite3) -> i32 {
    sqlite3_total_changes64(db) as i32
}

/// C: `void sqlite3CloseSavepoints(sqlite3 *db)`
///
/// Close all open savepoints. This function only manipulates fields of
/// the database handle object, it does not close any savepoints that may
/// be open at the b-tree/pager level.
pub fn sqlite3CloseSavepoints(db: &mut Sqlite3) {
    db.pSavepoint.clear();
    db.nSavepoint = 0;
    db.nStatement = 0;
    db.isTransactionSavepoint = 0;
}

/// Number of FuncDefs of db.aFunc that use the destructor `d` (C
/// FuncDestructor.nRef).
fn destructorRefs(db: &Sqlite3, d: &Rc<FuncDestructor>) -> usize {
    let mut n = 0;
    for (_, chain) in db.aFunc.iter() {
        for p in chain.iter() {
            if let Some(pd) = &p.pDestructor {
                if Rc::ptr_eq(pd, d) {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Invoke the destructor function associated with FuncDef p, if any.
/// Except, if this is not the last copy of the function, do not invoke
/// it.  Multiple copies of a single function are created when
/// create_function() is called with SQLITE_ANY as the encoding.
///
/// Rust: p must still be in db.aFunc (C decrements nRef first).  It can be
/// a copy of a built-in: where C modifies a built-in FuncDef in place
/// (sqlite3RegisterLikeFunctions() when sqlite3CreateFunc() failed), the
/// port installs a modified copy in db.aFunc; built-ins have no destructor.
fn functionDestroy(db: &mut Sqlite3, p: &FuncDef) {
    debug_assert!((p.funcFlags & SQLITE_FUNC_BUILTIN) == 0 || p.pDestructor.is_none());
    if let Some(pDestructor) = &p.pDestructor {
        if destructorRefs(db, pDestructor) <= 1 {
            if let Some(x) = &pDestructor.xDestroy {
                x();
            }
        }
    }
}

/// Disconnect all sqlite3_vtab objects that belong to database connection
/// db. This is called when db is being closed.
fn disconnectAllVtab(db: &mut Sqlite3) {
    for i in 0..db.aDb.len() {
        let tabs = db.aDb[i].pSchema.borrow().tblHash.values();
        for pTab in tabs.iter() {
            let isVirtual = pTab.borrow().IsVirtual();
            if isVirtual {
                crate::vtab::sqlite3VtabDisconnect(db, pTab);
            }
        }
    }
    let mods: Vec<ModuleRef> = db.aModule.iter().map(|(_, m)| m.clone()).collect();
    for pMod in mods.iter() {
        let pEpoTab = pMod.borrow().pEpoTab.clone();
        if let Some(pEpoTab) = pEpoTab {
            crate::vtab::sqlite3VtabDisconnect(db, &pEpoTab);
        }
    }
    crate::vtab::sqlite3VtabUnlockList(db);
}

/// Return TRUE if database connection db has unfinalized prepared
/// statements or unfinished sqlite3_backup objects.
fn connectionIsBusy(db: &mut Sqlite3) -> bool {
    if db.stmt_count() > 0 {
        return true;
    }
    for j in 0..db.aDb.len() {
        if let Some(pBt) = db.aDb[j].pBt.clone() {
            if sqlite3BtreeIsInBackup(db, &pBt) != 0 {
                return true;
            }
        }
    }
    false
}

/// Close an existing SQLite database
fn sqlite3Close(db: &mut Sqlite3, forceZombie: bool) -> i32 {
    if crate::util::sqlite3SafetyCheckSickOrOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    if (db.mTrace as u32 & SQLITE_TRACE_CLOSE) != 0 {
        if let Some(x) = db.xTrace.clone() {
            x(SQLITE_TRACE_CLOSE, b"");
        }
    }

    // Force xDisconnect calls on all virtual tables
    disconnectAllVtab(db);

    // If a transaction is open, the disconnectAllVtab() call above
    // will not have called the xDisconnect() method on any virtual
    // tables in the db->aVTrans[] array. The following sqlite3VtabRollback()
    // call will do so. We need to do this before the check for active
    // SQL statements below, as the v-table implementation may be storing
    // some prepared statements internally.
    crate::vtab::sqlite3VtabRollback(db);

    // Legacy behavior (sqlite3_close() behavior) is to return
    // SQLITE_BUSY if the connection can not be closed immediately.
    if !forceZombie && connectionIsBusy(db) {
        sqlite3ErrorWithMsg(
            db,
            SQLITE_BUSY,
            Some(b"unable to close due to unfinalized statements or unfinished backups"),
            &[],
        );
        return SQLITE_BUSY;
    }

    // Client data destructors (Rust: Drop of the client data).
    db.pDbData.clear();

    // Convert the connection into a zombie and then close it.
    db.eOpenState = SQLITE_STATE_ZOMBIE;
    sqlite3LeaveMutexAndCloseZombie(db);
    SQLITE_OK
}

/// C: `int sqlite3_txn_state(sqlite3 *db, const char *zSchema)`
///
/// Return the transaction state for a single databse, or the maximum
/// transaction state over all attached databases if zSchema is null.
pub fn sqlite3_txn_state(db: &mut Sqlite3, zSchema: Option<&[u8]>) -> i32 {
    let mut iDb;
    let mut nDb;
    let mut iTxn = -1;
    if let Some(z) = zSchema {
        iDb = sqlite3FindDbName(db, Some(z));
        nDb = iDb;
        if iDb < 0 {
            nDb -= 1;
        }
    } else {
        iDb = 0;
        nDb = db.nDb() - 1;
    }
    while iDb <= nDb {
        let x = match db.aDb[iDb as usize].pBt.clone() {
            Some(pBt) => sqlite3BtreeTxnState(db, &pBt),
            None => SQLITE_TXN_NONE,
        };
        if x > iTxn {
            iTxn = x;
        }
        iDb += 1;
    }
    iTxn
}

/// C: `int sqlite3_close(sqlite3 *db)`
///
/// Two variations on the public interface for closing a database
/// connection. The sqlite3_close() version returns SQLITE_BUSY and
/// leaves the connection open if there are unfinalized prepared
/// statements or unfinished sqlite3_backups.  The sqlite3_close_v2()
/// version forces the connection to become a zombie if there are
/// unclosed resources, and arranges for deallocation when the last
/// prepare statement or sqlite3_backup closes.
pub fn sqlite3_close(db: &mut Sqlite3) -> i32 {
    sqlite3Close(db, false)
}

/// C: `int sqlite3_close_v2(sqlite3 *db)`
pub fn sqlite3_close_v2(db: &mut Sqlite3) -> i32 {
    sqlite3Close(db, true)
}

/// C: `void sqlite3LeaveMutexAndCloseZombie(sqlite3 *db)`
///
/// Close the mutex on database connection db.
///
/// Furthermore, if database connection db is a zombie (meaning that there
/// has been a prior call to sqlite3_close(db) or sqlite3_close_v2(db)) and
/// every sqlite3_stmt has now been finalized and every sqlite3_backup has
/// finished, then free all resources.
pub fn sqlite3LeaveMutexAndCloseZombie(db: &mut Sqlite3) {
    // If there are outstanding sqlite3_stmt or sqlite3_backup objects
    // or if the connection has not yet been closed by sqlite3_close_v2(),
    // then just leave the mutex and return.
    if db.eOpenState != SQLITE_STATE_ZOMBIE || connectionIsBusy(db) {
        return;
    }

    // If we reach this point, it means that the database connection has
    // closed all sqlite3_stmt and sqlite3_backup objects and has been
    // passed to sqlite3_close (meaning that it is a zombie).  Therefore,
    // go ahead and free all resources.

    // If a transaction is open, roll it back. This also ensures that if
    // any database schemas have been modified by an uncommitted transaction
    // they are reset. And that the required b-tree mutex is held to make
    // the pager rollback and schema reset an atomic operation.
    sqlite3RollbackAll(db, SQLITE_OK);

    // Free any outstanding Savepoint structures.
    sqlite3CloseSavepoints(db);

    // Close all database connections
    for j in 0..db.aDb.len() {
        if let Some(pBt) = db.aDb[j].pBt.take() {
            // The schema belongs to the BtShared: sqlite3BtreeClose() frees
            // it (xFreeSchema) if this is the last connection using it.
            let bFree = crate::btree::sqlite3BtreeConnectionCount(&pBt) <= 1;
            sqlite3BtreeClose(db, &pBt);
            if j != 1 {
                if bFree {
                    crate::callback::sqlite3SchemaClear(db, j as i32);
                }
                // C: pDb->pSchema = 0
                db.aDb[j].pSchema = Schema::null();
            }
        }
    }
    // Clear the TEMP schema separately and last
    crate::callback::sqlite3SchemaClear(db, 1);
    debug_assert!(db.aDb[1].pSchema.borrow().trigHash.count() == 0);
    crate::vtab::sqlite3VtabUnlockList(db);

    // Free up the array of auxiliary databases
    sqlite3CollapseDatabaseArray(db);
    debug_assert!(db.nDb() <= 2);

    // Tell the code in notify.c that the connection no longer holds any
    // locks and does not require any further unlock-notify callbacks.
    // (Rust: unlock-notify is not compiled in.)

    let mut seen: Vec<Rc<FuncDestructor>> = Vec::new();
    for (_, chain) in db.aFunc.iter() {
        for p in chain.iter() {
            if let Some(d) = &p.pDestructor {
                if !seen.iter().any(|s| Rc::ptr_eq(s, d)) {
                    seen.push(d.clone());
                }
            }
        }
    }
    for d in seen.iter() {
        if let Some(x) = &d.xDestroy {
            x();
        }
    }
    db.aFunc.clear();
    for (_, aColl) in db.aCollSeq.iter() {
        // Invoke any destructors registered for collation sequence user data.
        for j in 0..3 {
            let xDel = aColl[j].xDel.borrow_mut().take();
            if let Some(x) = xDel {
                x();
            }
        }
    }
    db.aCollSeq.clear();
    let mods: Vec<ModuleRef> = db.aModule.iter().map(|(_, m)| m.clone()).collect();
    for pMod in mods.iter() {
        crate::vtab::sqlite3VtabEponymousTableClear(db, pMod);
        crate::vtab::sqlite3VtabModuleUnref(db, pMod);
    }
    db.aModule.clear();

    sqlite3Error(db, SQLITE_OK); // Deallocates any cached error strings.
    db.pErr = None;
    crate::loadext::sqlite3CloseExtensions(db);

    db.eOpenState = SQLITE_STATE_ERROR;

    // Drop the application callbacks (C xAutovacDestr and friends: the
    // closures' captured data is released by Drop).
    db.xAutovacPages = None;
    db.xCommitCallback = None;
    db.xRollbackCallback = None;
    db.xUpdateCallback = None;
    db.xTrace = None;
    db.xTraceLegacy = None;
    db.xProfile = None;
    db.xWalCallback = None;
    db.xCollNeeded = None;
    db.xProgress = None;
    db.xAuth = None;
    db.busyHandler.borrow_mut().xBusyHandler = None;
    db.eOpenState = SQLITE_STATE_CLOSED;
}

/// C: `void sqlite3RollbackAll(sqlite3 *db, int tripCode)`
///
/// Rollback all database files.  If tripCode is not SQLITE_OK, then
/// any write cursors are invalidated ("tripped" - as in "tripping a circuit
/// breaker") and made to return tripCode if there are any further
/// attempts to use that cursor.  Read cursors remain open and valid
/// but are "saved" in case the table pages are moved around.
pub fn sqlite3RollbackAll(db: &mut Sqlite3, tripCode: i32) {
    let mut inTrans = false;
    let schemaChange = (db.mDbFlags & DBFLAG_SchemaChange) != 0 && db.init.busy == 0;

    for i in 0..db.aDb.len() {
        if let Some(p) = db.aDb[i].pBt.clone() {
            if sqlite3BtreeTxnState(db, &p) == SQLITE_TXN_WRITE {
                inTrans = true;
            }
            sqlite3BtreeRollback(db, &p, tripCode, (!schemaChange) as i32);
        }
    }
    crate::vtab::sqlite3VtabRollback(db);

    if schemaChange {
        crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 0);
        crate::build::sqlite3ResetAllSchemasOfConnection(db);
    }

    // Any deferred constraint violations have now been resolved.
    db.nDeferredCons = 0;
    db.nDeferredImmCons = 0;
    db.flags &= !(SQLITE_DeferFKs | SQLITE_CorruptRdOnly);

    // If one has been configured, invoke the rollback-hook callback
    if let Some(x) = db.xRollbackCallback.clone() {
        if inTrans || db.autoCommit == 0 {
            x();
        }
    }
}

// ---------------------------------------------------------------------------
// Error codes and messages
// ---------------------------------------------------------------------------

/// C: `const char * sqlite3ErrName(int rc)`
///
/// Return a static string containing the name corresponding to the error
/// code specified in the argument.
pub fn sqlite3ErrName(rc: i32) -> Option<ZStr> {
    let origRc = rc;
    let mut rc = rc;
    let mut zName: Option<&str> = None;
    let mut i = 0;
    while i < 2 && zName.is_none() {
        zName = match rc {
            SQLITE_OK => Some("SQLITE_OK"),
            SQLITE_ERROR => Some("SQLITE_ERROR"),
            SQLITE_ERROR_SNAPSHOT => Some("SQLITE_ERROR_SNAPSHOT"),
            SQLITE_ERROR_RETRY => Some("SQLITE_ERROR_RETRY"),
            SQLITE_ERROR_MISSING_COLLSEQ => Some("SQLITE_ERROR_MISSING_COLLSEQ"),
            SQLITE_INTERNAL => Some("SQLITE_INTERNAL"),
            SQLITE_PERM => Some("SQLITE_PERM"),
            SQLITE_ABORT => Some("SQLITE_ABORT"),
            SQLITE_ABORT_ROLLBACK => Some("SQLITE_ABORT_ROLLBACK"),
            SQLITE_BUSY => Some("SQLITE_BUSY"),
            SQLITE_BUSY_RECOVERY => Some("SQLITE_BUSY_RECOVERY"),
            SQLITE_BUSY_SNAPSHOT => Some("SQLITE_BUSY_SNAPSHOT"),
            SQLITE_LOCKED => Some("SQLITE_LOCKED"),
            SQLITE_LOCKED_SHAREDCACHE => Some("SQLITE_LOCKED_SHAREDCACHE"),
            SQLITE_NOMEM => Some("SQLITE_NOMEM"),
            SQLITE_READONLY => Some("SQLITE_READONLY"),
            SQLITE_READONLY_RECOVERY => Some("SQLITE_READONLY_RECOVERY"),
            SQLITE_READONLY_CANTINIT => Some("SQLITE_READONLY_CANTINIT"),
            SQLITE_READONLY_ROLLBACK => Some("SQLITE_READONLY_ROLLBACK"),
            SQLITE_READONLY_DBMOVED => Some("SQLITE_READONLY_DBMOVED"),
            SQLITE_READONLY_DIRECTORY => Some("SQLITE_READONLY_DIRECTORY"),
            SQLITE_INTERRUPT => Some("SQLITE_INTERRUPT"),
            SQLITE_IOERR => Some("SQLITE_IOERR"),
            SQLITE_IOERR_READ => Some("SQLITE_IOERR_READ"),
            SQLITE_IOERR_SHORT_READ => Some("SQLITE_IOERR_SHORT_READ"),
            SQLITE_IOERR_WRITE => Some("SQLITE_IOERR_WRITE"),
            SQLITE_IOERR_FSYNC => Some("SQLITE_IOERR_FSYNC"),
            SQLITE_IOERR_DIR_FSYNC => Some("SQLITE_IOERR_DIR_FSYNC"),
            SQLITE_IOERR_TRUNCATE => Some("SQLITE_IOERR_TRUNCATE"),
            SQLITE_IOERR_FSTAT => Some("SQLITE_IOERR_FSTAT"),
            SQLITE_IOERR_UNLOCK => Some("SQLITE_IOERR_UNLOCK"),
            SQLITE_IOERR_RDLOCK => Some("SQLITE_IOERR_RDLOCK"),
            SQLITE_IOERR_DELETE => Some("SQLITE_IOERR_DELETE"),
            SQLITE_IOERR_NOMEM => Some("SQLITE_IOERR_NOMEM"),
            SQLITE_IOERR_ACCESS => Some("SQLITE_IOERR_ACCESS"),
            SQLITE_IOERR_CHECKRESERVEDLOCK => Some("SQLITE_IOERR_CHECKRESERVEDLOCK"),
            SQLITE_IOERR_LOCK => Some("SQLITE_IOERR_LOCK"),
            SQLITE_IOERR_CLOSE => Some("SQLITE_IOERR_CLOSE"),
            SQLITE_IOERR_DIR_CLOSE => Some("SQLITE_IOERR_DIR_CLOSE"),
            SQLITE_IOERR_SHMOPEN => Some("SQLITE_IOERR_SHMOPEN"),
            SQLITE_IOERR_SHMSIZE => Some("SQLITE_IOERR_SHMSIZE"),
            SQLITE_IOERR_SHMLOCK => Some("SQLITE_IOERR_SHMLOCK"),
            SQLITE_IOERR_SHMMAP => Some("SQLITE_IOERR_SHMMAP"),
            SQLITE_IOERR_SEEK => Some("SQLITE_IOERR_SEEK"),
            SQLITE_IOERR_DELETE_NOENT => Some("SQLITE_IOERR_DELETE_NOENT"),
            SQLITE_IOERR_MMAP => Some("SQLITE_IOERR_MMAP"),
            SQLITE_IOERR_GETTEMPPATH => Some("SQLITE_IOERR_GETTEMPPATH"),
            SQLITE_IOERR_CONVPATH => Some("SQLITE_IOERR_CONVPATH"),
            SQLITE_CORRUPT => Some("SQLITE_CORRUPT"),
            SQLITE_CORRUPT_VTAB => Some("SQLITE_CORRUPT_VTAB"),
            SQLITE_NOTFOUND => Some("SQLITE_NOTFOUND"),
            SQLITE_FULL => Some("SQLITE_FULL"),
            SQLITE_CANTOPEN => Some("SQLITE_CANTOPEN"),
            SQLITE_CANTOPEN_NOTEMPDIR => Some("SQLITE_CANTOPEN_NOTEMPDIR"),
            SQLITE_CANTOPEN_ISDIR => Some("SQLITE_CANTOPEN_ISDIR"),
            SQLITE_CANTOPEN_FULLPATH => Some("SQLITE_CANTOPEN_FULLPATH"),
            SQLITE_CANTOPEN_CONVPATH => Some("SQLITE_CANTOPEN_CONVPATH"),
            SQLITE_CANTOPEN_SYMLINK => Some("SQLITE_CANTOPEN_SYMLINK"),
            SQLITE_PROTOCOL => Some("SQLITE_PROTOCOL"),
            SQLITE_EMPTY => Some("SQLITE_EMPTY"),
            SQLITE_SCHEMA => Some("SQLITE_SCHEMA"),
            SQLITE_TOOBIG => Some("SQLITE_TOOBIG"),
            SQLITE_CONSTRAINT => Some("SQLITE_CONSTRAINT"),
            SQLITE_CONSTRAINT_UNIQUE => Some("SQLITE_CONSTRAINT_UNIQUE"),
            SQLITE_CONSTRAINT_TRIGGER => Some("SQLITE_CONSTRAINT_TRIGGER"),
            SQLITE_CONSTRAINT_FOREIGNKEY => Some("SQLITE_CONSTRAINT_FOREIGNKEY"),
            SQLITE_CONSTRAINT_CHECK => Some("SQLITE_CONSTRAINT_CHECK"),
            SQLITE_CONSTRAINT_PRIMARYKEY => Some("SQLITE_CONSTRAINT_PRIMARYKEY"),
            SQLITE_CONSTRAINT_NOTNULL => Some("SQLITE_CONSTRAINT_NOTNULL"),
            SQLITE_CONSTRAINT_COMMITHOOK => Some("SQLITE_CONSTRAINT_COMMITHOOK"),
            SQLITE_CONSTRAINT_VTAB => Some("SQLITE_CONSTRAINT_VTAB"),
            SQLITE_CONSTRAINT_FUNCTION => Some("SQLITE_CONSTRAINT_FUNCTION"),
            SQLITE_CONSTRAINT_ROWID => Some("SQLITE_CONSTRAINT_ROWID"),
            SQLITE_MISMATCH => Some("SQLITE_MISMATCH"),
            SQLITE_MISUSE => Some("SQLITE_MISUSE"),
            SQLITE_NOLFS => Some("SQLITE_NOLFS"),
            SQLITE_AUTH => Some("SQLITE_AUTH"),
            SQLITE_FORMAT => Some("SQLITE_FORMAT"),
            SQLITE_RANGE => Some("SQLITE_RANGE"),
            SQLITE_NOTADB => Some("SQLITE_NOTADB"),
            SQLITE_ROW => Some("SQLITE_ROW"),
            SQLITE_NOTICE => Some("SQLITE_NOTICE"),
            SQLITE_NOTICE_RECOVER_WAL => Some("SQLITE_NOTICE_RECOVER_WAL"),
            SQLITE_NOTICE_RECOVER_ROLLBACK => Some("SQLITE_NOTICE_RECOVER_ROLLBACK"),
            SQLITE_NOTICE_RBU => Some("SQLITE_NOTICE_RBU"),
            SQLITE_WARNING => Some("SQLITE_WARNING"),
            SQLITE_WARNING_AUTOINDEX => Some("SQLITE_WARNING_AUTOINDEX"),
            SQLITE_DONE => Some("SQLITE_DONE"),
            _ => None,
        };
        i += 1;
        rc &= 0xff;
    }
    match zName {
        Some(z) => Some(z.as_bytes().to_vec()),
        None => Some(format!("SQLITE_UNKNOWN({})", origRc).into_bytes()),
    }
}

/// C: `const char * sqlite3ErrStr(int rc)`
///
/// Return a static string that describes the kind of error specified in
/// the argument.
pub fn sqlite3ErrStr(rc: i32) -> Option<ZStr> {
    const aMsg: [Option<&str>; 29] = [
        /* SQLITE_OK          */ Some("not an error"),
        /* SQLITE_ERROR       */ Some("SQL logic error"),
        /* SQLITE_INTERNAL    */ None,
        /* SQLITE_PERM        */ Some("access permission denied"),
        /* SQLITE_ABORT       */ Some("query aborted"),
        /* SQLITE_BUSY        */ Some("database is locked"),
        /* SQLITE_LOCKED      */ Some("database table is locked"),
        /* SQLITE_NOMEM       */ Some("out of memory"),
        /* SQLITE_READONLY    */ Some("attempt to write a readonly database"),
        /* SQLITE_INTERRUPT   */ Some("interrupted"),
        /* SQLITE_IOERR       */ Some("disk I/O error"),
        /* SQLITE_CORRUPT     */ Some("database disk image is malformed"),
        /* SQLITE_NOTFOUND    */ Some("unknown operation"),
        /* SQLITE_FULL        */ Some("database or disk is full"),
        /* SQLITE_CANTOPEN    */ Some("unable to open database file"),
        /* SQLITE_PROTOCOL    */ Some("locking protocol"),
        /* SQLITE_EMPTY       */ None,
        /* SQLITE_SCHEMA      */ Some("database schema has changed"),
        /* SQLITE_TOOBIG      */ Some("string or blob too big"),
        /* SQLITE_CONSTRAINT  */ Some("constraint failed"),
        /* SQLITE_MISMATCH    */ Some("datatype mismatch"),
        /* SQLITE_MISUSE      */ Some("bad parameter or other API misuse"),
        /* SQLITE_NOLFS       */ None,
        /* SQLITE_AUTH        */ Some("authorization denied"),
        /* SQLITE_FORMAT      */ None,
        /* SQLITE_RANGE       */ Some("column index out of range"),
        /* SQLITE_NOTADB      */ Some("file is not a database"),
        /* SQLITE_NOTICE      */ Some("notification message"),
        /* SQLITE_WARNING     */ Some("warning message"),
    ];
    let mut zErr = "unknown error";
    match rc {
        SQLITE_ABORT_ROLLBACK => {
            zErr = "abort due to ROLLBACK";
        }
        SQLITE_ROW => {
            zErr = "another row available";
        }
        SQLITE_DONE => {
            zErr = "no more rows available";
        }
        _ => {
            let rc = rc & 0xff;
            if rc >= 0 && (rc as usize) < aMsg.len() {
                if let Some(m) = aMsg[rc as usize] {
                    zErr = m;
                }
            }
        }
    }
    Some(zErr.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// Busy handler, progress handler, interrupt
// ---------------------------------------------------------------------------

/// This routine implements a busy callback that sleeps and tries
/// again until a timeout value is reached.  The timeout value is
/// an integer number of milliseconds passed in as the first
/// argument.
///
/// Return non-zero to retry the lock.  Return zero to stop trying
/// and cause SQLite to return SQLITE_BUSY.
///
/// Rust: the closure installed by sqlite3_busy_timeout() captures the VFS
/// and the timeout (C reads both from the connection).
fn sqliteDefaultBusyCallback(pVfs: &Vfs, tmout: i32, count: i32) -> i32 {
    // This case is for systems that have support for sleeping for fractions
    // of a second.  Examples:  All windows systems, unix systems with
    // nanosleep()
    const delays: [u8; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
    const totals: [u8; 12] = [0, 1, 3, 8, 18, 33, 53, 78, 103, 128, 178, 228];
    const NDELAY: i32 = 12;
    let mut delay: i32;
    let prior: i32;

    debug_assert!(count >= 0);
    if count < NDELAY {
        delay = delays[count as usize] as i32;
        prior = totals[count as usize] as i32;
    } else {
        delay = delays[(NDELAY - 1) as usize] as i32;
        prior = totals[(NDELAY - 1) as usize] as i32 + delay * (count - (NDELAY - 1));
    }
    if prior + delay > tmout {
        delay = tmout - prior;
        if delay <= 0 {
            return 0;
        }
    }
    crate::os::sqlite3OsSleep(pVfs, delay * 1000);
    1
}

/// C: `int sqlite3InvokeBusyHandler(BusyHandler *p)`
///
/// Invoke the given busy handler.
///
/// This routine is called when an operation failed to acquire a
/// lock on VFS file pFile.
///
/// If this routine returns non-zero, the lock is retried.  If it
/// returns 0, the operation aborts with an SQLITE_BUSY error.
pub fn sqlite3InvokeBusyHandler(p: &mut BusyHandler) -> i32 {
    let Some(x) = p.xBusyHandler.clone() else {
        return 0;
    };
    if p.nBusy < 0 {
        return 0;
    }
    let rc = x(p.nBusy);
    if rc == 0 {
        p.nBusy = -1;
    } else {
        p.nBusy += 1;
    }
    rc
}

/// C: `int sqlite3_busy_handler( sqlite3 *db, int (*xBusy)(void*,int), void *pArg )`
///
/// This routine sets the busy callback for an Sqlite database to the
/// given callback function with the given argument.
pub fn sqlite3_busy_handler(db: &mut Sqlite3, xBusy: Option<XBusy>) -> i32 {
    {
        let mut b = db.busyHandler.borrow_mut();
        b.xBusyHandler = xBusy;
        b.nBusy = 0;
    }
    db.busyTimeout = 0;
    SQLITE_OK
}

/// C: `void sqlite3_progress_handler( sqlite3 *db, int nOps, int (*xProgress)(void*), void *pArg )`
///
/// This routine sets the progress callback for an Sqlite database to the
/// given callback function with the given argument. The progress callback
/// will be invoked every nOps opcodes.
pub fn sqlite3_progress_handler(
    db: &mut Sqlite3,
    nOps: i32,
    xProgress: Option<Rc<dyn Fn() -> i32>>,
) {
    if nOps > 0 {
        db.xProgress = xProgress;
        db.nProgressOps = nOps as u32;
    } else {
        db.xProgress = None;
        db.nProgressOps = 0;
    }
}

/// C: `int sqlite3_busy_timeout(sqlite3 *db, int ms)`
///
/// This routine installs a default busy handler that waits for the
/// specified number of milliseconds before returning 0.
pub fn sqlite3_busy_timeout(db: &mut Sqlite3, ms: i32) -> i32 {
    if ms > 0 {
        let pVfs = db.pVfs.clone();
        let x: XBusy = Rc::new(move |count: i32| sqliteDefaultBusyCallback(&pVfs, ms, count));
        sqlite3_busy_handler(db, Some(x));
        db.busyTimeout = ms;
    } else {
        sqlite3_busy_handler(db, None);
    }
    SQLITE_OK
}

/// C: `int sqlite3_setlk_timeout(sqlite3 *db, int ms, int flags)`
///
/// Set the setlk timeout value.  (SQLITE_ENABLE_SETLK_TIMEOUT is not
/// compiled in: only the range check remains.)
pub fn sqlite3_setlk_timeout(db: &mut Sqlite3, ms: i32, flags: i32) -> i32 {
    let _ = (db, flags);
    if ms < -1 {
        return SQLITE_RANGE;
    }
    SQLITE_OK
}

/// C: `void sqlite3_interrupt(sqlite3 *db)`
///
/// Cause any pending operation to stop at its earliest opportunity.
/// Rust: `db.isInterrupted` is shared (Rc<Cell>), so a clone of it can
/// also be set from a callback.
pub fn sqlite3_interrupt(db: &mut Sqlite3) {
    db.isInterrupted.set(1);
}

/// C: `int sqlite3_is_interrupted(sqlite3 *db)`
///
/// Return true or false depending on whether or not an interrupt is
/// pending on connection db.
pub fn sqlite3_is_interrupted(db: &mut Sqlite3) -> i32 {
    (db.isInterrupted.get() != 0) as i32
}

// ---------------------------------------------------------------------------
// Application-defined functions
// ---------------------------------------------------------------------------

/// C: `int sqlite3CreateFunc( sqlite3 *db, const char *zFunctionName, int nArg, int enc, void *pUserData, void (*xSFunc)(sqlite3_context*,int,sqlite3_value **), void (*xStep)(sqlite3_context*,int,sqlite3_value **), void (*xFinal)(sqlite3_context*), void (*xValue)(sqlite3_context*), void (*xInverse)(sqlite3_context*,int,sqlite3_value **), FuncDestructor *pDestructor )`
///
/// This function is exactly the same as sqlite3_create_function(), except
/// that it is designed to be called by internal code. The difference is
/// that if a malloc() fails in sqlite3_create_function(), an error code
/// is returned and the mallocFailed flag cleared.
#[allow(clippy::too_many_arguments)]
pub fn sqlite3CreateFunc(
    db: &mut Sqlite3,
    zFunctionName: Option<&[u8]>,
    nArg: i32,
    enc: i32,
    pUserData: FuncUserData,
    xSFunc: Option<XSFunc>,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
    xValue: Option<XFinal>,
    xInverse: Option<XSFunc>,
    pDestructor: Option<Rc<FuncDestructor>>,
) -> i32 {
    let mut enc = enc;
    debug_assert!(xValue.is_none() || xSFunc.is_none());
    let Some(zFunctionName) = zFunctionName else {
        return SQLITE_MISUSE; // Must have a valid name
    };
    let zFunctionName = &zFunctionName[..cstrlen(zFunctionName)];
    if (xSFunc.is_some() && xFinal.is_some()) // Not both xSFunc and xFinal
        || (xFinal.is_none() != xStep.is_none()) // Both or neither of xFinal and xStep
        || (xValue.is_none() != xInverse.is_none()) // Both or neither of xValue, xInverse
        || (nArg < -1 || nArg > SQLITE_MAX_FUNCTION_ARG)
        || (255 < zFunctionName.len().min(0x3fffffff))
    {
        return SQLITE_MISUSE;
    }

    let mut extraFlags = (enc as u32)
        & (SQLITE_DETERMINISTIC
            | SQLITE_DIRECTONLY
            | SQLITE_SUBTYPE
            | SQLITE_INNOCUOUS
            | SQLITE_RESULT_SUBTYPE
            | SQLITE_SELFORDER1);
    enc &= (SQLITE_FUNC_ENCMASK as i32) | SQLITE_ANY;

    // The SQLITE_INNOCUOUS flag is the same bit as SQLITE_FUNC_UNSAFE.  But
    // the meaning is inverted.  So flip the bit.
    extraFlags ^= SQLITE_FUNC_UNSAFE; // tag-20230109-1

    // If SQLITE_UTF16 is specified as the encoding type, transform this
    // to one of SQLITE_UTF16LE or SQLITE_UTF16BE using the
    // SQLITE_UTF16NATIVE macro. SQLITE_UTF16 is not used internally.
    //
    // If SQLITE_ANY is specified, add three versions of the function
    // to the hash table.
    if enc == SQLITE_UTF16 as i32 {
        enc = SQLITE_UTF16NATIVE as i32;
    } else if enc == SQLITE_ANY {
        let mut rc = sqlite3CreateFunc(
            db,
            Some(zFunctionName),
            nArg,
            ((SQLITE_UTF8 as u32 | extraFlags) ^ SQLITE_FUNC_UNSAFE) as i32, // tag-20230109-1
            pUserData.clone(),
            xSFunc.clone(),
            xStep.clone(),
            xFinal.clone(),
            xValue.clone(),
            xInverse.clone(),
            pDestructor.clone(),
        );
        if rc == SQLITE_OK {
            rc = sqlite3CreateFunc(
                db,
                Some(zFunctionName),
                nArg,
                ((SQLITE_UTF16LE as u32 | extraFlags) ^ SQLITE_FUNC_UNSAFE) as i32, // tag-20230109-1
                pUserData.clone(),
                xSFunc.clone(),
                xStep.clone(),
                xFinal.clone(),
                xValue.clone(),
                xInverse.clone(),
                pDestructor.clone(),
            );
        }
        if rc != SQLITE_OK {
            return rc;
        }
        enc = SQLITE_UTF16BE as i32;
    } else if enc == SQLITE_UTF8 as i32
        || enc == SQLITE_UTF16LE as i32
        || enc == SQLITE_UTF16BE as i32
    {
        // ok
    } else {
        enc = SQLITE_UTF8 as i32;
    }

    // Check if an existing function is being overridden or deleted. If so,
    // and there are active VMs, then return SQLITE_BUSY. If a function
    // is being overridden/deleted but there are no active VMs, allow the
    // operation to continue but invalidate all precompiled statements.
    let p = sqlite3FindFunction(db, Some(zFunctionName), nArg, enc as u8, 0);
    match &p {
        Some(p) if (p.funcFlags & SQLITE_FUNC_ENCMASK) == enc as u32 && p.nArg as i32 == nArg => {
            if db.nVdbeActive != 0 {
                sqlite3ErrorWithMsg(
                    db,
                    SQLITE_BUSY,
                    Some(b"unable to delete/modify user-function due to active statements"),
                    &[],
                );
                debug_assert!(db.mallocFailed == 0 || db.nVdbeExec > 0);
                return SQLITE_BUSY;
            } else {
                crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 0);
            }
        }
        _ => {
            if xSFunc.is_none() && xFinal.is_none() {
                // Trying to delete a function that does not exist.  This is
                // a no-op.  https://sqlite.org/forum/forumpost/726219164b
                return SQLITE_OK;
            }
        }
    }

    let Some(p) = sqlite3FindFunction(db, Some(zFunctionName), nArg, enc as u8, 1) else {
        return SQLITE_NOMEM;
    };

    // If an older version of the function with a configured destructor is
    // being replaced invoke the destructor function here.
    functionDestroy(db, &p);

    let pNew = FuncDef {
        nArg: nArg as u16 as i16,
        funcFlags: (p.funcFlags & SQLITE_FUNC_ENCMASK) | extraFlags,
        pUserData,
        xSFunc: xSFunc.or(xStep),
        xFinalize: xFinal,
        xValue,
        xInverse,
        zName: p.zName.clone(),
        pDestructor,
    };
    sqlite3FuncDefReplace(db, &p, pNew);
    SQLITE_OK
}

/// Worker function used by utf-8 APIs that create new functions:
///
///    sqlite3_create_function()
///    sqlite3_create_function_v2()
///    sqlite3_create_window_function()
#[allow(clippy::too_many_arguments)]
fn createFunctionApi(
    db: &mut Sqlite3,
    zFunc: Option<&[u8]>,
    nArg: i32,
    enc: i32,
    p: FuncUserData,
    xSFunc: Option<XSFunc>,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
    xValue: Option<XFinal>,
    xInverse: Option<XSFunc>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> i32 {
    let mut pArg: Option<Rc<FuncDestructor>> = None;
    if let Some(x) = &xDestroy {
        pArg = Some(Rc::new(FuncDestructor {
            xDestroy: Some(x.clone()),
        }));
    }
    let mut rc = sqlite3CreateFunc(
        db,
        zFunc,
        nArg,
        enc,
        p,
        xSFunc,
        xStep.clone(),
        xFinal.clone(),
        xValue,
        xInverse,
        pArg.clone(),
    );
    if let Some(pArg) = &pArg {
        if destructorRefs(db, pArg) == 0 {
            debug_assert!(rc != SQLITE_OK || (xStep.is_none() && xFinal.is_none()));
            if let Some(x) = &xDestroy {
                x();
            }
        }
    }
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_create_function( sqlite3 *db, const char *zFunc, int nArg, int enc, void *p, void (*xSFunc)(sqlite3_context*,int,sqlite3_value **), void (*xStep)(sqlite3_context*,int,sqlite3_value **), void (*xFinal)(sqlite3_context*) )`
///
/// Create new user functions.
#[allow(clippy::too_many_arguments)]
pub fn sqlite3_create_function(
    db: &mut Sqlite3,
    zFunc: Option<&[u8]>,
    nArg: i32,
    enc: i32,
    p: FuncUserData,
    xSFunc: Option<XSFunc>,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
) -> i32 {
    createFunctionApi(
        db, zFunc, nArg, enc, p, xSFunc, xStep, xFinal, None, None, None,
    )
}

/// C: `int sqlite3_create_function_v2( sqlite3 *db, const char *zFunc, int nArg, int enc, void *p, void (*xSFunc)(sqlite3_context*,int,sqlite3_value **), void (*xStep)(sqlite3_context*,int,sqlite3_value **), void (*xFinal)(sqlite3_context*), void (*xDestroy)(void *) )`
#[allow(clippy::too_many_arguments)]
pub fn sqlite3_create_function_v2(
    db: &mut Sqlite3,
    zFunc: Option<&[u8]>,
    nArg: i32,
    enc: i32,
    p: FuncUserData,
    xSFunc: Option<XSFunc>,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> i32 {
    createFunctionApi(
        db, zFunc, nArg, enc, p, xSFunc, xStep, xFinal, None, None, xDestroy,
    )
}

/// C: `int sqlite3_create_window_function( sqlite3 *db, const char *zFunc, int nArg, int enc, void *p, void (*xStep)(sqlite3_context*,int,sqlite3_value **), void (*xFinal)(sqlite3_context*), void (*xValue)(sqlite3_context*), void (*xInverse)(sqlite3_context*,int,sqlite3_value **), void (*xDestroy)(void *) )`
#[allow(clippy::too_many_arguments)]
pub fn sqlite3_create_window_function(
    db: &mut Sqlite3,
    zFunc: Option<&[u8]>,
    nArg: i32,
    enc: i32,
    p: FuncUserData,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
    xValue: Option<XFinal>,
    xInverse: Option<XSFunc>,
    xDestroy: Option<Rc<dyn Fn()>>,
) -> i32 {
    createFunctionApi(
        db, zFunc, nArg, enc, p, None, xStep, xFinal, xValue, xInverse, xDestroy,
    )
}

/// C: `int sqlite3_create_function16( sqlite3 *db, const void *zFunctionName, int nArg, int eTextRep, void *p, void (*xSFunc)(sqlite3_context*,int,sqlite3_value**), void (*xStep)(sqlite3_context*,int,sqlite3_value**), void (*xFinal)(sqlite3_context*) )`
///
/// Rust: zFunctionName is native-endian UTF-16 (terminated by the end of
/// the slice or a U+0000).
#[allow(clippy::too_many_arguments)]
pub fn sqlite3_create_function16(
    db: &mut Sqlite3,
    zFunctionName: &[u8],
    nArg: i32,
    eTextRep: i32,
    p: FuncUserData,
    xSFunc: Option<XSFunc>,
    xStep: Option<XSFunc>,
    xFinal: Option<XFinal>,
) -> i32 {
    let zFunc8 = utf16z_to_utf8(zFunctionName);
    let rc = sqlite3CreateFunc(
        db,
        Some(&zFunc8),
        nArg,
        eTextRep,
        p,
        xSFunc,
        xStep,
        xFinal,
        None,
        None,
        None,
    );
    sqlite3ApiExit(db, rc)
}

/// UTF-16 (native byte order, terminated by U+0000 or the end of the
/// slice) to UTF-8.
fn utf16z_to_utf8(z: &[u8]) -> ZStr {
    let mut n = 0;
    while n + 1 < z.len() && (z[n] != 0 || z[n + 1] != 0) {
        n += 2;
    }
    crate::prepare::utf16ne_to_utf8(&z[..n])
}

/// UTF-8 to native byte order UTF-16, with a U+0000 terminator.
fn utf8_to_utf16z(z: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(z);
    let mut out: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_ne_bytes()).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// The following is the implementation of an SQL function that always
/// fails with an error message stating that the function is used in the
/// wrong context.  The sqlite3_overload_function() API might construct
/// SQL function that use this routine so that the functions will exist
/// for name resolution but are actually overloaded by the xFindFunction
/// method of virtual tables.
fn sqlite3InvalidFunction(context: &mut Context, zName: &[u8]) {
    let zErr = crate::printf::sqlite3_mprintf(
        b"unable to use function %s in the requested context",
        &[PfArg::Str(Some(zName))],
    );
    crate::vdbeapi::sqlite3_result_error(context, Some(&zErr), -1);
}

/// C: `int sqlite3_overload_function( sqlite3 *db, const char *zName, int nArg )`
///
/// Declare that a function has been overloaded by a virtual table.
///
/// If the function already exists as a regular global function, then
/// this routine is a no-op.  If the function does not exist, then create
/// a new one that always throws a run-time error.
///
/// When virtual tables intend to provide an overloaded function, they
/// should call this routine to make sure the global function exists.
/// A global function must exist in order for name resolution to work
/// properly.
pub fn sqlite3_overload_function(db: &mut Sqlite3, zName: Option<&[u8]>, nArg: i32) -> i32 {
    let Some(zName) = zName else {
        return SQLITE_MISUSE;
    };
    if nArg < -2 {
        return SQLITE_MISUSE;
    }
    let rc = sqlite3FindFunction(db, Some(zName), nArg, SQLITE_UTF8, 0).is_some();
    if rc {
        return SQLITE_OK;
    }
    let zCopy: Rc<ZStr> = Rc::new(zName.to_vec());
    let zc = zCopy.clone();
    let x: XSFunc =
        Rc::new(move |ctx: &mut Context, _argv: &mut [Mem]| sqlite3InvalidFunction(ctx, &zc));
    sqlite3_create_function_v2(
        db,
        Some(zName),
        nArg,
        SQLITE_UTF8 as i32,
        FuncUserData::Any(zCopy),
        Some(x),
        None,
        None,
        None,
    )
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// C: `void * sqlite3_trace(sqlite3 *db, void(*xTrace)(void*,const char*), void *pArg)`
///
/// Register a trace function.  The previously registered trace is
/// returned.
///
/// A NULL trace function means that no tracing is executes.  A non-NULL
/// trace is a pointer to a function that is invoked at the start of each
/// SQL statement.
pub fn sqlite3_trace(
    db: &mut Sqlite3,
    xTrace: Option<Rc<dyn Fn(&[u8])>>,
) -> Option<Rc<dyn Fn(&[u8])>> {
    let pOld = db.xTraceLegacy.take();
    db.mTrace = if xTrace.is_some() {
        SQLITE_TRACE_LEGACY as u8
    } else {
        0
    };
    db.xTraceLegacy = xTrace;
    pOld
}

/// C: `int sqlite3_trace_v2( sqlite3 *db, unsigned mTrace, int(*xTrace)(unsigned,void*,void*,void*), void *pArg )`
///
/// Register a trace callback using the version-2 interface.
pub fn sqlite3_trace_v2(
    db: &mut Sqlite3,
    mTrace: u32,
    xTrace: Option<Rc<dyn Fn(u32, &[u8]) -> i32>>,
) -> i32 {
    let mut mTrace = mTrace;
    let mut xTrace = xTrace;
    if mTrace == 0 {
        xTrace = None;
    }
    if xTrace.is_none() {
        mTrace = 0;
    }
    db.mTrace = mTrace as u8;
    db.xTrace = xTrace;
    SQLITE_OK
}

/// C: `void * sqlite3_profile( sqlite3 *db, void (*xProfile)(void*,const char*,sqlite_uint64), void *pArg )`
///
/// Register a profile function.  The previously registered profile
/// function is returned.
///
/// A NULL profile function means that no profiling is executes.  A non-NULL
/// profile is a pointer to a function that is invoked at the conclusion of
/// each SQL statement that is run.
pub fn sqlite3_profile(
    db: &mut Sqlite3,
    xProfile: Option<Rc<dyn Fn(&[u8], u64)>>,
) -> Option<Rc<dyn Fn(&[u8], u64)>> {
    let pOld = db.xProfile.take();
    db.xProfile = xProfile;
    db.mTrace &= SQLITE_TRACE_NONLEGACY_MASK as u8;
    if db.xProfile.is_some() {
        db.mTrace |= SQLITE_TRACE_XPROFILE as u8;
    }
    pOld
}

/// C: `void * sqlite3_commit_hook( sqlite3 *db, int (*xCallback)(void*), void *pArg )`
///
/// Register a function to be invoked when a transaction commits.
/// If the invoked function returns non-zero, then the commit becomes a
/// rollback.
pub fn sqlite3_commit_hook(
    db: &mut Sqlite3,
    xCallback: Option<Rc<dyn Fn() -> i32>>,
) -> Option<Rc<dyn Fn() -> i32>> {
    std::mem::replace(&mut db.xCommitCallback, xCallback)
}

/// C: `void * sqlite3_update_hook( sqlite3 *db, void (*xCallback)(void*,int,char const *,char const *,sqlite_int64), void *pArg )`
///
/// Register a callback to be invoked each time a row is updated,
/// inserted or deleted using this database connection.
pub fn sqlite3_update_hook(
    db: &mut Sqlite3,
    xCallback: Option<Rc<dyn Fn(i32, &[u8], &[u8], i64)>>,
) -> Option<Rc<dyn Fn(i32, &[u8], &[u8], i64)>> {
    std::mem::replace(&mut db.xUpdateCallback, xCallback)
}

/// C: `void * sqlite3_rollback_hook( sqlite3 *db, void (*xCallback)(void*), void *pArg )`
///
/// Register a callback to be invoked each time a transaction is rolled
/// back by this database connection.
pub fn sqlite3_rollback_hook(
    db: &mut Sqlite3,
    xCallback: Option<Rc<dyn Fn()>>,
) -> Option<Rc<dyn Fn()>> {
    std::mem::replace(&mut db.xRollbackCallback, xCallback)
}

/// C: `int sqlite3_autovacuum_pages( sqlite3 *db, unsigned int (*xCallback)(void*,const char*,u32,u32,u32), void *pArg, void (*xDestructor)(void*) )`
///
/// Register a function to be invoked prior to each autovacuum that
/// determines the number of pages to vacuum.  Rust: the C destructor of
/// pArg is the Drop of the closure.
pub fn sqlite3_autovacuum_pages(
    db: &mut Sqlite3,
    xCallback: Option<Rc<dyn Fn(&[u8], u32, u32, u32) -> u32>>,
) -> i32 {
    db.xAutovacPages = xCallback;
    SQLITE_OK
}

/// C: `int sqlite3WalDefaultHook( void *pClientData, sqlite3 *db, const char *zDb, int nFrame )`
///
/// The sqlite3_wal_hook() callback registered by
/// sqlite3_wal_autocheckpoint().  Invoke sqlite3_wal_checkpoint if the
/// number of frames in the log file is greater than the configured
/// threshold (C: SQLITE_PTR_TO_INT(pClientData)).
pub fn sqlite3WalDefaultHook(
    db: &mut Sqlite3,
    nThreshold: i32,
    zDb: Option<&[u8]>,
    nFrame: i32,
) -> i32 {
    if nFrame >= nThreshold {
        sqlite3_wal_checkpoint(db, zDb);
    }
    SQLITE_OK
}

/// C: `int sqlite3_wal_autocheckpoint(sqlite3 *db, int nFrame)`
///
/// Configure an sqlite3_wal_hook() callback to automatically checkpoint
/// a database after committing a transaction if there are nFrame or
/// more frames in the log file. Passing zero or a negative value as the
/// nFrame parameter disables automatic checkpoints entirely.
///
/// The callback registered by this function replaces any existing callback
/// registered using sqlite3_wal_hook(). Likewise, registering a callback
/// using sqlite3_wal_hook() disables the automatic checkpoint mechanism
/// configured by this function.
pub fn sqlite3_wal_autocheckpoint(db: &mut Sqlite3, nFrame: i32) -> i32 {
    if nFrame > 0 {
        let x: Rc<dyn Fn(&mut Sqlite3, &[u8], i32) -> i32> =
            Rc::new(move |db: &mut Sqlite3, zDb: &[u8], n: i32| {
                sqlite3WalDefaultHook(db, nFrame, Some(zDb), n)
            });
        sqlite3_wal_hook(db, Some(x));
        db.walAutoCkpt = nFrame;
    } else {
        sqlite3_wal_hook(db, None);
    }
    SQLITE_OK
}

/// C: `void * sqlite3_wal_hook( sqlite3 *db, int(*xCallback)(void *, sqlite3*, const char*, int), void *pArg )`
///
/// Register a callback to be invoked each time a transaction is written
/// into the write-ahead-log by this database connection.
pub fn sqlite3_wal_hook(
    db: &mut Sqlite3,
    xCallback: Option<Rc<dyn Fn(&mut Sqlite3, &[u8], i32) -> i32>>,
) -> Option<Rc<dyn Fn(&mut Sqlite3, &[u8], i32) -> i32>> {
    db.walAutoCkpt = 0;
    std::mem::replace(&mut db.xWalCallback, xCallback)
}

/// C: `int sqlite3_wal_checkpoint_v2( sqlite3 *db, const char *zDb, int eMode, int *pnLog, int *pnCkpt )`
///
/// Checkpoint database zDb.
pub fn sqlite3_wal_checkpoint_v2(
    db: &mut Sqlite3,
    zDb: Option<&[u8]>,
    eMode: i32,
    pnLog: Option<&mut i32>,
    pnCkpt: Option<&mut i32>,
) -> i32 {
    let mut rc;
    let iDb;
    let mut pnLog = pnLog;
    let mut pnCkpt = pnCkpt;

    // Initialize the output variables to -1 in case an error occurs.
    if let Some(p) = pnLog.as_deref_mut() {
        *p = -1;
    }
    if let Some(p) = pnCkpt.as_deref_mut() {
        *p = -1;
    }

    if eMode < SQLITE_CHECKPOINT_NOOP || eMode > SQLITE_CHECKPOINT_TRUNCATE {
        // EVIDENCE-OF: R-03996-12088 The M parameter must be a valid
        // checkpoint mode:
        return SQLITE_MISUSE;
    }

    match zDb {
        Some(z) if !z.is_empty() && z[0] != 0 => {
            iDb = sqlite3FindDbName(db, Some(z));
        }
        _ => {
            iDb = SQLITE_MAX_DB; // This means process all schemas
        }
    }
    if iDb < 0 {
        rc = SQLITE_ERROR;
        sqlite3ErrorWithMsg(
            db,
            SQLITE_ERROR,
            Some(b"unknown database: %s"),
            &[PfArg::Str(zDb)],
        );
    } else {
        db.busyHandler.borrow_mut().nBusy = 0;
        rc = sqlite3Checkpoint(db, iDb, eMode, pnLog, pnCkpt);
        sqlite3Error(db, rc);
    }
    rc = sqlite3ApiExit(db, rc);

    // If there are no active statements, clear the interrupt flag at this
    // point.
    if db.nVdbeActive == 0 {
        db.isInterrupted.set(0);
    }
    rc
}

/// C: `int sqlite3_wal_checkpoint(sqlite3 *db, const char *zDb)`
///
/// Checkpoint database zDb. If zDb is NULL, or if the buffer zDb points
/// to contains a zero-length string, all attached databases are
/// checkpointed.
pub fn sqlite3_wal_checkpoint(db: &mut Sqlite3, zDb: Option<&[u8]>) -> i32 {
    // EVIDENCE-OF: R-41613-20553 The sqlite3_wal_checkpoint(D,X) is
    // equivalent to sqlite3_wal_checkpoint_v2(D,X,SQLITE_CHECKPOINT_PASSIVE,0,0).
    sqlite3_wal_checkpoint_v2(db, zDb, SQLITE_CHECKPOINT_PASSIVE, None, None)
}

/// C: `int sqlite3Checkpoint(sqlite3 *db, int iDb, int eMode, int *pnLog, int *pnCkpt)`
///
/// Run a checkpoint on database iDb. This is a no-op if database iDb is
/// not currently open in WAL mode.
///
/// If a transaction is open on the database being checkpointed, this
/// function returns SQLITE_LOCKED and a checkpoint is not attempted. If
/// an error occurs while running the checkpoint, an SQLite error code is
/// returned (i.e. SQLITE_IOERR). Otherwise, SQLITE_OK.
///
/// If iDb is passed SQLITE_MAX_DB then all attached databases are
/// checkpointed. If an error is encountered it is returned immediately -
/// no attempt is made to checkpoint any remaining databases.
///
/// Parameter eMode is one of SQLITE_CHECKPOINT_PASSIVE, FULL, RESTART
/// or TRUNCATE.
pub fn sqlite3Checkpoint(
    db: &mut Sqlite3,
    iDb: i32,
    eMode: i32,
    pnLog: Option<&mut i32>,
    pnCkpt: Option<&mut i32>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut bBusy = false;
    let mut pnLog = pnLog;
    let mut pnCkpt = pnCkpt;

    let mut i = 0;
    while i < db.aDb.len() && rc == SQLITE_OK {
        if i as i32 == iDb || iDb == SQLITE_MAX_DB {
            if let Some(pBt) = db.aDb[i].pBt.clone() {
                let mut dLog = -1;
                let mut dCkpt = -1;
                rc = sqlite3BtreeCheckpoint(
                    db,
                    &pBt,
                    eMode,
                    pnLog.as_deref_mut().unwrap_or(&mut dLog),
                    pnCkpt.as_deref_mut().unwrap_or(&mut dCkpt),
                );
            }
            pnLog = None;
            pnCkpt = None;
            if rc == SQLITE_BUSY {
                bBusy = true;
                rc = SQLITE_OK;
            }
        }
        i += 1;
    }

    if rc == SQLITE_OK && bBusy {
        SQLITE_BUSY
    } else {
        rc
    }
}

/// C: `int sqlite3TempInMemory(const sqlite3 *db)`
///
/// This function returns true if main-memory should be used instead of
/// a temporary file for transient pager files and statement journals.
/// The value returned depends on the value of db->temp_store (runtime
/// parameter) and the compile time value of SQLITE_TEMP_STORE (1 in this
/// build: memory only if temp_store==2).
pub fn sqlite3TempInMemory(db: &mut Sqlite3) -> i32 {
    (db.temp_store == 2) as i32
}

/// C: `const char * sqlite3_errmsg(sqlite3 *db)`
///
/// Return UTF-8 encoded English language explanation of the most recent
/// error.
pub fn sqlite3_errmsg(db: &mut Sqlite3) -> Option<ZStr> {
    if crate::util::sqlite3SafetyCheckSickOrOk(db) == 0 {
        return sqlite3ErrStr(SQLITE_MISUSE);
    }
    let z;
    if db.mallocFailed != 0 {
        z = sqlite3ErrStr(SQLITE_NOMEM);
    } else {
        let mut zz: Option<ZStr> = None;
        if db.errCode != 0 {
            if let Some(m) = db.pErr.as_deref_mut() {
                zz = crate::vdbeapi::sqlite3_value_text(m).map(|s| s.to_vec());
            }
        }
        if zz.is_none() {
            zz = sqlite3ErrStr(db.errCode);
        }
        z = zz;
    }
    z
}

/// C: `int sqlite3_set_errmsg(sqlite3 *db, int errcode, const char *zMsg)`
///
/// Set the error code and error message associated with the database
/// handle.
///
/// This routine is intended to be called by outside extensions (ex: the
/// Session extension). Internal logic should invoke sqlite3Error() or
/// sqlite3ErrorWithMsg() directly.
pub fn sqlite3_set_errmsg(db: &mut Sqlite3, errcode: i32, zMsg: Option<&[u8]>) -> i32 {
    let rc = SQLITE_OK;
    if crate::util::sqlite3SafetyCheckOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    if let Some(z) = zMsg {
        sqlite3ErrorWithMsg(db, errcode, Some(b"%s"), &[PfArg::Str(Some(z))]);
    } else {
        sqlite3Error(db, errcode);
    }
    sqlite3ApiExit(db, rc)
}

/// C: `int sqlite3_error_offset(sqlite3 *db)`
///
/// Return the byte offset of the most recent error
pub fn sqlite3_error_offset(db: &mut Sqlite3) -> i32 {
    let mut iOffset = -1;
    if crate::util::sqlite3SafetyCheckSickOrOk(db) != 0 && db.errCode != 0 {
        iOffset = db.errByteOffset;
    }
    iOffset
}

/// C: `const void * sqlite3_errmsg16(sqlite3 *db)`
///
/// Return UTF-16 encoded English language explanation of the most recent
/// error.  Rust: native byte order, with a U+0000 terminator.
pub fn sqlite3_errmsg16(db: &mut Sqlite3) -> Option<Vec<u8>> {
    if crate::util::sqlite3SafetyCheckSickOrOk(db) == 0 {
        return Some(utf8_to_utf16z(b"bad parameter or other API misuse"));
    }
    if db.mallocFailed != 0 {
        return Some(utf8_to_utf16z(b"out of memory"));
    }
    let z = sqlite3_errmsg(db).unwrap_or_default();
    Some(utf8_to_utf16z(&z))
}

/// C: `int sqlite3_errcode(sqlite3 *db)`
///
/// Return the most recent error code generated by an SQLite routine.
pub fn sqlite3_errcode(db: &mut Sqlite3) -> i32 {
    if crate::util::sqlite3SafetyCheckSickOrOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    if db.mallocFailed != 0 {
        SQLITE_NOMEM
    } else {
        db.errCode & db.errMask
    }
}

/// C: `int sqlite3_extended_errcode(sqlite3 *db)`
pub fn sqlite3_extended_errcode(db: &mut Sqlite3) -> i32 {
    if crate::util::sqlite3SafetyCheckSickOrOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    if db.mallocFailed != 0 {
        SQLITE_NOMEM
    } else {
        db.errCode
    }
}

/// C: `int sqlite3_system_errno(sqlite3 *db)`
pub fn sqlite3_system_errno(db: &mut Sqlite3) -> i32 {
    db.iSysErrno
}

/// C: `const char * sqlite3_errstr(int rc)`
///
/// Return a string that describes the kind of error specified in the
/// argument.  For now, this simply calls the internal sqlite3ErrStr()
/// function.
pub fn sqlite3_errstr(rc: i32) -> Option<ZStr> {
    sqlite3ErrStr(rc)
}

/// sqlite3ApiExit() (malloc.c).
///
/// This function must be called before exiting any API function (i.e.
/// returning control to the user) that has called sqlite3_malloc or
/// sqlite3_realloc.
///
/// The returned value is normally a copy of the second argument to this
/// function. However, if a malloc() failure has occurred since the
/// previous invocation SQLITE_NOMEM is returned instead.
pub fn sqlite3ApiExit(db: &mut Sqlite3, rc: i32) -> i32 {
    if db.mallocFailed != 0 || rc == SQLITE_IOERR_NOMEM {
        // apiHandleError()
        db.mallocFailed = 0;
        sqlite3Error(db, SQLITE_NOMEM);
        return SQLITE_NOMEM;
    }
    rc & db.errMask
}

/// sqlite3_soft_heap_limit64() (malloc.c).  Rust: the limit is recorded
/// but memory is not accounted.
pub fn sqlite3_soft_heap_limit64(n: i64) -> i64 {
    let rc = sqlite3_initialize();
    if rc != 0 {
        return -1;
    }
    let priorLimit = mem0_alarmThreshold.load(Ordering::Relaxed);
    if n < 0 {
        return priorLimit;
    }
    let hard = mem0_hardLimit.load(Ordering::Relaxed);
    let mut n = n;
    if hard > 0 && (n > hard || n == 0) {
        n = hard;
    }
    mem0_alarmThreshold.store(n, Ordering::Relaxed);
    priorLimit
}

/// sqlite3_hard_heap_limit64() (malloc.c).  Rust: the limit is recorded
/// but memory is not accounted.
pub fn sqlite3_hard_heap_limit64(n: i64) -> i64 {
    let rc = sqlite3_initialize();
    if rc != 0 {
        return -1;
    }
    let priorLimit = mem0_hardLimit.load(Ordering::Relaxed);
    if n >= 0 {
        mem0_hardLimit.store(n, Ordering::Relaxed);
        let a = mem0_alarmThreshold.load(Ordering::Relaxed);
        if n < a || a == 0 {
            mem0_alarmThreshold.store(n, Ordering::Relaxed);
        }
    }
    priorLimit
}

// ---------------------------------------------------------------------------
// Collations
// ---------------------------------------------------------------------------

/// Create a new collating function for database "db".  The name is zName
/// and the encoding is enc.
fn createCollation(
    db: &mut Sqlite3,
    zName: &[u8],
    enc: u8,
    xCompare: Option<XCmp>,
    xDel: Option<Rc<dyn Fn()>>,
) -> i32 {
    // If SQLITE_UTF16 is specified as the encoding type, transform this
    // to one of SQLITE_UTF16LE or SQLITE_UTF16BE using the
    // SQLITE_UTF16NATIVE macro. SQLITE_UTF16 is not used internally.
    let mut enc2 = enc;
    if enc2 == SQLITE_UTF16 || enc2 == SQLITE_UTF16_ALIGNED {
        enc2 = SQLITE_UTF16NATIVE;
    }
    if enc2 < SQLITE_UTF8 || enc2 > SQLITE_UTF16BE {
        return SQLITE_MISUSE;
    }

    // Check if this call is removing or replacing an existing collation
    // sequence. If so, and there are active VMs, return busy. If there
    // are no active VMs, invalidate any pre-compiled statements.
    let pColl = sqlite3FindCollSeq(db, enc2, Some(zName), 0);
    if let Some(pColl) = pColl {
        if pColl.xCmp.borrow().is_some() {
            if db.nVdbeActive != 0 {
                sqlite3ErrorWithMsg(
                    db,
                    SQLITE_BUSY,
                    Some(b"unable to delete/modify collation sequence due to active statements"),
                    &[],
                );
                return SQLITE_BUSY;
            }
            crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 0);

            // If collation sequence pColl was created directly by a call to
            // sqlite3_create_collation, and not generated by synthCollSeq(),
            // then any copies made by synthCollSeq() need to be invalidated.
            // Also, collation destructor - CollSeq.xDel() - function may need
            // to be called.
            if (pColl.enc.get() & !SQLITE_UTF16_ALIGNED) == enc2 {
                if let Some(aColl) = db.aCollSeq.find(zName).cloned() {
                    for p in aColl.iter() {
                        if p.enc.get() == pColl.enc.get() {
                            let xd = p.xDel.borrow_mut().take();
                            if let Some(x) = xd {
                                x();
                            }
                            *p.xCmp.borrow_mut() = None;
                        }
                    }
                }
            }
        }
    }

    let Some(pColl) = sqlite3FindCollSeq(db, enc2, Some(zName), 1) else {
        return SQLITE_NOMEM;
    };
    *pColl.xCmp.borrow_mut() = xCompare;
    *pColl.xDel.borrow_mut() = xDel;
    // Rust: CollSeq.enc is fixed at creation (the SQLITE_UTF16_ALIGNED bit
    // of C's `pColl->enc = enc2 | (enc & SQLITE_UTF16_ALIGNED)` is not
    // recorded; UTF-16 is not supported).
    sqlite3Error(db, SQLITE_OK);
    SQLITE_OK
}

/// This array defines hard upper bounds on limit values.  The
/// initializer must be kept in sync with the SQLITE_LIMIT_*
/// #defines in sqlite3.h.
const aHardLimit: [i32; SQLITE_N_LIMIT] = [
    SQLITE_MAX_LENGTH,
    SQLITE_MAX_SQL_LENGTH,
    SQLITE_MAX_COLUMN,
    SQLITE_MAX_EXPR_DEPTH,
    SQLITE_MAX_COMPOUND_SELECT,
    SQLITE_MAX_VDBE_OP,
    SQLITE_MAX_FUNCTION_ARG,
    SQLITE_MAX_ATTACHED,
    SQLITE_MAX_LIKE_PATTERN_LENGTH,
    SQLITE_MAX_VARIABLE_NUMBER, // IMP: R-38091-32352
    SQLITE_MAX_TRIGGER_DEPTH,
    SQLITE_MAX_WORKER_THREADS,
    SQLITE_MAX_PARSER_DEPTH,
    SQLITE_MAX_SCHEMA,
    SQLITE_MAX_TRIGGER_STEPS,
];

/// C: `int sqlite3_limit(sqlite3 *db, int limitId, int newLimit)`
///
/// Change the value of a limit.  Report the old value.
/// If an invalid limit index is supplied, report -1.
/// Make no changes but still report the old value if the
/// new limit is negative.
///
/// A new lower limit does not shrink existing constructs.
/// It merely prevents new constructs that exceed the limit
/// from forming.
pub fn sqlite3_limit(db: &mut Sqlite3, limitId: i32, newLimit: i32) -> i32 {
    // EVIDENCE-OF: R-30189-54097 For each limit category SQLITE_LIMIT_NAME
    // there is a hard upper bound set at compile-time by a C preprocessor
    // macro called SQLITE_MAX_NAME. (The "_LIMIT_" in the name is changed to
    // "_MAX_".)
    if limitId < 0 || limitId >= SQLITE_N_LIMIT as i32 {
        return -1;
    }
    let mut newLimit = newLimit;
    let oldLimit = db.aLimit[limitId as usize];
    if newLimit >= 0 {
        // IMP: R-52476-28732
        if newLimit > aHardLimit[limitId as usize] {
            newLimit = aHardLimit[limitId as usize]; // IMP: R-51463-25634
        } else if newLimit < SQLITE_MIN_LENGTH && limitId == SQLITE_LIMIT_LENGTH {
            newLimit = SQLITE_MIN_LENGTH;
        }
        db.aLimit[limitId as usize] = newLimit;
    }
    oldLimit // IMP: R-53341-35419
}

// ---------------------------------------------------------------------------
// URI parsing and opening
// ---------------------------------------------------------------------------

/// C: `int sqlite3ParseUri( const char *zDefaultVfs, const char *zUri, unsigned int *pFlags, sqlite3_vfs **ppVfs, char **pzFile, char **pzErrMsg )`
///
/// This function is used to parse both URIs and non-URI filenames passed
/// by the user to API functions sqlite3_open() or sqlite3_open_v2(), and
/// for database URIs specified as part of ATTACH statements.
///
/// The first argument to this function is the name of the VFS to use (or
/// a NULL to signify the default VFS) if the URI does not contain a
/// "vfs=xxx" query parameter. The second argument contains the URI (or
/// non-URI filename) itself. When this function is called the *pFlags
/// variable should contain the default flags to open the database handle
/// with. The value stored in *pFlags may be updated before returning if
/// the URI filename contains "cache=xxx" or "mode=xxx" query parameters.
///
/// If successful, SQLITE_OK is returned. In this case *ppVfs is set to
/// point to the VFS that should be used to open the database file.
/// *pzFile is set to a buffer containing the name of the file to open
/// (Rust: in the format described at the top of this module).
///
/// If an error occurs, then an SQLite error code is returned and
/// *pzErrMsg may be set to point to a buffer containing an English
/// language error message.
pub fn sqlite3ParseUri(
    zDefaultVfs: Option<&[u8]>,
    zUri: Option<&[u8]>,
    pFlags: &mut u32,
    ppVfs: &mut Option<Rc<Vfs>>,
    pzFile: &mut Option<ZStr>,
    pzErrMsg: &mut Option<ZStr>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut flags = *pFlags;
    let mut zVfs: Option<ZStr> = zDefaultVfs.map(|z| z.to_vec());
    let zUri = zUri.unwrap_or(b"");
    let zUri = &zUri[..cstrlen(zUri)];
    let nUri = zUri.len();
    // zUri[i] with C's NUL terminator.
    let at = |i: usize| -> u8 { zUri.get(i).copied().unwrap_or(0) };
    let mut zFile: Vec<u8>;

    debug_assert!(pzErrMsg.is_none());

    'parse_uri_out: {
        if ((flags & SQLITE_OPEN_URI as u32) != 0 // IMP: R-48725-32206
            || sqlite3GlobalConfig().bOpenUri != 0) // IMP: R-51689-46548
            && nUri >= 5
            && &zUri[..5] == b"file:"
        // IMP: R-57884-37496
        {
            let mut iIn: usize;
            let mut eState: i32; // Parser state when parsing URI

            // Make sure the SQLITE_OPEN_URI flag is set to indicate to the VFS
            // xOpen method that there may be extra parameters following the
            // file-name.
            flags |= SQLITE_OPEN_URI as u32;

            zFile = Vec::with_capacity(nUri + 8);

            iIn = 5;
            // Discard the scheme and authority segments of the URI.
            if at(5) == b'/' && at(6) == b'/' {
                iIn = 7;
                while at(iIn) != 0 && at(iIn) != b'/' {
                    iIn += 1;
                }
                if iIn != 7 && (iIn != 16 || &zUri[7..16] != b"localhost") {
                    *pzErrMsg = Some(crate::printf::sqlite3_mprintf(
                        b"invalid uri authority: %.*s",
                        &[PfArg::Int((iIn - 7) as i64), PfArg::Str(Some(&zUri[7..]))],
                    ));
                    rc = SQLITE_ERROR;
                    break 'parse_uri_out;
                }
            }

            // Copy the filename and any query parameters into the zFile
            // buffer.  Decode %HH escape codes along the way.
            //
            // Within this loop, variable eState may be set to 0, 1 or 2,
            // depending on the parsing context. As follows:
            //
            //   0: Parsing file-name.
            //   1: Parsing name section of a name=value query parameter.
            //   2: Parsing value section of a name=value query parameter.
            eState = 0;
            loop {
                let mut c = at(iIn);
                if c == 0 || c == b'#' {
                    break;
                }
                iIn += 1;
                if c == b'%'
                    && crate::global::sqlite3Isxdigit(at(iIn))
                    && crate::global::sqlite3Isxdigit(at(iIn + 1))
                {
                    let mut octet = (crate::util::sqlite3HexToInt(at(iIn) as i32) as i32) << 4;
                    iIn += 1;
                    octet += crate::util::sqlite3HexToInt(at(iIn) as i32) as i32;
                    iIn += 1;

                    debug_assert!(octet >= 0 && octet < 256);
                    if octet == 0 {
                        // This branch is taken when "%00" appears within the
                        // URI. In this case we ignore all text in the
                        // remainder of the path, name or value currently being
                        // parsed. So ignore the current character and skip to
                        // the next "?", "=" or "&", as appropriate.
                        loop {
                            let c2 = at(iIn);
                            if c2 != 0
                                && c2 != b'#'
                                && (eState != 0 || c2 != b'?')
                                && (eState != 1 || (c2 != b'=' && c2 != b'&'))
                                && (eState != 2 || c2 != b'&')
                            {
                                iIn += 1;
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                    c = octet as u8;
                } else if eState == 1 && (c == b'&' || c == b'=') {
                    if zFile.last().copied().unwrap_or(0) == 0 {
                        // An empty option name. Ignore this option altogether.
                        while at(iIn) != 0 && at(iIn) != b'#' && at(iIn - 1) != b'&' {
                            iIn += 1;
                        }
                        continue;
                    }
                    if c == b'&' {
                        zFile.push(0);
                    } else {
                        eState = 2;
                    }
                    c = 0;
                } else if (eState == 0 && c == b'?') || (eState == 2 && c == b'&') {
                    c = 0;
                    eState = 1;
                }
                zFile.push(c);
            }
            if eState == 1 {
                zFile.push(0);
            }
            // end-of-options + empty journal filenames
            zFile.extend_from_slice(&[0, 0, 0, 0]);

            // Check if there were any options specified that should be
            // interpreted here. Options that are interpreted here include
            // "vfs" and those that correspond to flags that may be passed to
            // the sqlite3_open_v2() method.
            let mut iOpt = cstrlen(&zFile) + 1;
            while iOpt < zFile.len() && zFile[iOpt] != 0 {
                let zOpt = cstr_at(&zFile, iOpt).to_vec();
                let nOpt = zOpt.len();
                let iVal = iOpt + nOpt + 1;
                let zVal = cstr_at(&zFile, iVal).to_vec();
                let nVal = zVal.len();

                if nOpt == 3 && zOpt == b"vfs" {
                    zVfs = Some(zVal.clone());
                } else {
                    let mut aMode: Option<&[(&[u8], i32)]> = None;
                    let mut zModeType: &[u8] = b"";
                    let mut mask = 0i32;
                    let mut limit = 0i32;

                    if nOpt == 5 && zOpt == b"cache" {
                        const aCacheMode: [(&[u8], i32); 2] = [
                            (b"shared", SQLITE_OPEN_SHAREDCACHE),
                            (b"private", SQLITE_OPEN_PRIVATECACHE),
                        ];
                        mask = SQLITE_OPEN_SHAREDCACHE | SQLITE_OPEN_PRIVATECACHE;
                        aMode = Some(&aCacheMode);
                        limit = mask;
                        zModeType = b"cache";
                    }
                    if nOpt == 4 && zOpt == b"mode" {
                        const aOpenMode: [(&[u8], i32); 4] = [
                            (b"ro", SQLITE_OPEN_READONLY),
                            (b"rw", SQLITE_OPEN_READWRITE),
                            (b"rwc", SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE),
                            (b"memory", SQLITE_OPEN_MEMORY),
                        ];
                        mask = SQLITE_OPEN_READONLY
                            | SQLITE_OPEN_READWRITE
                            | SQLITE_OPEN_CREATE
                            | SQLITE_OPEN_MEMORY;
                        aMode = Some(&aOpenMode);
                        limit = mask & flags as i32;
                        zModeType = b"access";
                    }

                    if let Some(aMode) = aMode {
                        let mut mode = 0;
                        for &(z, m) in aMode.iter() {
                            if nVal == z.len() && zVal == z {
                                mode = m;
                                break;
                            }
                        }
                        if mode == 0 {
                            *pzErrMsg = Some(crate::printf::sqlite3_mprintf(
                                b"no such %s mode: %s",
                                &[PfArg::Str(Some(zModeType)), PfArg::Str(Some(&zVal))],
                            ));
                            rc = SQLITE_ERROR;
                            break 'parse_uri_out;
                        }
                        if (mode & !SQLITE_OPEN_MEMORY) > limit {
                            *pzErrMsg = Some(crate::printf::sqlite3_mprintf(
                                b"%s mode not allowed: %s",
                                &[PfArg::Str(Some(zModeType)), PfArg::Str(Some(&zVal))],
                            ));
                            rc = SQLITE_PERM;
                            break 'parse_uri_out;
                        }
                        flags = (flags & !(mask as u32)) | mode as u32;
                    }
                }

                iOpt = iVal + nVal + 1;
            }
        } else {
            zFile = Vec::with_capacity(nUri + 4);
            zFile.extend_from_slice(zUri);
            zFile.extend_from_slice(&[0, 0, 0, 0]);
            flags &= !(SQLITE_OPEN_URI as u32);
        }

        *ppVfs = crate::os::sqlite3_vfs_find(zVfs.as_deref());
        if ppVfs.is_none() {
            *pzErrMsg = Some(crate::printf::sqlite3_mprintf(
                b"no such vfs: %s",
                &[PfArg::Str(zVfs.as_deref())],
            ));
            rc = SQLITE_ERROR;
        }
        *pFlags = flags;
        *pzFile = if rc != SQLITE_OK { None } else { Some(zFile) };
        return rc;
    }
    // parse_uri_out (error):
    *pFlags = flags;
    *pzFile = None;
    rc
}

/// This routine does the core work of extracting URI parameters from a
/// database filename for the sqlite3_uri_parameter() interface.
fn uriParameter(zFilename: &[u8], zParam: &[u8]) -> Option<ZStr> {
    let mut i = cstrlen(zFilename) + 1;
    while i < zFilename.len() && zFilename[i] != 0 {
        let k = cstr_at(zFilename, i);
        let x = k == zParam;
        i += k.len() + 1;
        let v = cstr_at(zFilename, i);
        if x {
            return Some(v.to_vec());
        }
        i += v.len() + 1;
    }
    None
}

/// Rust: allocate and initialize a new connection object (the zeroed
/// sqlite3 of C's sqlite3MallocZero plus the constant initializations of
/// openDatabase()).
fn sqlite3NewConnection(pVfs: Rc<Vfs>, flags: u32) -> Box<Sqlite3> {
    let mkdb = || Db {
        zDbSName: None,
        pBt: None,
        safety_level: 0,
        bSyncSet: 0,
        pSchema: Schema::null(),
    };
    let mut aLimit = aHardLimit;
    aLimit[SQLITE_LIMIT_WORKER_THREADS as usize] = SQLITE_DEFAULT_WORKER_THREADS;
    Box::new(Sqlite3 {
        arena: Arena::new(),
        parses: Vec::new(),
        aVdbe: Vec::new(),
        builtinFuncs: sqlite3BuiltinFunctions(),
        pVfs,
        pDfltColl: None,
        aDb: vec![mkdb(), mkdb()],
        mDbFlags: 0,
        flags: 0,
        lastRowid: 0,
        szMmap: sqlite3GlobalConfig().szMmap,
        nSchemaLock: 0,
        openFlags: 0,
        errCode: 0,
        errByteOffset: 0,
        errMask: if (flags & SQLITE_OPEN_EXRESCODE as u32) != 0 {
            -1
        } else {
            0xff
        },
        iSysErrno: 0,
        dbOptFlags: 0,
        enc: 0,
        autoCommit: 1,
        temp_store: 0,
        mallocFailed: 0,
        bBenignMalloc: 0,
        dfltLockMode: 0,
        nextAutovac: -1,
        suppressErr: 0,
        vtabOnConflict: 0,
        isTransactionSavepoint: 0,
        mTrace: 0,
        noSharedCache: 0,
        nSqlExec: 0,
        eOpenState: SQLITE_STATE_BUSY,
        nFpDigit: 17,
        nextPagesize: 0,
        nChange: 0,
        nTotalChange: 0,
        aLimit,
        nMaxSorterMmap: 0,
        init: sqlite3InitInfo {
            azInit: sqlite3StdType.iter().map(|s| s.to_vec()).collect(),
            ..Default::default()
        },
        nVdbeActive: 0,
        nVdbeRead: 0,
        nVdbeWrite: 0,
        nVdbeExec: 0,
        nVDestroy: 0,
        nExtension: 0,
        xTrace: None,
        xCommitCallback: None,
        xRollbackCallback: None,
        xUpdateCallback: None,
        xAutovacPages: None,
        xCollNeeded: None,
        pErr: None,
        isInterrupted: Rc::new(Cell::new(0)),
        xAuth: None,
        xProgress: None,
        nProgressOps: 0,
        aFunc: Hash::new(),
        aCollSeq: Hash::new(),
        busyHandler: Rc::new(RefCell::new(BusyHandler {
            xBusyHandler: None,
            nBusy: 0,
        })),
        pSavepoint: Vec::new(),
        nAnalysisLimit: 0,
        busyTimeout: 0,
        nSavepoint: 0,
        nStatement: 0,
        nDeferredCons: 0,
        nDeferredImmCons: 0,
        nSpill: 0,
        xTraceLegacy: None,
        xProfile: None,
        xWalCallback: None,
        walAutoCkpt: 0,
        pDbData: Vec::new(),
        aModule: Hash::new(),
        aVTrans: None,
        nVTrans: 0,
        pVtabCtx: Vec::new(),
        pDisconnect: Vec::new(),
    })
}

/// This routine does the work of opening a database on behalf of
/// sqlite3_open() and sqlite3_open16(). The database filename "zFilename"
/// is UTF-8 encoded.
fn openDatabase(
    zFilename: Option<&[u8]>,
    ppDb: &mut Option<Box<Sqlite3>>,
    flags: u32,
    zVfs: Option<&[u8]>,
) -> i32 {
    let mut rc: i32;
    let mut zOpen: Option<ZStr> = None;
    let mut zErrMsg: Option<ZStr> = None;
    let mut flags = flags;

    *ppDb = None;
    rc = sqlite3_initialize();
    if rc != 0 {
        return rc;
    }

    if (flags & SQLITE_OPEN_PRIVATECACHE as u32) != 0 {
        flags &= !(SQLITE_OPEN_SHAREDCACHE as u32);
    } else if crate::global::sqlite3SharedCacheEnabled() != 0 {
        flags |= SQLITE_OPEN_SHAREDCACHE as u32;
    }

    // Remove harmful bits from the flags parameter
    //
    // The SQLITE_OPEN_NOMUTEX and SQLITE_OPEN_FULLMUTEX flags were
    // dealt with in the previous code block.  Besides these, the only
    // valid input flags for sqlite3_open_v2() are SQLITE_OPEN_READONLY,
    // SQLITE_OPEN_READWRITE, SQLITE_OPEN_CREATE, SQLITE_OPEN_SHAREDCACHE,
    // SQLITE_OPEN_PRIVATECACHE, SQLITE_OPEN_EXRESCODE, and some reserved
    // bits.  Silently mask off all other flags.
    flags &= !((SQLITE_OPEN_DELETEONCLOSE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_MAIN_DB
        | SQLITE_OPEN_TEMP_DB
        | SQLITE_OPEN_TRANSIENT_DB
        | SQLITE_OPEN_MAIN_JOURNAL
        | SQLITE_OPEN_TEMP_JOURNAL
        | SQLITE_OPEN_SUBJOURNAL
        | SQLITE_OPEN_SUPER_JOURNAL
        | SQLITE_OPEN_NOMUTEX
        | SQLITE_OPEN_FULLMUTEX
        | SQLITE_OPEN_WAL) as u32);

    // Allocate the sqlite data structure.  Rust: the connection needs a VFS
    // from the start; the default one is used until sqlite3ParseUri()
    // picks the real one.
    let Some(pDfltVfs) = crate::os::sqlite3_vfs_find(None) else {
        return SQLITE_ERROR;
    };
    let mut db = sqlite3NewConnection(pDfltVfs, flags);
    let dbr: &mut Sqlite3 = &mut db;

    dbr.flags |= SQLITE_ShortColNames
        | SQLITE_EnableTrigger
        | SQLITE_EnableView
        | SQLITE_CacheSpill
        | SQLITE_AttachCreate
        | SQLITE_AttachWrite
        | SQLITE_Comments
        | SQLITE_TrustedSchema
        // The SQLITE_DQS compile-time option determines the default settings
        // for SQLITE_DBCONFIG_DQS_DDL and SQLITE_DBCONFIG_DQS_DML (3: both on).
        | SQLITE_DqsDML
        | SQLITE_DqsDDL
        | SQLITE_AutoIndex;

    'opendb_out: {
        // Add the default collation sequence BINARY. BINARY works for both
        // UTF-8 and UTF-16, so add a version for each to avoid any
        // unnecessary conversions. The only error that can occur here is a
        // malloc() failure.
        //
        // EVIDENCE-OF: R-52786-44878 SQLite defines three built-in collating
        // functions:
        let bin = binaryCmp.with(|b| b.clone());
        createCollation(dbr, sqlite3StrBINARY, SQLITE_UTF8, Some(bin.clone()), None);
        createCollation(
            dbr,
            sqlite3StrBINARY,
            SQLITE_UTF16BE,
            Some(bin.clone()),
            None,
        );
        createCollation(dbr, sqlite3StrBINARY, SQLITE_UTF16LE, Some(bin), None);
        createCollation(
            dbr,
            b"NOCASE",
            SQLITE_UTF8,
            Some(Rc::new(nocaseCollatingFunc)),
            None,
        );
        createCollation(
            dbr,
            b"RTRIM",
            SQLITE_UTF8,
            Some(Rc::new(rtrimCollFunc)),
            None,
        );
        if dbr.mallocFailed != 0 {
            break 'opendb_out;
        }

        // Parse the filename/URI argument
        //
        // Only allow sensible combinations of bits in the flags argument.
        // Throw an error if any non-sense combination is used.  If we
        // do not block illegal combinations here, it could trigger
        // assert() statements in deeper layers.  Sensible combinations
        // are:
        //
        //  1:  SQLITE_OPEN_READONLY
        //  2:  SQLITE_OPEN_READWRITE
        //  6:  SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE
        dbr.openFlags = flags;
        let mut pVfs: Option<Rc<Vfs>> = None;
        if ((1u32 << (flags & 7)) & 0x46) == 0 {
            rc = SQLITE_MISUSE; // IMP: R-18321-05872
        } else {
            let zFilename: &[u8] = zFilename.unwrap_or(b":memory:");
            rc = sqlite3ParseUri(
                zVfs,
                Some(zFilename),
                &mut flags,
                &mut pVfs,
                &mut zOpen,
                &mut zErrMsg,
            );
        }
        if rc != SQLITE_OK {
            match &zErrMsg {
                Some(m) => sqlite3ErrorWithMsg(dbr, rc, Some(b"%s"), &[PfArg::Str(Some(m))]),
                None => sqlite3ErrorWithMsg(dbr, rc, None, &[]),
            }
            break 'opendb_out;
        }
        let pVfs = pVfs.expect("vfs");
        dbr.pVfs = pVfs.clone();

        // Open the backend database driver
        let mut pBt: Option<BtreeP> = None;
        rc = sqlite3BtreeOpen(
            dbr,
            &pVfs,
            zOpen.as_deref(),
            &mut pBt,
            0,
            (flags | SQLITE_OPEN_MAIN_DB as u32) as i32,
        );
        if rc != SQLITE_OK {
            if rc == SQLITE_IOERR_NOMEM {
                rc = SQLITE_NOMEM;
            }
            sqlite3Error(dbr, rc);
            break 'opendb_out;
        }
        dbr.aDb[0].pBt = pBt.clone();
        dbr.aDb[0].pSchema = crate::callback::sqlite3SchemaGet(dbr, pBt.as_ref());
        if dbr.mallocFailed == 0 {
            let enc = dbr.aDb[0].pSchema.borrow().enc; // SCHEMA_ENC(db)
            crate::callback::sqlite3SetTextEncoding(dbr, enc);
        }
        dbr.aDb[1].pSchema = crate::callback::sqlite3SchemaGet(dbr, None);

        // The default safety_level for the main database is FULL; for the temp
        // database it is OFF. This matches the pager layer defaults.
        dbr.aDb[0].zDbSName = Some(b"main".to_vec());
        dbr.aDb[0].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;
        dbr.aDb[1].zDbSName = Some(b"temp".to_vec());
        dbr.aDb[1].safety_level = PAGER_SYNCHRONOUS_OFF as u8;

        dbr.eOpenState = SQLITE_STATE_OPEN;
        if dbr.mallocFailed != 0 {
            break 'opendb_out;
        }

        // Register all built-in functions, but do not attempt to read the
        // database schema yet. This is delayed until the first time the
        // database is accessed.
        sqlite3Error(dbr, SQLITE_OK);
        crate::func::sqlite3RegisterPerConnectionBuiltinFunctions(dbr);
        rc = sqlite3_errcode(dbr);

        // Load compiled-in extensions: none in this build.

        // Load automatic extensions - extensions that have been registered
        // using the sqlite3_automatic_extension() API.
        if rc == SQLITE_OK {
            crate::loadext::sqlite3AutoLoadExtensions(dbr);
            rc = sqlite3_errcode(dbr);
            if rc != SQLITE_OK {
                break 'opendb_out;
            }
        }

        if rc != 0 {
            sqlite3Error(dbr, rc);
        }

        // Enable the lookaside-malloc subsystem
        setupLookaside(
            dbr,
            sqlite3GlobalConfig().szLookaside,
            sqlite3GlobalConfig().nLookaside,
        );

        sqlite3_wal_autocheckpoint(dbr, SQLITE_DEFAULT_WAL_AUTOCHECKPOINT);
    }

    // opendb_out:
    rc = sqlite3_errcode(dbr);
    if (rc & 0xff) == SQLITE_NOMEM {
        sqlite3_close(dbr);
        return rc;
    } else if rc != SQLITE_OK {
        dbr.eOpenState = SQLITE_STATE_SICK;
    }
    *ppDb = Some(db);
    rc
}

/// C: `int sqlite3_open( const char *zFilename, sqlite3 **ppDb )`
///
/// Open a new database handle.
pub fn sqlite3_open(zFilename: Option<&[u8]>, ppDb: &mut Option<Box<Sqlite3>>) -> i32 {
    openDatabase(
        zFilename,
        ppDb,
        (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32,
        None,
    )
}

/// C: `int sqlite3_open_v2( const char *filename, sqlite3 **ppDb, int flags, const char *zVfs )`
pub fn sqlite3_open_v2(
    filename: Option<&[u8]>,
    ppDb: &mut Option<Box<Sqlite3>>,
    flags: i32,
    zVfs: Option<&[u8]>,
) -> i32 {
    openDatabase(filename, ppDb, flags as u32, zVfs)
}

/// C: `int sqlite3_open16( const void *zFilename, sqlite3 **ppDb )`
///
/// Open a new database handle.  Rust: zFilename is native-endian UTF-16.
pub fn sqlite3_open16(zFilename: &[u8], ppDb: &mut Option<Box<Sqlite3>>) -> i32 {
    *ppDb = None;
    let rc = sqlite3_initialize();
    if rc != 0 {
        return rc;
    }
    let zFilename8 = utf16z_to_utf8(zFilename);
    let rc = openDatabase(
        Some(&zFilename8),
        ppDb,
        (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32,
        None,
    );
    if let Some(db) = ppDb.as_deref_mut() {
        if rc == SQLITE_OK && !db.DbHasProperty(0, DB_SchemaLoaded) {
            db.aDb[0].pSchema.borrow_mut().enc = SQLITE_UTF16NATIVE;
            db.enc = SQLITE_UTF16NATIVE;
        }
    }
    rc & 0xff
}

/// C: `int sqlite3_create_collation( sqlite3* db, const char *zName, int enc, void* pCtx, int(*xCompare)(void*,int,const void*,int,const void*) )`
///
/// Register a new collation sequence with the database handle db.
pub fn sqlite3_create_collation(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    enc: i32,
    xCompare: Option<XCmp>,
) -> i32 {
    sqlite3_create_collation_v2(db, zName, enc, xCompare, None)
}

/// C: `int sqlite3_create_collation_v2( sqlite3* db, const char *zName, int enc, void* pCtx, int(*xCompare)(void*,int,const void*,int,const void*), void(*xDel)(void*) )`
///
/// Register a new collation sequence with the database handle db.
pub fn sqlite3_create_collation_v2(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    enc: i32,
    xCompare: Option<XCmp>,
    xDel: Option<Rc<dyn Fn()>>,
) -> i32 {
    let Some(zName) = zName else {
        return SQLITE_MISUSE;
    };
    let zName = &zName[..cstrlen(zName)];
    let mut rc = createCollation(db, zName, enc as u8, xCompare, xDel);
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_create_collation16( sqlite3* db, const void *zName, int enc, void* pCtx, int(*xCompare)(void*,int,const void*,int,const void*) )`
///
/// Register a new collation sequence with the database handle db.
/// Rust: zName is native-endian UTF-16.
pub fn sqlite3_create_collation16(
    db: &mut Sqlite3,
    zName: &[u8],
    enc: i32,
    xCompare: Option<XCmp>,
) -> i32 {
    let zName8 = utf16z_to_utf8(zName);
    let mut rc = createCollation(db, &zName8, enc as u8, xCompare, None);
    rc = sqlite3ApiExit(db, rc);
    rc
}

/// C: `int sqlite3_collation_needed( sqlite3 *db, void *pCollNeededArg, void(*xCollNeeded)(void*,sqlite3*,int eTextRep,const char*) )`
///
/// Register a collation sequence factory callback with the database
/// handle db. Replace any previously installed collation sequence factory.
pub fn sqlite3_collation_needed(
    db: &mut Sqlite3,
    xCollNeeded: Option<Rc<dyn Fn(&mut Sqlite3, i32, &[u8])>>,
) -> i32 {
    db.xCollNeeded = xCollNeeded;
    SQLITE_OK
}

/// C: `int sqlite3_collation_needed16( sqlite3 *db, void *pCollNeededArg, void(*xCollNeeded16)(void*,sqlite3*,int eTextRep,const void*) )`
///
/// Rust: the callback receives the collation name as native-endian UTF-16
/// (with a U+0000 terminator).
pub fn sqlite3_collation_needed16(
    db: &mut Sqlite3,
    xCollNeeded16: Option<Rc<dyn Fn(&mut Sqlite3, i32, &[u8])>>,
) -> i32 {
    db.xCollNeeded = xCollNeeded16.map(|x| {
        let f: Rc<dyn Fn(&mut Sqlite3, i32, &[u8])> =
            Rc::new(move |db: &mut Sqlite3, enc: i32, z: &[u8]| x(db, enc, &utf8_to_utf16z(z)));
        f
    });
    SQLITE_OK
}

/// C: `void * sqlite3_get_clientdata(sqlite3 *db, const char *zName)`
///
/// Find existing client data.
pub fn sqlite3_get_clientdata(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
) -> Option<Rc<dyn std::any::Any>> {
    let zName = zName?;
    for p in db.pDbData.iter() {
        if p.zName == zName {
            return Some(p.pData.clone());
        }
    }
    None
}

/// C: `int sqlite3_set_clientdata( sqlite3 *db, const char *zName, void *pData, void (*xDestructor)(void*) )`
///
/// Add new client data to a database connection.  Rust: None removes the
/// entry; the destructor is the Drop of the data.
pub fn sqlite3_set_clientdata(
    db: &mut Sqlite3,
    zName: Option<&[u8]>,
    pData: Option<Rc<dyn std::any::Any>>,
) -> i32 {
    let zName = zName.unwrap_or(b"");
    let pos = db.pDbData.iter().position(|p| p.zName == zName);
    match (pos, pData) {
        (Some(i), None) => {
            db.pDbData.remove(i);
        }
        (Some(i), Some(d)) => {
            db.pDbData[i].pData = d;
        }
        (None, None) => {}
        (None, Some(d)) => {
            db.pDbData.insert(
                0,
                DbClientData {
                    zName: zName.to_vec(),
                    pData: d,
                },
            );
        }
    }
    SQLITE_OK
}

/// C: `int sqlite3_global_recover(void)`
///
/// This function is now an anachronism. It used to be used to recover from
/// a malloc() failure, but SQLite now does this automatically.
pub fn sqlite3_global_recover() -> i32 {
    SQLITE_OK
}

/// C: `int sqlite3_get_autocommit(sqlite3 *db)`
///
/// Test to see whether or not the database connection is in autocommit
/// mode.  Return TRUE if it is and FALSE if not.  Autocommit mode is on
/// by default.  Autocommit is disabled by a BEGIN statement and reenabled
/// by the next COMMIT or ROLLBACK.
pub fn sqlite3_get_autocommit(db: &mut Sqlite3) -> i32 {
    db.autoCommit as i32
}

/// C: `int sqlite3ReportError(int iErr, int lineno, const char *zType)`
///
/// The following routines are substitutes for constants SQLITE_CORRUPT,
/// SQLITE_MISUSE, SQLITE_CANTOPEN, SQLITE_NOMEM and possibly other error
/// constants.  They serve two purposes:
///
///   1.  Serve as a convenient place to set a breakpoint in a debugger
///       to detect when version error conditions occurs.
///
///   2.  Invoke sqlite3_log() to provide the source code location where
///       a low-level error is first detected.
pub fn sqlite3ReportError(iErr: i32, lineno: i32, zType: Option<&[u8]>) -> i32 {
    let src = crate::SQLITE_SOURCE_ID.as_bytes();
    crate::printf::sqlite3_log(
        iErr,
        b"%s at line %d of [%.10s]",
        &[
            PfArg::Str(zType),
            PfArg::Int(lineno as i64),
            PfArg::Str(Some(&src[20.min(src.len())..])),
        ],
    );
    iErr
}

/// C: `int sqlite3CorruptError(int lineno)`
pub fn sqlite3CorruptError(lineno: i32) -> i32 {
    sqlite3ReportError(SQLITE_CORRUPT, lineno, Some(b"database corruption"))
}

/// C: `int sqlite3MisuseError(int lineno)`
pub fn sqlite3MisuseError(lineno: i32) -> i32 {
    sqlite3ReportError(SQLITE_MISUSE, lineno, Some(b"misuse"))
}

/// C: `int sqlite3CantopenError(int lineno)`
pub fn sqlite3CantopenError(lineno: i32) -> i32 {
    sqlite3ReportError(SQLITE_CANTOPEN, lineno, Some(b"cannot open file"))
}

/// C: `int sqlite3CorruptPgnoError(int lineno, Pgno pgno)`
pub fn sqlite3CorruptPgnoError(lineno: i32, pgno: Pgno) -> i32 {
    let zMsg = crate::printf::sqlite3_snprintf(
        100,
        b"database corruption page %d",
        &[PfArg::Int(pgno as i64)],
    );
    sqlite3ReportError(SQLITE_CORRUPT, lineno, Some(&zMsg))
}

/// C: `int sqlite3NomemError(int lineno)`
pub fn sqlite3NomemError(lineno: i32) -> i32 {
    sqlite3ReportError(SQLITE_NOMEM, lineno, Some(b"OOM"))
}

/// C: `int sqlite3IoerrnomemError(int lineno)`
pub fn sqlite3IoerrnomemError(lineno: i32) -> i32 {
    sqlite3ReportError(SQLITE_IOERR_NOMEM, lineno, Some(b"I/O OOM error"))
}

/// C: `void sqlite3_thread_cleanup(void)`
///
/// This is a convenience routine that makes sure that all thread-specific
/// data for this thread has been deallocated.
///
/// SQLite no longer uses thread-specific data so this routine is now a
/// no-op.  It is retained for historical compatibility.
pub fn sqlite3_thread_cleanup() {}

/// C: `int sqlite3_table_column_metadata( sqlite3 *db, const char *zDbName, const char *zTableName, const char *zColumnName, char const **pzDataType, char const **pzCollSeq, int *pNotNull, int *pPrimaryKey, int *pAutoinc )`
///
/// Return meta information about a specific column of a database table.
/// See comment in sqlite3.h (sqlite.h.in) for details.
#[allow(clippy::too_many_arguments)]
pub fn sqlite3_table_column_metadata(
    db: &mut Sqlite3,
    zDbName: Option<&[u8]>,
    zTableName: Option<&[u8]>,
    zColumnName: Option<&[u8]>,
    pzDataType: &mut Option<ZStr>,
    pzCollSeq: &mut Option<ZStr>,
    pNotNull: &mut i32,
    pPrimaryKey: &mut i32,
    pAutoinc: &mut i32,
) -> i32 {
    let mut rc;
    let mut zErrMsg: Option<ZStr> = None;
    let mut pTab: Option<TableRef> = None;
    let mut pCol: Option<Column> = None;
    let mut iCol = 0;
    let mut zDataType: Option<ZStr> = None;
    let mut zCollSeq: Option<ZStr> = None;
    let mut notnull = 0;
    let mut primarykey = 0;
    let mut autoinc = 0;

    if crate::util::sqlite3SafetyCheckOk(db) == 0 || zTableName.is_none() {
        return SQLITE_MISUSE;
    }

    // Ensure the database schema has been loaded
    'error_out: {
        rc = crate::prepare::sqlite3Init(db, &mut zErrMsg);
        if SQLITE_OK != rc {
            break 'error_out;
        }

        // Locate the table in question
        pTab = sqlite3FindTable(db, zTableName, zDbName);
        let Some(tab) = pTab.clone() else {
            break 'error_out;
        };
        if tab.borrow().IsView() {
            pTab = None;
            break 'error_out;
        }

        // Find the column for which info is requested
        if let Some(zColumnName) = zColumnName {
            iCol = crate::select::sqlite3ColumnIndex(&tab, Some(zColumnName));
            if iCol >= 0 {
                pCol = Some(tab.borrow().aCol[iCol as usize].clone());
            } else {
                let (hasRowid, iPKey) = {
                    let t = tab.borrow();
                    (t.HasRowid(), t.iPKey)
                };
                if hasRowid && crate::expr::sqlite3IsRowid(Some(zColumnName)) != 0 {
                    iCol = iPKey as i32;
                    pCol = if iCol >= 0 {
                        Some(tab.borrow().aCol[iCol as usize].clone())
                    } else {
                        None
                    };
                } else {
                    pTab = None;
                    break 'error_out;
                }
            }
        } else {
            // Query for existence of table only
        }

        // The following block stores the meta information that will be
        // returned to the caller in local variables zDataType, zCollSeq,
        // notnull, primarykey and autoinc. At this point there are two
        // possibilities:
        //
        //     1. The specified column name was rowid", "oid" or "_rowid_"
        //        and there is no explicitly declared IPK column.
        //
        //     2. The table is not a view and the column name identified an
        //        explicitly declared column. Copy meta information from *pCol.
        if let Some(mut col) = pCol.take() {
            zDataType = crate::util::sqlite3ColumnType(&mut col, None);
            zCollSeq = crate::build::sqlite3ColumnColl(&mut col);
            notnull = (col.notNull != 0) as i32;
            primarykey = ((col.colFlags & COLFLAG_PRIMKEY) != 0) as i32;
            let t = tab.borrow();
            autoinc = (t.iPKey as i32 == iCol && (t.tabFlags & TF_Autoincrement) != 0) as i32;
        } else {
            zDataType = Some(b"INTEGER".to_vec());
            primarykey = 1;
        }
        if zCollSeq.is_none() {
            zCollSeq = Some(sqlite3StrBINARY.to_vec());
        }
    }

    // error_out:
    // Whether the function call succeeded or failed, set the output
    // parameters to whatever their local counterparts contain. If an error
    // did occur, this has the effect of zeroing all output parameters.
    *pzDataType = zDataType;
    *pzCollSeq = zCollSeq;
    *pNotNull = notnull;
    *pPrimaryKey = primarykey;
    *pAutoinc = autoinc;

    if SQLITE_OK == rc && pTab.is_none() {
        zErrMsg = Some(mprintf!(
            db,
            b"no such table column: %s.%s",
            zTableName,
            zColumnName
        ));
        rc = SQLITE_ERROR;
    }
    match &zErrMsg {
        Some(m) => sqlite3ErrorWithMsg(db, rc, Some(b"%s"), &[PfArg::Str(Some(m))]),
        None => sqlite3ErrorWithMsg(db, rc, None, &[]),
    }
    sqlite3ApiExit(db, rc)
}

/// C: `int sqlite3_sleep(int ms)`
///
/// Sleep for a little while.  Return the amount of time slept.
pub fn sqlite3_sleep(ms: i32) -> i32 {
    let Some(pVfs) = crate::os::sqlite3_vfs_find(None) else {
        return 0;
    };

    // This function works in milliseconds, but the underlying OsSleep()
    // API uses microseconds. Hence the 1000's.
    crate::os::sqlite3OsSleep(&pVfs, if ms < 0 { 0 } else { 1000 * ms }) / 1000
}

/// C: `int sqlite3_extended_result_codes(sqlite3 *db, int onoff)`
///
/// Enable or disable the extended result codes.
pub fn sqlite3_extended_result_codes(db: &mut Sqlite3, onoff: i32) -> i32 {
    db.errMask = if onoff != 0 { -1 } else { 0xff };
    SQLITE_OK
}

/// C: `int sqlite3_file_control(sqlite3 *db, const char *zDbName, int op, void *pArg)`
///
/// Invoke the xFileControl method on a particular database.
///
/// Rust: pArg is the typed file-control argument (os::FcntlArg; C void*).
/// SQLITE_FCNTL_FILE_POINTER, _VFS_POINTER and _JOURNAL_POINTER cannot
/// return pointers and fail with SQLITE_NOTFOUND.
pub fn sqlite3_file_control(
    db: &mut Sqlite3,
    zDbName: Option<&[u8]>,
    op: i32,
    pArg: crate::os::FcntlArg,
) -> i32 {
    use crate::os::FcntlArg;
    let mut rc = SQLITE_ERROR;
    let pBtree = sqlite3DbNameToBtree(db, zDbName);
    if let Some(pBtree) = pBtree {
        let pPager = sqlite3BtreePager(&pBtree);
        if op == SQLITE_FCNTL_FILE_POINTER
            || op == SQLITE_FCNTL_VFS_POINTER
            || op == SQLITE_FCNTL_JOURNAL_POINTER
        {
            rc = SQLITE_NOTFOUND;
        } else if op == SQLITE_FCNTL_DATA_VERSION {
            let v = crate::pager::sqlite3PagerDataVersion(&mut pPager.borrow_mut());
            if let FcntlArg::Int(p) = pArg {
                *p = v as i32;
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_RESERVE_BYTES {
            let cur = sqlite3BtreeGetRequestedReserve(&pBtree);
            if let FcntlArg::Int(p) = pArg {
                let iNew = *p;
                *p = cur;
                if iNew >= 0 && iNew <= 255 {
                    sqlite3BtreeSetPageSize(&pBtree, 0, iNew, 0);
                }
            }
            rc = SQLITE_OK;
        } else if op == SQLITE_FCNTL_RESET_CACHE {
            sqlite3BtreeClearCache(&pBtree);
            rc = SQLITE_OK;
        } else {
            let nSave = db.busyHandler.borrow().nBusy;
            {
                let mut pg = pPager.borrow_mut();
                let fd = crate::pager::sqlite3PagerFile(&mut pg);
                rc = crate::os::sqlite3OsFileControl(fd, op, pArg);
            }
            db.busyHandler.borrow_mut().nBusy = nSave;
        }
    }
    rc
}

/// C: `int sqlite3_test_control(int op, ...)`
///
/// Interface to the testing logic.  Rust: only the operations listed here
/// are implemented; the others return 0.
///
/// SQLITE_TESTCTRL_PRNG_SEED: args = [Int(x)] (C db==0), or
/// [Int(x), Int(1)] to use the `db` argument (C db!=0).
pub fn sqlite3_test_control(db: &mut Sqlite3, op: i32, args: &[crate::printf::PfArg]) -> i32 {
    use crate::printf::PfArg;
    let rc = 0;
    if op == SQLITE_TESTCTRL_PRNG_SEED {
        let mut x = match args.first() {
            Some(PfArg::Int(v)) => *v as i32,
            _ => 0,
        };
        if let Some(PfArg::Int(1)) = args.get(1) {
            let y = db.aDb[0].pSchema.borrow().schema_cookie;
            if y != 0 {
                x = y;
            }
        }
        crate::random::iPrngSeed.store(x as u32, std::sync::atomic::Ordering::Relaxed);
        crate::random::sqlite3_randomness(0, &mut []);
    }
    rc
}

/// C: `const char * sqlite3_create_filename( const char *zDatabase, const char *zJournal, const char *zWal, int nParam, const char **azParam )`
///
/// Allocate memory to hold names for a database, journal file, WAL file,
/// and query parameters.  The value returned is valid for use by
/// sqlite3_filename_database() and sqlite3_uri_parameter() and related
/// functions.  Rust: azParam holds the 2*nParam key/value strings.
pub fn sqlite3_create_filename(
    zDatabase: Option<&[u8]>,
    zJournal: Option<&[u8]>,
    zWal: Option<&[u8]>,
    nParam: i32,
    azParam: &[&[u8]],
) -> Option<ZStr> {
    let mut p: Vec<u8> = Vec::new();
    let appendText = |p: &mut Vec<u8>, z: &[u8]| {
        p.extend_from_slice(&z[..cstrlen(z)]);
        p.push(0);
    };
    appendText(&mut p, zDatabase.unwrap_or(b""));
    for i in 0..(nParam.max(0) as usize * 2) {
        appendText(&mut p, azParam.get(i).copied().unwrap_or(b""));
    }
    p.push(0);
    appendText(&mut p, zJournal.unwrap_or(b""));
    appendText(&mut p, zWal.unwrap_or(b""));
    p.push(0);
    p.push(0);
    Some(p)
}

/// C: `void sqlite3_free_filename(const char *p)`
///
/// Free memory obtained from sqlite3_create_filename().  Rust: no-op.
pub fn sqlite3_free_filename(p: Option<&[u8]>) {
    let _ = p;
}

/// C: `const char * sqlite3_uri_parameter(const char *zFilename, const char *zParam)`
///
/// This is a utility routine, useful to VFS implementations, that checks
/// to see if a database file was a URI that contained a specific query
/// parameter, and if so obtains the value of the query parameter.
///
/// The zFilename argument is the filename passed into the xOpen() method
/// of a VFS implementation.  The zParam argument is the name of the query
/// parameter we seek.  This routine returns the value of the zParam
/// parameter if it exists.  If the parameter does not exist, this routine
/// returns a NULL pointer.
pub fn sqlite3_uri_parameter(zFilename: Option<&[u8]>, zParam: Option<&[u8]>) -> Option<ZStr> {
    let zFilename = zFilename?;
    let zParam = zParam?;
    uriParameter(zFilename, &zParam[..cstrlen(zParam)])
}

/// C: `const char * sqlite3_uri_key(const char *zFilename, int N)`
///
/// Return a pointer to the name of Nth query parameter of the filename.
pub fn sqlite3_uri_key(zFilename: Option<&[u8]>, N: i32) -> Option<ZStr> {
    let zFilename = zFilename?;
    if N < 0 {
        return None;
    }
    let mut N = N;
    let mut i = cstrlen(zFilename) + 1;
    while i < zFilename.len() && zFilename[i] != 0 && N > 0 {
        N -= 1;
        i += cstr_at(zFilename, i).len() + 1;
        i += cstr_at(zFilename, i).len() + 1;
    }
    let k = cstr_at(zFilename, i);
    if k.is_empty() { None } else { Some(k.to_vec()) }
}

/// C: `int sqlite3_uri_boolean(const char *zFilename, const char *zParam, int bDflt)`
///
/// Return a boolean value for a query parameter.
pub fn sqlite3_uri_boolean(zFilename: Option<&[u8]>, zParam: Option<&[u8]>, bDflt: i32) -> i32 {
    let z = sqlite3_uri_parameter(zFilename, zParam);
    let bDflt = (bDflt != 0) as u8;
    match z {
        Some(z) => crate::pragma::sqlite3GetBoolean(Some(&z), bDflt) as i32,
        None => bDflt as i32,
    }
}

/// C: `sqlite3_int64 sqlite3_uri_int64( const char *zFilename, const char *zParam, sqlite3_int64 bDflt )`
///
/// Return a 64-bit integer value for a query parameter.
pub fn sqlite3_uri_int64(zFilename: Option<&[u8]>, zParam: Option<&[u8]>, bDflt: i64) -> i64 {
    let z = sqlite3_uri_parameter(zFilename, zParam);
    let mut v: i64 = 0;
    if let Some(z) = z {
        if crate::util::sqlite3DecOrHexToI64(Some(&z), &mut v) == 0 {
            return v;
        }
    }
    bDflt
}

/// C: `const char * sqlite3_filename_database(const char *zFilename)`
///
/// Translate a filename that was handed to a VFS routine into the
/// corresponding database, journal, or WAL file.
pub fn sqlite3_filename_database(zFilename: Option<&[u8]>) -> Option<ZStr> {
    let z = zFilename?;
    Some(cstr_at(z, 0).to_vec())
}

/// C: `const char * sqlite3_filename_journal(const char *zFilename)`
pub fn sqlite3_filename_journal(zFilename: Option<&[u8]>) -> Option<ZStr> {
    let z = zFilename?;
    Some(cstr_at(z, journalOffset(z)).to_vec())
}

/// Offset of the journal name in a filename buffer.
fn journalOffset(z: &[u8]) -> usize {
    let mut i = cstrlen(z) + 1;
    while i < z.len() && z[i] != 0 {
        i += cstr_at(z, i).len() + 1;
        i += cstr_at(z, i).len() + 1;
    }
    i + 1
}

/// C: `const char * sqlite3_filename_wal(const char *zFilename)`
pub fn sqlite3_filename_wal(zFilename: Option<&[u8]>) -> Option<ZStr> {
    let z = zFilename?;
    let j = journalOffset(z);
    let w = j + cstr_at(z, j).len() + 1;
    Some(cstr_at(z, w).to_vec())
}

/// C: `Btree * sqlite3DbNameToBtree(sqlite3 *db, const char *zDbName)`
///
/// Return the Btree pointer identified by zDbName.  Return NULL if not
/// found.
pub fn sqlite3DbNameToBtree(db: &mut Sqlite3, zDbName: Option<&[u8]>) -> Option<BtreeP> {
    let iDb = if zDbName.is_some() {
        sqlite3FindDbName(db, zDbName)
    } else {
        0
    };
    if iDb < 0 {
        None
    } else {
        db.aDb[iDb as usize].pBt.clone()
    }
}

/// C: `const char * sqlite3_db_name(sqlite3 *db, int N)`
///
/// Return the name of the N-th database schema.  Return NULL if N is out
/// of range.
pub fn sqlite3_db_name(db: &mut Sqlite3, N: i32) -> Option<ZStr> {
    if N >= 0 && N < db.nDb() {
        db.aDb[N as usize].zDbSName.clone()
    } else {
        None
    }
}

/// C: `const char * sqlite3_db_filename(sqlite3 *db, const char *zDbName)`
///
/// Return the filename of the database associated with a database
/// connection.
pub fn sqlite3_db_filename(db: &mut Sqlite3, zDbName: Option<&[u8]>) -> Option<ZStr> {
    let pBt = sqlite3DbNameToBtree(db, zDbName)?;
    sqlite3BtreeGetFilename(&pBt)
}

/// C: `int sqlite3_db_readonly(sqlite3 *db, const char *zDbName)`
///
/// Return 1 if database is read-only or 0 if read/write.  Return -1 if
/// no such database exists.
pub fn sqlite3_db_readonly(db: &mut Sqlite3, zDbName: Option<&[u8]>) -> i32 {
    match sqlite3DbNameToBtree(db, zDbName) {
        Some(pBt) => sqlite3BtreeIsReadonly(&pBt),
        None => -1,
    }
}

/// C: `int sqlite3_compileoption_used(const char *zOptName)`
///
/// Given the name of a compile-time option, return true if that option
/// was used and false if not.
///
/// The name can optionally begin with "SQLITE_" but the "SQLITE_" prefix
/// is not required for a match.
pub fn sqlite3_compileoption_used(zOptName: Option<&[u8]>) -> i32 {
    let Some(mut zOptName) = zOptName else {
        return 0;
    };
    zOptName = &zOptName[..cstrlen(zOptName)];
    if zOptName.len() >= 7 && zOptName[..7].eq_ignore_ascii_case(b"SQLITE_") {
        zOptName = &zOptName[7..];
    }
    let n = zOptName.len();

    // Since nOpt is normally in single digits, a linear search is
    // adequate. No need for a binary search.
    for opt in azCompileOpt.iter() {
        if crate::util::sqlite3_strnicmp(Some(zOptName), Some(opt), n as i32) == 0
            && crate::tokenize::sqlite3IsIdChar(opt.get(n).copied().unwrap_or(0)) == 0
        {
            return 1;
        }
    }
    0
}

/// C: `const char * sqlite3_compileoption_get(int N)`
///
/// Return the N-th compile-time option string.  If N is out of range,
/// return a NULL pointer.
pub fn sqlite3_compileoption_get(N: i32) -> Option<ZStr> {
    if N >= 0 && (N as usize) < azCompileOpt.len() {
        Some(azCompileOpt[N as usize].to_vec())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errstr() {
        assert_eq!(sqlite3ErrStr(SQLITE_OK).unwrap(), b"not an error");
        assert_eq!(
            sqlite3ErrStr(SQLITE_CANTOPEN).unwrap(),
            b"unable to open database file"
        );
        assert_eq!(
            sqlite3ErrStr(SQLITE_IOERR_NOMEM).unwrap(),
            b"disk I/O error"
        );
        assert_eq!(sqlite3ErrStr(SQLITE_INTERNAL).unwrap(), b"unknown error");
        assert_eq!(sqlite3ErrStr(SQLITE_ROW).unwrap(), b"another row available");
        assert_eq!(
            sqlite3ErrStr(SQLITE_ABORT_ROLLBACK).unwrap(),
            b"abort due to ROLLBACK"
        );
        assert_eq!(
            sqlite3ErrName(SQLITE_IOERR_READ).unwrap(),
            b"SQLITE_IOERR_READ"
        );
        assert_eq!(sqlite3ErrName(0x1234).unwrap(), b"SQLITE_UNKNOWN(4660)");
    }

    #[test]
    fn collations() {
        assert!(sqlite3BinaryCompare(b"abc", b"abd") < 0);
        assert!(sqlite3BinaryCompare(b"abc", b"ab") > 0);
        assert_eq!(sqlite3BinaryCompare(b"abc", b"abc"), 0);
        assert_eq!(rtrimCollFunc(b"abc  ", b"abc"), 0);
        assert!(rtrimCollFunc(b"abc  x", b"abc") > 0);
    }

    #[test]
    fn filenames() {
        let f = sqlite3_create_filename(
            Some(b"/x/db"),
            Some(b"/x/db-journal"),
            Some(b"/x/db-wal"),
            2,
            &[b"mode", b"ro", b"cache", b"private"],
        )
        .unwrap();
        assert_eq!(sqlite3_filename_database(Some(&f)).unwrap(), b"/x/db");
        assert_eq!(
            sqlite3_filename_journal(Some(&f)).unwrap(),
            b"/x/db-journal"
        );
        assert_eq!(sqlite3_filename_wal(Some(&f)).unwrap(), b"/x/db-wal");
        assert_eq!(
            sqlite3_uri_parameter(Some(&f), Some(b"cache")).unwrap(),
            b"private"
        );
        assert_eq!(sqlite3_uri_parameter(Some(&f), Some(b"nope")), None);
        assert_eq!(sqlite3_uri_key(Some(&f), 0).unwrap(), b"mode");
        assert_eq!(sqlite3_uri_key(Some(&f), 1).unwrap(), b"cache");
        assert_eq!(sqlite3_uri_key(Some(&f), 2), None);
    }

    #[test]
    fn limits_table() {
        assert_eq!(aHardLimit[SQLITE_LIMIT_LENGTH as usize], SQLITE_MAX_LENGTH);
        assert_eq!(
            aHardLimit[SQLITE_LIMIT_TRIGGER_STEPS as usize],
            SQLITE_MAX_TRIGGER_STEPS
        );
    }
}
