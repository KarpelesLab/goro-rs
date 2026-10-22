//! Port of vacuum.c: the VACUUM and VACUUM INTO commands.

use crate::btree::*;
use crate::int::*;
use crate::printf::PfArg;

/// Execute zSql on database db.
///
/// If zSql returns rows, then each row will have exactly one
/// column.  (This will only happen if zSql begins with "SELECT".)
/// Take each row of result and call execSql() again recursively.
///
/// The execSqlF() routine does the same thing, except it accepts
/// a format string as its third argument
fn execSql(db: &mut Sqlite3, pzErrMsg: &mut Option<ZStr>, zSql: &[u8]) -> i32 {
    let mut pStmt: Option<StmtId> = None;
    let mut tail = 0usize;

    let mut rc = crate::prepare::sqlite3_prepare_v2(db, Some(zSql), -1, &mut pStmt, &mut tail);
    if rc != SQLITE_OK {
        return rc;
    }
    let stmt = match pStmt {
        Some(s) => s,
        // An empty statement: nothing to run (C sqlite3_step(0) is a no-op
        // returning SQLITE_MISUSE, which never happens with this SQL).
        None => return SQLITE_OK,
    };
    loop {
        rc = crate::vdbeapi::sqlite3_step(db, stmt);
        if rc != SQLITE_ROW {
            break;
        }
        let zSubSql = crate::vdbeapi::sqlite3_column_text(db, stmt, 0);
        debug_assert!(crate::util::sqlite3_strnicmp(Some(zSql), Some(b"SELECT"), 6) == 0);
        // The secondary SQL must be one of CREATE TABLE, CREATE INDEX,
        // or INSERT.  Historically there have been attacks that first
        // corrupt the sqlite_schema.sql field with other kinds of statements
        // then run VACUUM to get those statements to execute at
        // inappropriate times.
        if let Some(zSubSql) = zSubSql {
            if zSubSql.starts_with(b"CRE") || zSubSql.starts_with(b"INS") {
                rc = execSql(db, pzErrMsg, &zSubSql);
                if rc != SQLITE_OK {
                    break;
                }
            }
        }
    }
    debug_assert!(rc != SQLITE_ROW);
    if rc == SQLITE_DONE {
        rc = SQLITE_OK;
    }
    if rc != 0 {
        *pzErrMsg = crate::mainapi::sqlite3_errmsg(db);
    }
    crate::vdbeapi::sqlite3_finalize(db, stmt);
    rc
}

fn execSqlF(db: &mut Sqlite3, pzErrMsg: &mut Option<ZStr>, zSql: &str, args: &[PfArg]) -> i32 {
    let z = crate::printf::sqlite3VMPrintf(db, zSql.as_bytes(), args);
    execSql(db, pzErrMsg, &z)
}

/// C: `void sqlite3Vacuum(Parse *pParse, Token *pNm, Expr *pInto)`
///
/// The VACUUM command is used to clean up the database,
/// collapse free space, etc.  It is modelled after the VACUUM command
/// in PostgreSQL.  The VACUUM command works as follows:
///
///   (1)  Create a new transient database file
///   (2)  Copy all content from the database being vacuumed into
///        the new transient database file
///   (3)  Copy content from the transient database back into the
///        original database.
///
/// The transient database requires temporary disk space approximately
/// equal to the size of the original database.  The copy operation of
/// step (3) requires additional temporary disk space approximately equal
/// to the size of the original database for the rollback journal.
/// Hence, temporary disk space that is approximately 2x the size of the
/// original database is required.  Every page of the database is written
/// approximately 3 times:  Once for step (2) and twice for step (3).
/// Two writes per page are required in step (3) because the original
/// database content must be written into the rollback journal prior to
/// overwriting the database with the vacuumed content.
///
/// Only 1x temporary space and only 1x writes would be required if
/// the copy of step (3) were replaced by deleting the original database
/// and renaming the transient database as the original.  But that will
/// not work if other processes are attached to the original database.
/// And a power loss in between deleting the original and renaming the
/// transient would cause the database file to appear to be deleted
/// following reboot.
pub fn sqlite3Vacuum(db: &mut Sqlite3, pNm: Option<&Token>, pInto: ExprId) {
    vacuum_build(db, pNm, pInto);
    // build_vacuum_end:
    crate::expr::sqlite3ExprDelete(db, pInto);
}

fn vacuum_build(db: &mut Sqlite3, pNm: Option<&Token>, pInto: ExprId) {
    let mut iDb = 0;
    if !crate::select::sqlite3GetVdbe(db) {
        return;
    }
    if db.ppr().nErr != 0 {
        return;
    }
    if let Some(pNm) = pNm {
        // Default behavior:  Report an error if the argument to VACUUM is
        // not recognized
        let mut tok = Token::default();
        iDb = crate::build::sqlite3TwoPartName(db, pNm, pNm, &mut tok);
        if iDb < 0 {
            return;
        }
    }
    if iDb != 1 {
        let mut iIntoReg = 0;
        if pInto.nn()
            && crate::resolve::sqlite3ResolveSelfReference(db, None, 0, pInto, ExprListId::NULL)
                == 0
        {
            db.pp().nMem += 1;
            iIntoReg = db.ppr().nMem;
            crate::expr::sqlite3ExprCode(db, pInto, iIntoReg);
        }
        crate::vdbeaux::sqlite3VdbeAddOp2(db.v(), OP_Vacuum, iDb, iIntoReg);
        crate::vdbeaux::sqlite3VdbeUsesBtree(db.v(), iDb);
    }
}

/// C: `SQLITE_NOINLINE int sqlite3RunVacuum( char **pzErrMsg, sqlite3 *db, int iDb, sqlite3_value *pOut )`
///
/// This routine implements the OP_Vacuum opcode of the VDBE.
pub fn sqlite3RunVacuum(
    db: &mut Sqlite3,
    pzErrMsg: &mut Option<ZStr>,
    iDb: i32,
    pOut: Option<&mut Mem>,
) -> i32 {
    if db.autoCommit == 0 {
        *pzErrMsg = Some(b"cannot VACUUM from within a transaction".to_vec());
        return SQLITE_ERROR; // IMP: R-12218-18073
    }
    if db.nVdbeActive > 1 {
        *pzErrMsg = Some(b"cannot VACUUM - SQL statements in progress".to_vec());
        return SQLITE_ERROR; // IMP: R-15610-35227
    }
    let saved_openFlags = db.openFlags;
    let hasOut = pOut.is_some();
    let zOut: ZStr;
    if let Some(pOut) = pOut {
        if crate::vdbeapi::sqlite3_value_type(pOut) != SQLITE_TEXT {
            *pzErrMsg = Some(b"non-text filename".to_vec());
            return SQLITE_ERROR;
        }
        zOut = crate::vdbeapi::sqlite3_value_text(pOut)
            .map(|z| z.to_vec())
            .unwrap_or_default();
        db.openFlags &= !(SQLITE_OPEN_READONLY as u32);
        db.openFlags |= (SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE) as u32;
    } else {
        zOut = Vec::new();
    }

    // Save the current value of the database flags so that it can be
    // restored before returning. Then set the writable-schema flag, and
    // disable CHECK and foreign key constraints.
    let saved_flags = db.flags;
    let saved_mDbFlags = db.mDbFlags;
    let saved_nChange = db.nChange;
    let saved_nTotalChange = db.nTotalChange;
    let saved_mTrace = db.mTrace;
    db.flags |= SQLITE_WriteSchema
        | SQLITE_IgnoreChecks
        | SQLITE_Comments
        | SQLITE_AttachCreate
        | SQLITE_AttachWrite;
    db.mDbFlags |= DBFLAG_PreferBuiltin | DBFLAG_Vacuum;
    db.flags &= !(SQLITE_ForeignKeys | SQLITE_ReverseOrder | SQLITE_Defensive | SQLITE_CountRows);
    db.mTrace = 0;

    let zDbMain = db.aDb[iDb as usize].zDbSName.clone();
    let pMain = db.aDb[iDb as usize].pBt.clone().expect("pMain");
    let isMemDb = {
        let pager = sqlite3BtreePager(&pMain);
        let mut pg = pager.borrow_mut();
        crate::pager::sqlite3PagerIsMemdb(&mut pg) != 0
    };

    // Attach the temporary database as 'vacuum_XXXXXX'. The synchronous
    // pragma can be set to 'off' for this file, as it is not recovered if a
    // crash occurs anyway. The integrity of the database is maintained by a
    // (possibly synchronous) transaction opened on the main database before
    // sqlite3BtreeCopyFile() is called.
    //
    // An optimization would be to use a non-journaled pager.
    // (Later:) I tried setting "PRAGMA vacuum_XXXXXX.journal_mode=OFF" but
    // that actually made the VACUUM run slower.  Very little journalling
    // actually occurs when doing a vacuum since the vacuum_db is initially
    // empty.  Only the journal header is written.  Apparently it takes more
    // time to parse and run the PRAGMA to turn journalling off than it does
    // to write the journal header file.
    let mut aRandom = [0u8; 8];
    crate::random::sqlite3_randomness(8, &mut aRandom);
    let iRandom = u64::from_ne_bytes(aRandom);
    let zDbVacuum: ZStr =
        crate::printf::sqlite3_snprintf(42, b"vacuum_%016llx", &[PfArg::from(iRandom)]);
    let nDb = db.nDb();
    let mut pDbIdx: Option<usize> = None; // Database to detach at end of vacuum

    let rc = run_vacuum_body(
        db,
        pzErrMsg,
        iDb,
        hasOut,
        &zOut,
        zDbMain.as_deref(),
        &pMain,
        isMemDb,
        &zDbVacuum,
        nDb,
        saved_openFlags,
        &mut pDbIdx,
    );

    // end_of_vacuum:
    // Restore the original value of db->flags
    db.init.iDb = 0;
    db.mDbFlags = saved_mDbFlags;
    db.flags = saved_flags;
    db.nChange = saved_nChange;
    db.nTotalChange = saved_nTotalChange;
    db.mTrace = saved_mTrace;
    sqlite3BtreeSetPageSize(&pMain, -1, 0, 1);

    // Currently there is an SQL level transaction open on the vacuum
    // database. No locks are held on any other files (since the main file
    // was committed at the btree level). So it safe to end the transaction
    // by manually setting the autoCommit flag to true and detaching the
    // vacuum database. The vacuum_db journal file is deleted when the pager
    // is closed by the DETACH.
    db.autoCommit = 1;

    if let Some(i) = pDbIdx {
        if let Some(pBt) = db.aDb[i].pBt.take() {
            sqlite3BtreeClose(db, &pBt);
        }
    }

    // This both clears the schemas and reduces the size of the db->aDb[]
    // array.
    crate::build::sqlite3ResetAllSchemasOfConnection(db);

    rc
}

/// The part of sqlite3RunVacuum() between the ATTACH and end_of_vacuum.
#[allow(clippy::too_many_arguments)]
fn run_vacuum_body(
    db: &mut Sqlite3,
    pzErrMsg: &mut Option<ZStr>,
    iDb: i32,
    hasOut: bool,
    zOut: &[u8],
    zDbMain: Option<&[u8]>,
    pMain: &BtreeP,
    isMemDb: bool,
    zDbVacuum: &[u8],
    nDb: i32,
    saved_openFlags: u32,
    pDbIdx: &mut Option<usize>,
) -> i32 {
    let mut pgflags: u32 = PAGER_SYNCHRONOUS_OFF as u32; // sync flags for output db

    let mut rc = execSqlF(
        db,
        pzErrMsg,
        "ATTACH %Q AS %s",
        &[PfArg::from(zOut), PfArg::from(zDbVacuum)],
    );
    db.openFlags = saved_openFlags;
    if rc != SQLITE_OK {
        return rc;
    }
    debug_assert!((db.nDb() - 1) == nDb);
    *pDbIdx = Some(nDb as usize);
    let pTemp = db.aDb[nDb as usize].pBt.clone().expect("pTemp");
    let mut nRes = sqlite3BtreeGetRequestedReserve(pMain);
    if hasOut {
        let pager = sqlite3BtreePager(&pTemp);
        let exists = {
            let mut pg = pager.borrow_mut();
            let id = crate::pager::sqlite3PagerFile(&mut pg);
            let mut sz = 0i64;
            crate::os::isOpen(id) && (crate::os::sqlite3OsFileSize(id, &mut sz) != SQLITE_OK || sz > 0)
        };
        if exists {
            *pzErrMsg = Some(b"output file already exists".to_vec());
            return SQLITE_ERROR;
        }
        db.mDbFlags |= DBFLAG_VacuumInto;

        // For a VACUUM INTO, the pager-flags are set to the same values as
        // they are for the database being vacuumed, except that
        // PAGER_CACHESPILL is always set.
        pgflags =
            db.aDb[iDb as usize].safety_level as u32 | (db.flags & PAGER_FLAGS_MASK as u64) as u32;

        // If the VACUUM INTO target file is a URI filename and if the
        // "reserve=N" query parameter is present, reset the reserve to the
        // amount specified, if the amount is within range
        let zFilename = sqlite3BtreeGetFilename(&pTemp);
        if zFilename.is_some() {
            let nNew = crate::mainapi::sqlite3_uri_int64(
                zFilename.as_deref(),
                Some(b"reserve"),
                nRes as i64,
            ) as i32;
            if (0..=255).contains(&nNew) {
                nRes = nNew;
            }
        }
    }

    let cache_size = db.aDb[iDb as usize].pSchema.cache_size;
    sqlite3BtreeSetCacheSize(db, &pTemp, cache_size);
    let spill = sqlite3BtreeSetSpillSize(db, pMain, 0);
    sqlite3BtreeSetSpillSize(db, &pTemp, spill);
    sqlite3BtreeSetPagerFlags(db, &pTemp, pgflags | PAGER_CACHESPILL as u32);

    // Begin a transaction and take an exclusive lock on the main database
    // file. This is done before the sqlite3BtreeGetPageSize(pMain) call
    // below, to ensure that we do not try to change the page-size on a WAL
    // database.
    rc = execSql(db, pzErrMsg, b"BEGIN");
    if rc != SQLITE_OK {
        return rc;
    }
    let mut dummy = 0;
    rc = sqlite3BtreeBeginTrans(db, pMain, if !hasOut { 2 } else { 0 }, &mut dummy);
    if rc != SQLITE_OK {
        return rc;
    }

    // Do not attempt to change the page size for a WAL database
    let jmode = {
        let pager = sqlite3BtreePager(pMain);
        let mut pg = pager.borrow_mut();
        crate::pager::sqlite3PagerGetJournalMode(&mut pg)
    };
    if jmode == PAGER_JOURNALMODE_WAL && !hasOut {
        db.nextPagesize = 0;
    }

    if sqlite3BtreeSetPageSize(&pTemp, sqlite3BtreeGetPageSize(pMain), nRes, 0) != 0
        || (!isMemDb && sqlite3BtreeSetPageSize(&pTemp, db.nextPagesize, nRes, 0) != 0)
        || db.mallocFailed != 0
    {
        return SQLITE_NOMEM;
    }

    let av = if db.nextAutovac >= 0 {
        db.nextAutovac as i32
    } else {
        sqlite3BtreeGetAutoVacuum(pMain)
    };
    sqlite3BtreeSetAutoVacuum(&pTemp, av);

    // Query the schema of the main database. Create a mirror schema
    // in the temporary database.
    db.init.iDb = nDb as u8; // force new CREATE statements into vacuum_db
    rc = execSqlF(
        db,
        pzErrMsg,
        "SELECT sql FROM \"%w\".sqlite_schema WHERE type='table'AND name<>'sqlite_sequence' AND coalesce(rootpage,1)>0",
        &[PfArg::from(zDbMain)],
    );
    if rc != SQLITE_OK {
        return rc;
    }
    rc = execSqlF(
        db,
        pzErrMsg,
        "SELECT sql FROM \"%w\".sqlite_schema WHERE type='index'",
        &[PfArg::from(zDbMain)],
    );
    if rc != SQLITE_OK {
        return rc;
    }
    db.init.iDb = 0;

    // Loop through the tables in the main database. For each, do
    // an "INSERT INTO vacuum_db.xxx SELECT * FROM main.xxx;" to copy
    // the contents to the temporary database.
    rc = execSqlF(
        db,
        pzErrMsg,
        "SELECT'INSERT INTO %s.'||quote(name)||' SELECT*FROM\"%w\".'||quote(name)FROM %s.sqlite_schema WHERE type='table'AND coalesce(rootpage,1)>0",
        &[
            PfArg::from(zDbVacuum),
            PfArg::from(zDbMain),
            PfArg::from(zDbVacuum),
        ],
    );
    debug_assert!((db.mDbFlags & DBFLAG_Vacuum) != 0);
    db.mDbFlags &= !DBFLAG_Vacuum;
    if rc != SQLITE_OK {
        return rc;
    }

    // Copy the triggers, views, and virtual tables from the main database
    // over to the temporary database.  None of these objects has any
    // associated storage, so all we have to do is copy their entries
    // from the schema table.
    rc = execSqlF(
        db,
        pzErrMsg,
        "INSERT INTO %s.sqlite_schema SELECT*FROM \"%w\".sqlite_schema WHERE type IN('view','trigger') OR(type='table'AND rootpage=0)",
        &[PfArg::from(zDbVacuum), PfArg::from(zDbMain)],
    );
    if rc != 0 {
        return rc;
    }

    // At this point, there is a write transaction open on both the
    // vacuum database and the main database. Assuming no error occurs,
    // both transactions are closed by this block - the main database
    // transaction by sqlite3BtreeCopyFile() and the other by an explicit
    // call to sqlite3BtreeCommit().
    {
        // This array determines which meta meta values are preserved in the
        // vacuum.  Even entries are the meta value number and odd entries
        // are an increment to apply to the meta value after the vacuum.
        // The increment is used to increase the schema cookie so that other
        // connections to the same database will know to reread the schema.
        const aCopy: [i32; 10] = [
            BTREE_SCHEMA_VERSION,
            1, // Add one to the old schema cookie
            BTREE_DEFAULT_CACHE_SIZE,
            0, // Preserve the default page cache size
            BTREE_TEXT_ENCODING,
            0, // Preserve the text encoding
            BTREE_USER_VERSION,
            0, // Preserve the user version
            BTREE_APPLICATION_ID,
            0, // Preserve the application id
        ];

        debug_assert!(SQLITE_TXN_WRITE == sqlite3BtreeTxnState(db, &pTemp));
        debug_assert!(hasOut || SQLITE_TXN_WRITE == sqlite3BtreeTxnState(db, pMain));

        // Copy Btree meta values
        let mut i = 0;
        while i < aCopy.len() {
            // GetMeta() and UpdateMeta() cannot fail in this context because
            // we already have page 1 loaded into cache and marked dirty.
            let mut meta = 0u32;
            sqlite3BtreeGetMeta(pMain, aCopy[i], &mut meta);
            rc = sqlite3BtreeUpdateMeta(&pTemp, aCopy[i], meta.wrapping_add(aCopy[i + 1] as u32));
            if rc != SQLITE_OK {
                return rc;
            }
            i += 2;
        }

        if !hasOut {
            rc = crate::backup::sqlite3BtreeCopyFile(db, pMain, &pTemp);
        }
        if rc != SQLITE_OK {
            return rc;
        }
        rc = sqlite3BtreeCommit(&pTemp);
        if rc != SQLITE_OK {
            return rc;
        }
        if !hasOut {
            sqlite3BtreeSetAutoVacuum(pMain, sqlite3BtreeGetAutoVacuum(&pTemp));
        }
    }

    debug_assert!(rc == SQLITE_OK);
    if !hasOut {
        nRes = sqlite3BtreeGetRequestedReserve(&pTemp);
        rc = sqlite3BtreeSetPageSize(pMain, sqlite3BtreeGetPageSize(&pTemp), nRes, 1);
    }
    rc
}
