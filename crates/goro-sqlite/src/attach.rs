//! Port of attach.c.
//!
//! This file contains code used to implement the ATTACH and DETACH commands
//! and the DbFixer routines that bind the names of a view, trigger or
//! index to a single database.

use crate::int::*;
use crate::util::sqlite3StrICmp;
use crate::vdbeapi::{sqlite3_result_error, sqlite3_result_error_code, sqlite3_value_text};
use crate::walker::{
    sqlite3WalkExpr, sqlite3WalkExprList, sqlite3WalkSelect, sqlite3WalkWinDefnDummyCallback,
};
use std::rc::Rc;

/// Resolve an expression that was part of an ATTACH or DETACH statement. This
/// is slightly different from resolving a normal SQL expression, because simple
/// identifiers are treated as strings, not possible column names or aliases.
///
/// i.e. if the parser sees:
///
///     ATTACH DATABASE abc AS def
///
/// it treats the two expressions as literal strings 'abc' and 'def' instead of
/// looking for columns of the same name.
///
/// This only applies to the root node of pExpr, so the statement:
///
///     ATTACH DATABASE abc||def AS 'db2'
///
/// will fail because neither abc or def can be resolved.
fn resolveAttachExpr(db: &mut Sqlite3, pName: NcId, pExpr: ExprId) -> i32 {
    let mut rc = SQLITE_OK;
    if pExpr.nn() {
        if db.ex(pExpr).op != TK_ID {
            rc = crate::resolve::sqlite3ResolveExprNames(db, pName, pExpr);
        } else {
            db.exm(pExpr).op = TK_STRING;
        }
    }
    rc
}

/// C: `int sqlite3DbIsNamed(sqlite3 *db, int iDb, const char *zName)`
///
/// Return true if zName points to a name that may be used to refer to
/// database iDb attached to handle db.
pub fn sqlite3DbIsNamed(db: &mut Sqlite3, iDb: i32, zName: Option<&[u8]>) -> i32 {
    (sqlite3StrICmp(db.aDb[iDb as usize].zDbSName.as_deref(), zName) == 0
        || (iDb == 0 && sqlite3StrICmp(Some(b"main"), zName) == 0)) as i32
}

/// Make sure the TEMP schema has no triggers that refer to the schema
/// pSchema (which is about to be detached or replaced): such triggers are
/// re-pointed at their own (TEMP) schema.
fn detachTempTriggers(db: &mut Sqlite3, pSchema: SchemaId) {
    for pTrig in db.aDb[1].pSchema.trigHash.values() {
        let mut t = pTrig.borrow_mut();
        if t.pTabSchema == pSchema {
            t.pTabSchema = t.pSchema;
        }
    }
}

/// An SQL user-function registered to do the work of an ATTACH statement. The
/// three arguments to the function come directly from an attach statement:
///
///     ATTACH DATABASE x AS y KEY z
///
///     SELECT sqlite_attach(x, y, z)
///
/// If the optional "KEY z" syntax is omitted, an SQL NULL is passed as the
/// third argument.
///
/// If the db->init.reopenMemdb flags is set, then instead of attaching a
/// new database, close the database on db->init.iDb and reopen it as an
/// empty MemDB.
fn attachFunc(context: &mut Context, argv: &mut [Mem]) {
    let mut rc: i32 = 0;
    let mut zErrDyn: Option<ZStr> = None;
    let mut zErr: Option<ZStr> = None;
    let mut zPath: Option<ZStr> = None;

    let zFile: ZStr = sqlite3_value_text(&mut argv[0])
        .map(|z| z.to_vec())
        .unwrap_or_default();
    let zName: ZStr = sqlite3_value_text(&mut argv[1])
        .map(|z| z.to_vec())
        .unwrap_or_default();

    'attach_error: {
        let iNew: usize;
        if context.db.init.reopenMemdb != 0 {
            // This is not a real ATTACH.  Instead, this routine is being called
            // from sqlite3_deserialize() to close database db->init.iDb and
            // reopen it as a MemDB
            let db = &mut *context.db;
            iNew = db.init.iDb as usize;
            let pBt = db.aDb[iNew].pBt.clone().expect("btree");
            if crate::btree::sqlite3BtreeTxnState(db, &pBt) != SQLITE_TXN_NONE
                || crate::btree::sqlite3BtreeIsInBackup(db, &pBt) != 0
            {
                rc = SQLITE_BUSY;
                break 'attach_error;
            }
            let pVfs = match crate::os::sqlite3_vfs_find(Some(b"memdb")) {
                Some(v) => Rc::from(v),
                None => return,
            };
            let mut pNewBt: Option<BtreeP> = None;
            rc = crate::btree::sqlite3BtreeOpen(
                db,
                &pVfs,
                Some(b"x\0"),
                &mut pNewBt,
                0,
                SQLITE_OPEN_MAIN_DB,
            );
            if rc == SQLITE_OK {
                let pNewBt = pNewBt.expect("btree");
                let pNewSchema = crate::callback::sqlite3SchemaGet(db, Some(&pNewBt));
                // Both the Btree and the new Schema were allocated successfully.
                // Close the old db and update the aDb[] slot with the new memdb
                // values.
                let oldId = db.aDb[iNew].pSchema.id;
                detachTempTriggers(db, oldId);
                crate::callback::sqlite3SchemaClear(db, iNew as i32);
                crate::btree::sqlite3BtreeClose(db, &pBt);
                db.aDb[iNew].pBt = Some(pNewBt);
                db.aDb[iNew].pSchema = pNewSchema;
                if db.init.iDb == 0 {
                    // Clear all eponymous virtual table instances, as they are
                    // holding pointers to the schema object just freed.
                    crate::vtab::sqlite3VtabEponymousTableClearAll(db);
                }
                crate::vdbeaux::sqlite3ExpirePreparedStatements(db, 1);
            }
            if rc != 0 {
                break 'attach_error;
            }
        } else {
            let db = &mut *context.db;
            // This is a real ATTACH
            //
            // Check for the following errors:
            //
            //     * Too many attached databases,
            //     * Transaction currently open
            //     * Specified database name already being used.
            if db.nDb() >= db.aLimit[SQLITE_LIMIT_ATTACHED as usize] + 2 {
                let lim = db.aLimit[SQLITE_LIMIT_ATTACHED as usize];
                zErrDyn = Some(mprintf!(db, "too many attached databases - max %d", lim));
                break 'attach_error;
            }
            for i in 0..db.nDb() {
                if sqlite3DbIsNamed(db, i, Some(&zName)) != 0 {
                    zErrDyn = Some(mprintf!(db, "database %s is already in use", &zName));
                    break 'attach_error;
                }
            }

            // Allocate the new entry in the db->aDb[] array and initialize the
            // schema hash tables.
            let mut flags: u32 = db.openFlags;
            let mut pVfs: Option<Box<crate::os::Vfs>> = None;
            rc = crate::mainapi::sqlite3ParseUri(
                None,
                Some(&zFile),
                &mut flags,
                &mut pVfs,
                &mut zPath,
                &mut zErr,
            );
            if rc != SQLITE_OK {
                let z = zErr.take();
                sqlite3_result_error(context, z.as_deref(), -1);
                return;
            }
            if (db.flags & SQLITE_AttachWrite) == 0 {
                flags &= !((SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE) as u32);
                flags |= SQLITE_OPEN_READONLY as u32;
            } else if (db.flags & SQLITE_AttachCreate) == 0 {
                flags &= !(SQLITE_OPEN_CREATE as u32);
            }
            let pVfs: Rc<crate::os::Vfs> = match pVfs {
                Some(v) => Rc::from(v),
                None => db.pVfs.clone(),
            };
            flags |= SQLITE_OPEN_MAIN_DB as u32;
            let mut pBt: Option<BtreeP> = None;
            rc = crate::btree::sqlite3BtreeOpen(
                db,
                &pVfs,
                zPath.as_deref(),
                &mut pBt,
                0,
                flags as i32,
            );
            db.aDb.push(Db {
                zDbSName: Some(zName.clone()),
                pBt,
                safety_level: 0,
                bSyncSet: 0,
                pSchema: Schema::default(),
            });
            iNew = db.aDb.len() - 1;
        }
        let db = &mut *context.db;
        db.noSharedCache = 0;
        if rc == SQLITE_CONSTRAINT {
            rc = SQLITE_ERROR;
            zErrDyn = Some(mprintf!(db, "database is already attached"));
        } else if rc == SQLITE_OK {
            let pBt = db.aDb[iNew].pBt.clone().expect("btree");
            db.aDb[iNew].pSchema = crate::callback::sqlite3SchemaGet(db, Some(&pBt));
            let (ff, enc) = (db.aDb[iNew].pSchema.file_format, db.aDb[iNew].pSchema.enc);
            if ff != 0 && enc != db.ENC() {
                zErrDyn = Some(mprintf!(
                    db,
                    "attached databases must use the same text encoding as main database"
                ));
                rc = SQLITE_ERROR;
            }
            let pPager = crate::btree::sqlite3BtreePager(&pBt);
            crate::pager::sqlite3PagerLockingMode(&mut pPager.borrow_mut(), db.dfltLockMode as i32);
            let main = db.aDb[0].pBt.clone().expect("main btree");
            crate::btree::sqlite3BtreeSecureDelete(
                &pBt,
                crate::btree::sqlite3BtreeSecureDelete(&main, -1),
            );
            let pgFlags =
                PAGER_SYNCHRONOUS_FULL as u32 | (db.flags & PAGER_FLAGS_MASK as u64) as u32;
            crate::btree::sqlite3BtreeSetPagerFlags(db, &pBt, pgFlags);
        }
        db.aDb[iNew].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;
        crate::mainapi::sqlite3_free_filename(zPath.as_deref());

        // If the file was opened successfully, read the schema for the new
        // database.  If this fails, or if opening the file failed, then close
        // the file and remove the entry from the db->aDb[] array. i.e. put
        // everything back the way we found it.
        if rc == SQLITE_OK {
            db.init.iDb = 0;
            db.mDbFlags &= !DBFLAG_SchemaKnownOk;
            if db.init.reopenMemdb == 0 {
                rc = crate::prepare::sqlite3Init(db, &mut zErrDyn);
            }
            debug_assert!(zErrDyn.is_none() || rc != SQLITE_OK);
        }
        if rc != 0 {
            if db.init.reopenMemdb == 0 {
                let iDb = db.aDb.len() - 1;
                debug_assert!(iDb >= 2);
                if let Some(pBt) = db.aDb[iDb].pBt.take() {
                    crate::btree::sqlite3BtreeClose(db, &pBt);
                    db.aDb[iDb].pSchema = Schema::default();
                }
                crate::build::sqlite3ResetAllSchemasOfConnection(db);
                db.aDb.truncate(iDb);
                if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                    zErrDyn = Some(mprintf!(db, "out of memory"));
                } else if zErrDyn.is_none() {
                    zErrDyn = Some(mprintf!(db, "unable to open database: %s", &zFile));
                }
            }
            break 'attach_error;
        }
        return;
    }

    // attach_error:
    // Return an error if we get here
    if let Some(z) = zErrDyn {
        sqlite3_result_error(context, Some(&z), -1);
    }
    if rc != 0 {
        sqlite3_result_error_code(context, rc);
    }
}

/// An SQL user-function registered to do the work of an DETACH statement. The
/// three arguments to the function come directly from a detach statement:
///
///     DETACH DATABASE x
///
///     SELECT sqlite_detach(x)
fn detachFunc(context: &mut Context, argv: &mut [Mem]) {
    let zName: ZStr = sqlite3_value_text(&mut argv[0])
        .map(|z| z.to_vec())
        .unwrap_or_default();
    let zErr: ZStr;
    'detach_error: {
        let db = &mut *context.db;
        let mut i = 0;
        while i < db.nDb() {
            if db.aDb[i as usize].pBt.is_some() && sqlite3DbIsNamed(db, i, Some(&zName)) != 0 {
                break;
            }
            i += 1;
        }

        if i >= db.nDb() {
            zErr =
                crate::printf::sqlite3_snprintf(128, b"no such database: %s", &[(&zName).into()]);
            break 'detach_error;
        }
        if i < 2 {
            zErr = crate::printf::sqlite3_snprintf(
                128,
                b"cannot detach database %s",
                &[(&zName).into()],
            );
            break 'detach_error;
        }
        let pBt = db.aDb[i as usize].pBt.clone().expect("btree");
        if crate::btree::sqlite3BtreeTxnState(db, &pBt) != SQLITE_TXN_NONE
            || crate::btree::sqlite3BtreeIsInBackup(db, &pBt) != 0
        {
            zErr =
                crate::printf::sqlite3_snprintf(128, b"database %s is locked", &[(&zName).into()]);
            break 'detach_error;
        }

        let sid = db.aDb[i as usize].pSchema.id;
        detachTempTriggers(db, sid);
        // C: sqlite3BtreeClose() frees the schema through its xFreeSchema
        // callback (sqlite3SchemaClear()).
        crate::callback::sqlite3SchemaClear(db, i);
        crate::btree::sqlite3BtreeClose(db, &pBt);
        db.aDb[i as usize].pBt = None;
        db.aDb[i as usize].pSchema = Schema::default();
        crate::build::sqlite3CollapseDatabaseArray(db);
        return;
    }

    // detach_error:
    sqlite3_result_error(context, Some(&zErr), -1);
}

/// This procedure generates VDBE code for a single invocation of either the
/// sqlite_detach() or sqlite_attach() SQL user functions.
fn codeAttach(
    db: &mut Sqlite3,
    type_: i32,
    pFunc: &FuncDefRef,
    pAuthArg: ExprId,
    pFilename: ExprId,
    pDbname: ExprId,
    pKey: ExprId,
) {
    'attach_end: {
        if SQLITE_OK != crate::prepare::sqlite3ReadSchema(db) {
            break 'attach_end;
        }

        if db.ppr().nErr != 0 {
            break 'attach_end;
        }
        let sName = db.arena.new_nc(NameContext::default());

        if SQLITE_OK != resolveAttachExpr(db, sName, pFilename)
            || SQLITE_OK != resolveAttachExpr(db, sName, pDbname)
            || SQLITE_OK != resolveAttachExpr(db, sName, pKey)
        {
            break 'attach_end;
        }

        if pAuthArg.nn() {
            let zAuthArg: Option<ZStr> = {
                let e = db.ex(pAuthArg);
                if e.op == TK_STRING {
                    debug_assert!(!e.HasProperty(EP_IntValue));
                    e.u.zToken.clone()
                } else {
                    None
                }
            };
            let rc = crate::auth::sqlite3AuthCheck(db, type_, zAuthArg.as_deref(), None, None);
            if rc != SQLITE_OK {
                break 'attach_end;
            }
        }

        let hasV = crate::select::sqlite3GetVdbe(db);
        let regArgs = crate::expr::sqlite3GetTempRange(db, 4);
        crate::expr::sqlite3ExprCode(db, pFilename, regArgs);
        crate::expr::sqlite3ExprCode(db, pDbname, regArgs + 1);
        crate::expr::sqlite3ExprCode(db, pKey, regArgs + 2);

        if hasV {
            let nArg = pFunc.nArg as i32;
            crate::vdbeaux::sqlite3VdbeAddFunctionCall(
                db,
                0,
                regArgs + 3 - nArg,
                regArgs + 3,
                nArg,
                pFunc,
                0,
            );
            // Code an OP_Expire. For an ATTACH statement, set P1 to true
            // (expire this statement only). For DETACH, set it to false
            // (expire all existing statements).
            crate::vdbeaux::sqlite3VdbeAddOp1(db.v(), OP_Expire, (type_ == SQLITE_ATTACH) as i32);
        }
    }

    // attach_end:
    crate::expr::sqlite3ExprDelete(db, pFilename);
    crate::expr::sqlite3ExprDelete(db, pDbname);
    crate::expr::sqlite3ExprDelete(db, pKey);
}

fn attach_or_detach_funcdef(
    nArg: i16,
    xSFunc: fn(&mut Context, &mut [Mem]),
    zName: &[u8],
) -> FuncDefRef {
    Rc::new(FuncDef {
        nArg,
        funcFlags: SQLITE_UTF8 as u32,
        pUserData: FuncUserData::None,
        xSFunc: Some(Rc::new(xSFunc)),
        xFinalize: None,
        xValue: None,
        xInverse: None,
        zName: zName.to_vec(),
        pDestructor: None,
    })
}

/// C: `void sqlite3Detach(Parse *pParse, Expr *pDbname)`
///
/// Called by the parser to compile a DETACH statement.
///
///     DETACH pDbname
pub fn sqlite3Detach(db: &mut Sqlite3, pDbname: ExprId) {
    let detach_func = attach_or_detach_funcdef(1, detachFunc, b"sqlite_detach");
    codeAttach(
        db,
        SQLITE_DETACH,
        &detach_func,
        pDbname,
        ExprId::NULL,
        ExprId::NULL,
        pDbname,
    );
}

/// C: `void sqlite3Attach(Parse *pParse, Expr *p, Expr *pDbname, Expr *pKey)`
///
/// Called by the parser to compile an ATTACH statement.
///
///     ATTACH p AS pDbname KEY pKey
pub fn sqlite3Attach(db: &mut Sqlite3, p: ExprId, pDbname: ExprId, pKey: ExprId) {
    let attach_func = attach_or_detach_funcdef(3, attachFunc, b"sqlite_attach");
    codeAttach(db, SQLITE_ATTACH, &attach_func, p, p, pDbname, pKey);
}

/// The DbFixer of a walk (C `Walker.u.pFix`).
fn fixer(p: &Walker<'_>) -> DbFixer {
    p.u.any
        .as_ref()
        .and_then(|a| a.downcast_ref::<DbFixer>())
        .expect("DbFixer walker")
        .clone()
}

/// Expression callback used by sqlite3FixAAAA() routines.
fn fixExprCb(p: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let pFix = fixer(p);
    if pFix.bTemp == 0 {
        p.db.exm(pExpr).SetProperty(EP_FromDDL);
    }
    if p.db.ex(pExpr).op == TK_VARIABLE {
        if p.db.init.busy != 0 {
            p.db.exm(pExpr).op = TK_NULL;
        } else {
            errmsg!(p.db, "%s cannot use variables", pFix.zType);
            return WRC_Abort;
        }
    }
    WRC_Continue
}

/// Select callback used by sqlite3FixAAAA() routines.
fn fixSelectCb(p: &mut Walker<'_>, pSelect: SelectId) -> i32 {
    let pFix = fixer(p);
    let iDb = crate::build::sqlite3FindDbName(p.db, Some(&pFix.zDb));
    let pList = p.db.sel(pSelect).pSrc;

    if pList.is_null() {
        return WRC_Continue;
    }
    let n = p.db.src(pList).nSrc();
    for i in 0..n {
        let it = SrcItemId(pList, i as u32);
        let (isSubquery, fixedSchema, zDatabase) = {
            let x = p.db.item(it);
            (x.fg.isSubquery, x.fg.fixedSchema, x.u4.zDatabase.clone())
        };
        if pFix.bTemp == 0 && isSubquery == 0 {
            if fixedSchema == 0 && zDatabase.is_some() {
                if iDb != crate::build::sqlite3FindDbName(p.db, zDatabase.as_deref()) {
                    errmsg!(
                        p.db,
                        "%s %T cannot reference objects in database %s",
                        pFix.zType,
                        &pFix.pName,
                        &zDatabase
                    );
                    return WRC_Abort;
                }
                let x = p.db.itemm(it);
                x.u4.zDatabase = None;
                x.fg.notCte = 1;
                x.fg.hadSchema = 1;
            }
            let x = p.db.itemm(it);
            x.u4.pSchema = Some(pFix.pSchema);
            x.fg.fromDDL = 1;
            x.fg.fixedSchema = 1;
        }
        let (isUsing, pOn) = {
            let x = p.db.item(it);
            (x.fg.isUsing, x.u3.pOn)
        };
        if isUsing == 0 && sqlite3WalkExpr(p, pOn) != 0 {
            return WRC_Abort;
        }
    }
    let pWith = p.db.sel(pSelect).pWith;
    if pWith.nn() {
        let nCte = p.db.arena.with(pWith).a.len();
        for i in 0..nCte {
            let s = p.db.arena.with(pWith).a[i].pSelect;
            if sqlite3WalkSelect(p, s) != 0 {
                return WRC_Abort;
            }
        }
    }
    WRC_Continue
}

/// C: `void sqlite3FixInit( DbFixer *pFix, Parse *pParse, int iDb, const char *zType, const Token *pName )`
///
/// Initialize a DbFixer structure.  This routine must be called prior
/// to passing the structure to one of the sqliteFixAAAA() routines below.
///
/// Rust: zType must be one of "view", "trigger" or "index" (C passes
/// string literals); other values are mapped to "object".
pub fn sqlite3FixInit(
    db: &mut Sqlite3,
    pFix: &mut DbFixer,
    iDb: i32,
    zType: Option<&[u8]>,
    pName: &Token,
) {
    debug_assert!(db.nDb() > iDb);
    pFix.zDb = db.aDb[iDb as usize].zDbSName.clone().unwrap_or_default();
    pFix.pSchema = db.aDb[iDb as usize].pSchema.id;
    pFix.zType = match zType {
        Some(b"view") => "view",
        Some(b"trigger") => "trigger",
        Some(b"index") => "index",
        _ => "object",
    };
    pFix.pName = pName.clone();
    pFix.bTemp = (iDb == 1) as u8;
}

/// Run `f` with a Walker set up for the fixer (C `pFix->w`).
fn with_fix_walker<R>(db: &mut Sqlite3, pFix: &DbFixer, f: impl FnOnce(&mut Walker<'_>) -> R) -> R {
    let mut w = Walker::new(db);
    w.xExprCallback = Some(fixExprCb);
    w.xSelectCallback = Some(fixSelectCb);
    w.xSelectCallback2 = Some(sqlite3WalkWinDefnDummyCallback);
    w.walkerDepth = 0;
    w.eCode = 0;
    w.u.any = Some(Box::new(pFix.clone()));
    f(&mut w)
}

/// C: `int sqlite3FixSrcList( DbFixer *pFix, SrcList *pList )`
///
/// The following set of routines walk through the parse tree and assign
/// a specific database to all table references where the database name
/// was left unspecified in the original SQL statement.  The pFix structure
/// must have been initialized by a prior call to sqlite3FixInit().
///
/// These routines are used to make sure that an index, trigger, or
/// view in one database does not refer to objects in a different database.
/// (Exception: indices, triggers, and views in the TEMP database are
/// allowed to refer to anything.)  If a reference is explicitly made
/// to an object in a different database, an error message is added to
/// pParse->zErrMsg and these routines return non-zero.  If everything
/// checks out, these routines return 0.
pub fn sqlite3FixSrcList(db: &mut Sqlite3, pFix: &mut DbFixer, pList: SrcListId) -> i32 {
    let mut res = 0;
    if pList.nn() {
        let s = db.arena.new_select(Select {
            pSrc: pList,
            ..Default::default()
        });
        res = with_fix_walker(db, pFix, |w| sqlite3WalkSelect(w, s));
    }
    res
}

/// C: `int sqlite3FixSelect( DbFixer *pFix, Select *pSelect )`
pub fn sqlite3FixSelect(db: &mut Sqlite3, pFix: &mut DbFixer, pSelect: SelectId) -> i32 {
    with_fix_walker(db, pFix, |w| sqlite3WalkSelect(w, pSelect))
}

/// C: `int sqlite3FixExpr( DbFixer *pFix, Expr *pExpr )`
pub fn sqlite3FixExpr(db: &mut Sqlite3, pFix: &mut DbFixer, pExpr: ExprId) -> i32 {
    with_fix_walker(db, pFix, |w| sqlite3WalkExpr(w, pExpr))
}

/// C: `int sqlite3FixTriggerStep( DbFixer *pFix, TriggerStep *pStep )`
pub fn sqlite3FixTriggerStep(
    db: &mut Sqlite3,
    pFix: &mut DbFixer,
    mut pStep: TriggerStepId,
) -> i32 {
    while pStep.nn() {
        let (pSelect, pWhere, pExprList, pSrc, pUpsert, pNext) = {
            let s = db.arena.tstep(pStep);
            (s.pSelect, s.pWhere, s.pExprList, s.pSrc, s.pUpsert, s.pNext)
        };
        if with_fix_walker(db, pFix, |w| {
            sqlite3WalkSelect(w, pSelect) != 0
                || sqlite3WalkExpr(w, pWhere) != 0
                || sqlite3WalkExprList(w, pExprList) != 0
        }) || sqlite3FixSrcList(db, pFix, pSrc) != 0
        {
            return 1;
        }
        let mut pUp = pUpsert;
        while pUp.nn() {
            let (t, tw, s, w2, nx) = {
                let u = db.arena.ups(pUp);
                (
                    u.pUpsertTarget,
                    u.pUpsertTargetWhere,
                    u.pUpsertSet,
                    u.pUpsertWhere,
                    u.pNextUpsert,
                )
            };
            if with_fix_walker(db, pFix, |w| {
                sqlite3WalkExprList(w, t) != 0
                    || sqlite3WalkExpr(w, tw) != 0
                    || sqlite3WalkExprList(w, s) != 0
                    || sqlite3WalkExpr(w, w2) != 0
            }) {
                return 1;
            }
            pUp = nx;
        }
        pStep = pNext;
    }
    0
}
