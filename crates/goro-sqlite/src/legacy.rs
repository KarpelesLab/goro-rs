//! Port of legacy.c.
//!
//! Main file for the SQLite library.  The routines in this file
//! implement the programmer interface to the library.  Routines in
//! other files are for internal use by SQLite and should not be
//! accessed by users of the library.

use crate::global::sqlite3Isspace;
use crate::int::*;
use crate::util::sqlite3Error;
use crate::vdbeapi::{
    sqlite3_column_count, sqlite3_column_name, sqlite3_column_text, sqlite3_column_type,
    sqlite3_step,
};

/// Callback of sqlite3_exec_db(): (db, argv, azColName) -> nonzero to abort.
/// `argv` is None when the callback is invoked for a statement that
/// returned no rows (SQLITE_NullCallback, C argv==NULL).
pub type ExecCallback<'a> =
    &'a mut dyn FnMut(&mut Sqlite3, Option<&[Option<&[u8]>]>, &[&[u8]]) -> i32;

/// Rust: finalize the statement `id` (C `sqlite3VdbeFinalize((Vdbe*)pStmt)`):
/// the statement is removed from its slot and finalized.
fn vdbeFinalizeStmt(db: &mut Sqlite3, id: StmtId) -> i32 {
    match db.stmt_remove(id) {
        Some(mut v) => crate::vdbeaux::sqlite3VdbeFinalize(db, &mut v),
        None => SQLITE_OK,
    }
}

/// C: `int sqlite3_exec( sqlite3 *db, const char *zSql, sqlite3_callback xCallback, void *pArg, char **pzErrMsg )`
///
/// Execute SQL code.  Return one of the SQLITE_ success/failure
/// codes.  Also write an error message into memory obtained from
/// malloc() and make *pzErrMsg point to that message.
///
/// If the SQL is a query, then for each row in the query result
/// the xCallback() function is called.  If xCallback=NULL then no
/// callback is invoked, even for queries.
///
/// Rust: the callback receives the column values (None for SQL NULL)
/// and the column names; for the "empty result" callback
/// (SQLITE_NullCallback) the values slice is empty.
pub fn sqlite3_exec(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    xCallback: Option<&mut dyn FnMut(&[Option<&[u8]>], &[&[u8]]) -> i32>,
    pzErrMsg: &mut Option<ZStr>,
) -> i32 {
    match xCallback {
        Some(cb) => {
            let mut f = |_db: &mut Sqlite3, argv: Option<&[Option<&[u8]>]>, cols: &[&[u8]]| {
                cb(argv.unwrap_or(&[]), cols)
            };
            sqlite3_exec_db(db, zSql, Some(&mut f), Some(pzErrMsg))
        }
        None => sqlite3_exec_db(db, zSql, None, Some(pzErrMsg)),
    }
}

/// Rust: sqlite3_exec() for internal callers whose callback needs the
/// connection (sqlite3InitCallback, analysisLoader, ...).  `pzErrMsg` is
/// None for the C NULL pointer.
pub fn sqlite3_exec_db(
    db: &mut Sqlite3,
    zSql: Option<&[u8]>,
    mut xCallback: Option<ExecCallback<'_>>,
    pzErrMsg: Option<&mut Option<ZStr>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut pStmt: Option<StmtId> = None;
    let mut azCols: Vec<ZStr> = Vec::new();
    let mut callbackIsInit;

    if crate::util::sqlite3SafetyCheckOk(db) == 0 {
        return SQLITE_MISUSE;
    }
    let zAll: &[u8] = zSql.unwrap_or(b"");
    // Rust: the SQL text ends at the end of the slice or at the first NUL.
    let zAll = match zAll.iter().position(|&c| c == 0) {
        Some(n) => &zAll[..n],
        None => zAll,
    };
    let mut iSql = 0usize;

    sqlite3Error(db, SQLITE_OK);
    'outer: while rc == SQLITE_OK && iSql < zAll.len() {
        let mut nCol = 0;
        let zCur = &zAll[iSql..];
        let mut zLeftover = 0usize;

        pStmt = None;
        rc = crate::prepare::sqlite3_prepare_v2(db, Some(zCur), -1, &mut pStmt, &mut zLeftover);
        debug_assert!(rc == SQLITE_OK || pStmt.is_none());
        if rc != SQLITE_OK {
            continue;
        }
        let Some(id) = pStmt else {
            // this happens for a comment or white-space
            iSql += zLeftover;
            continue;
        };
        callbackIsInit = false;

        loop {
            rc = sqlite3_step(db, id);

            // Invoke the callback function if required
            if let Some(cb) = xCallback.as_mut() {
                if SQLITE_ROW == rc
                    || (SQLITE_DONE == rc
                        && !callbackIsInit
                        && (db.flags & SQLITE_NullCallback) != 0)
                {
                    if !callbackIsInit {
                        nCol = sqlite3_column_count(db, id);
                        azCols.clear();
                        for i in 0..nCol {
                            // sqlite3VdbeSetColName() installs column names as UTF8
                            // strings so there is no way for sqlite3_column_name() to fail.
                            azCols.push(sqlite3_column_name(db, id, i).unwrap_or_default());
                        }
                        callbackIsInit = true;
                    }
                    let mut azVals: Vec<Option<ZStr>> = Vec::new();
                    let have_vals = rc == SQLITE_ROW;
                    if have_vals {
                        for i in 0..nCol {
                            let z = sqlite3_column_text(db, id, i);
                            if z.is_none() && sqlite3_column_type(db, id, i) != SQLITE_NULL {
                                // sqlite3OomFault(db)
                                break 'outer;
                            }
                            azVals.push(z);
                        }
                    }
                    let vals: Vec<Option<&[u8]>> = azVals.iter().map(|v| v.as_deref()).collect();
                    let cols: Vec<&[u8]> = azCols.iter().map(|c| c.as_slice()).collect();
                    let r = cb(db, if have_vals { Some(&vals) } else { None }, &cols);
                    if r != 0 {
                        // EVIDENCE-OF: R-38229-40159 If the callback function to
                        // sqlite3_exec() returns non-zero, then sqlite3_exec() will
                        // return SQLITE_ABORT.
                        rc = SQLITE_ABORT;
                        vdbeFinalizeStmt(db, id);
                        pStmt = None;
                        sqlite3Error(db, SQLITE_ABORT);
                        break 'outer;
                    }
                }
            }

            if rc != SQLITE_ROW {
                rc = vdbeFinalizeStmt(db, id);
                pStmt = None;
                iSql += zLeftover;
                while iSql < zAll.len() && sqlite3Isspace(zAll[iSql]) {
                    iSql += 1;
                }
                break;
            }
        }

        azCols.clear();
    }

    // exec_out:
    if let Some(id) = pStmt {
        vdbeFinalizeStmt(db, id);
    }

    rc = crate::mainapi::sqlite3ApiExit(db, rc);
    if let Some(pzErrMsg) = pzErrMsg {
        if rc != SQLITE_OK {
            *pzErrMsg = crate::mainapi::sqlite3_errmsg(db);
        } else {
            *pzErrMsg = None;
        }
    }

    debug_assert!((rc & db.errMask) == rc);
    rc
}
