//! Port of upsert.c: various aspects of UPSERT processing and handling of
//! the Upsert object.

use crate::build::sqlite3TableColumnToStorage;
use crate::build::{sqlite3MayAbort, sqlite3PrimaryKeyIndex, sqlite3TableColumnToIndex};
use crate::expr::{
    sqlite3ExprCompare, sqlite3ExprDelete, sqlite3ExprDup, sqlite3ExprListDelete,
    sqlite3ExprListDup, sqlite3GetTempReg, sqlite3ReleaseTempReg, sqlite3SrcListDup,
};
use crate::int::*;
use crate::resolve::{sqlite3ResolveExprListNames, sqlite3ResolveExprNames};
use crate::update::sqlite3Update;
use crate::vdbeaux::*;
use std::rc::Rc;

/// True if `a` (an optional index) is the same object as `b` (C pointer
/// comparison `a==b` with either side possibly NULL).
pub(crate) fn idx_opt_eq(a: &Option<IndexRef>, b: Option<&IndexRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Free a list of Upsert objects
fn upsertDelete(db: &mut Sqlite3, mut p: UpsertId) {
    loop {
        let (pNext, t, tw, s, w) = {
            let u = db.arena.ups(p);
            (
                u.pNextUpsert,
                u.pUpsertTarget,
                u.pUpsertTargetWhere,
                u.pUpsertSet,
                u.pUpsertWhere,
            )
        };
        sqlite3ExprListDelete(db, t);
        sqlite3ExprDelete(db, tw);
        sqlite3ExprListDelete(db, s);
        sqlite3ExprDelete(db, w);
        p = pNext;
        if p.is_null() {
            break;
        }
    }
}

/// C: `void sqlite3UpsertDelete(sqlite3 *db, Upsert *p)`
pub fn sqlite3UpsertDelete(db: &mut Sqlite3, p: UpsertId) {
    if p.nn() {
        upsertDelete(db, p);
    }
}

/// C: `Upsert * sqlite3UpsertDup(sqlite3 *db, Upsert *p)`
/// Duplicate an Upsert object.
pub fn sqlite3UpsertDup(db: &mut Sqlite3, p: UpsertId) -> UpsertId {
    if p.is_null() {
        return UpsertId::NULL;
    }
    let (t, tw, s, w, n) = {
        let u = db.arena.ups(p);
        (
            u.pUpsertTarget,
            u.pUpsertTargetWhere,
            u.pUpsertSet,
            u.pUpsertWhere,
            u.pNextUpsert,
        )
    };
    let t = sqlite3ExprListDup(db, t, 0);
    let tw = sqlite3ExprDup(db, tw, 0);
    let s = sqlite3ExprListDup(db, s, 0);
    let w = sqlite3ExprDup(db, w, 0);
    let n = sqlite3UpsertDup(db, n);
    sqlite3UpsertNew(db, t, tw, s, w, n)
}

/// C: `Upsert * sqlite3UpsertNew( sqlite3 *db, ExprList *pTarget, Expr *pTargetWhere, ExprList *pSet, Expr *pWhere, Upsert *pNext )`
/// Create a new Upsert object.
pub fn sqlite3UpsertNew(
    db: &mut Sqlite3,
    pTarget: ExprListId,
    pTargetWhere: ExprId,
    pSet: ExprListId,
    pWhere: ExprId,
    pNext: UpsertId,
) -> UpsertId {
    db.arena.new_upsert(Upsert {
        pUpsertTarget: pTarget,
        pUpsertTargetWhere: pTargetWhere,
        pUpsertSet: pSet,
        pUpsertWhere: pWhere,
        isDoUpdate: pSet.nn() as u8,
        pNextUpsert: pNext,
        ..Default::default()
    })
}

/// C: `int sqlite3UpsertAnalyzeTarget( Parse *pParse, SrcList *pTabList, Upsert *pUpsert, Upsert *pAll )`
///
/// Analyze the ON CONFLICT clause described by pUpsert.  Resolve all
/// symbols in the conflict-target.
///
/// Return SQLITE_OK if everything works, or an error code is something
/// is wrong.
pub fn sqlite3UpsertAnalyzeTarget(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    mut pUpsert: UpsertId,
    pAll: UpsertId,
) -> i32 {
    let mut nClause = 0;
    debug_assert!(db.src(pTabList).nSrc() == 1);
    debug_assert!(db.src(pTabList).a[0].pSTab.is_some());
    debug_assert!(pUpsert.nn());

    /* Resolve all symbolic names in the conflict-target clause, which
     ** includes both the list of columns and the optional partial-index
     ** WHERE clause.
     */
    let sNC = db.arena.new_nc(NameContext {
        pSrcList: pTabList,
        ..Default::default()
    });
    while pUpsert.nn() && db.arena.ups(pUpsert).pUpsertTarget.nn() {
        let (tgt, tgtw) = {
            let u = db.arena.ups(pUpsert);
            (u.pUpsertTarget, u.pUpsertTargetWhere)
        };
        let rc = sqlite3ResolveExprListNames(db, sNC, tgt);
        if rc != 0 {
            return rc;
        }
        let rc = sqlite3ResolveExprNames(db, sNC, tgtw);
        if rc != 0 {
            return rc;
        }

        /* Check to see if the conflict target matches the rowid. */
        let pTab = db.src(pTabList).a[0].pSTab.clone().unwrap();
        let pTarget = tgt;
        let iCursor = db.src(pTabList).a[0].iCursor;
        let mut isRowid = false;
        if pTab.borrow().HasRowid() && db.el(pTarget).nExpr() == 1 {
            let pTerm = db.el(pTarget).a[0].pExpr;
            if db.ex(pTerm).op == TK_COLUMN && db.ex(pTerm).iColumn == XN_ROWID {
                isRowid = true;
            }
        }
        if isRowid {
            /* The conflict-target is the rowid of the primary table */
            debug_assert!(db.arena.ups(pUpsert).pUpsertIdx.is_none());
            pUpsert = db.arena.ups(pUpsert).pNextUpsert;
            nClause += 1;
            continue;
        }

        /* Initialize sCol[0..1] to be an expression parse tree for a
         ** single column of an index.  The sCol[0] node will be the TK_COLLATE
         ** operator and sCol[1] will be the TK_COLUMN operator.  Code below
         ** will populate the specific collation and column number values
         ** prior to comparing against the conflict-target expression.
         */
        let sCol1 = db.arena.new_expr(Expr {
            op: TK_COLUMN,
            iTable: iCursor,
            ..Default::default()
        });
        let sCol0 = db.arena.new_expr(Expr {
            op: TK_COLLATE,
            pLeft: sCol1,
            ..Default::default()
        });

        /* Check for matches against other indexes */
        let aIdx: Vec<IndexRef> = pTab.borrow().pIndex.clone();
        for pIdx in aIdx.iter() {
            let (uniq, nKeyCol, hasPart) = {
                let x = pIdx.borrow();
                (
                    x.IsUniqueIndex(),
                    x.nKeyCol as i32,
                    x.pPartIdxWhere.is_some(),
                )
            };
            if !uniq {
                continue;
            }
            if db.el(pTarget).nExpr() != nKeyCol {
                continue;
            }
            if hasPart {
                let tw = db.arena.ups(pUpsert).pUpsertTargetWhere;
                if tw.is_null() {
                    continue;
                }
                let pw = pIdx.borrow().pPartIdxWhere.clone();
                let pw = db.tree(&pw);
                if sqlite3ExprCompare(db, true, tw, pw, iCursor) != 0 {
                    continue;
                }
            }
            let nn = nKeyCol;
            let colExpr = {
                let t = pIdx.borrow().aColExpr.clone();
                db.tree(&t)
            };
            let mut ii = 0;
            while ii < nn {
                let (zColl, iCol) = {
                    let x = pIdx.borrow();
                    (x.azColl[ii as usize].clone(), x.aiColumn[ii as usize])
                };
                db.exm(sCol0).u.zToken = Some(zColl);
                let mut pExpr;
                if iCol == XN_EXPR {
                    debug_assert!(colExpr.nn());
                    pExpr = db.el(colExpr).a[ii as usize].pExpr;
                    if db.ex(pExpr).op != TK_COLLATE {
                        db.exm(sCol0).pLeft = pExpr;
                        pExpr = sCol0;
                    }
                } else {
                    db.exm(sCol0).pLeft = sCol1;
                    db.exm(sCol1).iColumn = iCol;
                    pExpr = sCol0;
                }
                let mut jj = 0;
                while jj < nn {
                    let t = db.el(pTarget).a[jj as usize].pExpr;
                    // C passes pParse==0 here (no bound-variable comparison).
                    if sqlite3ExprCompare(db, false, t, pExpr, iCursor) < 2 {
                        break; /* Column ii of the index matches column jj of target */
                    }
                    jj += 1;
                }
                if jj >= nn {
                    /* The target contains no match for column jj of the index */
                    break;
                }
                ii += 1;
            }
            if ii < nn {
                /* Column ii of the index did not match any term of the conflict target.
                 ** Continue the search with the next index. */
                continue;
            }
            db.arena.upsm(pUpsert).pUpsertIdx = Some(pIdx.clone());
            if sqlite3UpsertOfIndex(db, pAll, Some(pIdx)) != pUpsert {
                /* Really this should be an error.  The isDup ON CONFLICT clause will
                 ** never fire.  But this problem was not discovered until three years
                 ** after multi-CONFLICT upsert was added, and so we silently ignore
                 ** the problem to prevent breaking applications that might actually
                 ** have redundant ON CONFLICT clauses. */
                db.arena.upsm(pUpsert).isDup = 1;
            }
            break;
        }
        if db.arena.ups(pUpsert).pUpsertIdx.is_none() {
            let zWhich: ZStr = if nClause == 0 && db.arena.ups(pUpsert).pNextUpsert.is_null() {
                Vec::new()
            } else {
                let mut z = mprintf!(db, "%r ", nClause + 1);
                z.truncate(15);
                z
            };
            errmsg!(
                db,
                "%sON CONFLICT clause does not match any PRIMARY KEY or UNIQUE constraint",
                &zWhich
            );
            return SQLITE_ERROR;
        }
        pUpsert = db.arena.ups(pUpsert).pNextUpsert;
        nClause += 1;
    }
    SQLITE_OK
}

/// C: `int sqlite3UpsertNextIsIPK(Upsert *pUpsert)`
///
/// Return true if pUpsert is the last ON CONFLICT clause with a
/// conflict target, or if pUpsert is followed by another ON CONFLICT
/// clause that targets the INTEGER PRIMARY KEY.
pub fn sqlite3UpsertNextIsIPK(db: &mut Sqlite3, pUpsert: UpsertId) -> i32 {
    if pUpsert.is_null() {
        return 0;
    }
    let mut pNext = db.arena.ups(pUpsert).pNextUpsert;
    loop {
        if pNext.is_null() {
            return 1;
        }
        let u = db.arena.ups(pNext);
        if u.pUpsertTarget.is_null() {
            return 1;
        }
        if u.pUpsertIdx.is_none() {
            return 1;
        }
        if u.isDup == 0 {
            return 0;
        }
        pNext = u.pNextUpsert;
    }
}

/// C: `Upsert * sqlite3UpsertOfIndex(Upsert *pUpsert, Index *pIdx)`
///
/// Given the list of ON CONFLICT clauses described by pUpsert, and
/// a particular index pIdx, return a pointer to the particular ON CONFLICT
/// clause that applies to the index.  Or, if the index is not subject to
/// any ON CONFLICT clause, return NULL.
///
/// Rust: `pIdx` is None for the INTEGER PRIMARY KEY (C NULL).
pub fn sqlite3UpsertOfIndex(
    db: &mut Sqlite3,
    mut pUpsert: UpsertId,
    pIdx: Option<&IndexRef>,
) -> UpsertId {
    while pUpsert.nn() {
        let u = db.arena.ups(pUpsert);
        if u.pUpsertTarget.is_null() || idx_opt_eq(&u.pUpsertIdx, pIdx) {
            break;
        }
        pUpsert = u.pNextUpsert;
    }
    pUpsert
}

/// C: `void sqlite3UpsertDoUpdate( Parse *pParse, Upsert *pUpsert, Table *pTab, Index *pIdx, int iCur )`
///
/// Generate bytecode that does an UPDATE as part of an upsert.
///
/// If pIdx is NULL, then the UNIQUE constraint that failed was the IPK.
/// In this case parameter iCur is a cursor open on the table b-tree that
/// currently points to the conflicting table row. Otherwise, if pIdx
/// is not NULL, then pIdx is the constraint that failed and iCur is a
/// cursor points to the conflicting row.
pub fn sqlite3UpsertDoUpdate(
    db: &mut Sqlite3,
    pUpsert: UpsertId,
    pTab: &TableRef,
    pIdx: Option<&IndexRef>,
    iCur: i32,
) {
    let pTop = pUpsert;
    debug_assert!(pUpsert.nn());
    let iDataCur = db.arena.ups(pUpsert).iDataCur;
    let pUpsert = sqlite3UpsertOfIndex(db, pTop, pIdx);
    if let Some(pIdx) = pIdx {
        if iCur != iDataCur {
            if pTab.borrow().HasRowid() {
                let regRowid = sqlite3GetTempReg(db);
                sqlite3VdbeAddOp2(db.v(), OP_IdxRowid, iCur, regRowid);
                sqlite3VdbeAddOp3(db.v(), OP_SeekRowid, iDataCur, 0, regRowid);
                sqlite3ReleaseTempReg(db, regRowid);
            } else {
                let pPk = sqlite3PrimaryKeyIndex(pTab).expect("WITHOUT ROWID without PK");
                let nPk = pPk.borrow().nKeyCol as i32;
                let iPk = db.ppr().nMem + 1;
                db.pp().nMem += nPk;
                for i in 0..nPk {
                    let col = pPk.borrow().aiColumn[i as usize];
                    debug_assert!(col >= 0);
                    let k = sqlite3TableColumnToIndex(pIdx, col as i32);
                    sqlite3VdbeAddOp3(db.v(), OP_Column, iCur, k, iPk + i);
                }
                let i = sqlite3VdbeAddOp4Int(db.v(), OP_Found, iDataCur, 0, iPk, nPk);
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Halt,
                    SQLITE_CORRUPT,
                    OE_Abort as i32,
                    0,
                    P4::Z(b"corrupt database".to_vec()),
                    P4_STATIC as i32,
                );
                sqlite3MayAbort(db);
                sqlite3VdbeJumpHere(db.v(), i);
            }
        }
    }
    /* pUpsert does not own pTop->pUpsertSrc - the outer INSERT statement does.
     ** So we have to make a copy before passing it down into sqlite3Update() */
    let (src, regData) = {
        let u = db.arena.ups(pTop);
        (u.pUpsertSrc, u.regData)
    };
    let pSrc = sqlite3SrcListDup(db, src, 0);
    /* excluded.* columns of type REAL need to be converted to a hard real */
    let nCol = pTab.borrow().nCol as i32;
    for i in 0..nCol {
        if pTab.borrow().aCol[i as usize].affinity == SQLITE_AFF_REAL {
            let iStorage = regData + sqlite3TableColumnToStorage(pTab, i as i16) as i32;
            sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, iStorage);
        }
    }
    let (set, wh) = {
        let u = db.arena.ups(pUpsert);
        (u.pUpsertSet, u.pUpsertWhere)
    };
    let set = sqlite3ExprListDup(db, set, 0);
    let wh = sqlite3ExprDup(db, wh, 0);
    sqlite3Update(
        db,
        pSrc,
        set,
        wh,
        OE_Abort as i32,
        ExprListId::NULL,
        ExprId::NULL,
        pUpsert,
    );
}
