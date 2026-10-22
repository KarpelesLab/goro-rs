//! Port of delete.c: code generation for DELETE FROM statements, plus the
//! row/index deletion helpers shared with INSERT and UPDATE.

use crate::auth::{sqlite3AuthCheck, sqlite3AuthContextPop, sqlite3AuthContextPush};
use crate::build::*;
use crate::expr::*;
use crate::fkey::{sqlite3FkActions, sqlite3FkCheck, sqlite3FkOldmask, sqlite3FkRequired};
use crate::insert::{sqlite3AutoincrementEnd, sqlite3IndexAffinityStr, sqlite3OpenTableAndIndices};
use crate::int::*;
use crate::resolve::sqlite3ResolveExprNames;
use crate::select::{
    sqlite3GetVdbe, sqlite3IndexedByLookup, sqlite3Select, sqlite3SelectDelete,
    sqlite3SelectDestInit, sqlite3SelectNew,
};
use crate::trigger::{
    sqlite3CodeRowTrigger, sqlite3IsToplevel, sqlite3TriggerColmask, sqlite3TriggersExist,
};
use crate::util::sqlite3_stricmp;
use crate::vdbeaux::*;
use crate::where_::{
    sqlite3WhereBegin, sqlite3WhereEnd, sqlite3WhereOkOnePass,
    sqlite3WhereUsesDeferredSeek,
};

/// C: `Table * sqlite3SrcListLookup(Parse *pParse, SrcList *pSrc)`
///
/// While a SrcList can in general represent multiple tables and subqueries
/// (as in the FROM clause of a SELECT statement) in this case it contains
/// the name of a single table, as one might find in an INSERT, DELETE,
/// or UPDATE statement.  Look up that table in the symbol table and
/// return a pointer.  Set an error message and return NULL if the table
/// name is not found or if any other error occurs.
///
/// The following fields are initialized appropriate in pSrc:
///
///    pSrc->a[0].spTab        Pointer to the Table object
///    pSrc->a[0].u2.pIBIndex  Pointer to the INDEXED BY index, if there is one
pub fn sqlite3SrcListLookup(db: &mut Sqlite3, pSrc: SrcListId) -> Option<TableRef> {
    let pItem = SrcItemId(pSrc, 0);
    debug_assert!(db.src(pSrc).nSrc() >= 1);
    let mut pTab = sqlite3LocateTableItem(db, 0, pItem);
    if let Some(old) = db.item(pItem).pSTab.clone() {
        sqlite3DeleteTable(db, &old);
    }
    db.itemm(pItem).pSTab = pTab.clone();
    db.itemm(pItem).fg.notCte = 1;
    if let Some(t) = &pTab {
        t.borrow_mut().nTabRef += 1;
        if db.item(pItem).fg.isIndexedBy != 0 && sqlite3IndexedByLookup(db, pItem) != 0 {
            pTab = None;
        }
    }
    pTab
}

/// C: `void sqlite3CodeChangeCount(Vdbe *v, int regCounter, const char *zColName)`
///
/// Generate byte-code that will report the number of rows modified
/// by a DELETE, INSERT, or UPDATE statement.
pub fn sqlite3CodeChangeCount(v: &mut Vdbe, regCounter: i32, zColName: Option<&[u8]>) {
    sqlite3VdbeAddOp0(v, OP_FkCheck);
    sqlite3VdbeAddOp2(v, OP_ResultRow, regCounter, 1);
    sqlite3VdbeSetNumCols(v, 1);
    sqlite3VdbeSetColName(v, 0, COLNAME_NAME, zColName.unwrap_or(b""));
}

/// Return true if table pTab is read-only.
///
/// A table is read-only if any of the following are true:
///
///   1) It is a virtual table and no implementation of the xUpdate method
///      has been provided
///
///   2) A trigger is currently being coded and the table is a virtual table
///      that is SQLITE_VTAB_DIRECTONLY or if PRAGMA trusted_schema=OFF and
///      the table is not SQLITE_VTAB_INNOCUOUS.
///
///   3) It is a system table (i.e. sqlite_schema), this call is not
///      part of a nested parse and writable_schema pragma has not
///      been specified
///
///   4) The table is a shadow table, the database connection is in
///      defensive mode, and the current sqlite3_prepare()
///      is for a top-level SQL statement.
fn vtabIsReadOnly(db: &mut Sqlite3, pTab: &TableRef) -> i32 {
    debug_assert!(pTab.borrow().IsVirtual());
    // TODO(vtab): C tests sqlite3GetVTable(db,pTab)->pMod->pModule->xUpdate==0
    // (return 1), then, within triggers or DDL, raises "unsafe use of
    // virtual table \"%s\"" when pTab->u.vtab.p->eVtabRisk >
    // ((db->flags & SQLITE_TrustedSchema)!=0).  The VTable object is not
    // modelled yet.
    crate::vtab::sqlite3GetVTable(db, pTab);
    0
}

fn tabIsReadOnly(db: &mut Sqlite3, pTab: &TableRef) -> i32 {
    if pTab.borrow().IsVirtual() {
        return vtabIsReadOnly(db, pTab);
    }
    let tabFlags = pTab.borrow().tabFlags;
    if (tabFlags & (TF_Readonly | TF_Shadow)) == 0 {
        return 0;
    }
    if (tabFlags & TF_Readonly) != 0 {
        return (sqlite3WritableSchema(db) == 0 && db.ppr().nested == 0) as i32;
    }
    debug_assert!(tabFlags & TF_Shadow != 0);
    sqlite3ReadOnlyShadowTables(db)
}

/// C: `int sqlite3IsReadOnly(Parse *pParse, Table *pTab, Trigger *pTrigger)`
///
/// Check to make sure the given table is writable.
///
/// If pTab is not writable  ->  generate an error message and return 1.
/// If pTab is writable but other errors have occurred -> return 1.
/// If pTab is writable and no prior errors -> return 0;
///
/// Rust: `pTrigger` is the trigger list (empty for C NULL).
pub fn sqlite3IsReadOnly(db: &mut Sqlite3, pTab: &TableRef, pTrigger: &[TriggerRef]) -> i32 {
    if tabIsReadOnly(db, pTab) != 0 {
        let zName = pTab.borrow().zName.clone();
        errmsg!(db, "table %s may not be modified", &zName);
        return 1;
    }
    if pTab.borrow().IsView()
        && (pTrigger.is_empty() || (pTrigger[0].borrow().bReturning != 0 && pTrigger.len() == 1))
    {
        let zName = pTab.borrow().zName.clone();
        errmsg!(db, "cannot modify %s because it is a view", &zName);
        return 1;
    }
    0
}

/// C: `void sqlite3MaterializeView( Parse *pParse, Table *pView, Expr *pWhere, ExprList *pOrderBy, Expr *pLimit, int iCur )`
///
/// Evaluate a view and store its result in an ephemeral table.  The
/// pWhere argument is an optional WHERE clause that restricts the
/// set of rows in the view that are to be added to the ephemeral table.
pub fn sqlite3MaterializeView(
    db: &mut Sqlite3,
    pView: &TableRef,
    pWhere: ExprId,
    pOrderBy: ExprListId,
    pLimit: ExprId,
    iCur: i32,
) {
    let iDb = db.schema_index(pView.borrow().pSchema);
    let pWhere = sqlite3ExprDup(db, pWhere, 0);
    let pFrom = sqlite3SrcListAppend(db, SrcListId::NULL, None, None);
    if pFrom.nn() {
        debug_assert!(db.src(pFrom).nSrc() == 1);
        let zName = pView.borrow().zName.clone();
        let zDb = db.aDb[iDb as usize].zDbSName.clone();
        let it = &mut db.srcm(pFrom).a[0];
        it.zName = Some(zName);
        debug_assert!(it.fg.fixedSchema == 0 && it.fg.isSubquery == 0);
        it.u4.zDatabase = zDb;
        debug_assert!(it.fg.isUsing == 0);
        debug_assert!(it.u3.pOn.is_null());
    }
    let pSel = sqlite3SelectNew(
        db,
        ExprListId::NULL,
        pFrom,
        pWhere,
        ExprListId::NULL,
        ExprId::NULL,
        pOrderBy,
        SF_IncludeHidden,
        pLimit,
    );
    let mut dest = SelectDest::default();
    sqlite3SelectDestInit(&mut dest, SRT_EphemTab as i32, iCur);
    sqlite3Select(db, pSel, &mut dest);
    sqlite3SelectDelete(db, pSel);
}

/// C: `Expr * sqlite3LimitWhere( Parse *pParse, SrcList *pSrc, Expr *pWhere, ExprList *pOrderBy, Expr *pLimit, char *zStmtType )`
///
/// Generate an expression tree to implement the WHERE, ORDER BY,
/// and LIMIT/OFFSET portion of DELETE and UPDATE statements.
///
///     DELETE FROM table_wxyz WHERE a<5 ORDER BY a LIMIT 1;
///                            \__________________________/
///                               pLimitWhere (pInClause)
///
/// Only used with SQLITE_ENABLE_UPDATE_DELETE_LIMIT, which our reference
/// build does not define: nothing calls this function.
pub fn sqlite3LimitWhere(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    pWhere: ExprId,
    pOrderBy: ExprListId,
    pLimit: ExprId,
    zStmtType: Option<&[u8]>,
) -> ExprId {
    let mut pLhs;
    let mut pEList = ExprListId::NULL;

    /* Check that there isn't an ORDER BY without a LIMIT clause.
     */
    if pOrderBy.nn() && pLimit.is_null() {
        errmsg!(db, "ORDER BY without LIMIT on %s", zStmtType);
        sqlite3ExprDelete(db, pWhere);
        sqlite3ExprListDelete(db, pOrderBy);
        return ExprId::NULL;
    }

    /* We only need to generate a select expression if there
     ** is a limit/offset term to enforce.
     */
    if pLimit.is_null() {
        return pWhere;
    }

    /* Generate a select expression tree to enforce the limit/offset
     ** term for the DELETE or UPDATE statement.  For example:
     **   DELETE FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
     ** becomes:
     **   DELETE FROM table_a WHERE rowid IN (
     **     SELECT rowid FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
     **   );
     */
    let pTab = db.src(pSrc).a[0].pSTab.clone().unwrap();
    if pTab.borrow().HasRowid() {
        pLhs = sqlite3PExpr(db, TK_ROW as i32, ExprId::NULL, ExprId::NULL);
        let e = sqlite3PExpr(db, TK_ROW as i32, ExprId::NULL, ExprId::NULL);
        pEList = sqlite3ExprListAppend(db, ExprListId::NULL, e);
    } else {
        let pPk = sqlite3PrimaryKeyIndex(&pTab).unwrap();
        let nKeyCol = pPk.borrow().nKeyCol as usize;
        debug_assert!(nKeyCol >= 1);
        if nKeyCol == 1 {
            let c = pPk.borrow().aiColumn[0] as usize;
            let zName = pTab.borrow().aCol[c].zCnName.clone();
            pLhs = sqlite3Expr(db, TK_ID as i32, Some(&zName));
            let e = sqlite3Expr(db, TK_ID as i32, Some(&zName));
            pEList = sqlite3ExprListAppend(db, ExprListId::NULL, e);
        } else {
            for i in 0..nKeyCol {
                let c = pPk.borrow().aiColumn[i] as usize;
                let zName = pTab.borrow().aCol[c].zCnName.clone();
                let p = sqlite3Expr(db, TK_ID as i32, Some(&zName));
                pEList = sqlite3ExprListAppend(db, pEList, p);
            }
            pLhs = sqlite3PExpr(db, TK_VECTOR as i32, ExprId::NULL, ExprId::NULL);
            if pLhs.nn() {
                let l = sqlite3ExprListDup(db, pEList, 0);
                db.exm(pLhs).x.pList = l;
            }
        }
    }

    /* duplicate the FROM clause as it is needed by both the DELETE/UPDATE tree
     ** and the SELECT subtree. */
    db.srcm(pSrc).a[0].pSTab = None;
    let pSelectSrc = sqlite3SrcListDup(db, pSrc, 0);
    db.srcm(pSrc).a[0].pSTab = Some(pTab);
    if db.src(pSrc).a[0].fg.isIndexedBy != 0 {
        debug_assert!(db.src(pSrc).a[0].fg.isCte == 0);
        let it = &mut db.srcm(pSrc).a[0];
        it.u2.pIBIndex = None;
        it.fg.isIndexedBy = 0;
        it.u1.zIndexedBy = None;
    } else if db.src(pSrc).a[0].fg.isCte != 0 {
        let cu = db.src(pSrc).a[0].u2.pCteUse;
        db.arena.cteusem(cu).nUse += 1;
    }

    /* generate the SELECT expression tree. */
    let pSelect = sqlite3SelectNew(
        db,
        pEList,
        pSelectSrc,
        pWhere,
        ExprListId::NULL,
        ExprId::NULL,
        pOrderBy,
        0,
        pLimit,
    );

    /* now generate the new WHERE rowid IN clause for the DELETE/UPDATE */
    let pInClause = sqlite3PExpr(db, TK_IN as i32, pLhs, ExprId::NULL);
    sqlite3PExprAddSelect(db, pInClause, pSelect);
    pLhs = pInClause;
    pLhs
}

/// C: `void sqlite3DeleteFrom( Parse *pParse, SrcList *pTabList, Expr *pWhere, ExprList *pOrderBy, Expr *pLimit )`
///
/// Generate code for a DELETE FROM statement.
///
///     DELETE FROM table_wxyz WHERE a<5 AND b NOT NULL;
///                 \________/       \________________/
///                  pTabList              pWhere
pub fn sqlite3DeleteFrom(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pWhere: ExprId,
    pOrderBy: ExprListId,
    pLimit: ExprId,
) {
    let mut sContext = AuthContext::default();
    let mut pOrderBy = pOrderBy;
    let mut pLimit = pLimit;

    'cleanup: {
        if db.ppr().nErr != 0 {
            break 'cleanup;
        }
        debug_assert!(db.src(pTabList).nSrc() == 1);

        /* Locate the table which we want to delete.  This table has to be
         ** put in an SrcList structure because some of the subroutines we
         ** will be calling are designed to work with multiple tables and expect
         ** an SrcList* parameter instead of just a Table* parameter.
         */
        let pTab = match sqlite3SrcListLookup(db, pTabList) {
            Some(t) => t,
            None => break 'cleanup,
        };

        /* Figure out if we have any triggers and if the table being
         ** deleted from is a view
         */
        let pTrigger = sqlite3TriggersExist(db, &pTab, TK_DELETE as i32, ExprListId::NULL, None);
        let isView = pTab.borrow().IsView();
        let mut bComplex = !pTrigger.is_empty() || sqlite3FkRequired(db, &pTab, None, 0) != 0;

        /* If pTab is really a view, make sure it has been initialized.
         */
        if sqlite3ViewGetColumnNames(db, &pTab) != 0 {
            break 'cleanup;
        }

        if sqlite3IsReadOnly(db, &pTab, &pTrigger) != 0 {
            break 'cleanup;
        }
        let iDb = db.schema_index(pTab.borrow().pSchema);
        debug_assert!(iDb < db.nDb());
        let zTabName = pTab.borrow().zName.clone();
        let zDbSName = db.aDb[iDb as usize].zDbSName.clone();
        let rcauth = sqlite3AuthCheck(
            db,
            SQLITE_DELETE,
            Some(&zTabName),
            None,
            zDbSName.as_deref(),
        );
        debug_assert!(rcauth == SQLITE_OK || rcauth == SQLITE_DENY || rcauth == SQLITE_IGNORE);
        if rcauth == SQLITE_DENY {
            break 'cleanup;
        }
        debug_assert!(!isView || !pTrigger.is_empty());

        /* Assign cursor numbers to the table and all its indices.
         */
        let iTabCur = db.ppr().nTab;
        db.pp().nTab += 1;
        db.srcm(pTabList).a[0].iCursor = iTabCur;
        let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
        let nIdx = aIdx.len() as i32;
        db.pp().nTab += nIdx;

        /* Start the view context
         */
        if isView {
            sqlite3AuthContextPush(db, &mut sContext, Some(&zTabName));
        }

        /* Begin generating code.
         */
        if !sqlite3GetVdbe(db) {
            break 'cleanup;
        }
        if db.ppr().nested == 0 {
            sqlite3VdbeCountChanges(db.v());
        }
        sqlite3BeginWriteOperation(db, bComplex as i32, iDb);

        let mut iDataCur = 0;
        let mut iIdxCur = 0;

        /* If we are trying to delete from a view, realize that view into
         ** an ephemeral table.
         */
        if isView {
            sqlite3MaterializeView(db, &pTab, pWhere, pOrderBy, pLimit, iTabCur);
            iDataCur = iTabCur;
            iIdxCur = iTabCur;
            pOrderBy = ExprListId::NULL;
            pLimit = ExprId::NULL;
        }
        let _ = (pOrderBy, pLimit);

        /* Resolve the column names in the WHERE clause.
         */
        let sNC = db.arena.new_nc(NameContext {
            pSrcList: pTabList,
            ..Default::default()
        });
        if sqlite3ResolveExprNames(db, sNC, pWhere) != 0 {
            break 'cleanup;
        }

        /* Initialize the counter of the number of rows deleted, if
         ** we are counting rows.
         */
        let mut memCnt = 0;
        if (db.flags & SQLITE_CountRows) != 0
            && db.ppr().nested == 0
            && db.ppr().pTriggerTab.is_none()
            && db.ppr().bReturning == 0
        {
            db.pp().nMem += 1;
            memCnt = db.ppr().nMem;
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, memCnt);
        }

        /* Special case: A DELETE without a WHERE clause deletes everything.
         ** It is easier just to erase the whole table. Prior to version 3.6.5,
         ** this optimization caused the row change count (the value returned by
         ** API function sqlite3_count_changes) to be set incorrectly.
         **
         ** The "rcauth==SQLITE_OK" terms is the
         ** IMPLEMENTATION-OF: R-17228-37124 If the action code is SQLITE_DELETE and
         ** the callback returns SQLITE_IGNORE then the DELETE operation proceeds but
         ** the truncate optimization is disabled and all rows are deleted
         ** individually.
         */
        if rcauth == SQLITE_OK && pWhere.is_null() && !bComplex && !pTab.borrow().IsVirtual() {
            debug_assert!(!isView);
            let (tnum, hasRowid) = {
                let t = pTab.borrow();
                (t.tnum, t.HasRowid())
            };
            sqlite3TableLock(db, iDb, tnum, 1, Some(&zTabName));
            if hasRowid {
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Clear,
                    tnum as i32,
                    iDb,
                    if memCnt != 0 { memCnt } else { -1 },
                    P4::Z(zTabName.clone()),
                    P4_STATIC as i32,
                );
            }
            for pIdx in aIdx.iter() {
                let (isPk, itnum) = {
                    let x = pIdx.borrow();
                    (x.IsPrimaryKeyIndex(), x.tnum)
                };
                if isPk && !hasRowid {
                    sqlite3VdbeAddOp3(
                        db.v(),
                        OP_Clear,
                        itnum as i32,
                        iDb,
                        if memCnt != 0 { memCnt } else { -1 },
                    );
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_Clear, itnum as i32, iDb);
                }
            }
        } else {
            let mut wcf: u16 = WHERE_ONEPASS_DESIRED | WHERE_DUPLICATES_OK;
            let mut pPk: Option<IndexRef> = None;
            let mut nPk: i16 = 1;
            let mut iPk = 0;
            let mut iEphCur = 0;
            let mut iRowSet = 0;
            let mut addrEphOpen = 0;
            let mut addrBypass = 0;
            let mut addrLoop = 0;
            let mut aiCurOnePass = [0i32; 2];
            let mut aToOpen: Vec<u8> = Vec::new();
            let iKey;
            let nKey: i16;

            if (db.arena.nc(sNC).ncFlags & NC_Subquery) != 0 {
                bComplex = true;
            }
            wcf |= if bComplex { 0 } else { WHERE_ONEPASS_MULTIROW };
            if pTab.borrow().HasRowid() {
                /* For a rowid table, initialize the RowSet to an empty set */
                debug_assert!(nPk == 1);
                db.pp().nMem += 1;
                iRowSet = db.ppr().nMem;
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, iRowSet);
            } else {
                /* For a WITHOUT ROWID table, create an ephemeral table used to
                 ** hold all primary keys for rows to be deleted. */
                let pk = sqlite3PrimaryKeyIndex(&pTab).expect("no PK");
                nPk = pk.borrow().nKeyCol as i16;
                iPk = db.ppr().nMem + 1;
                db.pp().nMem += nPk as i32;
                iEphCur = db.ppr().nTab;
                db.pp().nTab += 1;
                addrEphOpen = sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, iEphCur, nPk as i32);
                sqlite3VdbeSetP4KeyInfo(db, &pk);
                pPk = Some(pk);
            }

            /* Construct a query to find the rowid or primary key for every row
             ** to be deleted, based on the WHERE clause. Set variable eOnePass
             ** to indicate the strategy used to implement this delete:
             **
             **  ONEPASS_OFF:    Two-pass approach - use a FIFO for rowids/PK values.
             **  ONEPASS_SINGLE: One-pass approach - at most one row deleted.
             **  ONEPASS_MULTI:  One-pass approach - any number of rows may be deleted.
             */
            let mut pWInfo = match sqlite3WhereBegin(
                db,
                pTabList,
                pWhere,
                ExprListId::NULL,
                ExprListId::NULL,
                SelectId::NULL,
                wcf,
                iTabCur + 1,
            ) {
                Some(w) => w,
                None => break 'cleanup,
            };
            let eOnePass = sqlite3WhereOkOnePass(&mut pWInfo, &mut aiCurOnePass);
            debug_assert!(!pTab.borrow().IsVirtual() || eOnePass != ONEPASS_MULTI);
            if eOnePass != ONEPASS_SINGLE {
                sqlite3MultiWrite(db);
            }
            if sqlite3WhereUsesDeferredSeek(&mut pWInfo) != 0 {
                sqlite3VdbeAddOp1(db.v(), OP_FinishSeek, iTabCur);
            }

            /* Keep track of the number of rows to be deleted */
            if memCnt != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_AddImm, memCnt, 1);
            }

            /* Extract the rowid or primary key for the current row */
            let mut iKeyV;
            if let Some(pk) = &pPk {
                for i in 0..nPk as usize {
                    let c = pk.borrow().aiColumn[i];
                    debug_assert!(c >= 0);
                    sqlite3ExprCodeGetColumnOfTable(
                        db,
                        &pTab,
                        iTabCur,
                        c as i32,
                        iPk + i as i32,
                    );
                }
                iKeyV = iPk;
            } else {
                db.pp().nMem += 1;
                iKeyV = db.ppr().nMem;
                sqlite3ExprCodeGetColumnOfTable(db, &pTab, iTabCur, -1, iKeyV);
            }

            if eOnePass != ONEPASS_OFF {
                /* For ONEPASS, no need to store the rowid/primary-key. There is only
                 ** one, so just keep it in its register(s) and fall through to the
                 ** delete code.  */
                nKey = nPk; /* OP_Found will use an unpacked key */
                aToOpen = vec![1u8; (nIdx + 2) as usize];
                aToOpen[(nIdx + 1) as usize] = 0;
                if aiCurOnePass[0] >= 0 {
                    aToOpen[(aiCurOnePass[0] - iTabCur) as usize] = 0;
                }
                if aiCurOnePass[1] >= 0 {
                    aToOpen[(aiCurOnePass[1] - iTabCur) as usize] = 0;
                }
                if addrEphOpen != 0 {
                    sqlite3VdbeChangeToNoop(db.v(), addrEphOpen);
                }
                addrBypass = sqlite3VdbeMakeLabel(db);
            } else {
                if let Some(pk) = &pPk {
                    /* Add the PK key for this row to the temporary table */
                    db.pp().nMem += 1;
                    iKeyV = db.ppr().nMem;
                    nKey = 0; /* Zero tells OP_Found to use a composite key */
                    let mut zAff = sqlite3IndexAffinityStr(db, pk).unwrap_or_default();
                    zAff.truncate(nPk as usize);
                    sqlite3VdbeAddOp4(
                        db.v(),
                        OP_MakeRecord,
                        iPk,
                        nPk as i32,
                        iKeyV,
                        P4::Z(zAff),
                        P4_DYNAMIC as i32,
                    );
                    sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, iEphCur, iKeyV, iPk, nPk as i32);
                } else {
                    /* Add the rowid of the row to be deleted to the RowSet */
                    nKey = 1; /* OP_DeferredSeek always uses a single rowid */
                    sqlite3VdbeAddOp2(db.v(), OP_RowSetAdd, iRowSet, iKeyV);
                }
                sqlite3WhereEnd(db, &mut pWInfo);
            }
            iKey = iKeyV;

            /* Unless this is a view, open cursors for the table we are
             ** deleting from and all its indices. If this is a view, then the
             ** only effect this statement has is to fire the INSTEAD OF
             ** triggers.
             */
            if !isView {
                let mut iAddrOnce = 0;
                if eOnePass == ONEPASS_MULTI {
                    iAddrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
                }
                sqlite3OpenTableAndIndices(
                    db,
                    &pTab,
                    OP_OpenWrite as i32,
                    OPFLAG_FORDELETE as u8,
                    iTabCur,
                    if aToOpen.is_empty() {
                        None
                    } else {
                        Some(&aToOpen)
                    },
                    &mut iDataCur,
                    &mut iIdxCur,
                );
                if eOnePass == ONEPASS_MULTI {
                    sqlite3VdbeJumpHereOrPopInst(db.v(), iAddrOnce);
                }
            }

            /* Set up a loop over the rowids/primary-keys that were found in the
             ** where-clause loop above.
             */
            if eOnePass != ONEPASS_OFF {
                debug_assert!(nKey == nPk); /* OP_Found will use an unpacked key */
                if !pTab.borrow().IsVirtual() && aToOpen[(iDataCur - iTabCur) as usize] != 0 {
                    debug_assert!(pPk.is_some() || pTab.borrow().IsView());
                    sqlite3VdbeAddOp4Int(
                        db.v(),
                        OP_NotFound,
                        iDataCur,
                        addrBypass,
                        iKey,
                        nKey as i32,
                    );
                }
            } else if pPk.is_some() {
                addrLoop = sqlite3VdbeAddOp1(db.v(), OP_Rewind, iEphCur);
                if pTab.borrow().IsVirtual() {
                    sqlite3VdbeAddOp3(db.v(), OP_Column, iEphCur, 0, iKey);
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_RowData, iEphCur, iKey);
                }
                debug_assert!(nKey == 0); /* OP_Found will use a composite key */
            } else {
                addrLoop = sqlite3VdbeAddOp3(db.v(), OP_RowSetRead, iRowSet, 0, iKey);
                debug_assert!(nKey == 1);
            }

            /* Delete the row */
            if pTab.borrow().IsVirtual() {
                crate::vtab::sqlite3GetVTable(db, &pTab);
                crate::vtab::sqlite3VtabMakeWritable(db, &pTab);
                debug_assert!(eOnePass == ONEPASS_OFF || eOnePass == ONEPASS_SINGLE);
                sqlite3MayAbort(db);
                if eOnePass == ONEPASS_SINGLE {
                    sqlite3VdbeAddOp1(db.v(), OP_Close, iTabCur);
                    if sqlite3IsToplevel(db) {
                        db.pp().isMultiWrite = 0;
                    }
                }
                // TODO(vtab): P4 should be the VTable (P4_VTAB).
                sqlite3VdbeAddOp4(db.v(), OP_VUpdate, 0, 1, iKey, P4::None, P4_VTAB as i32);
                sqlite3VdbeChangeP5(db.v(), OE_Abort as u16);
            } else {
                let count = (db.ppr().nested == 0) as u8; /* True to count changes */
                sqlite3GenerateRowDelete(
                    db,
                    &pTab,
                    &pTrigger,
                    iDataCur,
                    iIdxCur,
                    iKey,
                    nKey,
                    count,
                    OE_Default,
                    eOnePass as u8,
                    aiCurOnePass[1],
                );
            }

            /* End of the loop over all rowids/primary-keys. */
            if eOnePass != ONEPASS_OFF {
                sqlite3VdbeResolveLabel(db.v(), addrBypass);
                sqlite3WhereEnd(db, &mut pWInfo);
            } else if pPk.is_some() {
                sqlite3VdbeAddOp2(db.v(), OP_Next, iEphCur, addrLoop + 1);
                sqlite3VdbeJumpHere(db.v(), addrLoop);
            } else {
                sqlite3VdbeGoto(db.v(), addrLoop);
                sqlite3VdbeJumpHere(db.v(), addrLoop);
            }
        } /* End non-truncate path */

        /* Update the sqlite_sequence table by storing the content of the
         ** maximum rowid counter values recorded while inserting into
         ** autoincrement tables.
         */
        if db.ppr().nested == 0 && db.ppr().pTriggerTab.is_none() {
            sqlite3AutoincrementEnd(db);
        }

        /* Return the number of rows that were deleted. If this routine is
         ** generating code because of a call to sqlite3NestedParse(), do not
         ** invoke the callback function.
         */
        if memCnt != 0 {
            sqlite3CodeChangeCount(db.v(), memCnt, Some(b"rows deleted"));
        }
    }

    // delete_from_cleanup:
    sqlite3AuthContextPop(db, &mut sContext);
    sqlite3SrcListDelete(db, pTabList);
    sqlite3ExprDelete(db, pWhere);
}

/// C: `void sqlite3GenerateRowDelete( Parse *pParse, Table *pTab, Trigger *pTrigger, int iDataCur, int iIdxCur, int iPk, i16 nPk, u8 count, u8 onconf, u8 eMode, int iIdxNoSeek )`
///
/// This routine generates VDBE code that causes a single row of a
/// single table to be deleted.  Both the original table entry and
/// all indices are removed.
///
/// Preconditions:
///
///   1.  iDataCur is an open cursor on the btree that is the canonical data
///       store for the table.  (This will be either the table itself,
///       in the case of a rowid table, or the PRIMARY KEY index in the case
///       of a WITHOUT ROWID table.)
///
///   2.  Read/write cursors for all indices of pTab must be open as
///       cursor number iIdxCur+i for the i-th index.
///
///   3.  The primary key for the row to be deleted must be stored in a
///       sequence of nPk memory cells starting at iPk.  If nPk==0 that means
///       that a search record formed from OP_MakeRecord is contained in the
///       single memory location iPk.
///
/// eMode:
///   Parameter eMode may be passed either ONEPASS_OFF (0), ONEPASS_SINGLE, or
///   ONEPASS_MULTI.  If eMode is not ONEPASS_OFF, then the cursor
///   iDataCur already points to the row to delete. If eMode is ONEPASS_OFF
///   then this function must seek iDataCur to the entry identified by iPk
///   and nPk before reading from it.
///
///   If eMode is ONEPASS_MULTI, then this call is being made as part
///   of a ONEPASS delete that affects multiple rows. In this case, if
///   iIdxNoSeek is a valid cursor number (>=0) and is not the same as
///   iDataCur, then its position should be preserved following the delete
///   operation. Or, if iIdxNoSeek is not a valid cursor number, the
///   position of iDataCur should be preserved instead.
///
/// iIdxNoSeek:
///   If iIdxNoSeek is a valid cursor number (>=0) not equal to iDataCur,
///   then it identifies an index cursor (from within array of cursors
///   starting at iIdxCur) that already points to the index entry to be deleted.
///   Except, this optimization is disabled if there are BEFORE triggers since
///   the trigger body might have moved the cursor.
///
/// Rust: `pTrigger` is the trigger list (empty for C NULL).
pub fn sqlite3GenerateRowDelete(
    db: &mut Sqlite3,
    pTab: &TableRef,
    pTrigger: &[TriggerRef],
    iDataCur: i32,
    iIdxCur: i32,
    iPk: i32,
    nPk: i16,
    count: u8,
    onconf: u8,
    eMode: u8,
    iIdxNoSeek: i32,
) {
    let mut iIdxNoSeek = iIdxNoSeek;
    let mut iOld = 0; /* First register in OLD.* array */

    /* Seek cursor iCur to the row to delete. If this row no longer exists
     ** (this can happen if a trigger program has already deleted it), do
     ** not attempt to delete it or fire any DELETE triggers.  */
    let iLabel = sqlite3VdbeMakeLabel(db);
    let opSeek = if pTab.borrow().HasRowid() {
        OP_NotExists
    } else {
        OP_NotFound
    };
    if eMode as i32 == ONEPASS_OFF {
        sqlite3VdbeAddOp4Int(db.v(), opSeek, iDataCur, iLabel, iPk, nPk as i32);
    }

    /* If there are any triggers to fire, allocate a range of registers to
     ** use for the old.* references in the triggers.  */
    if sqlite3FkRequired(db, pTab, None, 0) != 0 || !pTrigger.is_empty() {
        /* TODO: Could use temporary registers here. Also could attempt to
         ** avoid copying the contents of the rowid register.  */
        let mut mask = sqlite3TriggerColmask(
            db,
            pTrigger,
            ExprListId::NULL,
            0,
            (TRIGGER_BEFORE | TRIGGER_AFTER) as i32,
            pTab,
            onconf as i32,
        );
        mask |= sqlite3FkOldmask(db, pTab);
        let nCol = pTab.borrow().nCol as i32;
        iOld = db.ppr().nMem + 1;
        db.pp().nMem += 1 + nCol;

        /* Populate the OLD.* pseudo-table register array. These values will be
         ** used by any BEFORE and AFTER triggers that exist.  */
        sqlite3VdbeAddOp2(db.v(), OP_Copy, iPk, iOld);
        for iCol in 0..nCol {
            if mask == 0xffffffff || (iCol <= 31 && (mask & MASKBIT32(iCol)) != 0) {
                let kk = sqlite3TableColumnToStorage(pTab, iCol as i16) as i32;
                sqlite3ExprCodeGetColumnOfTable(db, pTab, iDataCur, iCol, iOld + kk + 1);
            }
        }

        /* Invoke BEFORE DELETE trigger programs. */
        let addrStart = sqlite3VdbeCurrentAddr(db.v());
        sqlite3CodeRowTrigger(
            db,
            pTrigger,
            TK_DELETE as i32,
            ExprListId::NULL,
            TRIGGER_BEFORE as i32,
            pTab,
            iOld,
            onconf as i32,
            iLabel,
        );

        /* If any BEFORE triggers were coded, then seek the cursor to the
         ** row to be deleted again. It may be that the BEFORE triggers moved
         ** the cursor or already deleted the row that the cursor was
         ** pointing to.
         **
         ** Also disable the iIdxNoSeek optimization since the BEFORE trigger
         ** may have moved that cursor.
         */
        if addrStart < sqlite3VdbeCurrentAddr(db.v()) {
            sqlite3VdbeAddOp4Int(db.v(), opSeek, iDataCur, iLabel, iPk, nPk as i32);
            iIdxNoSeek = -1;
        }

        /* Do FK processing. This call checks that any FK constraints that
         ** refer to this table (i.e. constraints attached to other tables)
         ** are not violated by deleting this row.  */
        sqlite3FkCheck(db, pTab, iOld, 0, None, 0);
    }

    /* Delete the index and table entries. Skip this step if pTab is really
     ** a view (in which case the only effect of the DELETE statement is to
     ** fire the INSTEAD OF triggers).
     **
     ** If variable 'count' is non-zero, then this OP_Delete instruction should
     ** invoke the update-hook. The pre-update-hook, on the other hand should
     ** be invoked unless table pTab is a system table. The difference is that
     ** the update-hook is not invoked for rows removed by REPLACE, but the
     ** pre-update-hook is.
     */
    if !pTab.borrow().IsView() {
        let mut p5: u16 = 0;
        sqlite3GenerateRowIndexDelete(db, pTab, iDataCur, iIdxCur, None, iIdxNoSeek);
        sqlite3VdbeAddOp2(
            db.v(),
            OP_Delete,
            iDataCur,
            if count != 0 { OPFLAG_NCHANGE as i32 } else { 0 },
        );
        let zName = pTab.borrow().zName.clone();
        if db.ppr().nested == 0 || 0 == sqlite3_stricmp(Some(&zName), Some(b"sqlite_stat1")) {
            sqlite3VdbeAppendP4(db.v(), P4::Tab(pTab.clone()), P4_TABLE as i32);
        }
        if eMode as i32 != ONEPASS_OFF {
            sqlite3VdbeChangeP5(db.v(), OPFLAG_AUXDELETE as u16);
        }
        if iIdxNoSeek >= 0 && iIdxNoSeek != iDataCur {
            sqlite3VdbeAddOp1(db.v(), OP_Delete, iIdxNoSeek);
        }
        if eMode as i32 == ONEPASS_MULTI {
            p5 |= OPFLAG_SAVEPOSITION as u16;
        }
        sqlite3VdbeChangeP5(db.v(), p5);
    }

    /* Do any ON CASCADE, SET NULL or SET DEFAULT operations required to
     ** handle rows (possibly in other tables) that refer via a foreign key
     ** to the row just deleted. */
    sqlite3FkActions(db, pTab, ExprListId::NULL, iOld, None, 0);

    /* Invoke AFTER DELETE trigger programs. */
    if !pTrigger.is_empty() {
        sqlite3CodeRowTrigger(
            db,
            pTrigger,
            TK_DELETE as i32,
            ExprListId::NULL,
            TRIGGER_AFTER as i32,
            pTab,
            iOld,
            onconf as i32,
            iLabel,
        );
    }

    /* Jump here if the row had already been deleted before any BEFORE
     ** trigger programs were invoked. Or if a trigger program throws a
     ** RAISE(IGNORE) exception.  */
    sqlite3VdbeResolveLabel(db.v(), iLabel);
}

/// C: `void sqlite3GenerateRowIndexDelete( Parse *pParse, Table *pTab, int iDataCur, int iIdxCur, int *aRegIdx, int iIdxNoSeek )`
///
/// This routine generates VDBE code that causes the deletion of all
/// index entries associated with a single row of a single table, pTab
///
/// Preconditions:
///
///   1.  A read/write cursor "iDataCur" must be open on the canonical storage
///       btree for the table pTab.  (This will be either the table itself
///       for rowid tables or to the primary key index for WITHOUT ROWID
///       tables.)
///
///   2.  Read/write cursors for all indices of pTab must be open as
///       cursor number iIdxCur+i for the i-th index.  (The pTab->pIndex
///       index is the 0-th index.)
///
///   3.  The "iDataCur" cursor must be already be positioned on the row
///       that is to be deleted.
///
/// Rust: `aRegIdx` is None for the C NULL pointer.
pub fn sqlite3GenerateRowIndexDelete(
    db: &mut Sqlite3,
    pTab: &TableRef,
    iDataCur: i32,
    iIdxCur: i32,
    aRegIdx: Option<&[i32]>,
    iIdxNoSeek: i32,
) {
    let mut r1 = -1; /* Register holding an index key */
    let mut iPartIdxLabel = 0; /* Jump destination for skipping partial index entries */
    let mut pPrior: Option<IndexRef> = None; /* Prior index */

    let pPk = if pTab.borrow().HasRowid() {
        None
    } else {
        sqlite3PrimaryKeyIndex(pTab)
    };
    let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
    for (i, pIdx) in aIdx.iter().enumerate() {
        let i = i as i32;
        let mut p3 = 0;
        if let Some(a) = aRegIdx {
            if a[i as usize] == 0 {
                continue;
            }
        }
        if let Some(pk) = &pPk {
            if std::rc::Rc::ptr_eq(pk, pIdx) {
                continue;
            }
        }
        if iIdxCur + i == iIdxNoSeek {
            continue;
        }
        r1 = sqlite3GenerateIndexKey(
            db,
            pIdx,
            iDataCur,
            0,
            1,
            Some(&mut iPartIdxLabel),
            pPrior.as_ref(),
            r1,
        );
        let (bHasExpr, uniqNotNull, nKeyCol, nColumn) = {
            let x = pIdx.borrow();
            (x.bHasExpr, x.uniqNotNull, x.nKeyCol, x.nColumn)
        };
        if bHasExpr != 0 {
            if let Some(a) = aRegIdx {
                p3 = a[i as usize];
            }
        }
        sqlite3VdbeAddOp3(db.v(), OP_IdxDelete, iIdxCur + i, r1, p3);
        sqlite3VdbeChangeP4(db.v(), -1, P4::Idx(pIdx.clone()), P4_INDEX as i32);
        sqlite3VdbeChangeP5(db.v(), if uniqNotNull != 0 { nKeyCol } else { nColumn });
        sqlite3ResolvePartIdxLabel(db, iPartIdxLabel);
        pPrior = Some(pIdx.clone());
    }
}

/// C: `int sqlite3GenerateIndexKey( Parse *pParse, Index *pIdx, int iDataCur, int regOut, int prefixOnly, int *piPartIdxLabel, Index *pPrior, int regPrior )`
///
/// Generate code that will assemble an index key and stores it in register
/// regOut.  The key with be for index pIdx which is an index on pTab.
/// iCur is the index of a cursor open on the pTab table and pointing to
/// the entry that needs indexing.  If pTab is a WITHOUT ROWID table, then
/// iCur must be the cursor of the PRIMARY KEY index.
///
/// Return a register number which is the first in a block of
/// registers that holds the elements of the index key.  The
/// block of registers has already been deallocated by the time
/// this routine returns.
///
/// If *piPartIdxLabel is not NULL, fill it in with a label and jump
/// to that label if pIdx is a partial index that should be skipped.
/// The label should be resolved using sqlite3ResolvePartIdxLabel().
/// A partial index should be skipped if its WHERE clause evaluates
/// to false or null.  If pIdx is not a partial index, *piPartIdxLabel
/// will be set to zero which is an empty label that is ignored by
/// sqlite3ResolvePartIdxLabel().
///
/// The pPrior and regPrior parameters are used to implement a cache to
/// avoid unnecessary register loads.  If pPrior is not NULL, then it is
/// a pointer to a different index for which an index key has just been
/// computed into register regPrior.  If the current pIdx index is generating
/// its key into the same sequence of registers and if pPrior and pIdx share
/// a column in common, then the register corresponding to that column already
/// holds the correct value and the loading of that register is skipped.
/// This optimization is helpful when doing a DELETE or an INTEGRITY_CHECK
/// on a table with multiple indices, and especially with the ROWID or
/// PRIMARY KEY columns of the index.
///
/// Rust: `piPartIdxLabel` and `pPrior` are None for C NULL.
pub fn sqlite3GenerateIndexKey(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    iDataCur: i32,
    regOut: i32,
    prefixOnly: i32,
    piPartIdxLabel: Option<&mut i32>,
    pPrior: Option<&IndexRef>,
    regPrior: i32,
) -> i32 {
    let mut pPrior: Option<IndexRef> = pPrior.cloned();
    if let Some(piPartIdxLabel) = piPartIdxLabel {
        let pw = pIdx.borrow().pPartIdxWhere.clone();
        if pw.is_some() {
            let lbl = sqlite3VdbeMakeLabel(db);
            *piPartIdxLabel = lbl;
            db.pp().iSelfTab = iDataCur + 1;
            let e = db.tree(&pw);
            sqlite3ExprIfFalseDup(db, e, lbl, SQLITE_JUMPIFNULL as i32);
            db.pp().iSelfTab = 0;
            pPrior = None; /* Ticket a9efb42811fa41ee 2019-11-02;
         ** pPartIdxWhere may have corrupted regPrior registers */
        } else {
            *piPartIdxLabel = 0;
        }
    }
    let (uniqNotNull, nKeyCol, nColumn) = {
        let x = pIdx.borrow();
        (x.uniqNotNull, x.nKeyCol as i32, x.nColumn as i32)
    };
    let nCol = if prefixOnly != 0 && uniqNotNull != 0 {
        nKeyCol
    } else {
        nColumn
    };
    let regBase = sqlite3GetTempRange(db, nCol);
    if let Some(p) = &pPrior {
        if regBase != regPrior || p.borrow().pPartIdxWhere.is_some() {
            pPrior = None;
        }
    }
    for j in 0..nCol {
        let aj = pIdx.borrow().aiColumn[j as usize];
        if let Some(p) = &pPrior {
            let pj = p.borrow().aiColumn[j as usize];
            if pj == aj && pj != XN_EXPR {
                /* This column was already computed by the previous index */
                continue;
            }
        }
        sqlite3ExprCodeLoadIndexColumn(db, pIdx, iDataCur, j, regBase + j);
        if aj >= 0 {
            /* If the column affinity is REAL but the number is an integer, then it
             ** might be stored in the table as an integer (using a compact
             ** representation) then converted to REAL by an OP_RealAffinity opcode.
             ** But we are getting ready to store this value back into an index, where
             ** it should be converted by to INTEGER again.  So omit the
             ** OP_RealAffinity opcode if it is present */
            sqlite3VdbeDeletePriorOpcode(db.v(), OP_RealAffinity);
        }
    }
    if regOut != 0 {
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regBase, nCol, regOut);
    }
    sqlite3ReleaseTempRange(db, regBase, nCol);
    regBase
}

/// C: `void sqlite3ResolvePartIdxLabel(Parse *pParse, int iLabel)`
///
/// If a prior call to sqlite3GenerateIndexKey() generated a jump-over label
/// because it was a partial index, then this routine should be called to
/// resolve that label.
pub fn sqlite3ResolvePartIdxLabel(db: &mut Sqlite3, iLabel: i32) {
    if iLabel != 0 {
        sqlite3VdbeResolveLabel(db.v(), iLabel);
    }
}
