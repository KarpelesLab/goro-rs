//! Port of select.c: routines that are called by the parser to handle
//! SELECT statements (the SELECT compiler).
//!
//! The control flow follows the C code closely.  Notes on the Rust port:
//!
//! * `Parse*`/`sqlite3*` are replaced by `db: &mut Sqlite3`; the Vdbe under
//!   construction is `db.v()`.
//! * Stack objects of the C code that are linked from parse-tree nodes
//!   (NameContext) live in the arena; others (SortCtx, DistinctCtx,
//!   WhereConst, CheckOnCtx, SubstContext) are plain Rust structs.
//! * KeyInfo objects are reference counted (`Rc`), so sqlite3KeyInfoRef()
//!   clones the `Rc` and sqlite3KeyInfoUnref() is a no-op.
//! * Debug-only code (TREETRACE, VdbeComment, SQLITE_DEBUG asserts),
//!   SQLITE_ENABLE_SORTER_REFERENCES and SQLITE_ENABLE_STMT_SCANSTATUS code
//!   are not compiled in the reference build and are omitted.

use crate::build::*;
use crate::expr::*;
use crate::int::*;
use crate::printf::PfArg;
use crate::vdbeaux::*;
use crate::walker::*;
use std::rc::Rc;

/// An instance of the following object is used to record information about
/// how to process the DISTINCT keyword, to simplify passing that information
/// into the selectInnerLoop() routine.
#[derive(Clone, Copy, Default)]
struct DistinctCtx {
    /// 0: Not distinct. 1: DISTINCT  2: DISTINCT and ORDER BY
    isTnct: u8,
    /// One of the WHERE_DISTINCT_* operators
    eTnctType: u8,
    /// Ephemeral table used for DISTINCT processing
    tabTnct: i32,
    /// Address of OP_OpenEphemeral opcode for tabTnct
    addrTnct: i32,
}

/// An instance of this object holds information (beyond pParse and pSelect)
/// needed to load the next result row that is to be added to the sorter.
#[derive(Clone, Copy, Default)]
struct RowLoadInfo {
    /// Store results in array of registers here
    regResult: i32,
    /// Flag argument to ExprCodeExprList()
    ecelFlags: u8,
}

/// An instance of the following object is used to record information about
/// the ORDER BY (or GROUP BY) clause of query is being coded.
#[derive(Clone, Copy, Default)]
struct SortCtx {
    /// The ORDER BY (or GROUP BY clause)
    pOrderBy: ExprListId,
    /// Number of ORDER BY terms satisfied by indices
    nOBSat: i32,
    /// Cursor number for the sorter
    iECursor: i32,
    /// Register holding block-output return address
    regReturn: i32,
    /// Start label for the block-output subroutine
    labelBkOut: i32,
    /// Address of the OP_SorterOpen or OP_OpenEphemeral
    addrSortIndex: i32,
    /// Jump here when done, ex: LIMIT reached
    labelDone: i32,
    /// Jump here when sorter is full
    labelOBLopt: i32,
    /// Zero or more SORTFLAG_* bits
    sortFlags: u8,
    /// Deferred row loading info or None (C: pointer to a stack object of
    /// selectInnerLoop(); a copy is kept here).
    pDeferredRowLoad: Option<RowLoadInfo>,
}

/// Use SorterOpen instead of OpenEphemeral
const SORTFLAG_UseSorter: u8 = 0x01;

/// ExplainQueryPlan((pParse, bPush, zFmt, ...)): sqlite3VdbeExplain().
fn explainQP(db: &mut Sqlite3, bPush: u8, zFmt: &[u8], args: &[PfArg]) -> i32 {
    sqlite3VdbeExplain(db, bPush, zFmt, args)
}

/// P4 operand for an optional collating sequence (P4_COLLSEQ).
fn p4coll(p: Option<CollSeqRef>) -> P4 {
    match p {
        Some(c) => P4::Coll(c),
        None => P4::None,
    }
}

/// P4 operand for an optional KeyInfo (P4_KEYINFO).
fn p4key(p: Option<KeyInfoRef>) -> P4 {
    match p {
        Some(k) => P4::KeyInfo(k),
        None => P4::None,
    }
}

/// P4 operand for an optional string (affinity strings).
fn p4str(p: Option<ZStr>) -> P4 {
    match p {
        Some(z) => P4::Z(z),
        None => P4::None,
    }
}

/// Set a column name/decltype/origin (sqlite3VdbeSetColName with
/// SQLITE_TRANSIENT).  A NULL name leaves the entry NULL, which it already
/// is after sqlite3VdbeSetNumCols().
fn setColName(db: &mut Sqlite3, idx: i32, var: i32, zName: Option<&[u8]>) {
    if let Some(z) = zName {
        sqlite3VdbeSetColName(db.v(), idx, var, z);
    }
}

/// True if the two optional collating sequences are the same object.
fn collEq(a: &Option<CollSeqRef>, b: &Option<CollSeqRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// sqlite3IsBinary() on an optional collating sequence (NULL is binary).
fn isBinary(p: &Option<CollSeqRef>) -> bool {
    match p {
        None => true,
        Some(c) => crate::mainapi::sqlite3IsBinary(Some(c)) != 0,
    }
}

/// Create a new transient Table object (sqlite3DbMallocZero(sizeof(Table))).
fn newTable() -> TableRef {
    Rc::new(std::cell::RefCell::new(Table::default()))
}

/// The Select of a subquery FROM-clause item (pItem->u4.pSubq->pSelect).
fn subqSelect(db: &Sqlite3, it: SrcItemId) -> SelectId {
    match &db.item(it).u4.pSubq {
        Some(q) => q.pSelect,
        None => SelectId::NULL,
    }
}

/// The table of a FROM-clause item (pItem->pSTab), which must exist.
fn itemTab(db: &Sqlite3, it: SrcItemId) -> TableRef {
    db.item(it).pSTab.clone().expect("SrcItem without table")
}

/*
** Delete all the content of a Select structure.  Deallocate the structure
** itself depending on the value of bFree
**
** If bFree==1, call sqlite3DbFree() on the p object.
** If bFree==0, Leave the first Select object unfreed
*/
fn clearSelect(db: &mut Sqlite3, mut p: SelectId, _bFree: i32) {
    while p.nn() {
        let s = db.sel(p).clone();
        sqlite3ExprListDelete(db, s.pEList);
        sqlite3SrcListDelete(db, s.pSrc);
        sqlite3ExprDelete(db, s.pWhere);
        sqlite3ExprListDelete(db, s.pGroupBy);
        sqlite3ExprDelete(db, s.pHaving);
        sqlite3ExprListDelete(db, s.pOrderBy);
        sqlite3ExprDelete(db, s.pLimit);
        if s.pWith.nn() {
            sqlite3WithDelete(db, s.pWith);
        }
        if s.pWinDefn.nn() {
            crate::window::sqlite3WindowListDelete(db, s.pWinDefn);
        }
        while db.sel(p).pWin.nn() {
            let w = db.sel(p).pWin;
            debug_assert!(db.win(w).ppThis == WinLink::Select(p));
            crate::window::sqlite3WindowUnlinkFromSelect(db, w);
        }
        p = s.pPrior;
    }
}

/*
** Initialize a SelectDest structure.
*/
pub fn sqlite3SelectDestInit(pDest: &mut SelectDest, eDest: i32, iParm: i32) {
    pDest.eDest = eDest as u8;
    pDest.iSDParm = iParm;
    pDest.iSDParm2 = 0;
    pDest.zAffSdst = None;
    pDest.iSdst = 0;
    pDest.nSdst = 0;
}

/// A SelectDest initialized by sqlite3SelectDestInit().
fn selectDest(eDest: u8, iParm: i32) -> SelectDest {
    let mut d = SelectDest::default();
    sqlite3SelectDestInit(&mut d, eDest as i32, iParm);
    d
}

/*
** Allocate a new Select structure and return a pointer to that
** structure.
*/
pub fn sqlite3SelectNew(
    db: &mut Sqlite3,
    mut pEList: ExprListId,
    mut pSrc: SrcListId,
    pWhere: ExprId,
    pGroupBy: ExprListId,
    pHaving: ExprId,
    pOrderBy: ExprListId,
    selFlags: u32,
    pLimit: ExprId,
) -> SelectId {
    if pEList.is_null() {
        let e = sqlite3Expr(db, TK_ASTERISK as i32, None);
        pEList = sqlite3ExprListAppend(db, ExprListId::NULL, e);
    }
    db.pp().nSelect += 1;
    let selId = db.ppr().nSelect as u32;
    if pSrc.is_null() {
        pSrc = db.arena.new_srclist(SrcList::default());
    }
    let pNew = db.arena.new_select(Select {
        pEList,
        op: TK_SELECT,
        selFlags,
        iLimit: 0,
        iOffset: 0,
        selId,
        nSelectRow: 0,
        pSrc,
        pWhere,
        pGroupBy,
        pHaving,
        pOrderBy,
        pPrior: SelectId::NULL,
        pNext: SelectId::NULL,
        pLimit,
        pWith: WithId::NULL,
        pWin: WindowId::NULL,
        pWinDefn: WindowId::NULL,
    });
    if db.mallocFailed != 0 {
        clearSelect(db, pNew, 1);
        return SelectId::NULL;
    }
    pNew
}

/*
** Delete the given Select structure and all of its substructures.
*/
pub fn sqlite3SelectDelete(db: &mut Sqlite3, p: SelectId) {
    if p.nn() {
        clearSelect(db, p, 1);
    }
}

/// C: `void sqlite3SelectDeleteGeneric(sqlite3 *db, void *p)`
/// Rust: parse-tree objects are freed with the arena; this is used as a
/// parser cleanup callback in C and is a no-op here (the arena keeps the
/// nodes alive until the end of the parse).
pub fn sqlite3SelectDeleteGeneric(db: &mut Sqlite3, p: &mut [u8]) {
    let _ = (db, p);
}

/*
** Return a pointer to the right-most SELECT statement in a compound.
*/
fn findRightmost(db: &Sqlite3, mut p: SelectId) -> SelectId {
    while db.sel(p).pNext.nn() {
        p = db.sel(p).pNext;
    }
    p
}

/*
** Given 1 to 3 identifiers preceding the JOIN keyword, determine the
** type of join.  Return an integer constant that expresses that type
** in terms of the following bit values:
**
**     JT_INNER
**     JT_CROSS
**     JT_OUTER
**     JT_NATURAL
**     JT_LEFT
**     JT_RIGHT
**
** A full outer join is the combination of JT_LEFT and JT_RIGHT.
**
** If an illegal or unsupported join type is seen, then still return
** a join type, but put an error in the pParse structure.
**
** Rust: pB and pC are None for C NULL.
*/
pub fn sqlite3JoinType(
    db: &mut Sqlite3,
    pA: &Token,
    pB: Option<&Token>,
    pC: Option<&Token>,
) -> i32 {
    let mut jointype: i32 = 0;
    /*                          0123456789 123456789 123456789 123 */
    const zKeyText: &[u8] = b"naturaleftouterightfullinnercross";
    const aKeyword: [(u8, u8, u8); 7] = [
        /* (0) natural */ (0, 7, JT_NATURAL),
        /* (1) left    */ (6, 4, JT_LEFT | JT_OUTER),
        /* (2) outer   */ (10, 5, JT_OUTER),
        /* (3) right   */ (14, 5, JT_RIGHT | JT_OUTER),
        /* (4) full    */ (19, 4, JT_LEFT | JT_RIGHT | JT_OUTER),
        /* (5) inner   */ (23, 5, JT_INNER),
        /* (6) cross   */ (28, 5, JT_INNER | JT_CROSS),
    ];
    let apAll: [Option<&Token>; 3] = [Some(pA), pB, pC];
    for p in apAll.iter() {
        let Some(p) = *p else {
            break;
        };
        let mut j = 0;
        while j < aKeyword.len() {
            let (i, nChar, code) = aKeyword[j];
            if p.n == nChar as u32
                && crate::util::sqlite3_strnicmp(
                    Some(p.text()),
                    Some(&zKeyText[i as usize..]),
                    p.n as i32,
                ) == 0
            {
                jointype |= code as i32;
                break;
            }
            j += 1;
        }
        if j >= aKeyword.len() {
            jointype |= JT_ERROR as i32;
            break;
        }
    }
    let ji = (JT_INNER | JT_OUTER) as i32;
    if (jointype & ji) == ji
        || (jointype & JT_ERROR as i32) != 0
        || (jointype & (JT_OUTER | JT_LEFT | JT_RIGHT) as i32) == JT_OUTER as i32
    {
        let zSp1: &[u8] = if pB.is_none() { b"" } else { b" " };
        let zSp2: &[u8] = if pC.is_none() { b"" } else { b" " };
        let tB = pB.cloned().unwrap_or_default();
        let tC = pC.cloned().unwrap_or_default();
        errmsg!(
            db,
            b"unknown join type: %T%s%T%s%T",
            pA,
            zSp1,
            &tB,
            zSp2,
            &tC
        );
        jointype = JT_INNER as i32;
    }
    jointype
}

/*
** Return the index of a column in a table.  Return -1 if the column
** is not contained in the table.
*/
pub fn sqlite3ColumnIndex(pTab: &TableRef, zCol: Option<&[u8]>) -> i32 {
    let h = crate::util::sqlite3StrIHash(zCol);
    let t = pTab.borrow();
    let aCol = &t.aCol;
    let nCol = t.nCol as i32;
    if nCol <= 0 || aCol.is_empty() {
        return -1;
    }

    /* See if the aHx gives us a lucky match */
    let i = t.aHx[(h as usize) % t.aHx.len()] as usize;
    if i < aCol.len()
        && aCol[i].hName == h
        && crate::util::sqlite3StrICmp(Some(&aCol[i].zCnName), zCol) == 0
    {
        return i as i32;
    }

    /* No lucky match from the hash table.  Do a full search. */
    let mut i = 0usize;
    loop {
        if i < aCol.len()
            && aCol[i].hName == h
            && crate::util::sqlite3StrICmp(Some(&aCol[i].zCnName), zCol) == 0
        {
            return i as i32;
        }
        i += 1;
        if i as i32 >= nCol {
            break;
        }
    }
    -1
}

/*
** Mark a subquery result column as having been used.
*/
pub fn sqlite3SrcItemColumnUsed(db: &mut Sqlite3, pItem: SrcItemId, iCol: i32) {
    if db.item(pItem).fg.isNestedFrom != 0 {
        debug_assert!(db.item(pItem).fg.isSubquery != 0);
        let pSel = subqSelect(db, pItem);
        let pResults = db.sel(pSel).pEList;
        debug_assert!(iCol >= 0 && iCol < db.el(pResults).nExpr());
        db.elm(pResults).a[iCol as usize].fg.bUsed = 1;
    }
}

/*
** Search the tables iStart..iEnd (inclusive) in pSrc, looking for a
** table that has a column named zCol.  The search is left-to-right.
** The first match found is returned.
**
** When found, set *piTab and *piCol to the table index and column index
** of the matching column and return TRUE.
**
** If not found, return FALSE.
*/
fn tableAndColumnIndex(
    db: &mut Sqlite3,
    pSrc: SrcListId,
    iStart: i32,
    iEnd: i32,
    zCol: &[u8],
    piTabCol: Option<(&mut i32, &mut i32)>,
    bIgnoreHidden: i32,
) -> i32 {
    debug_assert!(iEnd < db.src(pSrc).nSrc());
    debug_assert!(iStart >= 0);
    for i in iStart..=iEnd {
        let pTab = itemTab(db, SrcItemId(pSrc, i as u32));
        let iCol = sqlite3ColumnIndex(&pTab, Some(zCol));
        if iCol >= 0 && (bIgnoreHidden == 0 || !IsHiddenColumn(&pTab.borrow().aCol[iCol as usize]))
        {
            if let Some((piTab, piCol)) = piTabCol {
                sqlite3SrcItemColumnUsed(db, SrcItemId(pSrc, i as u32), iCol);
                *piTab = i;
                *piCol = iCol;
            }
            return 1;
        }
    }
    0
}

/*
** Set the EP_OuterON property on all terms of the given expression.
** And set the Expr.w.iJoin to iTable for every term in the
** expression.
**
** The EP_OuterON property is used on terms of an expression to tell
** the OUTER JOIN processing logic that this term is part of the
** join restriction specified in the ON or USING clause and not a part
** of the more general WHERE clause.  These terms are moved over to the
** WHERE clause during join processing but we need to remember that they
** originated in the ON or USING clause.
**
** The Expr.w.iJoin tells the WHERE clause processing that the
** expression depends on table w.iJoin even if that table is not
** explicitly mentioned in the expression.  That information is needed
** for cases like this:
**
**    SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.b AND t1.x=5
**
** The where clause needs to defer the handling of the t1.x=5
** term until after the t2 loop of the join.  In that way, a
** NULL t2 row will be inserted whenever t1.x!=5.  If we do not
** defer the handling of t1.x=5, it will be processed immediately
** after the t2 loop and rows with t1.x!=5 will never appear in
** the output, which is incorrect.
*/
pub fn sqlite3SetJoinExpr(db: &mut Sqlite3, mut p: ExprId, iTable: i32, joinFlag: u32) {
    debug_assert!(joinFlag == EP_OuterON || joinFlag == EP_InnerON);
    while p.nn() {
        let (pList, pLeft, pRight, useList) = {
            let e = db.exm(p);
            e.SetProperty(joinFlag);
            debug_assert!(!e.HasProperty(EP_TokenOnly | EP_Reduced));
            e.w.iJoin = iTable;
            (e.x.pList, e.pLeft, e.pRight, e.UseXList())
        };
        if useList && pList.nn() {
            let n = db.el(pList).nExpr();
            for i in 0..n {
                let pe = db.el(pList).a[i as usize].pExpr;
                sqlite3SetJoinExpr(db, pe, iTable, joinFlag);
            }
        }
        sqlite3SetJoinExpr(db, pLeft, iTable, joinFlag);
        p = pRight;
    }
}

/* Undo the work of sqlite3SetJoinExpr().  This is used when a LEFT JOIN
** is simplified into an ordinary JOIN, and when an ON expression is
** "pushed down" into the WHERE clause of a subquery.
**
** Convert every term that is marked with EP_OuterON and w.iJoin==iTable into
** an ordinary term that omits the EP_OuterON mark.  Or if iTable<0, then
** just clear every EP_OuterON and EP_InnerON mark from the expression tree.
**
** If nullable is true, that means that Expr p might evaluate to NULL even
** if it is a reference to a NOT NULL column.  This can happen, for example,
** if the table that p references is on the left side of a RIGHT JOIN.
** If nullable is true, then take care to not remove the EP_CanBeNull bit.
** See forum thread https://sqlite.org/forum/forumpost/b40696f50145d21c
*/
fn unsetJoinExpr(db: &mut Sqlite3, mut p: ExprId, iTable: i32, nullable: i32) {
    while p.nn() {
        let (op, pList, pLeft, pRight) = {
            let e = db.exm(p);
            if iTable < 0 || (e.HasProperty(EP_OuterON) && e.w.iJoin == iTable) {
                e.ClearProperty(EP_OuterON | EP_InnerON);
                if iTable >= 0 {
                    e.SetProperty(EP_InnerON);
                }
            }
            if e.op == TK_COLUMN && e.iTable == iTable && nullable == 0 {
                e.ClearProperty(EP_CanBeNull);
            }
            (e.op, e.x.pList, e.pLeft, e.pRight)
        };
        if op == TK_FUNCTION {
            debug_assert!(db.ex(p).UseXList());
            debug_assert!(pLeft.is_null());
            if pList.nn() {
                let n = db.el(pList).nExpr();
                for i in 0..n {
                    let pe = db.el(pList).a[i as usize].pExpr;
                    unsetJoinExpr(db, pe, iTable, nullable);
                }
            }
        }
        unsetJoinExpr(db, pLeft, iTable, nullable);
        p = pRight;
    }
}

/*
** This routine processes the join information for a SELECT statement.
**
**   *  A NATURAL join is converted into a USING join.  After that, we
**      do not need to be concerned with NATURAL joins and we only have
**      think about USING joins.
**
**   *  ON and USING clauses result in extra terms being added to the
**      WHERE clause to enforce the specified constraints.  The extra
**      WHERE clause terms will be tagged with EP_OuterON or
**      EP_InnerON so that we know that they originated in ON/USING.
**
** The terms of a FROM clause are contained in the Select.pSrc structure.
** The left most table is the first entry in Select.pSrc.  The right-most
** table is the last entry.  The join operator is held in the entry to
** the right.  Thus entry 1 contains the join operator for the join between
** entries 0 and 1.  Any ON or USING clauses associated with the join are
** also attached to the right entry.
**
** This routine returns the number of errors encountered.
*/
fn sqlite3ProcessJoin(db: &mut Sqlite3, p: SelectId) -> i32 {
    let pSrc = db.sel(p).pSrc;
    let mut bSeenTFunc = 0;
    let nSrc = db.src(pSrc).nSrc();
    let mut i = 0;
    while i < nSrc - 1 {
        let pLeft = SrcItemId(pSrc, i as u32);
        let pRight = SrcItemId(pSrc, (i + 1) as u32);
        let pRightTab = db.item(pRight).pSTab.clone();
        if db.item(pLeft).pSTab.is_none() || pRightTab.is_none() {
            i += 1;
            continue;
        }
        let pRightTab = pRightTab.unwrap();
        let joinType: u32 = if (db.item(pRight).fg.jointype & JT_OUTER) != 0 {
            EP_OuterON
        } else {
            EP_InnerON
        };

        /* If this is a NATURAL join, synthesize an appropriate USING clause
         ** to specify which columns should be joined.
         */
        if (db.item(pRight).fg.jointype & JT_NATURAL) != 0 {
            let mut pUsing = IdListId::NULL;
            if db.item(pRight).fg.isUsing != 0 || db.item(pRight).u3.pOn.nn() {
                errmsg!(db, b"a NATURAL join may not have an ON or USING clause", 0);
                return 1;
            }
            let nCol = pRightTab.borrow().nCol as i32;
            for j in 0..nCol {
                let (hidden, zName) = {
                    let t = pRightTab.borrow();
                    let c = &t.aCol[j as usize];
                    (IsHiddenColumn(c), c.zCnName.clone())
                };
                if hidden {
                    continue;
                }
                if tableAndColumnIndex(db, pSrc, 0, i, &zName, None, 1) != 0 {
                    pUsing = sqlite3IdListAppend(db, pUsing, &Token::default());
                    if pUsing.nn() {
                        debug_assert!(db.idl(pUsing).nId() > 0);
                        let n = db.idl(pUsing).a.len();
                        db.idlm(pUsing).a[n - 1].zName = Some(zName.clone());
                    }
                }
            }
            if pUsing.nn() {
                let it = db.itemm(pRight);
                it.fg.isUsing = 1;
                it.fg.isSynthUsing = 1;
                it.u3.pUsing = pUsing;
            }
            if db.ppr().nErr != 0 {
                return 1;
            }
        }

        /* Create extra terms on the WHERE clause for each column named
         ** in the USING clause.  Example: If the two tables to be joined are
         ** A and B and the USING clause names X, Y, and Z, then add this
         ** to the WHERE clause:    A.X=B.X AND A.Y=B.Y AND A.Z=B.Z
         ** Report an error if any column mentioned in the USING clause is
         ** not contained in both tables to be joined.
         */
        if db.item(pRight).fg.isUsing != 0 {
            let pList = db.item(pRight).u3.pUsing;
            debug_assert!(pList.nn());
            let nId = db.idl(pList).nId();
            for j in 0..nId {
                let zName = db.idl(pList).a[j as usize]
                    .zName
                    .clone()
                    .unwrap_or_default();
                let mut iLeft = 0;
                let mut iLeftCol = 0;
                let iRightCol = sqlite3ColumnIndex(&pRightTab, Some(&zName));
                let isSynth = db.item(pRight).fg.isSynthUsing as i32;
                if iRightCol < 0
                    || tableAndColumnIndex(
                        db,
                        pSrc,
                        0,
                        i,
                        &zName,
                        Some((&mut iLeft, &mut iLeftCol)),
                        isSynth,
                    ) == 0
                {
                    errmsg!(
                        db,
                        b"cannot join using column %s - column not present in both tables",
                        &zName
                    );
                    return 1;
                }
                let mut pE1 = crate::resolve::sqlite3CreateColumnExpr(db, pSrc, iLeft, iLeftCol);
                sqlite3SrcItemColumnUsed(db, SrcItemId(pSrc, iLeft as u32), iLeftCol);
                if (db.src(pSrc).a[0].fg.jointype & JT_LTORJ) != 0 && db.ppr().nErr == 0 {
                    /* This branch runs if the query contains one or more RIGHT or FULL
                     ** JOINs.  If only a single table on the left side of this join
                     ** contains the zName column, then this branch is a no-op.
                     ** But if there are two or more tables on the left side
                     ** of the join, construct a coalesce() function that gathers all
                     ** such tables.  Raise an error if more than one of those references
                     ** to zName is not also within a prior USING clause.
                     **
                     ** We really ought to raise an error if there are two or more
                     ** non-USING references to zName on the left of an INNER or LEFT
                     ** JOIN.  But older versions of SQLite do not do that, so we avoid
                     ** adding a new error so as to not break legacy applications.
                     */
                    let mut pFuncArgs = ExprListId::NULL;
                    let tkCoalesce = Token::from_bytes(b"coalesce");
                    debug_assert!(pE1.nn());
                    db.exm(pE1).SetProperty(EP_CanBeNull);
                    loop {
                        let mut iL2 = 0;
                        let mut iC2 = 0;
                        if tableAndColumnIndex(
                            db,
                            pSrc,
                            iLeft + 1,
                            i,
                            &zName,
                            Some((&mut iL2, &mut iC2)),
                            isSynth,
                        ) == 0
                        {
                            break;
                        }
                        iLeft = iL2;
                        iLeftCol = iC2;
                        let (isUsing, pU) = {
                            let it = db.item(SrcItemId(pSrc, iLeft as u32));
                            (it.fg.isUsing, it.u3.pUsing)
                        };
                        if isUsing == 0 || sqlite3IdListIndex(db, pU, Some(&zName)) < 0 {
                            errmsg!(db, b"ambiguous reference to %s in USING()", &zName);
                            break;
                        }
                        pFuncArgs = sqlite3ExprListAppend(db, pFuncArgs, pE1);
                        pE1 = crate::resolve::sqlite3CreateColumnExpr(db, pSrc, iLeft, iLeftCol);
                        sqlite3SrcItemColumnUsed(db, SrcItemId(pSrc, iLeft as u32), iLeftCol);
                    }
                    if pFuncArgs.nn() {
                        pFuncArgs = sqlite3ExprListAppend(db, pFuncArgs, pE1);
                        pE1 = sqlite3ExprFunction(db, pFuncArgs, &tkCoalesce, 0);
                        if pE1.nn() {
                            db.exm(pE1).affExpr = SQLITE_AFF_DEFER;
                        }
                    }
                } else if (db.src(pSrc).a[(i + 1) as usize].fg.jointype & JT_LEFT) != 0
                    && db.ppr().nErr == 0
                {
                    debug_assert!(pE1.nn());
                    db.exm(pE1).SetProperty(EP_CanBeNull);
                }
                let pE2 = crate::resolve::sqlite3CreateColumnExpr(db, pSrc, i + 1, iRightCol);
                sqlite3SrcItemColumnUsed(db, pRight, iRightCol);
                let pEq = sqlite3PExpr(db, TK_EQ as i32, pE1, pE2);
                debug_assert!(pE2.nn() || pEq.is_null());
                if pEq.nn() {
                    let iJoin = db.ex(pE2).iTable;
                    let e = db.exm(pEq);
                    e.SetProperty(joinType);
                    debug_assert!(!e.HasProperty(EP_TokenOnly | EP_Reduced));
                    e.w.iJoin = iJoin;
                }
                let w = db.sel(p).pWhere;
                let nw = sqlite3ExprAnd(db, w, pEq);
                db.selm(p).pWhere = nw;
            }
        }
        /* Add the ON clause to the end of the WHERE clause, connected by
         ** an AND operator.
         */
        else if db.item(pRight).u3.pOn.nn() {
            let (pOn, iCursor) = {
                let it = db.item(pRight);
                (it.u3.pOn, it.iCursor)
            };
            sqlite3SetJoinExpr(db, pOn, iCursor, joinType);
            let w = db.sel(p).pWhere;
            let nw = sqlite3ExprAnd(db, w, pOn);
            db.selm(p).pWhere = nw;
            let it = db.itemm(pRight);
            it.u3.pOn = ExprId::NULL;
            it.fg.isOn = 1;
            db.selm(p).selFlags |= SF_OnToWhere;
        }

        if bSeenTFunc == 0 {
            let it = db.item(pLeft);
            bSeenTFunc = (it.fg.isTabFunc != 0 && it.u1.pFuncArg.nn()) as i32;
        }

        /* We also need to call sqlite3SelectCheckOnClauses() to verify that
         ** table-valued function arguments do not illegally refer to any tables to
         ** their right. This test is required if either (a) there is a RIGHT JOIN
         ** in the SrcList, or (b) there is a LEFT JOIN to the right of a
         ** table-valued function.  */
        let (rTabFunc, rFuncArg) = {
            let it = db.item(pRight);
            (it.fg.isTabFunc, it.u1.pFuncArg)
        };
        if (rTabFunc != 0 && joinType == EP_OuterON && rFuncArg.nn())
            || (bSeenTFunc != 0 && joinType == EP_OuterON)
        {
            db.selm(p).selFlags |= SF_OnToWhere;
        }
        i += 1;
    }
    0
}

/*
** This routine does the work of loading query data into an array of
** registers so that it can be added to the sorter.
*/
fn innerLoopLoadRow(db: &mut Sqlite3, pSelect: SelectId, pInfo: &RowLoadInfo) {
    let pEList = db.sel(pSelect).pEList;
    sqlite3ExprCodeExprList(db, pEList, pInfo.regResult, 0, pInfo.ecelFlags);
}

/*
** Code the OP_MakeRecord instruction that generates the entry to be
** added into the sorter.
**
** Return the register in which the result is stored.
*/
fn makeSorterRecord(
    db: &mut Sqlite3,
    pSort: &mut SortCtx,
    pSelect: SelectId,
    regBase: i32,
    nBase: i32,
) -> i32 {
    let nOBSat = pSort.nOBSat;
    db.pp().nMem += 1;
    let regOut = db.ppr().nMem;
    if let Some(info) = pSort.pDeferredRowLoad {
        innerLoopLoadRow(db, pSelect, &info);
    }
    sqlite3VdbeAddOp3(
        db.v(),
        OP_MakeRecord,
        regBase + nOBSat,
        nBase - nOBSat,
        regOut,
    );
    regOut
}

/*
** Generate code that will push the record in registers regData
** through regData+nData-1 onto the sorter.
*/
fn pushOntoSorter(
    db: &mut Sqlite3,
    pSort: &mut SortCtx,
    pSelect: SelectId,
    regData: i32,
    regOrigData: i32,
    nData: i32,
    nPrefixReg: i32,
) {
    let bSeq = ((pSort.sortFlags & SORTFLAG_UseSorter) == 0) as i32;
    let nExpr = db.el(pSort.pOrderBy).nExpr();
    let nBase = nExpr + bSeq + nData;
    let regBase;
    let mut regRecord = 0;
    let nOBSat = pSort.nOBSat;
    let mut iSkip = 0;

    /* Three cases:
     **   (1) The data to be sorted has already been packed into a Record
     **       by a prior OP_MakeRecord.  In this case nData==1 and regData
     **       will be completely unrelated to regOrigData.
     **   (2) All output columns are included in the sort record.  In that
     **       case regData==regOrigData.
     **   (3) Some output columns are omitted from the sort record due to
     **       the SQLITE_ENABLE_SORTER_REFERENCES optimization, or due to the
     **       SQLITE_ECEL_OMITREF optimization, or due to the
     **       SortCtx.pDeferredRowLoad optimization.  In any of these cases
     **       regOrigData is 0 to prevent this routine from trying to copy
     **       values that might not yet exist.
     */
    debug_assert!(nData == 1 || regData == regOrigData || regOrigData == 0);

    if nPrefixReg != 0 {
        debug_assert!(nPrefixReg == nExpr + bSeq);
        regBase = regData - nPrefixReg;
    } else {
        regBase = db.ppr().nMem + 1;
        db.pp().nMem += nBase;
    }
    let (iOffset, iLimitS) = {
        let s = db.sel(pSelect);
        (s.iOffset, s.iLimit)
    };
    debug_assert!(iOffset == 0 || iLimitS != 0);
    let iLimit = if iOffset != 0 { iOffset + 1 } else { iLimitS };
    pSort.labelDone = sqlite3VdbeMakeLabel(db);
    let flags = SQLITE_ECEL_DUP | if regOrigData != 0 { SQLITE_ECEL_REF } else { 0 };
    sqlite3ExprCodeExprList(db, pSort.pOrderBy, regBase, regOrigData, flags);
    if bSeq != 0 {
        sqlite3VdbeAddOp2(db.v(), OP_Sequence, pSort.iECursor, regBase + nExpr);
    }
    if nPrefixReg == 0 && nData > 0 {
        sqlite3ExprCodeMove(db, regData, regBase + nExpr + bSeq, nData);
    }
    if nOBSat > 0 {
        regRecord = makeSorterRecord(db, pSort, pSelect, regBase, nBase);
        let regPrevKey = db.ppr().nMem + 1;
        db.pp().nMem += pSort.nOBSat;
        let nKey = nExpr - pSort.nOBSat + bSeq;
        let addrFirst = if bSeq != 0 {
            sqlite3VdbeAddOp1(db.v(), OP_IfNot, regBase + nExpr)
        } else {
            sqlite3VdbeAddOp1(db.v(), OP_SequenceTest, pSort.iECursor)
        };
        sqlite3VdbeAddOp3(db.v(), OP_Compare, regPrevKey, regBase, pSort.nOBSat);
        if db.mallocFailed != 0 {
            return;
        }
        let pKI = {
            let pOp = sqlite3VdbeGetOp(db.v(), pSort.addrSortIndex);
            pOp.p2 = nKey + nData;
            match &pOp.p4 {
                P4::KeyInfo(k) => k.clone(),
                _ => return,
            }
        };
        {
            let mut k = pKI.borrow_mut();
            let n = (k.nKeyField as usize).min(k.aSortFlags.len());
            for f in k.aSortFlags[..n].iter_mut() {
                *f = 0; /* Makes OP_Jump testable */
            }
        }
        sqlite3VdbeChangeP4(db.v(), -1, P4::KeyInfo(pKI.clone()), P4_KEYINFO as i32);
        let nX = {
            let k = pKI.borrow();
            k.nAllField as i32 - k.nKeyField as i32 - 1
        };
        let pNewKI = sqlite3KeyInfoFromExprList(db, pSort.pOrderBy, nOBSat, nX);
        sqlite3VdbeGetOp(db.v(), pSort.addrSortIndex).p4 = p4key(pNewKI);
        let addrJmp = sqlite3VdbeCurrentAddr(db.v());
        sqlite3VdbeAddOp3(db.v(), OP_Jump, addrJmp + 1, 0, addrJmp + 1);
        pSort.labelBkOut = sqlite3VdbeMakeLabel(db);
        db.pp().nMem += 1;
        pSort.regReturn = db.ppr().nMem;
        sqlite3VdbeAddOp2(db.v(), OP_Gosub, pSort.regReturn, pSort.labelBkOut);
        sqlite3VdbeAddOp1(db.v(), OP_ResetSorter, pSort.iECursor);
        if iLimit != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_IfNot, iLimit, pSort.labelDone);
        }
        sqlite3VdbeJumpHere(db.v(), addrFirst);
        sqlite3ExprCodeMove(db, regBase, regPrevKey, pSort.nOBSat);
        sqlite3VdbeJumpHere(db.v(), addrJmp);
    }
    if iLimit != 0 {
        /* At this point the values for the new sorter entry are stored
         ** in an array of registers. They need to be composed into a record
         ** and inserted into the sorter if either (a) there are currently
         ** less than LIMIT+OFFSET items or (b) the new record is smaller than
         ** the largest record currently in the sorter. If (b) is true and there
         ** are already LIMIT+OFFSET items in the sorter, delete the largest
         ** entry before inserting the new one. This way there are never more
         ** than LIMIT+OFFSET items in the sorter.
         **
         ** If the new record does not need to be inserted into the sorter,
         ** jump to the next iteration of the loop. If the pSort->labelOBLopt
         ** value is not zero, then it is a label of where to jump.  Otherwise,
         ** just bypass the row insert logic.  See the header comment on the
         ** sqlite3WhereOrderByLimitOptLabel() function for additional info.
         */
        let iCsr = pSort.iECursor;
        let v = db.v();
        let a = sqlite3VdbeCurrentAddr(v) + 4;
        sqlite3VdbeAddOp2(v, OP_IfNotZero, iLimit, a);
        sqlite3VdbeAddOp2(v, OP_Last, iCsr, 0);
        iSkip = sqlite3VdbeAddOp4Int(v, OP_IdxLE, iCsr, 0, regBase + nOBSat, nExpr - nOBSat);
        sqlite3VdbeAddOp1(v, OP_Delete, iCsr);
    }
    if regRecord == 0 {
        regRecord = makeSorterRecord(db, pSort, pSelect, regBase, nBase);
    }
    let op = if (pSort.sortFlags & SORTFLAG_UseSorter) != 0 {
        OP_SorterInsert
    } else {
        OP_IdxInsert
    };
    let v = db.v();
    sqlite3VdbeAddOp4Int(
        v,
        op,
        pSort.iECursor,
        regRecord,
        regBase + nOBSat,
        nBase - nOBSat,
    );
    if iSkip != 0 {
        let dest = if pSort.labelOBLopt != 0 {
            pSort.labelOBLopt
        } else {
            sqlite3VdbeCurrentAddr(v)
        };
        sqlite3VdbeChangeP2(v, iSkip, dest);
    }
}

/*
** Add code to implement the OFFSET
*/
fn codeOffset(v: &mut Vdbe, iOffset: i32, iContinue: i32) {
    if iOffset > 0 {
        sqlite3VdbeAddOp3(v, OP_IfPos, iOffset, iContinue, 1);
    }
}

/*
** Add code that will check to make sure the array of registers starting at
** iMem form a distinct entry. This is used by both "SELECT DISTINCT ..." and
** distinct aggregates ("SELECT count(DISTINCT <expr>) ..."). Three strategies
** are available. Which is used depends on the value of parameter eTnctType,
** as follows:
**
**   WHERE_DISTINCT_UNORDERED/WHERE_DISTINCT_NOOP:
**     Build an ephemeral table that contains all entries seen before and
**     skip entries which have been seen before.
**
**     Parameter iTab is the cursor number of an ephemeral table that must
**     be opened before the VM code generated by this routine is executed.
**     The ephemeral cursor table is queried for a record identical to the
**     record formed by the current array of registers. If one is found,
**     jump to VM address addrRepeat. Otherwise, insert a new record into
**     the ephemeral cursor and proceed.
**
**     The returned value in this case is a copy of parameter iTab.
**
**   WHERE_DISTINCT_ORDERED:
**     In this case rows are being delivered sorted order. The ephemeral
**     table is not required. Instead, the current set of values
**     is compared against previous row. If they match, the new row
**     is not distinct and control jumps to VM address addrRepeat. Otherwise,
**     the VM program proceeds with processing the new row.
**
**     The returned value in this case is the register number of the first
**     in an array of registers used to store the previous result row so that
**     it can be compared to the next. The caller must ensure that this
**     register is initialized to NULL.  (The fixDistinctOpenEph() routine
**     will take care of this initialization.)
**
**   WHERE_DISTINCT_UNIQUE:
**     In this case it has already been determined that the rows are distinct.
**     No special action is required. The return value is zero.
**
** Parameter pEList is the list of expressions used to generated the
** contents of each row. It is used by this routine to determine (a)
** how many elements there are in the array of registers and (b) the
** collation sequences that should be used for the comparisons if
** eTnctType is WHERE_DISTINCT_ORDERED.
*/
fn codeDistinct(
    db: &mut Sqlite3,
    eTnctType: i32,
    iTab: i32,
    addrRepeat: i32,
    pEList: ExprListId,
    regElem: i32,
) -> i32 {
    let mut iRet = 0;
    let nResultCol = db.el(pEList).nExpr();

    match eTnctType as u8 {
        WHERE_DISTINCT_ORDERED => {
            /* Allocate space for the previous row */
            let regPrev = db.ppr().nMem + 1;
            iRet = regPrev;
            db.pp().nMem += nResultCol;

            let iJump = sqlite3VdbeCurrentAddr(db.v()) + nResultCol;
            for i in 0..nResultCol {
                let pe = db.el(pEList).a[i as usize].pExpr;
                let pColl = sqlite3ExprCollSeq(db, pe);
                let v = db.v();
                if i < nResultCol - 1 {
                    sqlite3VdbeAddOp3(v, OP_Ne, regElem + i, iJump, regPrev + i);
                } else {
                    sqlite3VdbeAddOp3(v, OP_Eq, regElem + i, addrRepeat, regPrev + i);
                }
                sqlite3VdbeChangeP4(v, -1, p4coll(pColl), P4_COLLSEQ as i32);
                sqlite3VdbeChangeP5(v, SQLITE_NULLEQ as u16);
            }
            debug_assert!(sqlite3VdbeCurrentAddr(db.v()) == iJump || db.mallocFailed != 0);
            sqlite3VdbeAddOp3(db.v(), OP_Copy, regElem, regPrev, nResultCol - 1);
        }

        WHERE_DISTINCT_UNIQUE => { /* nothing to do */ }

        _ => {
            let r1 = sqlite3GetTempReg(db);
            let v = db.v();
            sqlite3VdbeAddOp4Int(v, OP_Found, iTab, addrRepeat, regElem, nResultCol);
            sqlite3VdbeAddOp3(v, OP_MakeRecord, regElem, nResultCol, r1);
            sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iTab, r1, regElem, nResultCol);
            sqlite3VdbeChangeP5(v, OPFLAG_USESEEKRESULT);
            sqlite3ReleaseTempReg(db, r1);
            iRet = iTab;
        }
    }

    iRet
}

/*
** This routine runs after codeDistinct().  It makes necessary
** adjustments to the OP_OpenEphemeral opcode that the codeDistinct()
** routine made use of.  This processing must be done separately since
** sometimes codeDistinct is called before the OP_OpenEphemeral is actually
** laid down.
**
** WHERE_DISTINCT_NOOP:
** WHERE_DISTINCT_UNORDERED:
**
**     No adjustments necessary.  This function is a no-op.
**
** WHERE_DISTINCT_UNIQUE:
**
**     The ephemeral table is not needed.  So change the
**     OP_OpenEphemeral opcode into an OP_Noop.
**
** WHERE_DISTINCT_ORDERED:
**
**     The ephemeral table is not needed.  But we do need register
**     iVal to be initialized to NULL.  So change the OP_OpenEphemeral
**     into an OP_Null on the iVal register.
*/
fn fixDistinctOpenEph(db: &mut Sqlite3, eTnctType: i32, iVal: i32, iOpenEphAddr: i32) {
    let e = eTnctType as u8;
    if db.ppr().nErr == 0 && (e == WHERE_DISTINCT_UNIQUE || e == WHERE_DISTINCT_ORDERED) {
        let v = db.v();
        sqlite3VdbeChangeToNoop(v, iOpenEphAddr);
        if sqlite3VdbeGetOp(v, iOpenEphAddr + 1).opcode == OP_Explain {
            sqlite3VdbeChangeToNoop(v, iOpenEphAddr + 1);
        }
        if e == WHERE_DISTINCT_ORDERED {
            /* Change the OP_OpenEphemeral to an OP_Null that sets the MEM_Cleared
             ** bit on the first register of the previous value.  This will cause the
             ** OP_Ne added in codeDistinct() to always fail on the first iteration of
             ** the loop even if the first row is all NULLs.  */
            let pOp = sqlite3VdbeGetOp(v, iOpenEphAddr);
            pOp.opcode = OP_Null;
            pOp.p1 = 1;
            pOp.p2 = iVal;
        }
    }
}

/*
** This routine generates the code for the inside of the inner loop
** of a SELECT.
**
** If srcTab is negative, then the p->pEList expressions
** are evaluated in order to get the data for this row.  If srcTab is
** zero or more, then data is pulled from srcTab and p->pEList is used only
** to get the number of columns and the collation sequence for each column.
*/
fn selectInnerLoop(
    db: &mut Sqlite3,
    p: SelectId,
    srcTab: i32,
    pSortIn: Option<&mut SortCtx>,
    pDistinct: Option<&mut DistinctCtx>,
    pDest: &mut SelectDest,
    iContinue: i32,
    iBreak: i32,
) {
    let eDest = pDest.eDest;
    let iParm = pDest.iSDParm;
    let mut nPrefixReg = 0;

    /* Usually, regResult is the first cell in an array of memory cells
     ** containing the current result row. In this case regOrig is set to the
     ** same value. However, if the results are being sent to the sorter, the
     ** values for any expressions that are also part of the sort-key are omitted
     ** from this array. In this case regOrig is set to zero.  */
    let regResult;
    let mut regOrig;

    let pEListP = db.sel(p).pEList;
    debug_assert!(pEListP.nn());
    let hasDistinct = match &pDistinct {
        Some(d) => d.eTnctType,
        None => WHERE_DISTINCT_NOOP,
    };
    let mut pSort: Option<&mut SortCtx> = match pSortIn {
        Some(s) if s.pOrderBy.nn() => Some(s),
        _ => None,
    };
    let iOffsetP = db.sel(p).iOffset;
    if pSort.is_none() && hasDistinct == 0 {
        debug_assert!(iContinue != 0);
        codeOffset(db.v(), iOffsetP, iContinue);
    }

    /* Pull the requested columns.
     */
    let mut nResultCol = db.el(pEListP).nExpr();

    if pDest.iSdst == 0 {
        if let Some(s) = &pSort {
            nPrefixReg = db.el(s.pOrderBy).nExpr();
            if (s.sortFlags & SORTFLAG_UseSorter) == 0 {
                nPrefixReg += 1;
            }
            db.pp().nMem += nPrefixReg;
        }
        pDest.iSdst = db.ppr().nMem + 1;
        db.pp().nMem += nResultCol;
    } else if pDest.iSdst + nResultCol > db.ppr().nMem {
        /* This is an error condition that can result, for example, when a SELECT
         ** on the right-hand side of an INSERT contains more result columns than
         ** there are columns in the table on the left.  The error will be caught
         ** and reported later.  But we need to make sure enough memory is allocated
         ** to avoid other spurious errors in the meantime. */
        db.pp().nMem += nResultCol;
    }
    pDest.nSdst = nResultCol;
    regResult = pDest.iSdst;
    regOrig = regResult;
    if srcTab >= 0 {
        for i in 0..nResultCol {
            sqlite3VdbeAddOp3(db.v(), OP_Column, srcTab, i, regResult + i);
        }
    } else if eDest != SRT_Exists {
        /* If the destination is an EXISTS(...) expression, the actual
         ** values returned by the SELECT are not required.
         */
        let mut ecelFlags: u8 /* "ecel" is an abbreviation of "ExprCodeExprList" */ =
            if eDest == SRT_Mem || eDest == SRT_Output || eDest == SRT_Coroutine {
                SQLITE_ECEL_DUP
            } else {
                0
            };
        if let Some(s) = &pSort {
            if hasDistinct == 0 && eDest != SRT_EphemTab && eDest != SRT_Table {
                /* For each expression in p->pEList that is a copy of an expression in
                 ** the ORDER BY clause (pSort->pOrderBy), set the associated
                 ** iOrderByCol value to one more than the index of the ORDER BY
                 ** expression within the sort-key that pushOntoSorter() will generate.
                 ** This allows the p->pEList field to be omitted from the sorted record,
                 ** saving space and CPU cycles.  */
                ecelFlags |= SQLITE_ECEL_OMITREF | SQLITE_ECEL_REF;

                let nOB = db.el(s.pOrderBy).nExpr();
                for i in s.nOBSat..nOB {
                    let j = db.el(s.pOrderBy).a[i as usize].u.x.iOrderByCol as i32;
                    if j > 0 {
                        db.elm(pEListP).a[(j - 1) as usize].u.x.iOrderByCol =
                            (i + 1 - s.nOBSat) as u16;
                    }
                }

                /* Adjust nResultCol to account for columns that are omitted
                 ** from the sorter by the optimizations in this branch */
                let n = db.el(pEListP).nExpr();
                for i in 0..n {
                    if db.el(pEListP).a[i as usize].u.x.iOrderByCol > 0 {
                        nResultCol -= 1;
                        regOrig = 0;
                    }
                }

                debug_assert!(
                    eDest == SRT_Set
                        || eDest == SRT_Mem
                        || eDest == SRT_Coroutine
                        || eDest == SRT_Output
                        || eDest == SRT_Upfrom
                );
            }
        }
        let sRowLoadInfo = RowLoadInfo {
            regResult,
            ecelFlags,
        };
        if db.sel(p).iLimit != 0 && (ecelFlags & SQLITE_ECEL_OMITREF) != 0 && nPrefixReg > 0 {
            debug_assert!(pSort.is_some());
            debug_assert!(hasDistinct == 0);
            pSort.as_mut().unwrap().pDeferredRowLoad = Some(sRowLoadInfo);
            regOrig = 0;
        } else {
            innerLoopLoadRow(db, p, &sRowLoadInfo);
        }
    }

    /* If the DISTINCT keyword was present on the SELECT statement
     ** and this row has been seen before, then do not make this row
     ** part of the result.
     */
    if hasDistinct != 0 {
        let d = pDistinct.unwrap();
        let eType = d.eTnctType as i32;
        let mut iTab = d.tabTnct;
        debug_assert!(nResultCol == db.el(pEListP).nExpr());
        iTab = codeDistinct(db, eType, iTab, iContinue, pEListP, regResult);
        fixDistinctOpenEph(db, eType, iTab, d.addrTnct);
        if pSort.is_none() {
            codeOffset(db.v(), iOffsetP, iContinue);
        }
    }

    match eDest {
        /* Store the result as data using a unique key.
         */
        SRT_Fifo | SRT_DistFifo | SRT_Table | SRT_EphemTab => {
            let r1 = sqlite3GetTempRange(db, nPrefixReg + 1);
            sqlite3VdbeAddOp3(
                db.v(),
                OP_MakeRecord,
                regResult,
                nResultCol,
                r1 + nPrefixReg,
            );
            if eDest == SRT_DistFifo {
                /* If the destination is DistFifo, then cursor (iParm+1) is open
                 ** on an ephemeral index. If the current row is already present
                 ** in the index, do not write it to the output. If not, add the
                 ** current row to the index and proceed with writing it to the
                 ** output table as well.  */
                let v = db.v();
                let addr = sqlite3VdbeCurrentAddr(v) + 4;
                sqlite3VdbeAddOp4Int(v, OP_Found, iParm + 1, addr, r1, 0);
                sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm + 1, r1, regResult, nResultCol);
                debug_assert!(pSort.is_none());
            }
            if let Some(s) = pSort.as_mut() {
                debug_assert!(regResult == regOrig);
                pushOntoSorter(db, s, p, r1 + nPrefixReg, regOrig, 1, nPrefixReg);
            } else {
                let r2 = sqlite3GetTempReg(db);
                let v = db.v();
                sqlite3VdbeAddOp2(v, OP_NewRowid, iParm, r2);
                sqlite3VdbeAddOp3(v, OP_Insert, iParm, r1, r2);
                sqlite3VdbeChangeP5(v, OPFLAG_APPEND);
                sqlite3ReleaseTempReg(db, r2);
            }
            sqlite3ReleaseTempRange(db, r1, nPrefixReg + 1);
        }

        SRT_Upfrom => {
            if let Some(s) = pSort.as_mut() {
                pushOntoSorter(db, s, p, regResult, regOrig, nResultCol, nPrefixReg);
            } else {
                let i2 = pDest.iSDParm2;
                let r1 = sqlite3GetTempReg(db);
                let v = db.v();

                /* If the UPDATE FROM join is an aggregate that matches no rows, it
                 ** might still be trying to return one row, because that is what
                 ** aggregates do.  Don't record that empty row in the output table. */
                sqlite3VdbeAddOp2(v, OP_IsNull, regResult, iBreak);

                let b = (i2 < 0) as i32;
                sqlite3VdbeAddOp3(v, OP_MakeRecord, regResult + b, nResultCol - b, r1);
                if i2 < 0 {
                    sqlite3VdbeAddOp3(v, OP_Insert, iParm, r1, regResult);
                } else {
                    sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, r1, regResult, i2);
                }
            }
        }

        /* If we are creating a set for an "expr IN (SELECT ...)" construct,
         ** then there should be a single item on the stack.  Write this
         ** item into the set table with bogus data.
         */
        SRT_Set => {
            if let Some(s) = pSort.as_mut() {
                /* At first glance you would think we could optimize out the
                 ** ORDER BY in this case since the order of entries in the set
                 ** does not matter.  But there might be a LIMIT clause, in which
                 ** case the order does matter */
                pushOntoSorter(db, s, p, regResult, regOrig, nResultCol, nPrefixReg);
                pDest.iSDParm2 = 0; /* Signal that any Bloom filter is unpopulated */
            } else {
                let r1 = sqlite3GetTempReg(db);
                let aff = pDest.zAffSdst.clone();
                let v = db.v();
                sqlite3VdbeAddOp4(
                    v,
                    OP_MakeRecord,
                    regResult,
                    nResultCol,
                    r1,
                    p4str(aff),
                    nResultCol,
                );
                sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, r1, regResult, nResultCol);
                if pDest.iSDParm2 != 0 {
                    sqlite3VdbeAddOp4Int(v, OP_FilterAdd, pDest.iSDParm2, 0, regResult, nResultCol);
                    explainQP(db, 0, b"CREATE BLOOM FILTER", &[]);
                }
                sqlite3ReleaseTempReg(db, r1);
            }
        }

        /* If any row exist in the result set, record that fact and abort.
         */
        SRT_Exists => {
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, iParm);
            /* The LIMIT clause will terminate the loop for us */
        }

        /* If this is a scalar select that is part of an expression, then
         ** store the results in the appropriate memory cell or array of
         ** memory cells and break out of the scan loop.
         */
        SRT_Mem => {
            if let Some(s) = pSort.as_mut() {
                debug_assert!(nResultCol <= pDest.nSdst);
                pushOntoSorter(db, s, p, regResult, regOrig, nResultCol, nPrefixReg);
                pDest.iSDParm = regResult;
            } else {
                debug_assert!(nResultCol == pDest.nSdst);
                if regResult != iParm {
                    /* This occurs in cases where the SELECT had both a DISTINCT and
                     ** an OFFSET clause.  */
                    sqlite3VdbeAddOp3(db.v(), OP_Copy, regResult, iParm, nResultCol - 1);
                }
                /* The LIMIT clause will jump out of the loop for us */
            }
        }

        SRT_Coroutine | SRT_Output => {
            /* Send data to a co-routine / Return the results */
            if let Some(s) = pSort.as_mut() {
                pushOntoSorter(db, s, p, regResult, regOrig, nResultCol, nPrefixReg);
            } else if eDest == SRT_Coroutine {
                sqlite3VdbeAddOp1(db.v(), OP_Yield, pDest.iSDParm);
            } else {
                sqlite3VdbeAddOp2(db.v(), OP_ResultRow, regResult, nResultCol);
            }
        }

        /* Write the results into a priority queue that is order according to
         ** pDest->pOrderBy (in pSO).  pDest->iSDParm (in iParm) is the cursor for an
         ** index with pSO->nExpr+2 columns.  Build a key using pSO for the first
         ** pSO->nExpr columns, then make sure all keys are unique by adding a
         ** final OP_Sequence column.  The last column is the record as a blob.
         */
        SRT_DistQueue | SRT_Queue => {
            let mut addrTest = 0;
            let pSO = pDest.pOrderBy;
            debug_assert!(pSO.nn());
            let nKey = db.el(pSO).nExpr();
            let r1 = sqlite3GetTempReg(db);
            let r2 = sqlite3GetTempRange(db, nKey + 2);
            let r3 = r2 + nKey + 1;
            if eDest == SRT_DistQueue {
                /* If the destination is DistQueue, then cursor (iParm+1) is open
                 ** on a second ephemeral index that holds all values every previously
                 ** added to the queue. */
                addrTest =
                    sqlite3VdbeAddOp4Int(db.v(), OP_Found, iParm + 1, 0, regResult, nResultCol);
            }
            sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regResult, nResultCol, r3);
            if eDest == SRT_DistQueue {
                sqlite3VdbeAddOp2(db.v(), OP_IdxInsert, iParm + 1, r3);
                sqlite3VdbeChangeP5(db.v(), OPFLAG_USESEEKRESULT);
            }
            for i in 0..nKey {
                let c = db.el(pSO).a[i as usize].u.x.iOrderByCol as i32;
                sqlite3VdbeAddOp2(db.v(), OP_SCopy, regResult + c - 1, r2 + i);
            }
            let v = db.v();
            sqlite3VdbeAddOp2(v, OP_Sequence, iParm, r2 + nKey);
            sqlite3VdbeAddOp3(v, OP_MakeRecord, r2, nKey + 2, r1);
            sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, r1, r2, nKey + 2);
            if addrTest != 0 {
                sqlite3VdbeJumpHere(v, addrTest);
            }
            sqlite3ReleaseTempReg(db, r1);
            sqlite3ReleaseTempRange(db, r2, nKey + 2);
        }

        /* Discard the results.  This is used for SELECT statements inside
         ** the body of a TRIGGER.  The purpose of such selects is to call
         ** user-defined functions that have side effects.  We do not care
         ** about the actual results of the select.
         */
        _ => {
            debug_assert!(eDest == SRT_Discard);
        }
    }

    /* Jump to the end of the loop if the LIMIT is reached.  Except, if
     ** there is a sorter, in which case the sorter has already limited
     ** the output for us.
     */
    let iLimit = db.sel(p).iLimit;
    if pSort.is_none() && iLimit != 0 {
        sqlite3VdbeAddOp2(db.v(), OP_DecrJumpZero, iLimit, iBreak);
    }
}

/*
** Allocate a KeyInfo object sufficient for an index of N key columns and
** X extra columns.
*/
pub fn sqlite3KeyInfoAlloc(db: &mut Sqlite3, N: i32, X: i32) -> Option<KeyInfoRef> {
    debug_assert!(X >= 0);
    if N + X > 0xffff {
        db.mallocFailed = 1;
        return None;
    }
    let n = (N + X) as usize;
    Some(Rc::new(std::cell::RefCell::new(KeyInfo {
        enc: db.ENC(),
        nKeyField: N as u16,
        nAllField: (N + X) as u16,
        aSortFlags: vec![0; n],
        aColl: vec![None; n],
    })))
}

/*
** Deallocate a KeyInfo object
**
** Rust: KeyInfo objects are reference counted; dropping the last `Rc`
** frees it.
*/
pub fn sqlite3KeyInfoUnref(db: &mut Sqlite3, p: &KeyInfoRef) {
    let _ = (db, p);
}

/*
** Make a new pointer to a KeyInfo object
*/
pub fn sqlite3KeyInfoRef(p: &KeyInfoRef) -> Option<KeyInfoRef> {
    Some(p.clone())
}

/*
** Return TRUE if a KeyInfo object can be change.  The KeyInfo object
** can only be changed if this is just a single reference to the object.
**
** This routine is used only inside of assert() statements.
*/
pub fn sqlite3KeyInfoIsWriteable(p: &KeyInfoRef) -> i32 {
    (Rc::strong_count(p) == 1) as i32
}

/*
** Given an expression list, generate a KeyInfo structure that records
** the collating sequence for each expression in that expression list.
**
** If the ExprList is an ORDER BY or GROUP BY clause then the resulting
** KeyInfo structure is appropriate for initializing a virtual index to
** implement that clause.  If the ExprList is the result set of a SELECT
** then the KeyInfo structure is appropriate for initializing a virtual
** index to implement a DISTINCT test.
**
** Space to hold the KeyInfo structure is obtained from malloc.  The calling
** function is responsible for seeing that this structure is eventually
** freed.
*/
pub fn sqlite3KeyInfoFromExprList(
    db: &mut Sqlite3,
    pList: ExprListId,
    iStart: i32,
    nExtra: i32,
) -> Option<KeyInfoRef> {
    let nExpr = db.el(pList).nExpr();
    let pInfo = sqlite3KeyInfoAlloc(db, nExpr - iStart, nExtra + 1);
    if let Some(info) = &pInfo {
        debug_assert!(sqlite3KeyInfoIsWriteable(info) != 0);
        for i in iStart..nExpr {
            let (pe, sf) = {
                let it = &db.el(pList).a[i as usize];
                (it.pExpr, it.fg.sortFlags)
            };
            let c = sqlite3ExprNNCollSeq(db, pe);
            let mut k = info.borrow_mut();
            k.aColl[(i - iStart) as usize] = c;
            k.aSortFlags[(i - iStart) as usize] = sf;
        }
    }
    pInfo
}

/*
** Name of the connection operator, used for error messages.
*/
pub fn sqlite3SelectOpName(id: i32) -> Option<ZStr> {
    Some(selectOpName(id).to_vec())
}

/// sqlite3SelectOpName() returning the static string.
fn selectOpName(id: i32) -> &'static [u8] {
    match id as u8 {
        TK_ALL => b"UNION ALL",
        TK_INTERSECT => b"INTERSECT",
        TK_EXCEPT => b"EXCEPT",
        _ => b"UNION",
    }
}

/*
** Unless an "EXPLAIN QUERY PLAN" command is being processed, this function
** is a no-op. Otherwise, it adds a single row of output to the EQP result,
** where the caption is of the form:
**
**   "USE TEMP B-TREE FOR xxx"
**
** where xxx is one of "DISTINCT", "ORDER BY" or "GROUP BY". Exactly which
** is determined by the zUsage argument.
*/
fn explainTempTable(db: &mut Sqlite3, zUsage: &[u8]) {
    explainQP(db, 0, b"USE TEMP B-TREE FOR %s", &[PfArg::from(zUsage)]);
}

/*
** If the inner loop was generated using a non-null pOrderBy argument,
** then the results were placed in a sorter.  After the loop is terminated
** we need to run the sorter and output the results.  The following
** routine generates the code needed to do that.
*/
fn generateSortTail(
    db: &mut Sqlite3,
    p: SelectId,
    pSort: &mut SortCtx,
    mut nColumn: i32,
    pDest: &mut SelectDest,
) {
    let addrBreak = pSort.labelDone; /* Jump here to exit loop */
    let addrContinue = sqlite3VdbeMakeLabel(db); /* Jump here for next cycle */
    let addr;
    let mut addrOnce = 0;
    let pOrderBy = pSort.pOrderBy;
    let eDest = pDest.eDest;
    let iParm = pDest.iSDParm;
    let regRow;
    let regRowid;
    let mut iCol;
    let nKey;
    let iSortTab;
    let bSeq;
    let nRefKey = 0;
    let pEList = db.sel(p).pEList;
    let (iOffsetP, iLimitP) = {
        let s = db.sel(p);
        (s.iOffset, s.iLimit)
    };

    nKey = db.el(pOrderBy).nExpr() - pSort.nOBSat;
    if pSort.nOBSat == 0 || nKey == 1 {
        let z: &[u8] = if pSort.nOBSat != 0 {
            b"LAST TERM OF "
        } else {
            b""
        };
        explainQP(db, 0, b"USE TEMP B-TREE FOR %sORDER BY", &[PfArg::from(z)]);
    } else {
        explainQP(
            db,
            0,
            b"USE TEMP B-TREE FOR LAST %d TERMS OF ORDER BY",
            &[PfArg::from(nKey)],
        );
    }

    debug_assert!(addrBreak < 0);
    if pSort.labelBkOut != 0 {
        let v = db.v();
        sqlite3VdbeAddOp2(v, OP_Gosub, pSort.regReturn, pSort.labelBkOut);
        sqlite3VdbeGoto(v, addrBreak);
        sqlite3VdbeResolveLabel(v, pSort.labelBkOut);
    }

    let iTab = pSort.iECursor;
    if eDest == SRT_Output || eDest == SRT_Coroutine || eDest == SRT_Mem {
        if eDest == SRT_Mem && iOffsetP != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, pDest.iSdst);
        }
        regRowid = 0;
        regRow = pDest.iSdst;
    } else {
        regRowid = sqlite3GetTempReg(db);
        if eDest == SRT_EphemTab || eDest == SRT_Table {
            regRow = sqlite3GetTempReg(db);
            nColumn = 0;
        } else {
            regRow = sqlite3GetTempRange(db, nColumn);
        }
    }
    if (pSort.sortFlags & SORTFLAG_UseSorter) != 0 {
        db.pp().nMem += 1;
        let regSortOut = db.ppr().nMem;
        iSortTab = db.ppr().nTab;
        db.pp().nTab += 1;
        let v = db.v();
        if pSort.labelBkOut != 0 {
            addrOnce = sqlite3VdbeAddOp0(v, OP_Once);
        }
        sqlite3VdbeAddOp3(
            v,
            OP_OpenPseudo,
            iSortTab,
            regSortOut,
            nKey + 1 + nColumn + nRefKey,
        );
        if addrOnce != 0 {
            sqlite3VdbeJumpHere(v, addrOnce);
        }
        addr = 1 + sqlite3VdbeAddOp2(v, OP_SorterSort, iTab, addrBreak);
        debug_assert!(iLimitP == 0 && iOffsetP == 0);
        sqlite3VdbeAddOp3(v, OP_SorterData, iTab, regSortOut, iSortTab);
        bSeq = 0;
    } else {
        let v = db.v();
        addr = 1 + sqlite3VdbeAddOp2(v, OP_Sort, iTab, addrBreak);
        codeOffset(v, iOffsetP, addrContinue);
        iSortTab = iTab;
        bSeq = 1;
        if iOffsetP > 0 {
            sqlite3VdbeAddOp2(v, OP_AddImm, iLimitP, -1);
        }
    }
    iCol = nKey + bSeq - 1;
    for i in 0..nColumn {
        if db.el(pEList).a[i as usize].u.x.iOrderByCol == 0 {
            iCol += 1;
        }
    }
    let mut i = nColumn - 1;
    while i >= 0 {
        let iRead;
        let ob = db.el(pEList).a[i as usize].u.x.iOrderByCol as i32;
        if ob != 0 {
            iRead = ob - 1;
        } else {
            iRead = iCol;
            iCol -= 1;
        }
        sqlite3VdbeAddOp3(db.v(), OP_Column, iSortTab, iRead, regRow + i);
        i -= 1;
    }
    match eDest {
        SRT_Table | SRT_EphemTab => {
            let v = db.v();
            sqlite3VdbeAddOp3(v, OP_Column, iSortTab, nKey + bSeq, regRow);
            sqlite3VdbeAddOp2(v, OP_NewRowid, iParm, regRowid);
            sqlite3VdbeAddOp3(v, OP_Insert, iParm, regRow, regRowid);
            sqlite3VdbeChangeP5(v, OPFLAG_APPEND);
        }
        SRT_Set => {
            debug_assert!(nColumn == crate::util::sqlite3Strlen30(pDest.zAffSdst.as_deref()));
            let aff = pDest.zAffSdst.clone();
            let v = db.v();
            sqlite3VdbeAddOp4(
                v,
                OP_MakeRecord,
                regRow,
                nColumn,
                regRowid,
                p4str(aff),
                nColumn,
            );
            sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, regRowid, regRow, nColumn);
        }
        SRT_Mem => { /* The LIMIT clause will terminate the loop for us */ }
        SRT_Upfrom => {
            let i2 = pDest.iSDParm2;
            let r1 = sqlite3GetTempReg(db);
            let v = db.v();
            let b = (i2 < 0) as i32;
            sqlite3VdbeAddOp3(v, OP_MakeRecord, regRow + b, nColumn - b, r1);
            if i2 < 0 {
                sqlite3VdbeAddOp3(v, OP_Insert, iParm, r1, regRow);
            } else {
                sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, r1, regRow, i2);
            }
        }
        _ => {
            debug_assert!(eDest == SRT_Output || eDest == SRT_Coroutine);
            if eDest == SRT_Output {
                sqlite3VdbeAddOp2(db.v(), OP_ResultRow, pDest.iSdst, nColumn);
            } else {
                sqlite3VdbeAddOp1(db.v(), OP_Yield, pDest.iSDParm);
            }
        }
    }
    if regRowid != 0 {
        if eDest == SRT_Set {
            sqlite3ReleaseTempRange(db, regRow, nColumn);
        } else {
            sqlite3ReleaseTempReg(db, regRow);
        }
        sqlite3ReleaseTempReg(db, regRowid);
    }
    /* The bottom of the loop
     */
    let v = db.v();
    sqlite3VdbeResolveLabel(v, addrContinue);
    if (pSort.sortFlags & SORTFLAG_UseSorter) != 0 {
        sqlite3VdbeAddOp2(v, OP_SorterNext, iTab, addr);
    } else {
        sqlite3VdbeAddOp2(v, OP_Next, iTab, addr);
    }
    if pSort.regReturn != 0 {
        sqlite3VdbeAddOp1(v, OP_Return, pSort.regReturn);
    }
    sqlite3VdbeResolveLabel(v, addrBreak);
}

/// The chain of name contexts used by columnType() (the C code builds
/// NameContext objects on the stack, linked through pNext).
struct NcChain<'a> {
    pSrcList: SrcListId,
    pNext: Option<&'a NcChain<'a>>,
}

/// Result of columnTypeImpl(): (zType, zOrigDb, zOrigTab, zOrigCol).
type ColumnTypeInfo = (Option<ZStr>, Option<ZStr>, Option<ZStr>, Option<ZStr>);

/*
** Return a pointer to a string containing the 'declaration type' of the
** expression pExpr. The string may be treated as static by the caller.
**
** The declaration type is the exact datatype definition extracted from the
** original CREATE TABLE statement if the expression is a column. The
** declaration type for a ROWID field is INTEGER. Exactly when an expression
** is considered a column can be complex in the presence of subqueries. The
** result-set expression in all of the following SELECT statements is
** considered a column by this function.
**
**   SELECT col FROM tbl;
**   SELECT (SELECT col FROM tbl;
**   SELECT (SELECT col FROM tbl);
**   SELECT abc FROM (SELECT col AS abc FROM tbl);
**
** The declaration type for any expression other than a column is NULL.
**
** (SQLITE_ENABLE_COLUMN_METADATA build: the origin database, table and
** column names are returned too.)
*/
fn columnTypeImpl(db: &mut Sqlite3, pNC0: &NcChain, pExpr: ExprId) -> ColumnTypeInfo {
    let mut zType: Option<ZStr> = None;
    let mut zOrigDb: Option<ZStr> = None;
    let mut zOrigTab: Option<ZStr> = None;
    let mut zOrigCol: Option<ZStr> = None;

    debug_assert!(pExpr.nn());
    let (op, iTableE, iColumnE, pSelectE) = {
        let e = db.ex(pExpr);
        (e.op, e.iTable, e.iColumn, e.x.pSelect)
    };
    match op {
        TK_COLUMN => {
            /* The expression is a column. Locate the table the column is being
             ** extracted from in NameContext.pSrcList. This table may be real
             ** database table or a subquery.
             */
            let mut pTab: Option<TableRef> = None; /* Table structure column is extracted from */
            let mut pS = SelectId::NULL; /* Select the column is extracted from */
            let mut iCol = iColumnE as i32; /* Index of column in pTab */
            let mut pNC: Option<&NcChain> = Some(pNC0);
            while let Some(nc) = pNC {
                if pTab.is_some() {
                    break;
                }
                let pTabList = nc.pSrcList;
                let nSrc = db.src(pTabList).nSrc();
                let mut j = 0;
                while j < nSrc && db.src(pTabList).a[j as usize].iCursor != iTableE {
                    j += 1;
                }
                if j < nSrc {
                    let it = SrcItemId(pTabList, j as u32);
                    pTab = db.item(it).pSTab.clone();
                    if db.item(it).fg.isSubquery != 0 {
                        pS = subqSelect(db, it);
                    } else {
                        pS = SelectId::NULL;
                    }
                    if pTab.is_none() {
                        break;
                    }
                } else {
                    pNC = nc.pNext;
                }
            }

            let pTab = match pTab {
                Some(t) => t,
                None => {
                    /* At one time, code such as "SELECT new.x" within a trigger would
                     ** cause this condition to run.  Since then, we have restructured how
                     ** trigger code is generated and so this condition is no longer
                     ** possible. However, it can still be true for statements like
                     ** the following:
                     **
                     **   CREATE TABLE t1(col INTEGER);
                     **   SELECT (SELECT t1.col) FROM FROM t1;
                     **
                     ** when columnType() is called on the expression "t1.col" in the
                     ** sub-select. In this case, set the column type to NULL, even
                     ** though it should really be "INTEGER".
                     **
                     ** This is not a problem, as the column type of "t1.col" is never
                     ** used. When columnType() is called on the expression
                     ** "(SELECT t1.col)", the correct type is returned (see the TK_SELECT
                     ** branch below.  */
                    return (None, None, None, None);
                }
            };
            let pNC = pNC.unwrap();

            if pS.nn() {
                /* The "table" is actually a sub-select or a view in the FROM clause
                 ** of the SELECT statement. Return the declaration type and origin
                 ** data for the result-set column of the sub-select.
                 */
                let pSEList = db.sel(pS).pEList;
                if iCol < db.el(pSEList).nExpr() && iCol >= 0 {
                    /* If iCol is less than zero, then the expression requests the
                     ** rowid of the sub-select or view. This expression is legal (see
                     ** test case misc2.2.2) - it always evaluates to NULL.
                     */
                    let p = db.el(pSEList).a[iCol as usize].pExpr;
                    let sNC = NcChain {
                        pSrcList: db.sel(pS).pSrc,
                        pNext: Some(pNC),
                    };
                    let r = columnTypeImpl(db, &sNC, p);
                    zType = r.0;
                    zOrigDb = r.1;
                    zOrigTab = r.2;
                    zOrigCol = r.3;
                }
            } else {
                /* A real table or a CTE table */
                let t = pTab.borrow();
                if iCol < 0 {
                    iCol = t.iPKey as i32;
                }
                debug_assert!(iCol == XN_ROWID as i32 || (iCol >= 0 && iCol < t.nCol as i32));
                if iCol < 0 {
                    zType = Some(b"INTEGER".to_vec());
                    zOrigCol = Some(b"rowid".to_vec());
                } else {
                    zOrigCol = Some(t.aCol[iCol as usize].zCnName.clone());
                    let mut c = t.aCol[iCol as usize].clone();
                    zType = crate::util::sqlite3ColumnType(&mut c, None);
                }
                zOrigTab = Some(t.zName.clone());
                let iDb = crate::prepare::sqlite3SchemaToIndex(db, t.pSchema);
                if iDb >= 0 {
                    zOrigDb = db.aDb[iDb as usize].zDbSName.clone();
                }
            }
        }
        TK_SELECT => {
            /* The expression is a sub-select. Return the declaration type and
             ** origin info for the single column in the result set of the SELECT
             ** statement.
             */
            debug_assert!(db.ex(pExpr).UseXSelect());
            let pS = pSelectE;
            let pSEList = db.sel(pS).pEList;
            let p = db.el(pSEList).a[0].pExpr;
            let sNC = NcChain {
                pSrcList: db.sel(pS).pSrc,
                pNext: Some(pNC0),
            };
            let r = columnTypeImpl(db, &sNC, p);
            zType = r.0;
            zOrigDb = r.1;
            zOrigTab = r.2;
            zOrigCol = r.3;
        }
        _ => {}
    }

    (zType, zOrigDb, zOrigTab, zOrigCol)
}

/*
** Generate code that will tell the VDBE the declaration types of columns
** in the result set.
*/
fn generateColumnTypes(db: &mut Sqlite3, pTabList: SrcListId, pEList: ExprListId) {
    let sNC = NcChain {
        pSrcList: pTabList,
        pNext: None,
    };
    let n = db.el(pEList).nExpr();
    for i in 0..n {
        let p = db.el(pEList).a[i as usize].pExpr;
        let (zType, zOrigDb, zOrigTab, zOrigCol) = columnTypeImpl(db, &sNC, p);

        /* The vdbe must make its own copy of the column-type and other
         ** column specific strings, in case the schema is reset before this
         ** virtual machine is deleted.
         */
        setColName(db, i, COLNAME_DATABASE, zOrigDb.as_deref());
        setColName(db, i, COLNAME_TABLE, zOrigTab.as_deref());
        setColName(db, i, COLNAME_COLUMN, zOrigCol.as_deref());
        setColName(db, i, COLNAME_DECLTYPE, zType.as_deref());
    }
}

/*
** Compute the column names for a SELECT statement.
**
** The only guarantee that SQLite makes about column names is that if the
** column has an AS clause assigning it a name, that will be the name used.
** That is the only documented guarantee.  However, countless applications
** developed over the years have made baseless assumptions about column names
** and will break if those assumptions changes.  Hence, use extreme caution
** when modifying this routine to avoid breaking legacy.
**
** See Also: sqlite3ColumnsFromExprList()
**
** The PRAGMA short_column_names and PRAGMA full_column_names settings are
** deprecated.  The default setting is short=ON, full=OFF.  99.9% of all
** applications should operate this way.  Nevertheless, we need to support the
** other modes for legacy:
**
**    short=OFF, full=OFF:      Column name is the text of the expression has it
**                              originally appears in the SELECT statement.  In
**                              other words, the zSpan of the result expression.
**
**    short=ON, full=OFF:       (This is the default setting).  If the result
**                              refers directly to a table column, then the
**                              result column name is just the table column
**                              name: COLUMN.  Otherwise use zSpan.
**
**    full=ON, short=ANY:       If the result refers directly to a table column,
**                              then the result column name with the table name
**                              prefix, ex: TABLE.COLUMN.  Otherwise use zSpan.
*/
pub fn sqlite3GenerateColumnNames(db: &mut Sqlite3, mut pSelect: SelectId) {
    if db.ppr().colNamesSet != 0 {
        return;
    }
    /* Column names are determined by the left-most term of a compound select */
    while db.sel(pSelect).pPrior.nn() {
        pSelect = db.sel(pSelect).pPrior;
    }
    let pTabList = db.sel(pSelect).pSrc;
    let pEList = db.sel(pSelect).pEList;
    debug_assert!(pTabList.nn());
    db.pp().colNamesSet = 1;
    let fullName = (db.flags & SQLITE_FullColNames) != 0;
    let srcName = (db.flags & SQLITE_ShortColNames) != 0 || fullName;
    let n = db.el(pEList).nExpr();
    sqlite3VdbeSetNumCols(db.v(), n);
    for i in 0..n {
        let (p, zEName, eEName) = {
            let it = &db.el(pEList).a[i as usize];
            (it.pExpr, it.zEName.clone(), it.fg.eEName)
        };
        debug_assert!(p.nn());
        let (op, iColumn, pTab) = {
            let e = db.ex(p);
            (e.op, e.iColumn, e.y.pTab.clone())
        };
        debug_assert!(op != TK_AGG_COLUMN); /* Agg processing has not run yet */
        if zEName.is_some() && eEName == ENAME_NAME {
            /* An AS clause always takes first priority */
            sqlite3VdbeSetColName(db.v(), i, COLNAME_NAME, zEName.as_deref().unwrap());
        } else if srcName && op == TK_COLUMN {
            let pTab = pTab.expect("TK_COLUMN without table");
            let (zCol, zTabName) = {
                let t = pTab.borrow();
                let mut iCol = iColumn as i32;
                if iCol < 0 {
                    iCol = t.iPKey as i32;
                }
                debug_assert!(iCol == -1 || (iCol >= 0 && iCol < t.nCol as i32));
                let zCol: ZStr = if iCol < 0 {
                    b"rowid".to_vec()
                } else {
                    t.aCol[iCol as usize].zCnName.clone()
                };
                (zCol, t.zName.clone())
            };
            if fullName {
                let zName = mprintf!(db, b"%s.%s", &zTabName, &zCol);
                sqlite3VdbeSetColName(db.v(), i, COLNAME_NAME, &zName);
            } else {
                sqlite3VdbeSetColName(db.v(), i, COLNAME_NAME, &zCol);
            }
        } else {
            let z = match zEName {
                None => mprintf!(db, b"column%d", i + 1),
                Some(z) => z,
            };
            sqlite3VdbeSetColName(db.v(), i, COLNAME_NAME, &z);
        }
    }
    generateColumnTypes(db, pTabList, pEList);
}

/*
** Given an expression list (which is really the list of expressions
** that form the result set of a SELECT statement) compute appropriate
** column names for a table that would hold the expression list.
**
** All column names will be unique.
**
** Only the column names are computed.  Column.zType, Column.zColl,
** and other fields of Column are zeroed.
**
** Return SQLITE_OK on success.  If a memory allocation error occurs,
** store NULL in *paCol and 0 in *pnCol and return SQLITE_NOMEM.
**
** The only guarantee that SQLite makes about column names is that if the
** column has an AS clause assigning it a name, that will be the name used.
** That is the only documented guarantee.  However, countless applications
** developed over the years have made baseless assumptions about column names
** and will break if those assumptions changes.  Hence, use extreme caution
** when modifying this routine to avoid breaking legacy.
**
** See Also: sqlite3GenerateColumnNames()
**
** Rust: `paCol` is the Table::aCol vector (C `Column**`).
*/
pub fn sqlite3ColumnsFromExprList(
    db: &mut Sqlite3,
    pEList: ExprListId,
    pnCol: &mut i16,
    paCol: &mut Vec<Column>,
) -> i32 {
    let mut ht: Hash<usize> = Hash::new(); /* Hash table of column names */
    let mut nCol: i32;
    if pEList.nn() {
        nCol = db.el(pEList).nExpr();
        if nCol > 32767 {
            nCol = 32767;
        }
    } else {
        nCol = 0;
    }
    let mut aCol: Vec<Column> = vec![Column::default(); nCol as usize];
    *pnCol = nCol as i16;

    let mut i = 0;
    while i < nCol && db.ppr().nErr == 0 {
        let (pXExpr, pXName, pXeEName, bNoExpand) = {
            let it = &db.el(pEList).a[i as usize];
            (it.pExpr, it.zEName.clone(), it.fg.eEName, it.fg.bNoExpand)
        };
        /* Get an appropriate name for the column
         */
        let mut zName: Option<ZStr> = pXName.clone();
        if zName.is_some() && pXeEName == ENAME_NAME {
            /* If the column contains an "AS <name>" phrase, use <name> as the name */
        } else {
            let mut pColExpr = sqlite3ExprSkipCollateAndLikely(db, pXExpr);
            while pColExpr.nn() && db.ex(pColExpr).op == TK_DOT {
                pColExpr = db.ex(pColExpr).pRight;
                debug_assert!(pColExpr.nn());
            }
            let e = db.ex(pColExpr);
            if e.op == TK_COLUMN && e.UseYTab() && e.y.pTab.is_some() {
                /* For columns use the column name name */
                let mut iCol = e.iColumn as i32;
                let t = e.y.pTab.as_ref().unwrap().borrow();
                if iCol < 0 {
                    iCol = t.iPKey as i32;
                }
                zName = Some(if iCol >= 0 {
                    t.aCol[iCol as usize].zCnName.clone()
                } else {
                    b"rowid".to_vec()
                });
            } else if e.op == TK_ID {
                debug_assert!(!e.HasProperty(EP_IntValue));
                zName = e.u.zToken.clone();
            } else {
                /* Use the original text of the column expression as its name */
                debug_assert!(zName == pXName);
            }
        }
        let mut zName: ZStr = match zName {
            Some(z) if crate::expr::sqlite3IsTrueOrFalse(Some(&z)) == 0 => z,
            _ => mprintf!(db, b"column%d", i + 1),
        };

        /* Make sure the column name is unique.  If the name is not unique,
         ** append an integer to the name so that it becomes unique.
         */
        let mut cnt: u32 = 0;
        while let Some(&iCollide) = ht.find(&zName) {
            if db.el(pEList).a[iCollide].fg.bUsingTerm != 0 {
                aCol[i as usize].colFlags |= COLFLAG_NOEXPAND;
            }
            let mut nName = crate::util::sqlite3Strlen30(Some(&zName));
            if nName > 0 {
                let mut j = nName - 1;
                while j > 0 && crate::global::sqlite3Isdigit(zName[j as usize]) {
                    j -= 1;
                }
                if zName[j as usize] == b':' {
                    nName = j;
                }
            }
            cnt = cnt.wrapping_add(1);
            zName = mprintf!(db, b"%.*z:%u", nName, &zName, cnt);
            crate::util::sqlite3ProgressCheck(db);
            if cnt > 3 {
                let mut b = [0u8; 4];
                crate::random::sqlite3_randomness(4, &mut b);
                cnt = u32::from_ne_bytes(b);
            }
        }
        let pCol = &mut aCol[i as usize];
        pCol.hName = crate::util::sqlite3StrIHash(Some(&zName));
        pCol.zCnName = zName.clone();
        if bNoExpand != 0 {
            pCol.colFlags |= COLFLAG_NOEXPAND;
        }
        /* sqlite3ColumnPropertiesFromName(0, pCol) is a no-op without
         ** SQLITE_ENABLE_HIDDEN_COLUMNS */
        ht.insert(&zName, i as usize);
        i += 1;
    }
    if db.ppr().nErr != 0 {
        paCol.clear();
        *pnCol = 0;
        return db.ppr().rc;
    }
    *paCol = aCol;
    SQLITE_OK
}

/*
** pTab is a transient Table object that represents a subquery of some
** kind (maybe a parenthesized subquery in the FROM clause of a larger
** query, or a VIEW, or a CTE).  This routine computes type information
** for that Table object based on the Select object that implements the
** subquery.  For the purposes of this routine, "type information" means:
**
**    *   The datatype name, as it might appear in a CREATE TABLE statement
**    *   Which collating sequence to use for the column
**    *   The affinity of the column
*/
pub fn sqlite3SubqueryColumnTypes(
    db: &mut Sqlite3,
    pTab: &TableRef,
    mut pSelect: SelectId,
    aff: u8,
) {
    debug_assert!(pSelect.nn());
    debug_assert!((db.sel(pSelect).selFlags & SF_Resolved) != 0);
    debug_assert!(aff == SQLITE_AFF_NONE || aff == SQLITE_AFF_BLOB);
    if db.mallocFailed != 0 || db.ppr().IN_RENAME_OBJECT() {
        return;
    }
    while db.sel(pSelect).pPrior.nn() {
        pSelect = db.sel(pSelect).pPrior;
    }
    let a = db.sel(pSelect).pEList;
    let sNC = NcChain {
        pSrcList: db.sel(pSelect).pSrc,
        pNext: None,
    };
    let nCol = pTab.borrow().nCol as i32;
    for i in 0..nCol {
        let mut m: i32 = 0;
        let mut pS2 = pSelect;
        let mut pCol = pTab.borrow().aCol[i as usize].clone();
        pTab.borrow_mut().tabFlags |= (pCol.colFlags & COLFLAG_NOINSERT) as u32;
        let p = db.el(a).a[i as usize].pExpr;
        /* pCol->szEst = ... // Column size est for SELECT tables never used */
        pCol.affinity = sqlite3ExprAffinity(db, p);
        while pCol.affinity <= SQLITE_AFF_NONE && db.sel(pS2).pNext.nn() {
            let e = db.el(db.sel(pS2).pEList).a[i as usize].pExpr;
            m |= sqlite3ExprDataType(db, e);
            pS2 = db.sel(pS2).pNext;
            let e = db.el(db.sel(pS2).pEList).a[i as usize].pExpr;
            pCol.affinity = sqlite3ExprAffinity(db, e);
        }
        if pCol.affinity <= SQLITE_AFF_NONE {
            pCol.affinity = aff;
        }
        if pCol.affinity >= SQLITE_AFF_TEXT && (db.sel(pS2).pNext.nn() || pS2 != pSelect) {
            pS2 = db.sel(pS2).pNext;
            while pS2.nn() {
                let e = db.el(db.sel(pS2).pEList).a[i as usize].pExpr;
                m |= sqlite3ExprDataType(db, e);
                pS2 = db.sel(pS2).pNext;
            }
            if pCol.affinity == SQLITE_AFF_TEXT && (m & 0x01) != 0 {
                pCol.affinity = SQLITE_AFF_BLOB;
            } else if pCol.affinity >= SQLITE_AFF_NUMERIC && (m & 0x02) != 0 {
                pCol.affinity = SQLITE_AFF_BLOB;
            }
            if pCol.affinity >= SQLITE_AFF_NUMERIC && db.ex(p).op == TK_CAST {
                pCol.affinity = SQLITE_AFF_FLEXNUM;
            }
        }
        let mut zType: Option<ZStr> = columnTypeImpl(db, &sNC, p).0;
        let mismatch = match &zType {
            None => true,
            Some(z) => pCol.affinity != sqlite3AffinityType(Some(z), None),
        };
        if mismatch {
            if pCol.affinity == SQLITE_AFF_NUMERIC || pCol.affinity == SQLITE_AFF_FLEXNUM {
                zType = Some(b"NUM".to_vec());
            } else {
                zType = None;
                for j in 1..SQLITE_N_STDTYPE {
                    if crate::global::sqlite3StdTypeAffinity[j] == pCol.affinity {
                        zType = Some(crate::global::sqlite3StdType[j].to_vec());
                        break;
                    }
                }
            }
        }
        if let Some(z) = zType {
            pCol.colFlags &= !(COLFLAG_HASTYPE | COLFLAG_HASCOLL);
            pCol.zCnColl = None;
            pCol.zCnType = Some(z);
            pCol.colFlags |= COLFLAG_HASTYPE;
        }
        let pColl = sqlite3ExprCollSeq(db, p);
        if let Some(c) = pColl {
            debug_assert!(pTab.borrow().pIndex.is_empty());
            let zn = c.zName.clone();
            sqlite3ColumnSetColl(db, &mut pCol, Some(&zn));
        }
        pTab.borrow_mut().aCol[i as usize] = pCol;
    }
    pTab.borrow_mut().szTabRow = 1; /* Any non-zero value works */
}

/*
** Check the current subquery nesting depth.  Return true and set an
** error if it has gone too deep.
*/
fn checkSubqueryNestingDepth(db: &mut Sqlite3) -> i32 {
    if db.ppr().nNestSel >= db.aLimit[SQLITE_LIMIT_EXPR_DEPTH as usize] {
        errmsg!(db, b"VIEWs and/or subqueries nested too deep");
        return 1;
    }
    0
}

/*
** Given a SELECT statement, generate a Table structure that describes
** the result set of that SELECT.
*/
pub fn sqlite3ResultSetOfSelect(
    db: &mut Sqlite3,
    mut pSelect: SelectId,
    aff: u8,
) -> Option<TableRef> {
    db.pp().nNestSel += 1;
    if checkSubqueryNestingDepth(db) != 0 {
        return None;
    }
    let savedFlags = db.flags;
    db.flags &= !SQLITE_FullColNames;
    db.flags |= SQLITE_ShortColNames;
    sqlite3SelectPrep(db, pSelect, NcId::NULL);
    db.flags = savedFlags;
    if db.ppr().nErr != 0 {
        return None;
    }
    while db.sel(pSelect).pPrior.nn() {
        pSelect = db.sel(pSelect).pPrior;
    }
    let pTab = newTable();
    {
        let mut t = pTab.borrow_mut();
        t.nTabRef = 1;
        t.zName = Vec::new();
        t.nRowLogEst = 200;
    }
    let pEList = db.sel(pSelect).pEList;
    let mut nCol: i16 = 0;
    let mut aCol: Vec<Column> = Vec::new();
    sqlite3ColumnsFromExprList(db, pEList, &mut nCol, &mut aCol);
    {
        let mut t = pTab.borrow_mut();
        t.nCol = nCol;
        t.aCol = aCol;
    }
    sqlite3SubqueryColumnTypes(db, &pTab, pSelect, aff);
    pTab.borrow_mut().iPKey = -1;
    if db.mallocFailed != 0 {
        sqlite3DeleteTable(db, &pTab);
        return None;
    }
    db.pp().nNestSel -= 1;
    debug_assert!(db.ppr().nNestSel >= 0);
    Some(pTab)
}

/*
** Get a VDBE for the given parser context.  Create a new one if necessary.
** If an error occurs, return NULL and leave a message in pParse.
**
** Rust: returns false where C would return NULL; the Vdbe is
** `db.v()`.  sqlite3VdbeCreate() returns the new Vdbe, which is stored
** in the current Parse here (unless sqlite3VdbeCreate() already did).
*/
pub fn sqlite3GetVdbe(db: &mut Sqlite3) -> bool {
    if db.ppr().pVdbe.is_some() {
        return true;
    }
    let idx = db.parses.len() - 1;
    if db.ppr().iToplevel == idx {
        db.pp().okConstFactor = 1;
    }
    let v = sqlite3VdbeCreate(db);
    if db.ppr().pVdbe.is_none() {
        db.pp().pVdbe = Some(v);
    }
    db.ppr().pVdbe.is_some()
}

/*
** Compute the iLimit and iOffset fields of the SELECT based on the
** pLimit expressions.  pLimit->pLeft and pLimit->pRight hold the expressions
** that appear in the original SQL statement after the LIMIT and OFFSET
** keywords.  Or NULL if those keywords are omitted. iLimit and iOffset
** are the integer memory register numbers for counters used to compute
** the limit and offset.  If there is no limit and/or offset, then
** iLimit and iOffset are negative.
**
** This routine changes the values of iLimit and iOffset only if
** a limit or offset is defined by pLimit->pLeft and pLimit->pRight.  iLimit
** and iOffset should have been preset to appropriate default values (zero)
** prior to calling this routine.
**
** The iOffset register (if it exists) is initialized to the value
** of the OFFSET.  The iLimit register is initialized to LIMIT.  Register
** iOffset+1 is initialized to LIMIT+OFFSET.
**
** Only if pLimit->pLeft!=0 do the limit registers get
** redefined.  The UNION ALL operator uses this property to force
** the reuse of the same limit and offset registers across multiple
** SELECT statements.
*/
fn computeLimitRegisters(db: &mut Sqlite3, p: SelectId, iBreak: i32) {
    let pLimit = db.sel(p).pLimit;

    if db.sel(p).iLimit != 0 {
        return;
    }

    /*
     ** "LIMIT -1" always shows all rows.  There is some
     ** controversy about what the correct behavior should be.
     ** The current implementation interprets "LIMIT 0" to mean
     ** no rows.
     */
    if pLimit.nn() {
        let (lop, pLeft, pRight) = {
            let e = db.ex(pLimit);
            (e.op, e.pLeft, e.pRight)
        };
        debug_assert!(lop == TK_LIMIT);
        debug_assert!(pLeft.nn());
        db.pp().nMem += 1;
        let iLimit = db.ppr().nMem;
        db.selm(p).iLimit = iLimit;
        sqlite3GetVdbe(db);
        let mut n = 0;
        if sqlite3ExprIsInteger(db, pLeft, &mut n, true, 0) != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_Integer, n, iLimit);
            if n == 0 {
                sqlite3VdbeGoto(db.v(), iBreak);
            } else if n >= 0 && db.sel(p).nSelectRow > crate::util::sqlite3LogEst(n as u64) {
                let s = db.selm(p);
                s.nSelectRow = crate::util::sqlite3LogEst(n as u64);
                s.selFlags |= SF_FixedLimit;
            }
        } else {
            sqlite3ExprCode(db, pLeft, iLimit);
            sqlite3VdbeAddOp1(db.v(), OP_MustBeInt, iLimit);
            sqlite3MayAbort(db);
            sqlite3VdbeAddOp2(db.v(), OP_IfNot, iLimit, iBreak);
        }
        if pRight.nn() {
            db.pp().nMem += 1;
            let iOffset = db.ppr().nMem;
            db.selm(p).iOffset = iOffset;
            db.pp().nMem += 1; /* Allocate an extra register for limit+offset */
            sqlite3ExprCode(db, pRight, iOffset);
            sqlite3VdbeAddOp1(db.v(), OP_MustBeInt, iOffset);
            sqlite3MayAbort(db);
            sqlite3VdbeAddOp3(db.v(), OP_OffsetLimit, iLimit, iOffset + 1, iOffset);
        }
    }
}

/*
** Return the appropriate collating sequence for the iCol-th column of
** the result set for the compound-select statement "p".  Return NULL if
** the column has no default collating sequence.
**
** The collating sequence for the compound select is taken from the
** left-most term of the select that has a collating sequence.
*/
fn multiSelectCollSeq(db: &mut Sqlite3, p: SelectId, iCol: i32) -> Option<CollSeqRef> {
    let mut pRet;
    let pPrior = db.sel(p).pPrior;
    if pPrior.nn() {
        pRet = multiSelectCollSeq(db, pPrior, iCol);
    } else {
        pRet = None;
    }
    debug_assert!(iCol >= 0);
    /* iCol must be less than p->pEList->nExpr.  Otherwise an error would
     ** have been thrown during name resolution and we would not have gotten
     ** this far */
    let pEList = db.sel(p).pEList;
    if pRet.is_none() && iCol < db.el(pEList).nExpr() {
        let e = db.el(pEList).a[iCol as usize].pExpr;
        pRet = sqlite3ExprCollSeq(db, e);
    }
    pRet
}

/*
** The select statement passed as the second parameter is a compound SELECT
** with an ORDER BY clause. This function allocates and returns a KeyInfo
** structure suitable for implementing the ORDER BY.
**
** Space to hold the KeyInfo structure is obtained from malloc. The calling
** function is responsible for ensuring that this structure is eventually
** freed.
*/
fn multiSelectByMergeKeyInfo(db: &mut Sqlite3, p: SelectId, nExtra: i32) -> Option<KeyInfoRef> {
    let pOrderBy = db.sel(p).pOrderBy;
    let nOrderBy = if pOrderBy.nn() {
        db.el(pOrderBy).nExpr()
    } else {
        0
    };
    let pRet = sqlite3KeyInfoAlloc(db, nOrderBy + nExtra, 1);
    if let Some(ki) = &pRet {
        for i in 0..nOrderBy {
            let (pTerm, iOrderByCol) = {
                let it = &db.el(pOrderBy).a[i as usize];
                (it.pExpr, it.u.x.iOrderByCol)
            };
            let pColl;
            if (db.ex(pTerm).flags & EP_Collate) != 0 {
                pColl = sqlite3ExprCollSeq(db, pTerm);
            } else {
                let mut c = multiSelectCollSeq(db, p, iOrderByCol as i32 - 1);
                if c.is_none() {
                    c = db.pDfltColl.clone();
                }
                pColl = c;
                let zName = pColl.as_ref().map(|c| c.zName.clone());
                let ne = sqlite3ExprAddCollateString(db, pTerm, zName.as_deref());
                db.elm(pOrderBy).a[i as usize].pExpr = ne;
            }
            debug_assert!(sqlite3KeyInfoIsWriteable(ki) != 0);
            let sf = db.el(pOrderBy).a[i as usize].fg.sortFlags;
            let mut k = ki.borrow_mut();
            k.aColl[i as usize] = pColl;
            k.aSortFlags[i as usize] = sf;
        }
    }

    pRet
}

/*
** This routine generates VDBE code to compute the content of a WITH RECURSIVE
** query of the form:
**
**   <recursive-table> AS (<setup-query> UNION [ALL] <recursive-query>)
**                         \___________/             \_______________/
**                           p->pPrior                      p
**
**
** There is exactly one reference to the recursive-table in the FROM clause
** of recursive-query, marked with the SrcList->a[].fg.isRecursive flag.
**
** The setup-query runs once to generate an initial set of rows that go
** into a Queue table.  Rows are extracted from the Queue table one by
** one.  Each row extracted from Queue is output to pDest.  Then the single
** extracted row (now in the iCurrent table) becomes the content of the
** recursive-table for a recursive-query run.  The output of the recursive-query
** is added back into the Queue table.  Then another row is extracted from Queue
** and the iteration continues until the Queue table is empty.
**
** If the compound query operator is UNION then no duplicate rows are ever
** inserted into the Queue table.  The iDistinct table keeps a copy of all rows
** that have ever been inserted into Queue and causes duplicates to be
** discarded.  If the operator is UNION ALL, then duplicates are allowed.
**
** If the query has an ORDER BY, then entries in the Queue table are kept in
** ORDER BY order and the first entry is extracted for each cycle.  Without
** an ORDER BY, the Queue table is just a FIFO.
**
** If a LIMIT clause is provided, then the iteration stops after LIMIT rows
** have been output to pDest.  A LIMIT of zero means to output no rows and a
** negative LIMIT means to output all rows.  If there is also an OFFSET clause
** with a positive value, then the first OFFSET outputs are discarded rather
** than being sent to pDest.  The LIMIT count does not begin until after OFFSET
** rows have been skipped.
*/
fn generateWithRecursiveQuery(db: &mut Sqlite3, p: SelectId, pDest: &mut SelectDest) {
    let pSrc = db.sel(p).pSrc; /* The FROM clause of the recursive query */
    let mut nCol = db.el(db.sel(p).pEList).nExpr(); /* Number of columns in the recursive table */
    let mut iCurrent = 0; /* The Current table */
    let mut iDistinct = 0; /* To ensure unique results if UNION */
    let eDest; /* How to write to Queue */

    if db.sel(p).pWin.nn() {
        errmsg!(db, b"cannot use window functions in recursive queries");
        return;
    }

    /* Obtain authorization to do a recursive query */
    if crate::auth::sqlite3AuthCheck(db, SQLITE_RECURSIVE, None, None, None) != 0 {
        return;
    }

    /* Process the LIMIT and OFFSET clauses, if they exist */
    let addrBreak = sqlite3VdbeMakeLabel(db);
    db.selm(p).nSelectRow = 320; /* 4 billion rows */
    computeLimitRegisters(db, p, addrBreak);
    let (pLimit, regLimit, regOffset, pOrderBy) = {
        let s = db.selm(p);
        let r = (s.pLimit, s.iLimit, s.iOffset, s.pOrderBy);
        s.pLimit = ExprId::NULL;
        s.iLimit = 0;
        s.iOffset = 0;
        r
    };

    /* Locate the cursor number of the Current table */
    let nSrc = db.src(pSrc).nSrc();
    for i in 0..nSrc {
        let it = &db.src(pSrc).a[i as usize];
        if it.fg.isRecursive != 0 {
            iCurrent = it.iCursor;
            break;
        }
    }

    /* Allocate cursors numbers for Queue and Distinct.  The cursor number for
     ** the Distinct table must be exactly one greater than Queue in order
     ** for the SRT_DistFifo and SRT_DistQueue destinations to work. */
    let iQueue = db.ppr().nTab;
    db.pp().nTab += 1;
    if db.sel(p).op == TK_UNION {
        eDest = if pOrderBy.nn() {
            SRT_DistQueue
        } else {
            SRT_DistFifo
        };
        iDistinct = db.ppr().nTab;
        db.pp().nTab += 1;
    } else {
        eDest = if pOrderBy.nn() { SRT_Queue } else { SRT_Fifo };
    }
    let mut destQueue = selectDest(eDest, iQueue);

    /* Allocate cursors for Current, Queue, and Distinct. */
    db.pp().nMem += 1;
    let regCurrent = db.ppr().nMem;
    sqlite3VdbeAddOp3(db.v(), OP_OpenPseudo, iCurrent, regCurrent, nCol);
    if pOrderBy.nn() {
        let pKeyInfo = multiSelectByMergeKeyInfo(db, p, 1);
        let n = db.el(pOrderBy).nExpr();
        sqlite3VdbeAddOp4(
            db.v(),
            OP_OpenEphemeral,
            iQueue,
            n + 2,
            0,
            p4key(pKeyInfo),
            P4_KEYINFO as i32,
        );
        destQueue.pOrderBy = pOrderBy;
    } else {
        sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, iQueue, nCol);
    }
    if iDistinct != 0 {
        /* Generate an ephemeral table used to enforce distinctness on the
         ** output of the recursive part of the CTE.
         */
        debug_assert!(db.sel(p).pNext.is_null());
        debug_assert!(db.sel(p).pEList.nn());
        nCol = db.el(db.sel(p).pEList).nExpr();
        let pKeyInfo = sqlite3KeyInfoAlloc(db, nCol, 1);
        if let Some(ki) = pKeyInfo {
            for i in 0..nCol {
                let mut c = multiSelectCollSeq(db, p, i);
                if c.is_none() {
                    c = db.pDfltColl.clone();
                }
                ki.borrow_mut().aColl[i as usize] = c;
            }
            sqlite3VdbeAddOp4(
                db.v(),
                OP_OpenEphemeral,
                iDistinct,
                nCol,
                0,
                P4::KeyInfo(ki),
                P4_KEYINFO as i32,
            );
        } else {
            debug_assert!(db.ppr().nErr > 0);
        }
    }

    /* Detach the ORDER BY clause from the compound SELECT */
    db.selm(p).pOrderBy = ExprListId::NULL;

    'end_of_recursive_query: {
        /* Figure out how many elements of the compound SELECT are part of the
         ** recursive query.  Make sure no recursive elements use aggregate
         ** functions.  Mark the recursive elements as UNION ALL even if they
         ** are really UNION because the distinctness will be enforced by the
         ** iDistinct table.  pFirstRec is left pointing to the left-most
         ** recursive term of the CTE.
         */
        let mut pFirstRec = p;
        while pFirstRec.nn() {
            if (db.sel(pFirstRec).selFlags & SF_Aggregate) != 0 {
                errmsg!(db, b"recursive aggregate queries not supported");
                break 'end_of_recursive_query;
            }
            db.selm(pFirstRec).op = TK_ALL;
            let pr = db.sel(pFirstRec).pPrior;
            if (db.sel(pr).selFlags & SF_Recursive) == 0 {
                break;
            }
            pFirstRec = pr;
        }

        /* Store the results of the setup-query in Queue. */
        let pSetup = db.sel(pFirstRec).pPrior;
        db.selm(pSetup).pNext = SelectId::NULL;
        explainQP(db, 1, b"SETUP", &[]);
        let rc = sqlite3Select(db, pSetup, &mut destQueue);
        db.selm(pSetup).pNext = p;
        if rc != 0 {
            break 'end_of_recursive_query;
        }

        /* Find the next row in the Queue and output that row */
        let v = db.v();
        let addrTop = sqlite3VdbeAddOp2(v, OP_Rewind, iQueue, addrBreak);

        /* Transfer the next row in Queue over to Current */
        sqlite3VdbeAddOp1(v, OP_NullRow, iCurrent); /* To reset column cache */
        if pOrderBy.nn() {
            let n = db.el(pOrderBy).nExpr();
            sqlite3VdbeAddOp3(db.v(), OP_Column, iQueue, n + 1, regCurrent);
        } else {
            sqlite3VdbeAddOp2(db.v(), OP_RowData, iQueue, regCurrent);
        }
        sqlite3VdbeAddOp1(db.v(), OP_Delete, iQueue);

        /* Output the single row in Current */
        let addrCont = sqlite3VdbeMakeLabel(db);
        codeOffset(db.v(), regOffset, addrCont);
        selectInnerLoop(db, p, iCurrent, None, None, pDest, addrCont, addrBreak);
        if regLimit != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_DecrJumpZero, regLimit, addrBreak);
        }
        sqlite3VdbeResolveLabel(db.v(), addrCont);

        /* Execute the recursive SELECT taking the single row in Current as
         ** the value for the recursive-table. Store the results in the Queue.
         */
        db.selm(pFirstRec).pPrior = SelectId::NULL;
        explainQP(db, 1, b"RECURSIVE STEP", &[]);
        sqlite3Select(db, p, &mut destQueue);
        debug_assert!(db.sel(pFirstRec).pPrior.is_null());
        db.selm(pFirstRec).pPrior = pSetup;

        /* Keep running the loop until the Queue is empty */
        sqlite3VdbeGoto(db.v(), addrTop);
        sqlite3VdbeResolveLabel(db.v(), addrBreak);
    }

    /* end_of_recursive_query: */
    let ob = db.sel(p).pOrderBy;
    sqlite3ExprListDelete(db, ob);
    let s = db.selm(p);
    s.pOrderBy = pOrderBy;
    s.pLimit = pLimit;
}

/*
** Handle the special case of a compound-select that originates from a
** VALUES clause.  By handling this as a special case, we avoid deep
** recursion, and thus do not need to enforce the SQLITE_LIMIT_COMPOUND_SELECT
** on a VALUES clause.
**
** Because the Select object originates from a VALUES clause:
**   (1) There is no LIMIT or OFFSET or else there is a LIMIT of exactly 1
**   (2) All terms are UNION ALL
**   (3) There is no ORDER BY clause
**
** The "LIMIT of exactly 1" case of condition (1) comes about when a VALUES
** clause occurs within scalar expression (ex: "SELECT (VALUES(1),(2),(3))").
** The sqlite3CodeSubselect will have added the LIMIT 1 clause in tht case.
** Since the limit is exactly 1, we only need to evaluate the left-most VALUES.
*/
fn multiSelectValues(db: &mut Sqlite3, mut p: SelectId, pDest: &mut SelectDest) -> i32 {
    let mut nRow = 1;
    let rc = 0;
    let bShowAll = db.sel(p).pLimit.is_null() as i32;
    debug_assert!((db.sel(p).selFlags & SF_MultiValue) != 0);
    loop {
        debug_assert!((db.sel(p).selFlags & SF_Values) != 0);
        if db.sel(p).pWin.nn() {
            return -1;
        }
        if db.sel(p).pPrior.is_null() {
            break;
        }
        debug_assert!(db.sel(db.sel(p).pPrior).pNext == p);
        p = db.sel(p).pPrior;
        nRow += bShowAll;
    }
    let zS: &[u8] = if nRow == 1 { b"" } else { b"S" };
    explainQP(
        db,
        0,
        b"SCAN %d CONSTANT ROW%s",
        &[PfArg::from(nRow), PfArg::from(zS)],
    );
    while p.nn() {
        selectInnerLoop(db, p, -1, None, None, pDest, 1, 1);
        if bShowAll == 0 {
            break;
        }
        db.selm(p).nSelectRow = nRow as LogEst;
        p = db.sel(p).pNext;
    }
    rc
}

/*
** Return true if the SELECT statement which is known to be the recursive
** part of a recursive CTE still has its anchor terms attached.  If the
** anchor terms have already been removed, then return false.
*/
fn hasAnchor(db: &Sqlite3, mut p: SelectId) -> bool {
    while p.nn() && (db.sel(p).selFlags & SF_Recursive) != 0 {
        p = db.sel(p).pPrior;
    }
    p.nn()
}

/*
** Return TRUE if p is a UNION with a LIMIT of exactly 1 and no OFFSET
** clause.  This is a precondition for a couple of related optimizations.
**
** False negatives are harmless (apart from resulting in a slower query).
** False positives can result in incorrect answers, however.  To provoke
** false negatives for testing purposes, disable the SQLITE_UnionLimit
** optimization.
*/
fn unionWithLimitOne(db: &mut Sqlite3, p: SelectId) -> bool {
    let (op, pLimit) = {
        let s = db.sel(p);
        (s.op, s.pLimit)
    };
    if op != TK_UNION {
        return false;
    }
    if pLimit.is_null() {
        return false;
    }
    let (lRight, lLeft) = {
        let l = db.ex(pLimit);
        (l.pRight, l.pLeft)
    };
    if lRight.nn() {
        return false; /* No OFFSET */
    }
    let mut v = 0;
    debug_assert!(lLeft.nn());
    if sqlite3ExprIsInteger(db, lLeft, &mut v, false, 0) == 0 {
        return false;
    }
    if v != 1 {
        return false;
    }
    if db.OptimizationDisabled(SQLITE_UnionLimit) {
        return false;
    }
    true
}

/*
** This routine is called to process a compound query formed from
** two or more separate queries using UNION, UNION ALL, EXCEPT, or
** INTERSECT
**
** "p" points to the right-most of the two queries.  The query on the
** left is p->pPrior.  The left query could also be a compound query
** in which case this routine will be called recursively.
**
** The results of the total query are to be written into a destination
** of type eDest with parameter iParm.
**
** Example 1:  Consider a three-way compound SQL statement.
**
**     SELECT a FROM t1 UNION SELECT b FROM t2 UNION SELECT c FROM t3
**
** This statement is parsed up as follows:
**
**     SELECT c FROM t3
**      |
**      `----->  SELECT b FROM t2
**                |
**                `------>  SELECT a FROM t1
**
** The arrows in the diagram above represent the Select.pPrior pointer.
** So if this routine is called with p equal to the t3 query, then
** pPrior will be the t2 query.  p->op will be TK_UNION in this case.
**
** Notice that because of the way SQLite parses compound SELECTs, the
** individual selects always group from left to right.
*/
fn multiSelect(db: &mut Sqlite3, p: SelectId, pDest: &mut SelectDest) -> i32 {
    let mut rc = SQLITE_OK;

    /* Make sure there is no ORDER BY or LIMIT clause on prior SELECTs.  Only
     ** the last (right-most) SELECT in the series may have an ORDER BY or LIMIT.
     */
    debug_assert!(p.nn() && db.sel(p).pPrior.nn());
    debug_assert!(
        (db.sel(p).selFlags & SF_Recursive) == 0
            || db.sel(p).op == TK_ALL
            || db.sel(p).op == TK_UNION
    );
    debug_assert!((db.sel(p).selFlags & SF_Compound) != 0);
    let pPrior = db.sel(p).pPrior;
    let mut dest = pDest.clone();
    debug_assert!(db.sel(pPrior).pOrderBy.is_null());
    debug_assert!(db.sel(pPrior).pLimit.is_null());

    sqlite3GetVdbe(db);

    'multi_select_end: {
        /* Create the destination temporary table if necessary
         */
        if dest.eDest == SRT_EphemTab {
            let n = db.el(db.sel(p).pEList).nExpr();
            sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, dest.iSDParm, n);
            dest.eDest = SRT_Table;
        }

        /* Special handling for a compound-select that originates as a VALUES clause.
         */
        if (db.sel(p).selFlags & SF_MultiValue) != 0 {
            rc = multiSelectValues(db, p, &mut dest);
            if rc >= 0 {
                break 'multi_select_end;
            }
            rc = SQLITE_OK;
        }

        /* Make sure all SELECTs in the statement have the same number of elements
         ** in their result sets.
         */
        debug_assert!(db.sel(p).pEList.nn() && db.sel(pPrior).pEList.nn());

        if (db.sel(p).selFlags & SF_Recursive) != 0 && hasAnchor(db, p) {
            generateWithRecursiveQuery(db, p, &mut dest);
        } else if db.sel(p).pOrderBy.nn() {
            /* If the compound has an ORDER BY clause, then always use the merge
             ** algorithm. */
            return multiSelectByMerge(db, p, pDest);
        } else if db.sel(p).op != TK_ALL && !unionWithLimitOne(db, p) {
            /* If the compound is EXCEPT, INTERSECT, or UNION (anything other than
             ** UNION ALL) then also always use the merge algorithm.  However, the
             ** multiSelectByMerge() routine requires that the compound have an
             ** ORDER BY clause, and it doesn't right now.  So invent one first. */
            let pOne = sqlite3ExprInt32(db, 1);
            let ob = sqlite3ExprListAppend(db, ExprListId::NULL, pOne);
            db.selm(p).pOrderBy = ob;
            if db.ppr().nErr != 0 {
                break 'multi_select_end;
            }
            debug_assert!(ob.nn());
            db.elm(ob).a[0].u.x.iOrderByCol = 1;
            return multiSelectByMerge(db, p, pDest);
        } else {
            /* For a UNION ALL compound without ORDER BY, or for a UNION with a
             ** LIMIT of exactly 1 and no OFFSET and no ORDER BY, simply run the left
             ** query, then run the right query */
            let mut addr = 0;
            let mut nLimit = 0;

            if db.sel(pPrior).pPrior.is_null() {
                explainQP(db, 1, b"COMPOUND QUERY", &[]);
                explainQP(db, 1, b"LEFT-MOST SUBQUERY", &[]);
            }
            debug_assert!(db.sel(pPrior).pLimit.is_null());
            let (iLimit, iOffset, pLimit) = {
                let s = db.sel(p);
                (s.iLimit, s.iOffset, s.pLimit)
            };
            let dupLimit = sqlite3ExprDup(db, pLimit, 0);
            {
                let s = db.selm(pPrior);
                s.iLimit = iLimit;
                s.iOffset = iOffset;
                s.pLimit = dupLimit;
            }
            rc = sqlite3Select(db, pPrior, &mut dest);
            let pl = db.sel(pPrior).pLimit;
            sqlite3ExprDelete(db, pl);
            db.selm(pPrior).pLimit = ExprId::NULL;
            if rc != 0 {
                break 'multi_select_end;
            }
            let (pli, pof) = {
                let s = db.sel(pPrior);
                (s.iLimit, s.iOffset)
            };
            {
                let s = db.selm(p);
                s.pPrior = SelectId::NULL;
                s.iLimit = pli;
                s.iOffset = pof;
            }
            if pli != 0 {
                let v = db.v();
                addr = sqlite3VdbeAddOp1(v, OP_IfNot, pli);
                if pof != 0 {
                    sqlite3VdbeAddOp3(v, OP_OffsetLimit, pli, pof + 1, pof);
                }
            }
            explainQP(db, 1, b"UNION ALL", &[]);
            rc = sqlite3Select(db, p, &mut dest);
            db.selm(p).pPrior = pPrior;
            let nsr =
                crate::util::sqlite3LogEstAdd(db.sel(p).nSelectRow, db.sel(pPrior).nSelectRow);
            db.selm(p).nSelectRow = nsr;
            let pLimit = db.sel(p).pLimit;
            if pLimit.nn() {
                let pl = db.ex(pLimit).pLeft;
                if sqlite3ExprIsInteger(db, pl, &mut nLimit, true, 0) != 0
                    && nLimit > 0
                    && db.sel(p).nSelectRow > crate::util::sqlite3LogEst(nLimit as u64)
                {
                    db.selm(p).nSelectRow = crate::util::sqlite3LogEst(nLimit as u64);
                }
            }
            if addr != 0 {
                sqlite3VdbeJumpHere(db.v(), addr);
            }
            if db.sel(p).pNext.is_null() {
                sqlite3VdbeExplainPop(db);
            }
        }
    }

    /* multi_select_end: */
    pDest.iSdst = dest.iSdst;
    pDest.nSdst = dest.nSdst;
    pDest.iSDParm2 = dest.iSDParm2;
    /* pDelete (the right-hand arms detached above) is freed with the arena */
    rc
}

/*
** Error message for when two or more terms of a compound select have different
** size result sets.
*/
pub fn sqlite3SelectWrongNumTermsError(db: &mut Sqlite3, p: SelectId) {
    if (db.sel(p).selFlags & SF_Values) != 0 {
        errmsg!(db, b"all VALUES must have the same number of terms");
    } else {
        let z = selectOpName(db.sel(p).op as i32);
        errmsg!(
            db,
            b"SELECTs to the left and right of %s do not have the same number of result columns",
            z
        );
    }
}

/*
** Code an output subroutine for a coroutine implementation of a
** SELECT statement.
**
** The data to be output is contained in an array of pIn->nSdst registers
** starting at register pIn->iSdst.  pDest is where the output should
** be sent.
**
** regReturn is the number of the register holding the subroutine
** return address.
**
** If regPrev>0 then it is the first register in a vector that
** records the previous output.  mem[regPrev] is a flag that is false
** if there has been no previous output.  If regPrev>0 then code is
** generated to suppress duplicates.  pKeyInfo is used for comparing
** keys.
**
** If the LIMIT found in p->iLimit is reached, jump immediately to
** iBreak.
*/
fn generateOutputSubroutine(
    db: &mut Sqlite3,
    p: SelectId,
    pIn: &SelectDest,
    pDest: &mut SelectDest,
    regReturn: i32,
    regPrev: i32,
    pKeyInfo: &Option<KeyInfoRef>,
    iBreak: i32,
) -> i32 {
    debug_assert!(pIn.eDest == SRT_Coroutine);

    let addr = sqlite3VdbeCurrentAddr(db.v());
    let iContinue = sqlite3VdbeMakeLabel(db);

    /* Suppress duplicates for UNION, EXCEPT, and INTERSECT
     */
    if regPrev != 0 {
        let v = db.v();
        let addr1 = sqlite3VdbeAddOp1(v, OP_IfNot, regPrev);
        let k = match pKeyInfo {
            Some(k) => sqlite3KeyInfoRef(k),
            None => None,
        };
        let addr2 = sqlite3VdbeAddOp4(
            v,
            OP_Compare,
            pIn.iSdst,
            regPrev + 1,
            pIn.nSdst,
            p4key(k),
            P4_KEYINFO as i32,
        );
        sqlite3VdbeAddOp3(v, OP_Jump, addr2 + 2, iContinue, addr2 + 2);
        sqlite3VdbeJumpHere(v, addr1);
        sqlite3VdbeAddOp3(v, OP_Copy, pIn.iSdst, regPrev + 1, pIn.nSdst - 1);
        sqlite3VdbeAddOp2(v, OP_Integer, 1, regPrev);
    }
    if db.mallocFailed != 0 {
        return 0;
    }

    /* Suppress the first OFFSET entries if there is an OFFSET clause
     */
    let iOffset = db.sel(p).iOffset;
    codeOffset(db.v(), iOffset, iContinue);

    match pDest.eDest {
        /* Store the result as data using a unique key.
         */
        SRT_Fifo | SRT_DistFifo | SRT_Table | SRT_EphemTab => {
            let r1 = sqlite3GetTempReg(db);
            let r2 = sqlite3GetTempReg(db);
            let iParm = pDest.iSDParm;
            let v = db.v();
            sqlite3VdbeAddOp3(v, OP_MakeRecord, pIn.iSdst, pIn.nSdst, r1);
            if pDest.eDest == SRT_DistFifo {
                /* If the destination is DistFifo, then cursor (iParm+1) is open
                 ** on an ephemeral index that is used to enforce uniqueness on the
                 ** total result.  At this point, we are processing the setup portion
                 ** of the recursive CTE using the merge algorithm, so the results are
                 ** guaranteed to be unique anyhow.  But we still need to populate the
                 ** (iParm+1) cursor for use by the subsequent recursive phase.
                 */
                sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm + 1, r1, pIn.iSdst, pIn.nSdst);
            }
            sqlite3VdbeAddOp2(v, OP_NewRowid, iParm, r2);
            sqlite3VdbeAddOp3(v, OP_Insert, iParm, r1, r2);
            sqlite3VdbeChangeP5(v, OPFLAG_APPEND);
            sqlite3ReleaseTempReg(db, r2);
            sqlite3ReleaseTempReg(db, r1);
        }

        /* If any row exist in the result set, record that fact and abort.
         */
        SRT_Exists => {
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, pDest.iSDParm);
            /* The LIMIT clause will terminate the loop for us */
        }

        /* If we are creating a set for an "expr IN (SELECT ...)".
         */
        SRT_Set => {
            let r1 = sqlite3GetTempReg(db);
            let aff = pDest.zAffSdst.clone();
            let v = db.v();
            sqlite3VdbeAddOp4(
                v,
                OP_MakeRecord,
                pIn.iSdst,
                pIn.nSdst,
                r1,
                p4str(aff),
                pIn.nSdst,
            );
            sqlite3VdbeAddOp4Int(v, OP_IdxInsert, pDest.iSDParm, r1, pIn.iSdst, pIn.nSdst);
            if pDest.iSDParm2 > 0 {
                sqlite3VdbeAddOp4Int(v, OP_FilterAdd, pDest.iSDParm2, 0, pIn.iSdst, pIn.nSdst);
                explainQP(db, 0, b"CREATE BLOOM FILTER", &[]);
            }
            sqlite3ReleaseTempReg(db, r1);
        }

        /* If this is a scalar select that is part of an expression, then
         ** store the results in the appropriate memory cell and break out
         ** of the scan loop.  Note that the select might return multiple columns
         ** if it is the RHS of a row-value IN operator.
         */
        SRT_Mem => {
            sqlite3ExprCodeMove(db, pIn.iSdst, pDest.iSDParm, pIn.nSdst);
            /* The LIMIT clause will jump out of the loop for us */
        }

        /* The results are stored in a sequence of registers
         ** starting at pDest->iSdst.  Then the co-routine yields.
         */
        SRT_Coroutine => {
            if pDest.iSdst == 0 {
                pDest.iSdst = sqlite3GetTempRange(db, pIn.nSdst);
                pDest.nSdst = pIn.nSdst;
            }
            sqlite3ExprCodeMove(db, pIn.iSdst, pDest.iSdst, pIn.nSdst);
            sqlite3VdbeAddOp1(db.v(), OP_Yield, pDest.iSDParm);
        }

        /* Write the results into a priority queue that is order according to
         ** pDest->pOrderBy (in pSO).  pDest->iSDParm (in iParm) is the cursor for an
         ** index with pSO->nExpr+2 columns.  Build a key using pSO for the first
         ** pSO->nExpr columns, then make sure all keys are unique by adding a
         ** final OP_Sequence column.  The last column is the record as a blob.
         */
        SRT_DistQueue | SRT_Queue => {
            let iParm = pDest.iSDParm;
            let pSO = pDest.pOrderBy;
            debug_assert!(pSO.nn());
            let nKey = db.el(pSO).nExpr();
            let r1 = sqlite3GetTempReg(db);
            let r2 = sqlite3GetTempRange(db, nKey + 2);
            let r3 = r2 + nKey + 1;

            sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, pIn.iSdst, pIn.nSdst, r3);
            if pDest.eDest == SRT_DistQueue {
                sqlite3VdbeAddOp2(db.v(), OP_IdxInsert, iParm + 1, r3);
            }
            for ii in 0..nKey {
                let c = db.el(pSO).a[ii as usize].u.x.iOrderByCol as i32;
                sqlite3VdbeAddOp2(db.v(), OP_SCopy, pIn.iSdst + c - 1, r2 + ii);
            }
            let v = db.v();
            sqlite3VdbeAddOp2(v, OP_Sequence, iParm, r2 + nKey);
            sqlite3VdbeAddOp3(v, OP_MakeRecord, r2, nKey + 2, r1);
            sqlite3VdbeAddOp4Int(v, OP_IdxInsert, iParm, r1, r2, nKey + 2);
            sqlite3ReleaseTempReg(db, r1);
            sqlite3ReleaseTempRange(db, r2, nKey + 2);
        }

        /* Ignore the output */
        SRT_Discard => {}

        /* If none of the above, then the result destination must be
         ** SRT_Output.
         **
         ** For SRT_Output, results are stored in a sequence of registers.
         ** Then the OP_ResultRow opcode is used to cause sqlite3_step() to
         ** return the next row of result.
         */
        _ => {
            debug_assert!(pDest.eDest == SRT_Output);
            sqlite3VdbeAddOp2(db.v(), OP_ResultRow, pIn.iSdst, pIn.nSdst);
        }
    }

    /* Jump to the end of the loop if the LIMIT is reached.
     */
    let iLimit = db.sel(p).iLimit;
    let v = db.v();
    if iLimit != 0 {
        sqlite3VdbeAddOp2(v, OP_DecrJumpZero, iLimit, iBreak);
    }

    /* Generate the subroutine return
     */
    sqlite3VdbeResolveLabel(v, iContinue);
    sqlite3VdbeAddOp1(v, OP_Return, regReturn);

    addr
}

/*
** Generate code for a compound SELECT statement using a merge
** algorithm.  The compound must have an ORDER BY clause for this
** to work.
**
** We assume a query of the following form:
**
**      <selectA>  <operator>  <selectB>  ORDER BY <orderbylist>
**
** <operator> is one of UNION ALL, UNION, EXCEPT, or INTERSECT.  The idea
** is to code both <selectA> and <selectB> with the ORDER BY clause as
** co-routines.  Then run the co-routines in parallel and merge the results
** into the output.  In addition to the two coroutines (called selectA and
** selectB) there are 7 subroutines:
**
**    outA:    Move the output of the selectA coroutine into the output
**             of the compound query.
**
**    outB:    Move the output of the selectB coroutine into the output
**             of the compound query.  (Only generated for UNION and
**             UNION ALL.  EXCEPT and INTERSECT never output a row that
**             appears only in B.)
**
**    AltB:    Called when there is data from both coroutines and A<B.
**
**    AeqB:    Called when there is data from both coroutines and A==B.
**
**    AgtB:    Called when there is data from both coroutines and A>B.
**
**    EofA:    Called when data is exhausted from selectA.
**
**    EofB:    Called when data is exhausted from selectB.
**
** The implementation of the latter five subroutines depend on which
** <operator> is used:
**
**
**             UNION ALL         UNION            EXCEPT          INTERSECT
**          -------------  -----------------  --------------  -----------------
**   AltB:   outA, nextA      outA, nextA       outA, nextA         nextA
**
**   AeqB:   outA, nextA         nextA             nextA         outA, nextA
**
**   AgtB:   outB, nextB      outB, nextB          nextB            nextB
**
**   EofA:   outB, nextB      outB, nextB          halt             halt
**
**   EofB:   outA, nextA      outA, nextA       outA, nextA         halt
**
** In the AltB, AeqB, and AgtB subroutines, an EOF on A following nextA
** causes an immediate jump to EofA and an EOF on B following nextB causes
** an immediate jump to EofB.  Within EofA and EofB, and EOF on entry or
** following nextX causes a jump to the end of the select processing.
**
** Duplicate removal in the UNION, EXCEPT, and INTERSECT cases is handled
** within the output subroutine.  The regPrev register set holds the previously
** output value.  A comparison is made against this value and the output
** is skipped if the next results would be the same as the previous.
**
** The implementation plan is to implement the two coroutines and seven
** subroutines first, then put the control logic at the bottom.  Like this:
**
**          goto Init
**     coA: coroutine for left query (A)
**     coB: coroutine for right query (B)
**    outA: output one row of A
**    outB: output one row of B (UNION and UNION ALL only)
**    EofA: ...
**    EofB: ...
**    AltB: ...
**    AeqB: ...
**    AgtB: ...
**    Init: initialize coroutine registers
**          yield coA, on eof goto EofA
**          yield coB, on eof goto EofB
**    Cmpr: Compare A, B
**          Jump AltB, AeqB, AgtB
**     End: ...
**
** We call AltB, AeqB, AgtB, EofA, and EofB "subroutines" but they are not
** actually called using Gosub and they do not Return.  EofA and EofB loop
** until all data is exhausted then jump to the "end" label.  AltB, AeqB,
** and AgtB jump to either Cmpr or to one of EofA or EofB.
*/
fn multiSelectByMerge(db: &mut Sqlite3, p: SelectId, pDest: &mut SelectDest) -> i32 {
    let mut addrOutB = 0; /* Address of the output-B subroutine */
    let addrEofA; /* Address of the select-A-exhausted subroutine */
    let addrEofA_noB; /* Alternate addrEofA if B is uninitialized */
    let addrEofB; /* Address of the select-B-exhausted subroutine */
    let mut addrAltB; /* Address of the A<B subroutine */
    let addrAeqB; /* Address of the A==B subroutine */
    let mut addrAgtB; /* Address of the A>B subroutine */
    let regLimitA; /* Limit register for select-A */
    let regLimitB; /* Limit register for select-A */
    let regPrev; /* A range of registers to hold previous output */
    let mut pKeyDup: Option<KeyInfoRef> = None; /* Comparison information for duplicate removal */

    debug_assert!(db.sel(p).pOrderBy.nn());
    sqlite3GetVdbe(db);
    let labelEnd = sqlite3VdbeMakeLabel(db); /* Label for the end of the overall SELECT stmt */
    let labelCmpr = sqlite3VdbeMakeLabel(db); /* Label for the start of the merge algorithm */

    /* Patch up the ORDER BY clause
     */
    let op = db.sel(p).op; /* One of TK_ALL, TK_UNION, TK_EXCEPT, TK_INTERSECT */
    debug_assert!(db.sel(db.sel(p).pPrior).pOrderBy.is_null());
    let mut pOrderBy = db.sel(p).pOrderBy;
    debug_assert!(pOrderBy.nn());
    let mut nOrderBy = db.el(pOrderBy).nExpr();

    /* For operators other than UNION ALL we have to make sure that
     ** the ORDER BY clause covers every term of the result set.  Add
     ** terms to the ORDER BY clause as necessary.
     */
    if op != TK_ALL {
        let mut i = 1;
        while db.mallocFailed == 0 && i <= db.el(db.sel(p).pEList).nExpr() {
            let mut j = 0;
            while j < nOrderBy {
                let c = db.el(pOrderBy).a[j as usize].u.x.iOrderByCol as i32;
                debug_assert!(c > 0);
                if c == i {
                    break;
                }
                j += 1;
            }
            if j == nOrderBy {
                let pNew = sqlite3ExprInt32(db, i);
                if pNew.is_null() {
                    return SQLITE_NOMEM;
                }
                pOrderBy = sqlite3ExprListAppend(db, pOrderBy, pNew);
                db.selm(p).pOrderBy = pOrderBy;
                if pOrderBy.nn() {
                    db.elm(pOrderBy).a[nOrderBy as usize].u.x.iOrderByCol = i as u16;
                    nOrderBy += 1;
                }
            }
            i += 1;
        }
    }

    /* Compute the comparison permutation and keyinfo that is used with
     ** the permutation to determine if the next row of results comes
     ** from selectA or selectB.  Also add literal collations to the
     ** ORDER BY clause terms so that when selectA and selectB are
     ** evaluated, they use the correct collation.
     */
    let mut aPermute: Option<Vec<u32>> = {
        let mut a = vec![0u32; (nOrderBy + 1) as usize];
        let mut bKeep = false;
        a[0] = nOrderBy as u32;
        for i in 1..=nOrderBy {
            let c = db.el(pOrderBy).a[(i - 1) as usize].u.x.iOrderByCol as u32;
            debug_assert!(c > 0);
            a[i as usize] = c - 1;
            if a[i as usize] != (i as u32) - 1 {
                bKeep = true;
            }
        }
        if bKeep { Some(a) } else { None }
    };
    let pKeyMerge = multiSelectByMergeKeyInfo(db, p, 1);

    /* Allocate a range of temporary registers and the KeyInfo needed
     ** for the logic that removes duplicate result rows when the
     ** operator is UNION, EXCEPT, or INTERSECT (but not UNION ALL).
     */
    if op == TK_ALL {
        regPrev = 0;
    } else {
        let nExpr = db.el(db.sel(p).pEList).nExpr();
        debug_assert!(nOrderBy >= nExpr || db.mallocFailed != 0);
        regPrev = db.ppr().nMem + 1;
        db.pp().nMem += nExpr + 1;
        sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regPrev);
        pKeyDup = sqlite3KeyInfoAlloc(db, nExpr, 1);
        if let Some(kd) = &pKeyDup {
            debug_assert!(sqlite3KeyInfoIsWriteable(kd) != 0);
            for i in 0..nExpr {
                let c = multiSelectCollSeq(db, p, i);
                let mut k = kd.borrow_mut();
                k.aColl[i as usize] = c;
                k.aSortFlags[i as usize] = 0;
            }
        }
    }

    /* Separate the left and the right query from one another
     */
    let mut nSelect = 1;
    let mut pSplit;
    if (op == TK_ALL || op == TK_UNION) && db.OptimizationEnabled(SQLITE_BalancedMerge) {
        pSplit = p;
        while db.sel(pSplit).pPrior.nn() && db.sel(pSplit).op == op {
            nSelect += 1;
            debug_assert!(db.sel(db.sel(pSplit).pPrior).pNext == pSplit);
            pSplit = db.sel(pSplit).pPrior;
        }
    }
    if nSelect <= 3 {
        pSplit = p;
    } else {
        pSplit = p;
        let mut i = 2;
        while i < nSelect {
            pSplit = db.sel(pSplit).pPrior;
            i += 2;
        }
    }
    let pPrior = db.sel(pSplit).pPrior;
    debug_assert!(pPrior.nn());
    db.selm(pSplit).pPrior = SelectId::NULL;
    db.selm(pPrior).pNext = SelectId::NULL;
    debug_assert!(db.sel(p).pOrderBy == pOrderBy);
    let ob2 = sqlite3ExprListDup(db, pOrderBy, 0);
    db.selm(pPrior).pOrderBy = ob2;
    let pob = db.sel(p).pOrderBy;
    crate::resolve::sqlite3ResolveOrderGroupBy(db, p, pob, Some(b"ORDER"));
    let pob = db.sel(pPrior).pOrderBy;
    crate::resolve::sqlite3ResolveOrderGroupBy(db, pPrior, pob, Some(b"ORDER"));

    /* Compute the limit registers */
    computeLimitRegisters(db, p, labelEnd);
    if db.sel(p).iLimit != 0 && (op == TK_ALL || unionWithLimitOne(db, p)) {
        db.pp().nMem += 1;
        regLimitA = db.ppr().nMem;
        db.pp().nMem += 1;
        regLimitB = db.ppr().nMem;
        let (iOffset, iLimit) = {
            let s = db.sel(p);
            (s.iOffset, s.iLimit)
        };
        let v = db.v();
        sqlite3VdbeAddOp2(
            v,
            OP_Copy,
            if iOffset != 0 { iOffset + 1 } else { iLimit },
            regLimitA,
        );
        sqlite3VdbeAddOp2(v, OP_Copy, regLimitA, regLimitB);
    } else {
        regLimitA = 0;
        regLimitB = 0;
    }
    let pl = db.sel(p).pLimit;
    sqlite3ExprDelete(db, pl);
    db.selm(p).pLimit = ExprId::NULL;

    db.pp().nMem += 1;
    let regAddrA = db.ppr().nMem;
    db.pp().nMem += 1;
    let regAddrB = db.ppr().nMem;
    db.pp().nMem += 1;
    let regOutA = db.ppr().nMem;
    db.pp().nMem += 1;
    let regOutB = db.ppr().nMem;
    let mut destA = selectDest(SRT_Coroutine, regAddrA);
    let mut destB = selectDest(SRT_Coroutine, regAddrB);

    let zOp = selectOpName(db.sel(p).op as i32);
    explainQP(db, 1, b"MERGE (%s)", &[PfArg::from(zOp)]);

    /* Generate a coroutine to evaluate the SELECT statement to the
     ** left of the compound operator - the "A" select.
     */
    let addrSelectA = sqlite3VdbeCurrentAddr(db.v()) + 1;
    let mut addr1 = sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regAddrA, 0, addrSelectA);
    db.selm(pPrior).iLimit = regLimitA;
    explainQP(db, 1, b"LEFT", &[]);
    sqlite3Select(db, pPrior, &mut destA);
    sqlite3VdbeEndCoroutine(db, regAddrA);
    sqlite3VdbeJumpHere(db.v(), addr1);

    /* Generate a coroutine to evaluate the SELECT statement on
     ** the right - the "B" select
     */
    let addrSelectB = sqlite3VdbeCurrentAddr(db.v()) + 1;
    addr1 = sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regAddrB, 0, addrSelectB);
    let (savedLimit, savedOffset) = {
        let s = db.sel(p);
        (s.iLimit, s.iOffset)
    };
    {
        let s = db.selm(p);
        s.iLimit = regLimitB;
        s.iOffset = 0;
    }
    explainQP(db, 1, b"RIGHT", &[]);
    sqlite3Select(db, p, &mut destB);
    {
        let s = db.selm(p);
        s.iLimit = savedLimit;
        s.iOffset = savedOffset;
    }
    sqlite3VdbeEndCoroutine(db, regAddrB);

    /* Generate a subroutine that outputs the current row of the A
     ** select as the next output row of the compound select.
     */
    let addrOutA =
        generateOutputSubroutine(db, p, &destA, pDest, regOutA, regPrev, &pKeyDup, labelEnd);

    /* Generate a subroutine that outputs the current row of the B
     ** select as the next output row of the compound select.
     */
    if op == TK_ALL || op == TK_UNION {
        addrOutB =
            generateOutputSubroutine(db, p, &destB, pDest, regOutB, regPrev, &pKeyDup, labelEnd);
    }
    drop(pKeyDup);

    /* Generate a subroutine to run when the results from select A
     ** are exhausted and only data in select B remains.
     */
    if op == TK_EXCEPT || op == TK_INTERSECT {
        addrEofA = labelEnd;
        addrEofA_noB = labelEnd;
    } else {
        let v = db.v();
        addrEofA = sqlite3VdbeAddOp2(v, OP_Gosub, regOutB, addrOutB);
        addrEofA_noB = sqlite3VdbeAddOp2(v, OP_Yield, regAddrB, labelEnd);
        sqlite3VdbeGoto(v, addrEofA);
        let n = crate::util::sqlite3LogEstAdd(db.sel(p).nSelectRow, db.sel(pPrior).nSelectRow);
        db.selm(p).nSelectRow = n;
    }

    /* Generate a subroutine to run when the results from select B
     ** are exhausted and only data in select A remains.
     */
    if op == TK_INTERSECT {
        addrEofB = addrEofA;
        let pn = db.sel(pPrior).nSelectRow;
        if db.sel(p).nSelectRow > pn {
            db.selm(p).nSelectRow = pn;
        }
    } else {
        let v = db.v();
        addrEofB = sqlite3VdbeAddOp2(v, OP_Gosub, regOutA, addrOutA);
        sqlite3VdbeAddOp2(v, OP_Yield, regAddrA, labelEnd);
        sqlite3VdbeGoto(v, addrEofB);
    }

    /* Generate code to handle the case of A<B
     */
    let v = db.v();
    addrAltB = sqlite3VdbeAddOp2(v, OP_Gosub, regOutA, addrOutA);
    sqlite3VdbeAddOp2(v, OP_Yield, regAddrA, addrEofA);
    sqlite3VdbeGoto(v, labelCmpr);

    /* Generate code to handle the case of A==B
     */
    if op == TK_ALL {
        addrAeqB = addrAltB;
    } else if op == TK_INTERSECT {
        addrAeqB = addrAltB;
        addrAltB += 1;
    } else {
        addrAeqB = addrAltB + 1;
    }

    /* Generate code to handle the case of A>B
     */
    addrAgtB = sqlite3VdbeCurrentAddr(v);
    if op == TK_ALL || op == TK_UNION {
        sqlite3VdbeAddOp2(v, OP_Gosub, regOutB, addrOutB);
        sqlite3VdbeAddOp2(v, OP_Yield, regAddrB, addrEofB);
        sqlite3VdbeGoto(v, labelCmpr);
    } else {
        addrAgtB += 1; /* Just do next-B.  Might as well use the next-B call
         ** in the next code block */
    }

    /* This code runs once to initialize everything.
     */
    sqlite3VdbeJumpHere(v, addr1);
    sqlite3VdbeAddOp2(v, OP_Yield, regAddrA, addrEofA_noB);
    /* v---  Also the A>B case for EXCEPT and INTERSECT */
    sqlite3VdbeAddOp2(v, OP_Yield, regAddrB, addrEofB);

    /* Implement the main merge loop
     */
    let hasPermute = aPermute.is_some();
    if let Some(a) = aPermute.take() {
        sqlite3VdbeAddOp4(
            v,
            OP_Permutation,
            0,
            0,
            0,
            P4::Ai(Rc::new(a)),
            P4_INTARRAY as i32,
        );
    }
    sqlite3VdbeResolveLabel(v, labelCmpr);
    sqlite3VdbeAddOp4(
        v,
        OP_Compare,
        destA.iSdst,
        destB.iSdst,
        nOrderBy,
        p4key(pKeyMerge),
        P4_KEYINFO as i32,
    );
    if hasPermute {
        sqlite3VdbeChangeP5(v, OPFLAG_PERMUTE);
    }
    sqlite3VdbeAddOp3(v, OP_Jump, addrAltB, addrAeqB, addrAgtB);

    /* Jump to the this point in order to terminate the query.
     */
    sqlite3VdbeResolveLabel(v, labelEnd);

    /* Make arrangements to free the 2nd and subsequent arms of the compound
     ** after the parse has finished (Rust: freed with the arena) */
    db.selm(pSplit).pPrior = pPrior;
    db.selm(pPrior).pNext = pSplit;
    let ob = db.sel(pPrior).pOrderBy;
    sqlite3ExprListDelete(db, ob);
    db.selm(pPrior).pOrderBy = ExprListId::NULL;

    /*** TBD:  Insert subroutine calls to close cursors on incomplete
     **** subqueries ****/
    sqlite3VdbeExplainPop(db);
    (db.ppr().nErr != 0) as i32
}

/* An instance of the SubstContext object describes an substitution edit
** to be performed on a parse tree.
**
** All references to columns in table iTable are to be replaced by corresponding
** expressions in pEList.
**
** ## About "isOuterJoin":
**
** The isOuterJoin column indicates that the replacement will occur into a
** position in the parent that is NULL-able due to an OUTER JOIN.  Either the
** target slot in the parent is the right operand of a LEFT JOIN, or one of
** the left operands of a RIGHT JOIN.  In either case, we need to potentially
** bypass the substituted expression with OP_IfNullRow.
**
** Suppose the original expression is an integer constant. Even though the table
** has the nullRow flag set, because the expression is an integer constant,
** it will not be NULLed out.  So instead, we insert an OP_IfNullRow opcode
** that checks to see if the nullRow flag is set on the table.  If the nullRow
** flag is set, then the value in the register is set to NULL and the original
** expression is bypassed.  If the nullRow flag is not set, then the original
** expression runs to populate the register.
**
** Example where this is needed:
**
**      CREATE TABLE t1(a INTEGER PRIMARY KEY, b INT);
**      CREATE TABLE t2(x INT UNIQUE);
**
**      SELECT a,b,m,x FROM t1 LEFT JOIN (SELECT 59 AS m,x FROM t2) ON b=x;
**
** When the subquery on the right side of the LEFT JOIN is flattened, we
** have to add OP_IfNullRow in front of the OP_Integer that implements the
** "m" value of the subquery so that a NULL will be loaded instead of 59
** when processing a non-matched row of the left.
*/
struct SubstContext {
    /// Replace references to this table
    iTable: i32,
    /// New table number
    iNewTable: i32,
    /// Add TK_IF_NULL_ROW opcodes on each replacement
    isOuterJoin: i32,
    /// Depth of sub-query recursion.  Top==1
    nSelDepth: i32,
    /// Replacement expressions
    pEList: ExprListId,
    /// Collation sequences for replacement expr
    pCList: ExprListId,
}

/*
** Scan through the expression pExpr.  Replace every reference to
** a column in table number iTable with a copy of the iColumn-th
** entry in pEList.  (But leave references to the ROWID column
** unchanged.)
**
** This routine is part of the flattening procedure.  A subquery
** whose result set is defined by pEList appears as entry in the
** FROM clause of a SELECT such that the VDBE cursor assigned to that
** FORM clause entry is iTable.  This routine makes the necessary
** changes to pExpr so that it refers directly to the source table
** of the subquery rather the result set of the subquery.
*/
fn substExpr(db: &mut Sqlite3, pSubst: &mut SubstContext, mut pExpr: ExprId) -> ExprId {
    if pExpr.is_null() {
        return ExprId::NULL;
    }
    {
        let e = db.exm(pExpr);
        if e.HasProperty(EP_OuterON | EP_InnerON) && e.w.iJoin == pSubst.iTable {
            e.w.iJoin = pSubst.iNewTable;
        }
    }
    let (op, iTable, flags) = {
        let e = db.ex(pExpr);
        (e.op, e.iTable, e.flags)
    };
    if op == TK_COLUMN && iTable == pSubst.iTable && (flags & EP_FixedCol) == 0 {
        let iColumn = db.ex(pExpr).iColumn as i32;
        debug_assert!(iColumn >= 0);
        debug_assert!(pSubst.pEList.nn() && iColumn < db.el(pSubst.pEList).nExpr());
        debug_assert!(db.ex(pExpr).pRight.is_null());
        let mut pCopy = db.el(pSubst.pEList).a[iColumn as usize].pExpr;
        if sqlite3ExprIsVector(db, pCopy) != 0 {
            sqlite3VectorErrorMsg(db, pCopy);
        } else {
            let (cop, ctab) = {
                let c = db.ex(pCopy);
                (c.op, c.iTable)
            };
            if pSubst.isOuterJoin != 0 && (cop != TK_COLUMN || ctab != pSubst.iNewTable) {
                let ifNullRow = Expr {
                    op: TK_IF_NULL_ROW,
                    pLeft: pCopy,
                    iTable: pSubst.iNewTable,
                    iColumn: -99,
                    flags: EP_IfNullRow,
                    ..Expr::default()
                };
                pCopy = db.arena.new_expr(ifNullRow);
            }
            let mut pNew = sqlite3ExprDup(db, pCopy, 0);
            if db.mallocFailed != 0 {
                sqlite3ExprDelete(db, pNew);
                return pExpr;
            }
            if pSubst.isOuterJoin != 0 {
                db.exm(pNew).SetProperty(EP_CanBeNull);
            }
            if db.ex(pNew).op == TK_TRUEFALSE {
                let tv = sqlite3ExprTruthValue(db, pNew);
                let e = db.exm(pNew);
                e.u.iValue = tv;
                e.op = TK_INTEGER;
                e.SetProperty(EP_IntValue);
            }

            /* Ensure that the expression now has an implicit collation sequence,
             ** just as it did when it was a column of a view or sub-query. */
            {
                let pNat = sqlite3ExprCollSeq(db, pNew);
                let ce = db.el(pSubst.pCList).a[iColumn as usize].pExpr;
                let pColl = sqlite3ExprCollSeq(db, ce);
                let nop = db.ex(pNew).op;
                if !collEq(&pNat, &pColl) || (nop != TK_COLUMN && nop != TK_COLLATE) {
                    let zName: ZStr = match &pColl {
                        Some(c) => c.zName.clone(),
                        None => b"BINARY".to_vec(),
                    };
                    pNew = sqlite3ExprAddCollateString(db, pNew, Some(&zName));
                }
            }
            db.exm(pNew).ClearProperty(EP_Collate);
            let (eflags, iJoin) = {
                let e = db.ex(pExpr);
                (e.flags, e.w.iJoin)
            };
            if (eflags & (EP_OuterON | EP_InnerON)) != 0 {
                sqlite3SetJoinExpr(db, pNew, iJoin, eflags & (EP_OuterON | EP_InnerON));
            }
            sqlite3ExprDelete(db, pExpr);
            pExpr = pNew;
        }
    } else {
        {
            let e = db.exm(pExpr);
            if e.op == TK_IF_NULL_ROW && e.iTable == pSubst.iTable {
                e.iTable = pSubst.iNewTable;
            }
            if e.op == TK_AGG_FUNCTION && e.op2 as i32 >= pSubst.nSelDepth {
                e.op2 = e.op2.wrapping_sub(1); // C u8 arithmetic
            }
        }
        let l = db.ex(pExpr).pLeft;
        let nl = substExpr(db, pSubst, l);
        db.exm(pExpr).pLeft = nl;
        let r = db.ex(pExpr).pRight;
        let nr = substExpr(db, pSubst, r);
        db.exm(pExpr).pRight = nr;
        let (useSel, ps, pl) = {
            let e = db.ex(pExpr);
            (e.UseXSelect(), e.x.pSelect, e.x.pList)
        };
        if useSel {
            substSelect(db, pSubst, ps, 1);
        } else {
            substExprList(db, pSubst, pl);
        }
        if db.ex(pExpr).HasProperty(EP_WinFunc) {
            let pWin = db.ex(pExpr).y.pWin;
            let f = db.win(pWin).pFilter;
            let nf = substExpr(db, pSubst, f);
            db.winm(pWin).pFilter = nf;
            let pp = db.win(pWin).pPartition;
            substExprList(db, pSubst, pp);
            let po = db.win(pWin).pOrderBy;
            substExprList(db, pSubst, po);
        }
    }
    pExpr
}

fn substExprList(db: &mut Sqlite3, pSubst: &mut SubstContext, pList: ExprListId) {
    if pList.is_null() {
        return;
    }
    let n = db.el(pList).nExpr();
    for i in 0..n {
        let e = db.el(pList).a[i as usize].pExpr;
        let ne = substExpr(db, pSubst, e);
        db.elm(pList).a[i as usize].pExpr = ne;
    }
}

fn substSelect(db: &mut Sqlite3, pSubst: &mut SubstContext, mut p: SelectId, doPrior: i32) {
    if p.is_null() {
        return;
    }
    pSubst.nSelDepth += 1;
    loop {
        let s = db.sel(p).clone();
        substExprList(db, pSubst, s.pEList);
        substExprList(db, pSubst, s.pGroupBy);
        substExprList(db, pSubst, s.pOrderBy);
        let h = substExpr(db, pSubst, s.pHaving);
        db.selm(p).pHaving = h;
        let w = substExpr(db, pSubst, db.sel(p).pWhere);
        db.selm(p).pWhere = w;
        let pSrc = db.sel(p).pSrc;
        debug_assert!(pSrc.nn());
        let n = db.src(pSrc).nSrc();
        for i in 0..n {
            let it = SrcItemId(pSrc, i as u32);
            if db.item(it).fg.isSubquery != 0 {
                let ps = subqSelect(db, it);
                substSelect(db, pSubst, ps, 1);
            }
            if db.item(it).fg.isTabFunc != 0 {
                let fa = db.item(it).u1.pFuncArg;
                substExprList(db, pSubst, fa);
            }
        }
        if doPrior == 0 {
            break;
        }
        p = db.sel(p).pPrior;
        if p.is_null() {
            break;
        }
    }
    pSubst.nSelDepth -= 1;
}

/*
** pSelect is a SELECT statement and pSrcItem is one item in the FROM
** clause of that SELECT.
**
** This routine scans the entire SELECT statement and recomputes the
** pSrcItem->colUsed mask.
*/
fn recomputeColumnsUsedExpr(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, iTable, iColumn) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iTable, e.iColumn)
    };
    if op != TK_COLUMN {
        return WRC_Continue;
    }
    let pItem = w.u.pSrcItem;
    if w.db.item(pItem).iCursor != iTable {
        return WRC_Continue;
    }
    if iColumn < 0 {
        return WRC_Continue;
    }
    let m = crate::resolve::sqlite3ExprColUsed(w.db, pExpr);
    w.db.itemm(pItem).colUsed |= m;
    WRC_Continue
}
fn recomputeColumnsUsed(db: &mut Sqlite3, pSelect: SelectId, pSrcItem: SrcItemId) {
    if db.item(pSrcItem).pSTab.is_none() {
        return;
    }
    db.itemm(pSrcItem).colUsed = 0;
    let mut w = Walker::new(db);
    w.xExprCallback = Some(recomputeColumnsUsedExpr);
    w.xSelectCallback = Some(sqlite3SelectWalkNoop);
    w.u.pSrcItem = pSrcItem;
    sqlite3WalkSelect(&mut w, pSelect);
}

/*
** Assign new cursor numbers to each of the items in pSrc. For each
** new cursor number assigned, set an entry in the aCsrMap[] array
** to map the old cursor number to the new:
**
**     aCsrMap[iOld+1] = iNew;
**
** The array is guaranteed by the caller to be large enough for all
** existing cursor numbers in pSrc.  aCsrMap[0] is the array size.
**
** If pSrc contains any sub-selects, call this routine recursively
** on the FROM clause of each such sub-select, with iExcept set to -1.
*/
fn srclistRenumberCursors(db: &mut Sqlite3, aCsrMap: &mut Vec<i32>, pSrc: SrcListId, iExcept: i32) {
    let n = db.src(pSrc).nSrc();
    for i in 0..n {
        if i != iExcept {
            let it = SrcItemId(pSrc, i as u32);
            let (iCursor, isRecursive) = {
                let x = db.item(it);
                (x.iCursor, x.fg.isRecursive)
            };
            debug_assert!(iCursor < aCsrMap[0]);
            let k = (iCursor + 1) as usize;
            if isRecursive == 0 || aCsrMap[k] == 0 {
                aCsrMap[k] = db.ppr().nTab;
                db.pp().nTab += 1;
            }
            db.itemm(it).iCursor = aCsrMap[k];
            if db.item(it).fg.isSubquery != 0 {
                let mut p = subqSelect(db, it);
                while p.nn() {
                    let ps = db.sel(p).pSrc;
                    srclistRenumberCursors(db, aCsrMap, ps, -1);
                    p = db.sel(p).pPrior;
                }
            }
        }
    }
}

/*
** *piCursor is a cursor number.  Change it if it needs to be mapped.
*/
fn renumberCursorDoMapping(aCsrMap: &[i32], piCursor: &mut i32) {
    let iCsr = *piCursor;
    if iCsr < aCsrMap[0] && aCsrMap[(iCsr + 1) as usize] > 0 {
        *piCursor = aCsrMap[(iCsr + 1) as usize];
    }
}

/*
** Expression walker callback used by renumberCursors() to update
** Expr objects to match newly assigned cursor numbers.
*/
fn renumberCursorsCb(w: &mut Walker, pExpr: ExprId) -> i32 {
    let aCsrMap = std::mem::take(&mut w.u.aiCol);
    {
        let e = w.db.exm(pExpr);
        let op = e.op;
        if op == TK_COLUMN || op == TK_IF_NULL_ROW {
            renumberCursorDoMapping(&aCsrMap, &mut e.iTable);
        }
        if e.HasProperty(EP_OuterON | EP_InnerON) {
            renumberCursorDoMapping(&aCsrMap, &mut e.w.iJoin);
        }
    }
    w.u.aiCol = aCsrMap;
    WRC_Continue
}

/*
** Assign a new cursor number to each cursor in the FROM clause (Select.pSrc)
** of the SELECT statement passed as the second argument, and to each
** cursor in the FROM clause of any FROM clause sub-selects, recursively.
** Except, do not assign a new cursor number to the iExcept'th element in
** the FROM clause of (*p). Update all expressions and other references
** to refer to the new cursor numbers.
**
** Argument aCsrMap is an array that may be used for temporary working
** space. Two guarantees are made by the caller:
**
**   * the array is larger than the largest cursor number used within the
**     select statement passed as an argument, and
**
**   * the array entries for all cursor numbers that do *not* appear in
**     FROM clauses of the select statement as described above are
**     initialized to zero.
*/
fn renumberCursors(db: &mut Sqlite3, p: SelectId, iExcept: i32, aCsrMap: &mut Vec<i32>) {
    let pSrc = db.sel(p).pSrc;
    srclistRenumberCursors(db, aCsrMap, pSrc, iExcept);
    let mut w = Walker::new(db);
    w.u.aiCol = std::mem::take(aCsrMap);
    w.xExprCallback = Some(renumberCursorsCb);
    w.xSelectCallback = Some(sqlite3SelectWalkNoop);
    sqlite3WalkSelect(&mut w, p);
    *aCsrMap = std::mem::take(&mut w.u.aiCol);
}

/*
** If pSel is not part of a compound SELECT, return a pointer to its
** expression list. Otherwise, return a pointer to the expression list
** of the leftmost SELECT in the compound.
*/
fn findLeftmostExprlist(db: &Sqlite3, mut pSel: SelectId) -> ExprListId {
    while db.sel(pSel).pPrior.nn() {
        pSel = db.sel(pSel).pPrior;
    }
    db.sel(pSel).pEList
}

/*
** Return true if any of the result-set columns in the compound query
** have incompatible affinities on one or more arms of the compound.
*/
fn compoundHasDifferentAffinities(db: &mut Sqlite3, p: SelectId) -> bool {
    debug_assert!(p.nn());
    debug_assert!(db.sel(p).pEList.nn());
    debug_assert!(db.sel(p).pPrior.nn());
    let pList = db.sel(p).pEList;
    let n = db.el(pList).nExpr();
    for ii in 0..n {
        let e = db.el(pList).a[ii as usize].pExpr;
        let aff = sqlite3ExprAffinity(db, e);
        let mut pSub1 = db.sel(p).pPrior;
        while pSub1.nn() {
            let l = db.sel(pSub1).pEList;
            debug_assert!(db.el(l).nExpr() > ii);
            let e1 = db.el(l).a[ii as usize].pExpr;
            if sqlite3ExprAffinity(db, e1) != aff {
                return true;
            }
            pSub1 = db.sel(pSub1).pPrior;
        }
    }
    false
}

/*
** This routine attempts to flatten subqueries as a performance optimization.
** This routine returns 1 if it makes changes and 0 if no flattening occurs.
**
** To understand the concept of flattening, consider the following
** query:
**
**     SELECT a FROM (SELECT x+y AS a FROM t1 WHERE z<100) WHERE a>5
**
** The default way of implementing this query is to execute the
** subquery first and store the results in a temporary table, then
** run the outer query on that temporary table.  This requires two
** passes over the data.  Furthermore, because the temporary table
** has no indices, the WHERE clause on the outer query cannot be
** optimized.
**
** This routine attempts to rewrite queries such as the above into
** a single flat select, like this:
**
**     SELECT x+y AS a FROM t1 WHERE z<100 AND a>5
**
** The code generated for this simplification gives the same result
** but only has to scan the data once.  And because indices might
** exist on the table t1, a complete scan of the data might be
** avoided.
**
** Flattening is subject to the following constraints:
**
**  (**)  We no longer attempt to flatten aggregate subqueries. Was:
**        The subquery and the outer query cannot both be aggregates.
**
**  (**)  We no longer attempt to flatten aggregate subqueries. Was:
**        (2) If the subquery is an aggregate then
**        (2a) the outer query must not be a join and
**        (2b) the outer query must not use subqueries
**             other than the one FROM-clause subquery that is a candidate
**             for flattening.  (This is due to ticket [2f7170d73bf9abf80]
**             from 2015-02-09.)
**
**   (3)  If the subquery is the right operand of a LEFT JOIN then
**        (3a) the subquery may not be a join
**        (**) Was (3b): "the FROM clause of the subquery may not contain
**             a virtual table"
**        (**) Was: "The outer query may not have a GROUP BY." This case
**             is now managed correctly
**        (3d) the outer query may not be DISTINCT.
**        See also (26) for restrictions on RIGHT JOIN.
**
**   (4)  The subquery can not be DISTINCT.
**
**  (**)  At one point restrictions (4) and (5) defined a subset of DISTINCT
**        sub-queries that were excluded from this optimization. Restriction
**        (4) has since been expanded to exclude all DISTINCT subqueries.
**
**  (**)  We no longer attempt to flatten aggregate subqueries.  Was:
**        If the subquery is aggregate, the outer query may not be DISTINCT.
**
**   (7)  The subquery must have a FROM clause.  TODO:  For subqueries without
**        A FROM clause, consider adding a FROM clause with the special
**        table sqlite_once that consists of a single row containing a
**        single NULL.
**
**   (8)  If the subquery uses LIMIT then the outer query may not be a join.
**
**   (9)  If the subquery uses LIMIT then the outer query may not be aggregate.
**
**  (**)  Restriction (10) was removed from the code on 2005-02-05 but we
**        accidentally carried the comment forward until 2014-09-15.  Original
**        constraint: "If the subquery is aggregate then the outer query
**        may not use LIMIT."
**
**  (11)  The subquery and the outer query may not both have ORDER BY clauses.
**
**  (**)  Not implemented.  Subsumed into restriction (3).  Was previously
**        a separate restriction deriving from ticket #350.
**
**  (13)  The subquery and outer query may not both use LIMIT.
**
**  (14)  The subquery may not use OFFSET.
**
**  (15)  If the outer query is part of a compound select, then the
**        subquery may not use LIMIT.
**        (See ticket #2339 and ticket [02a8e81d44]).
**
**  (16)  If the outer query is aggregate, then the subquery may not
**        use ORDER BY.  (Ticket #2942)  This used to not matter
**        until we introduced the group_concat() function.
**
**  (17)  If the subquery is a compound select, then
**        (17a) all compound operators must be a UNION ALL, and
**        (17b) no terms within the subquery compound may be aggregate
**              or DISTINCT, and
**        (17c) every term within the subquery compound must have a FROM clause
**        (17d) the outer query may not be
**              (17d1) aggregate, or
**              (17d2) DISTINCT
**        (17e) the subquery may not contain window functions, and
**        (17f) the subquery must not be the RHS of a LEFT JOIN.
**        (17g) either the subquery is the first element of the outer
**              query or there are no RIGHT or FULL JOINs in any arm
**              of the subquery.  (This is a duplicate of condition (27b).)
**        (17h) The corresponding result set expressions in all arms of the
**              compound must have the same affinity.
**
**        The parent and sub-query may contain WHERE clauses. Subject to
**        rules (11), (13) and (14), they may also contain ORDER BY,
**        LIMIT and OFFSET clauses.  The subquery cannot use any compound
**        operator other than UNION ALL because all the other compound
**        operators have an implied DISTINCT which is disallowed by
**        restriction (4).
**
**        Also, each component of the sub-query must return the same number
**        of result columns. This is actually a requirement for any compound
**        SELECT statement, but all the code here does is make sure that no
**        such (illegal) sub-query is flattened. The caller will detect the
**        syntax error and return a detailed message.
**
**  (18)  If the sub-query is a compound select, then all terms of the
**        ORDER BY clause of the parent must be copies of a term returned
**        by the parent query.
**
**  (19)  If the subquery uses LIMIT then the outer query may not
**        have a WHERE clause.
**
**  (20)  If the sub-query is a compound select, then it must not use
**        an ORDER BY clause.  Ticket #3773.  We could relax this constraint
**        somewhat by saying that the terms of the ORDER BY clause must
**        appear as unmodified result columns in the outer query.  But we
**        have other optimizations in mind to deal with that case.
**
**  (21)  If the subquery uses LIMIT then the outer query may not be
**        DISTINCT.  (See ticket [752e1646fc]).
**
**  (22)  The subquery may not be a recursive CTE.
**
**  (23)  If the outer query is a recursive CTE, then the sub-query may not be
**        a compound query.  This restriction is because transforming the
**        parent to a compound query confuses the code that handles
**        recursive queries in multiSelect().
**
**  (**)  We no longer attempt to flatten aggregate subqueries.  Was:
**        The subquery may not be an aggregate that uses the built-in min() or
**        or max() functions.  (Without this restriction, a query like:
**        "SELECT x FROM (SELECT max(y), x FROM t1)" would not necessarily
**        return the value X for which Y was maximal.)
**
**  (25)  If either the subquery or the parent query contains a window
**        function in the select list or ORDER BY clause, flattening
**        is not attempted.
**
**  (26)  The subquery may not be the right operand of a RIGHT JOIN.
**        See also (3) for restrictions on LEFT JOIN.
**
**  (27)  The subquery may not contain a FULL or RIGHT JOIN unless it
**        is the first element of the parent query.  Two subcases:
**        (27a) the subquery is not a compound query.
**        (27b) the subquery is a compound query and the RIGHT JOIN occurs
**              in any arm of the compound query.  (See also (17g).)
**
**  (28)  The subquery is not a MATERIALIZED CTE.  (This is handled
**        in the caller before ever reaching this routine.)
**
**
** In this routine, the "p" parameter is a pointer to the outer query.
** The subquery is p->pSrc->a[iFrom].  isAgg is true if the outer query
** uses aggregates.
**
** If flattening is not attempted, this routine is a no-op and returns 0.
** If flattening is attempted this routine returns 1.
**
** All of the expression analysis must occur on both the outer query and
** the subquery before this routine runs.
*/
fn flattenSubquery(db: &mut Sqlite3, p: SelectId, iFrom: i32, isAgg: i32) -> i32 {
    let zSavedAuthContext = db.ppr().zAuthContext.clone();
    let mut isOuterJoin = 0; /* True if pSub is the right side of a LEFT JOIN */
    let mut aCsrMap: Option<Vec<i32>> = None;

    /* Check to see if flattening is permitted.  Return 0 if not.
     */
    debug_assert!(p.nn());
    debug_assert!(db.sel(p).pPrior.is_null());
    if db.OptimizationDisabled(SQLITE_QueryFlattener) {
        return 0;
    }
    let mut pSrc = db.sel(p).pSrc;
    debug_assert!(pSrc.nn() && iFrom >= 0 && iFrom < db.src(pSrc).nSrc());
    let pSubitem = SrcItemId(pSrc, iFrom as u32);
    let iParent = db.item(pSubitem).iCursor;
    debug_assert!(db.item(pSubitem).fg.isSubquery != 0);
    let mut pSub = subqSelect(db, pSubitem);
    debug_assert!(pSub.nn());

    let ps = db.sel(p).clone();
    let sb = db.sel(pSub).clone();
    if ps.pWin.nn() || sb.pWin.nn() {
        return 0; /* Restriction (25) */
    }

    let mut pSubSrc = sb.pSrc;
    debug_assert!(pSubSrc.nn());
    /* Prior to version 3.1.2, when LIMIT and OFFSET had to be simple constants,
     ** not arbitrary expressions, we allowed some combining of LIMIT and OFFSET
     ** because they could be computed at compile-time.  But when LIMIT and OFFSET
     ** became arbitrary expressions, we were forced to add restrictions (13)
     ** and (14). */
    if sb.pLimit.nn() && ps.pLimit.nn() {
        return 0; /* Restriction (13) */
    }
    if sb.pLimit.nn() && db.ex(sb.pLimit).pRight.nn() {
        return 0; /* Restriction (14) */
    }
    if (ps.selFlags & SF_Compound) != 0 && sb.pLimit.nn() {
        return 0; /* Restriction (15) */
    }
    if db.src(pSubSrc).nSrc() == 0 {
        return 0; /* Restriction (7)  */
    }
    if (sb.selFlags & SF_Distinct) != 0 {
        return 0; /* Restriction (4)  */
    }
    if sb.pLimit.nn() && (db.src(pSrc).nSrc() > 1 || isAgg != 0) {
        return 0; /* Restrictions (8)(9) */
    }
    if ps.pOrderBy.nn() && sb.pOrderBy.nn() {
        return 0; /* Restriction (11) */
    }
    if isAgg != 0 && sb.pOrderBy.nn() {
        return 0; /* Restriction (16) */
    }
    if sb.pLimit.nn() && ps.pWhere.nn() {
        return 0; /* Restriction (19) */
    }
    if sb.pLimit.nn() && (ps.selFlags & SF_Distinct) != 0 {
        return 0; /* Restriction (21) */
    }
    if (sb.selFlags & SF_Recursive) != 0 {
        return 0; /* Restrictions (22) */
    }

    /*
     ** If the subquery is the right operand of a LEFT JOIN, then the
     ** subquery may not be a join itself (3a). Example of why this is not
     ** allowed:
     **
     **         t1 LEFT OUTER JOIN (t2 JOIN t3)
     **
     ** If we flatten the above, we would get
     **
     **         (t1 LEFT OUTER JOIN t2) JOIN t3
     **
     ** which is not at all the same thing.
     **
     ** See also tickets #306, #350, and #3300.
     */
    let sjt = db.item(pSubitem).fg.jointype;
    if (sjt & (JT_OUTER | JT_LTORJ)) != 0 {
        if db.src(pSubSrc).nSrc() > 1                 /* (3a) */
            || (ps.selFlags & SF_Distinct) != 0       /* (3d) */
            || (sjt & JT_RIGHT) != 0
        /* (26) */
        {
            return 0;
        }
        isOuterJoin = 1;
    }

    debug_assert!(db.src(pSubSrc).nSrc() > 0); /* True by restriction (7) */
    if iFrom > 0 && (db.src(pSubSrc).a[0].fg.jointype & JT_LTORJ) != 0 {
        return 0; /* Restriction (27a) */
    }

    /* Condition (28) is blocked by the caller */

    /* Restriction (17): If the sub-query is a compound SELECT, then it must
     ** use only the UNION ALL operator. And none of the simple select queries
     ** that make up the compound SELECT are allowed to be aggregate or distinct
     ** queries.
     */
    if sb.pPrior.nn() {
        if sb.pOrderBy.nn() {
            return 0; /* Restriction (20) */
        }
        if isAgg != 0 || (ps.selFlags & SF_Distinct) != 0 || isOuterJoin > 0 {
            return 0; /* (17d1), (17d2), or (17f) */
        }
        let mut pSub1 = pSub;
        while pSub1.nn() {
            let s1 = db.sel(pSub1).clone();
            debug_assert!((sb.selFlags & SF_Recursive) == 0);
            if (s1.selFlags & (SF_Distinct | SF_Aggregate)) != 0  /* (17b) */
                || (s1.pPrior.nn() && s1.op != TK_ALL)          /* (17a) */
                || db.src(s1.pSrc).nSrc() < 1                   /* (17c) */
                || s1.pWin.nn()
            /* (17e) */
            {
                return 0;
            }
            if iFrom > 0 && (db.src(s1.pSrc).a[0].fg.jointype & JT_LTORJ) != 0 {
                /* Without this restriction, the JT_LTORJ flag would end up being
                 ** omitted on left-hand tables of the right join that is being
                 ** flattened. */
                return 0; /* Restrictions (17g), (27b) */
            }
            pSub1 = s1.pPrior;
        }

        /* Restriction (18). */
        if ps.pOrderBy.nn() {
            let n = db.el(ps.pOrderBy).nExpr();
            for ii in 0..n {
                if db.el(ps.pOrderBy).a[ii as usize].u.x.iOrderByCol == 0 {
                    return 0;
                }
            }
        }

        /* Restriction (23) */
        if (ps.selFlags & SF_Recursive) != 0 {
            return 0;
        }

        /* Restriction (17h) */
        if compoundHasDifferentAffinities(db, pSub) {
            return 0;
        }

        if db.src(pSrc).nSrc() > 1 {
            if db.ppr().nSelect > 500 {
                return 0;
            }
            if db.OptimizationDisabled(SQLITE_FlttnUnionAll) {
                return 0;
            }
            let nTab = db.ppr().nTab;
            let mut m = vec![0i32; (nTab + 1) as usize];
            m[0] = nTab;
            aCsrMap = Some(m);
        }
    }

    /***** If we reach this point, flattening is permitted. *****/

    /* Authorize the subquery */
    db.pp().zAuthContext = db.item(pSubitem).zName.clone();
    crate::auth::sqlite3AuthCheck(db, SQLITE_SELECT, None, None, None);
    db.pp().zAuthContext = zSavedAuthContext;

    /* Delete the transient structures associated with the subquery */
    let pSub1 = if db.item(pSubitem).fg.isSubquery != 0 {
        sqlite3SubqueryDetach(db, pSubitem)
    } else {
        SelectId::NULL
    };
    debug_assert!(db.item(pSubitem).fg.isSubquery == 0);
    debug_assert!(db.item(pSubitem).fg.fixedSchema == 0);
    {
        let it = db.itemm(pSubitem);
        it.zName = None;
        it.zAlias = None;
    }

    /* If the sub-query is a compound SELECT statement, then (by restrictions
     ** 17 and 18 above) it must be a UNION ALL and the parent query must
     ** be of the form:
     **
     **     SELECT <expr-list> FROM (<sub-query>) <where-clause>
     **
     ** followed by any ORDER BY, LIMIT and/or OFFSET clauses. This block
     ** creates N-1 copies of the parent query without any ORDER BY, LIMIT or
     ** OFFSET clauses and joins them to the left-hand-side of the original
     ** using UNION ALL operators. In this case N is the number of simple
     ** select statements in the compound sub-query.
     **
     ** Example:
     **
     **     SELECT a+1 FROM (
     **        SELECT x FROM tab
     **        UNION ALL
     **        SELECT y FROM tab
     **        UNION ALL
     **        SELECT abs(z*2) FROM tab2
     **     ) WHERE a!=5 ORDER BY 1
     **
     ** Transformed into:
     **
     **     SELECT x+1 FROM tab WHERE x+1!=5
     **     UNION ALL
     **     SELECT y+1 FROM tab WHERE y+1!=5
     **     UNION ALL
     **     SELECT abs(z*2)+1 FROM tab2 WHERE abs(z*2)+1!=5
     **     ORDER BY 1
     **
     ** We call this the "compound-subquery flattening".
     */
    pSub = db.sel(pSub).pPrior;
    while pSub.nn() {
        let (pOrderBy, pLimit, pPrior) = {
            let s = db.sel(p);
            (s.pOrderBy, s.pLimit, s.pPrior)
        };
        let pItemTab = db.itemm(pSubitem).pSTab.take();
        {
            let s = db.selm(p);
            s.pOrderBy = ExprListId::NULL;
            s.pPrior = SelectId::NULL;
            s.pLimit = ExprId::NULL;
        }
        let pNew = sqlite3SelectDup(db, p, 0);
        {
            let s = db.selm(p);
            s.pLimit = pLimit;
            s.pOrderBy = pOrderBy;
            s.op = TK_ALL;
        }
        db.itemm(pSubitem).pSTab = pItemTab;
        if pNew.is_null() {
            db.selm(p).pPrior = pPrior;
        } else {
            db.pp().nSelect += 1;
            db.selm(pNew).selId = db.ppr().nSelect as u32;
            if let Some(m) = aCsrMap.as_mut() {
                if db.mallocFailed == 0 {
                    renumberCursors(db, pNew, iFrom, m);
                }
            }
            db.selm(pNew).pPrior = pPrior;
            if pPrior.nn() {
                db.selm(pPrior).pNext = pNew;
            }
            db.selm(pNew).pNext = p;
            db.selm(p).pPrior = pNew;
        }
        debug_assert!(db.item(pSubitem).fg.isSubquery == 0);
        pSub = db.sel(pSub).pPrior;
    }
    drop(aCsrMap);
    if db.mallocFailed != 0 {
        sqlite3SrcItemAttachSubquery(db, pSubitem, pSub1, 0);
        return 1;
    }

    /* Defer deleting the Table object associated with the
     ** subquery until code generation is
     ** complete, since there may still exist Expr.pTab entries that
     ** refer to the subquery even after flattening.  Ticket #3346.
     **
     ** pSubitem->pSTab is always non-NULL by test restrictions and tests above.
     */
    if let Some(pTabToDel) = db.itemm(pSubitem).pSTab.take() {
        let mut t = pTabToDel.borrow_mut();
        if t.nTabRef != 1 {
            t.nTabRef -= 1;
        }
        /* (nTabRef==1: the Table is freed when the last Rc goes away) */
    }

    /* The following loop runs once for each term in a compound-subquery
     ** flattening (as described above).  If we are doing a different kind
     ** of flattening - a flattening other than a compound-subquery flattening -
     ** then this loop only runs once.
     **
     ** This loop moves all of the FROM elements of the subquery into the
     ** the FROM clause of the outer query.  Before doing this, remember
     ** the cursor number for the original outer query FROM element in
     ** iParent.  The iParent cursor will never be used.  Subsequent code
     ** will scan expressions looking for iParent references and replace
     ** those references with expressions that resolve to the subquery FROM
     ** elements we are now copying in.
     */
    pSub = pSub1;
    let mut pParent = p;
    while pParent.nn() {
        debug_assert!(pSub.nn());
        let jointype = db.src(db.sel(pParent).pSrc).a[iFrom as usize].fg.jointype;
        pSubSrc = db.sel(pSub).pSrc; /* FROM clause of subquery */
        let nSubSrc = db.src(pSubSrc).nSrc(); /* Number of terms in subquery FROM clause */
        pSrc = db.sel(pParent).pSrc; /* FROM clause of the outer query */

        /* The subquery uses a single slot of the FROM clause of the outer
         ** query.  If the subquery has more than one element in its FROM clause,
         ** then expand the outer query to make space for it to hold all elements
         ** of the subquery.
         **
         ** Example:
         **
         **    SELECT * FROM tabA, (SELECT * FROM sub1, sub2), tabB;
         **
         ** The outer query has 3 slots in its FROM clause.  One slot of the
         ** outer query (the middle slot) is used by the subquery.  The next
         ** block of code will expand the outer query FROM clause to 4 slots.
         ** The middle slot is expanded to two slots in order to make space
         ** for the two elements in the FROM clause of the subquery.
         */
        if nSubSrc > 1 {
            pSrc = sqlite3SrcListEnlarge(db, pSrc, nSubSrc - 1, iFrom + 1);
            if pSrc.is_null() {
                break;
            }
            db.selm(pParent).pSrc = pSrc;
        }

        /* Transfer the FROM clause terms from the subquery into the
         ** outer query.
         */
        let iNewParent = db.src(pSubSrc).a[0].iCursor;
        for i in 0..nSubSrc {
            let pItem = SrcItemId(pSrc, (i + iFrom) as u32);
            debug_assert!(db.item(pItem).fg.isTabFunc == 0);
            if db.item(pItem).fg.isUsing != 0 {
                let u = db.item(pItem).u3.pUsing;
                sqlite3IdListDelete(db, u);
            }
            let moved = std::mem::take(&mut db.srcm(pSubSrc).a[i as usize]);
            *db.itemm(pItem) = moved;
            db.itemm(pItem).fg.jointype |= jointype & JT_LTORJ;
        }
        db.srcm(pSrc).a[iFrom as usize].fg.jointype |= jointype;

        /* Begin substituting subquery result set expressions for
         ** references to the iParent in the outer query.
         **
         ** Example:
         **
         **   SELECT a+5, b*10 FROM (SELECT x*3 AS a, y+10 AS b FROM t1) WHERE a>b;
         **   \                     \_____________ subquery __________/          /
         **    \_____________________ outer query ______________________________/
         **
         ** We look at every expression in the outer query and every place we see
         ** "a" we substitute "x*3" and every place we see "b" we substitute "y+10".
         */
        let subOB = db.sel(pSub).pOrderBy;
        if subOB.nn() {
            /* At this point, any non-zero iOrderByCol values indicate that the
             ** ORDER BY column expression is identical to the iOrderByCol'th
             ** expression returned by SELECT statement pSub. Since these values
             ** do not necessarily correspond to columns in SELECT statement pParent,
             ** zero them before transferring the ORDER BY clause.
             **
             ** Not doing this may cause an error if a subsequent call to this
             ** function attempts to flatten a compound sub-query into pParent.
             ** See ticket [d11a6e908f].
             */
            for it in db.elm(subOB).a.iter_mut() {
                it.u.x.iOrderByCol = 0;
            }
            debug_assert!(db.sel(pParent).pOrderBy.is_null());
            db.selm(pParent).pOrderBy = subOB;
            db.selm(pSub).pOrderBy = ExprListId::NULL;
        }
        let pWhere = db.sel(pSub).pWhere;
        db.selm(pSub).pWhere = ExprId::NULL;
        if isOuterJoin > 0 {
            debug_assert!(db.src(pSubSrc).nSrc() == 1);
            sqlite3SetJoinExpr(db, pWhere, iNewParent, EP_OuterON);
        }
        if pWhere.nn() {
            let pw = db.sel(pParent).pWhere;
            if pw.nn() {
                let ne = sqlite3PExpr(db, TK_AND as i32, pWhere, pw);
                db.selm(pParent).pWhere = ne;
            } else {
                db.selm(pParent).pWhere = pWhere;
            }
        }
        if db.mallocFailed == 0 {
            let mut x = SubstContext {
                iTable: iParent,
                iNewTable: iNewParent,
                isOuterJoin,
                nSelDepth: 0,
                pEList: db.sel(pSub).pEList,
                pCList: findLeftmostExprlist(db, pSub),
            };
            substSelect(db, &mut x, pParent, 0);
        }

        /* The flattened query is a compound if either the inner or the
         ** outer query is a compound. */
        let sf = db.sel(pSub).selFlags & SF_Compound;
        db.selm(pParent).selFlags |= sf;
        debug_assert!((db.sel(pSub).selFlags & SF_Distinct) == 0); /* restriction (17b) */

        /*
         ** SELECT ... FROM (SELECT ... LIMIT a OFFSET b) LIMIT x OFFSET y;
         **
         ** One is tempted to try to add a and b to combine the limits.  But this
         ** does not work if either limit is negative.
         */
        let sl = db.sel(pSub).pLimit;
        if sl.nn() {
            db.selm(pParent).pLimit = sl;
            db.selm(pSub).pLimit = ExprId::NULL;
        }

        /* Recompute the SrcItem.colUsed masks for the flattened
         ** tables. */
        for i in 0..nSubSrc {
            recomputeColumnsUsed(db, pParent, SrcItemId(pSrc, (i + iFrom) as u32));
        }

        pParent = db.sel(pParent).pPrior;
        pSub = db.sel(pSub).pPrior;
    }

    /* Finally, delete what is left of the subquery and return success.
     */
    {
        let mut w = Walker::new(db);
        sqlite3AggInfoPersistWalkerInit(&mut w);
        sqlite3WalkSelect(&mut w, pSub1);
    }
    sqlite3SelectDelete(db, pSub1);

    1
}

/*
** A structure to keep track of all of the column values that are fixed to
** a known value due to WHERE clause constraints of the form COLUMN=VALUE.
*/
struct WhereConst {
    /// Number of times a constant is propagated
    nChng: i32,
    /// At least one column in apExpr[] as affinity BLOB
    bHasAffBlob: i32,
    /// Which ON expressions to exclude from considertion.
    /// Either EP_OuterON or EP_InnerON|EP_OuterON
    mExcludeOn: u32,
    /// [i*2] is COLUMN and [i*2+1] is VALUE (nConst is apExpr.len()/2)
    apExpr: Vec<ExprId>,
}

impl WhereConst {
    fn nConst(&self) -> usize {
        self.apExpr.len() / 2
    }
}

/*
** Add a new entry to the pConst object, if appropriate.
**
**    *   Do not add if pValue is not constant.  (This is enforced by
**        the caller)
**
**    *   Do not add duplicate pColumn entries
**
**    *   Do not add if pValue has an affinity
**
**    *   Do not add if the comparison uses a collating sequence other
**        than binary.
**
**    *   Do not add if pValue is a function that might return a subtype
**
** The caller guarantees the pColumn is a column.
*/
fn constInsert(
    db: &mut Sqlite3,
    pConst: &mut WhereConst,
    pColumn: ExprId,
    pValue: ExprId,
    pExpr: ExprId,
) {
    debug_assert!(db.ex(pColumn).op == TK_COLUMN);
    if db.ex(pColumn).HasProperty(EP_FixedCol) {
        return;
    }
    if sqlite3ExprAffinity(db, pValue) != 0 {
        return;
    }
    let c = sqlite3ExprCompareCollSeq(db, pExpr);
    if !isBinary(&c) {
        return;
    }
    if sqlite3ExprCanReturnSubtype(db, pValue) != 0 {
        return;
    }

    /* 2018-10-25 ticket [cf5ed20f]
     ** Make sure the same pColumn is not inserted more than once */
    let (ct, cc) = {
        let e = db.ex(pColumn);
        (e.iTable, e.iColumn)
    };
    for i in 0..pConst.nConst() {
        let pE2 = db.ex(pConst.apExpr[i * 2]);
        debug_assert!(pE2.op == TK_COLUMN);
        if pE2.iTable == ct && pE2.iColumn == cc {
            return; /* Already present.  Return without doing anything. */
        }
    }
    if sqlite3ExprAffinity(db, pColumn) <= SQLITE_AFF_BLOB {
        pConst.bHasAffBlob = 1;
    }

    pConst.apExpr.push(pColumn);
    pConst.apExpr.push(pValue);
}

/*
** Find all terms of COLUMN=VALUE or VALUE=COLUMN in pExpr where VALUE
** is a constant expression and where the term must be true because it
** is part of the AND-connected terms of the expression.  For each term
** found, add it to the pConst structure.
*/
fn findConstInWhere(db: &mut Sqlite3, pConst: &mut WhereConst, pExpr: ExprId) {
    if pExpr.is_null() {
        return;
    }
    let (op, pLeft, pRight) = {
        let e = db.ex(pExpr);
        if e.HasProperty(pConst.mExcludeOn) {
            return;
        }
        (e.op, e.pLeft, e.pRight)
    };
    if op == TK_AND {
        findConstInWhere(db, pConst, pRight);
        findConstInWhere(db, pConst, pLeft);
        return;
    }
    if op != TK_EQ {
        return;
    }
    debug_assert!(pRight.nn());
    debug_assert!(pLeft.nn());
    if db.ex(pRight).op == TK_COLUMN && sqlite3ExprIsConstant(db, true, pLeft) != 0 {
        constInsert(db, pConst, pRight, pLeft, pExpr);
    }
    if db.ex(pLeft).op == TK_COLUMN && sqlite3ExprIsConstant(db, true, pRight) != 0 {
        constInsert(db, pConst, pLeft, pRight, pExpr);
    }
}

/*
** This is a helper function for Walker callback propagateConstantExprRewrite().
**
** Argument pExpr is a candidate expression to be replaced by a value. If
** pExpr is equivalent to one of the columns named in pWalker->u.pConst,
** then overwrite it with the corresponding value. Except, do not do so
** if argument bIgnoreAffBlob is non-zero and the affinity of pExpr
** is SQLITE_AFF_BLOB.
*/
fn propagateConstantExprRewriteOne(
    db: &mut Sqlite3,
    pConst: &mut WhereConst,
    pExpr: ExprId,
    bIgnoreAffBlob: i32,
) -> i32 {
    if db.mallocFailed != 0 {
        return WRC_Prune;
    }
    let (op, iTable, iColumn) = {
        let e = db.ex(pExpr);
        if e.op != TK_COLUMN {
            return WRC_Continue;
        }
        if e.HasProperty(EP_FixedCol | pConst.mExcludeOn) {
            return WRC_Continue;
        }
        (e.op, e.iTable, e.iColumn)
    };
    let _ = op;
    for i in 0..pConst.nConst() {
        let pColumn = pConst.apExpr[i * 2];
        if pColumn == pExpr {
            continue;
        }
        if db.ex(pColumn).iTable != iTable {
            continue;
        }
        if db.ex(pColumn).iColumn != iColumn {
            continue;
        }
        if bIgnoreAffBlob != 0 && sqlite3ExprAffinity(db, pColumn) <= SQLITE_AFF_BLOB {
            break;
        }
        /* A match is found.  Add the EP_FixedCol property */
        pConst.nChng += 1;
        {
            let e = db.exm(pExpr);
            e.ClearProperty(EP_Leaf);
            e.SetProperty(EP_FixedCol);
            debug_assert!(e.pLeft.is_null());
        }
        let v = sqlite3ExprDup(db, pConst.apExpr[i * 2 + 1], 0);
        db.exm(pExpr).pLeft = v;
        if db.mallocFailed != 0 {
            return WRC_Prune;
        }
        break;
    }
    WRC_Prune
}

/*
** This is a Walker expression callback. pExpr is a node from the WHERE
** clause of a SELECT statement. This function examines pExpr to see if
** any substitutions based on the contents of pWalker->u.pConst should
** be made to pExpr or its immediate children.
**
** A substitution is made if:
**
**   + pExpr is a column with an affinity other than BLOB that matches
**     one of the columns in pWalker->u.pConst, or
**
**   + pExpr is a binary comparison operator (=, <=, >=, <, >) that
**     uses an affinity other than TEXT and one of its immediate
**     children is a column that matches one of the columns in
**     pWalker->u.pConst.
*/
fn propagateConstantExprRewrite(w: &mut Walker, pExpr: ExprId) -> i32 {
    let mut any = w.u.any.take();
    let pConst = any
        .as_mut()
        .and_then(|a| a.downcast_mut::<WhereConst>())
        .expect("WhereConst");
    let db = &mut *w.db;
    let mut rc = None;
    if pConst.bHasAffBlob != 0 {
        let (op, pLeft, pRight) = {
            let e = db.ex(pExpr);
            (e.op, e.pLeft, e.pRight)
        };
        if (op >= TK_EQ && op <= TK_GE) || op == TK_IS {
            propagateConstantExprRewriteOne(db, pConst, pLeft, 0);
            if db.mallocFailed != 0 {
                rc = Some(WRC_Prune);
            } else if sqlite3ExprAffinity(db, pLeft) != SQLITE_AFF_TEXT {
                propagateConstantExprRewriteOne(db, pConst, pRight, 0);
            }
        }
    }
    let r = match rc {
        Some(r) => r,
        None => {
            let b = pConst.bHasAffBlob;
            propagateConstantExprRewriteOne(db, pConst, pExpr, b)
        }
    };
    w.u.any = any;
    r
}

/*
** The WHERE-clause constant propagation optimization.
**
** If the WHERE clause contains terms of the form COLUMN=CONSTANT or
** CONSTANT=COLUMN that are top-level AND-connected terms that are not
** part of a ON clause from an OUTER JOIN, then throughout the query
** replace all other occurrences of COLUMN with CONSTANT.
**
** For example, the query:
**
**      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=t1.a AND t3.c=t2.b
**
** Is transformed into
**
**      SELECT * FROM t1, t2, t3 WHERE t1.a=39 AND t2.b=39 AND t3.c=39
**
** Return true if any transformations where made and false if not.
**
** Implementation note:  Constant propagation is tricky due to affinity
** and collating sequence interactions.  Consider this example:
**
**    CREATE TABLE t1(a INT,b TEXT);
**    INSERT INTO t1 VALUES(123,'0123');
**    SELECT * FROM t1 WHERE a=123 AND b=a;
**    SELECT * FROM t1 WHERE a=123 AND b=123;
**
** The two SELECT statements above should return different answers.  b=a
** is always true because the comparison uses numeric affinity, but b=123
** is false because it uses text affinity and '0123' is not the same as '123'.
** To work around this, the expression tree is not actually changed from
** "b=a" to "b=123" but rather the "a" in "b=a" is tagged with EP_FixedCol
** and the "123" value is hung off of the pLeft pointer.  Code generator
** routines know to generate the constant "123" instead of looking up the
** column value.  Also, to avoid collation problems, this optimization is
** only attempted if the "a=123" term uses the default BINARY collation.
**
** 2021-05-25 forum post 6a06202608: Another troublesome case is...
**
**    CREATE TABLE t1(x);
**    INSERT INTO t1 VALUES(10.0);
**    SELECT 1 FROM t1 WHERE x=10 AND x LIKE 10;
**
** The query should return no rows, because the t1.x value is '10.0' not '10'
** and '10.0' is not LIKE '10'.  But if we are not careful, the first WHERE
** term "x=10" will cause the second WHERE term to become "10 LIKE 10",
** resulting in a false positive.  To avoid this, constant propagation for
** columns with BLOB affinity is only allowed if the constant is used with
** operators ==, <=, <, >=, >, or IS in a way that will cause the correct
** type conversions to occur.  See logic associated with the bHasAffBlob flag
** for details.
*/
fn propagateConstants(db: &mut Sqlite3, p: SelectId) -> i32 {
    let mut nChng = 0;
    loop {
        let mut x = WhereConst {
            nChng: 0,
            bHasAffBlob: 0,
            mExcludeOn: 0,
            apExpr: Vec::new(),
        };
        let pSrc = db.sel(p).pSrc;
        if pSrc.nn() && db.src(pSrc).nSrc() > 0 && (db.src(pSrc).a[0].fg.jointype & JT_LTORJ) != 0 {
            /* Do not propagate constants on any ON clause if there is a
             ** RIGHT JOIN anywhere in the query */
            x.mExcludeOn = EP_InnerON | EP_OuterON;
        } else {
            /* Do not propagate constants through the ON clause of a LEFT JOIN */
            x.mExcludeOn = EP_OuterON;
        }
        let pWhere = db.sel(p).pWhere;
        findConstInWhere(db, &mut x, pWhere);
        let mut chng = 0;
        if x.nConst() != 0 {
            let mut w = Walker::new(db);
            w.xExprCallback = Some(propagateConstantExprRewrite);
            w.xSelectCallback = Some(sqlite3SelectWalkNoop);
            w.xSelectCallback2 = None;
            w.walkerDepth = 0;
            w.u.any = Some(Box::new(x));
            sqlite3WalkExpr(&mut w, pWhere);
            let x2 = w.u.any.take().unwrap().downcast::<WhereConst>().unwrap();
            chng = x2.nChng;
            nChng += chng;
        }
        if chng == 0 {
            break;
        }
    }
    nChng
}

/*
** This function is called to determine whether or not it is safe to
** push WHERE clause expression pExpr down to FROM clause sub-query
** pSubq, which contains at least one window function. Return 1
** if it is safe and the expression should be pushed down, or 0
** otherwise.
**
** It is only safe to push the expression down if it consists only
** of constants and copies of expressions that appear in the PARTITION
** BY clause of all window function used by the sub-query. It is safe
** to filter out entire partitions, but not rows within partitions, as
** this may change the results of the window functions.
**
** At the time this function is called it is guaranteed that
**
**   * the sub-query uses only one distinct window frame, and
**   * that the window frame has a PARTITION BY clause.
*/
fn pushDownWindowCheck(db: &mut Sqlite3, pSubq: SelectId, pExpr: ExprId) -> i32 {
    let pWin = db.sel(pSubq).pWin;
    debug_assert!(db.win(pWin).pPartition.nn());
    debug_assert!((db.sel(pSubq).selFlags & SF_MultiPart) == 0);
    debug_assert!(db.sel(pSubq).pPrior.is_null());
    let pPart = db.win(pWin).pPartition;
    sqlite3ExprIsConstantOrGroupBy(db, pExpr, pPart)
}

/*
** Make copies of relevant WHERE clause terms of the outer query into
** the WHERE clause of subquery.  Example:
**
**    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1) WHERE x=5 AND y=10;
**
** Transformed into:
**
**    SELECT * FROM (SELECT a AS x, c-d AS y FROM t1 WHERE a=5 AND c-d=10)
**     WHERE x=5 AND y=10;
**
** The hope is that the terms added to the inner query will make it more
** efficient.
**
** NAME AMBIGUITY
**
** This optimization is called the "WHERE-clause push-down optimization"
** or sometimes the "predicate push-down optimization".
**
** Do not confuse this optimization with another unrelated optimization
** with a similar name:  The "MySQL push-down optimization" causes WHERE
** clause terms that can be evaluated using only the index and without
** reference to the table are run first, so that if they are false,
** unnecessary table seeks are avoided.
**
** RULES
**
** Do not attempt this optimization if:
**
**   (1) (** This restriction was removed on 2017-09-29.  We used to
**           disallow this optimization for aggregate subqueries, but now
**           it is allowed by putting the extra terms on the HAVING clause.
**           The added HAVING clause is pointless if the subquery lacks
**           a GROUP BY clause.  But such a HAVING clause is also harmless
**           so there does not appear to be any reason to add extra logic
**           to suppress it. **)
**
**   (2) The inner query is the recursive part of a common table expression.
**
**   (3) The inner query has a LIMIT clause (since the changes to the WHERE
**       clause would change the meaning of the LIMIT).
**
**   (4) The inner query is the right operand of a LEFT JOIN and the
**       expression to be pushed down does not come from the ON clause
**       on that LEFT JOIN.
**
**   (5) The WHERE clause expression originates in the ON or USING clause
**       of a LEFT JOIN where iCursor is not the right-hand table of that
**       left join.  An example:
**
**           SELECT *
**           FROM (SELECT 1 AS a1 UNION ALL SELECT 2) AS aa
**           JOIN (SELECT 1 AS b2 UNION ALL SELECT 2) AS bb ON (a1=b2)
**           LEFT JOIN (SELECT 8 AS c3 UNION ALL SELECT 9) AS cc ON (b2=2);
**
**       The correct answer is three rows:  (1,1,NULL),(2,2,8),(2,2,9).
**       But if the (b2=2) term were to be pushed down into the bb subquery,
**       then the (1,1,NULL) row would be suppressed.
**
**   (6) Window functions make things tricky as changes to the WHERE clause
**       of the inner query could change the window over which window
**       functions are calculated. Therefore, do not attempt the optimization
**       if:
**
**     (6a) The inner query uses multiple incompatible window partitions.
**
**     (6b) The inner query is a compound and uses window-functions.
**
**     (6c) The WHERE clause does not consist entirely of constants and
**          copies of expressions found in the PARTITION BY clause of
**          all window-functions used by the sub-query. It is safe to
**          filter out entire partitions, as this does not change the
**          window over which any window-function is calculated.
**
**   (7) The inner query is a Common Table Expression (CTE) that should
**       be materialized.  (This restriction is implemented in the calling
**       routine.)
**
**   (8) If the subquery is a compound that uses UNION, INTERSECT,
**       or EXCEPT, then all of the result set columns for all arms of
**       the compound must use the BINARY collating sequence.
**
**   (9) All three of the following are true:
**
**       (9a) The WHERE clause expression originates in the ON or USING clause
**            of a join (either an INNER or an OUTER join), and
**
**       (9b) The subquery is to the right of the ON/USING clause
**
**       (9c) There is a RIGHT JOIN (or FULL JOIN) in between the ON/USING
**            clause and the subquery.
**
**       Without this restriction, the WHERE-clause push-down optimization
**       might move the ON/USING filter expression from the left side of a
**       RIGHT JOIN over to the right side, which leads to incorrect answers.
**       See also restriction (6) in sqlite3ExprIsSingleTableConstraint().
**
**  (10) The inner query is not the right-hand table of a RIGHT JOIN.
**
**  (11) The subquery is not a VALUES clause
**
**  (12) The WHERE clause is not "rowid ISNULL" or the equivalent.  This
**       case only comes up if SQLite is compiled using
**       SQLITE_ALLOW_ROWID_IN_VIEW.
**
** Return 0 if no changes are made and non-zero if one or more WHERE clause
** terms are duplicated into the subquery.
*/
fn pushDownWhereTerms(
    db: &mut Sqlite3,
    mut pSubq: SelectId,
    mut pWhere: ExprId,
    pSrcList: SrcListId,
    iSrc: i32,
) -> i32 {
    let mut nChng = 0;
    let pSrc = SrcItemId(pSrcList, iSrc as u32);
    if pWhere.is_null() {
        return 0;
    }
    if (db.sel(pSubq).selFlags & (SF_Recursive | SF_MultiPart)) != 0 {
        return 0; /* restrictions (2) and (11) */
    }
    if (db.item(pSrc).fg.jointype & (JT_LTORJ | JT_RIGHT)) != 0 {
        return 0; /* restrictions (10) */
    }

    if db.sel(pSubq).pPrior.nn() {
        let mut notUnionAll = false;
        let mut pSel = pSubq;
        while pSel.nn() {
            let op = db.sel(pSel).op;
            debug_assert!(
                op == TK_ALL
                    || op == TK_SELECT
                    || op == TK_UNION
                    || op == TK_INTERSECT
                    || op == TK_EXCEPT
            );
            if op != TK_ALL && op != TK_SELECT {
                notUnionAll = true;
            }
            if db.sel(pSel).pWin.nn() {
                return 0; /* restriction (6b) */
            }
            pSel = db.sel(pSel).pPrior;
        }
        if notUnionAll {
            /* If any of the compound arms are connected using UNION, INTERSECT,
             ** or EXCEPT, then we must ensure that none of the columns use a
             ** non-BINARY collating sequence. */
            pSel = pSubq;
            while pSel.nn() {
                let pList = db.sel(pSel).pEList;
                debug_assert!(pList.nn());
                let n = db.el(pList).nExpr();
                for ii in 0..n {
                    let e = db.el(pList).a[ii as usize].pExpr;
                    let pColl = sqlite3ExprCollSeq(db, e);
                    if !isBinary(&pColl) {
                        return 0; /* Restriction (8) */
                    }
                }
                pSel = db.sel(pSel).pPrior;
            }
        }
    } else {
        let pWin = db.sel(pSubq).pWin;
        if pWin.nn() && db.win(pWin).pPartition.is_null() {
            return 0;
        }
    }

    if db.sel(pSubq).pLimit.nn() {
        return 0; /* restriction (3) */
    }
    while db.ex(pWhere).op == TK_AND {
        let r = db.ex(pWhere).pRight;
        nChng += pushDownWhereTerms(db, pSubq, r, pSrcList, iSrc);
        pWhere = db.ex(pWhere).pLeft;
    }

    if sqlite3ExprIsSingleTableConstraint(db, pWhere, pSrcList, iSrc, 1) != 0 {
        nChng += 1;
        db.selm(pSubq).selFlags |= SF_PushDown;
        while pSubq.nn() {
            let mut pNew = sqlite3ExprDup(db, pWhere, 0);
            unsetJoinExpr(db, pNew, -1, 1);
            let iCursor = db.item(pSrc).iCursor;
            let mut x = SubstContext {
                iTable: iCursor,
                iNewTable: iCursor,
                isOuterJoin: 0,
                nSelDepth: 0,
                pEList: db.sel(pSubq).pEList,
                pCList: findLeftmostExprlist(db, pSubq),
            };
            pNew = substExpr(db, &mut x, pNew);
            debug_assert!(pNew.nn() || db.ppr().nErr != 0);
            if db.ppr().nErr == 0 && db.ex(pNew).op == TK_IN && db.ex(pNew).UseXSelect() {
                let s = db.ex(pNew).x.pSelect;
                debug_assert!(s.nn());
                db.selm(s).selFlags |= SF_ClonedRhsIn;
                debug_assert!(db.ex(pWhere).op == TK_IN);
                debug_assert!(db.ex(pWhere).UseXSelect());
                let s2 = db.ex(pWhere).x.pSelect;
                db.selm(s2).selFlags |= SF_ClonedRhsIn;
            }
            if db.sel(pSubq).pWin.nn() && pushDownWindowCheck(db, pSubq, pNew) == 0 {
                /* Restriction 6c has prevented push-down in this case */
                sqlite3ExprDelete(db, pNew);
                nChng -= 1;
                break;
            }
            if (db.sel(pSubq).selFlags & SF_Aggregate) != 0 {
                let h = db.sel(pSubq).pHaving;
                let nh = sqlite3ExprAnd(db, h, pNew);
                db.selm(pSubq).pHaving = nh;
            } else {
                let w = db.sel(pSubq).pWhere;
                let nw = sqlite3ExprAnd(db, w, pNew);
                db.selm(pSubq).pWhere = nw;
            }
            pSubq = db.sel(pSubq).pPrior;
        }
    }
    nChng
}

/*
** Check to see if a subquery contains result-set columns that are
** never used.  If it does, change the value of those result-set columns
** to NULL so that they do not cause unnecessary work to compute.
**
** Return the number of column that were changed to NULL.
*/
fn disableUnusedSubqueryResultColumns(db: &mut Sqlite3, pItem: SrcItemId) -> i32 {
    let mut nChng = 0; /* Number of columns converted to NULL */

    if db.item(pItem).fg.isCorrelated != 0 || db.item(pItem).fg.isCte != 0 {
        return 0;
    }
    let pTab = itemTab(db, pItem);
    debug_assert!(db.item(pItem).fg.isSubquery != 0);
    let pSub = subqSelect(db, pItem);
    debug_assert!(db.el(db.sel(pSub).pEList).nExpr() == pTab.borrow().nCol as i32);
    let mut pX = pSub;
    while pX.nn() {
        let s = db.sel(pX);
        if (s.selFlags & (SF_Distinct | SF_Aggregate)) != 0 {
            return 0;
        }
        if s.pPrior.nn() && s.op != TK_ALL {
            /* This optimization does not work for compound subqueries that
             ** use UNION, INTERSECT, or EXCEPT.  Only UNION ALL is allowed. */
            return 0;
        }
        if s.pWin.nn() {
            /* This optimization does not work for subqueries that use window
             ** functions. */
            return 0;
        }
        pX = s.pPrior;
    }
    let mut colUsed = db.item(pItem).colUsed;
    let pList = db.sel(pSub).pOrderBy;
    if pList.nn() {
        for it in db.el(pList).a.iter() {
            let mut iCol = it.u.x.iOrderByCol as i32;
            if iCol > 0 {
                iCol -= 1;
                colUsed |= (1 as Bitmask) << (if iCol >= BMS { BMS - 1 } else { iCol });
            }
        }
    }
    let nCol = pTab.borrow().nCol as i32;
    for j in 0..nCol {
        let m: Bitmask = if j < BMS - 1 { MASKBIT(j) } else { TOPBIT };
        if (m & colUsed) != 0 {
            continue;
        }
        let mut pX = pSub;
        while pX.nn() {
            let pY = db.el(db.sel(pX).pEList).a[j as usize].pExpr;
            if db.ex(pY).op != TK_NULL {
                let e = db.exm(pY);
                e.op = TK_NULL;
                e.ClearProperty(EP_Skip | EP_Unlikely);
                db.selm(pX).selFlags |= SF_PushDown;
                nChng += 1;
            }
            pX = db.sel(pX).pPrior;
        }
    }
    nChng
}

/*
** The pFunc is the only aggregate function in the query.  Check to see
** if the query is a candidate for the min/max optimization.
**
** If the query is a candidate for the min/max optimization, then set
** *ppMinMax to be an ORDER BY clause to be used for the optimization
** and return either WHERE_ORDERBY_MIN or WHERE_ORDERBY_MAX depending on
** whether pFunc is a min() or max() function.
**
** If the query is not a candidate for the min/max optimization, return
** WHERE_ORDERBY_NORMAL (which must be zero).
**
** This routine must be called after aggregate functions have been
** located but before their arguments have been subjected to aggregate
** analysis.
*/
fn minMaxQuery(db: &mut Sqlite3, pFunc: ExprId, ppMinMax: &mut ExprListId) -> u8 {
    let eRet;
    let mut sortFlags: u8 = 0;

    debug_assert!(ppMinMax.is_null());
    let (pEList, isWin, zFunc) = {
        let e = db.ex(pFunc);
        debug_assert!(e.op == TK_AGG_FUNCTION);
        debug_assert!(e.UseXList());
        (e.x.pList, e.HasProperty(EP_WinFunc), e.u.zToken.clone())
    };
    if pEList.is_null()
        || db.el(pEList).nExpr() != 1
        || isWin
        || db.OptimizationDisabled(SQLITE_MinMaxOpt)
    {
        return WHERE_ORDERBY_NORMAL as u8;
    }
    if crate::util::sqlite3StrICmp(zFunc.as_deref(), Some(b"min")) == 0 {
        eRet = WHERE_ORDERBY_MIN as u8;
        let a0 = db.el(pEList).a[0].pExpr;
        if sqlite3ExprCanBeNull(db, a0) != 0 {
            sortFlags = KEYINFO_ORDER_BIGNULL;
        }
    } else if crate::util::sqlite3StrICmp(zFunc.as_deref(), Some(b"max")) == 0 {
        eRet = WHERE_ORDERBY_MAX as u8;
        sortFlags = KEYINFO_ORDER_DESC;
    } else {
        return WHERE_ORDERBY_NORMAL as u8;
    }
    let pOrderBy = sqlite3ExprListDup(db, pEList, 0);
    *ppMinMax = pOrderBy;
    debug_assert!(pOrderBy.nn() || db.mallocFailed != 0);
    if pOrderBy.nn() {
        db.elm(pOrderBy).a[0].fg.sortFlags = sortFlags;
    }
    eRet
}

/*
** The select statement passed as the first argument is an aggregate query.
** The second argument is the associated aggregate-info object. This
** function tests if the SELECT is of the form:
**
**   SELECT count(*) FROM <tbl>
**
** where table is a database table, not a sub-select or view. If the query
** does match this pattern, then a pointer to the Table object representing
** <tbl> is returned. Otherwise, NULL is returned.
**
** This routine checks to see if it is safe to use the count optimization.
** A correct answer is still obtained (though perhaps more slowly) if
** this routine returns NULL when it could have returned a table pointer.
** But returning the pointer when NULL should have been returned can
** result in incorrect answers and/or crashes.  So, when in doubt, return NULL.
*/
fn isSimpleCount(db: &Sqlite3, p: SelectId, pAggInfo: AggInfoId) -> Option<TableRef> {
    let s = db.sel(p);
    debug_assert!(s.pGroupBy.is_null());
    let ai = db.arena.agg(pAggInfo);

    if s.pWhere.nn()
        || db.el(s.pEList).nExpr() != 1
        || db.src(s.pSrc).nSrc() != 1
        || db.src(s.pSrc).a[0].fg.isSubquery != 0
        || ai.nFunc() != 1
        || s.pHaving.nn()
    {
        return None;
    }
    let pTab = db.src(s.pSrc).a[0].pSTab.clone()?;
    debug_assert!(!pTab.borrow().IsView());
    if !pTab.borrow().IsOrdinaryTable() {
        return None;
    }
    let pExpr = db.el(s.pEList).a[0].pExpr;
    debug_assert!(pExpr.nn());
    let e = db.ex(pExpr);
    if e.op != TK_AGG_FUNCTION {
        return None;
    }
    if e.pAggInfo != pAggInfo {
        return None;
    }
    match &ai.aFunc[0].pFunc {
        Some(f) if (f.funcFlags & SQLITE_FUNC_COUNT) != 0 => {}
        _ => return None,
    }
    debug_assert!(ai.aFunc[0].pFExpr == pExpr);
    if e.HasProperty(EP_Distinct | EP_WinFunc) {
        return None;
    }

    Some(pTab)
}

/*
** If the source-list item passed as an argument was augmented with an
** INDEXED BY clause, then try to locate the specified index. If there
** was such a clause and the named index cannot be found, return
** SQLITE_ERROR and leave an error in pParse. Otherwise, populate
** pFrom->pIndex and return SQLITE_OK.
*/
pub fn sqlite3IndexedByLookup(db: &mut Sqlite3, pFrom: SrcItemId) -> i32 {
    let pTab = itemTab(db, pFrom);
    let zIndexedBy = db.item(pFrom).u1.zIndexedBy.clone();
    debug_assert!(db.item(pFrom).fg.isIndexedBy != 0);

    let pIdx = pTab
        .borrow()
        .pIndex
        .iter()
        .find(|x| crate::util::sqlite3StrICmp(Some(&x.borrow().zName), zIndexedBy.as_deref()) == 0)
        .cloned();
    match pIdx {
        None => {
            errmsg!(db, b"no such index: %s", zIndexedBy.as_deref(), 0);
            db.pp().checkSchema = 1;
            SQLITE_ERROR
        }
        Some(idx) => {
            debug_assert!(db.item(pFrom).fg.isCte == 0);
            db.itemm(pFrom).u2.pIBIndex = Some(idx);
            SQLITE_OK
        }
    }
}

/*
** Detect compound SELECT statements that use an ORDER BY clause with
** an alternative collating sequence.
**
**    SELECT ... FROM t1 EXCEPT SELECT ... FROM t2 ORDER BY .. COLLATE ...
**
** These are rewritten as a subquery:
**
**    SELECT * FROM (SELECT ... FROM t1 EXCEPT SELECT ... FROM t2)
**     ORDER BY ... COLLATE ...
**
** This transformation is necessary because the multiSelectByMerge() routine
** above that generates the code for a compound SELECT with an ORDER BY clause
** uses a merge algorithm that requires the same collating sequence on the
** result columns as on the ORDER BY clause.  See ticket
** http://sqlite.org/src/info/6709574d2a
**
** This transformation is only needed for EXCEPT, INTERSECT, and UNION.
** The UNION ALL operator works fine with multiSelectByMerge() even when
** there are COLLATE terms in the ORDER BY.
*/
fn convertCompoundSelectToSubquery(w: &mut Walker, p: SelectId) -> i32 {
    let db = &mut *w.db;
    if db.sel(p).pPrior.is_null() {
        return WRC_Continue;
    }
    if db.sel(p).pOrderBy.is_null() {
        return WRC_Continue;
    }
    let mut pX = p;
    while pX.nn() && (db.sel(pX).op == TK_ALL || db.sel(pX).op == TK_SELECT) {
        pX = db.sel(pX).pPrior;
    }
    if pX.is_null() {
        return WRC_Continue;
    }
    let a = db.sel(p).pOrderBy;
    /* If iOrderByCol is already non-zero, then it has already been matched
     ** to a result column of the SELECT statement. This occurs when the
     ** SELECT is rewritten for window-functions processing and then passed
     ** to sqlite3SelectPrep() and similar a second time. The rewriting done
     ** by this function is not required in this case. */
    if db.el(a).a[0].u.x.iOrderByCol != 0 {
        return WRC_Continue;
    }
    let mut i = db.el(a).nExpr() - 1;
    while i >= 0 {
        let e = db.el(a).a[i as usize].pExpr;
        if (db.ex(e).flags & EP_Collate) != 0 {
            break;
        }
        i -= 1;
    }
    if i < 0 {
        return WRC_Continue;
    }

    /* If we reach this point, that means the transformation is required. */

    let pNew = db.arena.new_select(Select::default());
    let dummy = Token::default();
    let pNewSrc = sqlite3SrcListAppendFromTerm(db, SrcListId::NULL, None, None, &dummy, pNew, None);
    debug_assert!(pNewSrc.nn() || db.ppr().nErr != 0);
    if db.ppr().nErr != 0 {
        sqlite3SrcListDelete(db, pNewSrc);
        return WRC_Abort;
    }
    let copy = db.sel(p).clone();
    *db.selm(pNew) = copy;
    let ast = sqlite3Expr(db, TK_ASTERISK as i32, None);
    let el = sqlite3ExprListAppend(db, ExprListId::NULL, ast);
    {
        let s = db.selm(p);
        s.pSrc = pNewSrc;
        s.pEList = el;
        s.op = TK_SELECT;
        s.pWhere = ExprId::NULL;
        s.pPrior = SelectId::NULL;
        s.pNext = SelectId::NULL;
        s.pWith = WithId::NULL;
        s.pWinDefn = WindowId::NULL;
        s.selFlags &= !SF_Compound;
        debug_assert!((s.selFlags & SF_Converted) == 0);
        s.selFlags |= SF_Converted;
    }
    {
        let n = db.selm(pNew);
        n.pGroupBy = ExprListId::NULL;
        n.pHaving = ExprId::NULL;
        n.pOrderBy = ExprListId::NULL;
        n.pLimit = ExprId::NULL;
    }
    let pp = db.sel(pNew).pPrior;
    debug_assert!(pp.nn());
    db.selm(pp).pNext = pNew;
    WRC_Continue
}

/*
** Check to see if the FROM clause term pFrom has table-valued function
** arguments.  If it does, leave an error message in pParse and return
** non-zero, since pFrom is not allowed to be a table-valued function.
*/
fn cannotBeFunction(db: &mut Sqlite3, pFrom: SrcItemId) -> i32 {
    if db.item(pFrom).fg.isTabFunc != 0 {
        let z = db.item(pFrom).zName.clone();
        errmsg!(db, b"'%s' is not a function", z.as_deref());
        return 1;
    }
    0
}

/*
** Argument pWith (which may be NULL) points to a linked list of nested
** WITH contexts, from inner to outermost. If the table identified by
** FROM clause element pItem is really a common-table-expression (CTE)
** then return a pointer to the CTE definition for that table. Otherwise
** return NULL.
**
** If a non-NULL value is returned, set *ppContext to point to the With
** object that the returned CTE belongs to.
**
** Rust: returns (the With, index of the CTE in With::a).
*/
fn searchWith(db: &Sqlite3, pWith: WithId, pItem: SrcItemId) -> Option<(WithId, usize)> {
    let zName = db.item(pItem).zName.clone();
    debug_assert!(zName.is_some());
    let mut p = pWith;
    while p.nn() {
        let w = db.arena.with(p);
        for (i, c) in w.a.iter().enumerate() {
            if crate::util::sqlite3StrICmp(zName.as_deref(), Some(&c.zName)) == 0 {
                return Some((p, i));
            }
        }
        if w.bView != 0 {
            break;
        }
        p = w.pOuter;
    }
    None
}

/* The code generator maintains a stack of active WITH clauses
** with the inner-most WITH clause being at the top of the stack.
**
** This routine pushes the WITH clause passed as the second argument
** onto the top of the stack. If argument bFree is true, then this
** WITH clause will never be popped from the stack but should instead
** be freed along with the Parse object. In other cases, when
** bFree==0, the With object will be freed along with the SELECT
** statement with which it is associated.
**
** This routine returns a copy of pWith.  Or, if bFree is true and
** the pWith object is destroyed immediately due to an OOM condition,
** then this routine return NULL.
**
** If bFree is true, do not continue to use the pWith pointer after
** calling this routine,  Instead, use only the return value.
*/
pub fn sqlite3WithPush(db: &mut Sqlite3, pWith: WithId, bFree: u8) -> WithId {
    let _ = bFree; /* (the With is freed with the arena) */
    if pWith.nn() && db.ppr().nErr == 0 {
        debug_assert!(db.ppr().pWith != pWith);
        let outer = db.ppr().pWith;
        db.arena.withm(pWith).pOuter = outer;
        db.pp().pWith = pWith;
    }
    pWith
}

/*
** This function checks to see if argument pFrom refers to a CTE declared by
** a WITH clause on the stack currently maintained by the parser (on the
** pParse->pWith linked list).  And if currently processing a CTE expression,
** it also checks to see if the reference is a recursive reference to the CTE.
**
** If pFrom matches a CTE, pFrom->pSTab and other fields are populated
** accordingly.
**
** Return 0 if no match is found.
** Return 1 if a match is found.
** Return 2 if an error condition is detected.
*/
fn resolveFromTermToCte(pWalker: &mut Walker, pFrom: SrcItemId) -> i32 {
    let db = &mut *pWalker.db;
    debug_assert!(db.item(pFrom).pSTab.is_none());
    if db.ppr().pWith.is_null() {
        /* There are no WITH clauses in the stack.  No match is possible */
        return 0;
    }
    if db.ppr().nErr != 0 {
        /* Prior errors might have left pParse->pWith in a goofy state, so
         ** go no further. */
        return 0;
    }
    {
        let f = db.item(pFrom);
        debug_assert!(f.fg.hadSchema == 0 || f.fg.notCte != 0);
        if f.fg.fixedSchema == 0 && f.u4.zDatabase.is_some() {
            /* The FROM term contains a schema qualifier (ex: main.t1) and so
             ** it cannot possibly be a CTE reference. */
            return 0;
        }
        if f.fg.notCte != 0 {
            /* The FROM term is specifically excluded from matching a CTE.
             **   (1)  It is part of a trigger that used to have zDatabase but had
             **        zDatabase removed by sqlite3FixTriggerStep().
             **   (2)  This is the first term in the FROM clause of an UPDATE.
             */
            return 0;
        }
    }
    let pw = db.ppr().pWith;
    let (pWith, iCte) = match searchWith(db, pw, pFrom) {
        Some(x) => x,
        None => return 0, /* No match */
    };
    let mut iRecTab = -1; /* Cursor for recursive table */
    let cte = db.arena.with(pWith).a[iCte].clone();

    /* If pCte->zCteErr is non-NULL at this point, then this is an illegal
     ** recursive reference to CTE pCte. Leave an error in pParse and return
     ** early. If pCte->zCteErr is NULL, then this is not a recursive reference.
     ** In this case, proceed.  */
    if let Some(zErr) = cte.zCteErr {
        errmsg!(db, zErr.as_bytes(), &cte.zName);
        return 2;
    }
    if cannotBeFunction(db, pFrom) != 0 {
        return 2;
    }

    debug_assert!(db.item(pFrom).pSTab.is_none());
    let pTab = newTable();
    let mut pCteUse = cte.pUse;
    if pCteUse.is_null() {
        pCteUse = db.arena.new_cteuse(CteUse {
            eM10d: cte.eM10d,
            ..CteUse::default()
        });
        db.arena.withm(pWith).a[iCte].pUse = pCteUse;
    }
    db.itemm(pFrom).pSTab = Some(pTab.clone());
    {
        let mut t = pTab.borrow_mut();
        t.nTabRef = 1;
        t.zName = cte.zName.clone();
        t.iPKey = -1;
        t.nRowLogEst = 200;
        t.tabFlags |= TF_Ephemeral | TF_NoVisibleRowid;
    }
    sqlite3SrcItemAttachSubquery(db, pFrom, cte.pSelect, 1);
    if db.mallocFailed != 0 {
        return 2;
    }
    debug_assert!(db.item(pFrom).fg.isSubquery != 0 && db.item(pFrom).u4.pSubq.is_some());
    let pSel = subqSelect(db, pFrom);
    debug_assert!(pSel.nn());
    db.selm(pSel).selFlags |= SF_CopyCte;
    if db.item(pFrom).fg.isIndexedBy != 0 {
        let z = db.item(pFrom).u1.zIndexedBy.clone();
        errmsg!(db, b"no such index: \"%s\"", z.as_deref());
        return 2;
    }
    debug_assert!(db.item(pFrom).fg.isIndexedBy == 0);
    {
        let f = db.itemm(pFrom);
        f.fg.isCte = 1;
        f.u2.pCteUse = pCteUse;
    }
    db.arena.cteusem(pCteUse).nUse += 1;

    /* Check if this is a recursive CTE. */
    let mut pRecTerm = pSel;
    let selOp = db.sel(pSel).op;
    let bMayRecursive = selOp == TK_ALL || selOp == TK_UNION;
    while bMayRecursive && db.sel(pRecTerm).op == selOp {
        let pSrc = db.sel(pRecTerm).pSrc;
        debug_assert!(db.sel(pRecTerm).pPrior.nn());
        let n = db.src(pSrc).nSrc();
        for i in 0..n {
            let pItem = SrcItemId(pSrc, i as u32);
            let matches = {
                let it = db.item(pItem);
                it.zName.is_some()
                    && it.fg.hadSchema == 0
                    && it.fg.isSubquery == 0
                    && (it.fg.fixedSchema != 0 || it.u4.zDatabase.is_none())
                    && crate::util::sqlite3StrICmp(it.zName.as_deref(), Some(&cte.zName)) == 0
            };
            if matches {
                db.itemm(pItem).pSTab = Some(pTab.clone());
                pTab.borrow_mut().nTabRef += 1;
                db.itemm(pItem).fg.isRecursive = 1;
                if (db.sel(pRecTerm).selFlags & SF_Recursive) != 0 {
                    errmsg!(
                        db,
                        b"multiple references to recursive table: %s",
                        &cte.zName
                    );
                    return 2;
                }
                db.selm(pRecTerm).selFlags |= SF_Recursive;
                if iRecTab < 0 {
                    iRecTab = db.ppr().nTab;
                    db.pp().nTab += 1;
                }
                db.itemm(pItem).iCursor = iRecTab;
            }
        }
        if (db.sel(pRecTerm).selFlags & SF_Recursive) == 0 {
            break;
        }
        pRecTerm = db.sel(pRecTerm).pPrior;
    }

    if db.ppr().nTab >= db.aLimit[SQLITE_LIMIT_EXPR_DEPTH as usize] {
        /* Bug 2026-10-04T05:35:14Z: Prevent nested CTEs from generating
         ** an exponential number of cursors.  The error message here will
         ** be "Nested too deep", which isn't exactly correct, but it is
         ** sufficient, and we don't want to use extra code space for more
         ** detail on such an obscure error. */
        let nt = db.ppr().nTab;
        db.pp().nNestSel = nt;
    }
    db.pp().nNestSel += 1;
    if checkSubqueryNestingDepth(db) != 0 {
        return 2;
    }
    db.arena.withm(pWith).a[iCte].zCteErr = Some("circular reference: %s");
    let pSavedWith = db.ppr().pWith;
    db.pp().pWith = pWith;
    if (db.sel(pSel).selFlags & SF_Recursive) != 0 {
        debug_assert!(pRecTerm.nn());
        debug_assert!((db.sel(pRecTerm).selFlags & SF_Recursive) == 0);
        debug_assert!(db.sel(pRecTerm).pNext.nn());
        debug_assert!((db.sel(db.sel(pRecTerm).pNext).selFlags & SF_Recursive) != 0);
        debug_assert!(db.sel(pRecTerm).pWith.is_null());
        let sw = db.sel(pSel).pWith;
        db.selm(pRecTerm).pWith = sw;
        let rc = sqlite3WalkSelect(pWalker, pRecTerm);
        let db = &mut *pWalker.db;
        db.selm(pRecTerm).pWith = WithId::NULL;
        if rc != 0 {
            db.pp().pWith = pSavedWith;
            return 2;
        }
    } else if sqlite3WalkSelect(pWalker, pSel) != 0 {
        pWalker.db.pp().pWith = pSavedWith;
        return 2;
    }
    let db = &mut *pWalker.db;
    db.pp().pWith = pWith;
    db.pp().nNestSel -= 1;
    debug_assert!(db.ppr().nNestSel >= 0);

    let mut pLeft = pSel;
    while db.sel(pLeft).pPrior.nn() {
        pLeft = db.sel(pLeft).pPrior;
    }
    let mut pEList = db.sel(pLeft).pEList;
    if cte.pCols.nn() {
        if pEList.nn() && db.el(pEList).nExpr() != db.el(cte.pCols).nExpr() {
            let (a, b) = (db.el(pEList).nExpr(), db.el(cte.pCols).nExpr());
            errmsg!(
                db,
                b"table %s has %d values for %d columns",
                &cte.zName,
                a,
                b
            );
            db.pp().pWith = pSavedWith;
            return 2;
        }
        pEList = cte.pCols;
    }

    let mut nCol: i16 = 0;
    let mut aCol: Vec<Column> = Vec::new();
    sqlite3ColumnsFromExprList(db, pEList, &mut nCol, &mut aCol);
    {
        let mut t = pTab.borrow_mut();
        t.nCol = nCol;
        t.aCol = aCol;
    }
    if bMayRecursive {
        if (db.sel(pSel).selFlags & SF_Recursive) != 0 {
            db.arena.withm(pWith).a[iCte].zCteErr = Some("multiple recursive references: %s");
        } else {
            db.arena.withm(pWith).a[iCte].zCteErr = Some("recursive reference in a subquery: %s");
        }
        sqlite3WalkSelect(pWalker, pSel);
    }
    let db = &mut *pWalker.db;
    db.arena.withm(pWith).a[iCte].zCteErr = None;
    db.pp().pWith = pSavedWith;
    1 /* Success */
}

/*
** If the SELECT passed as the second argument has an associated WITH
** clause, pop it from the stack stored as part of the Parse object.
**
** This function is used as the xSelectCallback2() callback by
** sqlite3SelectExpand() when walking a SELECT tree to resolve table
** names and other FROM clause elements.
*/
pub fn sqlite3SelectPopWith(w: &mut Walker, p: SelectId) {
    let db = &mut *w.db;
    if db.ppr().pWith.nn() && db.sel(p).pPrior.is_null() {
        let r = findRightmost(db, p);
        let pWith = db.sel(r).pWith;
        if pWith.nn() {
            debug_assert!(db.ppr().pWith == pWith || db.ppr().nErr != 0);
            let o = db.arena.with(pWith).pOuter;
            db.pp().pWith = o;
        }
    }
}

/*
** The SrcItem structure passed as the second argument represents a
** sub-query in the FROM clause of a SELECT statement. This function
** allocates and populates the SrcItem.pTab object. If successful,
** SQLITE_OK is returned. Otherwise, if an OOM error is encountered,
** SQLITE_NOMEM.
*/
pub fn sqlite3ExpandSubquery(db: &mut Sqlite3, pFrom: SrcItemId) -> i32 {
    debug_assert!(db.item(pFrom).fg.isSubquery != 0);
    debug_assert!(db.item(pFrom).u4.pSubq.is_some());
    let mut pSel = subqSelect(db, pFrom);
    debug_assert!(pSel.nn());
    let pTab = newTable();
    db.itemm(pFrom).pSTab = Some(pTab.clone());
    pTab.borrow_mut().nTabRef = 1;
    let zAlias = db.item(pFrom).zAlias.clone();
    let zName = match zAlias {
        Some(a) => a,
        None => mprintf!(db, b"%!S", pFrom),
    };
    pTab.borrow_mut().zName = zName;
    while db.sel(pSel).pPrior.nn() {
        pSel = db.sel(pSel).pPrior;
    }
    let pEList = db.sel(pSel).pEList;
    let mut nCol: i16 = 0;
    let mut aCol: Vec<Column> = Vec::new();
    sqlite3ColumnsFromExprList(db, pEList, &mut nCol, &mut aCol);
    {
        let mut t = pTab.borrow_mut();
        t.nCol = nCol;
        t.aCol = aCol;
        t.iPKey = -1;
        t.eTabType = TABTYP_VIEW;
        t.nRowLogEst = 200;
        /* The usual case - do not allow ROWID on a subquery */
        t.tabFlags |= TF_Ephemeral | TF_NoVisibleRowid;
    }
    if db.ppr().nErr != 0 {
        SQLITE_ERROR
    } else {
        SQLITE_OK
    }
}

/*
** Check the N SrcItem objects to the right of pBase.  (N might be zero!)
** If any of those SrcItem objects have a USING clause containing zName
** then return true.
**
** If N is zero, or none of the N SrcItem objects to the right of pBase
** contains a USING clause, or if none of the USING clauses contain zName,
** then return false.
*/
fn inAnyUsingClause(db: &mut Sqlite3, zName: &[u8], pBase: SrcItemId, mut N: i32) -> bool {
    let mut k = pBase.1;
    while N > 0 {
        N -= 1;
        k += 1;
        let (isUsing, pUsing) = {
            let it = db.item(SrcItemId(pBase.0, k));
            (it.fg.isUsing, it.u3.pUsing)
        };
        if isUsing == 0 {
            continue;
        }
        if pUsing.is_null() {
            continue;
        }
        if sqlite3IdListIndex(db, pUsing, Some(zName)) >= 0 {
            return true;
        }
    }
    false
}

/*
** This routine is a Walker callback for "expanding" a SELECT statement.
** "Expanding" means to do the following:
**
**    (1)  Make sure VDBE cursor numbers have been assigned to every
**         element of the FROM clause.
**
**    (2)  Fill in the pTabList->a[].pTab fields in the SrcList that
**         defines FROM clause.  When views appear in the FROM clause,
**         fill pTabList->a[].pSelect with a copy of the SELECT statement
**         that implements the view.  A copy is made of the view's SELECT
**         statement so that we can freely modify or delete that statement
**         without worrying about messing up the persistent representation
**         of the view.
**
**    (3)  Add terms to the WHERE clause to accommodate the NATURAL keyword
**         on joins and the ON and USING clause of joins.
**
**    (4)  Scan the list of columns in the result set (pEList) looking
**         for instances of the "*" operator or the TABLE.* operator.
**         If found, expand each "*" to be every column in every table
**         and TABLE.* to be every column in TABLE.
**
*/
fn selectExpander(pWalker: &mut Walker, p: SelectId) -> i32 {
    let db = &mut *pWalker.db;
    let selFlags: u32 = (db.sel(p).selFlags as u16) as u32;
    let mut elistFlags: u32 = 0;

    db.selm(p).selFlags |= SF_Expanded;
    if db.mallocFailed != 0 {
        return WRC_Abort;
    }
    debug_assert!(db.sel(p).pSrc.nn());
    if (selFlags & SF_Expanded) != 0 {
        return WRC_Prune;
    }
    if pWalker.eCode != 0 {
        /* Renumber selId because it has been copied from a view */
        db.pp().nSelect += 1;
        db.selm(p).selId = db.ppr().nSelect as u32;
    }
    let pTabList = db.sel(p).pSrc;
    let pEList = db.sel(p).pEList;
    if db.ppr().pWith.nn() && (db.sel(p).selFlags & SF_View) != 0 {
        if db.sel(p).pWith.is_null() {
            let w = db.arena.new_with(With::default());
            db.selm(p).pWith = w;
        }
        let w = db.sel(p).pWith;
        db.arena.withm(w).bView = 1;
    }
    let pw = db.sel(p).pWith;
    sqlite3WithPush(db, pw, 0);

    /* Make sure cursor numbers have been assigned to all entries in
     ** the FROM clause of the SELECT statement.
     */
    sqlite3SrcListAssignCursors(db, pTabList);

    /* Look up every table named in the FROM clause of the select.  If
     ** an entry of the FROM clause is a subquery instead of a table or view,
     ** then create a transient table structure to describe the subquery.
     */
    let nSrc = pWalker.db.src(pTabList).nSrc();
    for i in 0..nSrc {
        let pFrom = SrcItemId(pTabList, i as u32);
        let db = &mut *pWalker.db;
        debug_assert!(db.item(pFrom).fg.isRecursive == 0 || db.item(pFrom).pSTab.is_some());
        if db.item(pFrom).pSTab.is_some() {
            continue;
        }
        debug_assert!(db.item(pFrom).fg.isRecursive == 0);
        if db.item(pFrom).zName.is_none() {
            /* A sub-query in the FROM clause of a SELECT */
            debug_assert!(db.item(pFrom).fg.isSubquery != 0 && db.item(pFrom).u4.pSubq.is_some());
            let pSel = subqSelect(db, pFrom);
            debug_assert!(pSel.nn());
            if sqlite3WalkSelect(pWalker, pSel) != 0 {
                return WRC_Abort;
            }
            if sqlite3ExpandSubquery(pWalker.db, pFrom) != 0 {
                return WRC_Abort;
            }
        } else {
            let rc = resolveFromTermToCte(pWalker, pFrom);
            if rc != 0 {
                if rc > 1 {
                    return WRC_Abort;
                }
                debug_assert!(pWalker.db.item(pFrom).pSTab.is_some());
            } else {
                /* An ordinary table or view name in the FROM clause */
                let db = &mut *pWalker.db;
                debug_assert!(db.item(pFrom).pSTab.is_none());
                let pTab = match sqlite3LocateTableItem(db, 0, pFrom) {
                    Some(t) => t,
                    None => return WRC_Abort,
                };
                db.itemm(pFrom).pSTab = Some(pTab.clone());
                if pTab.borrow().nTabRef >= 0xffff {
                    let z = pTab.borrow().zName.clone();
                    errmsg!(db, b"too many references to \"%s\": max 65535", &z);
                    db.itemm(pFrom).pSTab = None;
                    return WRC_Abort;
                }
                pTab.borrow_mut().nTabRef += 1;
                if !pTab.borrow().IsVirtual() && cannotBeFunction(db, pFrom) != 0 {
                    return WRC_Abort;
                }
                if !pTab.borrow().IsOrdinaryTable() {
                    let eCodeOrig = pWalker.eCode;
                    let db = &mut *pWalker.db;
                    if sqlite3ViewGetColumnNames(db, &pTab) != 0 {
                        return WRC_Abort;
                    }
                    debug_assert!(db.item(pFrom).fg.isSubquery == 0);
                    if pTab.borrow().IsView() {
                        let pSchema = pTab.borrow().pSchema;
                        if (db.flags & SQLITE_EnableView) == 0
                            && pSchema != db.aDb[1].pSchema.borrow().id
                        {
                            let z = pTab.borrow().zName.clone();
                            errmsg!(db, b"access to view \"%s\" prohibited", &z);
                        }
                        let t = pTab.borrow().u.view.pSelect.clone();
                        let vs = db.tree(&t);
                        sqlite3SrcItemAttachSubquery(db, pFrom, vs, 1);
                    } else if pTab.borrow().IsVirtual()
                        && (db.item(pFrom).fg.fromDDL != 0
                            || (db.ppr().prepFlags as i32 & SQLITE_PREPARE_FROM_DDL) != 0)
                    {
                        let risk = crate::vtab::sqlite3GetVTable(db, &pTab)
                            .map_or(0, |v| v.borrow().eVtabRisk as i32);
                        if risk > ((db.flags & SQLITE_TrustedSchema) != 0) as i32 {
                            let z = pTab.borrow().zName.clone();
                            errmsg!(db, b"unsafe use of virtual table \"%s\"", &z);
                        }
                    }
                    let nCol = pTab.borrow().nCol;
                    pTab.borrow_mut().nCol = -1;
                    pWalker.eCode = 1; /* Turn on Select.selId renumbering */
                    if pWalker.db.item(pFrom).fg.isSubquery != 0 {
                        let s = subqSelect(pWalker.db, pFrom);
                        sqlite3WalkSelect(pWalker, s);
                    }
                    pWalker.eCode = eCodeOrig;
                    pTab.borrow_mut().nCol = nCol;
                }
            }
        }

        /* Locate the index named by the INDEXED BY clause, if any. */
        let db = &mut *pWalker.db;
        if db.item(pFrom).fg.isIndexedBy != 0 && sqlite3IndexedByLookup(db, pFrom) != 0 {
            return WRC_Abort;
        }
    }

    /* Process NATURAL keywords, and ON and USING clauses of joins.
     */
    let db = &mut *pWalker.db;
    debug_assert!(db.mallocFailed == 0 || db.ppr().nErr != 0);
    if db.ppr().nErr != 0 || sqlite3ProcessJoin(db, p) != 0 {
        return WRC_Abort;
    }

    /* For every "*" that occurs in the column list, insert the names of
     ** all columns in all tables.  And for every TABLE.* insert the names
     ** of all columns in TABLE.  The parser inserted a special expression
     ** with the TK_ASTERISK operator for each "*" that it found in the column
     ** list.  The following code just has to locate the TK_ASTERISK
     ** expressions and expand each one to the list of all columns in
     ** all tables.
     **
     ** The first loop just checks to see if there are any "*" operators
     ** that need expanding.
     */
    let nE = db.el(pEList).nExpr();
    let mut k = 0;
    while k < nE {
        let pE = db.el(pEList).a[k as usize].pExpr;
        let e = db.ex(pE);
        if e.op == TK_ASTERISK {
            break;
        }
        debug_assert!(e.op != TK_DOT || e.pRight.nn());
        if e.op == TK_DOT && db.ex(e.pRight).op == TK_ASTERISK {
            break;
        }
        elistFlags |= e.flags;
        k += 1;
    }
    if k < nE {
        /*
         ** If we get here it means the result set contains one or more "*"
         ** operators that need to be expanded.  Loop through each expression
         ** in the result set and expand them one by one.
         */
        let mut pNew = ExprListId::NULL;
        let flags = db.flags as i32;
        let longNames =
            (flags as u64 & SQLITE_FullColNames) != 0 && (flags as u64 & SQLITE_ShortColNames) == 0;

        for k in 0..nE {
            let pE = db.el(pEList).a[k as usize].pExpr;
            let (eop, eflags, pERight, pELeft, eOfst) = {
                let e = db.ex(pE);
                (e.op, e.flags, e.pRight, e.pLeft, e.w.iOfst)
            };
            elistFlags |= eflags;
            let pRightOp = if pERight.nn() { db.ex(pERight).op } else { 0 };
            debug_assert!(eop != TK_DOT || pERight.nn());
            if eop != TK_ASTERISK && (eop != TK_DOT || pRightOp != TK_ASTERISK) {
                /* This particular expression does not need to be expanded.
                 */
                pNew = sqlite3ExprListAppend(db, pNew, pE);
                if pNew.nn() {
                    let (zEName, eEName) = {
                        let it = &mut db.elm(pEList).a[k as usize];
                        (it.zEName.take(), it.fg.eEName)
                    };
                    let last = db.el(pNew).a.len() - 1;
                    let it = &mut db.elm(pNew).a[last];
                    it.zEName = zEName;
                    it.fg.eEName = eEName;
                }
                db.elm(pEList).a[k as usize].pExpr = ExprId::NULL;
            } else {
                /* This expression is a "*" or a "TABLE.*" and needs to be
                 ** expanded. */
                let mut tableSeen = false; /* Set to 1 when TABLE matches */
                let mut zTName: Option<ZStr> = None; /* text of name of TABLE */
                let iErrOfst;
                if eop == TK_DOT {
                    debug_assert!((selFlags & SF_NestedFrom) == 0);
                    debug_assert!(pELeft.nn());
                    debug_assert!(!db.ex(pELeft).HasProperty(EP_IntValue));
                    zTName = db.ex(pELeft).u.zToken.clone();
                    iErrOfst = db.ex(pERight).w.iOfst;
                } else {
                    iErrOfst = eOfst;
                }
                let nSrc = db.src(pTabList).nSrc();
                for i in 0..nSrc {
                    let pFrom = SrcItemId(pTabList, i as u32);
                    let pTab = itemTab(db, pFrom); /* Table for this data source */
                    let pNestedFrom; /* Result-set of a nested FROM clause */
                    let mut zSchemaName: Option<ZStr> = None; /* Schema name for this data source */
                    let pUsing; /* USING clause for pFrom[1] */

                    let zTabName: ZStr = match &db.item(pFrom).zAlias {
                        Some(a) => a.clone(),
                        None => pTab.borrow().zName.clone(),
                    };
                    if db.mallocFailed != 0 {
                        break;
                    }
                    if db.item(pFrom).fg.isNestedFrom != 0 {
                        debug_assert!(db.item(pFrom).fg.isSubquery != 0);
                        let s = subqSelect(db, pFrom);
                        pNestedFrom = db.sel(s).pEList;
                        debug_assert!(pNestedFrom.nn());
                        debug_assert!(db.el(pNestedFrom).nExpr() == pTab.borrow().nCol as i32);
                    } else {
                        if let Some(t) = &zTName {
                            if crate::util::sqlite3StrICmp(Some(t), Some(&zTabName)) != 0 {
                                continue;
                            }
                        }
                        pNestedFrom = ExprListId::NULL;
                        let iDb = crate::prepare::sqlite3SchemaToIndex(db, pTab.borrow().pSchema);
                        zSchemaName = if iDb >= 0 {
                            db.aDb[iDb as usize].zDbSName.clone()
                        } else {
                            Some(b"*".to_vec())
                        };
                    }
                    let next = SrcItemId(pTabList, (i + 1) as u32);
                    if i + 1 < nSrc
                        && db.item(next).fg.isUsing != 0
                        && (selFlags & SF_NestedFrom) != 0
                    {
                        pUsing = db.item(next).u3.pUsing;
                        let nId = db.idl(pUsing).nId();
                        for ii in 0..nId {
                            let zUName = db.idl(pUsing).a[ii as usize].zName.clone();
                            let pRight = sqlite3Expr(db, TK_ID as i32, zUName.as_deref());
                            sqlite3ExprSetErrorOffset(db, pRight, iErrOfst);
                            pNew = sqlite3ExprListAppend(db, pNew, pRight);
                            if pNew.nn() {
                                let z = mprintf!(db, b"..%s", zUName.as_deref());
                                let last = db.el(pNew).a.len() - 1;
                                let pX = &mut db.elm(pNew).a[last];
                                debug_assert!(pX.zEName.is_none());
                                pX.zEName = Some(z);
                                pX.fg.eEName = ENAME_TAB;
                                pX.fg.bUsingTerm = 1;
                            }
                        }
                    } else {
                        pUsing = IdListId::NULL;
                    }

                    let tnCol = pTab.borrow().nCol as i32;
                    let mut nAdd = tnCol; /* Number of cols including rowid */
                    if pTab.borrow().VisibleRowid() && (selFlags & SF_NestedFrom) != 0 {
                        nAdd += 1;
                    }
                    for j in 0..nAdd {
                        let zName: ZStr;
                        if j == tnCol {
                            match crate::expr::sqlite3RowidAlias(&pTab) {
                                Some(z) => zName = z,
                                None => continue,
                            }
                        } else {
                            let (cname, hidden, colFlags) = {
                                let t = pTab.borrow();
                                let c = &t.aCol[j as usize];
                                (c.zCnName.clone(), IsHiddenColumn(c), c.colFlags)
                            };
                            zName = cname;

                            /* If pTab is actually an SF_NestedFrom sub-select, do not
                             ** expand any ENAME_ROWID columns.  */
                            if pNestedFrom.nn()
                                && db.el(pNestedFrom).a[j as usize].fg.eEName == ENAME_ROWID
                            {
                                continue;
                            }

                            if let Some(t) = &zTName {
                                if pNestedFrom.nn()
                                    && crate::resolve::sqlite3MatchEName(
                                        &db.el(pNestedFrom).a[j as usize],
                                        None,
                                        Some(t),
                                        None,
                                        &mut 0,
                                    ) == 0
                                {
                                    continue;
                                }
                            }

                            /* If a column is marked as 'hidden', omit it from the expanded
                             ** result-set list unless the SELECT has the SF_IncludeHidden
                             ** bit set.
                             */
                            if (db.sel(p).selFlags & SF_IncludeHidden) == 0 && hidden {
                                continue;
                            }
                            if (colFlags & COLFLAG_NOEXPAND) != 0
                                && zTName.is_none()
                                && (selFlags & SF_NestedFrom) == 0
                            {
                                continue;
                            }
                        }
                        tableSeen = true;

                        if i > 0 && zTName.is_none() && (selFlags & SF_NestedFrom) == 0 {
                            let (isUsing, pU) = {
                                let it = db.item(pFrom);
                                (it.fg.isUsing, it.u3.pUsing)
                            };
                            if isUsing != 0 && sqlite3IdListIndex(db, pU, Some(&zName)) >= 0 {
                                /* In a join with a USING clause, omit columns in the
                                 ** using clause from the table on the right. */
                                continue;
                            }
                        }
                        let pRight = sqlite3Expr(db, TK_ID as i32, Some(&zName));
                        let pExpr;
                        let inRename = db.ppr().IN_RENAME_OBJECT();
                        if (nSrc > 1
                            && ((db.item(pFrom).fg.jointype & JT_LTORJ) == 0
                                || zTName.is_some()
                                || (selFlags & SF_NestedFrom) != 0
                                || !inAnyUsingClause(db, &zName, pFrom, nSrc - i - 1)))
                            || inRename
                        {
                            let mut pLeft = sqlite3Expr(db, TK_ID as i32, Some(&zTabName));
                            let mut pe = sqlite3PExpr(db, TK_DOT as i32, pLeft, pRight);
                            if inRename && pELeft.nn() {
                                crate::alter::sqlite3RenameTokenRemap(
                                    db,
                                    Some(RenameKey::Expr(pLeft)),
                                    RenameKey::Expr(pELeft),
                                );
                            }
                            if let Some(zs) = &zSchemaName {
                                pLeft = sqlite3Expr(db, TK_ID as i32, Some(zs));
                                pe = sqlite3PExpr(db, TK_DOT as i32, pLeft, pe);
                            }
                            pExpr = pe;
                        } else {
                            pExpr = pRight;
                        }
                        sqlite3ExprSetErrorOffset(db, pExpr, iErrOfst);
                        pNew = sqlite3ExprListAppend(db, pNew, pExpr);
                        if pNew.is_null() {
                            break; /* OOM */
                        }
                        let last = db.el(pNew).a.len() - 1;
                        debug_assert!(db.el(pNew).a[last].zEName.is_none());
                        if (selFlags & SF_NestedFrom) != 0 && !db.ppr().IN_RENAME_OBJECT() {
                            let zEName = if pNestedFrom.nn() {
                                debug_assert!(j < db.el(pNestedFrom).nExpr());
                                db.el(pNestedFrom).a[j as usize].zEName.clone()
                            } else {
                                Some(mprintf!(
                                    db,
                                    b"%s.%s.%s",
                                    zSchemaName.as_deref(),
                                    &zTabName,
                                    &zName
                                ))
                            };
                            let bNoExpand = {
                                let (fIsUsing, fU) = {
                                    let it = db.item(pFrom);
                                    (it.fg.isUsing, it.u3.pUsing)
                                };
                                (fIsUsing != 0 && sqlite3IdListIndex(db, fU, Some(&zName)) >= 0)
                                    || (pUsing.nn()
                                        && sqlite3IdListIndex(db, pUsing, Some(&zName)) >= 0)
                                    || (j < tnCol
                                        && (pTab.borrow().aCol[j as usize].colFlags
                                            & COLFLAG_NOEXPAND)
                                            != 0)
                            };
                            let pX = &mut db.elm(pNew).a[last];
                            pX.zEName = zEName;
                            pX.fg.eEName = if j == tnCol { ENAME_ROWID } else { ENAME_TAB };
                            if bNoExpand {
                                pX.fg.bNoExpand = 1;
                            }
                        } else if longNames {
                            let z = mprintf!(db, b"%s.%s", &zTabName, &zName);
                            let pX = &mut db.elm(pNew).a[last];
                            pX.zEName = Some(z);
                            pX.fg.eEName = ENAME_NAME;
                        } else {
                            let pX = &mut db.elm(pNew).a[last];
                            pX.zEName = Some(zName.clone());
                            pX.fg.eEName = ENAME_NAME;
                        }
                    }
                }
                if !tableSeen {
                    if let Some(t) = &zTName {
                        errmsg!(db, b"no such table: %s", t);
                    } else {
                        errmsg!(db, b"no tables specified");
                    }
                }
            }
        }
        sqlite3ExprListDelete(db, pEList);
        db.selm(p).pEList = pNew;
    }
    let pEList = db.sel(p).pEList;
    if pEList.nn() {
        if db.el(pEList).nExpr() > db.aLimit[SQLITE_LIMIT_COLUMN as usize] {
            errmsg!(db, b"too many columns in result set");
            return WRC_Abort;
        }
        if (elistFlags & (EP_HasFunc | EP_Subquery)) != 0 {
            db.selm(p).selFlags |= SF_ComplexResult;
        }
    }
    WRC_Continue
}

/*
** Always assert.  This xSelectCallback2 implementation proves that the
** xSelectCallback2 is never invoked.
*/
pub fn sqlite3SelectWalkAssert2(w: &mut Walker, NotUsed2: SelectId) {
    let _ = (w, NotUsed2);
    debug_assert!(false);
}

/*
** This routine "expands" a SELECT statement and all of its subqueries.
** For additional information on what it means to "expand" a SELECT
** statement, see the comment on the selectExpand worker callback above.
**
** Expanding a SELECT statement is the first step in processing a
** SELECT statement.  The SELECT statement must be expanded before
** name resolution is performed.
**
** If anything goes wrong, an error message is written into pParse.
** The calling function can detect the problem by looking at pParse->nErr
** and/or pParse->db->mallocFailed.
*/
fn sqlite3SelectExpand(db: &mut Sqlite3, pSelect: SelectId) {
    let hasCompound = db.ppr().hasCompound != 0;
    let mut w = Walker::new(db);
    w.xExprCallback = Some(sqlite3ExprWalkNoop);
    if hasCompound {
        w.xSelectCallback = Some(convertCompoundSelectToSubquery);
        w.xSelectCallback2 = None;
        sqlite3WalkSelect(&mut w, pSelect);
    }
    w.xSelectCallback = Some(selectExpander);
    w.xSelectCallback2 = Some(sqlite3SelectPopWith);
    w.eCode = 0;
    sqlite3WalkSelect(&mut w, pSelect);
}

/*
** This is a Walker.xSelectCallback callback for the sqlite3SelectTypeInfo()
** interface.
**
** For each FROM-clause subquery, add Column.zType, Column.zColl, and
** Column.affinity information to the Table structure that represents
** the result set of that subquery.
**
** The Table structure that represents the result set was constructed
** by selectExpander() but the type and collation and affinity information
** was omitted at that point because identifiers had not yet been resolved.
** This routine is called after identifier resolution.
*/
fn selectAddSubqueryTypeInfo(w: &mut Walker, p: SelectId) {
    let db = &mut *w.db;
    if (db.sel(p).selFlags & SF_HasTypeInfo) != 0 {
        return;
    }
    db.selm(p).selFlags |= SF_HasTypeInfo;
    debug_assert!((db.sel(p).selFlags & SF_Resolved) != 0);
    let pTabList = db.sel(p).pSrc;
    let n = db.src(pTabList).nSrc();
    for i in 0..n {
        let pFrom = SrcItemId(pTabList, i as u32);
        let pTab = itemTab(db, pFrom);
        if (pTab.borrow().tabFlags & TF_Ephemeral) != 0 && db.item(pFrom).fg.isSubquery != 0 {
            /* A sub-query in the FROM clause of a SELECT */
            let pSel = subqSelect(db, pFrom);
            sqlite3SubqueryColumnTypes(db, &pTab, pSel, SQLITE_AFF_NONE);
        }
    }
}

/*
** This routine adds datatype and collating sequence information to
** the Table structures of all FROM-clause subqueries in a
** SELECT statement.
**
** Use this routine after name resolution.
*/
fn sqlite3SelectAddTypeInfo(db: &mut Sqlite3, pSelect: SelectId) {
    let mut w = Walker::new(db);
    w.xSelectCallback = Some(sqlite3SelectWalkNoop);
    w.xSelectCallback2 = Some(selectAddSubqueryTypeInfo);
    w.xExprCallback = Some(sqlite3ExprWalkNoop);
    sqlite3WalkSelect(&mut w, pSelect);
}

/*
** This routine sets up a SELECT statement for processing.  The
** following is accomplished:
**
**     *  VDBE Cursor numbers are assigned to all FROM-clause terms.
**     *  Ephemeral Table objects are created for all FROM-clause subqueries.
**     *  ON and USING clauses are shifted into WHERE statements
**     *  Wildcards "*" and "TABLE.*" in result sets are expanded.
**     *  Identifiers in expression are matched to tables.
**
** This routine acts recursively on all subqueries within the SELECT.
*/
pub fn sqlite3SelectPrep(db: &mut Sqlite3, p: SelectId, pOuterNC: NcId) {
    debug_assert!(p.nn() || db.mallocFailed != 0);
    if db.mallocFailed != 0 {
        return;
    }
    if (db.sel(p).selFlags & SF_HasTypeInfo) != 0 {
        return;
    }
    sqlite3SelectExpand(db, p);
    if db.ppr().nErr != 0 {
        return;
    }
    crate::resolve::sqlite3ResolveSelectNames(db, p, pOuterNC);
    if db.ppr().nErr != 0 {
        return;
    }
    sqlite3SelectAddTypeInfo(db, p);
}

/*
** Analyze the arguments to aggregate functions.  Create new pAggInfo->aCol[]
** entries for columns that are arguments to aggregate functions but which
** are not otherwise used.
**
** The aCol[] entries in AggInfo prior to nAccumulator are columns that
** are referenced outside of aggregate functions.  These might be columns
** that are part of the GROUP by clause, for example.  Other database engines
** would throw an error if there is a column reference that is not in the
** GROUP BY clause and that is not part of an aggregate function argument.
** But SQLite allows this.
**
** The aCol[] entries beginning with the aCol[nAccumulator] and following
** are column references that are used exclusively as arguments to
** aggregate functions.  This routine is responsible for computing
** (or recomputing) those aCol[] entries.
*/
fn analyzeAggFuncArgs(db: &mut Sqlite3, pAggInfo: AggInfoId, pNC: NcId) {
    debug_assert!(pAggInfo.nn());
    debug_assert!(db.arena.agg(pAggInfo).iFirstReg == 0);
    db.arena.ncm(pNC).ncFlags |= NC_InAggFunc;
    let mut i = 0;
    while i < db.arena.agg(pAggInfo).nFunc() {
        let pExpr = db.arena.agg(pAggInfo).aFunc[i as usize].pFExpr;
        let (pList, pLeft, isWin, pWin) = {
            let e = db.ex(pExpr);
            debug_assert!(e.op == TK_FUNCTION || e.op == TK_AGG_FUNCTION);
            debug_assert!(e.UseXList());
            (e.x.pList, e.pLeft, e.HasProperty(EP_WinFunc), e.y.pWin)
        };
        sqlite3ExprAnalyzeAggList(db, pNC, pList);
        if pLeft.nn() {
            debug_assert!(db.ex(pLeft).op == TK_ORDER);
            debug_assert!(db.ex(pLeft).UseXList());
            let l = db.ex(pLeft).x.pList;
            sqlite3ExprAnalyzeAggList(db, pNC, l);
        }
        if isWin {
            let f = db.win(pWin).pFilter;
            sqlite3ExprAnalyzeAggregates(db, pNC, f);
        }
        i += 1;
    }
    db.arena.ncm(pNC).ncFlags &= !NC_InAggFunc;
}

/*
** An index on expressions is being used in the inner loop of an
** aggregate query with a GROUP BY clause.  This routine attempts
** to adjust the AggInfo object to take advantage of index and to
** perhaps use the index as a covering index.
**
*/
fn optimizeAggregateUseOfIndexedExpr(
    db: &mut Sqlite3,
    pSelect: SelectId,
    pAggInfo: AggInfoId,
    pNC: NcId,
) {
    debug_assert!(db.arena.agg(pAggInfo).iFirstReg == 0);
    debug_assert!(pSelect.nn());
    debug_assert!(db.sel(pSelect).pGroupBy.nn());
    let nGB = db.el(db.sel(pSelect).pGroupBy).nExpr();
    {
        let ai = db.arena.aggm(pAggInfo);
        let nAcc = ai.nAccumulator.max(0) as usize;
        ai.aCol.truncate(nAcc);
        if ai.nSortingColumn > 0 {
            let mut mx = nGB - 1;
            for c in ai.aCol.iter() {
                let k = c.iSorterColumn;
                if k > mx {
                    mx = k;
                }
            }
            ai.nSortingColumn = (mx + 1) as u32;
        }
    }
    analyzeAggFuncArgs(db, pAggInfo, pNC);
}

/*
** Walker callback for aggregateConvertIndexedExprRefToColumn().
*/
fn aggregateIdxEprRefToColCallback(w: &mut Walker, pExpr: ExprId) -> i32 {
    let db = &mut *w.db;
    let (pAggInfo, op, iAgg) = {
        let e = db.ex(pExpr);
        (e.pAggInfo, e.op, e.iAgg)
    };
    if pAggInfo.is_null() {
        return WRC_Continue;
    }
    if op == TK_AGG_COLUMN {
        return WRC_Continue;
    }
    if op == TK_AGG_FUNCTION {
        return WRC_Continue;
    }
    if op == TK_IF_NULL_ROW {
        return WRC_Continue;
    }
    if iAgg as i32 >= db.arena.agg(pAggInfo).nColumn() {
        return WRC_Continue;
    }
    debug_assert!(iAgg >= 0);
    let (iTable, iColumn) = {
        let c = &db.arena.agg(pAggInfo).aCol[iAgg as usize];
        (c.iTable, c.iColumn)
    };
    let e = db.exm(pExpr);
    e.op = TK_AGG_COLUMN;
    e.iTable = iTable;
    e.iColumn = iColumn as ynVar;
    e.ClearProperty(EP_Skip | EP_Collate | EP_Unlikely);
    WRC_Prune
}

/*
** Convert every pAggInfo->aFunc[].pExpr such that any node within
** those expressions that has pAppInfo set is changed into a TK_AGG_COLUMN
** opcode.
*/
fn aggregateConvertIndexedExprRefToColumn(db: &mut Sqlite3, pAggInfo: AggInfoId) {
    let exprs: Vec<ExprId> = db
        .arena
        .agg(pAggInfo)
        .aFunc
        .iter()
        .map(|f| f.pFExpr)
        .collect();
    let mut w = Walker::new(db);
    w.xExprCallback = Some(aggregateIdxEprRefToColCallback);
    for e in exprs {
        sqlite3WalkExpr(&mut w, e);
    }
}

/*
** Allocate a block of registers so that there is one register for each
** pAggInfo->aCol[] and pAggInfo->aFunc[] entry in pAggInfo.  The first
** register in this block is stored in pAggInfo->iFirstReg.
**
** This routine may only be called once for each AggInfo object.  Prior
** to calling this routine:
**
**     *  The aCol[] and aFunc[] arrays may be modified
**     *  The AggInfoColumnReg() and AggInfoFuncReg() macros may not be used
**
** After calling this routine:
**
**     *  The aCol[] and aFunc[] arrays are fixed
**     *  The AggInfoColumnReg() and AggInfoFuncReg() macros may be used
**
*/
fn assignAggregateRegisters(db: &mut Sqlite3, pAggInfo: AggInfoId) {
    debug_assert!(pAggInfo.nn());
    debug_assert!(db.arena.agg(pAggInfo).iFirstReg == 0);
    let first = db.ppr().nMem + 1;
    let n = {
        let ai = db.arena.aggm(pAggInfo);
        ai.iFirstReg = first;
        ai.nColumn() + ai.nFunc()
    };
    db.pp().nMem += n;
}

/*
** Reset the aggregate accumulator.
**
** The aggregate accumulator is a set of memory cells that hold
** intermediate results while calculating an aggregate.  This
** routine generates code that stores NULLs in all of those memory
** cells.
*/
fn resetAccumulator(db: &mut Sqlite3, pAggInfo: AggInfoId) {
    let (nReg, iFirstReg, nFunc) = {
        let ai = db.arena.agg(pAggInfo);
        (ai.nFunc() + ai.nColumn(), ai.iFirstReg, ai.nFunc())
    };
    debug_assert!(iFirstReg > 0);
    debug_assert!(db.mallocFailed == 0 || db.ppr().nErr != 0);
    if nReg == 0 {
        return;
    }
    if db.ppr().nErr != 0 {
        return;
    }
    sqlite3VdbeAddOp3(db.v(), OP_Null, 0, iFirstReg, iFirstReg + nReg - 1);
    for i in 0..nFunc as usize {
        let f = db.arena.agg(pAggInfo).aFunc[i].clone();
        let zFuncName: ZStr = f
            .pFunc
            .as_ref()
            .map(|x| x.zName.clone())
            .unwrap_or_default();
        if f.iDistinct >= 0 {
            let pE = f.pFExpr;
            debug_assert!(db.ex(pE).UseXList());
            let pl = db.ex(pE).x.pList;
            if pl.is_null() || db.el(pl).nExpr() != 1 {
                errmsg!(db, b"DISTINCT aggregates must have exactly one argument");
                db.arena.aggm(pAggInfo).aFunc[i].iDistinct = -1;
            } else {
                let pKeyInfo = sqlite3KeyInfoFromExprList(db, pl, 0, 0);
                let a = sqlite3VdbeAddOp4(
                    db.v(),
                    OP_OpenEphemeral,
                    f.iDistinct,
                    0,
                    0,
                    p4key(pKeyInfo),
                    P4_KEYINFO as i32,
                );
                db.arena.aggm(pAggInfo).aFunc[i].iDistAddr = a;
                explainQP(
                    db,
                    0,
                    b"USE TEMP B-TREE FOR %s(DISTINCT)",
                    &[PfArg::from(&zFuncName)],
                );
            }
        }
        if f.iOBTab >= 0 {
            let mut nExtra = 0;
            let pLeft = db.ex(f.pFExpr).pLeft;
            debug_assert!(pLeft.nn());
            debug_assert!(db.ex(pLeft).op == TK_ORDER);
            debug_assert!(db.ex(pLeft).UseXList());
            debug_assert!(f.pFunc.is_some());
            let pOBList = db.ex(pLeft).x.pList;
            if f.bOBUnique == 0 {
                nExtra += 1; /* One extra column for the OP_Sequence */
            }
            if f.bOBPayload != 0 {
                /* extra columns for the function arguments */
                let fl = db.ex(f.pFExpr).x.pList;
                debug_assert!(fl.nn());
                nExtra += db.el(fl).nExpr();
            }
            if f.bUseSubtype != 0 {
                let fl = db.ex(f.pFExpr).x.pList;
                nExtra += db.el(fl).nExpr();
            }
            let pKeyInfo = sqlite3KeyInfoFromExprList(db, pOBList, 0, nExtra);
            if f.bOBUnique == 0 && db.ppr().nErr == 0 {
                if let Some(k) = &pKeyInfo {
                    k.borrow_mut().nKeyField += 1;
                }
            }
            let n = db.el(pOBList).nExpr();
            sqlite3VdbeAddOp4(
                db.v(),
                OP_OpenEphemeral,
                f.iOBTab,
                n + nExtra,
                0,
                p4key(pKeyInfo),
                P4_KEYINFO as i32,
            );
            explainQP(
                db,
                0,
                b"USE TEMP B-TREE FOR %s(ORDER BY)",
                &[PfArg::from(&zFuncName)],
            );
        }
    }
}

/*
** Invoke the OP_AggFinalize opcode for every aggregate function
** in the AggInfo structure.
*/
fn finalizeAggFunctions(db: &mut Sqlite3, pAggInfo: AggInfoId) {
    let nFunc = db.arena.agg(pAggInfo).nFunc();
    for i in 0..nFunc {
        let pF = db.arena.agg(pAggInfo).aFunc[i as usize].clone();
        let regFunc = db.arena.agg(pAggInfo).AggInfoFuncReg(i);
        debug_assert!(db.ex(pF.pFExpr).UseXList());
        if db.ppr().nErr != 0 {
            return;
        }
        let pList = db.ex(pF.pFExpr).x.pList;
        let pFunc = pF.pFunc.clone().expect("aggregate without FuncDef");
        if pF.iOBTab >= 0 {
            /* For an ORDER BY aggregate, calls to OP_AggStep were deferred.  Inputs
             ** were stored in emphermal table pF->iOBTab.  Here, we extract those
             ** inputs (in ORDER BY order) and make all calls to OP_AggStep
             ** before doing the OP_AggFinal call. */
            let nKey; /* Key columns to be skipped */
            let nArg = db.el(pList).nExpr(); /* Number of columns to extract */
            let regAgg = sqlite3GetTempRange(db, nArg); /* Extract into this array */

            if pF.bOBPayload == 0 {
                nKey = 0;
            } else {
                let pLeft = db.ex(pF.pFExpr).pLeft;
                debug_assert!(pLeft.nn());
                debug_assert!(db.ex(pLeft).UseXList());
                let l = db.ex(pLeft).x.pList;
                debug_assert!(l.nn());
                let mut n = db.el(l).nExpr();
                if pF.bOBUnique == 0 {
                    n += 1;
                }
                nKey = n;
            }
            let iTop = sqlite3VdbeAddOp1(db.v(), OP_Rewind, pF.iOBTab);
            let mut j = nArg - 1;
            while j >= 0 {
                sqlite3VdbeAddOp3(db.v(), OP_Column, pF.iOBTab, nKey + j, regAgg + j);
                j -= 1;
            }
            if pF.bUseSubtype != 0 {
                let regSubtype = sqlite3GetTempReg(db);
                let iBaseCol = nKey + nArg + (pF.bOBPayload == 0 && pF.bOBUnique == 0) as i32;
                let mut j = nArg - 1;
                while j >= 0 {
                    sqlite3VdbeAddOp3(db.v(), OP_Column, pF.iOBTab, iBaseCol + j, regSubtype);
                    sqlite3VdbeAddOp2(db.v(), OP_SetSubtype, regSubtype, regAgg + j);
                    j -= 1;
                }
                sqlite3ReleaseTempReg(db, regSubtype);
            }
            let v = db.v();
            sqlite3VdbeAddOp3(v, OP_AggStep, 0, regAgg, regFunc);
            sqlite3VdbeAppendP4(v, P4::Func(pFunc.clone()), P4_FUNCDEF as i32);
            sqlite3VdbeChangeP5(v, nArg as u16);
            sqlite3VdbeAddOp2(v, OP_Next, pF.iOBTab, iTop + 1);
            sqlite3VdbeJumpHere(v, iTop);
            sqlite3ReleaseTempRange(db, regAgg, nArg);
        }
        let n = if pList.nn() { db.el(pList).nExpr() } else { 0 };
        let v = db.v();
        sqlite3VdbeAddOp2(v, OP_AggFinal, regFunc, n);
        sqlite3VdbeAppendP4(v, P4::Func(pFunc), P4_FUNCDEF as i32);
    }
}

/*
** Generate code that will update the accumulator memory cells for an
** aggregate based on the current cursor position.
**
** If regAcc is non-zero and there are no min() or max() aggregates
** in pAggInfo, then only populate the pAggInfo->nAccumulator accumulator
** registers if register regAcc contains 0. The caller will take care
** of setting and clearing regAcc.
**
** For an ORDER BY aggregate, the actual accumulator memory cell update
** is deferred until after all input rows have been received, so that they
** can be run in the requested order.  In that case, instead of invoking
** OP_AggStep to update the accumulator, just add the arguments that would
** have been passed into OP_AggStep into the sorting ephemeral table
** (along with the appropriate sort key).
*/
fn updateAccumulator(db: &mut Sqlite3, regAcc: i32, pAggInfo: AggInfoId, eDistinctType: i32) {
    let mut regHit = 0;
    let mut addrHitTest = 0;

    debug_assert!(db.arena.agg(pAggInfo).iFirstReg > 0);
    if db.ppr().nErr != 0 {
        return;
    }
    db.arena.aggm(pAggInfo).directMode = 1;
    let nFunc = db.arena.agg(pAggInfo).nFunc();
    let nAccumulator = db.arena.agg(pAggInfo).nAccumulator;
    for i in 0..nFunc {
        let pF = db.arena.agg(pAggInfo).aFunc[i as usize].clone();
        let nArg;
        let mut addrNext = 0;
        let regAgg;
        let mut regAggSz = 0;
        let mut regDistinct = 0;
        let pFunc = pF.pFunc.clone().expect("aggregate without FuncDef");
        debug_assert!(db.ex(pF.pFExpr).UseXList());
        let pList = db.ex(pF.pFExpr).x.pList;
        if db.ex(pF.pFExpr).HasProperty(EP_WinFunc) {
            let pWin = db.ex(pF.pFExpr).y.pWin;
            let pFilter = db.win(pWin).pFilter;
            if nAccumulator != 0 && (pFunc.funcFlags & SQLITE_FUNC_NEEDCOLL) != 0 && regAcc != 0 {
                /* If regAcc==0, there there exists some min() or max() function
                 ** without a FILTER clause that will ensure the magnet registers
                 ** are populated. */
                if regHit == 0 {
                    db.pp().nMem += 1;
                    regHit = db.ppr().nMem;
                }
                /* If this is the first row of the group (regAcc contains 0), clear the
                 ** "magnet" register regHit so that the accumulator registers
                 ** are populated if the FILTER clause jumps over the the
                 ** invocation of min() or max() altogether. Or, if this is not
                 ** the first row (regAcc contains 1), set the magnet register so that
                 ** the accumulators are not populated unless the min()/max() is invoked
                 ** and indicates that they should be.  */
                sqlite3VdbeAddOp2(db.v(), OP_Copy, regAcc, regHit);
            }
            addrNext = sqlite3VdbeMakeLabel(db);
            sqlite3ExprIfFalse(db, pFilter, addrNext, SQLITE_JUMPIFNULL as i32);
        }
        if pF.iOBTab >= 0 {
            /* Instead of invoking AggStep, we must push the arguments that would
             ** have been passed to AggStep onto the sorting table. */
            debug_assert!(pList.nn());
            nArg = db.el(pList).nExpr();
            debug_assert!(nArg > 0);
            let pLeft = db.ex(pF.pFExpr).pLeft;
            debug_assert!(pLeft.nn());
            debug_assert!(db.ex(pLeft).op == TK_ORDER);
            debug_assert!(db.ex(pLeft).UseXList());
            let pOBList = db.ex(pLeft).x.pList;
            debug_assert!(pOBList.nn());
            debug_assert!(db.el(pOBList).nExpr() > 0);
            regAggSz = db.el(pOBList).nExpr();
            if pF.bOBUnique == 0 {
                regAggSz += 1; /* One register for OP_Sequence */
            }
            if pF.bOBPayload != 0 {
                regAggSz += nArg;
            }
            if pF.bUseSubtype != 0 {
                regAggSz += nArg;
            }
            regAggSz += 1; /* One extra register to hold result of MakeRecord */
            regAgg = sqlite3GetTempRange(db, regAggSz);
            regDistinct = regAgg;
            sqlite3ExprCodeExprList(db, pOBList, regAgg, 0, SQLITE_ECEL_DUP);
            let mut jj = db.el(pOBList).nExpr(); /* Registered used so far in building the record */
            if pF.bOBUnique == 0 {
                sqlite3VdbeAddOp2(db.v(), OP_Sequence, pF.iOBTab, regAgg + jj);
                jj += 1;
            }
            if pF.bOBPayload != 0 {
                regDistinct = regAgg + jj;
                sqlite3ExprCodeExprList(db, pList, regDistinct, 0, SQLITE_ECEL_DUP);
                jj += nArg;
            }
            if pF.bUseSubtype != 0 {
                let regBase = if pF.bOBPayload != 0 {
                    regDistinct
                } else {
                    regAgg
                };
                for kk in 0..nArg {
                    sqlite3VdbeAddOp2(db.v(), OP_GetSubtype, regBase + kk, regAgg + jj);
                    jj += 1;
                }
            }
        } else if pList.nn() {
            nArg = db.el(pList).nExpr();
            regAgg = sqlite3GetTempRange(db, nArg);
            regDistinct = regAgg;
            sqlite3ExprCodeExprList(db, pList, regAgg, 0, SQLITE_ECEL_DUP);
        } else {
            nArg = 0;
            regAgg = 0;
        }
        if pF.iDistinct >= 0 && pList.nn() {
            if addrNext == 0 {
                addrNext = sqlite3VdbeMakeLabel(db);
            }
            let r = codeDistinct(
                db,
                eDistinctType,
                pF.iDistinct,
                addrNext,
                pList,
                regDistinct,
            );
            db.arena.aggm(pAggInfo).aFunc[i as usize].iDistinct = r;
        }
        if pF.iOBTab >= 0 {
            /* Insert a new record into the ORDER BY table */
            let v = db.v();
            sqlite3VdbeAddOp3(
                v,
                OP_MakeRecord,
                regAgg,
                regAggSz - 1,
                regAgg + regAggSz - 1,
            );
            sqlite3VdbeAddOp4Int(
                v,
                OP_IdxInsert,
                pF.iOBTab,
                regAgg + regAggSz - 1,
                regAgg,
                regAggSz - 1,
            );
            sqlite3ReleaseTempRange(db, regAgg, regAggSz);
        } else {
            /* Invoke the AggStep function */
            if (pFunc.funcFlags & SQLITE_FUNC_NEEDCOLL) != 0 {
                let mut pColl: Option<CollSeqRef> = None;
                debug_assert!(pList.nn()); /* pList!=0 if pF->pFunc has NEEDCOLL */
                let mut j = 0;
                while pColl.is_none() && j < nArg {
                    let e = db.el(pList).a[j as usize].pExpr;
                    pColl = sqlite3ExprCollSeq(db, e);
                    j += 1;
                }
                if pColl.is_none() {
                    pColl = db.pDfltColl.clone();
                }
                if regHit == 0 && nAccumulator != 0 {
                    db.pp().nMem += 1;
                    regHit = db.ppr().nMem;
                }
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_CollSeq,
                    regHit,
                    0,
                    0,
                    p4coll(pColl),
                    P4_COLLSEQ as i32,
                );
            }
            let regFunc = db.arena.agg(pAggInfo).AggInfoFuncReg(i);
            let v = db.v();
            sqlite3VdbeAddOp3(v, OP_AggStep, 0, regAgg, regFunc);
            sqlite3VdbeAppendP4(v, P4::Func(pFunc.clone()), P4_FUNCDEF as i32);
            sqlite3VdbeChangeP5(v, nArg as u16);
            sqlite3ReleaseTempRange(db, regAgg, nArg);
        }
        if addrNext != 0 {
            sqlite3VdbeResolveLabel(db.v(), addrNext);
        }
        if db.ppr().nErr != 0 {
            return;
        }
    }
    if regHit == 0 && nAccumulator != 0 {
        regHit = regAcc;
    }
    if regHit != 0 {
        addrHitTest = sqlite3VdbeAddOp1(db.v(), OP_If, regHit);
    }
    for i in 0..nAccumulator {
        let (pCExpr, reg) = {
            let ai = db.arena.agg(pAggInfo);
            (ai.aCol[i as usize].pCExpr, ai.AggInfoColumnReg(i))
        };
        sqlite3ExprCode(db, pCExpr, reg);
        if db.ppr().nErr != 0 {
            return;
        }
    }

    db.arena.aggm(pAggInfo).directMode = 0;
    if addrHitTest != 0 {
        sqlite3VdbeJumpHereOrPopInst(db.v(), addrHitTest);
    }
}

/*
** Add a single OP_Explain instruction to the VDBE to explain a simple
** count(*) query ("SELECT count(*) FROM pTab").
*/
fn explainSimpleCount(db: &mut Sqlite3, pTab: &TableRef, pIdx: &Option<IndexRef>) {
    if db.ppr().explain == 2 {
        let bCover = match pIdx {
            Some(i) => pTab.borrow().HasRowid() || !i.borrow().IsPrimaryKeyIndex(),
            None => false,
        };
        let zTab = pTab.borrow().zName.clone();
        let zIdx: ZStr = if bCover {
            pIdx.as_ref().unwrap().borrow().zName.clone()
        } else {
            Vec::new()
        };
        let zMid: &[u8] = if bCover {
            b" USING COVERING INDEX "
        } else {
            b""
        };
        sqlite3VdbeExplain(
            db,
            0,
            b"SCAN %s%s%s",
            &[PfArg::from(&zTab), PfArg::from(zMid), PfArg::from(&zIdx)],
        );
    }
}

/*
** sqlite3WalkExpr() callback used by havingToWhere().
**
** If the node passed to the callback is a TK_AND node, return
** WRC_Continue to tell sqlite3WalkExpr() to iterate through child nodes.
**
** Otherwise, return WRC_Prune. In this case, also check if the
** sub-expression matches the criteria for being moved to the WHERE
** clause. If so, add it to the WHERE clause and replace the sub-expression
** within the HAVING expression with a constant "1".
*/
fn havingToWhereExprCb(pWalker: &mut Walker, pExpr: ExprId) -> i32 {
    let db = &mut *pWalker.db;
    if db.ex(pExpr).op != TK_AND {
        let pS = pWalker.u.pSelect;
        /* This routine is called before the HAVING clause of the current
         ** SELECT is analyzed for aggregates. So if pExpr->pAggInfo is set
         ** here, it indicates that the expression is a correlated reference to a
         ** column from an outer aggregate query, or an aggregate function that
         ** belongs to an outer query. Do not move the expression to the WHERE
         ** clause in this obscure case, as doing so may corrupt the outer Select
         ** statements AggInfo structure.  */
        let gb = db.sel(pS).pGroupBy;
        if sqlite3ExprIsConstantOrGroupBy(db, pExpr, gb) != 0
            && !db.ex(pExpr).AlwaysFalse()
            && db.ex(pExpr).pAggInfo.is_null()
        {
            let pNew = sqlite3ExprInt32(db, 1);
            if pNew.nn() {
                let pWhere = db.sel(pS).pWhere;
                /* SWAP(Expr, *pNew, *pExpr) */
                let a = std::mem::take(db.exm(pNew));
                let b = std::mem::replace(db.exm(pExpr), a);
                *db.exm(pNew) = b;
                let nw = sqlite3ExprAnd(db, pWhere, pNew);
                db.selm(pS).pWhere = nw;
                pWalker.eCode = 1;
            }
        }
        return WRC_Prune;
    }
    WRC_Continue
}

/*
** Transfer eligible terms from the HAVING clause of a query, which is
** processed after grouping, to the WHERE clause, which is processed before
** grouping. For example, the query:
**
**   SELECT * FROM <tables> WHERE a=? GROUP BY b HAVING b=? AND c=?
**
** can be rewritten as:
**
**   SELECT * FROM <tables> WHERE a=? AND b=? GROUP BY b HAVING c=?
**
** A term of the HAVING expression is eligible for transfer if it consists
** entirely of constants and expressions that are also GROUP BY terms that
** use the "BINARY" collation sequence.
*/
fn havingToWhere(db: &mut Sqlite3, p: SelectId) {
    let pHaving = db.sel(p).pHaving;
    let mut sWalker = Walker::new(db);
    sWalker.xExprCallback = Some(havingToWhereExprCb);
    sWalker.u.pSelect = p;
    sqlite3WalkExpr(&mut sWalker, pHaving);
}

/*
** Check to see if the pThis entry of pTabList is a self-join of another view.
** Search FROM-clause entries in the range of iFirst..iEnd, including iFirst
** but stopping before iEnd.
**
** If pThis is a self-join, then return the SrcItem for the first other
** instance of that view found.  If pThis is not a self-join then return 0.
*/
fn isSelfJoinView(
    db: &Sqlite3,
    pTabList: SrcListId,
    pThis: SrcItemId,
    mut iFirst: i32,
    iEnd: i32,
) -> Option<SrcItemId> {
    debug_assert!(db.item(pThis).fg.isSubquery != 0);
    let pSel = subqSelect(db, pThis);
    debug_assert!(pSel.nn());
    if (db.sel(pSel).selFlags & SF_PushDown) != 0 {
        return None;
    }
    let thisTab = db.item(pThis).pSTab.clone().expect("pThis->pSTab");
    let thisSchema = thisTab.borrow().pSchema;
    while iFirst < iEnd {
        let pItem = SrcItemId(pTabList, iFirst as u32);
        iFirst += 1;
        let it = db.item(pItem);
        if it.fg.isSubquery == 0 {
            continue;
        }
        if it.fg.viaCoroutine != 0 {
            continue;
        }
        if it.zName.is_none() {
            continue;
        }
        let itTab = it.pSTab.clone().expect("pItem->pSTab");
        let itSchema = itTab.borrow().pSchema;
        if itSchema != thisSchema {
            continue;
        }
        if crate::util::sqlite3_stricmp(it.zName.as_deref(), db.item(pThis).zName.as_deref()) != 0 {
            continue;
        }
        let pS1 = subqSelect(db, pItem);
        if itSchema == SchemaId::default() && db.sel(pSel).selId != db.sel(pS1).selId {
            /* The query flattener left two different CTE tables with identical
             ** names in the same FROM clause. */
            continue;
        }
        if (db.sel(pS1).selFlags & SF_PushDown) != 0 {
            /* The view was modified by some other optimization such as
             ** pushDownWhereTerms() */
            continue;
        }
        return Some(pItem);
    }
    None
}

/*
** Attempt to transform a query of the form
**
**    SELECT count(*) FROM (SELECT x FROM t1 UNION ALL SELECT y FROM t2)
**
** Into this:
**
**    SELECT (SELECT count(*) FROM t1)+(SELECT count(*) FROM t2)
**
** The transformation only works if all of the following are true:
**
**   *  The subquery is a UNION ALL of two or more terms
**   *  The subquery does not have a LIMIT clause
**   *  There is no WHERE or GROUP BY or HAVING clauses on the subqueries
**   *  The outer query is a simple count(*) with no WHERE clause or other
**      extraneous syntax.
**   *  None of the subqueries are DISTINCT (forumpost/a860f5fb2e 2025-03-10)
**
** Return TRUE if the optimization is undertaken.
*/
fn countOfViewOptimization(db: &mut Sqlite3, p: SelectId) -> i32 {
    let s = db.sel(p).clone();
    if (s.selFlags & SF_Aggregate) == 0 {
        return 0; /* This is an aggregate */
    }
    if db.el(s.pEList).nExpr() != 1 {
        return 0; /* Single result column */
    }
    if s.pWhere.nn() {
        return 0;
    }
    if s.pHaving.nn() {
        return 0;
    }
    if s.pGroupBy.nn() {
        return 0;
    }
    if s.pOrderBy.nn() {
        return 0;
    }
    let mut pExpr = db.el(s.pEList).a[0].pExpr;
    {
        let e = db.ex(pExpr);
        if e.op != TK_AGG_FUNCTION {
            return 0; /* Result is an aggregate */
        }
        debug_assert!(e.UseUToken());
        if crate::util::sqlite3_stricmp(e.u.zToken.as_deref(), Some(b"count")) != 0 {
            return 0; /* Is count() */
        }
        debug_assert!(e.UseXList());
        if e.x.pList.nn() {
            return 0; /* Must be count(*) */
        }
    }
    if db.src(s.pSrc).nSrc() != 1 {
        return 0; /* One table in FROM  */
    }
    if db.ex(pExpr).HasProperty(EP_WinFunc) {
        return 0; /* Not a window function */
    }
    let pFrom = SrcItemId(s.pSrc, 0);
    if db.item(pFrom).fg.isSubquery == 0 {
        return 0; /* FROM is a subquery */
    }
    if (s.selFlags & SF_Correlated) != 0 {
        return 0; /* Not a correlated subq */
    }
    let mut pSub = subqSelect(db, pFrom);
    if db.sel(pSub).pPrior.is_null() {
        return 0; /* Must be a compound */
    }
    if (db.sel(pSub).selFlags & SF_CopyCte) != 0 {
        return 0; /* Not a CTE */
    }
    loop {
        let ss = db.sel(pSub);
        if ss.op != TK_ALL && ss.pPrior.nn() {
            return 0; /* Must be UNION ALL */
        }
        if ss.pWhere.nn() {
            return 0; /* No WHERE clause */
        }
        if ss.pLimit.nn() {
            return 0; /* No LIMIT clause */
        }
        if (ss.selFlags & (SF_Aggregate | SF_Distinct)) != 0 {
            return 0; /* Not an aggregate nor DISTINCT */
        }
        debug_assert!(ss.pHaving.is_null()); /* Due to the previous */
        pSub = ss.pPrior; /* Repeat over compound */
        if pSub.is_null() {
            break;
        }
    }

    /* If we reach this point then it is OK to perform the transformation */

    let pCount = pExpr;
    pExpr = ExprId::NULL;
    pSub = sqlite3SubqueryDetach(db, pFrom);
    sqlite3SrcListDelete(db, s.pSrc);
    let ns = db.arena.new_srclist(SrcList::default());
    db.selm(p).pSrc = ns;
    while pSub.nn() {
        let pPrior = db.sel(pSub).pPrior;
        {
            let ss = db.selm(pSub);
            ss.pPrior = SelectId::NULL;
            ss.pNext = SelectId::NULL;
            ss.selFlags |= SF_Aggregate;
            ss.selFlags &= !SF_Compound;
            ss.nSelectRow = 0;
        }
        /* (the old pSub->pEList is freed with the arena) */
        let mut pTerm = if pPrior.nn() {
            sqlite3ExprDup(db, pCount, 0)
        } else {
            pCount
        };
        let el = sqlite3ExprListAppend(db, ExprListId::NULL, pTerm);
        db.selm(pSub).pEList = el;
        pTerm = sqlite3PExpr(db, TK_SELECT as i32, ExprId::NULL, ExprId::NULL);
        sqlite3PExprAddSelect(db, pTerm, pSub);
        if pExpr.is_null() {
            pExpr = pTerm;
        } else {
            pExpr = sqlite3PExpr(db, TK_PLUS as i32, pTerm, pExpr);
        }
        if db.ppr().nErr != 0 {
            sqlite3ExprDelete(db, pExpr);
            pExpr = ExprId::NULL;
        }
        pSub = pPrior;
    }
    let el = db.sel(p).pEList;
    db.elm(el).a[0].pExpr = pExpr;
    db.selm(p).selFlags &= !SF_Aggregate;

    1
}

/*
** If any term of pSrc, or any SF_NestedFrom sub-query, is not the same
** as pSrcItem but has the same alias as p0, then return true.
** Otherwise return false.
*/
fn sameSrcAlias(db: &Sqlite3, p0: SrcItemId, pSrc: SrcListId) -> bool {
    let n = db.src(pSrc).nSrc();
    let (t0, a0) = {
        let x = db.item(p0);
        (x.pSTab.clone(), x.zAlias.clone())
    };
    for i in 0..n {
        let p1 = SrcItemId(pSrc, i as u32);
        if p1 == p0 {
            continue;
        }
        let it = db.item(p1);
        let sameTab = match (&t0, &it.pSTab) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        if sameTab && crate::util::sqlite3_stricmp(a0.as_deref(), it.zAlias.as_deref()) == 0 {
            return true;
        }
        if it.fg.isSubquery != 0 {
            let s = subqSelect(db, p1);
            if (db.sel(s).selFlags & SF_NestedFrom) != 0 && sameSrcAlias(db, p0, db.sel(s).pSrc) {
                return true;
            }
        }
    }
    false
}

/*
** Return TRUE (non-zero) if the i-th entry in the pTabList SrcList can
** be implemented as a co-routine.  The i-th entry is guaranteed to be
** a subquery.
**
** The subquery is implemented as a co-routine if all of the following are
** true:
**
**    (1)  The subquery will likely be implemented in the outer loop of
**         the query.  This will be the case if any one of the following
**         conditions hold:
**         (a)  The subquery is the only term in the FROM clause
**         (b)  The subquery is the left-most term and a CROSS JOIN or similar
**              requires it to be the outer loop
**         (c)  All of the following are true:
**                (i) The subquery is the left-most subquery in the FROM clause
**               (ii) There is nothing that would prevent the subquery from
**                    being used as the outer loop if the sqlite3WhereBegin()
**                    routine nominates it to that position.
**              (iii) The query is not a UPDATE ... FROM
**    (2)  The subquery is not a CTE that should be materialized because
**         (a) the AS MATERIALIZED keyword is used, or
**         (b) the CTE is used multiple times and does not have the
**             NOT MATERIALIZED keyword
**    (3)  The subquery is not part of a left operand for a RIGHT JOIN
**    (4)  The SQLITE_Coroutine optimization disable flag is not set
**    (5)  The subquery is not self-joined
*/
fn fromClauseTermCanBeCoroutine(
    db: &Sqlite3,
    pTabList: SrcListId,
    mut i: i32,
    selFlags: u32,
) -> bool {
    let pItem0 = SrcItemId(pTabList, i as u32);
    if db.item(pItem0).fg.isCte != 0 {
        let pCteUse = db.arena.cteuse(db.item(pItem0).u2.pCteUse);
        if pCteUse.eM10d == M10d_Yes {
            return false; /* (2a) */
        }
        if pCteUse.nUse >= 2 && pCteUse.eM10d != M10d_No {
            return false; /* (2b) */
        }
    }
    let tl = db.src(pTabList);
    if (tl.a[0].fg.jointype & JT_LTORJ) != 0 {
        return false; /* (3)  */
    }
    if db.OptimizationDisabled(SQLITE_Coroutines) {
        return false; /* (4)  */
    }
    if isSelfJoinView(db, pTabList, pItem0, i + 1, tl.nSrc()).is_some() {
        return false; /* (5) */
    }
    if i == 0 {
        if tl.nSrc() == 1 {
            return true; /* (1a) */
        }
        if (tl.a[1].fg.jointype & JT_CROSS) != 0 {
            return true; /* (1b) */
        }
        if (selFlags & SF_UpdateFrom) != 0 {
            return false; /* (1c-iii) */
        }
        return true;
    }
    if (selFlags & SF_UpdateFrom) != 0 {
        return false; /* (1c-iii) */
    }
    loop {
        let pItem = &tl.a[i as usize];
        if (pItem.fg.jointype & (JT_OUTER | JT_CROSS)) != 0 {
            return false; /* (1c-ii) */
        }
        if i == 0 {
            break;
        }
        i -= 1;
        if tl.a[i as usize].fg.isSubquery != 0 {
            return false; /* (1c-i) */
        }
    }
    true
}

/*
** Argument pWhere is the WHERE clause belonging to SELECT statement p. This
** function attempts to transform expressions of the form:
**
**     EXISTS (SELECT ...)
**
** into joins. For example, given
**
**    CREATE TABLE sailors(sid INTEGER PRIMARY KEY, name TEXT);
**    CREATE TABLE reserves(sid INT, day DATE, PRIMARY KEY(sid, day));
**
**    SELECT name FROM sailors AS S WHERE EXISTS (
**      SELECT * FROM reserves AS R WHERE S.sid = R.sid AND R.day = '2022-10-25'
**    );
**
** the SELECT statement may be transformed as follows:
**
**    SELECT name FROM sailors AS S, reserves AS R
**      WHERE S.sid = R.sid AND R.day = '2022-10-25';
**
** **Approximately**.  Really, we have to ensure that the FROM-clause term
** that was formerly inside the EXISTS is only executed once.  This is handled
** by setting the SrcItem.fg.fromExists flag, which then causes code in
** the where.c file to exit the corresponding loop after the first successful
** match (if any).
*/
fn existsToJoin(db: &mut Sqlite3, p: SelectId, pWhere: ExprId) {
    if db.ppr().nErr == 0
        && pWhere.nn()
        && !db.ex(pWhere).HasProperty(EP_OuterON | EP_InnerON)
        && db.sel(p).pSrc.nn()
        && db.src(db.sel(p).pSrc).nSrc() < BMS
        && (db.sel(p).pLimit.is_null() || db.ex(db.sel(p).pLimit).pRight.is_null())
    {
        let (op, pLeft, pRight, pSub) = {
            let e = db.ex(pWhere);
            (e.op, e.pLeft, e.pRight, e.x.pSelect)
        };
        if op == TK_AND {
            existsToJoin(db, p, pLeft);
            existsToJoin(db, p, pRight);
        } else if op == TK_EXISTS {
            let pSubWhere = db.sel(pSub).pWhere;
            let ok = {
                let ss = db.sel(pSub);
                db.src(ss.pSrc).nSrc() == 1
                    && (ss.selFlags & SF_Aggregate) == 0
                    && db.src(ss.pSrc).a[0].fg.isSubquery == 0
                    && ss.pLimit.is_null()
                    && ss.pPrior.is_null()
            };
            if ok {
                /* Before combining the sub-select with the parent, renumber the
                 ** cursor used by the subselect. This is because the EXISTS expression
                 ** might be a copy of another EXISTS expression from somewhere
                 ** else in the tree, and in this case it is important that it use
                 ** a unique cursor number.  */
                let nTab = db.ppr().nTab;
                let mut aCsrMap = vec![0i32; (nTab + 2) as usize];
                aCsrMap[0] = nTab + 1;
                renumberCursors(db, pSub, -1, &mut aCsrMap);

                *db.exm(pWhere) = Expr {
                    op: TK_INTEGER,
                    u: ExprU {
                        zToken: None,
                        iValue: 1,
                    },
                    flags: EP_IntValue,
                    ..Expr::default()
                };
                debug_assert!(db.sel(p).pWhere.nn());
                let subSrc = db.sel(pSub).pSrc;
                db.srcm(subSrc).a[0].fg.fromExists = 1;
                let ps = db.sel(p).pSrc;
                let ns = sqlite3SrcListAppendList(db, ps, subSrc);
                db.selm(p).pSrc = ns;
                if pSubWhere.nn() {
                    let pw = db.sel(p).pWhere;
                    let nw = sqlite3PExpr(db, TK_AND as i32, pw, pSubWhere);
                    db.selm(p).pWhere = nw;
                    db.selm(pSub).pWhere = ExprId::NULL;
                }
                db.selm(pSub).pSrc = SrcListId::NULL;
                /* (pSub is freed with the arena) */
                let ps = db.sel(p).pSrc;
                let last = db.src(ps).nSrc() - 1;
                recomputeColumnsUsed(db, p, SrcItemId(ps, last as u32));
            }
        }
    }
}

/*
** Type used for Walker callbacks by selectCheckOnClauses().
** (Rust: the chain of contexts is a stack stored in Walker::u.any; the
** last element is the current context, earlier ones are its parents.)
*/
#[derive(Clone, Copy)]
struct CheckOnCtx {
    /// SrcList for this context
    pSrc: SrcListId,
    /// Cursors must be left of this one, if not negative
    iJoin: i32,
    /// True for table-function arg
    bFuncArg: i32,
}

/*
** True if the SrcList passed as the only argument contains at least
** one RIGHT or FULL JOIN. False otherwise.
*/
fn hasRightJoin(db: &Sqlite3, pSrc: SrcListId) -> bool {
    (db.src(pSrc).a[0].fg.jointype & JT_LTORJ) != 0
}

fn checkOnStack<'a>(w: &'a mut Option<Box<dyn std::any::Any>>) -> &'a mut Vec<CheckOnCtx> {
    w.as_mut()
        .and_then(|a| a.downcast_mut::<Vec<CheckOnCtx>>())
        .expect("CheckOnCtx")
}

/*
** The xExpr callback for the search of invalid ON clause terms.
*/
fn selectCheckOnClausesExpr(pWalker: &mut Walker, pExpr: ExprId) -> i32 {
    let (eflags, iJoinE, op, iTab) = {
        let e = pWalker.db.ex(pExpr);
        (e.flags, e.w.iJoin, e.op, e.iTable)
    };
    let top = *checkOnStack(&mut pWalker.u.any).last().unwrap();

    /* Check if pExpr is root or near-root of an ON clause constraint that needs
     ** to be checked to ensure that it does not refer to tables in its FROM
     ** clause to the right of itself. i.e. it is either:
     **
     **   + an ON clause on an OUTER join, or
     **   + an ON clause on an INNER join within a FROM that features at
     **     least one RIGHT or FULL join.
     */
    if (eflags & EP_OuterON) != 0
        || ((eflags & EP_InnerON) != 0 && hasRightJoin(pWalker.db, top.pSrc))
    {
        /* If CheckOnCtx.iJoin is already set, then fall through and process
         ** this expression node as normal. Or, if CheckOnCtx.iJoin is still 0,
         ** set it to the cursor number of the RHS of the join to which this
         ** ON expression was attached and then iterate through the entire
         ** expression.  */
        debug_assert!(top.iJoin < 0 || top.iJoin == iJoinE);
        if top.iJoin < 0 {
            checkOnStack(&mut pWalker.u.any).last_mut().unwrap().iJoin = iJoinE;
            sqlite3WalkExprNN(pWalker, pExpr);
            checkOnStack(&mut pWalker.u.any).last_mut().unwrap().iJoin = -1;
            return WRC_Prune;
        }
    }

    if op == TK_COLUMN {
        /* A column expression. Find the SrcList (if any) to which it refers.
         ** Then, if CheckOnCtx.iJoin indicates that this expression is part of an
         ** ON clause from that SrcList (i.e. if iJoin is non-zero), check that it
         ** does not refer to a table to the right of CheckOnCtx.iJoin. */
        let stack: Vec<CheckOnCtx> = checkOnStack(&mut pWalker.u.any).clone();
        let db = &mut *pWalker.db;
        for pCtx in stack.iter().rev() {
            let pSrc = pCtx.pSrc;
            let nSrc = db.src(pSrc).nSrc();
            let mut ii = 0;
            while ii < nSrc && db.src(pSrc).a[ii as usize].iCursor != iTab {
                ii += 1;
            }
            if ii < nSrc {
                /* pSrc is the FROM clause that contains iTab */
                if pCtx.iJoin >= 0 {
                    ii -= 1;
                    while ii >= 0 && db.src(pSrc).a[ii as usize].iCursor != pCtx.iJoin {
                        ii -= 1;
                    }
                    if ii >= 0 {
                        /* Table iJoin appears to the left of table iTab in the SrcList.
                         ** Therefore the expression refers to a table to its right. */
                        let z: &[u8] = if pCtx.bFuncArg != 0 {
                            b"table-function argument"
                        } else {
                            b"ON clause"
                        };
                        errmsg!(db, b"%s references tables to its right", z);
                        return WRC_Abort;
                    }
                }
                break;
            }
        }
    }
    WRC_Continue
}

/*
** The xSelect callback for the search of invalid ON clause terms.
*/
fn selectCheckOnClausesSelect(pWalker: &mut Walker, pSelect: SelectId) -> i32 {
    let topSrc = checkOnStack(&mut pWalker.u.any).last().unwrap().pSrc;
    let pSrc = pWalker.db.sel(pSelect).pSrc;
    if pSrc == topSrc || pWalker.db.src(pSrc).nSrc() == 0 {
        WRC_Continue
    } else {
        checkOnStack(&mut pWalker.u.any).push(CheckOnCtx {
            pSrc,
            iJoin: -1,
            bFuncArg: 0,
        });
        sqlite3WalkSelect(pWalker, pSelect);
        checkOnStack(&mut pWalker.u.any).pop();
        pWalker.db.selm(pSelect).selFlags &= !SF_OnToWhere;
        WRC_Prune
    }
}

/*
** Check all ON clauses in pSelect to verify that they do not reference
** columns to the right.
*/
pub fn sqlite3SelectCheckOnClauses(db: &mut Sqlite3, pSelect: SelectId) {
    debug_assert!((db.sel(pSelect).selFlags & SF_OnToWhere) != 0);
    let pSrc = db.sel(pSelect).pSrc;
    debug_assert!(pSrc.nn() && db.src(pSrc).nSrc() >= 2);
    let pWhere = db.sel(pSelect).pWhere;
    let mut w = Walker::new(db);
    w.xExprCallback = Some(selectCheckOnClausesExpr);
    w.xSelectCallback = Some(selectCheckOnClausesSelect);
    w.u.any = Some(Box::new(vec![CheckOnCtx {
        pSrc,
        iJoin: -1,
        bFuncArg: 0,
    }]));
    sqlite3WalkExpr(&mut w, pWhere);
    w.db.selm(pSelect).selFlags &= !SF_OnToWhere;

    /* Check for any table-function args that are attached to virtual tables
     ** on the RHS of an outer join. They are subject to similar constraints
     ** as ON clauses. Specifically:
     **
     ** * If the table-valued function is to the left of any RIGHT JOIN, or if
     **   it is the RHS of a RIGHT JOIN, then the table-valued function arguments
     **   may not refer to any tables to the right of this one.
     **
     ** * If the table-valued function is to the left of a LEFT JOIN, then it
     **   may not refer to any table that occurs to the right of the LEFT JOIN.
     */
    checkOnStack(&mut w.u.any)[0].bFuncArg = 1;
    let n = w.db.src(pSrc).nSrc();
    for ii in 0..n {
        let (isTabFunc, jointype, iCursor, pFuncArg) = {
            let it = &w.db.src(pSrc).a[ii as usize];
            (it.fg.isTabFunc, it.fg.jointype, it.iCursor, it.u1.pFuncArg)
        };
        if isTabFunc != 0 {
            let mut iJoin = -1;
            if (jointype & (JT_LTORJ | JT_RIGHT)) != 0 {
                iJoin = iCursor;
            } else {
                for jj in (ii + 1)..n {
                    if (w.db.src(pSrc).a[jj as usize].fg.jointype & JT_OUTER) != 0 {
                        iJoin = w.db.src(pSrc).a[(jj - 1) as usize].iCursor;
                        break;
                    }
                }
            }
            checkOnStack(&mut w.u.any)[0].iJoin = iJoin;
            if iJoin >= 0 {
                sqlite3WalkExprList(&mut w, pFuncArg);
            }
        }
    }
}

/*
** If p2 exists and p1 and p2 have the same number of terms, then change
** every term of p1 to have the same sort order as p2 and return true.
**
** If p2 is NULL or p1 and p2 are different lengths, then make no changes
** and return false.
**
** p1 must be non-NULL.
*/
fn sqlite3CopySortOrder(db: &mut Sqlite3, p1: ExprListId, p2: ExprListId) -> bool {
    debug_assert!(p1.nn());
    if p2.nn() && db.el(p1).nExpr() == db.el(p2).nExpr() {
        let n = db.el(p1).nExpr();
        for ii in 0..n as usize {
            let sortFlags = db.el(p2).a[ii].fg.sortFlags & KEYINFO_ORDER_DESC;
            db.elm(p1).a[ii].fg.sortFlags = sortFlags;
        }
        true
    } else {
        false
    }
}

/// IgnorableDistinct(X)
fn IgnorableDistinct(p: &SelectDest) -> bool {
    p.eDest <= SRT_DistQueue
}

/// IgnorableOrderby(X)
fn IgnorableOrderby(p: &SelectDest) -> bool {
    p.eDest <= SRT_Fifo
}

/// Access the Subquery object of a FROM-clause item.
fn subqm(db: &mut Sqlite3, it: SrcItemId) -> &mut Subquery {
    db.itemm(it)
        .u4
        .pSubq
        .as_deref_mut()
        .expect("SrcItem without Subquery")
}

/*
** Generate byte-code for the SELECT statement given in the p argument.
**
** The results are returned according to the SelectDest structure.
** See comments in sqliteInt.h for further information.
**
** This routine returns the number of errors.  If any errors are
** encountered, then an appropriate error message is left in
** pParse->zErrMsg.
**
** This routine does NOT free the Select structure passed in.  The
** calling function needs to do that.
**
** This is a long function.  The following is an outline of the processing
** steps, with tags referencing various milestones:
**
**  *  Resolve names and similar preparation                tag-select-0100
**  *  Scan of the FROM clause                              tag-select-0200
**      +  OUTER JOIN strength reduction                      tag-select-0220
**      +  Sub-query ORDER BY removal                         tag-select-0230
**      +  Query flattening                                   tag-select-0240
**  *  Separate subroutine for compound-SELECT              tag-select-0300
**  *  WHERE-clause constant propagation                    tag-select-0330
**  *  Count()-of-VIEW optimization                         tag-select-0350
**  *  Scan of the FROM clause again                        tag-select-0400
**      +  Authorize unreferenced tables                      tag-select-0410
**      +  Predicate push-down optimization                   tag-select-0420
**      +  Omit unused subquery columns optimization          tag-select-0440
**      +  Generate code to implement subqueries              tag-select-0480
**         -  Co-routines                                       tag-select-0482
**         -  Reuse previously computed CTE                     tag-select-0484
**         -  REuse previously computed VIEW                    tag-select-0486
**         -  Materialize a VIEW or CTE                         tag-select-0488
**  *  DISTINCT ORDER BY -> GROUP BY optimization           tag-select-0500
**  *  Set up for ORDER BY                                  tag-select-0600
**  *  Create output table                                  tag-select-0630
**  *  Prepare registers for LIMIT                          tag-select-0650
**  *  Setup for DISTINCT                                   tag-select-0680
**  *  Generate code for non-aggregate and non-GROUP BY     tag-select-0700
**  *  Generate code for aggregate and/or GROUP BY          tag-select-0800
**      +  GROUP BY queries                                   tag-select-0810
**      +  non-GROUP BY queries                               tag-select-0820
**         -  Special case of count() w/o GROUP BY              tag-select-0821
**         -  General case of non-GROUP BY aggregates           tag-select-0822
**  *  Sort results, as needed                              tag-select-0900
**  *  Internal self-checks                                 tag-select-1000
*/
pub fn sqlite3Select(db: &mut Sqlite3, p: SelectId, pDest: &mut SelectDest) -> i32 {
    let mut rc = 1; /* Value to return from this function */
    let mut sDistinct = DistinctCtx::default(); /* Info on how to code the DISTINCT keyword */
    let mut sSort = SortCtx::default(); /* Info on how to code the ORDER BY clause */
    let mut pMinMaxOrderBy = ExprListId::NULL; /* Added ORDER BY for min/max queries */
    let minMaxFlag: u8; /* Flag for min/max queries */

    sqlite3GetVdbe(db);
    if p.is_null() || db.ppr().nErr != 0 {
        return 1;
    }
    debug_assert!(db.mallocFailed == 0);
    if crate::auth::sqlite3AuthCheck(db, SQLITE_SELECT, None, None, None) != 0 {
        return 1;
    }

    'select_end: {
        /* tag-select-0100 */
        if IgnorableDistinct(pDest) {
            debug_assert!(
                pDest.eDest == SRT_Exists
                    || pDest.eDest == SRT_Discard
                    || pDest.eDest == SRT_DistQueue
                    || pDest.eDest == SRT_DistFifo
            );
            /* All of these destinations are also able to ignore the ORDER BY clause */
            if db.sel(p).pOrderBy.nn() {
                /* (the ORDER BY list is freed with the arena) */
                db.selm(p).pOrderBy = ExprListId::NULL;
            }
            db.selm(p).selFlags &= !SF_Distinct;
        }
        sqlite3SelectPrep(db, p, NcId::NULL);
        if db.ppr().nErr != 0 {
            break 'select_end;
        }
        debug_assert!(db.mallocFailed == 0);
        debug_assert!(db.sel(p).pEList.nn());

        /* If the SF_UFSrcCheck flag is set, then this function is being called
         ** as part of populating the temp table for an UPDATE...FROM statement.
         ** In this case, it is an error if the target object (pSrc->a[0]) name
         ** or alias is duplicated within FROM clause (pSrc->a[1..n]).
         **
         ** Postgres disallows this case too. The reason is that some other
         ** systems handle this case differently, and not all the same way,
         ** which is just confusing. To avoid this, we follow PG's lead and
         ** disallow it altogether.  */
        if (db.sel(p).selFlags & SF_UFSrcCheck) != 0 {
            let ps = db.sel(p).pSrc;
            let p0 = SrcItemId(ps, 0);
            if sameSrcAlias(db, p0, ps) {
                let z: ZStr = match &db.item(p0).zAlias {
                    Some(a) => a.clone(),
                    None => itemTab(db, p0).borrow().zName.clone(),
                };
                errmsg!(
                    db,
                    b"target object/alias may not appear in FROM clause: %s",
                    &z
                );
                break 'select_end;
            }

            /* Clear the SF_UFSrcCheck flag. The check has already been performed,
             ** and leaving this flag set can cause errors if a compound sub-query
             ** in p->pSrc is flattened into this query and this function called
             ** again as part of compound SELECT processing.  */
            db.selm(p).selFlags &= !SF_UFSrcCheck;
        }

        if pDest.eDest == SRT_Output {
            sqlite3GenerateColumnNames(db, p);
        }

        if crate::window::sqlite3WindowRewrite(db, p) != 0 {
            debug_assert!(db.ppr().nErr != 0);
            break 'select_end;
        }
        let mut pTabList = db.sel(p).pSrc;
        let isAgg = ((db.sel(p).selFlags & SF_Aggregate) != 0) as i32;
        sSort.pOrderBy = db.sel(p).pOrderBy;

        /* Try to do various optimizations (flattening subqueries, and strength
         ** reduction of join operators) in the FROM clause up into the main query
         ** tag-select-0200
         */
        let mut i: i32 = 0;
        while db.sel(p).pPrior.is_null() && i < db.src(pTabList).nSrc() {
            let pItem = SrcItemId(pTabList, i as u32);
            let pSub = if db.item(pItem).fg.isSubquery != 0 {
                subqSelect(db, pItem)
            } else {
                SelectId::NULL
            };
            let pTab = itemTab(db, pItem);

            /* The expander should have already created transient Table objects
             ** even for FROM clause elements such as subqueries that do not correspond
             ** to a real table */

            /* Try to simplify joins:
             **
             **      LEFT JOIN  ->  JOIN
             **     RIGHT JOIN  ->  JOIN
             **      FULL JOIN  ->  RIGHT JOIN
             **
             ** If terms of the i-th table are used in the WHERE clause in such a
             ** way that the i-th table cannot be the NULL row of a join, then
             ** perform the appropriate simplification. This is called
             ** "OUTER JOIN strength reduction" in the SQLite documentation.
             ** tag-select-0220
             */
            let (jt, iCursor) = {
                let it = db.item(pItem);
                (it.fg.jointype, it.iCursor)
            };
            if (jt & (JT_LEFT | JT_LTORJ)) != 0
                && {
                    let w = db.sel(p).pWhere;
                    sqlite3ExprImpliesNonNullRow(db, w, iCursor, (jt & JT_LTORJ) as i32) != 0
                }
                && db.OptimizationEnabled(SQLITE_SimplifyJoin)
            {
                if (jt & JT_LEFT) != 0 {
                    if (jt & JT_RIGHT) != 0 {
                        db.itemm(pItem).fg.jointype &= !JT_LEFT;
                    } else {
                        db.itemm(pItem).fg.jointype &= !(JT_LEFT | JT_OUTER);
                        let w = db.sel(p).pWhere;
                        unsetJoinExpr(db, w, iCursor, 0);
                    }
                }
                if (db.item(pItem).fg.jointype & JT_LTORJ) != 0 {
                    let n = db.src(pTabList).nSrc();
                    for j in (i + 1)..n {
                        let pI2 = SrcItemId(pTabList, j as u32);
                        let (jt2, c2) = {
                            let it = db.item(pI2);
                            (it.fg.jointype, it.iCursor)
                        };
                        if (jt2 & JT_RIGHT) != 0 {
                            if (jt2 & JT_LEFT) != 0 {
                                db.itemm(pI2).fg.jointype &= !JT_RIGHT;
                            } else {
                                db.itemm(pI2).fg.jointype &= !(JT_RIGHT | JT_OUTER);
                                let w = db.sel(p).pWhere;
                                unsetJoinExpr(db, w, c2, 1);
                            }
                        }
                    }
                    let mut j = n - 1;
                    while j >= 0 {
                        let it = &mut db.srcm(pTabList).a[j as usize];
                        it.fg.jointype &= !JT_LTORJ;
                        if (it.fg.jointype & JT_RIGHT) != 0 {
                            break;
                        }
                        j -= 1;
                    }
                }
            }

            /* No further action if this term of the FROM clause is not a subquery */
            if pSub.is_null() {
                i += 1;
                continue;
            }

            /* Catch mismatch in the declared columns of a view and the number of
             ** columns in the SELECT on the RHS */
            let nSubE = db.el(db.sel(pSub).pEList).nExpr();
            if pTab.borrow().nCol as i32 != nSubE {
                let (nc, zn) = {
                    let t = pTab.borrow();
                    (t.nCol as i32, t.zName.clone())
                };
                errmsg!(
                    db,
                    b"expected %d columns for '%s' but got %d",
                    nc,
                    &zn,
                    nSubE
                );
                break 'select_end;
            }

            /* Do not attempt the usual optimizations (flattening and ORDER BY
             ** elimination) on a MATERIALIZED common table expression because
             ** a MATERIALIZED common table expression is an optimization fence.
             */
            if db.item(pItem).fg.isCte != 0
                && db.arena.cteuse(db.item(pItem).u2.pCteUse).eM10d == M10d_Yes
            {
                i += 1;
                continue;
            }

            /* Do not try to flatten an aggregate subquery.
             **
             ** Flattening an aggregate subquery is only possible if the outer query
             ** is not a join.  But if the outer query is not a join, then the subquery
             ** will be implemented as a co-routine and there is no advantage to
             ** flattening in that case.
             */
            if (db.sel(pSub).selFlags & SF_Aggregate) != 0 {
                i += 1;
                continue;
            }
            debug_assert!(db.sel(pSub).pGroupBy.is_null());

            /* tag-select-0230:
             ** If a FROM-clause subquery has an ORDER BY clause that is not
             ** really doing anything, then delete it now so that it does not
             ** interfere with query flattening.  See the discussion at
             ** https://sqlite.org/forum/forumpost/2d76f2bcf65d256a
             **
             ** Beware of these cases where the ORDER BY clause may not be safely
             ** omitted:
             **
             **    (1)   There is also a LIMIT clause
             **    (2)   The subquery was added to help with window-function
             **          processing
             **    (3)   The subquery is in the FROM clause of an UPDATE
             **    (4)   The outer query uses an aggregate function other than
             **          the built-in count(), min(), or max().
             **    (5)   The ORDER BY isn't going to accomplish anything because
             **          one of:
             **            (a)  The outer query has a different ORDER BY clause
             **            (b)  The subquery is part of a join
             **          See forum post 062d576715d277c8
             **    (6)   The subquery is not a recursive CTE.  ORDER BY has a different
             **          meaning for recursive CTEs and this optimization does not
             **          apply.
             **
             ** Also retain the ORDER BY if the OmitOrderBy optimization is disabled.
             */
            {
                let ss = db.sel(pSub);
                let sp = db.sel(p);
                if ss.pOrderBy.nn()
                    && (sp.pOrderBy.nn() || db.src(pTabList).nSrc() > 1) /* Condition (5) */
                    && ss.pLimit.is_null()                              /* Condition (1) */
                    && (ss.selFlags & (SF_OrderByReqd | SF_Recursive)) == 0 /* (2) and (6) */
                    && (sp.selFlags & SF_OrderByReqd) == 0              /* Condition (3) and (4) */
                    && db.OptimizationEnabled(SQLITE_OmitOrderBy)
                {
                    /* (the ORDER BY list is freed with the arena) */
                    db.selm(pSub).pOrderBy = ExprListId::NULL;
                }
            }

            /* If the outer query contains a "complex" result set (that is,
             ** if the result set of the outer query uses functions or subqueries)
             ** and if the subquery contains an ORDER BY clause and if
             ** it will be implemented as a co-routine, then do not flatten.  This
             ** restriction allows SQL constructs like this:
             **
             **  SELECT expensive_function(x)
             **    FROM (SELECT x FROM tab ORDER BY y LIMIT 10);
             **
             ** The expensive_function() is only computed on the 10 rows that
             ** are output, rather than every row of the table.
             **
             ** The requirement that the outer query have a complex result set
             ** means that flattening does occur on simpler SQL constraints without
             ** the expensive_function() like:
             **
             **  SELECT x FROM (SELECT x FROM tab ORDER BY y LIMIT 10);
             */
            if db.sel(pSub).pOrderBy.nn()
                && i == 0
                && (db.sel(p).selFlags & SF_ComplexResult) != 0
                && (db.src(pTabList).nSrc() == 1
                    || (db.src(pTabList).a[1].fg.jointype & (JT_OUTER | JT_CROSS)) != 0)
            {
                i += 1;
                continue;
            }

            /* tag-select-0240 */
            if flattenSubquery(db, p, i, isAgg) != 0 {
                if db.ppr().nErr != 0 {
                    break 'select_end;
                }
                /* This subquery can be absorbed into its parent. */
                i = -1;
            }
            pTabList = db.sel(p).pSrc;
            if db.mallocFailed != 0 {
                break 'select_end;
            }
            if !IgnorableOrderby(pDest) {
                sSort.pOrderBy = db.sel(p).pOrderBy;
            }
            i += 1;
        }

        /* Handle compound SELECT statements using the separate multiSelect()
         ** procedure.  tag-select-0300
         */
        if db.sel(p).pPrior.nn() {
            rc = multiSelect(db, p, pDest);
            if db.sel(p).pNext.is_null() {
                sqlite3VdbeExplainPop(db);
            }
            return rc;
        }

        /* If there may be an "EXISTS (SELECT ...)" in the WHERE clause, attempt
         ** to change it into a join.  */
        if db.ppr().bHasExists != 0 && db.OptimizationEnabled(SQLITE_ExistsToJoin) {
            let w = db.sel(p).pWhere;
            existsToJoin(db, p, w);
            pTabList = db.sel(p).pSrc;
        }

        /* Do the WHERE-clause constant propagation optimization if this is
         ** a join.  No need to spend time on this operation for non-join queries
         ** as the equivalent optimization will be handled by query planner in
         ** sqlite3WhereBegin().  tag-select-0330
         */
        {
            let w = db.sel(p).pWhere;
            if w.nn() && db.ex(w).op == TK_AND && db.OptimizationEnabled(SQLITE_PropagateConst) {
                propagateConstants(db, p);
            }
        }

        /* tag-select-0350 */
        if db.OptimizationEnabled(SQLITE_QueryFlattener | SQLITE_CountOfView)
            && countOfViewOptimization(db, p) != 0
        {
            if db.mallocFailed != 0 {
                break 'select_end;
            }
            pTabList = db.sel(p).pSrc;
        }

        /* Loop over all terms in the FROM clause and do two things for each term:
         **
         **   (1) Authorize unreferenced tables
         **   (2) Generate code for all sub-queries
         **
         ** tag-select-0400
         */
        let nSrc = db.src(pTabList).nSrc();
        for i in 0..nSrc {
            let pItem = SrcItemId(pTabList, i as u32);

            /* Authorized unreferenced tables.  tag-select-0410
             **
             ** Issue SQLITE_READ authorizations with a fake column name for any
             ** tables that are referenced but from which no values are extracted.
             ** Examples of where these kinds of null SQLITE_READ authorizations
             ** would occur:
             **
             **     SELECT count(*) FROM t1;   -- SQLITE_READ t1.""
             **     SELECT t1.* FROM t1, t2;   -- SQLITE_READ t2.""
             **
             ** The fake column name is an empty string.  It is possible for a table to
             ** have a column named by the empty string, in which case there is no way to
             ** distinguish between an unreferenced table and an actual reference to the
             ** "" column. The original design was for the fake column name to be a NULL,
             ** which would be unambiguous.  But legacy authorization callbacks might
             ** assume the column name is non-NULL and segfault.  The use of an empty
             ** string for the fake column name seems safer.
             */
            let (colUsed, zName) = {
                let it = db.item(pItem);
                (it.colUsed, it.zName.clone())
            };
            if colUsed == 0 && zName.is_some() {
                let zDb: Option<ZStr> = {
                    let it = db.item(pItem);
                    if it.fg.fixedSchema != 0 {
                        let sid = it.u4.pSchema.unwrap_or_default();
                        let iDb = crate::prepare::sqlite3SchemaToIndex(db, sid);
                        if iDb >= 0 {
                            db.aDb[iDb as usize].zDbSName.clone()
                        } else {
                            None
                        }
                    } else if it.fg.isSubquery != 0 {
                        None
                    } else {
                        it.u4.zDatabase.clone()
                    }
                };
                crate::auth::sqlite3AuthCheck(
                    db,
                    SQLITE_READ,
                    zName.as_deref(),
                    Some(b""),
                    zDb.as_deref(),
                );
            }

            /* Generate code for all sub-queries in the FROM clause
             */
            if db.item(pItem).fg.isSubquery == 0 {
                continue;
            }
            let pSub = subqSelect(db, pItem);

            /* The code for a subquery should only be generated once. */
            if subqm(db, pItem).addrFillSub != 0 {
                continue;
            }

            /* Increment Parse.nHeight by the height of the largest expression
             ** tree referred to by this, the parent select. The child select
             ** may contain expression trees of at most
             ** (SQLITE_MAX_EXPR_DEPTH-Parse.nHeight) height. This is a bit
             ** more conservative than necessary, but much easier than enforcing
             ** an exact limit.
             */
            let h = sqlite3SelectExprHeight(db, p);
            db.pp().nHeight += h;

            /* Make copies of constant WHERE-clause terms in the outer query down
             ** inside the subquery.  This can help the subquery to run more efficiently.
             ** This is the "predicate push-down optimization".  tag-select-0420
             */
            if db.OptimizationEnabled(SQLITE_PushDown)
                && (db.item(pItem).fg.isCte == 0 || {
                    let cu = db.arena.cteuse(db.item(pItem).u2.pCteUse);
                    cu.eM10d != M10d_Yes && cu.nUse < 2
                })
            {
                let w = db.sel(p).pWhere;
                if pushDownWhereTerms(db, pSub, w, pTabList, i) != 0 {
                    debug_assert!((db.sel(pSub).selFlags & SF_PushDown) != 0);
                }
            }

            /* Convert unused result columns of the subquery into simple NULL
             ** expressions, to avoid unneeded searching and computation.
             ** tag-select-0440
             */
            if db.OptimizationEnabled(SQLITE_NullUnusedCols) {
                disableUnusedSubqueryResultColumns(db, pItem);
            }

            let zSavedAuthContext = db.ppr().zAuthContext.clone();
            db.pp().zAuthContext = zName.clone();

            /* Generate byte-code to implement the subquery  tag-select-0480
             */
            let selFlagsP = db.sel(p).selFlags;
            let pPrior;
            if fromClauseTermCanBeCoroutine(db, pTabList, i, selFlagsP) {
                /* Implement a co-routine that will return a single row of the result
                 ** set on each invocation.  tag-select-0482
                 */
                let addrTop = sqlite3VdbeCurrentAddr(db.v()) + 1;

                db.pp().nMem += 1;
                let regReturn = db.ppr().nMem;
                subqm(db, pItem).regReturn = regReturn;
                sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regReturn, 0, addrTop);
                subqm(db, pItem).addrFillSub = addrTop;
                let mut dest = selectDest(SRT_Coroutine, regReturn);
                explainQP(db, 1, b"CO-ROUTINE %!S", &[PfArg::from(pItem)]);
                sqlite3Select(db, pSub, &mut dest);
                let nsr = db.sel(pSub).nSelectRow;
                itemTab(db, pItem).borrow_mut().nRowLogEst = nsr;
                db.itemm(pItem).fg.viaCoroutine = 1;
                subqm(db, pItem).regResult = dest.iSdst;
                sqlite3VdbeEndCoroutine(db, regReturn);
                sqlite3VdbeJumpHere(db.v(), addrTop - 1);
                sqlite3ClearTempRegCache(db);
            } else if db.item(pItem).fg.isCte != 0
                && db.arena.cteuse(db.item(pItem).u2.pCteUse).addrM9e > 0
            {
                /* This is a CTE for which materialization code has already been
                 ** generated.  Invoke the subroutine to compute the materialization,
                 ** then make the pItem->iCursor be a copy of the ephemeral table that
                 ** holds the result of the materialization. tag-select-0484 */
                let pCteUse = *db.arena.cteuse(db.item(pItem).u2.pCteUse);
                let iCursor = db.item(pItem).iCursor;
                sqlite3VdbeAddOp2(db.v(), OP_Gosub, pCteUse.regRtn, pCteUse.addrM9e);
                if iCursor != pCteUse.iCur {
                    sqlite3VdbeAddOp2(db.v(), OP_OpenDup, iCursor, pCteUse.iCur);
                }
                db.selm(pSub).nSelectRow = pCteUse.nRowEst;
            } else if {
                pPrior = isSelfJoinView(db, pTabList, pItem, 0, i);
                pPrior.is_some()
            } {
                /* This view has already been materialized by a prior entry in
                 ** this same FROM clause.  Reuse it.  tag-select-0486 */
                let pPrior = pPrior.unwrap();
                debug_assert!(db.item(pPrior).fg.isSubquery != 0);
                let pPriorSubq = **db.item(pPrior).u4.pSubq.as_ref().unwrap();
                if pPriorSubq.addrFillSub != 0 {
                    sqlite3VdbeAddOp2(
                        db.v(),
                        OP_Gosub,
                        pPriorSubq.regReturn,
                        pPriorSubq.addrFillSub,
                    );
                }
                let (c1, c2) = (db.item(pItem).iCursor, db.item(pPrior).iCursor);
                sqlite3VdbeAddOp2(db.v(), OP_OpenDup, c1, c2);
                let nsr = db.sel(pPriorSubq.pSelect).nSelectRow;
                db.selm(pSub).nSelectRow = nsr;
            } else {
                /* Materialize the view.  If the view is not correlated, generate a
                 ** subroutine to do the materialization so that subsequent uses of
                 ** the same view can reuse the materialization.  tag-select-0488 */
                let mut onceAddr = 0;

                db.pp().nMem += 1;
                let regReturn = db.ppr().nMem;
                subqm(db, pItem).regReturn = regReturn;
                let topAddr = sqlite3VdbeAddOp0(db.v(), OP_Goto);
                subqm(db, pItem).addrFillSub = topAddr + 1;
                db.itemm(pItem).fg.isMaterialized = 1;
                if db.item(pItem).fg.isCorrelated == 0 {
                    /* If the subquery is not correlated and if we are not inside of
                     ** a trigger, then we only need to compute the value of the subquery
                     ** once. */
                    onceAddr = sqlite3VdbeAddOp0(db.v(), OP_Once);
                }
                let iCursor = db.item(pItem).iCursor;
                let mut dest = selectDest(SRT_EphemTab, iCursor);

                explainQP(db, 1, b"MATERIALIZE %!S", &[PfArg::from(pItem)]);
                sqlite3Select(db, pSub, &mut dest);
                let nsr = db.sel(pSub).nSelectRow;
                itemTab(db, pItem).borrow_mut().nRowLogEst = nsr;
                let v = db.v();
                if onceAddr != 0 {
                    sqlite3VdbeJumpHere(v, onceAddr);
                }
                sqlite3VdbeAddOp2(v, OP_Return, regReturn, topAddr + 1);
                sqlite3VdbeJumpHere(v, topAddr);
                sqlite3ClearTempRegCache(db);
                if db.item(pItem).fg.isCte != 0 && db.item(pItem).fg.isCorrelated == 0 {
                    let cu = db.item(pItem).u2.pCteUse;
                    let addrFillSub = subqm(db, pItem).addrFillSub;
                    let pCteUse = db.arena.cteusem(cu);
                    pCteUse.addrM9e = addrFillSub;
                    pCteUse.regRtn = regReturn;
                    pCteUse.iCur = iCursor;
                    pCteUse.nRowEst = nsr;
                }
            }
            if db.mallocFailed != 0 {
                break 'select_end;
            }
            let h = sqlite3SelectExprHeight(db, p);
            db.pp().nHeight -= h;
            db.pp().zAuthContext = zSavedAuthContext;
        }

        /* Various elements of the SELECT copied into local variables for
         ** convenience */
        let pEList = db.sel(p).pEList;
        let mut pWhere = db.sel(p).pWhere;
        let mut pGroupBy = db.sel(p).pGroupBy;
        let pHaving = db.sel(p).pHaving;
        sDistinct.isTnct = ((db.sel(p).selFlags & SF_Distinct) != 0) as u8;

        /* tag-select-0500
         **
         ** If the query is DISTINCT with an ORDER BY but is not an aggregate, and
         ** if the select-list is the same as the ORDER BY list, then this query
         ** can be rewritten as a GROUP BY. In other words, this:
         **
         **     SELECT DISTINCT xyz FROM ... ORDER BY xyz
         **
         ** is transformed to:
         **
         **     SELECT xyz FROM ... GROUP BY xyz ORDER BY xyz
         **
         ** The second form is preferred as a single index (or temp-table) may be
         ** used for both the ORDER BY and DISTINCT processing. As originally
         ** written the query must use a temp-table for at least one of the ORDER
         ** BY and DISTINCT, and an index or separate temp-table for the other.
         */
        if (db.sel(p).selFlags & (SF_Distinct | SF_Aggregate)) == SF_Distinct
            && sqlite3CopySortOrder(db, pEList, sSort.pOrderBy)
            && sqlite3ExprListCompare(db, pEList, sSort.pOrderBy, -1) == 0
            && db.OptimizationEnabled(SQLITE_GroupByOrder)
            && db.sel(p).pWin.is_null()
        {
            db.selm(p).selFlags &= !SF_Distinct;
            pGroupBy = sqlite3ExprListDup(db, pEList, 0);
            db.selm(p).pGroupBy = pGroupBy;
            if pGroupBy.nn() {
                for (i, it) in db.elm(pGroupBy).a.iter_mut().enumerate() {
                    it.u.x.iOrderByCol = (i + 1) as u16;
                }
            }
            db.selm(p).selFlags |= SF_Aggregate;
            /* Notice that even thought SF_Distinct has been cleared from p->selFlags,
             ** the sDistinct.isTnct is still set.  Hence, isTnct represents the
             ** original setting of the SF_Distinct flag, not the current setting */
            debug_assert!(sDistinct.isTnct != 0);
            sDistinct.isTnct = 2;
        }

        /* If there is an ORDER BY clause, then create an ephemeral index to
         ** do the sorting.  But this sorting ephemeral index might end up
         ** being unused if the data can be extracted in pre-sorted order.
         ** If that is the case, then the OP_OpenEphemeral instruction will be
         ** changed to an OP_Noop once we figure out that the sorting index is
         ** not needed.  The sSort.addrSortIndex variable is used to facilitate
         ** that change.  tag-select-0600
         */
        if sSort.pOrderBy.nn() {
            let nE = db.el(pEList).nExpr();
            let pKeyInfo = sqlite3KeyInfoFromExprList(db, sSort.pOrderBy, 0, nE);
            sSort.iECursor = db.ppr().nTab;
            db.pp().nTab += 1;
            let nOB = db.el(sSort.pOrderBy).nExpr();
            sSort.addrSortIndex = sqlite3VdbeAddOp4(
                db.v(),
                OP_OpenEphemeral,
                sSort.iECursor,
                nOB + 1 + nE,
                0,
                p4key(pKeyInfo),
                P4_KEYINFO as i32,
            );
        } else {
            sSort.addrSortIndex = -1;
        }

        /* If the output is destined for a temporary table, open that table.
         ** tag-select-0630
         */
        if pDest.eDest == SRT_EphemTab {
            let nE = db.el(pEList).nExpr();
            sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, pDest.iSDParm, nE);
            if (db.sel(p).selFlags & SF_NestedFrom) != 0 {
                /* Delete or NULL-out result columns that will never be used */
                let mut ii = db.el(pEList).nExpr() - 1;
                while ii > 0 && db.el(pEList).a[ii as usize].fg.bUsed == 0 {
                    let e = db.el(pEList).a[ii as usize].pExpr;
                    sqlite3ExprDelete(db, e);
                    db.elm(pEList).a.pop();
                    ii -= 1;
                }
                let n = db.el(pEList).nExpr();
                for ii in 0..n as usize {
                    if db.el(pEList).a[ii].fg.bUsed == 0 {
                        let e = db.el(pEList).a[ii].pExpr;
                        db.exm(e).op = TK_NULL;
                    }
                }
            }
        }

        /* Set the limiter.  tag-select-0650
         */
        let iEnd = sqlite3VdbeMakeLabel(db);
        if (db.sel(p).selFlags & SF_FixedLimit) == 0 {
            db.selm(p).nSelectRow = 320; /* 4 billion rows */
        }
        if db.sel(p).pLimit.nn() {
            computeLimitRegisters(db, p, iEnd);
        }
        if db.sel(p).iLimit == 0 && sSort.addrSortIndex >= 0 {
            sqlite3VdbeChangeOpcode(db.v(), sSort.addrSortIndex, OP_SorterOpen);
            sSort.sortFlags |= SORTFLAG_UseSorter;
        }

        /* Open an ephemeral index to use for the distinct set. tag-select-0680
         */
        if (db.sel(p).selFlags & SF_Distinct) != 0 {
            sDistinct.tabTnct = db.ppr().nTab;
            db.pp().nTab += 1;
            let el = db.sel(p).pEList;
            let ki = sqlite3KeyInfoFromExprList(db, el, 0, 0);
            sDistinct.addrTnct = sqlite3VdbeAddOp4(
                db.v(),
                OP_OpenEphemeral,
                sDistinct.tabTnct,
                0,
                0,
                p4key(ki),
                P4_KEYINFO as i32,
            );
            sqlite3VdbeChangeP5(db.v(), BTREE_UNORDERED as u16);
            sDistinct.eTnctType = WHERE_DISTINCT_UNORDERED;
        } else {
            sDistinct.eTnctType = WHERE_DISTINCT_NOOP;
        }

        if isAgg == 0 && pGroupBy.is_null() {
            /* No aggregate functions and no GROUP BY clause.  tag-select-0700 */
            let wctrlFlags: u16 = (if sDistinct.isTnct != 0 {
                WHERE_WANT_DISTINCT
            } else {
                0
            }) | ((db.sel(p).selFlags & SF_FixedLimit) as u16);
            let pWin = db.sel(p).pWin; /* Main window object (or NULL) */
            if pWin.nn() {
                crate::window::sqlite3WindowCodeInit(db, p);
            }
            debug_assert!(WHERE_USE_LIMIT as u32 == SF_FixedLimit);

            /* Begin the database scan. */
            let el = db.sel(p).pEList;
            let nsr = db.sel(p).nSelectRow as i32;
            let mut pWInfo = match crate::where_::sqlite3WhereBegin(
                db,
                pTabList,
                pWhere,
                sSort.pOrderBy,
                el,
                p,
                wctrlFlags,
                nsr,
            ) {
                Some(w) => w,
                None => break 'select_end,
            };
            let orc = crate::where_::sqlite3WhereOutputRowCount(&mut pWInfo);
            if orc < db.sel(p).nSelectRow {
                db.selm(p).nSelectRow = orc;
                if pDest.eDest <= SRT_DistQueue && pDest.eDest >= SRT_DistFifo {
                    /* TUNING: For a UNION CTE, because UNION is implies DISTINCT,
                     ** reduce the estimated output row count by 8 (LogEst 30).
                     ** Search for tag-20250414a to see other cases */
                    db.selm(p).nSelectRow -= 30;
                }
            }
            if sDistinct.isTnct != 0 && crate::where_::sqlite3WhereIsDistinct(&mut pWInfo) != 0 {
                sDistinct.eTnctType = crate::where_::sqlite3WhereIsDistinct(&mut pWInfo) as u8;
            }
            if sSort.pOrderBy.nn() {
                sSort.nOBSat = crate::where_::sqlite3WhereIsOrdered(&mut pWInfo);
                sSort.labelOBLopt = crate::where_::sqlite3WhereOrderByLimitOptLabel(&mut pWInfo);
                if sSort.nOBSat == db.el(sSort.pOrderBy).nExpr() {
                    sSort.pOrderBy = ExprListId::NULL;
                }
            }

            /* If sorting index that was created by a prior OP_OpenEphemeral
             ** instruction ended up not being needed, then change the OP_OpenEphemeral
             ** into an OP_Noop.
             */
            if sSort.addrSortIndex >= 0 && sSort.pOrderBy.is_null() {
                sqlite3VdbeChangeToNoop(db.v(), sSort.addrSortIndex);
            }

            debug_assert!(db.sel(p).pEList == pEList);
            if pWin.nn() {
                let addrGosub = sqlite3VdbeMakeLabel(db);
                let iCont = sqlite3VdbeMakeLabel(db);
                let iBreak = sqlite3VdbeMakeLabel(db);
                db.pp().nMem += 1;
                let regGosub = db.ppr().nMem;

                crate::window::sqlite3WindowCodeStep(db, p, &mut pWInfo, regGosub, addrGosub);

                sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, iBreak);
                sqlite3VdbeResolveLabel(db.v(), addrGosub);
                sSort.labelOBLopt = 0;
                selectInnerLoop(
                    db,
                    p,
                    -1,
                    Some(&mut sSort),
                    Some(&mut sDistinct),
                    pDest,
                    iCont,
                    iBreak,
                );
                sqlite3VdbeResolveLabel(db.v(), iCont);
                sqlite3VdbeAddOp1(db.v(), OP_Return, regGosub);
                sqlite3VdbeResolveLabel(db.v(), iBreak);
            } else {
                /* Use the standard inner loop. */
                let c = crate::where_::sqlite3WhereContinueLabel(&mut pWInfo);
                let b = crate::where_::sqlite3WhereBreakLabel(&mut pWInfo);
                selectInnerLoop(
                    db,
                    p,
                    -1,
                    Some(&mut sSort),
                    Some(&mut sDistinct),
                    pDest,
                    c,
                    b,
                );

                /* End the database scan loop.
                 */
                crate::where_::sqlite3WhereEnd(db, &mut pWInfo);
            }
        } else {
            /* This case is for when there exist aggregate functions or a GROUP BY
             ** clause or both.  tag-select-0800 */
            let mut sortPTab = 0; /* Pseudotable used to decode sorting results */
            let mut sortOut = 0; /* Output register from the sorter */
            let mut orderByGrp = false; /* True if the GROUP BY and ORDER BY are the same */

            /* Remove any and all aliases between the result set and the
             ** GROUP BY clause.
             */
            if pGroupBy.nn() {
                let el = db.sel(p).pEList;
                for it in db.elm(el).a.iter_mut() {
                    it.u.x.iAlias = 0;
                }
                for it in db.elm(pGroupBy).a.iter_mut() {
                    it.u.x.iAlias = 0;
                }
                debug_assert!(66 == crate::util::sqlite3LogEst(100));
                if db.sel(p).nSelectRow > 66 {
                    db.selm(p).nSelectRow = 66;
                }

                /* If there is both a GROUP BY and an ORDER BY clause and they are
                 ** identical, then it may be possible to disable the ORDER BY clause
                 ** on the grounds that the GROUP BY will cause elements to come out
                 ** in the correct order. It also may not - the GROUP BY might use a
                 ** database index that causes rows to be grouped together as required
                 ** but not actually sorted. Either way, record the fact that the
                 ** ORDER BY and GROUP BY clauses are the same by setting the orderByGrp
                 ** variable.  */
                if sqlite3CopySortOrder(db, pGroupBy, sSort.pOrderBy)
                    && sqlite3ExprListCompare(db, pGroupBy, sSort.pOrderBy, -1) == 0
                {
                    orderByGrp = true;
                }
            } else {
                debug_assert!(0 == crate::util::sqlite3LogEst(1));
                db.selm(p).nSelectRow = 0;
            }

            /* Create a label to jump to when we want to abort the query */
            let addrEnd = sqlite3VdbeMakeLabel(db);

            /* Convert TK_COLUMN nodes into TK_AGG_COLUMN and make entries in
             ** sAggInfo for all TK_AGG_FUNCTION nodes in expressions of the
             ** SELECT statement.
             */
            let selId = db.sel(p).selId;
            let pAggInfo = db.arena.new_agg(AggInfo {
                selId,
                ..AggInfo::default()
            });
            if db.mallocFailed != 0 {
                break 'select_end;
            }
            let sNC = db.arena.new_nc(NameContext {
                pSrcList: pTabList,
                uNC: NameContextU {
                    pAggInfo,
                    ..NameContextU::default()
                },
                ncFlags: NC_UAggInfo,
                ..NameContext::default()
            });
            {
                let nsc = if pGroupBy.nn() {
                    db.el(pGroupBy).nExpr()
                } else {
                    0
                };
                let ai = db.arena.aggm(pAggInfo);
                ai.nSortingColumn = nsc as u32;
                ai.pGroupBy = pGroupBy;
            }
            sqlite3ExprAnalyzeAggList(db, sNC, pEList);
            sqlite3ExprAnalyzeAggList(db, sNC, sSort.pOrderBy);
            if pHaving.nn() {
                if pGroupBy.nn() {
                    debug_assert!(pWhere == db.sel(p).pWhere);
                    debug_assert!(pHaving == db.sel(p).pHaving);
                    debug_assert!(pGroupBy == db.sel(p).pGroupBy);
                    havingToWhere(db, p);
                    pWhere = db.sel(p).pWhere;
                }
                sqlite3ExprAnalyzeAggregates(db, sNC, pHaving);
            }
            {
                let ai = db.arena.aggm(pAggInfo);
                ai.nAccumulator = ai.nColumn();
            }
            if db.sel(p).pGroupBy.is_null()
                && db.sel(p).pHaving.is_null()
                && db.arena.agg(pAggInfo).nFunc() == 1
            {
                let fe = db.arena.agg(pAggInfo).aFunc[0].pFExpr;
                minMaxFlag = minMaxQuery(db, fe, &mut pMinMaxOrderBy);
            } else {
                minMaxFlag = WHERE_ORDERBY_NORMAL as u8;
            }
            analyzeAggFuncArgs(db, pAggInfo, sNC);
            if db.mallocFailed != 0 {
                break 'select_end;
            }

            /* Processing for aggregates with GROUP BY is very different and
             ** much more complex than aggregates without a GROUP BY.  tag-select-0810
             */
            if pGroupBy.nn() {
                let groupBySort; /* Rows come from source in GROUP BY order */
                let mut pDistinct = ExprListId::NULL;
                let mut distFlag: u16 = 0;
                let nGroupBy = db.el(pGroupBy).nExpr();

                {
                    let ai = db.arena.agg(pAggInfo);
                    if ai.nFunc() == 1
                        && ai.aFunc[0].iDistinct >= 0
                        && ai.aFunc[0].pFExpr.nn()
                        && db.ex(ai.aFunc[0].pFExpr).UseXList()
                        && db.ex(ai.aFunc[0].pFExpr).x.pList.nn()
                    {
                        let fl = db.ex(ai.aFunc[0].pFExpr).x.pList;
                        let mut pExpr = db.el(fl).a[0].pExpr;
                        pExpr = sqlite3ExprDup(db, pExpr, 0);
                        pDistinct = sqlite3ExprListDup(db, pGroupBy, 0);
                        pDistinct = sqlite3ExprListAppend(db, pDistinct, pExpr);
                        distFlag = if pDistinct.nn() {
                            WHERE_WANT_DISTINCT | WHERE_AGG_DISTINCT
                        } else {
                            0
                        };
                    }
                }

                /* If there is a GROUP BY clause we might need a sorting index to
                 ** implement it.  Allocate that sorting index now.  If it turns out
                 ** that we do not need it after all, the OP_SorterOpen instruction
                 ** will be converted into a Noop.
                 */
                let sortingIdx = db.ppr().nTab;
                db.pp().nTab += 1;
                db.arena.aggm(pAggInfo).sortingIdx = sortingIdx;
                let nColumn = db.arena.agg(pAggInfo).nColumn();
                let pKeyInfo = sqlite3KeyInfoFromExprList(db, pGroupBy, 0, nColumn);
                let nSortingColumn = db.arena.agg(pAggInfo).nSortingColumn as i32;
                let addrSortingIdx = sqlite3VdbeAddOp4(
                    db.v(),
                    OP_SorterOpen,
                    sortingIdx,
                    nSortingColumn,
                    0,
                    p4key(pKeyInfo.clone()),
                    P4_KEYINFO as i32,
                );

                /* Initialize memory locations used by GROUP BY aggregate processing
                 */
                db.pp().nMem += 1;
                let iUseFlag = db.ppr().nMem;
                db.pp().nMem += 1;
                let iAbortFlag = db.ppr().nMem;
                db.pp().nMem += 1;
                let regOutputRow = db.ppr().nMem;
                let mut addrOutputRow = sqlite3VdbeMakeLabel(db);
                db.pp().nMem += 1;
                let regReset = db.ppr().nMem;
                let addrReset = sqlite3VdbeMakeLabel(db);
                let iAMem = db.ppr().nMem + 1;
                db.pp().nMem += nGroupBy;
                let iBMem = db.ppr().nMem + 1;
                db.pp().nMem += nGroupBy;
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, iAbortFlag);
                sqlite3VdbeAddOp3(db.v(), OP_Null, 0, iAMem, iAMem + nGroupBy - 1);
                sqlite3ExprNullRegisterRange(db, iAMem, nGroupBy);

                /* Begin a loop that will extract all source rows in GROUP BY order.
                 ** This might involve two separate loops with an OP_Sort in between, or
                 ** it might be a single loop that uses an index to extract information
                 ** in the right order to begin with.
                 */
                sqlite3VdbeAddOp2(db.v(), OP_Gosub, regReset, addrReset);
                let wflags: u16 = (if sDistinct.isTnct == 2 {
                    WHERE_DISTINCTBY
                } else {
                    WHERE_GROUPBY
                }) | (if orderByGrp { WHERE_SORTBYGROUP } else { 0 })
                    | distFlag;
                let mut pWInfo = match crate::where_::sqlite3WhereBegin(
                    db, pTabList, pWhere, pGroupBy, pDistinct, p, wflags, 0,
                ) {
                    Some(w) => w,
                    None => {
                        sqlite3ExprListDelete(db, pDistinct);
                        break 'select_end;
                    }
                };
                if !db.ppr().pIdxEpr.is_empty() {
                    optimizeAggregateUseOfIndexedExpr(db, p, pAggInfo, sNC);
                }
                assignAggregateRegisters(db, pAggInfo);
                let eDist = crate::where_::sqlite3WhereIsDistinct(&mut pWInfo);
                if crate::where_::sqlite3WhereIsOrdered(&mut pWInfo) == nGroupBy {
                    /* The optimizer is able to deliver rows in group by order so
                     ** we do not have to sort.  The OP_OpenEphemeral table will be
                     ** cancelled later because we still need to use the pKeyInfo
                     */
                    groupBySort = false;
                } else {
                    /* Rows are coming out in undetermined order.  We have to push
                     ** each row into a sorting index, terminate the first loop,
                     ** then loop over the sorting index in order to get the output
                     ** in sorted order
                     */
                    let z: &[u8] =
                        if sDistinct.isTnct != 0 && (db.sel(p).selFlags & SF_Distinct) == 0 {
                            b"DISTINCT"
                        } else {
                            b"GROUP BY"
                        };
                    explainQP(db, 0, b"USE TEMP B-TREE FOR %s", &[PfArg::from(z)]);

                    groupBySort = true;
                    let mut nCol = nGroupBy;
                    let mut j = nGroupBy;
                    let nColumn = db.arena.agg(pAggInfo).nColumn();
                    for i in 0..nColumn {
                        if db.arena.agg(pAggInfo).aCol[i as usize].iSorterColumn >= j {
                            nCol += 1;
                            j += 1;
                        }
                    }
                    let regBase = sqlite3GetTempRange(db, nCol);
                    sqlite3ExprCodeExprList(db, pGroupBy, regBase, 0, 0);
                    j = nGroupBy;
                    db.arena.aggm(pAggInfo).directMode = 1;
                    for i in 0..nColumn {
                        let (isc, pCExpr) = {
                            let c = &db.arena.agg(pAggInfo).aCol[i as usize];
                            (c.iSorterColumn, c.pCExpr)
                        };
                        if isc >= j {
                            sqlite3ExprCode(db, pCExpr, j + regBase);
                            j += 1;
                        }
                    }
                    db.arena.aggm(pAggInfo).directMode = 0;
                    let regRecord = sqlite3GetTempReg(db);
                    sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, regBase, nCol, regRecord);
                    sqlite3VdbeAddOp2(db.v(), OP_SorterInsert, sortingIdx, regRecord);
                    sqlite3ReleaseTempReg(db, regRecord);
                    sqlite3ReleaseTempRange(db, regBase, nCol);
                    crate::where_::sqlite3WhereEnd(db, &mut pWInfo);
                    sortPTab = db.ppr().nTab;
                    db.pp().nTab += 1;
                    db.arena.aggm(pAggInfo).sortingIdxPTab = sortPTab;
                    sortOut = sqlite3GetTempReg(db);
                    sqlite3VdbeAddOp3(db.v(), OP_OpenPseudo, sortPTab, sortOut, nCol);
                    sqlite3VdbeAddOp2(db.v(), OP_SorterSort, sortingIdx, addrEnd);
                    db.arena.aggm(pAggInfo).useSortingIdx = 1;
                }

                /* If there are entries in pAgggInfo->aFunc[] that contain subexpressions
                 ** that are indexed (and that were previously identified and tagged
                 ** in optimizeAggregateUseOfIndexedExpr()) then those subexpressions
                 ** must now be converted into a TK_AGG_COLUMN node so that the value
                 ** is correctly pulled from the index rather than being recomputed. */
                if !db.ppr().pIdxEpr.is_empty() {
                    aggregateConvertIndexedExprRefToColumn(db, pAggInfo);
                }

                /* If the index or temporary table used by the GROUP BY sort
                 ** will naturally deliver rows in the order required by the ORDER BY
                 ** clause, cancel the ephemeral table open coded earlier.
                 **
                 ** This is an optimization - the correct answer should result regardless.
                 ** Use the SQLITE_GroupByOrder flag with SQLITE_TESTCTRL_OPTIMIZER to
                 ** disable this optimization for testing purposes.  */
                if orderByGrp
                    && db.OptimizationEnabled(SQLITE_GroupByOrder)
                    && (groupBySort || crate::where_::sqlite3WhereIsSorted(&mut pWInfo) != 0)
                {
                    sSort.pOrderBy = ExprListId::NULL;
                    sqlite3VdbeChangeToNoop(db.v(), sSort.addrSortIndex);
                }

                /* Evaluate the current GROUP BY terms and store in b0, b1, b2...
                 ** (b0 is memory location iBMem+0, b1 is iBMem+1, and so forth)
                 ** Then compare the current GROUP BY terms against the GROUP BY terms
                 ** from the previous row currently stored in a0, a1, a2...
                 */
                let addrTopOfLoop = sqlite3VdbeCurrentAddr(db.v());
                if groupBySort {
                    sqlite3VdbeAddOp3(db.v(), OP_SorterData, sortingIdx, sortOut, sortPTab);
                }
                for j in 0..nGroupBy {
                    let (iOrderByCol, gbExpr) = {
                        let it = &db.el(pGroupBy).a[j as usize];
                        (it.u.x.iOrderByCol as i32, it.pExpr)
                    };

                    if groupBySort {
                        sqlite3VdbeAddOp3(db.v(), OP_Column, sortPTab, j, iBMem + j);
                    } else {
                        db.arena.aggm(pAggInfo).directMode = 1;
                        sqlite3ExprCode(db, gbExpr, iBMem + j);
                    }

                    if iOrderByCol != 0 {
                        let el = db.sel(p).pEList;
                        let mut pX = db.el(el).a[(iOrderByCol - 1) as usize].pExpr;
                        let mut pBase = sqlite3ExprSkipCollateAndLikely(db, pX);
                        while pBase.nn() && db.ex(pBase).op == TK_IF_NULL_ROW {
                            pX = db.ex(pBase).pLeft;
                            pBase = sqlite3ExprSkipCollateAndLikely(db, pX);
                        }
                        if pBase.nn()
                            && db.ex(pBase).op != TK_AGG_COLUMN
                            && db.ex(pBase).op != TK_REGISTER
                        {
                            sqlite3ExprToRegister(db, pX, iAMem + j);
                        }
                    }
                }
                let kr = match &pKeyInfo {
                    Some(k) => sqlite3KeyInfoRef(k),
                    None => None,
                };
                let v = db.v();
                sqlite3VdbeAddOp4(
                    v,
                    OP_Compare,
                    iAMem,
                    iBMem,
                    nGroupBy,
                    p4key(kr),
                    P4_KEYINFO as i32,
                );
                let addr1 = sqlite3VdbeCurrentAddr(v);
                sqlite3VdbeAddOp3(v, OP_Jump, addr1 + 1, 0, addr1 + 1);

                /* Generate code that runs whenever the GROUP BY changes.
                 ** Changes in the GROUP BY are detected by the previous code
                 ** block.  If there were no changes, this block is skipped.
                 **
                 ** This code copies current group by terms in b0,b1,b2,...
                 ** over to a0,a1,a2.  It then calls the output subroutine
                 ** and resets the aggregate accumulator registers in preparation
                 ** for the next GROUP BY batch.
                 */
                sqlite3VdbeAddOp2(v, OP_Gosub, regOutputRow, addrOutputRow);
                sqlite3ExprCodeMove(db, iBMem, iAMem, nGroupBy);
                let v = db.v();
                sqlite3VdbeAddOp2(v, OP_IfPos, iAbortFlag, addrEnd);
                sqlite3VdbeAddOp2(v, OP_Gosub, regReset, addrReset);

                /* Update the aggregate accumulators based on the content of
                 ** the current row
                 */
                sqlite3VdbeJumpHere(v, addr1);
                updateAccumulator(db, iUseFlag, pAggInfo, eDist);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, iUseFlag);

                /* End of the loop
                 */
                if groupBySort {
                    sqlite3VdbeAddOp2(db.v(), OP_SorterNext, sortingIdx, addrTopOfLoop);
                } else {
                    crate::where_::sqlite3WhereEnd(db, &mut pWInfo);
                    sqlite3VdbeChangeToNoop(db.v(), addrSortingIdx);
                }
                sqlite3ExprListDelete(db, pDistinct);

                /* Output the final row of result
                 */
                let v = db.v();
                sqlite3VdbeAddOp2(v, OP_Gosub, regOutputRow, addrOutputRow);

                /* Jump over the subroutines
                 */
                sqlite3VdbeGoto(v, addrEnd);

                /* Generate a subroutine that outputs a single row of the result
                 ** set.  This subroutine first looks at the iUseFlag.  If iUseFlag
                 ** is less than or equal to zero, the subroutine is a no-op.  If
                 ** the processing calls for the query to abort, this subroutine
                 ** increments the iAbortFlag memory location before returning in
                 ** order to signal the caller to abort.
                 */
                let addrSetAbort = sqlite3VdbeCurrentAddr(v);
                sqlite3VdbeAddOp2(v, OP_Integer, 1, iAbortFlag);
                sqlite3VdbeAddOp1(v, OP_Return, regOutputRow);
                sqlite3VdbeResolveLabel(v, addrOutputRow);
                addrOutputRow = sqlite3VdbeCurrentAddr(v);
                sqlite3VdbeAddOp2(v, OP_IfPos, iUseFlag, addrOutputRow + 2);
                sqlite3VdbeAddOp1(v, OP_Return, regOutputRow);
                finalizeAggFunctions(db, pAggInfo);
                sqlite3ExprIfFalse(db, pHaving, addrOutputRow + 1, SQLITE_JUMPIFNULL as i32);
                selectInnerLoop(
                    db,
                    p,
                    -1,
                    Some(&mut sSort),
                    Some(&mut sDistinct),
                    pDest,
                    addrOutputRow + 1,
                    addrSetAbort,
                );
                sqlite3VdbeAddOp1(db.v(), OP_Return, regOutputRow);

                /* Generate a subroutine that will reset the group-by accumulator
                 */
                sqlite3VdbeResolveLabel(db.v(), addrReset);
                resetAccumulator(db, pAggInfo);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, iUseFlag);
                sqlite3VdbeAddOp1(db.v(), OP_Return, regReset);

                if distFlag != 0 && eDist != WHERE_DISTINCT_NOOP as i32 {
                    let (iDistinct, iDistAddr) = {
                        let f = &db.arena.agg(pAggInfo).aFunc[0];
                        (f.iDistinct, f.iDistAddr)
                    };
                    fixDistinctOpenEph(db, eDist, iDistinct, iDistAddr);
                }
            }
            /* endif pGroupBy.  Begin aggregate queries without GROUP BY: */
            else {
                /* Aggregate functions without GROUP BY. tag-select-0820 */
                if let Some(pTab) = isSimpleCount(db, p, pAggInfo) {
                    /* tag-select-0821
                     **
                     ** If isSimpleCount() returns a pointer to a Table structure, then
                     ** the SQL statement is of the form:
                     **
                     **   SELECT count(*) FROM <tbl>
                     **
                     ** where the Table structure returned represents table <tbl>.
                     **
                     ** This statement is so common that it is optimized specially. The
                     ** OP_Count instruction is executed either on the intkey table that
                     ** contains the data for table <tbl> or on one of its indexes. It
                     ** is better to execute the op on an index, as indexes are almost
                     ** always spread across less pages than their corresponding tables.
                     */
                    let iDb = crate::prepare::sqlite3SchemaToIndex(db, pTab.borrow().pSchema);
                    let iCsr = db.ppr().nTab; /* Cursor to scan b-tree */
                    db.pp().nTab += 1;
                    let mut pKeyInfo: Option<KeyInfoRef> = None; /* Keyinfo for scanned index */
                    let mut pBest: Option<IndexRef> = None; /* Best index found so far */
                    let (mut iRoot, tabName, hasRowid, szTabRow) = {
                        let t = pTab.borrow();
                        (t.tnum, t.zName.clone(), t.HasRowid(), t.szTabRow)
                    }; /* Root page of scanned b-tree */

                    crate::build::sqlite3CodeVerifySchema(db, iDb);
                    sqlite3TableLock(db, iDb, iRoot, 0, Some(&tabName));

                    /* Search for the index that has the lowest scan cost.
                     **
                     ** (2011-04-15) Do not do a full scan of an unordered index.
                     **
                     ** (2013-10-03) Do not count the entries in a partial index.
                     **
                     ** In practice the KeyInfo structure will not be used. It is only
                     ** passed to keep OP_OpenRead happy.
                     */
                    if !hasRowid {
                        pBest = sqlite3PrimaryKeyIndex(&pTab);
                    }
                    let ps = db.sel(p).pSrc;
                    if db.src(ps).a[0].fg.notIndexed == 0 {
                        let idxs = pTab.borrow().pIndex.clone();
                        for pIdx in idxs.iter() {
                            let better = {
                                let x = pIdx.borrow();
                                x.bUnordered == 0
                                    && x.szIdxRow < szTabRow
                                    && x.pPartIdxWhere.is_none()
                                    && match &pBest {
                                        None => true,
                                        Some(b) => x.szIdxRow < b.borrow().szIdxRow,
                                    }
                            };
                            if better {
                                pBest = Some(pIdx.clone());
                            }
                        }
                    }
                    if let Some(b) = &pBest {
                        iRoot = b.borrow().tnum;
                        pKeyInfo = sqlite3KeyInfoOfIndex(db, b);
                    }

                    /* Open a read-only cursor, execute the OP_Count, close the cursor. */
                    sqlite3VdbeAddOp4Int(db.v(), OP_OpenRead, iCsr, iRoot as i32, iDb, 1);
                    if let Some(k) = pKeyInfo {
                        sqlite3VdbeChangeP4(db.v(), -1, P4::KeyInfo(k), P4_KEYINFO as i32);
                    }
                    assignAggregateRegisters(db, pAggInfo);
                    let r = db.arena.agg(pAggInfo).AggInfoFuncReg(0);
                    sqlite3VdbeAddOp2(db.v(), OP_Count, iCsr, r);
                    sqlite3VdbeAddOp1(db.v(), OP_Close, iCsr);
                    explainSimpleCount(db, &pTab, &pBest);
                } else {
                    /* The general case of an aggregate query without GROUP BY
                     ** tag-select-0822 */
                    let mut regAcc = 0; /* "populate accumulators" flag */
                    let mut pDistinct = ExprListId::NULL;
                    let mut distFlag: u16 = 0;

                    /* If there are accumulator registers but no min() or max() functions
                     ** without FILTER clauses, allocate register regAcc. Register regAcc
                     ** will contain 0 the first time the inner loop runs, and 1 thereafter.
                     ** The code generated by updateAccumulator() uses this to ensure
                     ** that the accumulator registers are (a) updated only once if
                     ** there are no min() or max functions or (b) always updated for the
                     ** first row visited by the aggregate, so that they are updated at
                     ** least once even if the FILTER clause means the min() or max()
                     ** function visits zero rows.  */
                    let (nAcc, nFunc) = {
                        let ai = db.arena.agg(pAggInfo);
                        (ai.nAccumulator, ai.nFunc())
                    };
                    if nAcc != 0 {
                        let mut i = 0;
                        while i < nFunc {
                            let f = &db.arena.agg(pAggInfo).aFunc[i as usize];
                            if db.ex(f.pFExpr).HasProperty(EP_WinFunc) {
                                i += 1;
                                continue;
                            }
                            if (f.pFunc.as_ref().unwrap().funcFlags & SQLITE_FUNC_NEEDCOLL) != 0 {
                                break;
                            }
                            i += 1;
                        }
                        if i == nFunc {
                            db.pp().nMem += 1;
                            regAcc = db.ppr().nMem;
                            sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regAcc);
                        }
                    } else if nFunc == 1 && db.arena.agg(pAggInfo).aFunc[0].iDistinct >= 0 {
                        let fe = db.arena.agg(pAggInfo).aFunc[0].pFExpr;
                        debug_assert!(db.ex(fe).UseXList());
                        pDistinct = db.ex(fe).x.pList;
                        distFlag = if pDistinct.nn() {
                            WHERE_WANT_DISTINCT | WHERE_AGG_DISTINCT
                        } else {
                            0
                        };
                    }
                    assignAggregateRegisters(db, pAggInfo);

                    /* This case runs if the aggregate has no GROUP BY clause.  The
                     ** processing is much simpler since there is only a single row
                     ** of output.
                     */
                    debug_assert!(db.sel(p).pGroupBy.is_null());
                    resetAccumulator(db, pAggInfo);

                    /* If this query is a candidate for the min/max optimization, then
                     ** minMaxFlag will have been previously set to either
                     ** WHERE_ORDERBY_MIN or WHERE_ORDERBY_MAX and pMinMaxOrderBy will
                     ** be an appropriate ORDER BY expression for the optimization.
                     */
                    debug_assert!(minMaxFlag == WHERE_ORDERBY_NORMAL as u8 || pMinMaxOrderBy.nn());

                    let mut pWInfo = match crate::where_::sqlite3WhereBegin(
                        db,
                        pTabList,
                        pWhere,
                        pMinMaxOrderBy,
                        pDistinct,
                        p,
                        minMaxFlag as u16 | distFlag,
                        0,
                    ) {
                        Some(w) => w,
                        None => break 'select_end,
                    };
                    let eDist = crate::where_::sqlite3WhereIsDistinct(&mut pWInfo);
                    updateAccumulator(db, regAcc, pAggInfo, eDist);
                    if eDist != WHERE_DISTINCT_NOOP as i32 {
                        if !db.arena.agg(pAggInfo).aFunc.is_empty() {
                            let (iDistinct, iDistAddr) = {
                                let f = &db.arena.agg(pAggInfo).aFunc[0];
                                (f.iDistinct, f.iDistAddr)
                            };
                            fixDistinctOpenEph(db, eDist, iDistinct, iDistAddr);
                        }
                    }

                    if regAcc != 0 {
                        sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, regAcc);
                    }
                    if minMaxFlag != 0 {
                        crate::where_::sqlite3WhereMinMaxOptEarlyOut(db.v(), &mut pWInfo);
                    }
                    crate::where_::sqlite3WhereEnd(db, &mut pWInfo);
                    finalizeAggFunctions(db, pAggInfo);
                }

                sSort.pOrderBy = ExprListId::NULL;
                sqlite3ExprIfFalse(db, pHaving, addrEnd, SQLITE_JUMPIFNULL as i32);
                selectInnerLoop(db, p, -1, None, None, pDest, addrEnd, addrEnd);
            }
            sqlite3VdbeResolveLabel(db.v(), addrEnd);
        } /* endif aggregate query */

        if sDistinct.eTnctType == WHERE_DISTINCT_UNORDERED {
            explainTempTable(db, b"DISTINCT");
        }

        /* If there is an ORDER BY clause, then we need to sort the results
         ** and send them to the callback one by one.  tag-select-0900
         */
        if sSort.pOrderBy.nn() {
            debug_assert!(db.sel(p).pEList == pEList);
            let n = db.el(pEList).nExpr();
            generateSortTail(db, p, &mut sSort, n, pDest);
        }

        /* Jump here to skip this query
         */
        sqlite3VdbeResolveLabel(db.v(), iEnd);

        /* The SELECT has been coded. If there is an error in the Parse structure,
         ** set the return code to 1. Otherwise 0. */
        rc = (db.ppr().nErr > 0) as i32;
    }

    /* Control jumps to here if an error is encountered above, or upon
     ** successful coding of the SELECT.
     */
    /* select_end: */
    debug_assert!(db.mallocFailed == 0 || db.ppr().nErr != 0);
    sqlite3ExprListDelete(db, pMinMaxOrderBy);
    sqlite3VdbeExplainPop(db);
    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_names() {
        assert_eq!(selectOpName(TK_ALL as i32), b"UNION ALL");
        assert_eq!(selectOpName(TK_INTERSECT as i32), b"INTERSECT");
        assert_eq!(selectOpName(TK_EXCEPT as i32), b"EXCEPT");
        assert_eq!(selectOpName(TK_UNION as i32), b"UNION");
        assert_eq!(
            sqlite3SelectOpName(TK_SELECT as i32),
            Some(b"UNION".to_vec())
        );
    }
}
