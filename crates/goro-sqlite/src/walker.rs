//! Port of walker.c.
//!
//! Routines used for walking the parser tree for an SQL statement.
//!
//! The callbacks are plain fn pointers stored in the Walker
//! (`fn(&mut Walker, ExprId) -> i32` ...); they reach the connection, and
//! through it the parse-tree arena, via `w.db`.

use crate::int::*;

/// Walk all expressions linked into the list of Window objects passed
/// as the second argument.
fn walkWindowList(w: &mut Walker, pList: WindowId, bOneOnly: bool) -> i32 {
    let mut pWin = pList;
    while pWin.nn() {
        let (pOrderBy, pPartition, pFilter, pStart, pEnd, pNext) = {
            let x = w.db.win(pWin);
            (
                x.pOrderBy,
                x.pPartition,
                x.pFilter,
                x.pStart,
                x.pEnd,
                x.pNextWin,
            )
        };
        if sqlite3WalkExprList(w, pOrderBy) != 0 {
            return WRC_Abort;
        }
        if sqlite3WalkExprList(w, pPartition) != 0 {
            return WRC_Abort;
        }
        if sqlite3WalkExpr(w, pFilter) != 0 {
            return WRC_Abort;
        }
        if sqlite3WalkExpr(w, pStart) != 0 {
            return WRC_Abort;
        }
        if sqlite3WalkExpr(w, pEnd) != 0 {
            return WRC_Abort;
        }
        if bOneOnly {
            break;
        }
        pWin = pNext;
    }
    WRC_Continue
}

/// Walk an expression tree.  Invoke the callback once for each node
/// of the expression, while descending.  (In other words, the callback
/// is invoked before visiting children.)
///
/// The return value from the callback should be one of the WRC_*
/// constants to specify how to proceed with the walk.
///
///    WRC_Continue      Continue descending down the tree.
///
///    WRC_Prune         Do not descend into child nodes, but allow
///                      the walk to continue with sibling nodes.
///
///    WRC_Abort         Do no more callbacks.  Unwind the stack and
///                      return from the top-level walk call.
///
/// The return value from this routine is WRC_Abort to abandon the tree walk
/// and WRC_Continue to continue.
pub fn sqlite3WalkExprNN(w: &mut Walker, pExpr: ExprId) -> i32 {
    let mut pExpr = pExpr;
    loop {
        let xcb = w.xExprCallback.expect("Walker without xExprCallback");
        let rc = xcb(w, pExpr);
        if rc != 0 {
            return rc & WRC_Abort;
        }
        let (flags, pLeft, pRight, pList, pSelect, pWin) = {
            let e = w.db.ex(pExpr);
            (e.flags, e.pLeft, e.pRight, e.x.pList, e.x.pSelect, e.y.pWin)
        };
        if (flags & (EP_TokenOnly | EP_Leaf)) == 0 {
            debug_assert!(pList.is_null() || pRight.is_null());
            if pLeft.nn() && sqlite3WalkExprNN(w, pLeft) != 0 {
                return WRC_Abort;
            }
            if pRight.nn() {
                debug_assert!((flags & EP_WinFunc) == 0);
                pExpr = pRight;
                continue;
            } else if (flags & EP_xIsSelect) != 0 {
                debug_assert!((flags & EP_WinFunc) == 0);
                if sqlite3WalkSelect(w, pSelect) != 0 {
                    return WRC_Abort;
                }
            } else {
                if pList.nn() && sqlite3WalkExprList(w, pList) != 0 {
                    return WRC_Abort;
                }
                if (flags & EP_WinFunc) != 0 && walkWindowList(w, pWin, true) != 0 {
                    return WRC_Abort;
                }
            }
        }
        break;
    }
    WRC_Continue
}

/// C: `int sqlite3WalkExpr(Walker *pWalker, Expr *pExpr)`
pub fn sqlite3WalkExpr(w: &mut Walker, pExpr: ExprId) -> i32 {
    if pExpr.nn() {
        sqlite3WalkExprNN(w, pExpr)
    } else {
        WRC_Continue
    }
}

/// Call sqlite3WalkExpr() for every expression in list p or until
/// an abort request is seen.
pub fn sqlite3WalkExprList(w: &mut Walker, p: ExprListId) -> i32 {
    if p.nn() {
        let n = w.db.el(p).a.len();
        let mut i = 0;
        while i < n {
            // The list may not change size during the walk, but re-read the
            // item each time as callbacks may rewrite it.
            let e = match w.db.el(p).a.get(i) {
                Some(it) => it.pExpr,
                None => break,
            };
            if sqlite3WalkExpr(w, e) != 0 {
                return WRC_Abort;
            }
            i += 1;
        }
    }
    WRC_Continue
}

/// This is a no-op callback for Walker->xSelectCallback2.  If this
/// callback is set, then the Select->pWinDefn list is traversed.
pub fn sqlite3WalkWinDefnDummyCallback(w: &mut Walker, p: SelectId) {
    let _ = (w, p);
    /* No-op */
}

/// Walk all expressions associated with SELECT statement p.  Do
/// not invoke the SELECT callback on p, but do (of course) invoke
/// any expr callbacks and SELECT callbacks that come from subqueries.
/// Return WRC_Abort or WRC_Continue.
pub fn sqlite3WalkSelectExpr(w: &mut Walker, p: SelectId) -> i32 {
    let (pEList, pWhere, pGroupBy, pHaving, pOrderBy, pLimit) = {
        let s = w.db.sel(p);
        (
            s.pEList, s.pWhere, s.pGroupBy, s.pHaving, s.pOrderBy, s.pLimit,
        )
    };
    if sqlite3WalkExprList(w, pEList) != 0 {
        return WRC_Abort;
    }
    if sqlite3WalkExpr(w, pWhere) != 0 {
        return WRC_Abort;
    }
    if sqlite3WalkExprList(w, pGroupBy) != 0 {
        return WRC_Abort;
    }
    if sqlite3WalkExpr(w, pHaving) != 0 {
        return WRC_Abort;
    }
    if sqlite3WalkExprList(w, pOrderBy) != 0 {
        return WRC_Abort;
    }
    if sqlite3WalkExpr(w, pLimit) != 0 {
        return WRC_Abort;
    }
    let pWinDefn = w.db.sel(p).pWinDefn;
    if pWinDefn.nn() {
        let cb2 = w.xSelectCallback2;
        let isDummy = matches!(cb2, Some(f) if std::ptr::fn_addr_eq(
            f,
            sqlite3WalkWinDefnDummyCallback as XSelectCallback2
        ));
        let isPopWith = matches!(cb2, Some(f) if std::ptr::fn_addr_eq(
            f,
            crate::select::sqlite3SelectPopWith as XSelectCallback2
        ));
        let inRename = w.pParse
            && w.db
                .parses
                .last()
                .map(|pp| pp.IN_RENAME_OBJECT())
                .unwrap_or(false);
        if isDummy || inRename || isPopWith {
            /* The following may return WRC_Abort if there are unresolvable
             ** symbols (e.g. a table that does not exist) in a window definition. */
            return walkWindowList(w, pWinDefn, false);
        }
    }
    WRC_Continue
}

/// Walk the parse trees associated with all subqueries in the
/// FROM clause of SELECT statement p.  Do not invoke the select
/// callback on p, but do invoke it on each FROM clause subquery
/// and on any subqueries further down in the tree.  Return
/// WRC_Abort or WRC_Continue;
pub fn sqlite3WalkSelectFrom(w: &mut Walker, p: SelectId) -> i32 {
    let pSrc = w.db.sel(p).pSrc;
    if pSrc.nn() {
        let n = w.db.src(pSrc).a.len();
        for i in 0..n {
            let (isSubquery, subSel, isTabFunc, pFuncArg) = {
                let it = &w.db.src(pSrc).a[i];
                (
                    it.fg.isSubquery != 0,
                    it.u4.pSubq.as_ref().map(|q| q.pSelect).unwrap_or_default(),
                    it.fg.isTabFunc != 0,
                    it.u1.pFuncArg,
                )
            };
            if isSubquery && sqlite3WalkSelect(w, subSel) != 0 {
                return WRC_Abort;
            }
            if isTabFunc && sqlite3WalkExprList(w, pFuncArg) != 0 {
                return WRC_Abort;
            }
        }
    }
    WRC_Continue
}

/// Call sqlite3WalkExpr() for every expression in Select statement p.
/// Invoke sqlite3WalkSelect() for subqueries in the FROM clause and
/// on the compound select chain, p->pPrior.
///
/// If it is not NULL, the xSelectCallback() callback is invoked before
/// the walk of the expressions and FROM clause. The xSelectCallback2()
/// method is invoked following the walk of the expressions and FROM clause,
/// but only if both xSelectCallback and xSelectCallback2 are both non-NULL
/// and if the expressions and FROM clause both return WRC_Continue;
///
/// Return WRC_Continue under normal conditions.  Return WRC_Abort if
/// there is an abort request.
///
/// If the Walker does not have an xSelectCallback() then this routine
/// is a no-op returning WRC_Continue.
pub fn sqlite3WalkSelect(w: &mut Walker, p: SelectId) -> i32 {
    let mut p = p;
    if p.is_null() {
        return WRC_Continue;
    }
    let Some(xcb) = w.xSelectCallback else {
        return WRC_Continue;
    };
    loop {
        let rc = xcb(w, p);
        if rc != 0 {
            return rc & WRC_Abort;
        }
        if sqlite3WalkSelectExpr(w, p) != 0 || sqlite3WalkSelectFrom(w, p) != 0 {
            return WRC_Abort;
        }
        if let Some(cb2) = w.xSelectCallback2 {
            cb2(w, p);
        }
        p = w.db.sel(p).pPrior;
        if p.is_null() {
            break;
        }
    }
    WRC_Continue
}

/// Increase the walkerDepth when entering a subquery, and
/// decrease when leaving the subquery.
pub fn sqlite3WalkerDepthIncrease(w: &mut Walker, pSelect: SelectId) -> i32 {
    let _ = pSelect;
    w.walkerDepth += 1;
    WRC_Continue
}

/// C: `void sqlite3WalkerDepthDecrease(Walker *pWalker, Select *pSelect)`
pub fn sqlite3WalkerDepthDecrease(w: &mut Walker, pSelect: SelectId) {
    let _ = pSelect;
    w.walkerDepth -= 1;
}

/// No-op routine for the parse-tree walker.
///
/// When this routine is the Walker.xExprCallback then expression trees
/// are walked without any actions being taken at each node.  Presumably,
/// when this routine is used for Walker.xExprCallback then
/// Walker.xSelectCallback is set to do something useful for every
/// subquery in the parser tree.
pub fn sqlite3ExprWalkNoop(w: &mut Walker, NotUsed2: ExprId) -> i32 {
    let _ = (w, NotUsed2);
    WRC_Continue
}

/// No-op routine for the parse-tree walker for SELECT statements.
/// subquery in the parser tree.
pub fn sqlite3SelectWalkNoop(w: &mut Walker, NotUsed2: SelectId) -> i32 {
    let _ = (w, NotUsed2);
    WRC_Continue
}
