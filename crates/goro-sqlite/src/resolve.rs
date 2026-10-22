//! Port of resolve.c.
//!
//! This file contains routines used for walking the parser tree and
//! resolve all identifiers by associating them with a particular table
//! and column.
//!
//! NameContexts live in the arena (`NcId`); the tree walker carries the
//! current name context in `w.u.pNC`.  C stack-allocated NameContexts are
//! allocated in the arena too (they are freed when the arena is reset).

use crate::alter::sqlite3RenameTokenRemap;
use crate::auth::{sqlite3AuthCheck, sqlite3AuthRead};
use crate::build::{sqlite3IdListIndex, sqlite3TableColumnToStorage, sqlite3WritableSchema};
use crate::callback::sqlite3FindFunction;
use crate::expr::*;
use crate::int::*;
use crate::printf::{PfArg, sqlite3_log, sqlite3RecordErrorOffsetOfExpr};
use crate::select::{
    sqlite3ColumnIndex, sqlite3SelectCheckOnClauses, sqlite3SelectPrep,
    sqlite3SelectWrongNumTermsError, sqlite3SrcItemColumnUsed,
};
use crate::util::{sqlite3_stricmp, sqlite3_strnicmp, sqlite3AtoF, sqlite3StrICmp};
use crate::walker::{sqlite3WalkExpr, sqlite3WalkExprList, sqlite3WalkExprNN, sqlite3WalkSelect};
use crate::window::{sqlite3WindowLink, sqlite3WindowUnlinkFromSelect, sqlite3WindowUpdate};

/// Magic table number to mean the EXCLUDED table in an UPSERT statement.
const EXCLUDED_TABLE_NUMBER: i32 = 2;

#[inline]
fn nc(db: &Sqlite3, p: NcId) -> &NameContext {
    db.arena.nc(p)
}
#[inline]
fn ncm(db: &mut Sqlite3, p: NcId) -> &mut NameContext {
    db.arena.ncm(p)
}

/// The schema id of a table, None for the C NULL schema (ephemeral and
/// subquery tables, whose `pSchema` is the default `SchemaId(0)`).
#[inline]
fn schema_opt(id: SchemaId) -> Option<SchemaId> {
    if id.0 == 0 { None } else { Some(id) }
}

/*
** Walk the expression tree pExpr and increase the aggregate function
** depth (the Expr.op2 field) by N on every TK_AGG_FUNCTION node.
** This needs to occur when copying a TK_AGG_FUNCTION node from an
** outer query into an inner subquery.
**
** incrAggFunctionDepth(pExpr,n) is the main routine.  incrAggDepth(..)
** is a helper function - a callback for the tree walker.
**
** See also the sqlite3WindowExtraAggFuncDepth() routine in window.c
*/
fn incrAggDepth(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let n = w.u.n;
    let e = w.db.exm(pExpr);
    if e.op == TK_AGG_FUNCTION {
        e.op2 = e.op2.wrapping_add(n as u8);
    }
    WRC_Continue
}
fn incrAggFunctionDepth(db: &mut Sqlite3, pExpr: ExprId, N: i32) {
    if N > 0 {
        let mut w = Walker::new(db);
        w.xExprCallback = Some(incrAggDepth);
        w.u.n = N;
        sqlite3WalkExpr(&mut w, pExpr);
    }
}

/*
** Turn the pExpr expression into an alias for the iCol-th column of the
** result set in pEList.
**
** If the reference is followed by a COLLATE operator, then make sure
** the COLLATE operator is preserved.  For example:
**
**     SELECT a+b, c+d FROM t1 ORDER BY 1 COLLATE nocase;
**
** Should be transformed into:
**
**     SELECT a+b, c+d FROM t1 ORDER BY (a+b) COLLATE nocase;
**
** The nSubquery parameter specifies how many levels of subquery the
** alias is removed from the original expression.  The usual value is
** zero but it might be more if the alias is contained within a subquery
** of the original expression.  The Expr.op2 field of TK_AGG_FUNCTION
** structures must be increased by the nSubquery amount.
*/
fn resolveAlias(db: &mut Sqlite3, pEList: ExprListId, iCol: i32, pExpr: ExprId, nSubquery: i32) {
    debug_assert!(iCol >= 0 && iCol < db.el(pEList).nExpr());
    let pOrig = db.el(pEList).a[iCol as usize].pExpr;
    debug_assert!(pOrig.nn());
    if db.ex(pExpr).pAggInfo.nn() {
        return;
    }
    let mut pDup = sqlite3ExprDup(db, pOrig, 0);
    if db.mallocFailed != 0 {
        sqlite3ExprDelete(db, pDup);
    } else {
        incrAggFunctionDepth(db, pDup, nSubquery);
        if db.ex(pExpr).op == TK_COLLATE {
            debug_assert!(!db.ex(pExpr).HasProperty(EP_IntValue));
            let z = db.ex(pExpr).u.zToken.clone();
            pDup = sqlite3ExprAddCollateString(db, pDup, z.as_deref());
        }
        /* Swap the content of pDup and pExpr (the C memcpy() dance) */
        let tmp = std::mem::take(db.exm(pDup));
        let orig = std::mem::take(db.exm(pExpr));
        *db.exm(pDup) = orig;
        *db.exm(pExpr) = tmp;
        if db.ex(pExpr).HasProperty(EP_WinFunc) {
            let pWin = db.ex(pExpr).y.pWin;
            if pWin.nn() {
                db.winm(pWin).pOwner = pExpr;
            }
        }
        sqlite3ExprDeferredDelete(db, pDup);
    }
}

/*
** Subqueries store the original database, table and column names for their
** result sets in ExprList.a[].zSpan, in the form "DATABASE.TABLE.COLUMN",
** and mark the expression-list item by setting ExprList.a[].fg.eEName
** to ENAME_TAB.
**
** Check to see if the zSpan/eEName of the expression-list item passed to this
** routine matches the zDb, zTab, and zCol.  If any of zDb, zTab, and zCol are
** NULL then those fields will match anything. Return true if there is a match,
** or false otherwise.
**
** SF_NestedFrom subqueries also store an entry for the implicit rowid (or
** _rowid_, or oid) column by setting ExprList.a[].fg.eEName to ENAME_ROWID,
** and setting zSpan to "DATABASE.TABLE.<rowid-alias>". This type of pItem
** argument matches if zCol is a rowid alias. If it is not NULL, (*pbRowid)
** is set to 1 if there is this kind of match.
*/
/// C: `int sqlite3MatchEName( const struct ExprList_item *pItem, const char *zCol, const char *zTab, const char *zDb, int *pbRowid )`
///
/// Rust: `pItem` is a reference to the ExprList item
/// (`&db.el(list).a[i]`).
pub fn sqlite3MatchEName(
    pItem: &ExprListItem,
    zCol: Option<&[u8]>,
    zTab: Option<&[u8]>,
    zDb: Option<&[u8]>,
    pbRowid: &mut i32,
) -> i32 {
    let eEName = pItem.fg.eEName;
    if eEName != ENAME_TAB && eEName != ENAME_ROWID {
        return 0;
    }
    debug_assert!(*pbRowid == 0);
    let full: &[u8] = pItem.zEName.as_deref().unwrap_or(b"");
    let mut zSpan: &[u8] = full;
    let n = zSpan.iter().position(|&c| c == b'.').unwrap_or(zSpan.len());
    if let Some(zDb) = zDb {
        if sqlite3_strnicmp(Some(zSpan), Some(zDb), n as i32) != 0 || zDb.len() != n {
            return 0;
        }
    }
    zSpan = &zSpan[(n + 1).min(zSpan.len())..];
    let n = zSpan.iter().position(|&c| c == b'.').unwrap_or(zSpan.len());
    if let Some(zTab) = zTab {
        if sqlite3_strnicmp(Some(zSpan), Some(zTab), n as i32) != 0 || zTab.len() != n {
            return 0;
        }
    }
    zSpan = &zSpan[(n + 1).min(zSpan.len())..];
    if let Some(zCol) = zCol {
        if eEName == ENAME_TAB && sqlite3StrICmp(Some(zSpan), Some(zCol)) != 0 {
            return 0;
        }
        if eEName == ENAME_ROWID && sqlite3IsRowid(Some(zCol)) == 0 {
            return 0;
        }
    }
    if eEName == ENAME_ROWID {
        *pbRowid = 1;
    }
    1
}

/*
** Return TRUE if the double-quoted string  mis-feature should be supported.
*/
fn areDoubleQuotedStringsEnabled(db: &mut Sqlite3, pTopNC: NcId) -> bool {
    if db.init.busy != 0 {
        return true; /* Always support for legacy schemas */
    }
    if (nc(db, pTopNC).ncFlags & NC_IsDDL) != 0 {
        /* Currently parsing a DDL statement */
        if sqlite3WritableSchema(db) != 0 && (db.flags & SQLITE_DqsDML) != 0 {
            return true;
        }
        (db.flags & SQLITE_DqsDDL) != 0
    } else {
        /* Currently parsing a DML statement */
        (db.flags & SQLITE_DqsDML) != 0
    }
}

/*
** The argument is guaranteed to be a non-NULL Expr node of type TK_COLUMN.
** return the appropriate colUsed mask.
*/
/// C: `Bitmask sqlite3ExprColUsed(Expr *pExpr)`
pub fn sqlite3ExprColUsed(db: &mut Sqlite3, pExpr: ExprId) -> Bitmask {
    let (mut n, pExTab) = {
        let e = db.ex(pExpr);
        debug_assert!(e.UseYTab());
        (e.iColumn as i32, e.y.pTab.clone())
    };
    let pExTab = pExTab.expect("sqlite3ExprColUsed: no table");
    let t = pExTab.borrow();
    debug_assert!(n < t.nCol as i32);
    if (t.tabFlags & TF_HasGenerated) != 0
        && n >= 0
        && (t.aCol[n as usize].colFlags & COLFLAG_GENERATED) != 0
    {
        if t.nCol as i32 >= BMS {
            ALLBITS
        } else {
            MASKBIT(t.nCol as i32) - 1
        }
    } else {
        if n >= BMS {
            n = BMS - 1;
        }
        (1 as Bitmask) << n
    }
}

/*
** Create a new expression term for the column specified by pMatch and
** iColumn.  Append this new expression term to the FULL JOIN Match set
** in *ppList.  Create a new *ppList if this is the first term in the
** set.
*/
fn extendFJMatch(
    db: &mut Sqlite3,
    ppList: &mut ExprListId,
    pMatch: SrcItemId, /* Source table containing the column */
    iColumn: i16,      /* The column number */
) {
    let pNew = sqlite3ExprAlloc(db, TK_COLUMN as i32, &Token::default(), 0);
    if pNew.nn() {
        let (iCursor, pTab) = {
            let it = db.item(pMatch);
            debug_assert!((it.fg.jointype & (JT_LEFT | JT_LTORJ)) != 0);
            (it.iCursor, it.pSTab.clone())
        };
        let e = db.exm(pNew);
        e.iTable = iCursor;
        e.iColumn = iColumn;
        e.y.pTab = pTab;
        e.SetProperty(EP_CanBeNull);
        *ppList = sqlite3ExprListAppend(db, *ppList, pNew);
    }
}

/*
** Return TRUE (non-zero) if zTab is a valid name for the schema table pTab.
*/
fn isValidSchemaTableName(
    zTab: &[u8],        /* Name as it appears in the SQL */
    pTab: &TableRef,    /* The schema table we are trying to match */
    zDb: Option<&[u8]>, /* non-NULL if a database qualifier is present */
) -> bool {
    debug_assert!(pTab.borrow().tnum == 1);
    if sqlite3_strnicmp(Some(zTab), Some(b"sqlite_"), 7) != 0 {
        return false;
    }
    let zTab7 = zTab.get(7..).unwrap_or(b"");
    let isLegacyTemp = {
        let t = pTab.borrow();
        t.zName.get(7..).unwrap_or(b"") == &b"sqlite_temp_master"[7..]
    };
    if isLegacyTemp {
        if sqlite3StrICmp(Some(zTab7), Some(&b"sqlite_temp_schema"[7..])) == 0 {
            return true;
        }
        if zDb.is_none() {
            return false;
        }
        if sqlite3StrICmp(Some(zTab7), Some(&b"sqlite_master"[7..])) == 0 {
            return true;
        }
        if sqlite3StrICmp(Some(zTab7), Some(&b"sqlite_schema"[7..])) == 0 {
            return true;
        }
    } else if sqlite3StrICmp(Some(zTab7), Some(&b"sqlite_schema"[7..])) == 0 {
        return true;
    }
    false
}

/// Result of the "is there already a match" test of lookupName() when a
/// second matching column is found.
enum Dup {
    /* Two or more tables have the same column name (ambiguous) */
    Ambiguous,
    /* An INNER or LEFT JOIN.  Use the left-most table */
    Skip,
    /* A RIGHT JOIN.  Use the right-most table */
    Restart,
    /* For a FULL JOIN, we must construct a coalesce() func */
    FullJoin,
}

/*
** Given the name of a column of the form X.Y.Z or Y.Z or just Z, look up
** that name in the set of source tables in pSrcList and make the pExpr
** expression node refer back to that source column.  The following changes
** are made to pExpr:
**
**    pExpr->iDb           Set the index in db->aDb[] of the database X
**                         (even if X is implied).
**    pExpr->iTable        Set to the cursor number for the table obtained
**                         from pSrcList.
**    pExpr->y.pTab        Points to the Table structure of X.Y (even if
**                         X and/or Y are implied.)
**    pExpr->iColumn       Set to the column number within the table.
**    pExpr->op            Set to TK_COLUMN.
**    pExpr->pLeft         Any expression this points to is deleted
**    pExpr->pRight        Any expression this points to is deleted.
**
** The zDb variable is the name of the database (the "X").  This value may be
** NULL meaning that name is of the form Y.Z or Z.  Any available database
** can be used.  The zTable variable is the name of the table (the "Y").  This
** value can be NULL if zDb is also NULL.  If zTable is NULL it
** means that the form of the name is Z and that columns from any table
** can be used.
**
** If the name cannot be resolved unambiguously, leave an error message
** in pParse and return WRC_Abort.  Return WRC_Prune on success.
*/
fn lookupName(
    db: &mut Sqlite3,
    zDb: Option<ZStr>,  /* Name of the database containing table, or NULL */
    zTab: Option<ZStr>, /* Name of table containing column, or NULL */
    pRight: ExprId,     /* Name of the column. */
    pNCin: NcId,        /* The name context used to resolve the name */
    pExpr: ExprId,      /* Make this EXPR node point to the selected column */
) -> i32 {
    let mut zDb = zDb;
    let zTab = zTab;
    let mut cnt: i32 = 0; /* Number of matching column names */
    let mut cntTab: i32 = 0; /* Number of potential "rowid" matches */
    let mut nSubquery: i32 = 0; /* How many levels of subquery */
    let mut pMatch: Option<SrcItemId> = None; /* The matching pSrcList item */
    let mut pNC = pNCin;
    let mut pTopNC = pNCin; /* First namecontext in the list */
    let mut pSchema: Option<SchemaId> = None; /* Schema of the expression */
    let mut eNewExprOp: u8 = TK_COLUMN; /* New value for pExpr->op on success */
    let mut pFJMatch = ExprListId::NULL; /* Matches for FULL JOIN .. USING */
    let zCol: ZStr = db.ex(pRight).u.zToken.clone().unwrap_or_default();
    let zColR: &[u8] = &zCol;

    debug_assert!(pNC.nn()); /* the name context cannot be NULL. */
    debug_assert!(zDb.is_none() || zTab.is_some());
    debug_assert!(!db.ex(pExpr).HasProperty(EP_TokenOnly | EP_Reduced));

    /* Initialize the node to no-match */
    db.exm(pExpr).iTable = -1;

    /* Translate the schema name in zDb into a pointer to the corresponding
     ** schema.  If not found, pSchema will remain NULL and nothing will match
     ** resulting in an appropriate error message toward the end of this routine
     */
    if zDb.is_some() {
        if (nc(db, pNC).ncFlags & (NC_PartIdx | NC_IsCheck)) != 0 {
            /* Silently ignore database qualifiers inside CHECK constraints and
             ** partial indices.  Do not raise errors because that might break
             ** legacy and because it does not hurt anything to just ignore the
             ** database name. */
            zDb = None;
        } else {
            let nDb = db.nDb();
            let mut i = 0;
            while i < nDb {
                debug_assert!(db.aDb[i as usize].zDbSName.is_some());
                if sqlite3StrICmp(db.aDb[i as usize].zDbSName.as_deref(), zDb.as_deref()) == 0 {
                    pSchema = Some(db.aDb[i as usize].pSchema.id);
                    break;
                }
                i += 1;
            }
            if i == nDb && sqlite3StrICmp(Some(b"main"), zDb.as_deref()) == 0 {
                /* This branch is taken when the main database has been renamed
                 ** using SQLITE_DBCONFIG_MAINDBNAME. */
                pSchema = Some(db.aDb[0].pSchema.id);
                zDb = db.aDb[0].zDbSName.clone();
            }
        }
    }

    /* Start at the inner-most context and move outward until a match is found */
    let mut goto_end = false;
    loop {
        let pSrcList = nc(db, pNC).pSrcList;

        if pSrcList.nn() {
            let nSrc = db.src(pSrcList).nSrc();
            for i in 0..nSrc {
                let pItem = SrcItemId(pSrcList, i as u32);
                let tab = db.item(pItem).pSTab.clone().expect("SrcItem without table");
                let isNestedFrom = db.item(pItem).fg.isNestedFrom != 0;
                if isNestedFrom {
                    /* In this case, pItem is a subquery that has been formed from a
                     ** parenthesized subset of the FROM clause terms.  Example:
                     **   .... FROM t1 LEFT JOIN (t2 RIGHT JOIN t3 USING(x)) USING(y) ...
                     **                          \_________________________/
                     **             This pItem -------------^
                     */
                    let mut hit = false;
                    debug_assert!(db.item(pItem).fg.isSubquery != 0);
                    let pSel = db
                        .item(pItem)
                        .u4
                        .pSubq
                        .as_ref()
                        .expect("nested FROM")
                        .pSelect;
                    let pEList = db.sel(pSel).pEList;
                    let nExpr = db.el(pEList).nExpr();
                    for j in 0..nExpr {
                        let mut bRowid = 0; /* True if possible rowid match */
                        if sqlite3MatchEName(
                            &db.el(pEList).a[j as usize],
                            Some(zColR),
                            zTab.as_deref(),
                            zDb.as_deref(),
                            &mut bRowid,
                        ) == 0
                        {
                            continue;
                        }
                        if bRowid == 0 {
                            if cnt > 0 {
                                match dupMatch(db, pItem, zColR, pMatch, true) {
                                    Dup::Ambiguous => {
                                        /* Two or more tables have the same column name which is
                                         ** not joined by USING. Or, a single table has two columns
                                         ** that match a USING term (if pMatch==pItem). These are both
                                         ** "ambiguous column name" errors. Signal as much by clearing
                                         ** pFJMatch and letting cnt go above 1. */
                                        sqlite3ExprListDelete(db, pFJMatch);
                                        pFJMatch = ExprListId::NULL;
                                    }
                                    Dup::Skip => continue,
                                    Dup::Restart => {
                                        cnt = 0;
                                        sqlite3ExprListDelete(db, pFJMatch);
                                        pFJMatch = ExprListId::NULL;
                                    }
                                    Dup::FullJoin => {
                                        let iColumn = db.ex(pExpr).iColumn;
                                        extendFJMatch(db, &mut pFJMatch, pMatch.unwrap(), iColumn);
                                    }
                                }
                            }
                            cnt += 1;
                            hit = true;
                        } else if cnt > 0 {
                            /* This is a potential rowid match, but there has already been
                             ** a real match found. So this can be ignored.  */
                            continue;
                        }
                        cntTab += 1;
                        pMatch = Some(pItem);
                        db.exm(pExpr).iColumn = j as i16;
                        db.elm(pEList).a[j as usize].fg.bUsed = 1;

                        /* rowid cannot be part of a USING clause - assert() this. */
                        debug_assert!(
                            bRowid == 0 || db.el(pEList).a[j as usize].fg.bUsingTerm == 0
                        );
                        if db.el(pEList).a[j as usize].fg.bUsingTerm != 0 {
                            break;
                        }
                    }
                    if hit || zTab.is_none() {
                        continue;
                    }
                }
                debug_assert!(zDb.is_none() || zTab.is_some());
                if let Some(zt) = zTab.as_deref() {
                    if let Some(zd) = zDb.as_deref() {
                        if schema_opt(tab.borrow().pSchema) != pSchema {
                            continue;
                        }
                        if pSchema.is_none() && zd != b"*" {
                            continue;
                        }
                    }
                    let zAlias = db.item(pItem).zAlias.clone();
                    if let Some(zAlias) = &zAlias {
                        if sqlite3StrICmp(Some(zt), Some(zAlias)) != 0 {
                            continue;
                        }
                    } else {
                        let nameMatch = {
                            let t = tab.borrow();
                            sqlite3StrICmp(Some(zt), Some(&t.zName)) == 0
                        };
                        if !nameMatch {
                            if tab.borrow().tnum != 1 {
                                continue;
                            }
                            if !isValidSchemaTableName(zt, &tab, zDb.as_deref()) {
                                continue;
                            }
                        }
                    }
                    debug_assert!(db.ex(pExpr).UseYTab());
                    if db.ppr().IN_RENAME_OBJECT() && zAlias.is_some() {
                        sqlite3RenameTokenRemap(db, None, RenameKey::ExprTab(pExpr));
                    }
                }
                let j = sqlite3ColumnIndex(&tab, Some(zColR));
                if j >= 0 {
                    if cnt > 0 {
                        match dupMatch(db, pItem, zColR, pMatch, false) {
                            Dup::Ambiguous => {
                                /* Two or more tables have the same column name which is
                                 ** not joined by USING.  This is an error.  Signal as much
                                 ** by clearing pFJMatch and letting cnt go above 1. */
                                sqlite3ExprListDelete(db, pFJMatch);
                                pFJMatch = ExprListId::NULL;
                            }
                            Dup::Skip => continue,
                            Dup::Restart => {
                                cnt = 0;
                                sqlite3ExprListDelete(db, pFJMatch);
                                pFJMatch = ExprListId::NULL;
                            }
                            Dup::FullJoin => {
                                let iColumn = db.ex(pExpr).iColumn;
                                extendFJMatch(db, &mut pFJMatch, pMatch.unwrap(), iColumn);
                            }
                        }
                    }
                    cnt += 1;
                    pMatch = Some(pItem);
                    /* Substitute the rowid (column -1) for the INTEGER PRIMARY KEY */
                    let iPKey = tab.borrow().iPKey as i32;
                    db.exm(pExpr).iColumn = if j == iPKey { -1 } else { j as i16 };
                    if isNestedFrom {
                        sqlite3SrcItemColumnUsed(db, pItem, j);
                    }
                }
                if 0 == cnt && tab.borrow().VisibleRowid() {
                    /* pTab is a potential ROWID match.  Keep track of it and match
                     ** the ROWID later if that seems appropriate.  (Search for "cntTab"
                     ** to find related code.)  Only allow a ROWID match if there is
                     ** a single ROWID match candidate.
                     */
                    cntTab += 1;
                    pMatch = Some(pItem);
                }
            }
            if let Some(m) = pMatch {
                let (iCursor, mtab, jointype) = {
                    let it = db.item(m);
                    (it.iCursor, it.pSTab.clone(), it.fg.jointype)
                };
                let sch = mtab
                    .as_ref()
                    .map(|t| t.borrow().pSchema)
                    .unwrap_or_default();
                let e = db.exm(pExpr);
                e.iTable = iCursor;
                debug_assert!(e.UseYTab());
                e.y.pTab = mtab;
                if (jointype & (JT_LEFT | JT_LTORJ)) != 0 {
                    e.SetProperty(EP_CanBeNull);
                }
                pSchema = schema_opt(sch);
            }
        } /* if( pSrcList ) */

        /* If we have not already resolved the name, then maybe
         ** it is a new.* or old.* trigger argument reference.  Or
         ** maybe it is an excluded.* from an upsert.  Or maybe it is
         ** a reference in the RETURNING clause to a table being modified.
         */
        if cnt == 0 && zDb.is_none() {
            let mut pTab: Option<TableRef> = None; /* Table holding the row */
            let pTriggerTab = db.ppr().pTriggerTab.clone();
            if let Some(tt) = pTriggerTab {
                let op = db.ppr().eTriggerOp;
                debug_assert!(op == TK_DELETE || op == TK_UPDATE || op == TK_INSERT);
                if db.ppr().bReturning != 0 {
                    if (nc(db, pNC).ncFlags & NC_UBaseReg) != 0
                        && (zTab.is_none()
                            || sqlite3StrICmp(zTab.as_deref(), Some(&tt.borrow().zName)) == 0
                            || isValidSchemaTableName(zTab.as_deref().unwrap(), &tt, None))
                    {
                        db.exm(pExpr).iTable = (op != TK_DELETE) as i32;
                        pTab = Some(tt);
                    }
                } else if op != TK_DELETE
                    && zTab.is_some()
                    && sqlite3StrICmp(Some(b"new"), zTab.as_deref()) == 0
                {
                    db.exm(pExpr).iTable = 1;
                    pTab = Some(tt);
                } else if op != TK_INSERT
                    && zTab.is_some()
                    && sqlite3StrICmp(Some(b"old"), zTab.as_deref()) == 0
                {
                    db.exm(pExpr).iTable = 0;
                    pTab = Some(tt);
                }
            }
            if (nc(db, pNC).ncFlags & NC_UUpsert) != 0 && zTab.is_some() {
                let pUpsert = nc(db, pNC).uNC.pUpsert;
                if pUpsert.nn() && sqlite3StrICmp(Some(b"excluded"), zTab.as_deref()) == 0 {
                    let pUpsertSrc = db.arena.ups(pUpsert).pUpsertSrc;
                    pTab = db.src(pUpsertSrc).a[0].pSTab.clone();
                    db.exm(pExpr).iTable = EXCLUDED_TABLE_NUMBER;
                }
            }

            if let Some(tab) = pTab.clone() {
                let (tabSchema, iPKey, nCol, visibleRowid) = {
                    let t = tab.borrow();
                    (t.pSchema, t.iPKey as i32, t.nCol as i32, t.VisibleRowid())
                };
                pSchema = schema_opt(tabSchema);
                cntTab += 1;
                let mut iCol = sqlite3ColumnIndex(&tab, Some(zColR));
                if iCol >= 0 {
                    if iPKey == iCol {
                        iCol = -1;
                    }
                } else if sqlite3IsRowid(Some(zColR)) != 0 && visibleRowid {
                    iCol = -1;
                } else {
                    iCol = nCol;
                }
                if iCol < nCol {
                    cnt += 1;
                    pMatch = None;
                    if db.ex(pExpr).iTable == EXCLUDED_TABLE_NUMBER {
                        debug_assert!(db.ex(pExpr).UseYTab());
                        if db.ppr().IN_RENAME_OBJECT() {
                            let e = db.exm(pExpr);
                            e.iColumn = iCol as i16;
                            e.y.pTab = Some(tab.clone());
                            eNewExprOp = TK_COLUMN;
                        } else {
                            let pUpsert = nc(db, pNC).uNC.pUpsert;
                            let regData = db.arena.ups(pUpsert).regData;
                            db.exm(pExpr).iTable =
                                regData + sqlite3TableColumnToStorage(&tab, iCol as i16) as i32;
                            eNewExprOp = TK_REGISTER;
                        }
                    } else {
                        debug_assert!(db.ex(pExpr).UseYTab());
                        db.exm(pExpr).y.pTab = Some(tab.clone());
                        if db.ppr().bReturning != 0 {
                            eNewExprOp = TK_REGISTER;
                            let iBaseReg = nc(db, pNC).uNC.iBaseReg;
                            let iStor = sqlite3TableColumnToStorage(&tab, iCol as i16) as i32;
                            let e = db.exm(pExpr);
                            e.op2 = TK_COLUMN;
                            e.iColumn = iCol as i16;
                            e.iTable = iBaseReg + (nCol + 1) * e.iTable + iStor + 1;
                        } else {
                            db.exm(pExpr).iColumn = iCol as i16;
                            eNewExprOp = TK_TRIGGER;
                            if iCol < 0 {
                                db.exm(pExpr).affExpr = SQLITE_AFF_INTEGER;
                            } else if db.ex(pExpr).iTable == 0 {
                                db.pp().oldmask |=
                                    if iCol >= 32 { 0xffffffff } else { 1u32 << iCol };
                            } else {
                                db.pp().newmask |=
                                    if iCol >= 32 { 0xffffffff } else { 1u32 << iCol };
                            }
                        }
                    }
                }
            }
        }

        /*
         ** Perhaps the name is a reference to the ROWID
         */
        if cnt == 0
            && cntTab >= 1
            && pMatch.is_some()
            && (nc(db, pNC).ncFlags & (NC_IdxExpr | NC_GenCol)) == 0
            && sqlite3IsRowid(Some(zColR)) != 0
        {
            let m = pMatch.unwrap();
            let (visible, isNestedFrom) = {
                let it = db.item(m);
                (
                    it.pSTab
                        .as_ref()
                        .map(|t| t.borrow().VisibleRowid())
                        .unwrap_or(false),
                    it.fg.isNestedFrom != 0,
                )
            };
            if visible || isNestedFrom {
                cnt = cntTab;
                if !isNestedFrom {
                    db.exm(pExpr).iColumn = -1;
                }
                db.exm(pExpr).affExpr = SQLITE_AFF_INTEGER;
            }
        }

        /*
         ** If the input is of the form Z (not Y.Z or X.Y.Z) then the name Z
         ** might refer to an result-set alias.  This happens, for example, when
         ** we are resolving names in the WHERE clause of the following command:
         **
         **     SELECT a+b AS x FROM table WHERE x<10;
         **
         ** In cases like this, replace pExpr with a copy of the expression that
         ** forms the result set entry ("a+b" in the example) and return immediately.
         ** Note that the expression in the result set should have already been
         ** resolved by the time the WHERE clause is resolved.
         **
         ** The ability to use an output result-set column in the WHERE, GROUP BY,
         ** or HAVING clauses, or as part of a larger expression in the ORDER BY
         ** clause is not standard SQL.  This is a (goofy) SQLite extension, that
         ** is supported for backwards compatibility only. Hence, we issue a warning
         ** on sqlite3_log() whenever the capability is used.
         */
        if cnt == 0 && (nc(db, pNC).ncFlags & NC_UEList) != 0 && zTab.is_none() {
            let pEList = nc(db, pNC).uNC.pEList;
            debug_assert!(pEList.nn());
            let nExpr = db.el(pEList).nExpr();
            for j in 0..nExpr {
                let (eEName, zAs, pOrig) = {
                    let it = &db.el(pEList).a[j as usize];
                    (it.fg.eEName, it.zEName.clone(), it.pExpr)
                };
                if eEName == ENAME_NAME && sqlite3_stricmp(zAs.as_deref(), Some(zColR)) == 0 {
                    let ncFlags = nc(db, pNC).ncFlags;
                    if (ncFlags & NC_AllowAgg) == 0 && db.ex(pOrig).HasProperty(EP_Agg) {
                        errmsg!(db, "misuse of aliased aggregate %s", zAs.as_deref());
                        return WRC_Abort;
                    }
                    if db.ex(pOrig).HasProperty(EP_Win)
                        && ((ncFlags & NC_AllowWin) == 0 || pNC != pTopNC)
                    {
                        errmsg!(db, "misuse of aliased window function %s", zAs.as_deref());
                        return WRC_Abort;
                    }
                    if sqlite3ExprVectorSize(db, pOrig) != 1 {
                        errmsg!(db, "row value misused");
                        return WRC_Abort;
                    }
                    resolveAlias(db, pEList, j, pExpr, nSubquery);
                    cnt = 1;
                    pMatch = None;
                    debug_assert!(zTab.is_none() && zDb.is_none());
                    if db.ppr().IN_RENAME_OBJECT() {
                        sqlite3RenameTokenRemap(db, None, RenameKey::Expr(pExpr));
                    }
                    goto_end = true;
                    break;
                }
            }
            if goto_end {
                break;
            }
        }

        /* Advance to the next name context.  The loop will exit when either
         ** we have a match (cnt>0) or when we run out of name contexts.
         */
        if cnt != 0 {
            break;
        }
        pNC = nc(db, pNC).pNext;
        nSubquery += 1;
        if pNC.is_null() {
            break;
        }
    }

    if !goto_end {
        /*
         ** If X and Y are NULL (in other words if only the column name Z is
         ** supplied) and the value of Z is enclosed in double-quotes, then
         ** Z is a string literal if it doesn't match any column names.  In that
         ** case, we need to return right away and not make any changes to
         ** pExpr.
         **
         ** Because no reference was made to outer contexts, the pNC->nRef
         ** fields are not changed in any context.
         */
        if cnt == 0 && zTab.is_none() {
            debug_assert!(db.ex(pExpr).op == TK_ID);
            if db.ex(pExpr).HasProperty(EP_DblQuoted) && areDoubleQuotedStringsEnabled(db, pTopNC) {
                /* If a double-quoted identifier does not match any known column name,
                 ** then treat it as a string.
                 **
                 ** This hack was added in the early days of SQLite in a misguided attempt
                 ** to be compatible with MySQL 3.x, which used double-quotes for strings.
                 ** I now sorely regret putting in this hack. The effect of this hack is
                 ** that misspelled identifier names are silently converted into strings
                 ** rather than causing an error, to the frustration of countless
                 ** programmers. To all those frustrated programmers, my apologies.
                 **
                 ** Someday, I hope to get rid of this hack. Unfortunately there is
                 ** a huge amount of legacy SQL that uses it. So for now, we just
                 ** issue a warning.
                 */
                sqlite3_log(
                    SQLITE_WARNING,
                    b"double-quoted string literal: \"%w\"",
                    &[PfArg::from(zColR)],
                );
                let e = db.exm(pExpr);
                e.op = TK_STRING;
                e.y = ExprY::default();
                return WRC_Prune;
            }
            if sqlite3ExprIdToTrueFalse(db, pExpr) != 0 {
                return WRC_Prune;
            }
        }

        /*
         ** cnt==0 means there was not match.
         ** cnt>1 means there were two or more matches.
         **
         ** cnt==0 is always an error.  cnt>1 is often an error, but might
         ** be multiple matches for a NATURAL OUTER JOIN or a OUTER JOIN USING.
         */
        debug_assert!(pFJMatch.is_null() || cnt > 0);
        debug_assert!(!db.ex(pExpr).HasProperty(EP_xIsSelect | EP_IntValue));
        if cnt != 1 {
            if pFJMatch.nn() {
                if db.el(pFJMatch).nExpr() == cnt - 1 {
                    if db.ex(pExpr).HasProperty(EP_Leaf) {
                        db.exm(pExpr).ClearProperty(EP_Leaf);
                    } else {
                        let (l, r) = (db.ex(pExpr).pLeft, db.ex(pExpr).pRight);
                        sqlite3ExprDelete(db, l);
                        db.exm(pExpr).pLeft = ExprId::NULL;
                        sqlite3ExprDelete(db, r);
                        db.exm(pExpr).pRight = ExprId::NULL;
                    }
                    let iColumn = db.ex(pExpr).iColumn;
                    extendFJMatch(db, &mut pFJMatch, pMatch.unwrap(), iColumn);
                    let e = db.exm(pExpr);
                    e.op = TK_FUNCTION;
                    e.u.zToken = Some(b"coalesce".to_vec());
                    e.x.pList = pFJMatch;
                    e.affExpr = SQLITE_AFF_DEFER;
                    cnt = 1;
                    goto_end = true;
                } else {
                    sqlite3ExprListDelete(db, pFJMatch);
                    pFJMatch = ExprListId::NULL;
                }
            }
            if !goto_end {
                let zErr: &[u8] = if cnt == 0 {
                    b"no such column"
                } else {
                    b"ambiguous column name"
                };
                if zDb.is_some() {
                    errmsg!(
                        db,
                        "%s: %s.%s.%s",
                        zErr,
                        zDb.as_deref(),
                        zTab.as_deref(),
                        zColR
                    );
                } else if zTab.is_some() {
                    errmsg!(db, "%s: %s.%s", zErr, zTab.as_deref(), zColR);
                } else if cnt == 0 && db.ex(pRight).HasProperty(EP_DblQuoted) {
                    errmsg!(
                        db,
                        "%s: \"%s\" - should this be a string literal in single-quotes?",
                        zErr,
                        zColR
                    );
                } else {
                    errmsg!(db, "%s: %s", zErr, zColR);
                }
                sqlite3RecordErrorOffsetOfExpr(db, pExpr);
                db.pp().checkSchema = 1;
                ncm(db, pTopNC).nNcErr += 1;
                eNewExprOp = TK_NULL;
            }
        }
    }

    if !goto_end {
        debug_assert!(pFJMatch.is_null());

        /* Remove all substructure from pExpr */
        if !db.ex(pExpr).HasProperty(EP_TokenOnly | EP_Leaf) {
            let (l, r) = (db.ex(pExpr).pLeft, db.ex(pExpr).pRight);
            sqlite3ExprDelete(db, l);
            db.exm(pExpr).pLeft = ExprId::NULL;
            sqlite3ExprDelete(db, r);
            db.exm(pExpr).pRight = ExprId::NULL;
            db.exm(pExpr).SetProperty(EP_Leaf);
        }

        /* If a column from a table in pSrcList is referenced, then record
         ** this fact in the pSrcList.a[].colUsed bitmask.  Column 0 causes
         ** bit 0 to be set.  Column 1 sets bit 1.  And so forth.  Bit 63 is
         ** set if the 63rd or any subsequent column is used.
         **
         ** The colUsed mask is an optimization used to help determine if an
         ** index is a covering index.  The correct answer is still obtained
         ** if the mask contains extra set bits.  However, it is important to
         ** avoid setting bits beyond the maximum column number of the table.
         ** (See ticket [b92e5e8ec2cdbaa1]).
         **
         ** If a generated column is referenced, set bits for every column
         ** of the table.
         */
        if let Some(m) = pMatch {
            if db.ex(pExpr).iColumn >= 0 {
                let u = sqlite3ExprColUsed(db, pExpr);
                db.itemm(m).colUsed |= u;
            } else {
                db.itemm(m).fg.rowidUsed = 1;
            }
        }

        db.exm(pExpr).op = eNewExprOp;
    }

    /* lookupname_end: */
    if cnt == 1 {
        debug_assert!(pNC.nn());
        if db.xAuth.is_some() {
            let pSrcList = nc(db, pNC).pSrcList;
            if pFJMatch.nn() {
                debug_assert!(db.ex(pExpr).op == TK_FUNCTION);
                debug_assert!(db.ex(pExpr).x.pList == pFJMatch);
                let n = db.el(pFJMatch).nExpr();
                for i in 0..n {
                    let e = db.el(pFJMatch).a[i as usize].pExpr;
                    debug_assert!(db.ex(e).op == TK_COLUMN);
                    sqlite3AuthRead(db, e, pSchema, pSrcList);
                }
            } else {
                let op = db.ex(pExpr).op;
                if op == TK_COLUMN || op == TK_TRIGGER {
                    sqlite3AuthRead(db, pExpr, pSchema, pSrcList);
                }
            }
        }
        /* Increment the nRef value on all name contexts from TopNC up to
         ** the point where the name matched. */
        loop {
            debug_assert!(pTopNC.nn());
            ncm(db, pTopNC).nRef += 1;
            if pTopNC == pNC {
                break;
            }
            pTopNC = nc(db, pTopNC).pNext;
        }
        WRC_Prune
    } else {
        WRC_Abort
    }
}

/// The "cnt>0" test of lookupName(): decide what to do with a second
/// match of the column in source item pItem.  `nested` selects the
/// nested-FROM variant (where a single table with two columns matching a
/// USING term, pMatch==pItem, is also ambiguous).
fn dupMatch(
    db: &mut Sqlite3,
    pItem: SrcItemId,
    zCol: &[u8],
    pMatch: Option<SrcItemId>,
    nested: bool,
) -> Dup {
    let (isUsing, pUsing, jointype) = {
        let it = db.item(pItem);
        (it.fg.isUsing, it.u3.pUsing, it.fg.jointype)
    };
    if isUsing == 0
        || sqlite3IdListIndex(db, pUsing, Some(zCol)) < 0
        || (nested && pMatch == Some(pItem))
    {
        Dup::Ambiguous
    } else if (jointype & JT_RIGHT) == 0 {
        Dup::Skip
    } else if (jointype & JT_LEFT) == 0 {
        Dup::Restart
    } else {
        Dup::FullJoin
    }
}

/*
** Allocate and return a pointer to an expression to load the column iCol
** from datasource iSrc in SrcList pSrc.
*/
/// C: `Expr * sqlite3CreateColumnExpr(sqlite3 *db, SrcList *pSrc, int iSrc, int iCol)`
pub fn sqlite3CreateColumnExpr(db: &mut Sqlite3, pSrc: SrcListId, iSrc: i32, iCol: i32) -> ExprId {
    let p = sqlite3ExprAlloc(db, TK_COLUMN as i32, &Token::default(), 0);
    if p.nn() {
        let pItem = SrcItemId(pSrc, iSrc as u32);
        let (pTab, iCursor) = {
            let it = db.item(pItem);
            (it.pSTab.clone().expect("SrcItem without table"), it.iCursor)
        };
        debug_assert!(db.ex(p).UseYTab());
        {
            let e = db.exm(p);
            e.y.pTab = Some(pTab.clone());
            e.iTable = iCursor;
        }
        let (iPKey, tabFlags, nCol, generated) = {
            let t = pTab.borrow();
            let generated = iCol >= 0
                && (iCol as usize) < t.aCol.len()
                && (t.aCol[iCol as usize].colFlags & COLFLAG_GENERATED) != 0;
            (t.iPKey as i32, t.tabFlags, t.nCol as i32, generated)
        };
        if iPKey == iCol {
            db.exm(p).iColumn = -1;
        } else {
            db.exm(p).iColumn = iCol as ynVar;
            if (tabFlags & TF_HasGenerated) != 0 && generated {
                db.itemm(pItem).colUsed = if nCol >= 64 {
                    ALLBITS
                } else {
                    MASKBIT(nCol) - 1
                };
            } else {
                db.itemm(pItem).colUsed |=
                    (1 as Bitmask) << (if iCol >= BMS { BMS - 1 } else { iCol });
            }
        }
    }
    p
}

/*
** Report an error that an expression is not valid for some set of
** pNC->ncFlags values determined by validMask.
**
** static void notValid(
**   Parse *pParse,       // Leave error message here
**   NameContext *pNC,    // The name context
**   const char *zMsg,    // Type of error
**   int validMask,       // Set of contexts for which prohibited
**   Expr *pExpr          // Invalidate this expression on error
** ){...}
**
** As an optimization, since the conditional is almost always false
** (because errors are rare), the conditional is moved outside of the
** function call using a macro.
*/
fn notValidImpl(
    db: &mut Sqlite3,
    pNC: NcId,      /* The name context */
    zMsg: &[u8],    /* Type of error */
    pExpr: ExprId,  /* Invalidate this expression on error */
    pError: ExprId, /* Associate error with this expression */
) {
    let ncFlags = nc(db, pNC).ncFlags;
    let zIn: &[u8] = if (ncFlags & NC_IdxExpr) != 0 {
        b"index expressions"
    } else if (ncFlags & NC_IsCheck) != 0 {
        b"CHECK constraints"
    } else if (ncFlags & NC_GenCol) != 0 {
        b"generated columns"
    } else {
        b"partial index WHERE clauses"
    };
    errmsg!(db, "%s prohibited in %s", zMsg, zIn);
    if pExpr.nn() {
        db.exm(pExpr).op = TK_NULL;
    }
    sqlite3RecordErrorOffsetOfExpr(db, pError);
}
/// The sqlite3ResolveNotValid() macro.
#[inline]
fn sqlite3ResolveNotValid(
    db: &mut Sqlite3,
    pNC: NcId,
    zMsg: &[u8],
    X: i32,
    pExpr: ExprId,
    pError: ExprId,
) {
    debug_assert!((X & !(NC_IsCheck | NC_PartIdx | NC_IdxExpr | NC_GenCol)) == 0);
    if (nc(db, pNC).ncFlags & X) != 0 {
        notValidImpl(db, pNC, zMsg, pExpr, pError);
    }
}

/*
** Expression p should encode a floating point value between 1.0 and 0.0.
** Return 134,217,728 (2^27) times this value.  Or return -1 if p is not
** a floating point value between 1.0 and 0.0.
*/
fn exprProbability(db: &mut Sqlite3, p: ExprId) -> i32 {
    let mut r: f64 = -1.0;
    if db.ex(p).op != TK_FLOAT {
        return -1;
    }
    debug_assert!(!db.ex(p).HasProperty(EP_IntValue));
    let z = db.ex(p).u.zToken.clone();
    sqlite3AtoF(z.as_deref(), &mut r);
    debug_assert!(r >= 0.0);
    if r > 1.0 {
        return -1;
    }
    (r * 134217728.0) as i32
}

/*
** Set the EP_SubtArg property on every expression inside of
** pList.  If any subexpression is actually a subquery, then
** also set the EP_SubtArg property on the first result-set
** column of that subquery.
*/
fn resolveSetExprSubtypeArg(db: &mut Sqlite3, pList: ExprListId) {
    let nn = if pList.nn() { db.el(pList).nExpr() } else { 0 };
    for ii in 0..nn {
        let mut pExpr = db.el(pList).a[ii as usize].pExpr;
        loop {
            /*exit-by-break*/
            db.exm(pExpr).SetProperty(EP_SubtArg);
            let op = db.ex(pExpr).op;
            if op == TK_SELECT {
                debug_assert!(db.ex(pExpr).UseXSelect());
                let pSel = db.ex(pExpr).x.pSelect;
                debug_assert!(pSel.nn());
                let pEList = db.sel(pSel).pEList;
                resolveSetExprSubtypeArg(db, pEList);
                break;
            }
            if op == TK_UPLUS {
                pExpr = db.ex(pExpr).pLeft;
                debug_assert!(pExpr.nn());
            } else {
                break;
            }
        }
    }
}

/// IsWindowFunc(p)
fn IsWindowFunc(db: &Sqlite3, p: ExprId) -> bool {
    let e = db.ex(p);
    e.HasProperty(EP_WinFunc) && e.y.pWin.nn() && db.win(e.y.pWin).eFrmType != TK_FILTER
}

/*
** This routine is callback for sqlite3WalkExpr().
**
** Resolve symbolic names into TK_COLUMN operators for the current
** node in the expression tree.  Return 0 to continue the search down
** the tree or 2 to abort the tree walk.
**
** This routine also does error checking and name resolution for
** function names.  The operator for aggregate functions is changed
** to TK_AGG_FUNCTION.
*/
fn resolveExprStep(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    let pNC = w.u.pNC;
    debug_assert!(pNC.nn());

    let op = w.db.ex(pExpr).op;
    match op {
        /* The special operator TK_ROW means use the rowid for the first
         ** column in the FROM clause.  This is used by the LIMIT and ORDER BY
         ** clause processing on UPDATE and DELETE statements, and by
         ** UPDATE ... FROM statement processing.
         */
        TK_ROW => {
            let db = &mut *w.db;
            let pSrcList = nc(db, pNC).pSrcList;
            debug_assert!(pSrcList.nn() && db.src(pSrcList).nSrc() >= 1);
            let (pTab, iCursor) = {
                let it = &db.src(pSrcList).a[0];
                (it.pSTab.clone(), it.iCursor)
            };
            let e = db.exm(pExpr);
            e.op = TK_COLUMN;
            debug_assert!(e.UseYTab());
            e.y.pTab = pTab;
            e.iTable = iCursor;
            e.iColumn -= 1;
            e.affExpr = SQLITE_AFF_INTEGER;
        }

        /* An optimization:  Attempt to convert
         **
         **      "expr IS NOT NULL"  -->  "TRUE"
         **      "expr IS NULL"      -->  "FALSE"
         **
         ** if we can prove that "expr" is never NULL.  Call this the
         ** "NOT NULL strength reduction optimization".
         **
         ** If this optimization occurs, also restore the NameContext ref-counts
         ** to the state they where in before the "column" LHS expression was
         ** resolved.  This prevents "column" from being counted as having been
         ** referenced, which might prevent a SELECT from being erroneously
         ** marked as correlated.
         **
         ** 2024-03-28: Beware of aggregates.  A bare column of aggregated table
         ** can still evaluate to NULL even though it is marked as NOT NULL.
         ** Example:
         **
         **       CREATE TABLE t1(a INT NOT NULL);
         **       SELECT a, a IS NULL, a IS NOT NULL, count(*) FROM t1;
         **
         ** The "a IS NULL" and "a IS NOT NULL" expressions cannot be optimized
         ** here because at the time this case is hit, we do not yet know whether
         ** or not t1 is being aggregated.  We have to assume the worst and omit
         ** the optimization.  The only time it is safe to apply this optimization
         ** is within the WHERE clause.
         */
        TK_NOTNULL | TK_ISNULL => {
            let mut anRef = [0i32; 8];
            let mut p = pNC;
            let mut i = 0;
            while p.nn() && i < anRef.len() {
                anRef[i] = nc(w.db, p).nRef;
                p = nc(w.db, p).pNext;
                i += 1;
            }
            let pLeft = w.db.ex(pExpr).pLeft;
            sqlite3WalkExpr(w, pLeft);
            let db = &mut *w.db;
            if db.ppr().IN_RENAME_OBJECT() {
                return WRC_Prune;
            }
            let pLeft = db.ex(pExpr).pLeft;
            if sqlite3ExprCanBeNull(db, pLeft) != 0 {
                /* The expression can be NULL.  So the optimization does not apply */
                return WRC_Prune;
            }

            let mut p = pNC;
            while p.nn() {
                if (nc(db, p).ncFlags & NC_Where) == 0 {
                    return WRC_Prune; /* Not in a WHERE clause.  Unsafe to optimize. */
                }
                p = nc(db, p).pNext;
            }
            debug_assert!(!db.ex(pExpr).HasProperty(EP_IntValue));
            {
                let e = db.exm(pExpr);
                e.u.iValue = (e.op == TK_NOTNULL) as i32;
                e.flags |= EP_IntValue;
                e.op = TK_INTEGER;
            }
            let mut p = pNC;
            let mut i = 0;
            while p.nn() && i < anRef.len() {
                ncm(db, p).nRef = anRef[i];
                p = nc(db, p).pNext;
                i += 1;
            }
            let pLeft = db.ex(pExpr).pLeft;
            sqlite3ExprDelete(db, pLeft);
            db.exm(pExpr).pLeft = ExprId::NULL;
            return WRC_Prune;
        }

        /* A column name:                    ID
         ** Or table name and column name:    ID.ID
         ** Or a database, table and column:  ID.ID.ID
         **
         ** The TK_ID and TK_OUT cases are combined so that there will only
         ** be one call to lookupName().  Then the compiler will in-line
         ** lookupName() for a size reduction and performance increase.
         */
        TK_ID | TK_DOT => {
            let db = &mut *w.db;
            let zTable: Option<ZStr>;
            let zDb: Option<ZStr>;
            let mut pRight: ExprId;

            if op == TK_ID {
                zDb = None;
                zTable = None;
                debug_assert!(!db.ex(pExpr).HasProperty(EP_IntValue));
                pRight = pExpr;
            } else {
                let mut pLeft = db.ex(pExpr).pLeft;
                sqlite3ResolveNotValid(
                    db,
                    pNC,
                    b"the \".\" operator",
                    NC_IdxExpr | NC_GenCol,
                    ExprId::NULL,
                    pExpr,
                );
                pRight = db.ex(pExpr).pRight;
                if db.ex(pRight).op == TK_ID {
                    zDb = None;
                } else {
                    debug_assert!(db.ex(pRight).op == TK_DOT);
                    debug_assert!(!db.ex(pRight).HasProperty(EP_IntValue));
                    zDb = db.ex(pLeft).u.zToken.clone();
                    pLeft = db.ex(pRight).pLeft;
                    pRight = db.ex(pRight).pRight;
                }
                debug_assert!(db.ex(pLeft).UseUToken() && db.ex(pRight).UseUToken());
                zTable = db.ex(pLeft).u.zToken.clone();
                debug_assert!(db.ex(pExpr).UseYTab());
                if db.ppr().IN_RENAME_OBJECT() {
                    sqlite3RenameTokenRemap(
                        db,
                        Some(RenameKey::Expr(pExpr)),
                        RenameKey::Expr(pRight),
                    );
                    sqlite3RenameTokenRemap(
                        db,
                        Some(RenameKey::ExprTab(pExpr)),
                        RenameKey::Expr(pLeft),
                    );
                }
            }
            return lookupName(db, zDb, zTable, pRight, pNC, pExpr);
        }

        /* Resolve function names
         */
        TK_FUNCTION => {
            return resolveFunction(w, pExpr, pNC);
        }

        TK_EXISTS | TK_SELECT | TK_IN => {
            if w.db.ex(pExpr).UseXSelect() {
                let nRef = nc(w.db, pNC).nRef;
                debug_assert!(w.db.ex(pExpr).x.pSelect.nn());
                if op == TK_EXISTS {
                    w.db.pp().bHasExists = 1;
                }
                if (nc(w.db, pNC).ncFlags & NC_SelfRef) != 0 {
                    notValidImpl(w.db, pNC, b"subqueries", pExpr, pExpr);
                } else {
                    let pSel = w.db.ex(pExpr).x.pSelect;
                    sqlite3WalkSelect(w, pSel);
                }
                let db = &mut *w.db;
                debug_assert!(nc(db, pNC).nRef >= nRef);
                if nRef != nc(db, pNC).nRef {
                    db.exm(pExpr).SetProperty(EP_VarSelect);
                    let pSel = db.ex(pExpr).x.pSelect;
                    db.selm(pSel).selFlags |= SF_Correlated;
                }
                ncm(db, pNC).ncFlags |= NC_Subquery;
            }
        }
        TK_VARIABLE => {
            sqlite3ResolveNotValid(
                w.db,
                pNC,
                b"parameters",
                NC_IsCheck | NC_PartIdx | NC_IdxExpr | NC_GenCol,
                pExpr,
                pExpr,
            );
        }
        TK_IS | TK_ISNOT | TK_BETWEEN | TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
            if op == TK_IS || op == TK_ISNOT {
                let r = w.db.ex(pExpr).pRight;
                let pRight = sqlite3ExprSkipCollateAndLikely(w.db, r);
                debug_assert!(!w.db.ex(pExpr).HasProperty(EP_Reduced));
                /* Handle special cases of "x IS TRUE", "x IS FALSE", "x IS NOT TRUE",
                 ** and "x IS NOT FALSE". */
                if pRight.nn()
                    && (w.db.ex(pRight).op == TK_ID || w.db.ex(pRight).op == TK_TRUEFALSE)
                {
                    let rc = resolveExprStep(w, pRight);
                    if rc == WRC_Abort {
                        return WRC_Abort;
                    }
                    if w.db.ex(pRight).op == TK_TRUEFALSE {
                        let e = w.db.exm(pExpr);
                        e.op2 = e.op;
                        e.op = TK_TRUTH;
                        return WRC_Continue;
                    }
                }
                /* no break */
            }
            let db = &mut *w.db;
            if db.mallocFailed == 0 {
                let op = db.ex(pExpr).op;
                let pLeft = db.ex(pExpr).pLeft;
                debug_assert!(pLeft.nn());
                let nLeft = sqlite3ExprVectorSize(db, pLeft);
                let mut nRight;
                if op == TK_BETWEEN {
                    debug_assert!(db.ex(pExpr).UseXList());
                    let pList = db.ex(pExpr).x.pList;
                    let e0 = db.el(pList).a[0].pExpr;
                    nRight = sqlite3ExprVectorSize(db, e0);
                    if nRight == nLeft {
                        let e1 = db.el(pList).a[1].pExpr;
                        nRight = sqlite3ExprVectorSize(db, e1);
                    }
                } else {
                    let r = db.ex(pExpr).pRight;
                    debug_assert!(r.nn());
                    nRight = sqlite3ExprVectorSize(db, r);
                }
                if nLeft != nRight {
                    errmsg!(db, "row value misused");
                    sqlite3RecordErrorOffsetOfExpr(db, pExpr);
                }
            }
        }
        _ => {}
    }
    let db = &mut *w.db;
    debug_assert!(db.mallocFailed == 0 || db.ppr().nErr != 0);
    if db.ppr().nErr != 0 {
        WRC_Abort
    } else {
        WRC_Continue
    }
}

/// The TK_FUNCTION case of resolveExprStep().
fn resolveFunction(w: &mut Walker<'_>, pExpr: ExprId, pNC: NcId) -> i32 {
    let mut no_such_func = 0; /* True if no such function exists */
    let mut wrong_num_args = false; /* True if wrong number of arguments */
    let mut is_agg = false; /* True if is an aggregate function */
    let enc = w.db.ENC(); /* The database encoding */
    let savedAllowFlags = nc(w.db, pNC).ncFlags & (NC_AllowAgg | NC_AllowWin);
    let pWin = if IsWindowFunc(w.db, pExpr) {
        w.db.ex(pExpr).y.pWin
    } else {
        WindowId::NULL
    };
    {
        let db = &mut *w.db;
        debug_assert!(!db.ex(pExpr).HasProperty(EP_xIsSelect | EP_IntValue));
        debug_assert!({
            let l = db.ex(pExpr).pLeft;
            l.is_null() || db.ex(l).op == TK_ORDER
        });
    }
    let pList = w.db.ex(pExpr).x.pList;
    let n = if pList.nn() {
        w.db.el(pList).nExpr()
    } else {
        0
    };
    let zId = w.db.ex(pExpr).u.zToken.clone();
    let mut pDef = sqlite3FindFunction(w.db, zId.as_deref(), n, enc, 0);
    {
        let db = &mut *w.db;
        if pDef.is_none() {
            pDef = sqlite3FindFunction(db, zId.as_deref(), -2, enc, 0);
            if pDef.is_none() {
                no_such_func = 1;
            } else {
                wrong_num_args = true;
            }
        } else {
            let def = pDef.clone().unwrap();
            is_agg = def.xFinalize.is_some();
            if (def.funcFlags & SQLITE_FUNC_UNLIKELY) != 0 {
                db.exm(pExpr).SetProperty(EP_Unlikely);
                if n == 2 {
                    let a1 = db.el(pList).a[1].pExpr;
                    let pr = exprProbability(db, a1);
                    db.exm(pExpr).iTable = pr;
                    if db.ex(pExpr).iTable < 0 {
                        errmsg!(
                            db,
                            "second argument to %#T() must be a constant between 0.0 and 1.0",
                            pExpr
                        );
                        ncm(db, pNC).nNcErr += 1;
                    }
                } else {
                    /* EVIDENCE-OF: R-61304-29449 The unlikely(X) function is
                     ** equivalent to likelihood(X, 0.0625).
                     ** EVIDENCE-OF: R-01283-11636 The unlikely(X) function is
                     ** short-hand for likelihood(X,0.0625).
                     ** EVIDENCE-OF: R-36850-34127 The likely(X) function is short-hand
                     ** for likelihood(X,0.9375).
                     ** EVIDENCE-OF: R-53436-40973 The likely(X) function is equivalent
                     ** to likelihood(X,0.9375). */
                    /* TUNING: unlikely() probability is 0.0625.  likely() is 0.9375 */
                    db.exm(pExpr).iTable = if def.zName.first() == Some(&b'u') {
                        8388608
                    } else {
                        125829120
                    };
                }
            }
            {
                let auth = sqlite3AuthCheck(db, SQLITE_FUNCTION, None, Some(&def.zName), None);
                if auth != SQLITE_OK {
                    if auth == SQLITE_DENY {
                        errmsg!(db, "not authorized to use function: %#T", pExpr);
                        ncm(db, pNC).nNcErr += 1;
                    }
                    db.exm(pExpr).op = TK_NULL;
                    return WRC_Prune;
                }
            }

            /* If the function may call sqlite3_value_subtype(), then set the
             ** EP_SubtArg flag on all of its argument expressions. This prevents
             ** where.c from replacing the expression with a value read from an
             ** index on the same expression, which will not have the correct
             ** subtype. Also set the flag if the function expression itself is
             ** an EP_SubtArg expression. In this case subtypes are required as
             ** the function may return a value with a subtype back to its
             ** caller using sqlite3_result_value().  */
            if (def.funcFlags & SQLITE_SUBTYPE) != 0 || db.ex(pExpr).HasProperty(EP_SubtArg) {
                resolveSetExprSubtypeArg(db, pList);
            }

            if (def.funcFlags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) != 0 {
                /* For the purposes of the EP_ConstFunc flag, date and time
                 ** functions and other functions that change slowly are considered
                 ** constant because they are constant for the duration of one query.
                 ** This allows them to be factored out of inner loops. */
                db.exm(pExpr).SetProperty(EP_ConstFunc);
            }
            if (def.funcFlags & SQLITE_FUNC_CONSTANT) == 0 {
                /* Clearly non-deterministic functions like random(), but also
                 ** date/time functions that use 'now', and other functions like
                 ** sqlite_version() that might change over time cannot be used
                 ** in an index or generated column.  Curiously, they can be used
                 ** in a CHECK constraint.  SQLServer, MySQL, and PostgreSQL all
                 ** allow this. */
                sqlite3ResolveNotValid(
                    db,
                    pNC,
                    b"non-deterministic functions",
                    NC_IdxExpr | NC_PartIdx | NC_GenCol,
                    ExprId::NULL,
                    pExpr,
                );
            } else {
                debug_assert!((NC_SelfRef & 0xff) == NC_SelfRef); /* Must fit in 8 bits */
                let f = (nc(db, pNC).ncFlags & NC_SelfRef) as u8;
                db.exm(pExpr).op2 = f;
            }
            if (def.funcFlags & SQLITE_FUNC_INTERNAL) != 0
                && db.ppr().nested == 0
                && (db.mDbFlags & DBFLAG_InternalFunc) == 0
            {
                /* Internal-use-only functions are disallowed unless the
                 ** SQL is being compiled using sqlite3NestedParse() or
                 ** the SQLITE_TESTCTRL_INTERNAL_FUNCTIONS test-control has be
                 ** used to activate internal functions for testing purposes.
                 **
                 ** The 2 value for no_such_func means that the function is
                 ** an internal-use-only function which should be treated as a
                 ** non-existant function for name resolution purposes.
                 */
                no_such_func = 2;
                pDef = None;
            } else if (def.funcFlags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0
                && !db.ppr().IN_RENAME_OBJECT()
            {
                if (nc(db, pNC).ncFlags & NC_FromDDL) != 0 {
                    db.exm(pExpr).SetProperty(EP_FromDDL);
                }
                sqlite3ExprFunctionUsable(db, pExpr, &def);
            }
        }

        if !db.ppr().IN_RENAME_OBJECT() {
            let ncFlags = nc(db, pNC).ncFlags;
            let defFlags = pDef.as_ref().map(|d| d.funcFlags).unwrap_or(0);
            if pDef.as_ref().map(|d| d.xValue.is_none()).unwrap_or(false) && pWin.nn() {
                errmsg!(db, "%#T() may not be used as a window function", pExpr);
                ncm(db, pNC).nNcErr += 1;
            } else if (is_agg && (ncFlags & NC_AllowAgg) == 0)
                || (is_agg && (defFlags & SQLITE_FUNC_WINDOW) != 0 && pWin.is_null())
                || (is_agg && pWin.nn() && (ncFlags & NC_AllowWin) == 0)
            {
                let zType: &[u8] = if (defFlags & SQLITE_FUNC_WINDOW) != 0 || pWin.nn() {
                    b"window"
                } else {
                    b"aggregate"
                };
                errmsg!(db, "misuse of %s function %#T()", zType, pExpr);
                ncm(db, pNC).nNcErr += 1;
                is_agg = false;
            } else if no_such_func != 0
                && (db.init.busy == 0 || (no_such_func == 2 && db.init.busy == 2))
            /* Suppress "no such function" errors when reading
             ** the sqlite_schema table.  Except, do raise the error
             ** if init.busy is 2, meaning the schema parse is due
             ** to an ALTER TABLE ADD COLUMN statement, and the function
             ** is an internal-use-only function (no_such_func==2). */
            {
                errmsg!(db, "no such function: %#T", pExpr);
                ncm(db, pNC).nNcErr += 1;
            } else if wrong_num_args {
                errmsg!(db, "wrong number of arguments to function %#T()", pExpr);
                ncm(db, pNC).nNcErr += 1;
            } else if !is_agg && db.ex(pExpr).HasProperty(EP_WinFunc) {
                errmsg!(db, "FILTER may not be used with non-aggregate %#T()", pExpr);
                ncm(db, pNC).nNcErr += 1;
            } else if !is_agg && db.ex(pExpr).pLeft.nn() {
                sqlite3ExprOrderByAggregateError(db, pExpr);
                ncm(db, pNC).nNcErr += 1;
            }
            if is_agg {
                /* Window functions may not be arguments of aggregate functions.
                 ** Or arguments of other window functions. But aggregate functions
                 ** may be arguments for window functions.  */
                ncm(db, pNC).ncFlags &=
                    !(NC_AllowWin | if pWin.is_null() { NC_AllowAgg } else { 0 });
            }
        } else if db.ex(pExpr).HasProperty(EP_WinFunc) || db.ex(pExpr).pLeft.nn() {
            is_agg = true;
        }
    }
    sqlite3WalkExprList(w, pList);
    if is_agg {
        let pLeft = w.db.ex(pExpr).pLeft;
        if pLeft.nn() {
            debug_assert!(w.db.ex(pLeft).op == TK_ORDER);
            debug_assert!(w.db.ex(pLeft).UseXList());
            let l = w.db.ex(pLeft).x.pList;
            sqlite3WalkExprList(w, l);
        }
        if pWin.nn() && w.db.ppr().nErr == 0 {
            let pSel = nc(w.db, pNC).pWinSelect;
            debug_assert!(w.db.ex(pExpr).UseYWin() && pWin == w.db.ex(pExpr).y.pWin);
            if !w.db.ppr().IN_RENAME_OBJECT() {
                let pDefn = if pSel.nn() {
                    w.db.sel(pSel).pWinDefn
                } else {
                    WindowId::NULL
                };
                if let Some(def) = pDef.clone() {
                    sqlite3WindowUpdate(w.db, pDefn, pWin, &def);
                }
                if w.db.mallocFailed != 0 {
                    return if w.db.ppr().nErr != 0 {
                        WRC_Abort
                    } else {
                        WRC_Continue
                    };
                }
            }
            let (pPartition, pOrderBy, pFilter) = {
                let x = w.db.win(pWin);
                (x.pPartition, x.pOrderBy, x.pFilter)
            };
            sqlite3WalkExprList(w, pPartition);
            sqlite3WalkExprList(w, pOrderBy);
            sqlite3WalkExpr(w, pFilter);
            sqlite3WindowLink(w.db, pSel, pWin);
            ncm(w.db, pNC).ncFlags |= NC_HasWin;
        } else {
            {
                let e = w.db.exm(pExpr);
                e.op = TK_AGG_FUNCTION;
                e.op2 = 0;
            }
            if w.db.ex(pExpr).HasProperty(EP_WinFunc) {
                let pw = w.db.ex(pExpr).y.pWin;
                let pFilter = w.db.win(pw).pFilter;
                sqlite3WalkExpr(w, pFilter);
            }
            let db = &mut *w.db;
            let mut pNC2 = pNC;
            while pNC2.nn() {
                let pSrc2 = nc(db, pNC2).pSrcList;
                if sqlite3ReferencesSrcList(db, pExpr, pSrc2) != 0 {
                    break;
                }
                let nNested = nc(db, pNC2).nNestedSelect;
                let e = db.exm(pExpr);
                e.op2 = e.op2.wrapping_add((1 + nNested) as u8);
                pNC2 = nc(db, pNC2).pNext;
            }
            debug_assert!(pDef.is_some() || db.ppr().IN_RENAME_OBJECT());
            if pNC2.nn() {
                if let Some(def) = &pDef {
                    let nNested = nc(db, pNC2).nNestedSelect;
                    let e = db.exm(pExpr);
                    e.op2 = e.op2.wrapping_add(nNested as u8);
                    debug_assert!(SQLITE_FUNC_MINMAX as i32 == NC_MinMaxAgg);
                    debug_assert!(SQLITE_FUNC_ANYORDER as i32 == NC_OrderAgg);
                    ncm(db, pNC2).ncFlags |= NC_HasAgg
                        | ((def.funcFlags ^ SQLITE_FUNC_ANYORDER)
                            & (SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER))
                            as i32;
                }
            }
        }
        ncm(w.db, pNC).ncFlags |= savedAllowFlags;
    }
    /* FIX ME:  Compute pExpr->affinity based on the expected return
     ** type of the function
     */
    WRC_Prune
}

/*
** pEList is a list of expressions which are really the result set of the
** a SELECT statement.  pE is a term in an ORDER BY or GROUP BY clause.
** This routine checks to see if pE is a simple identifier which corresponds
** to the AS-name of one of the terms of the expression list.  If it is,
** this routine return an integer between 1 and N where N is the number of
** elements in pEList, corresponding to the matching entry.  If there is
** no match, or if pE is not a simple identifier, then this routine
** return 0.
**
** pEList has been resolved.  pE has not.
*/
fn resolveAsName(
    db: &mut Sqlite3,
    pEList: ExprListId, /* List of expressions to scan */
    pE: ExprId,         /* Expression we are trying to match */
) -> i32 {
    if db.ex(pE).op == TK_ID {
        debug_assert!(!db.ex(pE).HasProperty(EP_IntValue));
        let zCol = db.ex(pE).u.zToken.clone();
        let l = db.el(pEList);
        for (i, it) in l.a.iter().enumerate() {
            if it.fg.eEName == ENAME_NAME
                && sqlite3_stricmp(it.zEName.as_deref(), zCol.as_deref()) == 0
            {
                return i as i32 + 1;
            }
        }
    }
    0
}

/*
** pE is a pointer to an expression which is a single term in the
** ORDER BY of a compound SELECT.  The expression has not been
** name resolved.
**
** At the point this routine is called, we already know that the
** ORDER BY term is not an integer index into the result set.  That
** case is handled by the calling routine.
**
** Attempt to match pE against result set columns in the left-most
** SELECT statement.  Return the index i of the matching column,
** as an indication to the caller that it should sort by the i-th column.
** The left-most column is 1.  In other words, the value returned is the
** same integer value that would be used in the SQL statement to indicate
** the column.
**
** If there is no match, return 0.  Return -1 if an error occurs.
*/
fn resolveOrderByTermToExprList(
    db: &mut Sqlite3,
    pSelect: SelectId, /* The SELECT statement with the ORDER BY clause */
    pE: ExprId,        /* The specific ORDER BY term */
) -> i32 {
    let pEList = db.sel(pSelect).pEList;

    /* Resolve all names in the ORDER BY term expression
     */
    let mut sNC = NameContext::default();
    sNC.pSrcList = db.sel(pSelect).pSrc;
    sNC.uNC.pEList = pEList;
    sNC.ncFlags = NC_AllowAgg | NC_UEList | NC_NoSelect;
    sNC.nNcErr = 0;
    let nc = db.arena.new_nc(sNC);
    let savedSuppErr = db.suppressErr;
    db.suppressErr = 1;
    let rc = sqlite3ResolveExprNames(db, nc, pE);
    db.suppressErr = savedSuppErr;
    if rc != 0 {
        return 0;
    }

    /* Try to match the ORDER BY expression against an expression
     ** in the result set.  Return an 1-based index of the matching
     ** result-set entry.
     */
    let n = db.el(pEList).nExpr();
    for i in 0..n {
        let e = db.el(pEList).a[i as usize].pExpr;
        if sqlite3ExprCompare(db, e, pE, -1) < 2 {
            return i + 1;
        }
    }

    /* If no match, return 0. */
    0
}

/*
** Generate an ORDER BY or GROUP BY term out-of-range error.
*/
fn resolveOutOfRangeError(
    db: &mut Sqlite3,
    zType: &[u8],   /* "ORDER" or "GROUP" */
    i: i32,         /* The index (1-based) of the term out of range */
    mx: i32,        /* Largest permissible value of i */
    pError: ExprId, /* Associate the error with the expression */
) {
    errmsg!(
        db,
        "%r %s BY term out of range - should be between 1 and %d",
        i,
        zType,
        mx
    );
    sqlite3RecordErrorOffsetOfExpr(db, pError);
}

/*
** Analyze the ORDER BY clause in a compound SELECT statement.   Modify
** each term of the ORDER BY clause is a constant integer between 1
** and N where N is the number of columns in the compound SELECT.
**
** ORDER BY terms that are already an integer between 1 and N are
** unmodified.  ORDER BY terms that are integers outside the range of
** 1 through N generate an error.  ORDER BY terms that are expressions
** are matched against result set expressions of compound SELECT
** beginning with the left-most SELECT and working toward the right.
** At the first match, the ORDER BY expression is transformed into
** the integer column number.
**
** Return the number of errors seen.
*/
fn resolveCompoundOrderBy(
    db: &mut Sqlite3,
    pSelect: SelectId, /* The SELECT statement containing the ORDER BY */
) -> i32 {
    let mut pSelect = pSelect;
    let mut moreToDo = true;

    let pOrderBy = db.sel(pSelect).pOrderBy;
    if pOrderBy.is_null() {
        return 0;
    }
    if db.el(pOrderBy).nExpr() > db.aLimit[SQLITE_LIMIT_COLUMN as usize] {
        errmsg!(db, "too many terms in ORDER BY clause");
        return 1;
    }
    for it in db.elm(pOrderBy).a.iter_mut() {
        it.fg.done = 0;
    }
    db.selm(pSelect).pNext = SelectId::NULL;
    while db.sel(pSelect).pPrior.nn() {
        let pPrior = db.sel(pSelect).pPrior;
        db.selm(pPrior).pNext = pSelect;
        pSelect = pPrior;
    }
    let nOrderBy = db.el(pOrderBy).nExpr();
    while pSelect.nn() && moreToDo {
        moreToDo = false;
        let pEList = db.sel(pSelect).pEList;
        debug_assert!(pEList.nn());
        let nEList = db.el(pEList).nExpr();
        for i in 0..nOrderBy {
            let mut iCol: i32 = -1;
            if db.el(pOrderBy).a[i as usize].fg.done != 0 {
                continue;
            }
            let pItemExpr = db.el(pOrderBy).a[i as usize].pExpr;
            let pE = sqlite3ExprSkipCollateAndLikely(db, pItemExpr);
            if pE.is_null() {
                continue;
            }
            if sqlite3ExprIsInteger(db, pE, &mut iCol, 0) != 0 {
                if iCol <= 0 || iCol > nEList {
                    resolveOutOfRangeError(db, b"ORDER", i + 1, nEList, pE);
                    return 1;
                }
            } else {
                iCol = resolveAsName(db, pEList, pE);
                if iCol == 0 {
                    /* Now test if expression pE matches one of the values returned
                     ** by pSelect. In the usual case this is done by duplicating the
                     ** expression, resolving any symbols in it, and then comparing
                     ** it against each expression returned by the SELECT statement.
                     ** Once the comparisons are finished, the duplicate expression
                     ** is deleted.
                     **
                     ** If this is running as part of an ALTER TABLE operation and
                     ** the symbols resolve successfully, also resolve the symbols in the
                     ** actual expression. This allows the code in alter.c to modify
                     ** column references within the ORDER BY expression as required.  */
                    let pDup = sqlite3ExprDup(db, pE, 0);
                    if db.mallocFailed == 0 {
                        debug_assert!(pDup.nn());
                        iCol = resolveOrderByTermToExprList(db, pSelect, pDup);
                        if db.ppr().IN_RENAME_OBJECT() && iCol > 0 {
                            resolveOrderByTermToExprList(db, pSelect, pE);
                        }
                    }
                    sqlite3ExprDelete(db, pDup);
                }
            }
            if iCol > 0 {
                /* Convert the ORDER BY term into an integer column number iCol,
                 ** taking care to preserve the COLLATE clause if it exists. */
                if !db.ppr().IN_RENAME_OBJECT() {
                    let pNew = sqlite3ExprInt32(db, iCol);
                    if pNew.is_null() {
                        return 1;
                    }
                    let cur = db.el(pOrderBy).a[i as usize].pExpr;
                    if cur == pE {
                        db.elm(pOrderBy).a[i as usize].pExpr = pNew;
                    } else {
                        let mut pParent = cur;
                        debug_assert!(db.ex(pParent).op == TK_COLLATE);
                        while db.ex(db.ex(pParent).pLeft).op == TK_COLLATE {
                            pParent = db.ex(pParent).pLeft;
                        }
                        debug_assert!(db.ex(pParent).pLeft == pE);
                        db.exm(pParent).pLeft = pNew;
                    }
                    sqlite3ExprDelete(db, pE);
                    db.elm(pOrderBy).a[i as usize].u.x.iOrderByCol = iCol as u16;
                }
                db.elm(pOrderBy).a[i as usize].fg.done = 1;
            } else {
                moreToDo = true;
            }
        }
        pSelect = db.sel(pSelect).pNext;
    }
    for i in 0..nOrderBy {
        if db.el(pOrderBy).a[i as usize].fg.done == 0 {
            errmsg!(
                db,
                "%r ORDER BY term does not match any column in the result set",
                i + 1
            );
            return 1;
        }
    }
    0
}

/*
** Check every term in the ORDER BY or GROUP BY clause pOrderBy of
** the SELECT statement pSelect.  If any term is reference to a
** result set expression (as determined by the ExprList.a.u.x.iOrderByCol
** field) then convert that term into a copy of the corresponding result set
** column.
**
** If any errors are detected, add an error message to pParse and
** return non-zero.  Return zero if no errors are seen.
*/
/// C: `int sqlite3ResolveOrderGroupBy( Parse *pParse, Select *pSelect, ExprList *pOrderBy, const char *zType )`
pub fn sqlite3ResolveOrderGroupBy(
    db: &mut Sqlite3,
    pSelect: SelectId,
    pOrderBy: ExprListId,
    zType: Option<&[u8]>,
) -> i32 {
    let zType = zType.unwrap_or(b"");
    if pOrderBy.is_null() || db.mallocFailed != 0 || db.ppr().IN_RENAME_OBJECT() {
        return 0;
    }
    if db.el(pOrderBy).nExpr() > db.aLimit[SQLITE_LIMIT_COLUMN as usize] {
        errmsg!(db, "too many terms in %s BY clause", zType);
        return 1;
    }
    let pEList = db.sel(pSelect).pEList;
    debug_assert!(pEList.nn()); /* sqlite3SelectNew() guarantees this */
    let n = db.el(pOrderBy).nExpr();
    for i in 0..n {
        let (iOrderByCol, pE) = {
            let it = &db.el(pOrderBy).a[i as usize];
            (it.u.x.iOrderByCol as i32, it.pExpr)
        };
        if iOrderByCol != 0 {
            let nE = db.el(pEList).nExpr();
            if iOrderByCol > nE {
                resolveOutOfRangeError(db, zType, i + 1, nE, ExprId::NULL);
                return 1;
            }
            resolveAlias(db, pEList, iOrderByCol - 1, pE, 0);
        }
    }
    0
}

/*
** Walker callback for windowRemoveExprFromSelect().
*/
fn resolveRemoveWindowsCb(w: &mut Walker<'_>, pExpr: ExprId) -> i32 {
    if w.db.ex(pExpr).HasProperty(EP_WinFunc) {
        let pWin = w.db.ex(pExpr).y.pWin;
        sqlite3WindowUnlinkFromSelect(w.db, pWin);
    }
    WRC_Continue
}

/*
** Remove any Window objects owned by the expression pExpr from the
** Select.pWin list of Select object pSelect.
*/
fn windowRemoveExprFromSelect(db: &mut Sqlite3, pSelect: SelectId, pExpr: ExprId) {
    if db.sel(pSelect).pWin.nn() {
        let mut w = Walker::new(db);
        w.xExprCallback = Some(resolveRemoveWindowsCb);
        w.u.pSelect = pSelect;
        sqlite3WalkExpr(&mut w, pExpr);
    }
}

/*
** pOrderBy is an ORDER BY or GROUP BY clause in SELECT statement pSelect.
** The Name context of the SELECT statement is pNC.  zType is either
** "ORDER" or "GROUP" depending on which type of clause pOrderBy is.
**
** This routine resolves each term of the clause into an expression.
** If the order-by term is an integer I between 1 and N (where N is the
** number of columns in the result set of the SELECT) then the expression
** in the resolution is a copy of the I-th result-set expression.  If
** the order-by term is an identifier that corresponds to the AS-name of
** a result-set expression, then the term resolves to a copy of the
** result-set expression.  Otherwise, the expression is resolved in
** the usual way - using sqlite3ResolveExprNames().
**
** This routine returns the number of errors.  If errors occur, then
** an appropriate error message might be left in pParse.  (OOM errors
** excepted.)
*/
fn resolveOrderGroupBy(
    db: &mut Sqlite3,
    pNC: NcId,            /* The name context of the SELECT statement */
    pSelect: SelectId,    /* The SELECT statement holding pOrderBy */
    pOrderBy: ExprListId, /* An ORDER BY or GROUP BY clause to resolve */
    zType: &[u8],         /* Either "ORDER" or "GROUP", as appropriate */
) -> i32 {
    debug_assert!(pOrderBy.nn());
    let pSelEList = db.sel(pSelect).pEList;
    let nResult = db.el(pSelEList).nExpr();
    let n = db.el(pOrderBy).nExpr();
    for i in 0..n {
        let pE = db.el(pOrderBy).a[i as usize].pExpr;
        let pE2 = sqlite3ExprSkipCollateAndLikely(db, pE);
        if pE2.is_null() {
            continue;
        }
        if zType[0] != b'G' {
            let iCol = resolveAsName(db, pSelEList, pE2);
            if iCol > 0 {
                /* If an AS-name match is found, mark this ORDER BY column as being
                 ** a copy of the iCol-th result-set column.  The subsequent call to
                 ** sqlite3ResolveOrderGroupBy() will convert the expression to a
                 ** copy of the iCol-th result-set expression. */
                db.elm(pOrderBy).a[i as usize].u.x.iOrderByCol = iCol as u16;
                continue;
            }
        }
        let mut iCol = 0;
        if sqlite3ExprIsInteger(db, pE2, &mut iCol, 0) != 0 {
            /* The ORDER BY term is an integer constant.  Again, set the column
             ** number so that sqlite3ResolveOrderGroupBy() will convert the
             ** order-by term to a copy of the result-set expression */
            if iCol < 1 || iCol > 0xffff {
                resolveOutOfRangeError(db, zType, i + 1, nResult, pE2);
                return 1;
            }
            db.elm(pOrderBy).a[i as usize].u.x.iOrderByCol = iCol as u16;
            continue;
        }

        /* Otherwise, treat the ORDER BY term as an ordinary expression */
        db.elm(pOrderBy).a[i as usize].u.x.iOrderByCol = 0;
        if sqlite3ResolveExprNames(db, pNC, pE) != 0 {
            return 1;
        }
        let pSelEList = db.sel(pSelect).pEList;
        let nE = db.el(pSelEList).nExpr();
        for j in 0..nE {
            let ej = db.el(pSelEList).a[j as usize].pExpr;
            if sqlite3ExprCompare(db, pE, ej, -1) == 0 {
                /* Since this expression is being changed into a reference
                 ** to an identical expression in the result set, remove all Window
                 ** objects belonging to the expression from the Select.pWin list. */
                windowRemoveExprFromSelect(db, pSelect, pE);
                db.elm(pOrderBy).a[i as usize].u.x.iOrderByCol = (j + 1) as u16;
            }
        }
    }
    sqlite3ResolveOrderGroupBy(db, pSelect, pOrderBy, Some(zType))
}

/*
** Resolve names in the SELECT statement p and all of its descendants.
*/
fn resolveSelectStep(w: &mut Walker<'_>, p: SelectId) -> i32 {
    debug_assert!(p.nn());
    if (w.db.sel(p).selFlags & SF_Resolved) != 0 {
        return WRC_Prune;
    }
    let pOuterNC = w.u.pNC; /* Context that contains this SELECT */
    let db = &mut *w.db;

    /* Normally sqlite3SelectExpand() will be called first and will have
     ** already expanded this SELECT.  However, if this is a subquery within
     ** an expression, sqlite3ResolveExprNames() will be called without a
     ** prior call to sqlite3SelectExpand().  When that happens, let
     ** sqlite3SelectPrep() do all of the processing for this SELECT.
     ** sqlite3SelectPrep() will invoke both sqlite3SelectExpand() and
     ** this routine in the correct order.
     */
    if (db.sel(p).selFlags & SF_Expanded) == 0 {
        sqlite3SelectPrep(db, p, pOuterNC);
        return if db.ppr().nErr != 0 {
            WRC_Abort
        } else {
            WRC_Prune
        };
    }

    let isCompound = db.sel(p).pPrior.nn() as i32; /* True if p is a compound select */
    let mut nCompound = 0; /* Number of compound terms processed so far */
    let pLeftmost = p; /* Left-most of SELECT of a compound */
    let sNC = db.arena.new_nc(NameContext::default()); /* Name context of this SELECT */
    let mut p = p;
    while p.nn() {
        debug_assert!((db.sel(p).selFlags & SF_Expanded) != 0);
        debug_assert!((db.sel(p).selFlags & SF_Resolved) == 0);
        db.selm(p).selFlags |= SF_Resolved;

        /* Resolve the expressions in the LIMIT and OFFSET clauses. These
         ** are not allowed to refer to any names, so pass an empty NameContext.
         */
        *ncm(db, sNC) = NameContext::default();
        ncm(db, sNC).pWinSelect = p;
        let pLimit = db.sel(p).pLimit;
        if sqlite3ResolveExprNames(db, sNC, pLimit) != 0 {
            return WRC_Abort;
        }

        /* If the SF_Converted flags is set, then this Select object was
         ** was created by the convertCompoundSelectToSubquery() function.
         ** In this case the ORDER BY clause (p->pOrderBy) should be resolved
         ** as if it were part of the sub-query, not the parent. This block
         ** moves the pOrderBy down to the sub-query. It will be moved back
         ** after the names have been resolved.  */
        if (db.sel(p).selFlags & SF_Converted) != 0 {
            let pSub = convertedSub(db, p);
            debug_assert!(db.src(db.sel(p).pSrc).nSrc() == 1 && db.sel(p).pOrderBy.nn());
            debug_assert!(db.sel(pSub).pPrior.nn() && db.sel(pSub).pOrderBy.is_null());
            let ob = db.sel(p).pOrderBy;
            db.selm(pSub).pOrderBy = ob;
            db.selm(p).pOrderBy = ExprListId::NULL;
        }

        /* Recursively resolve names in all subqueries in the FROM clause
         */
        if pOuterNC.nn() {
            ncm(db, pOuterNC).nNestedSelect += 1;
        }
        let pSrc = db.sel(p).pSrc;
        let nSrc = db.src(pSrc).nSrc();
        for i in 0..nSrc {
            let pItem = SrcItemId(pSrc, i as u32);
            debug_assert!(db.item(pItem).zName.is_some() || db.item(pItem).fg.isSubquery != 0);
            let pSubSel = if db.item(pItem).fg.isSubquery != 0 {
                db.item(pItem)
                    .u4
                    .pSubq
                    .as_ref()
                    .map(|q| q.pSelect)
                    .unwrap_or_default()
            } else {
                SelectId::NULL
            };
            if pSubSel.nn() && (db.sel(pSubSel).selFlags & SF_Resolved) == 0 {
                let nRef = if pOuterNC.nn() {
                    nc(db, pOuterNC).nRef
                } else {
                    0
                };
                let zSavedContext = db.ppr().zAuthContext.clone();

                if let Some(zName) = db.item(pItem).zName.clone() {
                    db.pp().zAuthContext = Some(zName);
                }
                sqlite3ResolveSelectNames(db, pSubSel, pOuterNC);
                db.pp().zAuthContext = zSavedContext;
                if db.ppr().nErr != 0 {
                    return WRC_Abort;
                }
                debug_assert!(db.mallocFailed == 0);

                /* If the number of references to the outer context changed when
                 ** expressions in the sub-select were resolved, the sub-select
                 ** is correlated. It is not required to check the refcount on any
                 ** but the innermost outer context object, as lookupName() increments
                 ** the refcount on all contexts between the current one and the
                 ** context containing the column when it resolves a name. */
                if pOuterNC.nn() {
                    let now = nc(db, pOuterNC).nRef;
                    debug_assert!(db.item(pItem).fg.isCorrelated == 0 && now >= nRef);
                    db.itemm(pItem).fg.isCorrelated = (now > nRef) as u8;
                }
            }
        }
        if pOuterNC.nn() && nc(db, pOuterNC).nNestedSelect > 0 {
            ncm(db, pOuterNC).nNestedSelect -= 1;
        }

        /* Set up the local name-context to pass to sqlite3ResolveExprNames() to
         ** resolve the result-set expression list.
         */
        {
            let x = ncm(db, sNC);
            x.ncFlags = NC_AllowAgg | NC_AllowWin;
            x.pSrcList = pSrc;
            x.pNext = pOuterNC;
        }

        /* Resolve names in the result set. */
        let pEList = db.sel(p).pEList;
        if sqlite3ResolveExprListNames(db, sNC, pEList) != 0 {
            return WRC_Abort;
        }
        ncm(db, sNC).ncFlags &= !NC_AllowWin;

        /* If there are no aggregate functions in the result-set, and no GROUP BY
         ** expression, do not allow aggregates in any of the other expressions.
         */
        debug_assert!((db.sel(p).selFlags & SF_Aggregate) == 0);
        let pGroupBy = db.sel(p).pGroupBy;
        let ncFlags = nc(db, sNC).ncFlags;
        if pGroupBy.nn() || (ncFlags & NC_HasAgg) != 0 {
            debug_assert!(NC_MinMaxAgg as u32 == SF_MinMaxAgg);
            debug_assert!(NC_OrderAgg as u32 == SF_OrderByReqd);
            db.selm(p).selFlags |= SF_Aggregate | (ncFlags & (NC_MinMaxAgg | NC_OrderAgg)) as u32;
        } else {
            ncm(db, sNC).ncFlags &= !NC_AllowAgg;
        }

        /* Add the output column list to the name-context before parsing the
         ** other expressions in the SELECT statement. This is so that
         ** expressions in the WHERE clause (etc.) can refer to expressions by
         ** aliases in the result set.
         **
         ** Minor point: If this is the case, then the expression will be
         ** re-evaluated for each reference to it.
         */
        debug_assert!((nc(db, sNC).ncFlags & (NC_UAggInfo | NC_UUpsert | NC_UBaseReg)) == 0);
        ncm(db, sNC).uNC.pEList = pEList;
        ncm(db, sNC).ncFlags |= NC_UEList;
        let pHaving = db.sel(p).pHaving;
        if pHaving.nn() {
            if (db.sel(p).selFlags & SF_Aggregate) == 0 {
                errmsg!(db, "HAVING clause on a non-aggregate query");
                return WRC_Abort;
            }
            if sqlite3ResolveExprNames(db, sNC, pHaving) != 0 {
                return WRC_Abort;
            }
        }
        ncm(db, sNC).ncFlags |= NC_Where;
        let pWhere = db.sel(p).pWhere;
        if sqlite3ResolveExprNames(db, sNC, pWhere) != 0 {
            return WRC_Abort;
        }
        ncm(db, sNC).ncFlags &= !NC_Where;

        /* Resolve names in table-valued-function arguments */
        let pSrc = db.sel(p).pSrc;
        let nSrc = db.src(pSrc).nSrc();
        for i in 0..nSrc {
            let (isTabFunc, pFuncArg) = {
                let it = &db.src(pSrc).a[i as usize];
                (it.fg.isTabFunc, it.u1.pFuncArg)
            };
            if isTabFunc != 0 && sqlite3ResolveExprListNames(db, sNC, pFuncArg) != 0 {
                return WRC_Abort;
            }
        }

        if db.ppr().IN_RENAME_OBJECT() {
            let mut pWin = db.sel(p).pWinDefn;
            while pWin.nn() {
                let (ob, part) = (db.win(pWin).pOrderBy, db.win(pWin).pPartition);
                if sqlite3ResolveExprListNames(db, sNC, ob) != 0
                    || sqlite3ResolveExprListNames(db, sNC, part) != 0
                {
                    return WRC_Abort;
                }
                pWin = db.win(pWin).pNextWin;
            }
        }

        ncm(db, sNC).ncFlags |= NC_AllowAgg | NC_AllowWin;

        /* If this is a converted compound query, move the ORDER BY clause from
         ** the sub-query back to the parent query. At this point each term
         ** within the ORDER BY clause has been transformed to an integer value.
         ** These integers will be replaced by copies of the corresponding result
         ** set expressions by the call to resolveOrderGroupBy() below.  */
        if (db.sel(p).selFlags & SF_Converted) != 0 {
            let pSub = convertedSub(db, p);
            let ob = db.sel(pSub).pOrderBy;
            db.selm(p).pOrderBy = ob;
            db.selm(pSub).pOrderBy = ExprListId::NULL;
        }

        /* Process the ORDER BY clause for singleton SELECT statements.
         ** The ORDER BY clause for compounds SELECT statements is handled
         ** below, after all of the result-sets for all of the elements of
         ** the compound have been resolved.
         **
         ** If there is an ORDER BY clause on a term of a compound-select other
         ** than the right-most term, then that is a syntax error.  But the error
         ** is not detected until much later, and so we need to go ahead and
         ** resolve those symbols on the incorrect ORDER BY for consistency.
         */
        let pOrderBy = db.sel(p).pOrderBy;
        if pOrderBy.nn()
            && isCompound <= nCompound /* Defer right-most ORDER BY of a compound */
            && resolveOrderGroupBy(db, sNC, p, pOrderBy, b"ORDER") != 0
        {
            return WRC_Abort;
        }
        if db.mallocFailed != 0 {
            return WRC_Abort;
        }
        ncm(db, sNC).ncFlags &= !NC_AllowWin;

        /* Resolve the GROUP BY clause.  At the same time, make sure
         ** the GROUP BY clause does not contain aggregate functions.
         */
        if pGroupBy.nn() {
            if resolveOrderGroupBy(db, sNC, p, pGroupBy, b"GROUP") != 0 || db.mallocFailed != 0 {
                return WRC_Abort;
            }
            let n = db.el(pGroupBy).nExpr();
            for i in 0..n {
                let e = db.el(pGroupBy).a[i as usize].pExpr;
                if db.ex(e).HasProperty(EP_Agg) {
                    errmsg!(
                        db,
                        "aggregate functions are not allowed in the GROUP BY clause"
                    );
                    return WRC_Abort;
                }
            }
        }

        /* If this is part of a compound SELECT, check that it has the right
         ** number of expressions in the select list. */
        let pNext = db.sel(p).pNext;
        if pNext.nn() {
            let n1 = db.el(db.sel(p).pEList).nExpr();
            let n2 = db.el(db.sel(pNext).pEList).nExpr();
            if n1 != n2 {
                sqlite3SelectWrongNumTermsError(db, pNext);
                return WRC_Abort;
            }
        }

        /* If the SELECT statement contains ON clauses that were moved into
         ** the WHERE clause, go through and verify that none of the terms
         ** in the ON clauses reference tables to the right of the ON clause. */
        if (db.sel(p).selFlags & SF_OnToWhere) != 0 {
            sqlite3SelectCheckOnClauses(db, p);
            if db.ppr().nErr != 0 {
                return WRC_Abort;
            }
        }

        /* Advance to the next term of the compound
         */
        p = db.sel(p).pPrior;
        nCompound += 1;
    }

    /* Resolve the ORDER BY on a compound SELECT after all terms of
     ** the compound have been resolved.
     */
    if isCompound != 0 && resolveCompoundOrderBy(db, pLeftmost) != 0 {
        return WRC_Abort;
    }

    WRC_Prune
}

/// The sub-query of a SF_Converted SELECT (p->pSrc->a[0].u4.pSubq->pSelect).
fn convertedSub(db: &Sqlite3, p: SelectId) -> SelectId {
    let pSrc = db.sel(p).pSrc;
    let it = &db.src(pSrc).a[0];
    debug_assert!(it.fg.isSubquery != 0);
    let pSub = it
        .u4
        .pSubq
        .as_ref()
        .expect("converted compound without subquery")
        .pSelect;
    debug_assert!(pSub.nn());
    pSub
}

/*
** This routine walks an expression tree and resolves references to
** table columns and result-set columns.  At the same time, do error
** checking on function usage and set a flag if any aggregate functions
** are seen.
**
** To resolve table columns references we look for nodes (or subtrees) of the
** form X.Y.Z or Y.Z or just Z where
**
**      X:   The name of a database.  Ex:  "main" or "temp" or
**           the symbolic name assigned to an ATTACH-ed database.
**
**      Y:   The name of a table in a FROM clause.  Or in a trigger
**           one of the special names "old" or "new".
**
**      Z:   The name of a column in table Y.
**
** The node at the root of the subtree is modified as follows:
**
**    Expr.op        Changed to TK_COLUMN
**    Expr.pTab      Points to the Table object for X.Y
**    Expr.iColumn   The column index in X.Y.  -1 for the rowid.
**    Expr.iTable    The VDBE cursor number for X.Y
**
**
** To resolve result-set references, look for expression nodes of the
** form Z (with no X and Y prefix) where the Z matches the right-hand
** size of an AS clause in the result-set of a SELECT.  The Z expression
** is replaced by a copy of the left-hand side of the result-set expression.
** Table-name and function resolution occurs on the substituted expression
** tree.  For example, in:
**
**      SELECT a+b AS x, c+d AS y FROM t1 ORDER BY x;
**
** The "x" term of the order by is replaced by "a+b" to render:
**
**      SELECT a+b AS x, c+d AS y FROM t1 ORDER BY a+b;
**
** Function calls are checked to make sure that the function is
** defined and that the correct number of arguments are specified.
** If the function is an aggregate function, then the NC_HasAgg flag is
** set and the opcode is changed from TK_FUNCTION to TK_AGG_FUNCTION.
** If an expression contains aggregate functions then the EP_Agg
** property on the expression is set.
**
** An error message is left in pParse if anything is amiss.  The number
** if errors is returned.
*/
/// C: `int sqlite3ResolveExprNames( NameContext *pNC, Expr *pExpr )`
pub fn sqlite3ResolveExprNames(db: &mut Sqlite3, pNC: NcId, pExpr: ExprId) -> i32 {
    if pExpr.is_null() {
        return SQLITE_OK;
    }
    const M: i32 = NC_HasAgg | NC_MinMaxAgg | NC_HasWin | NC_OrderAgg;
    let savedHasAgg = nc(db, pNC).ncFlags & M;
    ncm(db, pNC).ncFlags &= !M;
    let noSelect = (nc(db, pNC).ncFlags & NC_NoSelect) != 0;
    let h = db.ex(pExpr).nHeight;
    db.pp().nHeight += h;
    let nHeight = db.ppr().nHeight;
    if sqlite3ExprCheckHeight(db, nHeight) != 0 {
        return SQLITE_ERROR;
    }
    {
        let mut w = Walker::new(db);
        w.xExprCallback = Some(resolveExprStep);
        w.xSelectCallback = if noSelect {
            None
        } else {
            Some(resolveSelectStep)
        };
        w.xSelectCallback2 = None;
        w.u.pNC = pNC;
        sqlite3WalkExprNN(&mut w, pExpr);
    }
    let h = db.ex(pExpr).nHeight;
    db.pp().nHeight -= h;
    debug_assert!(EP_Agg as i32 == NC_HasAgg);
    debug_assert!(EP_Win as i32 == NC_HasWin);
    let f = (nc(db, pNC).ncFlags & (NC_HasAgg | NC_HasWin)) as u32;
    db.exm(pExpr).SetProperty(f);
    ncm(db, pNC).ncFlags |= savedHasAgg;
    (nc(db, pNC).nNcErr > 0 || db.ppr().nErr > 0) as i32
}

/*
** Resolve all names for all expression in an expression list.  This is
** just like sqlite3ResolveExprNames() except that it works for an expression
** list rather than a single expression.
**
** The return value is SQLITE_OK (0) for success or SQLITE_ERROR (1) for a
** failure.
*/
/// C: `int sqlite3ResolveExprListNames( NameContext *pNC, ExprList *pList )`
pub fn sqlite3ResolveExprListNames(db: &mut Sqlite3, pNC: NcId, pList: ExprListId) -> i32 {
    if pList.is_null() {
        return SQLITE_OK;
    }
    const M: i32 = NC_HasAgg | NC_MinMaxAgg | NC_HasWin | NC_OrderAgg;
    let mut savedHasAgg = nc(db, pNC).ncFlags & M;
    ncm(db, pNC).ncFlags &= !M;
    let n = db.el(pList).nExpr();
    for i in 0..n {
        let pExpr = db.el(pList).a[i as usize].pExpr;
        if pExpr.is_null() {
            continue;
        }
        let h = db.ex(pExpr).nHeight;
        db.pp().nHeight += h;
        let nHeight = db.ppr().nHeight;
        if sqlite3ExprCheckHeight(db, nHeight) != 0 {
            return SQLITE_ERROR;
        }
        {
            let mut w = Walker::new(db);
            w.xExprCallback = Some(resolveExprStep);
            w.xSelectCallback = Some(resolveSelectStep);
            w.xSelectCallback2 = None;
            w.u.pNC = pNC;
            sqlite3WalkExprNN(&mut w, pExpr);
        }
        let h = db.ex(pExpr).nHeight;
        db.pp().nHeight -= h;
        debug_assert!(EP_Agg as i32 == NC_HasAgg);
        debug_assert!(EP_Win as i32 == NC_HasWin);
        let ncFlags = nc(db, pNC).ncFlags;
        if (ncFlags & M) != 0 {
            db.exm(pExpr)
                .SetProperty((ncFlags & (NC_HasAgg | NC_HasWin)) as u32);
            savedHasAgg |= ncFlags & M;
            ncm(db, pNC).ncFlags &= !M;
        }
        if db.ppr().nErr > 0 {
            return SQLITE_ERROR;
        }
    }
    ncm(db, pNC).ncFlags |= savedHasAgg;
    SQLITE_OK
}

/*
** Resolve all names in all expressions of a SELECT and in all
** descendants of the SELECT, including compounds off of p->pPrior,
** subqueries in expressions, and subqueries used as FROM clause
** terms.
**
** See sqlite3ResolveExprNames() for a description of the kinds of
** transformations that occur.
**
** All SELECT statements should have been expanded using
** sqlite3SelectExpand() prior to invoking this routine.
*/
/// C: `void sqlite3ResolveSelectNames( Parse *pParse, Select *p, NameContext *pOuterNC )`
pub fn sqlite3ResolveSelectNames(db: &mut Sqlite3, p: SelectId, pOuterNC: NcId) {
    debug_assert!(p.nn());
    let mut w = Walker::new(db);
    w.xExprCallback = Some(resolveExprStep);
    w.xSelectCallback = Some(resolveSelectStep);
    w.xSelectCallback2 = None;
    w.u.pNC = pOuterNC;
    sqlite3WalkSelect(&mut w, p);
}

/*
** Resolve names in expressions that can only reference a single table
** or which cannot reference any tables at all.  Examples:
**
**                                                    "type" flag
**                                                    ------------
**    (1)   CHECK constraints                         NC_IsCheck
**    (2)   WHERE clauses on partial indices          NC_PartIdx
**    (3)   Expressions in indexes on expressions     NC_IdxExpr
**    (4)   Expression arguments to VACUUM INTO.      0
**    (5)   GENERATED ALWAYS as expressions           NC_GenCol
**
** In all cases except (4), the Expr.iTable value for Expr.op==TK_COLUMN
** nodes of the expression is set to -1 and the Expr.iColumn value is
** set to the column number.  In case (4), TK_COLUMN nodes cause an error.
**
** Any errors cause an error message to be set in pParse.
*/
/// C: `int sqlite3ResolveSelfReference( Parse *pParse, Table *pTab, int type, Expr *pExpr, ExprList *pList )`
///
/// Rust: `pTab` is None for the C NULL table (VACUUM INTO).
pub fn sqlite3ResolveSelfReference(
    db: &mut Sqlite3,
    pTab: Option<&TableRef>,
    type_: i32,
    pExpr: ExprId,
    pList: ExprListId,
) -> i32 {
    let mut type_ = type_;
    debug_assert!(type_ == 0 || pTab.is_some());
    debug_assert!(
        type_ == NC_IsCheck
            || type_ == NC_PartIdx
            || type_ == NC_IdxExpr
            || type_ == NC_GenCol
            || pTab.is_none()
    );
    let mut sSrc = SrcList::default(); /* Fake SrcList for pParse->pNewTable */
    if let Some(t) = pTab {
        let (zName, tabSchema) = {
            let b = t.borrow();
            (b.zName.clone(), b.pSchema)
        };
        let mut it = SrcItem::default();
        it.zName = Some(zName);
        it.pSTab = Some(t.clone());
        it.iCursor = -1;
        sSrc.a.push(it);
        if db.aDb.len() > 1 && tabSchema != db.aDb[1].pSchema.id {
            /* Cause EP_FromDDL to be set on TK_FUNCTION nodes of non-TEMP
             ** schema elements */
            type_ |= NC_FromDDL;
        }
    }
    let pSrc = db.arena.new_srclist(sSrc);
    let mut sNC = NameContext::default(); /* Name context for pParse->pNewTable */
    sNC.pSrcList = pSrc;
    sNC.ncFlags = type_ | NC_IsDDL;
    let sNC = db.arena.new_nc(sNC);
    let mut rc = sqlite3ResolveExprNames(db, sNC, pExpr);
    if rc != SQLITE_OK {
        return rc;
    }
    if pList.nn() {
        rc = sqlite3ResolveExprListNames(db, sNC, pList);
    }
    rc
}
