//! Port of update.c: code generation for UPDATE statements.

use crate::auth::{sqlite3AuthCheck, sqlite3AuthContextPop, sqlite3AuthContextPush};
use crate::build::*;
use crate::delete::{
    sqlite3CodeChangeCount, sqlite3GenerateRowIndexDelete, sqlite3IsReadOnly,
    sqlite3MaterializeView, sqlite3SrcListLookup,
};
use crate::expr::*;
use crate::fkey::{sqlite3FkActions, sqlite3FkCheck, sqlite3FkOldmask, sqlite3FkRequired};
use crate::insert::{
    sqlite3AutoincrementEnd, sqlite3CompleteInsertion, sqlite3ComputeGeneratedColumns,
    sqlite3ExprReferencesUpdatedColumn, sqlite3GenerateConstraintChecks, sqlite3IndexAffinityStr,
    sqlite3OpenTableAndIndices, sqlite3TableAffinity,
};
use crate::int::*;
use crate::resolve::sqlite3ResolveExprNames;
use crate::select::{
    sqlite3ColumnIndex, sqlite3GetVdbe, sqlite3Select, sqlite3SelectDelete, sqlite3SelectDestInit,
    sqlite3SelectNew,
};
use crate::trigger::{sqlite3CodeRowTrigger, sqlite3TriggerColmask, sqlite3TriggersExist};
use crate::vdbeaux::*;
use crate::where_::{sqlite3WhereBegin, sqlite3WhereEnd, sqlite3WhereUsesDeferredSeek};

/// The most recently coded instruction was an OP_Column to retrieve the
/// i-th column of table pTab. This routine sets the P4 parameter of the
/// OP_Column to the default value, if any.
///
/// The default value of a column is specified by a DEFAULT clause in the
/// column definition. This was either supplied by the user when the table
/// was created, or added later to the table definition by an ALTER TABLE
/// command. If the latter, then the row-records in the table btree on disk
/// may not contain a value for the column and the default value, taken
/// from the P4 parameter of the OP_Column instruction, is returned instead.
/// If the former, then all row-records are guaranteed to include a value
/// for the column and the P4 value is not required.
///
/// Column definitions created by an ALTER TABLE command may only have
/// literal default values specified: a number, null or a string. (If a more
/// complicated default expression value was provided, it is evaluated
/// when the ALTER TABLE is executed and one of the literal values written
/// into the sqlite_schema table.)
///
/// Therefore, the P4 parameter is only required if the default value for
/// the column is a literal number, string or null. The sqlite3ValueFromExpr()
/// function is capable of transforming these types of expressions into
/// sqlite3_value objects.
///
/// If column as REAL affinity and the table is an ordinary b-tree table
/// (not a virtual table) then the value might have been stored as an
/// integer.  In that case, add an OP_RealAffinity opcode to make sure
/// it has been converted into REAL.
fn columnDefaultUncommonCase(db: &mut Sqlite3, pTab: &TableRef, i: i32) {
    let enc = db.ENC();
    debug_assert!(!pTab.borrow().IsView());
    let affinity = pTab.borrow().aCol[i as usize].affinity;
    let e = sqlite3ColumnExprAuth(db, pTab, i as usize);
    let mut pValue: Option<Box<Mem>> = None;
    crate::vdbemem::sqlite3ValueFromExpr(db, e, enc, affinity, &mut pValue);
    if let Some(pValue) = pValue {
        sqlite3VdbeAppendP4(db.v(), P4::Mem(pValue), P4_MEM as i32);
    }
}

/// C: `void sqlite3ColumnDefault( Vdbe *v, Table *pTab, int i, int iReg )`
///
/// Rust: takes `db` (the Vdbe is db.v()) because the default value must be
/// computed with sqlite3ValueFromExpr() / sqlite3ColumnExprAuth().
pub fn sqlite3ColumnDefault(db: &mut Sqlite3, pTab: &TableRef, i: i32, iReg: i32) {
    debug_assert!((pTab.borrow().nCol as i32) > i);
    let (iDflt, affinity) = {
        let t = pTab.borrow();
        let c = &t.aCol[i as usize];
        (c.iDflt, c.affinity)
    };
    if iDflt != 0 {
        columnDefaultUncommonCase(db, pTab, i);
    }
    if affinity == SQLITE_AFF_REAL {
        sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, iReg);
    }
}

/// Check to see if column iCol of index pIdx references any of the
/// columns defined by aXRef and chngRowid.  Return true if it does
/// and false if not.  This is an optimization.  False-positives are a
/// performance degradation, but false-negatives can result in a corrupt
/// index and incorrect answers.
///
/// aXRef[j] will be non-negative if column j of the original table is
/// being updated.  chngRowid will be true if the rowid of the table is
/// being updated.
fn indexColumnIsBeingUpdated(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    iCol: i32,
    aXRef: &[i32],
    chngRowid: i32,
) -> bool {
    let iIdxCol = pIdx.borrow().aiColumn[iCol as usize];
    debug_assert!(iIdxCol != XN_ROWID); /* Cannot index rowid */
    if iIdxCol >= 0 {
        return aXRef[iIdxCol as usize] >= 0;
    }
    debug_assert!(iIdxCol == XN_EXPR);
    let t = pIdx.borrow().aColExpr.clone();
    let l = db.tree(&t);
    debug_assert!(l.nn());
    let e = db.el(l).a[iCol as usize].pExpr;
    sqlite3ExprReferencesUpdatedColumn(db, e, aXRef, chngRowid) != 0
}

/// Check to see if index pIdx is a partial index whose conditional
/// expression might change values due to an UPDATE.  Return true if
/// the index is subject to change and false if the index is guaranteed
/// to be unchanged.  This is an optimization.  False-positives are a
/// performance degradation, but false-negatives can result in a corrupt
/// index and incorrect answers.
///
/// aXRef[j] will be non-negative if column j of the original table is
/// being updated.  chngRowid will be true if the rowid of the table is
/// being updated.
fn indexWhereClauseMightChange(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    aXRef: &[i32],
    chngRowid: i32,
) -> bool {
    let t = pIdx.borrow().pPartIdxWhere.clone();
    if t.is_none() {
        return false;
    }
    let e = db.tree(&t);
    sqlite3ExprReferencesUpdatedColumn(db, e, aXRef, chngRowid) != 0
}

/// Allocate and return a pointer to an expression of type TK_ROW with
/// Expr.iColumn set to value (iCol+1). The resolver will modify the
/// expression to be a TK_COLUMN reading column iCol of the first
/// table in the source-list (pSrc->a[0]).
fn exprRowColumn(db: &mut Sqlite3, iCol: i32) -> ExprId {
    let pRet = sqlite3PExpr(db, TK_ROW as i32, ExprId::NULL, ExprId::NULL);
    if pRet.nn() {
        db.exm(pRet).iColumn = (iCol + 1) as ynVar;
    }
    pRet
}

/// Assuming both the pLimit and pOrderBy parameters are NULL, this function
/// generates VM code to run the query:
///
///   SELECT <other-columns>, pChanges FROM pTabList WHERE pWhere
///
/// and write the results to the ephemeral table already opened as cursor
/// iEph. None of pChanges, pTabList or pWhere are modified or consumed by
/// this function, they must be deleted by the caller.
///
/// Or, if pLimit and pOrderBy are not NULL, and pTab is not a view:
///
///   SELECT <other-columns>, pChanges FROM pTabList
///   WHERE pWhere
///   GROUP BY <other-columns>
///   ORDER BY pOrderBy LIMIT pLimit
///
/// If pTab is a view, the GROUP BY clause is omitted.
///
/// Exactly how results are written to table iEph, and exactly what
/// the <other-columns> in the query above are is determined by the type
/// of table pTabList->a[0].pTab.
///
/// If the table is a WITHOUT ROWID table, then argument pPk must be its
/// PRIMARY KEY. In this case <other-columns> are the primary key columns
/// of the table, in order. The results of the query are written to ephemeral
/// table iEph as index keys, using OP_IdxInsert.
///
/// If the table is actually a view, then <other-columns> are all columns of
/// the view. The results are written to the ephemeral table iEph as records
/// with automatically assigned integer keys.
///
/// If the table is a virtual or ordinary intkey table, then <other-columns>
/// is its rowid. For a virtual table, the results are written to iEph as
/// records with automatically assigned integer keys For intkey tables, the
/// rowid value in <other-columns> is used as the integer key, and the
/// remaining fields make up the table record.
fn updateFromSelect(
    db: &mut Sqlite3,
    iEph: i32,
    pPk: Option<&IndexRef>,
    pChanges: ExprListId,
    pTabList: SrcListId,
    pWhere: ExprId,
    _pOrderBy: ExprListId,
    _pLimit: ExprId,
) {
    let mut pList = ExprListId::NULL;
    let pGrp = ExprListId::NULL;
    let pLimit2 = ExprId::NULL;
    let pOrderBy2 = ExprListId::NULL;
    let pTab = db.src(pTabList).a[0].pSTab.clone().unwrap();
    let eDest;

    let pSrc = sqlite3SrcListDup(db, pTabList, 0);
    let pWhere2 = sqlite3ExprDup(db, pWhere, 0);

    debug_assert!(db.src(pTabList).nSrc() > 1);
    if pSrc.nn() {
        debug_assert!(db.src(pSrc).a[0].fg.notCte != 0);
        db.srcm(pSrc).a[0].iCursor = -1;
        if let Some(t) = db.srcm(pSrc).a[0].pSTab.take() {
            t.borrow_mut().nTabRef -= 1;
        }
    }
    let isVirtual = pTab.borrow().IsVirtual();
    if let Some(pk) = pPk {
        let n = pk.borrow().nKeyCol as usize;
        for i in 0..n {
            let c = pk.borrow().aiColumn[i] as i32;
            let pNew = exprRowColumn(db, c);
            pList = sqlite3ExprListAppend(db, pList, pNew);
        }
        eDest = if isVirtual { SRT_Table } else { SRT_Upfrom };
    } else if pTab.borrow().IsView() {
        let n = pTab.borrow().nCol as i32;
        for i in 0..n {
            let e = exprRowColumn(db, i);
            pList = sqlite3ExprListAppend(db, pList, e);
        }
        eDest = SRT_Table;
    } else {
        eDest = if isVirtual { SRT_Table } else { SRT_Upfrom };
        let e = sqlite3PExpr(db, TK_ROW as i32, ExprId::NULL, ExprId::NULL);
        pList = sqlite3ExprListAppend(db, ExprListId::NULL, e);
    }
    debug_assert!(pChanges.nn() || db.mallocFailed != 0);
    if pChanges.nn() {
        let n = db.el(pChanges).nExpr();
        for i in 0..n {
            let e = db.el(pChanges).a[i as usize].pExpr;
            let e = sqlite3ExprDup(db, e, 0);
            pList = sqlite3ExprListAppend(db, pList, e);
        }
    }
    let pSelect = sqlite3SelectNew(
        db,
        pList,
        pSrc,
        pWhere2,
        pGrp,
        ExprId::NULL,
        pOrderBy2,
        SF_UFSrcCheck | SF_IncludeHidden | SF_UpdateFrom,
        pLimit2,
    );
    if pSelect.nn() {
        db.selm(pSelect).selFlags |= SF_OrderByReqd;
    }
    let mut dest = SelectDest::default();
    sqlite3SelectDestInit(&mut dest, eDest as i32, iEph);
    dest.iSDParm2 = match pPk {
        Some(pk) => pk.borrow().nKeyCol as i32,
        None => -1,
    };
    sqlite3Select(db, pSelect, &mut dest);
    sqlite3SelectDelete(db, pSelect);
}

/// C: `void sqlite3Update( Parse *pParse, SrcList *pTabList, ExprList *pChanges, Expr *pWhere, int onError, ExprList *pOrderBy, Expr *pLimit, Upsert *pUpsert )`
///
/// Process an UPDATE statement.
///
///   UPDATE OR IGNORE tbl SET a=b, c=d FROM tbl2... WHERE e<5 AND f NOT NULL;
///          \_______/ \_/     \______/      \_____/       \________________/
///           onError   |      pChanges         |                pWhere
///                     \_______________________/
///                               pTabList
pub fn sqlite3Update(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pChanges: ExprListId,
    pWhere: ExprId,
    onError: i32,
    pOrderBy: ExprListId,
    pLimit: ExprId,
    pUpsert: UpsertId,
) {
    let mut sContext = AuthContext::default();
    let mut pOrderBy = pOrderBy;
    let mut pLimit = pLimit;

    'cleanup: {
        if db.ppr().nErr != 0 {
            break 'cleanup;
        }

        /* Locate the table which we want to update.
         */
        let pTab = match sqlite3SrcListLookup(db, pTabList) {
            Some(t) => t,
            None => break 'cleanup,
        };
        let iDb = db.schema_index(pTab.borrow().pSchema);

        /* Figure out if we have any triggers and if the table being
         ** updated is a view.
         */
        let mut tmask = 0;
        let pTrigger =
            sqlite3TriggersExist(db, &pTab, TK_UPDATE as i32, pChanges, Some(&mut tmask));
        let isView = pTab.borrow().IsView();
        debug_assert!(!pTrigger.is_empty() || tmask == 0);

        /* If there was a FROM clause, set nChangeFrom to the number of expressions
         ** in the change-list. Otherwise, set it to 0. There cannot be a FROM
         ** clause if this function is being called to generate code for part of
         ** an UPSERT statement.  */
        let nChangeFrom = if db.src(pTabList).nSrc() > 1 {
            db.el(pChanges).nExpr()
        } else {
            0
        };
        debug_assert!(nChangeFrom == 0 || pUpsert.is_null());

        if sqlite3ViewGetColumnNames(db, &pTab) != 0 {
            break 'cleanup;
        }
        if sqlite3IsReadOnly(db, &pTab, &pTrigger) != 0 {
            break 'cleanup;
        }

        let (nCol, iPKey, hasRowid, isVirtual, tabFlags, zTabName) = {
            let t = pTab.borrow();
            (
                t.nCol as i32,
                t.iPKey as i32,
                t.HasRowid(),
                t.IsVirtual(),
                t.tabFlags,
                t.zName.clone(),
            )
        };

        /* Allocate a cursors for the main database table and for all indices.
         ** The index cursors might not be used, but if they are used they
         ** need to occur right after the database cursor.  So go ahead and
         ** allocate enough space, just in case.
         */
        let iBaseCur = db.ppr().nTab;
        let mut iDataCur = iBaseCur;
        db.pp().nTab += 1;
        let mut iIdxCur = iDataCur + 1;
        let pPk = if hasRowid {
            None
        } else {
            sqlite3PrimaryKeyIndex(&pTab)
        };
        let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
        let nIdx = aIdx.len() as i32;
        for pIdx in aIdx.iter() {
            if let Some(pk) = &pPk {
                if std::rc::Rc::ptr_eq(pk, pIdx) {
                    iDataCur = db.ppr().nTab;
                }
            }
            db.pp().nTab += 1;
        }
        if pUpsert.nn() {
            /* On an UPSERT, reuse the same cursors already opened by INSERT */
            iDataCur = db.arena.ups(pUpsert).iDataCur;
            iIdxCur = db.arena.ups(pUpsert).iIdxCur;
            db.pp().nTab = iBaseCur;
        }
        db.srcm(pTabList).a[0].iCursor = iDataCur;

        /* Allocate space for aXRef[], aRegIdx[], and aToOpen[].
         ** Initialize aXRef[] and aToOpen[] to their default values.
         */
        let mut aXRef: Vec<i32> = vec![-1; nCol as usize];
        let mut aRegIdx: Vec<i32> = vec![0; (nIdx + 1) as usize];
        let mut aToOpen: Vec<u8> = vec![1; (nIdx + 2) as usize];
        aToOpen[(nIdx + 1) as usize] = 0;

        /* Initialize the name-context */
        let sNC = db.arena.new_nc(NameContext {
            pSrcList: pTabList,
            uNC: NameContextU {
                pUpsert,
                ..Default::default()
            },
            ncFlags: NC_UUpsert,
            ..Default::default()
        });

        /* Begin generating code. */
        if !sqlite3GetVdbe(db) {
            break 'cleanup;
        }

        /* Resolve the column names in all the expressions of the
         ** of the UPDATE statement.  Also find the column index
         ** for each column to be updated in the pChanges array.  For each
         ** column to be updated, make sure we have authorization to change
         ** that column.
         */
        let mut chngRowid: u8 = 0;
        let mut chngPk: u8 = 0;
        let mut pRowidExpr = ExprId::NULL;
        let mut iRowidExpr = -1;
        let nChanges = db.el(pChanges).nExpr();
        for i in 0..nChanges {
            let (e, zEName) = {
                let it = &db.el(pChanges).a[i as usize];
                (it.pExpr, it.zEName.clone())
            };
            /* If this is an UPDATE with a FROM clause, do not resolve expressions
             ** here. The call to sqlite3Select() below will do that. */
            if nChangeFrom == 0 && sqlite3ResolveExprNames(db, sNC, e) != 0 {
                break 'cleanup;
            }
            let mut j = sqlite3ColumnIndex(&pTab, zEName.as_deref());
            if j >= 0 {
                let (cf, zCn) = {
                    let t = pTab.borrow();
                    (
                        t.aCol[j as usize].colFlags,
                        t.aCol[j as usize].zCnName.clone(),
                    )
                };
                if j == iPKey {
                    chngRowid = 1;
                    pRowidExpr = e;
                    iRowidExpr = i;
                } else if pPk.is_some() && (cf & COLFLAG_PRIMKEY) != 0 {
                    chngPk = 1;
                } else if cf & COLFLAG_GENERATED != 0 {
                    errmsg!(db, "cannot UPDATE generated column \"%s\"", &zCn);
                    break 'cleanup;
                }
                aXRef[j as usize] = i;
            } else if pPk.is_none() && crate::expr::sqlite3IsRowid(zEName.as_deref()) != 0 {
                j = -1;
                chngRowid = 1;
                pRowidExpr = e;
                iRowidExpr = i;
            } else {
                errmsg!(db, "no such column: %s", &zEName);
                db.pp().checkSchema = 1;
                break 'cleanup;
            }
            {
                let zCol: ZStr = if j < 0 {
                    b"ROWID".to_vec()
                } else {
                    pTab.borrow().aCol[j as usize].zCnName.clone()
                };
                let zDb = db.aDb[iDb as usize].zDbSName.clone();
                let rc = sqlite3AuthCheck(
                    db,
                    SQLITE_UPDATE,
                    Some(&zTabName),
                    Some(&zCol),
                    zDb.as_deref(),
                );
                if rc == SQLITE_DENY {
                    break 'cleanup;
                } else if rc == SQLITE_IGNORE {
                    // C writes aXRef[j] even for j==-1 (out of bounds); skip that.
                    if j >= 0 {
                        aXRef[j as usize] = -1;
                    }
                }
            }
        }
        debug_assert!((chngRowid & chngPk) == 0);
        let chngKey = chngRowid + chngPk;

        /* Mark generated columns as changing if their generator expressions
         ** reference any changing column.  The actual aXRef[] value for
         ** generated expressions is not used, other than to check to see that it
         ** is non-negative, so the value of aXRef[] for generated columns can be
         ** set to any non-negative number.  We use 99999 so that the value is
         ** obvious when looking at aXRef[] in a symbolic debugger.
         */
        if tabFlags & TF_HasGenerated != 0 {
            loop {
                let mut bProgress = false;
                for i in 0..nCol as usize {
                    if aXRef[i] >= 0 {
                        continue;
                    }
                    if (pTab.borrow().aCol[i].colFlags & COLFLAG_GENERATED) == 0 {
                        continue;
                    }
                    let e = sqlite3ColumnExpr(db, &pTab, i);
                    if sqlite3ExprReferencesUpdatedColumn(db, e, &aXRef, chngRowid as i32) != 0 {
                        aXRef[i] = 99999;
                        bProgress = true;
                    }
                }
                if !bProgress {
                    break;
                }
            }
        }

        /* The SET expressions are not actually used inside the WHERE loop.
         ** So reset the colUsed mask. Unless this is a virtual table. In that
         ** case, set all bits of the colUsed mask (to ensure that the virtual
         ** table implementation makes all columns available).
         */
        db.srcm(pTabList).a[0].colUsed = if isVirtual { ALLBITS } else { 0 };

        let hasFK = sqlite3FkRequired(db, &pTab, Some(&aXRef), chngKey as i32);

        /* There is one entry in the aRegIdx[] array for each index on the table
         ** being updated.  Fill in aRegIdx[] with a register number that will hold
         ** the key for accessing each index.
         */
        let mut bReplace = (onError == OE_Replace as i32) as i32;
        let mut nAllIdx = 0usize;
        for pIdx in aIdx.iter() {
            let mut reg;
            let isPk = pPk
                .as_ref()
                .map_or(false, |pk| std::rc::Rc::ptr_eq(pk, pIdx));
            let (nColumn, nKeyCol, idxOnError) = {
                let x = pIdx.borrow();
                (x.nColumn as i32, x.nKeyCol as i32, x.onError)
            };
            if chngKey != 0
                || hasFK > 1
                || isPk
                || indexWhereClauseMightChange(db, pIdx, &aXRef, chngRowid as i32)
            {
                db.pp().nMem += 1;
                reg = db.ppr().nMem;
                db.pp().nMem += nColumn;
            } else {
                reg = 0;
                for i in 0..nKeyCol {
                    if indexColumnIsBeingUpdated(db, pIdx, i, &aXRef, chngRowid as i32) {
                        db.pp().nMem += 1;
                        reg = db.ppr().nMem;
                        db.pp().nMem += nColumn;
                        if onError == OE_Default as i32 && idxOnError == OE_Replace {
                            bReplace = 1;
                        }
                        break;
                    }
                }
            }
            if reg == 0 {
                aToOpen[nAllIdx + 1] = 0;
            }
            aRegIdx[nAllIdx] = reg;
            nAllIdx += 1;
        }
        db.pp().nMem += 1;
        aRegIdx[nAllIdx] = db.ppr().nMem; /* Register storing the table record */
        if bReplace != 0 {
            /* If REPLACE conflict resolution might be invoked, open cursors on all
             ** indexes in case they are needed to delete records.  */
            for x in aToOpen.iter_mut().take((nIdx + 1) as usize) {
                *x = 1;
            }
        }

        if db.ppr().nested == 0 {
            sqlite3VdbeCountChanges(db.v());
        }
        sqlite3BeginWriteOperation(db, (!pTrigger.is_empty() || hasFK != 0) as i32, iDb);

        let mut regRowSet = 0; /* Rowset of rows to be updated */
        let mut regOldRowid = 0; /* The old rowid */
        let mut regNewRowid = 0; /* The new rowid */
        let mut regNew = 0; /* Content of the NEW.* table in triggers */
        let mut regOld = 0; /* Content of OLD.* table in triggers */

        /* Allocate required registers. */
        if !isVirtual {
            /* For now, regRowSet and aRegIdx[nAllIdx] share the same register.
             ** If regRowSet turns out to be needed, then aRegIdx[nAllIdx] will be
             ** reallocated.  aRegIdx[nAllIdx] is the register in which the main
             ** table record is written.  regRowSet holds the RowSet for the
             ** two-pass update algorithm. */
            debug_assert!(aRegIdx[nAllIdx] == db.ppr().nMem);
            regRowSet = aRegIdx[nAllIdx];
            db.pp().nMem += 1;
            regOldRowid = db.ppr().nMem;
            regNewRowid = regOldRowid;
            if chngPk != 0 || !pTrigger.is_empty() || hasFK != 0 {
                regOld = db.ppr().nMem + 1;
                db.pp().nMem += nCol;
            }
            if chngKey != 0 || !pTrigger.is_empty() || hasFK != 0 {
                db.pp().nMem += 1;
                regNewRowid = db.ppr().nMem;
            }
            regNew = db.ppr().nMem + 1;
            db.pp().nMem += nCol;
        }

        /* Start the view context. */
        if isView {
            sqlite3AuthContextPush(db, &mut sContext, Some(&zTabName));
        }

        /* If we are trying to update a view, realize that view into
         ** an ephemeral table.
         */
        if nChangeFrom == 0 && isView {
            sqlite3MaterializeView(db, &pTab, pWhere, pOrderBy, pLimit, iDataCur);
            pOrderBy = ExprListId::NULL;
            pLimit = ExprId::NULL;
        }

        /* Resolve the column names in all the expressions in the
         ** WHERE clause.
         */
        if nChangeFrom == 0 && sqlite3ResolveExprNames(db, sNC, pWhere) != 0 {
            break 'cleanup;
        }

        /* Virtual tables must be handled separately */
        if isVirtual {
            updateVirtualTable(
                db, pTabList, &pTab, pChanges, pRowidExpr, &aXRef, pWhere, onError,
            );
            break 'cleanup;
        }

        /* Jump to labelBreak to abandon further processing of this UPDATE */
        let labelBreak = sqlite3VdbeMakeLabel(db);
        let mut labelContinue = labelBreak;

        /* Not an UPSERT.  Normal processing.  Begin by
         ** initialize the count of updated rows */
        let mut regRowCount = 0;
        if (db.flags & SQLITE_CountRows) != 0
            && db.ppr().pTriggerTab.is_none()
            && db.ppr().nested == 0
            && db.ppr().bReturning == 0
            && pUpsert.is_null()
        {
            db.pp().nMem += 1;
            regRowCount = db.ppr().nMem;
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regRowCount);
        }

        let mut iEph = 0; /* Ephemeral table holding all primary key values */
        let mut nKey = 0; /* Number of elements in regKey for WITHOUT ROWID */
        let mut addrOpen = 0; /* Address of OP_OpenEphemeral */
        let mut iPk = 0; /* First of nPk cells holding PRIMARY KEY value */
        let mut nPk: i16 = 0; /* Number of components of the PRIMARY KEY */
        let mut regKey = 0; /* composite PRIMARY KEY value */
        let mut aiCurOnePass = [0i32; 2];
        let mut bFinishSeek = 1; /* The OP_FinishSeek opcode is needed */
        let eOnePass: i32;
        let mut pWInfo: Option<Box<crate::where_::WhereInfo>> = None;
        let mut addrTop = 0;

        if nChangeFrom == 0 && hasRowid {
            sqlite3VdbeAddOp3(db.v(), OP_Null, 0, regRowSet, regOldRowid);
            iEph = db.ppr().nTab;
            db.pp().nTab += 1;
            addrOpen = sqlite3VdbeAddOp3(db.v(), OP_OpenEphemeral, iEph, 0, regRowSet);
        } else {
            debug_assert!(pPk.is_some() || hasRowid);
            nPk = pPk.as_ref().map_or(0, |pk| pk.borrow().nKeyCol as i16);
            iPk = db.ppr().nMem + 1;
            db.pp().nMem += nPk as i32;
            db.pp().nMem += nChangeFrom;
            db.pp().nMem += 1;
            regKey = db.ppr().nMem;
            if pUpsert.is_null() {
                let nEphCol = nPk as i32 + nChangeFrom + if isView { nCol } else { 0 };
                iEph = db.ppr().nTab;
                db.pp().nTab += 1;
                if pPk.is_some() {
                    sqlite3VdbeAddOp3(db.v(), OP_Null, 0, iPk, iPk + nPk as i32 - 1);
                }
                addrOpen = sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, iEph, nEphCol);
                if let Some(pk) = &pPk {
                    if let Some(pKeyInfo) = sqlite3KeyInfoOfIndex(db, pk) {
                        pKeyInfo.borrow_mut().nAllField = nEphCol as u16;
                        sqlite3VdbeAppendP4(db.v(), P4::KeyInfo(pKeyInfo), P4_KEYINFO as i32);
                    }
                }
                if nChangeFrom != 0 {
                    updateFromSelect(
                        db,
                        iEph,
                        pPk.as_ref(),
                        pChanges,
                        pTabList,
                        pWhere,
                        pOrderBy,
                        pLimit,
                    );
                    if isView {
                        iDataCur = iEph;
                    }
                }
            }
        }

        if nChangeFrom != 0 {
            sqlite3MultiWrite(db);
            eOnePass = ONEPASS_OFF;
            nKey = nPk as i32;
            regKey = iPk;
        } else {
            let mut eOP;
            if pUpsert.nn() {
                /* If this is an UPSERT, then all cursors have already been opened by
                 ** the outer INSERT and the data cursor should be pointing at the row
                 ** that is to be updated.  So bypass the code that searches for the
                 ** row(s) to be updated.
                 */
                pWInfo = None;
                eOP = ONEPASS_SINGLE;
                sqlite3ExprIfFalse(db, pWhere, labelBreak, SQLITE_JUMPIFNULL as i32);
                bFinishSeek = 0;
            } else {
                /* Begin the database scan.
                 **
                 ** Do not consider a single-pass strategy for a multi-row update if
                 ** there is anything that might disrupt the cursor being used to do
                 ** the UPDATE:
                 **   (1) This is a nested UPDATE
                 **   (2) There are triggers
                 **   (3) There are FOREIGN KEY constraints
                 **   (4) There are REPLACE conflict handlers
                 **   (5) There are subqueries in the WHERE clause
                 */
                let mut flags: u16 = WHERE_ONEPASS_DESIRED;
                if db.ppr().nested == 0
                    && pTrigger.is_empty()
                    && hasFK == 0
                    && chngKey == 0
                    && bReplace == 0
                    && (pWhere.is_null() || !db.ex(pWhere).HasProperty(EP_Subquery))
                {
                    flags |= WHERE_ONEPASS_MULTIROW;
                }
                let mut w = match sqlite3WhereBegin(
                    db,
                    pTabList,
                    pWhere,
                    ExprListId::NULL,
                    ExprListId::NULL,
                    SelectId::NULL,
                    flags,
                    iIdxCur,
                ) {
                    Some(w) => w,
                    None => break 'cleanup,
                };

                /* A one-pass strategy that might update more than one row may not
                 ** be used if any column of the index used for the scan is being
                 ** updated. Otherwise, if there is an index on "b", statements like
                 ** the following could create an infinite loop:
                 **
                 **   UPDATE t1 SET b=b+1 WHERE b>?
                 **
                 ** Fall back to ONEPASS_OFF if where.c has selected a ONEPASS_MULTI
                 ** strategy that uses an index for which one or more columns are being
                 ** updated.  */
                eOP = crate::where_::sqlite3WhereOkOnePass(&mut w, &mut aiCurOnePass);
                bFinishSeek = sqlite3WhereUsesDeferredSeek(&mut w);
                if eOP != ONEPASS_SINGLE {
                    sqlite3MultiWrite(db);
                    if eOP == ONEPASS_MULTI {
                        let iCur = aiCurOnePass[1];
                        if iCur >= 0 && iCur != iDataCur && aToOpen[(iCur - iBaseCur) as usize] != 0
                        {
                            eOP = ONEPASS_OFF;
                        }
                        debug_assert!(iCur != iDataCur || !hasRowid);
                    }
                }
                pWInfo = Some(w);
            }

            if hasRowid {
                /* Read the rowid of the current row of the WHERE scan. In ONEPASS_OFF
                 ** mode, write the rowid into the FIFO. In either of the one-pass modes,
                 ** leave it in register regOldRowid.  */
                sqlite3VdbeAddOp2(db.v(), OP_Rowid, iDataCur, regOldRowid);
                if eOP == ONEPASS_OFF {
                    db.pp().nMem += 1;
                    aRegIdx[nAllIdx] = db.ppr().nMem;
                    sqlite3VdbeAddOp3(db.v(), OP_Insert, iEph, regRowSet, regOldRowid);
                } else if addrOpen != 0 {
                    sqlite3VdbeChangeToNoop(db.v(), addrOpen);
                }
            } else {
                /* Read the PK of the current row into an array of registers. In
                 ** ONEPASS_OFF mode, serialize the array into a record and store it in
                 ** the ephemeral table. Or, in ONEPASS_SINGLE or MULTI mode, change
                 ** the OP_OpenEphemeral instruction to a Noop (the ephemeral table
                 ** is not required) and leave the PK fields in the array of registers.  */
                let pk = pPk.as_ref().unwrap();
                for i in 0..nPk as usize {
                    let c = pk.borrow().aiColumn[i];
                    debug_assert!(c >= 0);
                    sqlite3ExprCodeGetColumnOfTable(db, &pTab, iDataCur, c as i32, iPk + i as i32);
                }
                if eOP != 0 {
                    if addrOpen != 0 {
                        sqlite3VdbeChangeToNoop(db.v(), addrOpen);
                    }
                    nKey = nPk as i32;
                    regKey = iPk;
                } else {
                    let mut zAff = sqlite3IndexAffinityStr(db, pk).unwrap_or_default();
                    zAff.truncate(nPk as usize);
                    sqlite3VdbeAddOp4(
                        db.v(),
                        OP_MakeRecord,
                        iPk,
                        nPk as i32,
                        regKey,
                        P4::Z(zAff),
                        P4_DYNAMIC as i32,
                    );
                    sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, iEph, regKey, iPk, nPk as i32);
                }
            }
            eOnePass = eOP;
        }

        if pUpsert.is_null() {
            if nChangeFrom == 0 && eOnePass != ONEPASS_MULTI {
                if let Some(w) = pWInfo.as_mut() {
                    sqlite3WhereEnd(db, w);
                }
            }

            if !isView {
                let mut addrOnce = 0;
                let mut iNotUsed1 = 0;
                let mut iNotUsed2 = 0;

                /* Open every index that needs updating. */
                if eOnePass != ONEPASS_OFF {
                    if aiCurOnePass[0] >= 0 {
                        aToOpen[(aiCurOnePass[0] - iBaseCur) as usize] = 0;
                    }
                    if aiCurOnePass[1] >= 0 {
                        aToOpen[(aiCurOnePass[1] - iBaseCur) as usize] = 0;
                    }
                }

                if eOnePass == ONEPASS_MULTI && (nIdx - (aiCurOnePass[1] >= 0) as i32) > 0 {
                    addrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
                }
                sqlite3OpenTableAndIndices(
                    db,
                    &pTab,
                    OP_OpenWrite as i32,
                    0,
                    iBaseCur,
                    Some(&aToOpen),
                    &mut iNotUsed1,
                    &mut iNotUsed2,
                );
                if addrOnce != 0 {
                    sqlite3VdbeJumpHereOrPopInst(db.v(), addrOnce);
                }
            }

            /* Top of the update loop */
            if eOnePass != ONEPASS_OFF {
                if aiCurOnePass[0] != iDataCur && aiCurOnePass[1] != iDataCur {
                    debug_assert!(pPk.is_some());
                    sqlite3VdbeAddOp4Int(db.v(), OP_NotFound, iDataCur, labelBreak, regKey, nKey);
                }
                if eOnePass != ONEPASS_SINGLE {
                    labelContinue = sqlite3VdbeMakeLabel(db);
                }
                sqlite3VdbeAddOp2(
                    db.v(),
                    OP_IsNull,
                    if pPk.is_some() { regKey } else { regOldRowid },
                    labelBreak,
                );
            } else if pPk.is_some() || nChangeFrom != 0 {
                labelContinue = sqlite3VdbeMakeLabel(db);
                sqlite3VdbeAddOp2(db.v(), OP_Rewind, iEph, labelBreak);
                addrTop = sqlite3VdbeCurrentAddr(db.v());
                if nChangeFrom != 0 {
                    if !isView {
                        if pPk.is_some() {
                            for i in 0..nPk as i32 {
                                sqlite3VdbeAddOp3(db.v(), OP_Column, iEph, i, iPk + i);
                            }
                            sqlite3VdbeAddOp4Int(
                                db.v(),
                                OP_NotFound,
                                iDataCur,
                                labelContinue,
                                iPk,
                                nPk as i32,
                            );
                        } else {
                            sqlite3VdbeAddOp2(db.v(), OP_Rowid, iEph, regOldRowid);
                            sqlite3VdbeAddOp3(
                                db.v(),
                                OP_NotExists,
                                iDataCur,
                                labelContinue,
                                regOldRowid,
                            );
                        }
                    }
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_RowData, iEph, regKey);
                    sqlite3VdbeAddOp4Int(db.v(), OP_NotFound, iDataCur, labelContinue, regKey, 0);
                }
            } else {
                sqlite3VdbeAddOp2(db.v(), OP_Rewind, iEph, labelBreak);
                labelContinue = sqlite3VdbeMakeLabel(db);
                addrTop = sqlite3VdbeAddOp2(db.v(), OP_Rowid, iEph, regOldRowid);
                sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDataCur, labelContinue, regOldRowid);
            }
        }

        /* If the rowid value will change, set register regNewRowid to
         ** contain the new value. If the rowid is not being modified,
         ** then regNewRowid is the same register as regOldRowid, which is
         ** already populated.  */
        debug_assert!(
            chngKey != 0 || !pTrigger.is_empty() || hasFK != 0 || regOldRowid == regNewRowid
        );
        if chngRowid != 0 {
            debug_assert!(iRowidExpr >= 0);
            if nChangeFrom == 0 {
                sqlite3ExprCode(db, pRowidExpr, regNewRowid);
            } else {
                sqlite3VdbeAddOp3(db.v(), OP_Column, iEph, iRowidExpr, regNewRowid);
            }
            sqlite3VdbeAddOp1(db.v(), OP_MustBeInt, regNewRowid);
            sqlite3MayAbort(db);
        }

        /* Compute the old pre-UPDATE content of the row being changed, if that
         ** information is needed */
        if chngPk != 0 || hasFK != 0 || !pTrigger.is_empty() {
            let mut oldmask = if hasFK != 0 {
                sqlite3FkOldmask(db, &pTab)
            } else {
                0
            };
            oldmask |= sqlite3TriggerColmask(
                db,
                &pTrigger,
                pChanges,
                0,
                (TRIGGER_BEFORE | TRIGGER_AFTER) as i32,
                &pTab,
                onError,
            );
            for i in 0..nCol {
                let colFlags = pTab.borrow().aCol[i as usize].colFlags;
                let k = sqlite3TableColumnToStorage(&pTab, i as i16) as i32 + regOld;
                if oldmask == 0xffffffff
                    || (i < 32 && (oldmask & MASKBIT32(i)) != 0)
                    || (colFlags & COLFLAG_PRIMKEY) != 0
                {
                    sqlite3ExprCodeGetColumnOfTable(db, &pTab, iDataCur, i, k);
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, k);
                }
            }
            if chngRowid == 0 && pPk.is_none() {
                sqlite3VdbeAddOp2(db.v(), OP_Copy, regOldRowid, regNewRowid);
            }
        }

        /* Populate the array of registers beginning at regNew with the new
         ** row data. This array is used to check constants, create the new
         ** table and index records, and as the values for any new.* references
         ** made by triggers.
         **
         ** If there are one or more BEFORE triggers, then do not populate the
         ** registers associated with columns that are (a) not modified by
         ** this UPDATE statement and (b) not accessed by new.* references. The
         ** values for registers not modified by the UPDATE must be reloaded from
         ** the database after the BEFORE triggers are fired anyway (as the trigger
         ** may have modified them). So not loading those that are not going to
         ** be used eliminates some redundant opcodes.
         */
        let newmask = sqlite3TriggerColmask(
            db,
            &pTrigger,
            pChanges,
            1,
            TRIGGER_BEFORE as i32,
            &pTab,
            onError,
        );
        let mut k = regNew;
        for i in 0..nCol {
            let colFlags = pTab.borrow().aCol[i as usize].colFlags;
            if i == iPKey {
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, k);
            } else if (colFlags & COLFLAG_GENERATED) != 0 {
                if colFlags & COLFLAG_VIRTUAL != 0 {
                    k -= 1;
                }
            } else {
                let j = aXRef[i as usize];
                if j >= 0 {
                    if nChangeFrom != 0 {
                        let nOff = if isView { nCol } else { nPk as i32 };
                        debug_assert!(eOnePass == ONEPASS_OFF);
                        sqlite3VdbeAddOp3(db.v(), OP_Column, iEph, nOff + j, k);
                    } else {
                        let e = db.el(pChanges).a[j as usize].pExpr;
                        sqlite3ExprCode(db, e, k);
                    }
                } else if 0 == (tmask & TRIGGER_BEFORE as i32)
                    || i > 31
                    || (newmask & MASKBIT32(i)) != 0
                {
                    /* This branch loads the value of a column that will not be changed
                     ** into a register. This is done if there are no BEFORE triggers, or
                     ** if there are one or more BEFORE triggers that use this value via
                     ** a new.* reference in a trigger program.
                     */
                    sqlite3ExprCodeGetColumnOfTable(db, &pTab, iDataCur, i, k);
                    bFinishSeek = 0;
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, k);
                }
            }
            k += 1;
        }
        if tabFlags & TF_HasGenerated != 0 {
            sqlite3ComputeGeneratedColumns(db, regNew, &pTab);
        }

        /* Fire any BEFORE UPDATE triggers. This happens before constraints are
         ** verified. One could argue that this is wrong.
         */
        if (tmask & TRIGGER_BEFORE as i32) != 0 {
            sqlite3TableAffinity(db.v(), &pTab, regNew);
            sqlite3CodeRowTrigger(
                db,
                &pTrigger,
                TK_UPDATE as i32,
                pChanges,
                TRIGGER_BEFORE as i32,
                &pTab,
                regOldRowid,
                onError,
                labelContinue,
            );

            if !isView {
                /* The row-trigger may have deleted the row being updated. In this
                 ** case, jump to the next row. No updates or AFTER triggers are
                 ** required. This behavior - what happens when the row being updated
                 ** is deleted or renamed by a BEFORE trigger - is left undefined in the
                 ** documentation.
                 */
                if pPk.is_some() {
                    sqlite3VdbeAddOp4Int(
                        db.v(),
                        OP_NotFound,
                        iDataCur,
                        labelContinue,
                        regKey,
                        nKey,
                    );
                } else {
                    sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDataCur, labelContinue, regOldRowid);
                }

                /* After-BEFORE-trigger-reload-loop:
                 ** If it did not delete it, the BEFORE trigger may still have modified
                 ** some of the columns of the row being updated. Load the values for
                 ** all columns not modified by the update statement into their registers
                 ** in case this has happened. Only unmodified columns are reloaded.
                 ** The values computed for modified columns use the values before the
                 ** BEFORE trigger runs.  See test case trigger1-18.0 (added 2018-04-26)
                 ** for an example.
                 */
                let mut k = regNew;
                for i in 0..nCol {
                    let colFlags = pTab.borrow().aCol[i as usize].colFlags;
                    if colFlags & COLFLAG_GENERATED != 0 {
                        if colFlags & COLFLAG_VIRTUAL != 0 {
                            k -= 1;
                        }
                    } else if aXRef[i as usize] < 0 && i != iPKey {
                        sqlite3ExprCodeGetColumnOfTable(db, &pTab, iDataCur, i, k);
                    }
                    k += 1;
                }
                if tabFlags & TF_HasGenerated != 0 {
                    sqlite3ComputeGeneratedColumns(db, regNew, &pTab);
                }
            }
        }

        if !isView {
            /* Do constraint checks. */
            debug_assert!(regOldRowid > 0);
            sqlite3GenerateConstraintChecks(
                db,
                &pTab,
                &aRegIdx,
                iDataCur,
                iIdxCur,
                regNewRowid,
                regOldRowid,
                chngKey,
                onError as u8,
                labelContinue,
                &mut bReplace,
                Some(&aXRef),
                UpsertId::NULL,
            );

            /* If REPLACE conflict handling may have been used, or if the PK of the
             ** row is changing, then the GenerateConstraintChecks() above may have
             ** moved cursor iDataCur. Reseek it. */
            if bReplace != 0 || chngKey != 0 {
                if pPk.is_some() {
                    sqlite3VdbeAddOp4Int(
                        db.v(),
                        OP_NotFound,
                        iDataCur,
                        labelContinue,
                        regKey,
                        nKey,
                    );
                } else {
                    sqlite3VdbeAddOp3(db.v(), OP_NotExists, iDataCur, labelContinue, regOldRowid);
                }
            }

            /* Do FK constraint checks. */
            if hasFK != 0 {
                sqlite3FkCheck(db, &pTab, regOldRowid, 0, Some(&aXRef), chngKey as i32);
            }

            /* Delete the index entries associated with the current record.  */
            sqlite3GenerateRowIndexDelete(db, &pTab, iDataCur, iIdxCur, Some(&aRegIdx), -1);

            /* We must run the OP_FinishSeek opcode to resolve a prior
             ** OP_DeferredSeek if there is any possibility that there have been
             ** no OP_Column opcodes since the OP_DeferredSeek was issued.  But
             ** we want to avoid the OP_FinishSeek if possible, as running it
             ** costs CPU cycles. */
            if bFinishSeek != 0 {
                sqlite3VdbeAddOp1(db.v(), OP_FinishSeek, iDataCur);
            }

            /* If changing the rowid value, or if there are foreign key constraints
             ** to process, delete the old record. Otherwise, add a noop OP_Delete
             ** to invoke the pre-update hook.
             **
             ** That (regNew==regnewRowid+1) is true is also important for the
             ** pre-update hook. If the caller invokes preupdate_new(), the returned
             ** value is copied from memory cell (regNewRowid+1+iCol), where iCol
             ** is the column index supplied by the user.
             */
            debug_assert!(regNew == regNewRowid + 1);
            if hasFK > 1 || chngKey != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_Delete, iDataCur, 0);
            }

            if hasFK != 0 {
                sqlite3FkCheck(db, &pTab, 0, regNewRowid, Some(&aXRef), chngKey as i32);
            }

            /* Insert the new index entries and the new record. */
            sqlite3CompleteInsertion(
                db,
                &pTab,
                iDataCur,
                iIdxCur,
                regNewRowid,
                &aRegIdx,
                OPFLAG_ISUPDATE as i32
                    | if eOnePass == ONEPASS_MULTI {
                        OPFLAG_SAVEPOSITION as i32
                    } else {
                        0
                    },
                0,
                0,
            );

            /* Do any ON CASCADE, SET NULL or SET DEFAULT operations required to
             ** handle rows (possibly in other tables) that refer via a foreign key
             ** to the row just updated. */
            if hasFK != 0 {
                sqlite3FkActions(
                    db,
                    &pTab,
                    pChanges,
                    regOldRowid,
                    Some(&aXRef),
                    chngKey as i32,
                );
            }
        }

        /* Increment the row counter
         */
        if regRowCount != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_AddImm, regRowCount, 1);
        }

        if !pTrigger.is_empty() {
            sqlite3CodeRowTrigger(
                db,
                &pTrigger,
                TK_UPDATE as i32,
                pChanges,
                TRIGGER_AFTER as i32,
                &pTab,
                regOldRowid,
                onError,
                labelContinue,
            );
        }

        /* Repeat the above with the next record to be updated, until
         ** all record selected by the WHERE clause have been updated.
         */
        if eOnePass == ONEPASS_SINGLE {
            /* Nothing to do at end-of-loop for a single-pass */
        } else if eOnePass == ONEPASS_MULTI {
            sqlite3VdbeResolveLabel(db.v(), labelContinue);
            if let Some(w) = pWInfo.as_mut() {
                sqlite3WhereEnd(db, w);
            }
        } else {
            sqlite3VdbeResolveLabel(db.v(), labelContinue);
            sqlite3VdbeAddOp2(db.v(), OP_Next, iEph, addrTop);
        }
        sqlite3VdbeResolveLabel(db.v(), labelBreak);

        /* Update the sqlite_sequence table by storing the content of the
         ** maximum rowid counter values recorded while inserting into
         ** autoincrement tables.
         */
        if db.ppr().nested == 0 && db.ppr().pTriggerTab.is_none() && pUpsert.is_null() {
            sqlite3AutoincrementEnd(db);
        }

        /*
         ** Return the number of rows that were changed, if we are tracking
         ** that information.
         */
        if regRowCount != 0 {
            sqlite3CodeChangeCount(db.v(), regRowCount, Some(b"rows updated"));
        }
    }

    // update_cleanup:
    sqlite3AuthContextPop(db, &mut sContext);
    sqlite3SrcListDelete(db, pTabList);
    sqlite3ExprListDelete(db, pChanges);
    sqlite3ExprDelete(db, pWhere);
}

/// Generate code for an UPDATE of a virtual table.
///
/// There are two possible strategies - the default and the special
/// "onepass" strategy. Onepass is only used if the virtual table
/// implementation indicates that pWhere may match at most one row.
///
/// The default strategy is to create an ephemeral table that contains
/// for each row to be changed:
///
///   (A)  The original rowid of that row.
///   (B)  The revised rowid for the row.
///   (C)  The content of every column in the row.
///
/// Then loop through the contents of this ephemeral table executing a
/// VUpdate for each row. When finished, drop the ephemeral table.
///
/// The "onepass" strategy does not use an ephemeral table. Instead, it
/// stores the same values (A, B and C above) in a register array and
/// makes a single invocation of VUpdate.
fn updateVirtualTable(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    pTab: &TableRef,
    pChanges: ExprListId,
    pRowid: ExprId,
    aXRef: &[i32],
    pWhere: ExprId,
    onError: i32,
) {
    let pVTab = crate::vtab::sqlite3VtabP4(db, pTab);
    let mut pWInfo: Option<Box<crate::where_::WhereInfo>> = None;
    let nCol = pTab.borrow().nCol as i32;
    let nArg = 2 + nCol; /* Number of arguments to VUpdate */
    let iCsr = db.src(pSrc).a[0].iCursor; /* Cursor used for virtual table scan */
    let mut aDummy = [0i32; 2]; /* Unused arg for sqlite3WhereOkOnePass() */
    let eOnePass: i32; /* True to use onepass strategy */
    let hasRowid = pTab.borrow().HasRowid();

    /* Allocate nArg registers in which to gather the arguments for VUpdate. Then
     ** create and open the ephemeral table in which the records created from
     ** these arguments will be temporarily stored. */
    let ephemTab = db.ppr().nTab;
    db.pp().nTab += 1;
    let mut addr = sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, ephemTab, nArg);
    let regArg = db.ppr().nMem + 1;
    db.pp().nMem += nArg;
    let nSrc = db.src(pSrc).nSrc();
    if nSrc > 1 {
        let mut pPk: Option<IndexRef> = None;
        let pRow;
        if hasRowid {
            if pRowid.nn() {
                pRow = sqlite3ExprDup(db, pRowid, 0);
            } else {
                pRow = sqlite3PExpr(db, TK_ROW as i32, ExprId::NULL, ExprId::NULL);
            }
        } else {
            let pk = sqlite3PrimaryKeyIndex(pTab).unwrap();
            debug_assert!(pk.borrow().nKeyCol == 1);
            let iPk = pk.borrow().aiColumn[0];
            if aXRef[iPk as usize] >= 0 {
                let e = db.el(pChanges).a[aXRef[iPk as usize] as usize].pExpr;
                pRow = sqlite3ExprDup(db, e, 0);
            } else {
                pRow = exprRowColumn(db, iPk as i32);
            }
            pPk = Some(pk);
        }
        let mut pList = sqlite3ExprListAppend(db, ExprListId::NULL, pRow);

        for i in 0..nCol as usize {
            if aXRef[i] >= 0 {
                let e = db.el(pChanges).a[aXRef[i] as usize].pExpr;
                let e = sqlite3ExprDup(db, e, 0);
                pList = sqlite3ExprListAppend(db, pList, e);
            } else {
                let pRowExpr = exprRowColumn(db, i as i32);
                if pRowExpr.nn() {
                    db.exm(pRowExpr).op2 = OPFLAG_NOCHNG as u8;
                }
                pList = sqlite3ExprListAppend(db, pList, pRowExpr);
            }
        }

        updateFromSelect(
            db,
            ephemTab,
            pPk.as_ref(),
            pList,
            pSrc,
            pWhere,
            ExprListId::NULL,
            ExprId::NULL,
        );
        sqlite3ExprListDelete(db, pList);
        eOnePass = ONEPASS_OFF;
    } else {
        db.pp().nMem += 1;
        let regRec = db.ppr().nMem;
        db.pp().nMem += 1;
        let regRowid = db.ppr().nMem;

        /* Start scanning the virtual table */
        let mut w = match sqlite3WhereBegin(
            db,
            pSrc,
            pWhere,
            ExprListId::NULL,
            ExprListId::NULL,
            SelectId::NULL,
            WHERE_ONEPASS_DESIRED,
            0,
        ) {
            Some(w) => w,
            None => return,
        };

        /* Populate the argument registers. */
        for i in 0..nCol {
            debug_assert!((pTab.borrow().aCol[i as usize].colFlags & COLFLAG_GENERATED) == 0);
            if aXRef[i as usize] >= 0 {
                let e = db.el(pChanges).a[aXRef[i as usize] as usize].pExpr;
                sqlite3ExprCode(db, e, regArg + 2 + i);
            } else {
                sqlite3VdbeAddOp3(db.v(), OP_VColumn, iCsr, i, regArg + 2 + i);
                sqlite3VdbeChangeP5(db.v(), OPFLAG_NOCHNG as u16); /* For sqlite3_vtab_nochange() */
            }
        }
        if hasRowid {
            sqlite3VdbeAddOp2(db.v(), OP_Rowid, iCsr, regArg);
            if pRowid.nn() {
                sqlite3ExprCode(db, pRowid, regArg + 1);
            } else {
                sqlite3VdbeAddOp2(db.v(), OP_Rowid, iCsr, regArg + 1);
            }
        } else {
            let pk = sqlite3PrimaryKeyIndex(pTab).unwrap();
            debug_assert!(pk.borrow().nKeyCol == 1);
            let iPk = pk.borrow().aiColumn[0] as i32;
            sqlite3VdbeAddOp3(db.v(), OP_VColumn, iCsr, iPk, regArg);
            sqlite3VdbeAddOp2(db.v(), OP_SCopy, regArg + 2 + iPk, regArg + 1);
        }

        eOnePass = crate::where_::sqlite3WhereOkOnePass(&mut w, &mut aDummy);

        /* There is no ONEPASS_MULTI on virtual tables */
        debug_assert!(eOnePass == ONEPASS_OFF || eOnePass == ONEPASS_SINGLE);

        if eOnePass != 0 {
            /* If using the onepass strategy, no-op out the OP_OpenEphemeral coded
             ** above. */
            sqlite3VdbeChangeToNoop(db.v(), addr);
            sqlite3VdbeAddOp1(db.v(), OP_Close, iCsr);
        } else {
            /* Create a record from the argument register contents and insert it into
             ** the ephemeral table. */
            sqlite3MultiWrite(db);
            sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regArg, nArg, regRec);
            sqlite3VdbeAddOp2(db.v(), OP_NewRowid, ephemTab, regRowid);
            sqlite3VdbeAddOp3(db.v(), OP_Insert, ephemTab, regRec, regRowid);
        }
        pWInfo = Some(w);
    }

    if eOnePass == ONEPASS_OFF {
        /* End the virtual table scan */
        if nSrc == 1 {
            if let Some(w) = pWInfo.as_mut() {
                sqlite3WhereEnd(db, w);
            }
        }

        /* Begin scanning through the ephemeral table. */
        addr = sqlite3VdbeAddOp1(db.v(), OP_Rewind, ephemTab);

        /* Extract arguments from the current row of the ephemeral table and
         ** invoke the VUpdate method.  */
        for i in 0..nArg {
            sqlite3VdbeAddOp3(db.v(), OP_Column, ephemTab, i, regArg + i);
        }
    }
    crate::vtab::sqlite3VtabMakeWritable(db, pTab);
    sqlite3VdbeAddOp4(db.v(), OP_VUpdate, 0, nArg, regArg, pVTab, P4_VTAB as i32);
    sqlite3VdbeChangeP5(
        db.v(),
        if onError == OE_Default as i32 {
            OE_Abort as u16
        } else {
            onError as u16
        },
    );
    sqlite3MayAbort(db);

    /* End of the ephemeral table scan. Or, if using the onepass strategy,
     ** jump to here if the scan visited zero rows. */
    if eOnePass == ONEPASS_OFF {
        sqlite3VdbeAddOp2(db.v(), OP_Next, ephemTab, addr + 1);
        sqlite3VdbeJumpHere(db.v(), addr);
        sqlite3VdbeAddOp2(db.v(), OP_Close, ephemTab, 0);
    } else if let Some(w) = pWInfo.as_mut() {
        sqlite3WhereEnd(db, w);
    }
}
