//! Port of prepare.c.
//!
//! This file contains the implementation of the sqlite3_prepare()
//! interface, and routines that contribute to loading the database schema
//! from disk.

use crate::btree::{
    sqlite3BtreeBeginTrans, sqlite3BtreeCommit, sqlite3BtreeGetMeta, sqlite3BtreeLastPage,
    sqlite3BtreeSchemaLocked, sqlite3BtreeSetCacheSize, sqlite3BtreeTxnState,
};
use crate::build::{
    sqlite3CommitInternalChanges, sqlite3FindIndex, sqlite3ResetAllSchemasOfConnection,
    sqlite3ResetOneSchema,
};
use crate::global::{sqlite3Config, sqlite3StdType, sqlite3UpperToLower};
use crate::int::*;
use crate::mainapi::{sqlite3_errmsg, sqlite3ApiExit, sqlite3ErrStr};
use crate::util::{sqlite3Error, sqlite3ErrorClear, sqlite3ErrorWithMsg, sqlite3GetUInt32};
use std::rc::Rc;

/// LEGACY_SCHEMA_TABLE
pub const LEGACY_SCHEMA_TABLE: &[u8] = b"sqlite_master";
/// LEGACY_TEMP_SCHEMA_TABLE
pub const LEGACY_TEMP_SCHEMA_TABLE: &[u8] = b"sqlite_temp_master";

/// SCHEMA_TABLE(x): the name of the schema table of database x.
#[inline]
pub fn SCHEMA_TABLE(x: i32) -> &'static [u8] {
    if x == 1 {
        LEGACY_TEMP_SCHEMA_TABLE
    } else {
        LEGACY_SCHEMA_TABLE
    }
}

/// Maximum number of times that we will try again to prepare a statement
/// that returns SQLITE_ERROR_RETRY.
const SQLITE_MAX_PREPARE_RETRY: i32 = 25;

/// Fill the InitData structure with an error message that indicates
/// that the database is corrupt.
fn corruptSchema(
    db: &mut Sqlite3,
    pData: &mut InitData,
    azObj: &[Option<&[u8]>],
    zExtra: Option<&[u8]>,
) {
    let obj = |i: usize| azObj.get(i).copied().flatten();
    if db.mallocFailed != 0 {
        pData.rc = SQLITE_NOMEM;
    } else if pData.pzErrMsg.is_some() {
        // A error message has already been generated.  Do not overwrite it
    } else if (pData.mInitFlags & INITFLAG_AlterMask) != 0 {
        const azAlterType: [&[u8]; 4] =
            [b"rename", b"drop column", b"add column", b"drop constraint"];
        let z = mprintf!(
            db,
            b"error in %s %s after %s: %s",
            obj(0),
            obj(1),
            azAlterType[((pData.mInitFlags & INITFLAG_AlterMask) - 1) as usize],
            zExtra
        );
        pData.pzErrMsg = Some(z);
        pData.rc = SQLITE_ERROR;
    } else if (db.flags & SQLITE_WriteSchema) != 0 {
        pData.rc = SQLITE_CORRUPT;
    } else {
        let zObj: &[u8] = obj(1).unwrap_or(b"?");
        let mut z = mprintf!(db, b"malformed database schema (%s)", zObj);
        if let Some(e) = zExtra {
            if !e.is_empty() {
                z = mprintf!(db, b"%s - %s", &z, e);
            }
        }
        pData.pzErrMsg = Some(z);
        pData.rc = SQLITE_CORRUPT;
    }
}

/// C: `int sqlite3IndexHasDuplicateRootPage(Index *pIndex)`
///
/// Check to see if any sibling index (another index on the same table)
/// of pIndex has the same root page number, and if it does, return true.
/// This would indicate a corrupt schema.
pub fn sqlite3IndexHasDuplicateRootPage(pIndex: &IndexRef) -> i32 {
    let (tnum, pTab) = {
        let i = pIndex.borrow();
        (i.tnum, i.table())
    };
    let t = pTab.borrow();
    for p in t.pIndex.iter() {
        if !Rc::ptr_eq(p, pIndex) && p.borrow().tnum == tnum {
            return 1;
        }
    }
    0
}

/// C: `int sqlite3InitCallback(void *pInit, int argc, char **argv, char **NotUsed)`
///
/// This is the callback routine for the code that initializes the
/// database.  See sqlite3Init() below for additional information.
/// This routine is also called from the OP_ParseSchema opcode of the VDBE.
///
/// Each callback contains the following information:
///
///     argv[0] = type of object: "table", "index", "trigger", or "view".
///     argv[1] = name of thing being created
///     argv[2] = associated table if an index or trigger
///     argv[3] = root page number for table or index. 0 for trigger or view.
///     argv[4] = SQL text for the CREATE statement.
///
/// Rust: `argv` is None for the "empty result" callback (C argv==0).  The
/// error message goes to `pData.pzErrMsg` (callers that pass a message
/// location in C move it in and out of InitData).
pub fn sqlite3InitCallback(
    db: &mut Sqlite3,
    pData: &mut InitData,
    argv: Option<&[Option<&[u8]>]>,
) -> i32 {
    let iDb = pData.iDb;

    db.mDbFlags |= DBFLAG_EncodingFixed;
    let Some(argv) = argv else {
        return 0; // Might happen if EMPTY_RESULT_CALLBACKS are on
    };
    debug_assert!(argv.len() >= 5);
    let arg = |i: usize| argv.get(i).copied().flatten();
    pData.nInitRow += 1;
    if db.mallocFailed != 0 {
        corruptSchema(db, pData, argv, None);
        return 1;
    }

    debug_assert!(iDb >= 0 && iDb < db.nDb());
    let a4 = arg(4);
    let is_create = match a4 {
        Some(z) => {
            b'c' == sqlite3UpperToLower[z.first().copied().unwrap_or(0) as usize]
                && b'r' == sqlite3UpperToLower[z.get(1).copied().unwrap_or(0) as usize]
        }
        None => false,
    };
    if arg(3).is_none() {
        corruptSchema(db, pData, argv, None);
    } else if is_create {
        // Call the parser to process a CREATE TABLE, INDEX or VIEW.
        // But because db->init.busy is set to 1, no VDBE code is generated
        // or executed.  All the parser does is build the internal data
        // structures that describe the table, index, or view.
        //
        // No other valid SQL statement, other than the variable CREATE
        // statements, can begin with the letters "C" and "R".  Thus, it is
        // not possible run any other kind of statement while parsing the
        // schema, even a corrupt schema.
        let saved_iDb = db.init.iDb;

        debug_assert!(db.init.busy != 0);
        db.init.iDb = iDb as u8;
        let mut newTnum: u32 = 0;
        let ok = sqlite3GetUInt32(arg(3), &mut newTnum);
        db.init.newTnum = newTnum;
        if ok == 0 || (db.init.newTnum > pData.mxPage && pData.mxPage > 0) {
            if sqlite3Config.bExtraSchemaChecks != 0 {
                corruptSchema(db, pData, argv, Some(b"invalid rootpage"));
            }
        }
        db.init.orphanTrigger = 0;
        db.init.azInit = (0..3).map(|i| arg(i).unwrap_or(b"").to_vec()).collect();
        let zSql = a4.unwrap();
        let (_rcp, pStmt, zEnd) = sqlite3Prepare(db, zSql, -1, 0, None);
        let rc = db.errCode;
        db.init.iDb = saved_iDb;
        // assert( saved_iDb==0 || (db->mDbFlags & DBFLAG_Vacuum)!=0 );
        if SQLITE_OK != rc {
            if db.init.orphanTrigger != 0 {
                debug_assert!(iDb == 1);
            } else {
                if rc > pData.rc {
                    pData.rc = rc;
                }
                if rc == SQLITE_NOMEM {
                    // sqlite3OomFault(db)
                } else if rc != SQLITE_INTERRUPT && (rc & 0xFF) != SQLITE_LOCKED {
                    let zErr = sqlite3_errmsg(db);
                    corruptSchema(db, pData, argv, zErr.as_deref());
                }
            }
        } else if zEnd < sqlText(zSql, -1).len() {
            corruptSchema(db, pData, argv, None);
        }
        db.init.azInit = sqlite3StdType.iter().map(|s| s.to_vec()).collect();
        if let Some(mut v) = pStmt {
            // sqlite3_finalize(pStmt)
            crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v);
        }
    } else if arg(1).is_none() || a4.map_or(false, |z| !z.is_empty()) {
        corruptSchema(db, pData, argv, None);
    } else {
        // If the SQL column is blank it means this is an index that
        // was created to be the PRIMARY KEY or to fulfill a UNIQUE
        // constraint for a CREATE TABLE.  The index should have already
        // been created when we processed the CREATE TABLE.  All we have
        // to do here is record the root page number for that index.
        let zDbSName = db.aDb[iDb as usize].zDbSName.clone();
        let pIndex = sqlite3FindIndex(db, arg(1), zDbSName.as_deref());
        match pIndex {
            None => {
                corruptSchema(db, pData, argv, Some(b"orphan index"));
            }
            Some(pIndex) => {
                let mut tnum: u32 = 0;
                let ok = sqlite3GetUInt32(arg(3), &mut tnum);
                pIndex.borrow_mut().tnum = tnum;
                if ok == 0
                    || tnum < 2
                    || tnum > pData.mxPage
                    || sqlite3IndexHasDuplicateRootPage(&pIndex) != 0
                {
                    if sqlite3Config.bExtraSchemaChecks != 0 {
                        corruptSchema(db, pData, argv, Some(b"invalid rootpage"));
                    }
                }
            }
        }
    }
    if pData.pzErrMsg.is_none() {
        let pX = &db.aDb[iDb as usize].pSchema;
        if (pX.tblHash.count() as u64 + pX.idxHash.count() as u64 + pX.trigHash.count() as u64)
            > db.aLimit[SQLITE_LIMIT_SCHEMA as usize] as u32 as u64
        {
            pData.pzErrMsg = Some(mprintf!(db, b"too many schema objects"));
            pData.rc = SQLITE_ERROR;
            return 1;
        }
    }
    0
}

/// C: `int sqlite3InitOne(sqlite3 *db, int iDb, char **pzErrMsg, u32 mFlags)`
///
/// Attempt to read the database schema and initialize internal
/// data structures for a single database file.  The index of the
/// database file is given by iDb.  iDb==0 is used for the main
/// database.  iDb==1 should never be used.  iDb>=2 is used for
/// auxiliary databases.  Return one of the SQLITE_ error codes to
/// indicate success or failure.
pub fn sqlite3InitOne(db: &mut Sqlite3, iDb: i32, pzErrMsg: &mut Option<ZStr>, mFlags: u32) -> i32 {
    let mut rc;
    let mut meta = [0i32; 5];
    let mut openedTransaction = false;
    let mask = (db.mDbFlags & DBFLAG_EncodingFixed) | !DBFLAG_EncodingFixed;

    debug_assert!((db.mDbFlags & DBFLAG_SchemaKnownOk) == 0);
    debug_assert!(iDb >= 0 && iDb < db.nDb());

    db.init.busy = 1 + ((mFlags & INITFLAG_AlterAdd) != 0) as u8;
    // ^--- Any non-zero value for init.busy means that we are scanning
    // the sqlite_schema table to build the internal schema representation,
    // rather than running actual CREATE statements.  init.busy==2 has the
    // additional meaning that the scan is happening as part of
    // ALTER TABLE ADD COLUMN, which is stricter in its enforcement of
    // function name resolution.

    // Construct the in-memory representation schema tables (sqlite_schema or
    // sqlite_temp_schema) by invoking the parser directly.  The appropriate
    // table name will be inserted automatically by the parser so we can just
    // use the abbreviation "x" here.  The parser will also automatically tag
    // the schema table as read-only.
    let zSchemaTabName = SCHEMA_TABLE(iDb);
    let azArg: [Option<&[u8]>; 6] = [
        Some(b"table"),
        Some(zSchemaTabName),
        Some(zSchemaTabName),
        Some(b"1"),
        Some(b"CREATE TABLE x(type text,name text,tbl_name text,rootpage int,sql text)"),
        None,
    ];
    let mut initData = InitData {
        iDb,
        rc: SQLITE_OK,
        pzErrMsg: pzErrMsg.take(),
        mInitFlags: mFlags,
        nInitRow: 0,
        mxPage: 0,
    };
    sqlite3InitCallback(db, &mut initData, Some(&azArg[..5]));
    db.mDbFlags &= mask;

    'error_out: {
        if initData.rc != 0 {
            rc = initData.rc;
            break 'error_out;
        }

        // Create a cursor to hold the database open
        let Some(pBt) = db.aDb[iDb as usize].pBt.clone() else {
            debug_assert!(iDb == 1);
            db.DbSetProperty(1, DB_SchemaLoaded);
            rc = SQLITE_OK;
            break 'error_out;
        };

        'initone_error_out: {
            // If there is not already a read-only (or read-write) transaction
            // opened on the b-tree database, open one now. If a transaction is
            // opened, it will be closed before this function returns.
            if sqlite3BtreeTxnState(db, &pBt) == SQLITE_TXN_NONE {
                let mut dummy = 0;
                rc = sqlite3BtreeBeginTrans(db, &pBt, 0, &mut dummy);
                if rc != SQLITE_OK {
                    initData.pzErrMsg = sqlite3ErrStr(rc);
                    break 'initone_error_out;
                }
                openedTransaction = true;
            }

            // Get the database meta information.
            //
            // Meta values are as follows:
            //    meta[0]   Schema cookie.  Changes with each schema change.
            //    meta[1]   File format of schema layer.
            //    meta[2]   Size of the page cache.
            //    meta[3]   Largest rootpage (auto/incr_vacuum mode)
            //    meta[4]   Db text encoding. 1:UTF-8 2:UTF-16LE 3:UTF-16BE
            //    meta[5]   User version
            //    meta[6]   Incremental vacuum mode
            //    meta[7]   unused
            //    meta[8]   unused
            //    meta[9]   unused
            //
            // Note: The #defined SQLITE_UTF* symbols in sqliteInt.h correspond
            // to the possible values of meta[4].
            for i in 0..meta.len() {
                let mut m: u32 = 0;
                sqlite3BtreeGetMeta(&pBt, i as i32 + 1, &mut m);
                meta[i] = m as i32;
            }
            if (db.flags & SQLITE_ResetDatabase) != 0 {
                meta = [0; 5];
            }
            db.aDb[iDb as usize].pSchema.schema_cookie = meta[(BTREE_SCHEMA_VERSION - 1) as usize];

            // If opening a non-empty database, check the text encoding. For the
            // main database, set sqlite3.enc to the encoding of the main
            // database.  For an attached db, it is an error if the encoding is
            // not the same as sqlite3.enc.
            let menc = meta[(BTREE_TEXT_ENCODING - 1) as usize];
            if menc != 0 {
                if iDb == 0 && (db.mDbFlags & DBFLAG_EncodingFixed) == 0 {
                    // If opening the main database, set ENC(db).
                    let mut encoding = (menc as u8) & 3;
                    if encoding == 0 {
                        encoding = SQLITE_UTF8;
                    }
                    crate::callback::sqlite3SetTextEncoding(db, encoding);
                } else {
                    // If opening an attached database, the encoding much match ENC(db)
                    if (menc & 3) != db.ENC() as i32 {
                        initData.pzErrMsg = Some(
                            b"attached databases must use the same text encoding as main database"
                                .to_vec(),
                        );
                        rc = SQLITE_ERROR;
                        break 'initone_error_out;
                    }
                }
            }
            let enc = db.ENC();
            db.aDb[iDb as usize].pSchema.enc = enc;

            if db.aDb[iDb as usize].pSchema.cache_size == 0 {
                let mut size =
                    crate::util::sqlite3AbsInt32(meta[(BTREE_DEFAULT_CACHE_SIZE - 1) as usize]);
                if size == 0 {
                    size = SQLITE_DEFAULT_CACHE_SIZE;
                }
                db.aDb[iDb as usize].pSchema.cache_size = size;
                sqlite3BtreeSetCacheSize(db, &pBt, size);
            }

            // file_format==1    Version 3.0.0.
            // file_format==2    Version 3.1.3.  // ALTER TABLE ADD COLUMN
            // file_format==3    Version 3.1.4.  // ditto but with non-NULL defaults
            // file_format==4    Version 3.3.0.  // DESC indices.  Boolean constants
            {
                let s = &mut db.aDb[iDb as usize].pSchema;
                s.file_format = meta[(BTREE_FILE_FORMAT - 1) as usize] as u8;
                if s.file_format == 0 {
                    s.file_format = 1;
                }
                if s.file_format as i32 > SQLITE_MAX_FILE_FORMAT {
                    initData.pzErrMsg = Some(b"unsupported file format".to_vec());
                    rc = SQLITE_ERROR;
                    break 'initone_error_out;
                }
            }

            // Ticket #2804:  When we open a database in the newer file format,
            // clear the legacy_file_format pragma flag so that a VACUUM will
            // not downgrade the database and thus invalidate any descending
            // indices that the user might have created.
            if iDb == 0 && meta[(BTREE_FILE_FORMAT - 1) as usize] >= 4 {
                db.flags &= !SQLITE_LegacyFileFmt;
            }

            // Read the schema information out of the schema tables
            debug_assert!(db.init.busy != 0);
            initData.mxPage = sqlite3BtreeLastPage(&pBt);
            {
                let zDbSName = db.aDb[iDb as usize].zDbSName.clone();
                let zSql = mprintf!(
                    db,
                    b"SELECT*FROM\"%w\".%s ORDER BY rowid",
                    zDbSName.as_deref(),
                    zSchemaTabName
                );
                let xAuth = db.xAuth.take();
                {
                    let mut cb =
                        |db: &mut Sqlite3, argv: Option<&[Option<&[u8]>]>, _c: &[&[u8]]| {
                            sqlite3InitCallback(db, &mut initData, argv)
                        };
                    rc = crate::legacy::sqlite3_exec_db(db, Some(&zSql), Some(&mut cb), None);
                }
                db.xAuth = xAuth;
                if rc == SQLITE_OK {
                    rc = initData.rc;
                }
                if rc == SQLITE_OK {
                    crate::analyze::sqlite3AnalysisLoad(db, iDb);
                }
            }
            if db.mallocFailed != 0 {
                rc = SQLITE_NOMEM;
                sqlite3ResetAllSchemasOfConnection(db);
            } else if rc == SQLITE_OK
                || ((db.flags & SQLITE_NoSchemaError) != 0 && rc != SQLITE_NOMEM)
            {
                // Hack: If the SQLITE_NoSchemaError flag is set, then consider
                // the schema loaded, even if errors (other than OOM) occurred. In
                // this situation the current sqlite3_prepare() operation will fail,
                // but the following one will attempt to compile the supplied
                // statement against whatever subset of the schema was loaded
                // before the error occurred.
                //
                // The primary purpose of this is to allow access to the
                // sqlite_schema table even when its contents have been corrupted.
                db.DbSetProperty(iDb, DB_SchemaLoaded);
                rc = SQLITE_OK;
            }
        }

        // Jump here for an error that occurs after successfully allocating
        // curMain and calling sqlite3BtreeEnter(). For an error that occurs
        // before that point, jump to error_out.
        // initone_error_out:
        if openedTransaction {
            sqlite3BtreeCommit(db, &pBt);
        }
    }

    // error_out:
    *pzErrMsg = initData.pzErrMsg.take();
    if rc != 0 {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            // sqlite3OomFault(db)
        }
        sqlite3ResetOneSchema(db, iDb);
    }
    db.init.busy = 0;
    rc
}

/// C: `int sqlite3Init(sqlite3 *db, char **pzErrMsg)`
///
/// Initialize all database files - the main database file, the file
/// used to store temporary tables, and any additional database files
/// created using ATTACH statements.  Return a success code.  If an
/// error occurs, write an error message into *pzErrMsg.
///
/// After a database is initialized, the DB_SchemaLoaded bit is set
/// bit is set in the flags field of the Db structure.
pub fn sqlite3Init(db: &mut Sqlite3, pzErrMsg: &mut Option<ZStr>) -> i32 {
    let commit_internal = (db.mDbFlags & DBFLAG_SchemaChange) == 0;

    debug_assert!(db.init.busy == 0);
    db.enc = db.aDb[0].pSchema.enc; // ENC(db) = SCHEMA_ENC(db);
    debug_assert!(db.nDb() > 0);
    // Do the main schema first
    if !db.DbHasProperty(0, DB_SchemaLoaded) {
        let rc = sqlite3InitOne(db, 0, pzErrMsg, 0);
        if rc != 0 {
            return rc;
        }
    }
    // All other schemas after the main schema. The "temp" schema must be last
    let mut i = db.nDb() - 1;
    while i > 0 {
        if !db.DbHasProperty(i, DB_SchemaLoaded) {
            let rc = sqlite3InitOne(db, i, pzErrMsg, 0);
            if rc != 0 {
                return rc;
            }
        }
        i -= 1;
    }
    if commit_internal {
        sqlite3CommitInternalChanges(db);
    }
    SQLITE_OK
}

/// C: `int sqlite3ReadSchema(Parse *pParse)`
///
/// This routine is a no-op if the database schema is already initialized.
/// Otherwise, the schema is loaded. An error code is returned.
pub fn sqlite3ReadSchema(db: &mut Sqlite3) -> i32 {
    let mut rc = SQLITE_OK;
    if db.init.busy == 0 {
        let mut zErrMsg = db.pp().zErrMsg.take();
        rc = sqlite3Init(db, &mut zErrMsg);
        db.pp().zErrMsg = zErrMsg;
        if rc != SQLITE_OK {
            let p = db.pp();
            p.rc = rc;
            p.nErr += 1;
        } else if db.noSharedCache != 0 {
            db.mDbFlags |= DBFLAG_SchemaKnownOk;
        }
    }
    rc
}

/// Check schema cookies in all databases.  If any cookie is out
/// of date set pParse->rc to SQLITE_SCHEMA.  If all schema cookies
/// make no changes to pParse->rc.
fn schemaIsValid(db: &mut Sqlite3) {
    debug_assert!(db.ppr().checkSchema != 0);
    let mut iDb = 0;
    while iDb < db.nDb() {
        let mut openedTransaction = false; // True if a transaction is opened
        let Some(pBt) = db.aDb[iDb as usize].pBt.clone() else {
            iDb += 1;
            continue;
        };

        // If there is not already a read-only (or read-write) transaction
        // opened on the b-tree database, open one now. If a transaction is
        // opened, it will be closed immediately after reading the meta-value.
        if sqlite3BtreeTxnState(db, &pBt) == SQLITE_TXN_NONE {
            let mut dummy = 0;
            let rc = sqlite3BtreeBeginTrans(db, &pBt, 0, &mut dummy);
            if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                db.pp().rc = SQLITE_NOMEM;
            }
            if rc != SQLITE_OK {
                return;
            }
            openedTransaction = true;
        }

        // Read the schema cookie from the database. If it does not match the
        // value stored as part of the in-memory schema representation,
        // set Parse.rc to SQLITE_SCHEMA.
        let mut cookie: u32 = 0;
        sqlite3BtreeGetMeta(&pBt, BTREE_SCHEMA_VERSION, &mut cookie);
        if cookie as i32 != db.aDb[iDb as usize].pSchema.schema_cookie {
            if db.DbHasProperty(iDb, DB_SchemaLoaded) {
                db.pp().rc = SQLITE_SCHEMA;
            }
            sqlite3ResetOneSchema(db, iDb);
        }

        // Close the transaction, if one was opened.
        if openedTransaction {
            sqlite3BtreeCommit(db, &pBt);
        }
        iDb += 1;
    }
}

/// C: `int sqlite3SchemaToIndex(sqlite3 *db, Schema *pSchema)`
///
/// Convert a schema pointer into the iDb index that indicates
/// which database file in db->aDb[] the schema refers to.
///
/// If the same database is attached more than once, the first
/// attached database is returned.
///
/// Rust: takes the SchemaId; an unknown id (C: a NULL pSchema, i.e. a
/// transient table created by a sub-select) returns -32768.
pub fn sqlite3SchemaToIndex(db: &Sqlite3, pSchema: SchemaId) -> i32 {
    db.schema_index(pSchema)
}

/// C: `void sqlite3ParseObjectReset(Parse *pParse)`
///
/// Free all memory allocations in the pParse object.
///
/// Rust: runs the registered cleanups and pops the current Parse off
/// db.parses (C: db->pParse = pParse->pOuterParse).  When the last
/// Parse is popped, the parse-tree arena is reset.
pub fn sqlite3ParseObjectReset(db: &mut Sqlite3) {
    debug_assert!(!db.parses.is_empty());
    debug_assert!(db.ppr().nested == 0);
    loop {
        let Some(xCleanup) = db.pp().pCleanup.pop() else {
            break;
        };
        xCleanup(db);
    }
    let pConstExpr = db.ppr().pConstExpr;
    if pConstExpr.nn() {
        crate::expr::sqlite3ExprListDelete(db, pConstExpr);
    }
    db.parses.pop();
    if db.parses.is_empty() {
        db.arena.reset();
    }
}

/// C: `void * sqlite3ParserAddCleanup( Parse *pParse, void (*xCleanup)(sqlite3*,void*), void *pPtr )`
///
/// Add a new cleanup operation to a Parser.  The cleanup should happen
/// when the parser object is destroyed.
///
/// Rust: the cleanup is a closure (it captures whatever C passes as
/// pPtr); there is no early-cleanup case since allocations cannot fail.
pub fn sqlite3ParserAddCleanup(db: &mut Sqlite3, xCleanup: Box<dyn FnOnce(&mut Sqlite3)>) {
    db.pp().pCleanup.push(xCleanup);
}

/// C: `void sqlite3ParseObjectInit(Parse *pParse, sqlite3 *db)`
///
/// Turn bulk memory into a valid Parse object and link that Parse object
/// into database connection db.
///
/// Call sqlite3ParseObjectReset() to undo this operation.
///
/// Rust: pushes a fresh (toplevel) Parse onto db.parses.
pub fn sqlite3ParseObjectInit(db: &mut Sqlite3) {
    let i = db.parses.len();
    db.parses.push(Parse {
        iToplevel: i,
        ..Default::default()
    });
    if db.mallocFailed != 0 {
        errmsg!(db, b"out of memory");
    }
}

/// Rust: the SQL text of a statement: the slice up to nBytes bytes
/// (nBytes<0: no limit), cut at the first NUL character.
fn sqlText(zSql: &[u8], nBytes: i32) -> &[u8] {
    let z = if nBytes >= 0 && (nBytes as usize) < zSql.len() {
        &zSql[..nBytes as usize]
    } else {
        zSql
    };
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// Compile the UTF-8 encoded SQL statement zSql into a statement handle.
///
/// Rust: returns (rc, the prepared statement (not yet registered in
/// db.aVdbe), byte offset of the unparsed tail in zSql).
fn sqlite3Prepare(
    db: &mut Sqlite3,
    zSql: &[u8],
    nBytes: i32,
    prepFlags: u32,
    pReprepare: Option<StmtId>,
) -> (i32, Option<Box<Vdbe>>, usize) {
    let mut rc: i32;
    let mut ppStmt: Option<Box<Vdbe>> = None;
    let mut zTail: usize = 0;

    // sqlite3ParseObjectInit(&sParse, db); // inlined for performance
    sqlite3ParseObjectInitNoErr(db);
    if let Some(id) = pReprepare {
        let explain = crate::vdbeapi::sqlite3_stmt_isexplain(db, id) as u8;
        let p = db.pp();
        p.pReprepare = Some(id);
        p.explain = explain;
    }
    'end_prepare: {
        if db.mallocFailed != 0 {
            errmsg!(db, b"out of memory");
            rc = SQLITE_NOMEM;
            db.errCode = rc;
            break 'end_prepare;
        }

        // For a long-term use prepared statement avoid the use of
        // lookaside memory.
        if (prepFlags & SQLITE_PREPARE_PERSISTENT as u32) != 0 {
            db.pp().disableLookaside += 1;
        }
        db.pp().prepFlags = (prepFlags & 0xff) as u8;

        // Check to verify that it is possible to get a read lock on all
        // database schemas.  The inability to get a read lock indicates that
        // some other database connection is holding a write-lock, which in
        // turn means that the other connection has made uncommitted changes
        // to the schema.
        //
        // Were we to proceed and prepare the statement against the uncommitted
        // schema changes and if those schema changes are subsequently rolled
        // back and different changes are made in their place, then when this
        // prepared statement goes to run the schema cookie would fail to detect
        // the schema change.  Disaster would follow.
        //
        // Note that setting READ_UNCOMMITTED overrides most lock detection,
        // but it does *not* override schema lock detection, so this all still
        // works even if READ_UNCOMMITTED is set.
        if db.noSharedCache == 0 {
            for i in 0..db.aDb.len() {
                if let Some(pBt) = db.aDb[i].pBt.clone() {
                    rc = sqlite3BtreeSchemaLocked(db, &pBt);
                    if rc != 0 {
                        let zDb = db.aDb[i].zDbSName.clone();
                        sqlite3ErrorWithMsg(
                            db,
                            rc,
                            Some(b"database schema is locked: %s"),
                            &[crate::printf::PfArg::Str(zDb.as_deref())],
                        );
                        break 'end_prepare;
                    }
                }
            }
        }

        if !db.pDisconnect.is_empty() {
            crate::vtab::sqlite3VtabUnlockList(db);
        }

        let n = if nBytes >= 0 {
            (nBytes as usize).min(zSql.len())
        } else {
            0
        };
        if nBytes >= 0 && (n == 0 || zSql[n - 1] != 0) {
            let mxLen = db.aLimit[SQLITE_LIMIT_SQL_LENGTH as usize];
            if nBytes > mxLen {
                sqlite3ErrorWithMsg(db, SQLITE_TOOBIG, Some(b"statement too long"), &[]);
                rc = sqlite3ApiExit(db, SQLITE_TOOBIG);
                break 'end_prepare;
            }
        }
        let zText: Rc<[u8]> = Rc::from(sqlText(zSql, nBytes));
        crate::tokenize::sqlite3RunParser(db, &zText, 0);
        zTail = db.ppr().zTail.min(zText.len());
        debug_assert!(0 == db.ppr().nQueryLoop);

        if db.init.busy == 0 {
            if let Some(v) = db.pp().pVdbe.as_deref_mut() {
                crate::vdbeaux::sqlite3VdbeSetSql(v, &zText[..zTail], prepFlags as u8);
            }
        }
        if db.mallocFailed != 0 {
            let p = db.pp();
            p.rc = SQLITE_NOMEM;
            p.checkSchema = 0;
        }
        let prc = db.ppr().rc;
        if prc != SQLITE_OK && prc != SQLITE_DONE {
            if db.ppr().checkSchema != 0 && db.init.busy == 0 {
                schemaIsValid(db);
            }
            if let Some(mut v) = db.pp().pVdbe.take() {
                crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v);
            }
            debug_assert!(ppStmt.is_none());
            rc = db.ppr().rc;
            if let Some(zErrMsg) = db.pp().zErrMsg.take() {
                sqlite3ErrorWithMsg(
                    db,
                    rc,
                    Some(b"%s"),
                    &[crate::printf::PfArg::Str(Some(&zErrMsg))],
                );
            } else {
                sqlite3Error(db, rc);
            }
        } else {
            debug_assert!(db.ppr().zErrMsg.is_none());
            ppStmt = db.pp().pVdbe.take();
            rc = SQLITE_OK;
            sqlite3ErrorClear(db);
        }

        // Delete any TriggerPrg structures allocated while parsing this
        // statement.
        db.pp().pTriggerPrg.clear();
    }

    // end_prepare:
    sqlite3ParseObjectReset(db);
    (rc, ppStmt, zTail)
}

/// sqlite3ParseObjectInit() without the "out of memory" check (the
/// inlined copy at the top of sqlite3Prepare()).
fn sqlite3ParseObjectInitNoErr(db: &mut Sqlite3) {
    let i = db.parses.len();
    db.parses.push(Parse {
        iToplevel: i,
        ..Default::default()
    });
}

/// Rust: sqlite3LockAndPrepare() returning the new statement (not yet
/// registered) and the tail offset.
fn sqlite3LockAndPrepare(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    nBytes: i32,
    prepFlags: u32,
    pOld: Option<StmtId>,
    ppStmt: &mut Option<Box<Vdbe>>,
    pzTail: &mut usize,
) -> i32 {
    let mut rc;
    let mut cnt = 0;

    *ppStmt = None;
    let Some(zSql) = zSql else {
        return SQLITE_MISUSE;
    };
    if crate::util::sqlite3SafetyCheckOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    loop {
        // Make multiple attempts to compile the SQL, until it either succeeds
        // or encounters a permanent error.  A schema problem after one schema
        // reset is considered a permanent error.
        let (r, pStmt, zTail) = sqlite3Prepare(db, zSql, nBytes, prepFlags, pOld);
        rc = r;
        *ppStmt = pStmt;
        *pzTail = zTail;
        debug_assert!(rc == SQLITE_OK || ppStmt.is_none());
        if rc == SQLITE_OK || db.mallocFailed != 0 {
            break;
        }
        cnt += 1;
        let again = (rc == SQLITE_ERROR_RETRY && cnt <= SQLITE_MAX_PREPARE_RETRY)
            || (rc == SQLITE_SCHEMA && {
                sqlite3ResetOneSchema(db, -1);
                cnt == 1
            });
        if !again {
            break;
        }
    }
    debug_assert!(rc != SQLITE_ERROR_RETRY);
    rc = sqlite3ApiExit(db, rc);
    debug_assert!((rc & db.errMask) == rc);
    db.busyHandler.borrow_mut().nBusy = 0;
    debug_assert!(rc == SQLITE_OK || ppStmt.is_none());
    rc
}

/// sqlite3LockAndPrepare() for the public API: registers the statement in
/// db.aVdbe and returns its handle.
fn lockAndPrepareApi(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    nBytes: i32,
    prepFlags: u32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    let mut pNew: Option<Box<Vdbe>> = None;
    let rc = sqlite3LockAndPrepare(db, zSql, nBytes, prepFlags, None, &mut pNew, pzTail);
    *ppStmt = pNew.map(|v| db.stmt_new(v));
    rc
}

/// C: `int sqlite3Reprepare(Vdbe *p)`
///
/// Rerun the compilation of a statement after a schema change.
///
/// If the statement is successfully recompiled, return SQLITE_OK.
/// Otherwise, if the statement cannot be recompiled because another
/// connection has locked the sqlite3_schema table, return SQLITE_LOCKED.
/// If any other error occurs, return SQLITE_SCHEMA.
///
/// Rust: takes the statement handle; the statement must be in its slot
/// (not taken out) so that the parser can consult it through
/// Parse::pReprepare.  The new program is swapped into the existing
/// statement, so the handle stays valid.
pub fn sqlite3Reprepare(db: &mut Sqlite3, p: StmtId) -> i32 {
    let zSql = crate::vdbeapi::sqlite3_sql(db, p);
    debug_assert!(zSql.is_some()); // Reprepare only called for prepare_v2() statements
    let prepFlags = db
        .with_stmt(p, |_db, v| crate::vdbeaux::sqlite3VdbePrepareFlags(v))
        .unwrap_or(0);
    let mut pNew: Option<Box<Vdbe>> = None;
    let mut zTail = 0usize;
    let rc = sqlite3LockAndPrepare(
        db,
        zSql.as_deref(),
        -1,
        prepFlags as u32,
        Some(p),
        &mut pNew,
        &mut zTail,
    );
    if rc != 0 {
        if rc == SQLITE_NOMEM {
            // sqlite3OomFault(db)
        }
        debug_assert!(pNew.is_none());
        return rc;
    }
    let Some(mut pNew) = pNew else {
        // C asserts pNew!=0.
        return rc;
    };
    db.with_stmt(p, |db, pOld| {
        crate::vdbeaux::sqlite3VdbeSwap(db, &mut pNew, pOld);
        crate::vdbeapi::sqlite3TransferBindings(db, &mut pNew, pOld);
    });
    crate::vdbeaux::sqlite3VdbeResetStepResult(&mut pNew);
    crate::vdbeaux::sqlite3VdbeFinalize(db, &mut pNew);
    SQLITE_OK
}

/// C: `int sqlite3_prepare( sqlite3 *db, const char *zSql, int nBytes, sqlite3_stmt **ppStmt, const char **pzTail )`
///
/// Two versions of the official API.  Legacy and new use.  In the legacy
/// version, the original SQL text is not saved in the prepared statement
/// and so if a schema change occurs, SQLITE_SCHEMA is returned by
/// sqlite3_step().  In the new version, the original SQL text is retained
/// and the statement is automatically recompiled if an schema change
/// occurs.
///
/// Rust: `*pzTail` receives the byte offset of the unparsed tail in zSql.
pub fn sqlite3_prepare(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    nBytes: i32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    let rc = lockAndPrepareApi(db, zSql, nBytes, 0, ppStmt, pzTail);
    debug_assert!(rc == SQLITE_OK || ppStmt.is_none()); // VERIFY: F13021
    rc
}

/// C: `int sqlite3_prepare_v2( sqlite3 *db, const char *zSql, int nBytes, sqlite3_stmt **ppStmt, const char **pzTail )`
pub fn sqlite3_prepare_v2(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    nBytes: i32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    // EVIDENCE-OF: R-37923-12173 The sqlite3_prepare_v2() interface works
    // exactly the same as sqlite3_prepare_v3() with a zero prepFlags
    // parameter.
    let rc = lockAndPrepareApi(
        db,
        zSql,
        nBytes,
        SQLITE_PREPARE_SAVESQL as u32,
        ppStmt,
        pzTail,
    );
    debug_assert!(rc == SQLITE_OK || ppStmt.is_none());
    rc
}

/// C: `int sqlite3_prepare_v3( sqlite3 *db, const char *zSql, int nBytes, unsigned int prepFlags, sqlite3_stmt **ppStmt, const char **pzTail )`
pub fn sqlite3_prepare_v3(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    nBytes: i32,
    prepFlags: u32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    // EVIDENCE-OF: R-56861-42673 sqlite3_prepare_v3() differs from
    // sqlite3_prepare_v2() only in having the extra prepFlags parameter,
    // which is a bit array consisting of zero or more of the
    // SQLITE_PREPARE_* flags.
    let rc = lockAndPrepareApi(
        db,
        zSql,
        nBytes,
        SQLITE_PREPARE_SAVESQL as u32 | (prepFlags & SQLITE_PREPARE_MASK as u32),
        ppStmt,
        pzTail,
    );
    debug_assert!(rc == SQLITE_OK || ppStmt.is_none());
    rc
}

/// Rust: decode native (little-endian) UTF-16 into UTF-8 (unpaired
/// surrogates become U+FFFD).
pub(crate) fn utf16ne_to_utf8(z: &[u8]) -> ZStr {
    let units: Vec<u16> = z
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    let mut out = Vec::with_capacity(units.len());
    for r in char::decode_utf16(units.iter().copied()) {
        let ch = r.unwrap_or('\u{FFFD}');
        let mut b = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
    }
    out
}

/// Compile the UTF-16 encoded SQL statement zSql into a statement handle.
fn sqlite3Prepare16(
    db: &mut Sqlite3,
    zSql: &[u8],
    nBytes: i32,
    prepFlags: u32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    // This function currently works by first transforming the UTF-16
    // encoded string to UTF-8, then invoking sqlite3_prepare(). The
    // tricky bit is figuring out the pointer to return in *pzTail.
    *ppStmt = None;
    if crate::util::sqlite3SafetyCheckOk(db) == 0 {
        return SQLITE_MISUSE;
    }

    // Make sure nBytes is non-negative and correct.  It should be the
    // number of bytes until the end of the input buffer or until the first
    // U+0000 character.  If the input nBytes is odd, convert it into
    // an even number.  If the input nBytes is negative, then the input
    // must be terminated by at least one U+0000 character
    let lim = if nBytes >= 0 {
        (nBytes as usize).min(zSql.len())
    } else {
        zSql.len()
    };
    let mut sz = 0usize;
    while sz < lim {
        let a = zSql[sz];
        let b = zSql.get(sz + 1).copied().unwrap_or(0);
        if a == 0 && b == 0 {
            break;
        }
        sz += 2;
    }
    let sz = sz.min(zSql.len() & !1);
    let zSql16 = &zSql[..sz];
    let zSql8 = utf16ne_to_utf8(zSql16);
    let mut zTail8 = 0usize;
    let rc = lockAndPrepareApi(db, Some(&zSql8), -1, prepFlags, ppStmt, &mut zTail8);

    // If sqlite3_prepare returns a tail pointer, we calculate the
    // equivalent pointer into the UTF-16 string by counting the unicode
    // characters between zSql8 and zTail8, and then returning a pointer
    // the same number of characters into the UTF-16 string.
    let chars_parsed = String::from_utf8_lossy(&zSql8[..zTail8.min(zSql8.len())])
        .chars()
        .map(|c| c.len_utf16())
        .sum::<usize>();
    *pzTail = (chars_parsed * 2).min(sz);
    sqlite3ApiExit(db, rc)
}

/// C: `int sqlite3_prepare16( sqlite3 *db, const void *zSql, int nBytes, sqlite3_stmt **ppStmt, const void **pzTail )`
///
/// Rust: `*pzTail` receives the byte offset of the unparsed tail.
pub fn sqlite3_prepare16(
    db: &mut Sqlite3,
    zSql: &[u8],
    nBytes: i32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    sqlite3Prepare16(db, zSql, nBytes & !1, 0, ppStmt, pzTail)
}

/// C: `int sqlite3_prepare16_v2( sqlite3 *db, const void *zSql, int nBytes, sqlite3_stmt **ppStmt, const void **pzTail )`
pub fn sqlite3_prepare16_v2(
    db: &mut Sqlite3,
    zSql: &[u8],
    nBytes: i32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    sqlite3Prepare16(
        db,
        zSql,
        nBytes & !1,
        SQLITE_PREPARE_SAVESQL as u32,
        ppStmt,
        pzTail,
    )
}

/// C: `int sqlite3_prepare16_v3( sqlite3 *db, const void *zSql, int nBytes, unsigned int prepFlags, sqlite3_stmt **ppStmt, const void **pzTail )`
pub fn sqlite3_prepare16_v3(
    db: &mut Sqlite3,
    zSql: &[u8],
    nBytes: i32,
    prepFlags: u32,
    ppStmt: &mut Option<StmtId>,
    pzTail: &mut usize,
) -> i32 {
    sqlite3Prepare16(
        db,
        zSql,
        nBytes & !1,
        SQLITE_PREPARE_SAVESQL as u32 | (prepFlags & SQLITE_PREPARE_MASK as u32),
        ppStmt,
        pzTail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_text() {
        assert_eq!(sqlText(b"select 1", -1), b"select 1");
        assert_eq!(sqlText(b"select 1\0junk", -1), b"select 1");
        assert_eq!(sqlText(b"select 1", 6), b"select");
        assert_eq!(sqlText(b"select 1", 100), b"select 1");
    }

    #[test]
    fn utf16() {
        let z: Vec<u8> = "sélect"
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .collect();
        assert_eq!(utf16ne_to_utf8(&z), "sélect".as_bytes());
    }
}
