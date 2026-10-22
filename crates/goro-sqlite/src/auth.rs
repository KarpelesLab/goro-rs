//! Port of auth.c: the sqlite3_set_authorizer() API and the code that
//! invokes the authorizer callback while statements are compiled.
//!
//! All of the code in this file may be omitted by defining
//! SQLITE_OMIT_AUTHORIZATION in C; the port always includes it.

use crate::int::*;
use crate::util::sqlite3ErrorMsg;
use crate::vdbeaux::sqlite3ExpirePreparedStatements;

/// C: `int sqlite3_set_authorizer( sqlite3 *db, int (*xAuth)(void*,int,const char*,const char*,const char*,const char*), void *pArg )`
///
/// Set or clear the access authorization function.
///
/// The access authorization function is called during the compilation
/// phase to verify that the user has read and/or write access permission
/// on various fields of the database.  The first argument to the auth
/// function is a copy of the 3rd argument to this routine.  The second
/// argument to the auth function is one of these constants: SQLITE_CREATE_INDEX
/// ... SQLITE_DROP_VIEW.  The callback returns SQLITE_OK, SQLITE_DENY or
/// SQLITE_IGNORE.  Any other return value is an "authorizer malfunction".
///
/// Rust: the user data (C pArg) is captured by the closure.
pub fn sqlite3_set_authorizer(db: &mut Sqlite3, xAuth: Option<XAuth>) -> i32 {
    db.xAuth = xAuth;
    sqlite3ExpirePreparedStatements(db, 1);
    SQLITE_OK
}

/// Write an error message into pParse->zErrMsg that explains that the
/// user-supplied authorization function returned an illegal value.
fn sqliteAuthBadReturnCode(db: &mut Sqlite3) {
    sqlite3ErrorMsg(db, b"authorizer malfunction", &[]);
    db.pp().rc = SQLITE_ERROR;
}

/// C: `int sqlite3AuthReadCol( Parse *pParse, const char *zTab, const char *zCol, int iDb )`
///
/// Invoke the authorization callback for permission to read column zCol from
/// table zTab in database zDb. This function assumes that an authorization
/// callback has been registered (i.e. that sqlite3.xAuth is not NULL).
///
/// If SQLITE_IGNORE is returned and pExpr is not NULL, then pExpr is changed
/// to an SQL NULL expression. Otherwise, if pExpr is NULL, then SQLITE_IGNORE
/// is treated as SQLITE_DENY. In this case an error is left in pParse.
pub fn sqlite3AuthReadCol(
    db: &mut Sqlite3,
    zTab: Option<&[u8]>,
    zCol: Option<&[u8]>,
    iDb: i32,
) -> i32 {
    let zDb = db.aDb[iDb as usize].zDbSName.clone();
    if db.init.busy != 0 {
        return SQLITE_OK;
    }
    let xAuth = match db.xAuth.clone() {
        Some(x) => x,
        None => return SQLITE_OK,
    };
    let zCtx = db.ppr().zAuthContext.clone();
    let rc = xAuth(SQLITE_READ, zTab, zCol, zDb.as_deref(), zCtx.as_deref());
    if rc == SQLITE_DENY {
        let mut z = crate::printf::sqlite3_mprintf(b"%s.%s", &[zTab.into(), zCol.into()]);
        if db.nDb() > 2 || iDb != 0 {
            z = crate::printf::sqlite3_mprintf(b"%s.%s", &[zDb.as_deref().into(), (&z).into()]);
        }
        errmsg!(db, "access to %s is prohibited", &z);
        db.pp().rc = SQLITE_AUTH;
    } else if rc != SQLITE_IGNORE && rc != SQLITE_OK {
        sqliteAuthBadReturnCode(db);
    }
    rc
}

/// C: `void sqlite3AuthRead( Parse *pParse, Expr *pExpr, Schema *pSchema, SrcList *pTabList )`
///
/// The pExpr should be a TK_COLUMN expression.  The table referred to
/// is in pTabList or else it is the NEW or OLD table of a trigger.
/// Check to see if it is OK to read this particular column.
///
/// If the auth function returns SQLITE_IGNORE, change the TK_COLUMN
/// instruction into a TK_NULL.  If the auth function returns SQLITE_DENY,
/// then generate an error.
///
/// Rust: the schema is identified by its SchemaId (None for the C NULL
/// pointer, i.e. a subquery or other temporary table).
pub fn sqlite3AuthRead(
    db: &mut Sqlite3,
    pExpr: ExprId,
    pSchema: Option<SchemaId>,
    pTabList: SrcListId,
) {
    let (op, iTable, iCol) = {
        let e = db.ex(pExpr);
        (e.op, e.iTable, e.iColumn)
    };
    debug_assert!(op == TK_COLUMN || op == TK_TRIGGER);
    debug_assert!(!db.ppr().IN_RENAME_OBJECT());
    debug_assert!(db.xAuth.is_some());
    let iDb = match pSchema {
        Some(s) => db.schema_index(s),
        None => -1,
    };
    if iDb < 0 {
        // An attempt to read a column out of a subquery or other
        // temporary table.
        return;
    }

    let mut pTab: Option<TableRef> = None;
    if op == TK_TRIGGER {
        pTab = db.ppr().pTriggerTab.clone();
    } else {
        debug_assert!(pTabList.nn());
        let n = db.src(pTabList).nSrc();
        for iSrc in 0..n {
            let it = db.item(SrcItemId(pTabList, iSrc as u32));
            if iTable == it.iCursor {
                pTab = it.pSTab.clone();
                break;
            }
        }
    }
    let pTab = match pTab {
        Some(t) => t,
        None => return,
    };
    let (zTab, zCol) = {
        let t = pTab.borrow();
        let zCol: ZStr = if iCol >= 0 {
            debug_assert!((iCol as i32) < t.nCol as i32);
            t.aCol[iCol as usize].zCnName.clone()
        } else if t.iPKey >= 0 {
            debug_assert!(t.iPKey < t.nCol);
            t.aCol[t.iPKey as usize].zCnName.clone()
        } else {
            b"ROWID".to_vec()
        };
        (t.zName.clone(), zCol)
    };
    debug_assert!(iDb >= 0 && iDb < db.nDb());
    if SQLITE_IGNORE == sqlite3AuthReadCol(db, Some(&zTab), Some(&zCol), iDb) {
        db.exm(pExpr).op = TK_NULL;
    }
}

/// Do an authorization check using the code and arguments given.  Return
/// either SQLITE_OK (zero) or SQLITE_IGNORE or SQLITE_DENY.  If SQLITE_DENY
/// is returned, then the error count and error message in pParse are
/// modified appropriately.
fn realAuthCheck(
    db: &mut Sqlite3,
    code: i32,
    zArg1: Option<&[u8]>,
    zArg2: Option<&[u8]>,
    zArg3: Option<&[u8]>,
) -> i32 {
    // Don't do any authorization checks if the database is initializing
    // or if the parser is being invoked from within sqlite3_declare_vtab.
    debug_assert!(!db.ppr().IN_RENAME_OBJECT() || db.xAuth.is_none());
    if db.ppr().IN_SPECIAL_PARSE() {
        return SQLITE_OK;
    }
    let xAuth = match db.xAuth.clone() {
        Some(x) => x,
        None => return SQLITE_OK,
    };
    let zCtx = db.ppr().zAuthContext.clone();
    let mut rc = xAuth(code, zArg1, zArg2, zArg3, zCtx.as_deref());
    if rc == SQLITE_DENY {
        sqlite3ErrorMsg(db, b"not authorized", &[]);
        db.pp().rc = SQLITE_AUTH;
    } else if rc != SQLITE_OK && rc != SQLITE_IGNORE {
        rc = SQLITE_DENY;
        sqliteAuthBadReturnCode(db);
    }
    rc
}

/// C: `int sqlite3AuthCheck( Parse *pParse, int code, const char *zArg1, const char *zArg2, const char *zArg3 )`
pub fn sqlite3AuthCheck(
    db: &mut Sqlite3,
    code: i32,
    zArg1: Option<&[u8]>,
    zArg2: Option<&[u8]>,
    zArg3: Option<&[u8]>,
) -> i32 {
    if db.xAuth.is_some() && db.init.busy == 0 {
        realAuthCheck(db, code, zArg1, zArg2, zArg3)
    } else {
        SQLITE_OK
    }
}

/// C: `void sqlite3AuthContextPush( Parse *pParse, AuthContext *pContext, const char *zContext )`
///
/// Push an authorization context.  After this routine is called, the
/// zArg3 argument to authorization callbacks will be zContext until
/// popped.  Or if pParse==0, this routine is a no-op.
pub fn sqlite3AuthContextPush(
    db: &mut Sqlite3,
    pContext: &mut AuthContext,
    zContext: Option<&[u8]>,
) {
    let i = db.parses.len() - 1;
    pContext.pParse = Some(i);
    pContext.zAuthContext = db.parses[i].zAuthContext.take();
    db.parses[i].zAuthContext = zContext.map(|z| z.to_vec());
}

/// C: `void sqlite3AuthContextPop(AuthContext *pContext)`
///
/// Pop an authorization context that was previously pushed
/// by sqlite3AuthContextPush.
///
/// Rust: takes the connection (the C AuthContext points to its Parse).
pub fn sqlite3AuthContextPop(db: &mut Sqlite3, pContext: &mut AuthContext) {
    if let Some(i) = pContext.pParse.take() {
        if i < db.parses.len() {
            db.parses[i].zAuthContext = pContext.zAuthContext.take();
        }
    }
}
