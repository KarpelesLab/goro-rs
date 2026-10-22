//! Port of wherecode.c.
//!
//! This module contains the routines that actually generate the bulk of
//! the WHERE loop code.  The original where.c file retains the code that
//! does query planning and analysis.  See the module documentation of
//! `where_` for the index-based representation of the planner objects: the
//! functions here take the WhereInfo plus the index of the level (and the
//! TermId of terms) instead of C pointers.

#![allow(unused_variables)]

use crate::expr::*;
use crate::int::*;
use crate::vdbeaux::*;
use crate::where_::*;
use crate::whereexpr::*;
use std::rc::Rc;

/// Return the name of the i-th column of the pIdx index.
fn explainIndexColumnName(pIdx: &IndexRef, i: i32) -> ZStr {
    let p = pIdx.borrow();
    let i = p.aiColumn[i as usize];
    if i == XN_EXPR {
        return b"<expr>".to_vec();
    }
    if i == XN_ROWID {
        return b"rowid".to_vec();
    }
    p.table().borrow().aCol[i as usize].zCnName.clone()
}

/// This routine is a helper for explainIndexRange() below
///
/// pStr holds the text of an expression that we are building up one term
/// at a time.  This routine adds a new term to the end of the expression.
/// Terms are separated by AND so add the "AND" text for second and
/// subsequent terms only.
fn explainAppendTerm(
    pStr: &mut Vec<u8>,
    pIdx: &IndexRef,
    nTerm: i32,
    iTerm: i32,
    bAnd: i32,
    zOp: u8,
) {
    debug_assert!(nTerm >= 1);
    if bAnd != 0 {
        pStr.extend_from_slice(b" AND ");
    }

    if nTerm > 1 {
        pStr.push(b'(');
    }
    for i in 0..nTerm {
        if i != 0 {
            pStr.push(b',');
        }
        pStr.extend_from_slice(&explainIndexColumnName(pIdx, iTerm + i));
    }
    if nTerm > 1 {
        pStr.push(b')');
    }

    pStr.push(zOp);

    if nTerm > 1 {
        pStr.push(b'(');
    }
    for i in 0..nTerm {
        if i != 0 {
            pStr.push(b',');
        }
        pStr.push(b'?');
    }
    if nTerm > 1 {
        pStr.push(b')');
    }
}

/// Argument pLevel describes a strategy for scanning table pTab. This
/// function appends text to pStr that describes the subset of table
/// rows scanned by the strategy in the form of an SQL expression.
///
/// For example, if the query:
///
///   SELECT * FROM t1 WHERE a=1 AND b>2;
///
/// is run and there is an index on (a, b), then this function returns a
/// string similar to:
///
///   "a=? AND b>?"
fn explainIndexRange(pStr: &mut Vec<u8>, pLoop: &WhereLoop) {
    let pIndex = pLoop.u.btree.pIndex.as_ref().unwrap();
    let nEq = pLoop.u.btree.nEq as i32;
    let nSkip = pLoop.nSkip as i32;

    if nEq == 0 && (pLoop.wsFlags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) == 0 {
        return;
    }
    pStr.extend_from_slice(b" (");
    let mut i = 0;
    while i < nEq {
        let z = explainIndexColumnName(pIndex, i);
        if i != 0 {
            pStr.extend_from_slice(b" AND ");
        }
        if i >= nSkip {
            pStr.extend_from_slice(&z);
            pStr.extend_from_slice(b"=?");
        } else {
            pStr.extend_from_slice(b"ANY(");
            pStr.extend_from_slice(&z);
            pStr.push(b')');
        }
        i += 1;
    }

    let j = i;
    if pLoop.wsFlags & WHERE_BTM_LIMIT != 0 {
        explainAppendTerm(pStr, pIndex, pLoop.u.btree.nBtm as i32, j, i, b'>');
        i = 1;
    }
    if pLoop.wsFlags & WHERE_TOP_LIMIT != 0 {
        explainAppendTerm(pStr, pIndex, pLoop.u.btree.nTop as i32, j, i, b'<');
    }
    pStr.push(b')');
}

/// This function sets the P4 value of an existing OP_Explain opcode to
/// text describing the loop in pLevel. If the OP_Explain opcode already has
/// a P4 value, it is freed before it is overwritten.
///
/// C: `void sqlite3WhereAddExplainText( Parse *pParse, int addr, SrcList *pTabList, WhereLevel *pLevel, u16 wctrlFlags )`
/// (Rust: the level is designated by its index in `pWInfo.a`)
pub fn sqlite3WhereAddExplainText(
    db: &mut Sqlite3,
    addr: i32,
    pTabList: SrcListId,
    pWInfo: &WhereInfo,
    iLevel: usize,
    wctrlFlags: u16,
) {
    if db.toplevel().explain == 2 {
        let pItem = SrcItemId(pTabList, pWInfo.a[iLevel].iFrom as u32);
        if db.mallocFailed != 0 {
            return;
        }

        let pLoop = pWInfo.lp(iLevel);
        let flags = pLoop.wsFlags;

        let isSearch = (flags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) != 0
            || ((flags & WHERE_VIRTUALTABLE) == 0 && (pLoop.u.btree.nEq > 0))
            || (wctrlFlags & (WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)) != 0;

        let fromExists = db.item(pItem).fg.fromExists != 0;
        let mut str: Vec<u8> = mprintf!(
            db,
            b"%s %S%s",
            if isSearch {
                &b"SEARCH"[..]
            } else {
                &b"SCAN"[..]
            },
            pItem,
            if fromExists {
                &b" EXISTS"[..]
            } else {
                &b""[..]
            }
        );
        let pTab = item_tab(db, pItem);
        if (flags & (WHERE_IPK | WHERE_VIRTUALTABLE)) == 0 {
            let mut zFmt: Option<&[u8]> = None;

            debug_assert!(pLoop.u.btree.pIndex.is_some());
            let pIdx = pLoop.u.btree.pIndex.as_ref().unwrap();
            debug_assert!((flags & WHERE_AUTO_INDEX) == 0 || (flags & WHERE_IDX_ONLY) != 0);
            let mut withName = false;
            if !pTab.borrow().HasRowid() && pIdx.borrow().IsPrimaryKeyIndex() {
                if isSearch {
                    zFmt = Some(b"PRIMARY KEY");
                }
            } else if flags & WHERE_PARTIALIDX != 0 {
                zFmt = Some(b"AUTOMATIC PARTIAL COVERING INDEX");
            } else if flags & WHERE_AUTO_INDEX != 0 {
                zFmt = Some(b"AUTOMATIC COVERING INDEX");
            } else if flags & (WHERE_IDX_ONLY | WHERE_EXPRIDX) != 0 {
                zFmt = Some(b"COVERING INDEX ");
                withName = true;
            } else {
                zFmt = Some(b"INDEX ");
                withName = true;
            }
            if let Some(z) = zFmt {
                str.extend_from_slice(b" USING ");
                str.extend_from_slice(z);
                if withName {
                    str.extend_from_slice(&pIdx.borrow().zName);
                }
                explainIndexRange(&mut str, pLoop);
            }
        } else if (flags & WHERE_IPK) != 0 && (flags & WHERE_CONSTRAINT) != 0 {
            let cRangeOp: u8;
            let zRowid: &[u8] = b"rowid";
            str.extend_from_slice(b" USING INTEGER PRIMARY KEY (");
            str.extend_from_slice(zRowid);
            if flags & (WHERE_COLUMN_EQ | WHERE_COLUMN_IN) != 0 {
                cRangeOp = b'=';
            } else if (flags & WHERE_BOTH_LIMIT) == WHERE_BOTH_LIMIT {
                str.extend_from_slice(b">? AND ");
                str.extend_from_slice(zRowid);
                cRangeOp = b'<';
            } else if flags & WHERE_BTM_LIMIT != 0 {
                cRangeOp = b'>';
            } else {
                debug_assert!(flags & WHERE_TOP_LIMIT != 0);
                cRangeOp = b'<';
            }
            str.push(cRangeOp);
            str.extend_from_slice(b"?)");
        } else if (flags & WHERE_VIRTUALTABLE) != 0 {
            str.extend_from_slice(b" VIRTUAL TABLE INDEX ");
            let z = pLoop.u.vtab.idxStr.clone().unwrap_or_default();
            if pLoop.u.vtab.bIdxNumHex != 0 {
                str.extend_from_slice(format!("0x{:x}:", pLoop.u.vtab.idxNum).as_bytes());
            } else {
                str.extend_from_slice(format!("{}:", pLoop.u.vtab.idxNum).as_bytes());
            }
            str.extend_from_slice(&z);
        }
        if db.item(pItem).fg.jointype & JT_LEFT != 0 {
            str.extend_from_slice(b" LEFT-JOIN");
        }

        let pOp = sqlite3VdbeGetOp(db.v(), addr);
        debug_assert!(pOp.opcode == OP_Explain);
        pOp.p4type = P4_DYNAMIC;
        pOp.p4 = P4::Z(str);
    }
}

/// This function is a no-op unless currently processing an EXPLAIN QUERY
/// PLAN command, or if stmt_scanstatus_v2() stats are enabled, or if
/// SQLITE_DEBUG was defined at compile-time. If it is not a no-op, a single
/// OP_Explain opcode is added to the output to describe the table scan
/// strategy in pLevel.
///
/// If an OP_Explain opcode is added to the VM, its address is returned.
/// Otherwise, if no OP_Explain is coded, zero is returned.
///
/// C: `int sqlite3WhereExplainOneScan( Parse *pParse, SrcList *pTabList, WhereLevel *pLevel, u16 wctrlFlags )`
pub fn sqlite3WhereExplainOneScan(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pWInfo: &WhereInfo,
    iLevel: usize,
    wctrlFlags: u16,
) -> i32 {
    let mut ret = 0;
    if db.toplevel().explain == 2 {
        let pLoop = pWInfo.lp(iLevel);
        if (pLoop.wsFlags & WHERE_MULTI_OR) == 0 && (wctrlFlags & WHERE_OR_SUBCLAUSE) == 0 {
            let rRun = pLoop.rRun as i32;
            let ae = db.ppr().addrExplain;
            let addr = sqlite3VdbeCurrentAddr(db.v());
            ret = sqlite3VdbeAddOp3(db.v(), OP_Explain, addr, ae, rRun);
            sqlite3WhereAddExplainText(db, addr, pTabList, pWInfo, iLevel, wctrlFlags);
        }
    }
    ret
}

/// Add a single OP_Explain opcode that describes a Bloom filter.
///
/// Or if not processing EXPLAIN QUERY PLAN and not in a SQLITE_DEBUG and/or
/// SQLITE_ENABLE_STMT_SCANSTATUS build, then OP_Explain opcodes are not
/// required and this routine is a no-op.
///
/// If an OP_Explain opcode is added to the VM, its address is returned.
/// Otherwise, if no OP_Explain is coded, zero is returned.
///
/// C: `int sqlite3WhereExplainBloomFilter( const Parse *pParse, const WhereInfo *pWInfo, const WhereLevel *pLevel )`
pub fn sqlite3WhereExplainBloomFilter(db: &mut Sqlite3, pWInfo: &WhereInfo, iLevel: usize) -> i32 {
    let pItem = SrcItemId(pWInfo.pTabList, pWInfo.a[iLevel].iFrom as u32);
    let mut str: Vec<u8> = mprintf!(db, b"BLOOM FILTER ON %S (", pItem);
    let pLoop = pWInfo.lp(iLevel);
    if pLoop.wsFlags & WHERE_IPK != 0 {
        let pTab = item_tab(db, pItem);
        let t = pTab.borrow();
        if t.iPKey >= 0 {
            str.extend_from_slice(&t.aCol[t.iPKey as usize].zCnName);
            str.extend_from_slice(b"=?");
        } else {
            str.extend_from_slice(b"rowid=?");
        }
    } else {
        let pIdx = pLoop.u.btree.pIndex.as_ref().unwrap();
        for i in pLoop.nSkip as i32..pLoop.u.btree.nEq as i32 {
            let z = explainIndexColumnName(pIdx, i);
            if i > pLoop.nSkip as i32 {
                str.extend_from_slice(b" AND ");
            }
            str.extend_from_slice(&z);
            str.extend_from_slice(b"=?");
        }
    }
    str.push(b')');
    let ae = db.ppr().addrExplain;
    let addr = sqlite3VdbeCurrentAddr(db.v());
    let ret = sqlite3VdbeAddOp4(
        db.v(),
        OP_Explain,
        addr,
        ae,
        0,
        P4::Z(str),
        P4_DYNAMIC as i32,
    );

    // sqlite3VdbeScanStatus(v, sqlite3VdbeCurrentAddr(v)-1, 0, 0, 0, 0):
    // a no-op without SQLITE_ENABLE_STMT_SCANSTATUS.
    ret
}

/// Configure the VM passed as the first argument with an
/// sqlite3_stmt_scanstatus() entry corresponding to the scan used to
/// implement level pLvl. Argument pSrclist is a pointer to the FROM
/// clause that the scan reads data from.
///
/// SQLITE_ENABLE_STMT_SCANSTATUS is not enabled: this is a no-op.
///
/// C: `void sqlite3WhereAddScanStatus( Vdbe *v, SrcList *pSrclist, WhereLevel *pLvl, int addrExplain )`
pub fn sqlite3WhereAddScanStatus(
    db: &mut Sqlite3,
    pSrclist: SrcListId,
    pWInfo: &WhereInfo,
    iLevel: usize,
    addrExplain: i32,
) {
}

/// Disable a term in the WHERE clause.  Except, do not disable the term
/// if it controls a LEFT OUTER JOIN and it did not originate in the ON
/// or USING clause of that join.
///
/// Consider the term t2.z='ok' in the following queries:
///
///   (1)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x WHERE t2.z='ok'
///   (2)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x AND t2.z='ok'
///   (3)  SELECT * FROM t1, t2 WHERE t1.a=t2.x AND t2.z='ok'
///
/// The t2.z='ok' is disabled in the in (2) because it originates
/// in the ON clause.  The term is disabled in (3) because it is not part
/// of a LEFT OUTER JOIN.  In (1), the term is not disabled.
///
/// Disabling a term causes that term to not be tested in the inner loop
/// of the join.  Disabling is an optimization.  When terms are satisfied
/// by indices, we disable them to prevent redundant tests in the inner
/// loop.  We would get the correct results if nothing were ever disabled,
/// but joins might run a little slower.  The trick is to disable as much
/// as we can without disabling too much.  If we disabled in (1), we'd get
/// the wrong answer.  See ticket #813.
///
/// If all the children of a term are disabled, then that term is also
/// automatically disabled.  In this way, terms get disabled if derived
/// virtual terms are tested first.  For example:
///
///      x GLOB 'abc*' AND x>='abc' AND x<'acd'
///      \___________/     \______/     \_____/
///         parent          child1       child2
///
/// Only the parent term was in the original WHERE clause.  The child1
/// and child2 terms were added by the LIKE optimization.  If both of
/// the virtual child terms are valid, then testing of the parent can be
/// skipped.
///
/// Usually the parent term is marked as TERM_CODED.  But if the parent
/// term was originally TERM_LIKE, then the parent gets TERM_LIKECOND
/// instead.  The TERM_LIKECOND marking indicates that the term should be
/// coded inside a conditional such that is only evaluated on the second
/// pass of a LIKE-optimization loop, when scanning BLOBs instead of
/// strings.
fn disableTerm(db: &Sqlite3, wi: &mut WhereInfo, iLevel: usize, pTerm: TermId) {
    let mut nLoop = 0;
    let mut pTerm = pTerm;
    let (iLeftJoin, notReady) = (wi.a[iLevel].iLeftJoin, wi.a[iLevel].notReady);
    loop {
        let t = *wi.t(pTerm);
        if !((t.wtFlags & TERM_CODED) == 0
            && (iLeftJoin == 0 || db.ex(t.pExpr).HasProperty(EP_OuterON))
            && (notReady & t.prereqAll) == 0)
        {
            break;
        }
        if nLoop != 0 && (t.wtFlags & TERM_LIKE) != 0 {
            wi.tm(pTerm).wtFlags |= TERM_LIKECOND;
        } else {
            wi.tm(pTerm).wtFlags |= TERM_CODED;
        }
        if t.iParent < 0 {
            break;
        }
        pTerm = TermId {
            i: t.iParent as u32,
            ..pTerm
        };
        let p = wi.tm(pTerm);
        p.nChild = p.nChild.wrapping_sub(1);
        if p.nChild != 0 {
            break;
        }
        nLoop += 1;
    }
}

/// Code an OP_Affinity opcode to apply the column affinity string zAff
/// to the n registers starting at base.
///
/// As an optimization, SQLITE_AFF_BLOB and SQLITE_AFF_NONE entries (which
/// are no-ops) at the beginning and end of zAff are ignored.  If all
/// entries in zAff are SQLITE_AFF_BLOB or SQLITE_AFF_NONE, then no code
/// gets generated.
///
/// This routine makes its own copy of zAff so that the caller is free
/// to modify zAff after this routine returns.
fn codeApplyAffinity(db: &mut Sqlite3, base: i32, n: i32, zAff: Option<&[u8]>) {
    let Some(zAff) = zAff else {
        debug_assert!(db.mallocFailed != 0);
        return;
    };
    let mut base = base;
    let mut n = n;
    let mut off = 0usize;
    let at = |i: usize| -> u8 { zAff.get(i).copied().unwrap_or(0) };

    // Adjust base and n to skip over SQLITE_AFF_BLOB and SQLITE_AFF_NONE
    // entries at the beginning and end of the affinity string.
    while n > 0 && at(off) <= SQLITE_AFF_BLOB {
        n -= 1;
        base += 1;
        off += 1;
    }
    while n > 1 && at(off + n as usize - 1) <= SQLITE_AFF_BLOB {
        n -= 1;
    }

    // Code the OP_Affinity opcode if there is anything left to do.
    if n > 0 {
        let z: Vec<u8> = (0..n as usize).map(|i| at(off + i)).collect();
        sqlite3VdbeAddOp4(db.v(), OP_Affinity, base, n, 0, P4::Z(z), P4_DYNAMIC as i32);
    }
}

/// Expression pRight, which is the RHS of a comparison operation, is
/// either a vector of n elements or, if n==1, a scalar expression.
/// Before the comparison operation, affinity zAff is to be applied
/// to the pRight values. This function modifies characters within the
/// affinity string to SQLITE_AFF_BLOB if either:
///
///   * the comparison will be performed with no affinity, or
///   * the affinity change in zAff is guaranteed not to change the value.
fn updateRangeAffinityStr(db: &mut Sqlite3, pRight: ExprId, n: i32, zAff: &mut [u8]) {
    for i in 0..n as usize {
        if i >= zAff.len() {
            break;
        }
        let p = sqlite3VectorFieldSubexpr(db, pRight, i as i32);
        if sqlite3CompareAffinity(db, p, zAff[i]) == SQLITE_AFF_BLOB
            || sqlite3ExprNeedsNoAffinityChange(db, p, zAff[i]) != 0
        {
            zAff[i] = SQLITE_AFF_BLOB;
        }
    }
}

/// The pOrderBy->a[].u.x.iOrderByCol values might be incorrect because
/// columns might have been rearranged in the result set.  This routine
/// fixes them up.
///
/// pEList is the new result set.  The pEList->a[].u.x.iOrderByCol values
/// contain the *old* locations of each expression.  This is a temporary
/// use of u.x.iOrderByCol, not its intended use.  The caller must reset
/// u.x.iOrderByCol back to zero for all entries in pEList before the
/// caller returns.
///
/// This routine changes pOrderBy->a[].u.x.iOrderByCol values from
/// pEList->a[N].u.x.iOrderByCol into N+1.  (The "+1" is because of the
/// 1-based indexing used by iOrderByCol.)  Or if no match, iOrderByCol is
/// set to zero.
fn adjustOrderByCol(db: &mut Sqlite3, pOrderBy: ExprListId, pEList: ExprListId) {
    if pOrderBy.is_null() {
        return;
    }
    let n = db.el(pOrderBy).nExpr();
    let ne = db.el(pEList).nExpr();
    for i in 0..n as usize {
        let t = db.el(pOrderBy).a[i].u.x.iOrderByCol;
        if t == 0 {
            continue;
        }
        let mut j = 0;
        while j < ne as usize {
            if db.el(pEList).a[j].u.x.iOrderByCol == t {
                db.elm(pOrderBy).a[i].u.x.iOrderByCol = (j + 1) as u16;
                break;
            }
            j += 1;
        }
        if j >= ne as usize {
            db.elm(pOrderBy).a[i].u.x.iOrderByCol = 0;
        }
    }
}

/// pX is an expression of the form:  (vector) IN (SELECT ...)
/// In other words, it is a vector IN operator with a SELECT clause on the
/// RHS.  But not all terms in the vector are indexable and the terms might
/// not be in the correct order for indexing.
///
/// This routine makes a copy of the input pX expression and then adjusts
/// the vector on the LHS with corresponding changes to the SELECT so that
/// the vector contains only index terms and those terms are in the correct
/// order.  The modified IN expression is returned.  The caller is
/// responsible for deleting the returned expression.
///
/// Example:
///
///    CREATE TABLE t1(a,b,c,d,e,f);
///    CREATE INDEX t1x1 ON t1(e,c);
///    SELECT * FROM t1 WHERE (a,b,c,d,e) IN (SELECT v,w,x,y,z FROM t2)
///                           \_______________________________________/
///                                     The pX expression
///
/// Since only columns e and c can be used with the index, in that order,
/// the modified IN expression that is returned will be:
///
///        (e,c) IN (SELECT z,x FROM t2)
///
/// The reduced pX is different from the original (obviously) and thus is
/// only used for indexing, to improve performance.  The original unaltered
/// IN expression must also be run on each output row for correctness.
fn removeUnindexableInClauseTerms(
    db: &mut Sqlite3,
    wi: &WhereInfo,
    iEq: i32,
    pLoop: &WhereLoop,
    pX: ExprId,
) -> ExprId {
    let pNew = sqlite3ExprDup(db, pX, 0);
    if db.mallocFailed == 0 {
        let mut pSelect = db.ex(pNew).x.pSelect;
        while pSelect.nn() {
            let pOrigRhs = db.sel(pSelect).pEList;
            let mut pOrigLhs = ExprListId::NULL;
            let mut pRhs = ExprListId::NULL;
            let mut pLhs = ExprListId::NULL;
            let mut nRhs = 0;

            debug_assert!(db.ex(pNew).UseXSelect());
            let nl = db.ex(pNew).pLeft;
            debug_assert!(nl.nn());
            if pSelect == db.ex(pNew).x.pSelect {
                pOrigLhs = db.ex(nl).x.pList;
            }
            for i in iEq..pLoop.nLTerm as i32 {
                let lt = *wi.t(pLoop.lterm(i as usize).unwrap());
                if lt.pExpr == pX {
                    debug_assert!((lt.eOperator & (WO_OR | WO_AND)) == 0);
                    let iField = (lt.u.x.iField - 1) as usize;
                    if iField as i32 != nRhs {
                        db.exm(pNew).ClearProperty(EP_Subrtn);
                    }
                    let e = db.el(pOrigRhs).a[iField].pExpr;
                    if e.is_null() {
                        continue; // Duplicate PK column
                    }
                    pRhs = sqlite3ExprListAppend(db, pRhs, e);
                    db.elm(pOrigRhs).a[iField].pExpr = ExprId::NULL;
                    if pRhs.nn() {
                        let last = db.el(pRhs).a.len() - 1;
                        db.elm(pRhs).a[last].u.x.iOrderByCol = (iField + 1) as u16;
                    }
                    if pOrigLhs.nn() {
                        let le = db.el(pOrigLhs).a[iField].pExpr;
                        debug_assert!(le.nn());
                        pLhs = sqlite3ExprListAppend(db, pLhs, le);
                        db.elm(pOrigLhs).a[iField].pExpr = ExprId::NULL;
                    }
                    nRhs += 1;
                }
            }
            sqlite3ExprListDelete(db, pOrigRhs);
            if pOrigLhs.nn() {
                sqlite3ExprListDelete(db, pOrigLhs);
                db.exm(nl).x.pList = pLhs;
            }
            db.selm(pSelect).pEList = pRhs;
            db.pp().nSelect += 1;
            let sid = db.ppr().nSelect as u32;
            db.selm(pSelect).selId = sid; // Req'd for SubrtnSig validity
            if pLhs.nn() && db.el(pLhs).nExpr() == 1 {
                // Take care here not to generate a TK_VECTOR containing only a
                // single value. Since the parser never creates such a vector,
                // some of the subroutines do not handle this case.
                let p = db.el(pLhs).a[0].pExpr;
                db.elm(pLhs).a[0].pExpr = ExprId::NULL;
                let ol = db.ex(pNew).pLeft;
                sqlite3ExprDelete(db, ol);
                db.exm(pNew).pLeft = p;
            }

            // If either the ORDER BY clause or the GROUP BY clause contains
            // references to result-set columns, those references might now
            // be obsolete.  So fix them up.
            debug_assert!(pRhs.nn() || db.mallocFailed != 0);
            if pRhs.nn() {
                let (ob, gb) = (db.sel(pSelect).pOrderBy, db.sel(pSelect).pGroupBy);
                adjustOrderByCol(db, ob, pRhs);
                adjustOrderByCol(db, gb, pRhs);
                for it in db.elm(pRhs).a.iter_mut() {
                    it.u.x.iOrderByCol = 0;
                }
            }
            pSelect = db.sel(pSelect).pPrior;
        }
    }
    pNew
}

/// Generate code for a single X IN (....) term of the WHERE clause.
///
/// This is a special-case of codeEqualityTerm() that works for IN operators
/// only.  It is broken out into a subroutine because this case is
/// uncommon and by splitting it off into a subroutine, the common case
/// runs faster.
///
/// The current value for the constraint is left in  register iTarget.
/// This routine sets up a loop that will iterate over all values of X.
fn codeINTerm(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pTerm: TermId,
    iLevel: usize,
    iEq: i32,
    bRev: i32,
    iTarget: i32,
) {
    let pX = wi.t(pTerm).pExpr;
    let mut eType = IN_INDEX_NOOP;
    let mut iTab: i32;
    let mut bRev = bRev;
    let mut nEq = 0;
    let mut aiMap: Option<Vec<i32>> = None;

    {
        let pLoop = wi.lp(iLevel);
        if (pLoop.wsFlags & WHERE_VIRTUALTABLE) == 0 {
            if let Some(ix) = &pLoop.u.btree.pIndex {
                if ix
                    .borrow()
                    .aSortOrder
                    .get(iEq as usize)
                    .copied()
                    .unwrap_or(0)
                    != 0
                {
                    bRev = (bRev == 0) as i32;
                }
            }
        }
    }
    debug_assert!(db.ex(pX).op == TK_IN);

    let pLoop = wi.lp(iLevel).clone();
    for i in 0..iEq {
        if let Some(lt) = pLoop.lterm(i as usize) {
            if wi.t(lt).pExpr == pX {
                disableTerm(db, wi, iLevel, pTerm);
                return;
            }
        }
    }
    for i in iEq..pLoop.nLTerm as i32 {
        let lt = pLoop.lterm(i as usize).unwrap();
        if wi.t(lt).pExpr == pX {
            nEq += 1;
        }
    }

    iTab = 0;
    let (useSel, sel) = (db.ex(pX).UseXSelect(), db.ex(pX).x.pSelect);
    if !useSel || db.el(db.sel(sel).pEList).nExpr() == 1 {
        eType = sqlite3FindInIndex(db, pX, IN_INDEX_LOOP as u32, None, None, &mut iTab);
    } else {
        let pXMod = removeUnindexableInClauseTerms(db, wi, iEq, &pLoop, pX);
        if nEq > 1 {
            // If this IN(SELECT ...) expression drives more than one column
            // of the index, disable the seek-scan optimization. The reasons
            // for this are that (a) it is only possible to make this happen
            // by populating the sqlite_stat1 table with inconsistent
            // information, and (b) it would require sqlite3FindInIndex() to
            // find an index that is not only unique for the columns in
            // question, but also delivers them in sorted order (requires
            // checking asc/desc, and rejecting cases where the indexed
            // columns are not in the right order).
            wi.lpm(iLevel).wsFlags &= !WHERE_IN_SEEKSCAN;
        }
        if db.mallocFailed == 0 {
            let nCol = db.el(db.sel(sel).pEList).nExpr();
            let mut m = vec![0i32; nCol as usize];
            eType = sqlite3FindInIndex(
                db,
                pXMod,
                IN_INDEX_LOOP as u32,
                None,
                Some(&mut m[..]),
                &mut iTab,
            );
            aiMap = Some(m);
        }
        sqlite3ExprDelete(db, pXMod);
    }

    if eType == IN_INDEX_INDEX_DESC {
        bRev = (bRev == 0) as i32;
    }
    sqlite3VdbeAddOp2(db.v(), if bRev != 0 { OP_Last } else { OP_Rewind }, iTab, 0);

    debug_assert!((wi.lp(iLevel).wsFlags & WHERE_MULTI_OR) == 0);
    wi.lpm(iLevel).wsFlags |= WHERE_IN_ABLE;
    if wi.a[iLevel].u.r#in.nIn() == 0 {
        wi.a[iLevel].addrNxt = sqlite3VdbeMakeLabel(db);
    }
    if iEq > 0 && (wi.lp(iLevel).wsFlags & WHERE_IN_SEEKSCAN) == 0 {
        wi.lpm(iLevel).wsFlags |= WHERE_IN_EARLYOUT;
    }

    let mut iMap = 0usize; // Index in aiMap[]
    for i in iEq..pLoop.nLTerm as i32 {
        let lt = pLoop.lterm(i as usize).unwrap();
        if wi.t(lt).pExpr == pX {
            let mut pIn = InLoop::default();
            let iOut = iTarget + i - iEq;
            if eType == IN_INDEX_ROWID {
                pIn.addrInTop = sqlite3VdbeAddOp2(db.v(), OP_Rowid, iTab, iOut);
            } else {
                let iCol = match &aiMap {
                    Some(m) => {
                        let c = m[iMap];
                        iMap += 1;
                        c
                    }
                    None => 0,
                };
                pIn.addrInTop = sqlite3VdbeAddOp3(db.v(), OP_Column, iTab, iCol, iOut);
            }
            sqlite3VdbeAddOp1(db.v(), OP_IsNull, iOut);
            if i == iEq {
                pIn.iCur = iTab;
                pIn.eEndLoopOp = if bRev != 0 { OP_Prev } else { OP_Next };
                if iEq > 0 {
                    pIn.iBase = iTarget - i;
                    pIn.nPrefix = i;
                } else {
                    pIn.nPrefix = 0;
                }
            } else {
                pIn.eEndLoopOp = OP_Noop;
            }
            wi.a[iLevel].u.r#in.aInLoop.push(pIn);
        }
    }
    if iEq > 0 && (wi.lp(iLevel).wsFlags & (WHERE_IN_SEEKSCAN | WHERE_VIRTUALTABLE)) == 0 {
        let ic = wi.a[iLevel].iIdxCur;
        sqlite3VdbeAddOp3(db.v(), OP_SeekHit, ic, 0, iEq);
    }
}

/// Generate code for a single equality term of the WHERE clause.  An
/// equality term can be either X=expr or X IN (...).   pTerm is the term
/// to be coded.
///
/// The current value for the constraint is left in a register, the index
/// of which is returned.  An attempt is made store the result in iTarget
/// but this is only guaranteed for TK_ISNULL and TK_IN constraints.  If the
/// constraint is a TK_EQ or TK_IS, then the current value might be left in
/// some other register and it is the caller's responsibility to compensate.
///
/// For a constraint of the form X=expr, the expression is evaluated in
/// straight-line code.  For constraints of the form X IN (...)
/// this routine sets up a loop that will iterate over all values of X.
fn codeEqualityTerm(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pTerm: TermId,
    iLevel: usize,
    iEq: i32,
    bRev: i32,
    iTarget: i32,
) -> i32 {
    let pX = wi.t(pTerm).pExpr;
    let iReg: i32;

    debug_assert!(wi.lp(iLevel).lterm(iEq as usize) == Some(pTerm));
    debug_assert!(iTarget > 0);
    let op = db.ex(pX).op;
    if op == TK_EQ || op == TK_IS {
        let r = db.ex(pX).pRight;
        iReg = sqlite3ExprCodeTarget(db, r, iTarget);
    } else if op == TK_ISNULL {
        iReg = iTarget;
        sqlite3VdbeAddOp2(db.v(), OP_Null, 0, iReg);
    } else {
        debug_assert!(op == TK_IN);
        iReg = iTarget;
        codeINTerm(db, wi, pTerm, iLevel, iEq, bRev, iTarget);
    }

    // As an optimization, try to disable the WHERE clause term that is
    // driving the index as it will always be true.  The correct answer is
    // obtained regardless, but we might get the answer with fewer CPU
    // cycles by omitting the term.
    //
    // But do not disable the term unless we are certain that the term is
    // not a transitive constraint.  For an example of where that does not
    // work, see https://sqlite.org/forum/forumpost/eb8613976a (2021-05-04)
    if (wi.lp(iLevel).wsFlags & WHERE_TRANSCONS) == 0 || (wi.t(pTerm).eOperator & WO_EQUIV) == 0 {
        disableTerm(db, wi, iLevel, pTerm);
    }

    iReg
}

/// Generate code that will evaluate all == and IN constraints for an
/// index scan.
///
/// For example, consider table t1(a,b,c,d,e,f) with index i1(a,b,c).
/// Suppose the WHERE clause is this:  a==5 AND b IN (1,2,3) AND c>5 AND c<10
/// The index has as many as three equality constraints, but in this
/// example, the third "c" value is an inequality.  So only two
/// constraints are coded.  This routine will generate code to evaluate
/// a==5 and b IN (1,2,3).  The current values for a and b will be stored
/// in consecutive registers and the index of the first register is
/// returned.
///
/// In the example above nEq==2.  But this subroutine works for any value
/// of nEq including 0.  If nEq==0, this routine is nearly a no-op.
/// The only thing it does is allocate the pLevel->iMem memory cell and
/// compute the affinity string.
///
/// The nExtraReg parameter is 0 or 1.  It is 0 if all WHERE clause
/// constraints are == or IN and are covered by the nEq.  nExtraReg is 1 if
/// there is an inequality constraint (such as the "c>=5 AND c<10" in the
/// example) that occurs after the nEq quality constraints.
///
/// This routine allocates a range of nEq+nExtraReg memory cells and
/// returns the index of the first memory cell in that range. The code that
/// calls this routine will use that memory range to store keys for
/// start and termination conditions of the loop.
/// key value of the loop.  If one or more IN operators appear, then
/// this routine allocates an additional nEq memory cells for internal
/// use.
///
/// Before returning, *pzAff is set to point to a buffer containing a
/// copy of the column affinity string of the index allocated using
/// sqlite3DbMalloc(). Except, entries in the copy of the string associated
/// with equality constraints that use BLOB or NONE affinity are set to
/// SQLITE_AFF_BLOB. This is to deal with SQL such as the following:
///
///   CREATE TABLE t1(a TEXT PRIMARY KEY, b);
///   SELECT ... FROM t1 AS t2, t1 WHERE t1.a = t2.b;
///
/// In the example above, the index on t1(a) has TEXT affinity. But since
/// the right hand side of the equality constraint (t2.b) has BLOB/NONE
/// affinity, no conversion should be attempted before using a t2.b value as
/// part of a key to search the index. Hence the first byte in the returned
/// affinity string in this example would be set to SQLITE_AFF_BLOB.
fn codeAllEqualityTerms(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    iLevel: usize,
    bRev: i32,
    nExtraReg: i32,
    pzAff: &mut Option<Vec<u8>>,
) -> i32 {
    // This module is only called on query plans that use an index.
    let pLoop = wi.lp(iLevel).clone();
    debug_assert!((pLoop.wsFlags & WHERE_VIRTUALTABLE) == 0);
    let nEq = pLoop.u.btree.nEq as i32;
    let nSkip = pLoop.nSkip as i32;
    let pIdx = pLoop.u.btree.pIndex.clone().unwrap();

    // Figure out how many memory cells we will need then allocate them.
    let regBase = db.ppr().nMem + 1;
    let nReg = nEq + nExtraReg;
    db.pp().nMem += nReg;

    let mut zAff: Option<Vec<u8>> = crate::insert::sqlite3IndexAffinityStr(db, &pIdx);
    debug_assert!(zAff.is_some() || db.mallocFailed != 0);

    if nSkip != 0 {
        let iIdxCur = wi.a[iLevel].iIdxCur;
        sqlite3VdbeAddOp3(db.v(), OP_Null, 0, regBase, regBase + nSkip - 1);
        sqlite3VdbeAddOp1(db.v(), if bRev != 0 { OP_Last } else { OP_Rewind }, iIdxCur);
        let j = sqlite3VdbeAddOp0(db.v(), OP_Goto);
        debug_assert!(wi.a[iLevel].addrSkip == 0);
        wi.a[iLevel].addrSkip = sqlite3VdbeAddOp4Int(
            db.v(),
            if bRev != 0 { OP_SeekLT } else { OP_SeekGT },
            iIdxCur,
            0,
            regBase,
            nSkip,
        );
        sqlite3VdbeJumpHere(db.v(), j);
        for j in 0..nSkip {
            sqlite3VdbeAddOp3(db.v(), OP_Column, iIdxCur, j, regBase + j);
        }
    }

    // Evaluate the equality constraints
    for j in nSkip..nEq {
        let pTerm = pLoop.lterm(j as usize).unwrap();
        // The following testcase is true for indices with redundant columns.
        // Ex: CREATE INDEX i1 ON t1(a,b,a); SELECT * FROM t1 WHERE a=0 AND b=0;
        let r1 = codeEqualityTerm(db, wi, pTerm, iLevel, j, bRev, regBase + j);
        if r1 != regBase + j {
            sqlite3VdbeAddOp2(db.v(), OP_Copy, r1, regBase + j);
        }
        let t = *wi.t(pTerm);
        if t.eOperator & WO_IN != 0 {
            if db.ex(t.pExpr).flags & EP_xIsSelect != 0 {
                // No affinity ever needs to be (or should be) applied to a
                // value from the RHS of an "? IN (SELECT ...)" expression.
                // The sqlite3FindInIndex() routine has already ensured that
                // the affinity of the comparison has been applied to the
                // value.
                if let Some(z) = zAff.as_mut() {
                    if (j as usize) < z.len() {
                        z[j as usize] = SQLITE_AFF_BLOB;
                    }
                }
            }
        } else if (t.eOperator & WO_ISNULL) == 0 {
            let pRight = db.ex(t.pExpr).pRight;
            if (t.wtFlags & TERM_IS) == 0 && sqlite3ExprCanBeNull(db, pRight) != 0 {
                let brk = wi.a[iLevel].addrBrk;
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, regBase + j, brk);
            }
            if db.ppr().nErr == 0 {
                debug_assert!(db.mallocFailed == 0);
                if let Some(z) = zAff.as_mut() {
                    if (j as usize) < z.len() {
                        if sqlite3CompareAffinity(db, pRight, z[j as usize]) == SQLITE_AFF_BLOB {
                            z[j as usize] = SQLITE_AFF_BLOB;
                        }
                        if sqlite3ExprNeedsNoAffinityChange(db, pRight, z[j as usize]) != 0 {
                            z[j as usize] = SQLITE_AFF_BLOB;
                        }
                    }
                }
            }
        }
    }
    *pzAff = zAff;
    regBase
}

/// If the most recently coded instruction is a constant range constraint
/// (a string literal) that originated from the LIKE optimization, then
/// set P3 and P5 on the OP_String opcode so that the string will be cast
/// to a BLOB at appropriate times.
///
/// The LIKE optimization trys to evaluate "x LIKE 'abc%'" as a range
/// expression: "x>='ABC' AND x<'abd'".  But this requires that the range
/// scan loop run twice, once for strings and a second time for BLOBs.
/// The OP_String opcodes on the second pass convert the upper and lower
/// bound string constants to blobs.  This routine makes the necessary
/// changes to the OP_String opcodes for that to happen.
fn whereLikeOptimizationStringFixup(db: &mut Sqlite3, iLikeRepCntr: u32, pTerm: &WhereTerm) {
    if pTerm.wtFlags & TERM_LIKEOPT != 0 {
        debug_assert!(iLikeRepCntr > 0);
        if let Some(pOp) = sqlite3VdbeGetLastOp(db.v()) {
            pOp.p3 = (iLikeRepCntr >> 1) as i32; // Register holding counter
            pOp.p5 = (iLikeRepCntr & 1) as u16; // ASC or DESC
        }
    }
}

/// Cursor iCur is open on an intkey b-tree (a table). Register iRowid
/// contains a rowid value just read from cursor iIdxCur, open on index
/// pIdx. This function generates code to do a deferred seek of cursor iCur
/// to the rowid stored in register iRowid.
///
/// Normally, this is just:
///
///   OP_DeferredSeek $iCur $iRowid
///
/// Which causes a seek on $iCur to the row with rowid $iRowid.
///
/// However, if the scan currently being coded is a branch of an OR-loop and
/// the statement currently being coded is a SELECT, then additional
/// information is added that might allow OP_Column to omit the seek and
/// instead do its lookup on the index, thus avoiding an expensive seek
/// operation.  To enable this optimization, the P3 of OP_DeferredSeek is
/// set to iIdxCur and P4 is set to an array of integers containing one
/// entry for each column in the table.  For each table column, if the
/// column is the i'th column of the index, then the corresponding array
/// entry is set to (i+1).  If the column does not appear in the index at
/// all, the array entry is set to 0.  The OP_Column opcode can check this
/// array to see if the column it wants is in the index and if it is, it
/// will substitute the index cursor and column number and continue with
/// those new values, rather than seeking the table cursor.
fn codeDeferredSeek(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pIdx: &IndexRef,
    iCur: i32,
    iIdxCur: i32,
) {
    debug_assert!(iIdxCur > 0);

    wi.bDeferredSeek = 1;
    sqlite3VdbeAddOp3(db.v(), OP_DeferredSeek, iIdxCur, 0, iCur);
    if (wi.wctrlFlags & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) != 0
        && db.toplevel().writeMask == 0
    {
        let (aiColumn, nColumn, pTab) = {
            let p = pIdx.borrow();
            (p.aiColumn.clone(), p.nColumn as usize, p.table())
        };
        let nCol = pTab.borrow().nCol as usize;
        let mut ai: Vec<u32> = vec![0; nCol + 1];
        ai[0] = nCol as u32;
        for i in 0..nColumn.saturating_sub(1) {
            let x1 = aiColumn[i];
            let x2 = crate::build::sqlite3TableColumnToStorage(&pTab, x1);
            if x1 >= 0 {
                ai[x2 as usize + 1] = (i + 1) as u32;
            }
        }
        sqlite3VdbeChangeP4(db.v(), -1, P4::Ai(Rc::new(ai)), P4_INTARRAY as i32);
    }
}

/// If the expression passed as the second argument is a vector, generate
/// code to write the first nReg elements of the vector into an array
/// of registers starting with iReg.
///
/// If the expression is not a vector, then nReg must be passed 1. In
/// this case, generate code to evaluate the expression and leave the
/// result in register iReg.
fn codeExprOrVector(db: &mut Sqlite3, p: ExprId, iReg: i32, nReg: i32) {
    debug_assert!(nReg > 0);
    if p.nn() && sqlite3ExprIsVector(db, p) != 0 {
        if db.ex(p).UseXSelect() {
            debug_assert!(db.ex(p).op == TK_SELECT);
            let iSelect = sqlite3CodeSubselect(db, p);
            sqlite3VdbeAddOp3(db.v(), OP_Copy, iSelect, iReg, nReg - 1);
        } else {
            let pList = db.ex(p).x.pList;
            debug_assert!(nReg <= db.el(pList).nExpr());
            for i in 0..nReg {
                let e = db.el(pList).a[i as usize].pExpr;
                sqlite3ExprCode(db, e, iReg + i);
            }
        }
    } else {
        debug_assert!(nReg == 1 || db.ppr().nErr != 0);
        sqlite3ExprCode(db, p, iReg);
    }
}

/// The pTruth expression is always true because it is the WHERE clause
/// a partial index that is driving a query loop.  Look through all of the
/// WHERE clause terms on the query, and if any of those terms must be
/// true because pTruth is true, then mark those WHERE clause terms as
/// coded.
fn whereApplyPartialIndexConstraints(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pTruth: ExprId,
    iTabCur: i32,
    pWC: WcId,
) {
    let mut pTruth = pTruth;
    while db.ex(pTruth).op == TK_AND {
        let l = db.ex(pTruth).pLeft;
        whereApplyPartialIndexConstraints(db, wi, l, iTabCur, pWC);
        pTruth = db.ex(pTruth).pRight;
    }
    let n = wi.wc(pWC).a.len();
    for i in 0..n {
        let t = wi.wc(pWC).a[i];
        if t.wtFlags & TERM_CODED != 0 {
            continue;
        }
        if sqlite3ExprCompare(db, false, t.pExpr, pTruth, iTabCur) == 0 {
            wi.wcm(pWC).a[i].wtFlags |= TERM_CODED;
        }
    }
}

/// This routine is called right after An OP_Filter has been generated and
/// before the corresponding index search has been performed.  This routine
/// checks to see if there are additional Bloom filters in inner loops that
/// can be checked prior to doing the index lookup.  If there are available
/// inner-loop Bloom filters, then evaluate those filters now, before the
/// index lookup.  The idea is that a Bloom filter check is way faster than
/// an index lookup, and the Bloom filter might return false, meaning that
/// the index lookup can be skipped.
///
/// We know that an inner loop uses a Bloom filter because it has the
/// WhereLevel.regFilter set.  If an inner-loop Bloom filter is checked,
/// then clear the WhereLevel.regFilter value to prevent the Bloom filter
/// from being checked a second time when the inner loop is evaluated.
fn filterPullDown(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    iLevel: usize,
    addrNxt: i32,
    notReady: Bitmask,
) {
    let mut iLevel = iLevel;
    loop {
        iLevel += 1;
        if iLevel >= wi.nLevel as usize {
            break;
        }
        if wi.a[iLevel].regFilter == 0 {
            continue;
        }
        let pLoop = wi.lp(iLevel).clone();
        if pLoop.nSkip != 0 {
            continue;
        }
        //         ,--- Because sqlite3ConstructBloomFilter() has will not have set
        //  vvvvv--'    pLevel->regFilter if this were true.
        if pLoop.prereq & notReady != 0 {
            continue;
        }
        let saved_addrBrk = wi.a[iLevel].addrBrk;
        wi.a[iLevel].addrBrk = addrNxt;
        let regFilter = wi.a[iLevel].regFilter;
        if pLoop.wsFlags & WHERE_IPK != 0 {
            let pTerm = pLoop.lterm(0).unwrap();
            let mut regRowid = sqlite3GetTempReg(db);
            regRowid = codeEqualityTerm(db, wi, pTerm, iLevel, 0, 0, regRowid);
            sqlite3VdbeAddOp2(db.v(), OP_MustBeInt, regRowid, addrNxt);
            debug_assert!(sqlite3WhereLoopBloomable(&pLoop) != 0);
            sqlite3VdbeAddOp4Int(db.v(), OP_Filter, regFilter, addrNxt, regRowid, 1);
        } else {
            let nEq = pLoop.u.btree.nEq as i32;
            let mut zStartAff: Option<Vec<u8>> = None;

            debug_assert!(pLoop.wsFlags & WHERE_INDEXED != 0);
            debug_assert!((pLoop.wsFlags & WHERE_COLUMN_IN) == 0);
            let r1 = codeAllEqualityTerms(db, wi, iLevel, 0, 0, &mut zStartAff);
            codeApplyAffinity(db, r1, nEq, zStartAff.as_deref());
            debug_assert!(sqlite3WhereLoopBloomable(&pLoop) != 0);
            sqlite3VdbeAddOp4Int(db.v(), OP_Filter, regFilter, addrNxt, r1, nEq);
        }
        wi.a[iLevel].regFilter = 0;
        wi.a[iLevel].addrBrk = saved_addrBrk;
    }
}

/// Loop pLoop is a WHERE_INDEXED level that uses at least one IN(...)
/// operator. Return true if level pLoop is guaranteed to visit only one
/// row for each key generated for the index.
fn whereLoopIsOneRow(wi: &WhereInfo, pLoop: &WhereLoop) -> i32 {
    let (onError, nKeyCol) = {
        let p = pLoop.u.btree.pIndex.as_ref().unwrap().borrow();
        (p.onError, p.nKeyCol)
    };
    if onError != 0 && pLoop.nSkip == 0 && pLoop.u.btree.nEq == nKeyCol {
        for ii in 0..pLoop.u.btree.nEq as usize {
            if wi.t(pLoop.lterm(ii).unwrap()).eOperator & (WO_IS | WO_ISNULL) != 0 {
                return 0;
            }
        }
        return 1;
    }
    0
}

/// This is called while coding loop pLevel, which scans FROM clause item
/// pTabItem. pE is an expression for which all the prerequisites are
/// available. This function tests if pE can be coded as part of the
/// current loop, or whether it needs to be deferred to ensure outer joins
/// are processed correctly. This function returns non-zero if the
/// expression can be coded as part of the loop, or 0 if it must be
/// deferred.
///
/// The expression must be deferred if:
///
///   * There are any RIGHT joins in the FROM clause and the expression
///     was not part of an ON clause, or was part of an ON clause to the
///     right of the join.
///
///   * The table is the RHS of a LEFT JOIN and the expression was not
///     part of an ON clause on an OUTER join, or was part of an ON clause
///     on an OUTER join to the right of the join.
fn whereExprIsReady(
    db: &Sqlite3,
    wi: &mut WhereInfo,
    iLevel: usize,
    pTabItem: SrcItemId,
    pE: ExprId,
) -> i32 {
    let jtype = db.item(pTabItem).fg.jointype;
    if jtype & (JT_LEFT | JT_LTORJ | JT_RIGHT) != 0 {
        let e = db.ex(pE);
        if !e.HasProperty(EP_OuterON | EP_InnerON) {
            // Defer processing WHERE clause constraints until after outer
            // join processing.  tag-20220513a
            return 0;
        } else if (jtype & JT_LEFT) != 0 && !e.HasProperty(EP_OuterON) {
            return 0;
        } else {
            let iJoin = e.w.iJoin;
            let m = sqlite3WhereGetMask(&mut wi.sMaskSet, iJoin);
            if m & wi.a[iLevel].notReady != 0 {
                // An ON clause that is not ripe
                return 0;
            }
        }
    }
    1
}

/// SMASKBIT32(n): 1<<n if n<=31, else 0
#[inline]
fn SMASKBIT32(n: i32) -> u32 {
    if n <= 31 { 1u32 << n } else { 0 }
}

/// Generate code for the start of the iLevel-th loop in the WHERE clause
/// implementation described by pWInfo.
///
/// C: `Bitmask sqlite3WhereCodeOneLoopStart( Parse *pParse, Vdbe *v, WhereInfo *pWInfo, int iLevel, WhereLevel *pLevel, Bitmask notReady )`
/// (Rust: the Vdbe is db.v(); the level is pWInfo.a[iLevel])
pub fn sqlite3WhereCodeOneLoopStart(
    db: &mut Sqlite3,
    pWInfo: &mut crate::where_::WhereInfo,
    iLevel: i32,
    notReady: Bitmask,
) -> Bitmask {
    let wi = pWInfo;
    let lv = iLevel as usize;
    let addrNxt: i32; // Where to jump to continue with the next IN case
    let iRowidReg: i32; // Rowid is stored in this register, if not zero
    let iReleaseReg: i32; // Temp register to free before returning
    let mut pIdx: Option<IndexRef> = None; // Index used by loop (if any)
    let mut iLoop: i32; // Iteration of constraint generator loop

    let pWC = SWC;
    let pTabItem = SrcItemId(wi.pTabList, wi.a[lv].iFrom as u32);
    let iCur = db.item(pTabItem).iCursor;
    let m = sqlite3WhereGetMask(&mut wi.sMaskSet, iCur);
    wi.a[lv].notReady = notReady & !m;
    let bRev: i32 = ((wi.revMask >> iLevel) & 1) as i32;

    // Create labels for the "break" and "continue" instructions
    // for the current loop.  Jump to addrBrk to break out of a loop.
    // Jump to cont to go immediately to the next iteration of the
    // loop.
    //
    // When there is an IN operator, we also have a "addrNxt" label that
    // means to continue with the next IN value combination.  When
    // there are no IN operators in the constraints, the "addrNxt" label
    // is the same as "addrBrk".
    let addrBrk = wi.a[lv].addrBrk;
    wi.a[lv].addrNxt = addrBrk;
    let addrCont = sqlite3VdbeMakeLabel(db);
    wi.a[lv].addrCont = addrCont;

    let fg = db.item(pTabItem).fg;

    // If this is the right table of a LEFT OUTER JOIN, allocate and
    // initialize a memory cell that records if this table matches any
    // row of the left table of the join.
    debug_assert!(
        (wi.wctrlFlags & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) != 0
            || wi.a[lv].iFrom > 0
            || (fg.jointype & JT_LEFT) == 0
    );
    if wi.a[lv].iFrom > 0 && (fg.jointype & JT_LEFT) != 0 {
        db.pp().nMem += 1;
        let r = db.ppr().nMem;
        wi.a[lv].iLeftJoin = r;
        sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, r);
        if fg.viaCoroutine != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_NullRow, iCur, 1);
        }
    }

    let pLoop0 = wi.lp(lv).clone();
    let wsFlags = pLoop0.wsFlags;

    // Special case of a FROM clause subquery implemented as a co-routine
    if fg.viaCoroutine != 0 {
        debug_assert!(fg.isSubquery != 0 && db.item(pTabItem).u4.pSubq.is_some());
        let pSubq = **db.item(pTabItem).u4.pSubq.as_ref().unwrap();
        let regYield = pSubq.regReturn;
        sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regYield, 0, pSubq.addrFillSub);
        wi.a[lv].p2 = sqlite3VdbeAddOp2(db.v(), OP_Yield, regYield, addrBrk);
        wi.a[lv].op = OP_Goto;
    } else if (wsFlags & WHERE_VIRTUALTABLE) != 0 {
        // Case 1:  The table is a virtual-table.  Use the VFilter and VNext
        //          to access the data.
        let pLoop = pLoop0.clone();
        let nConstraint = pLoop.nLTerm as i32;

        let iReg = sqlite3GetTempRange(db, nConstraint + 2);
        let mut addrNotFound = wi.a[lv].addrBrk;
        for j in 0..nConstraint {
            let iTarget = iReg + j + 2;
            let Some(pTerm) = pLoop.lterm(j as usize) else {
                continue;
            };
            let t = *wi.t(pTerm);
            if t.eOperator & WO_IN != 0 {
                if SMASKBIT32(j) & pLoop.u.vtab.mHandleIn != 0 {
                    let iTab = db.ppr().nTab;
                    db.pp().nTab += 1;
                    db.pp().nMem += 1;
                    let iCache = db.ppr().nMem;
                    sqlite3CodeRhsOfIN(db, t.pExpr, iTab, 0);
                    sqlite3VdbeAddOp3(db.v(), OP_VInitIn, iTab, iTarget, iCache);
                } else {
                    codeEqualityTerm(db, wi, pTerm, lv, j, bRev, iTarget);
                    addrNotFound = wi.a[lv].addrNxt;
                }
            } else {
                let pRight = db.ex(t.pExpr).pRight;
                codeExprOrVector(db, pRight, iTarget, 1);
                if t.eMatchOp as i32 == SQLITE_INDEX_CONSTRAINT_OFFSET
                    && pLoop.u.vtab.bOmitOffset != 0
                {
                    debug_assert!(t.eOperator == WO_AUX);
                    debug_assert!(wi.pSelect.nn());
                    let off = db.sel(wi.pSelect).iOffset;
                    debug_assert!(off > 0);
                    sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, off);
                }
            }
        }
        sqlite3VdbeAddOp2(db.v(), OP_Integer, pLoop.u.vtab.idxNum, iReg);
        sqlite3VdbeAddOp2(db.v(), OP_Integer, nConstraint, iReg + 1);
        // The instruction immediately prior to OP_VFilter must be an
        // OP_Integer that sets the "argc" value for xVFilter.  This is
        // necessary for resolveP2() to work correctly.  See tag-20250207a.
        let (p4, p4t) = match &pLoop.u.vtab.idxStr {
            Some(z) => (
                P4::Z(z.clone()),
                if pLoop.u.vtab.needFree != 0 {
                    P4_DYNAMIC
                } else {
                    P4_STATIC
                },
            ),
            None => (P4::None, P4_STATIC),
        };
        sqlite3VdbeAddOp4(db.v(), OP_VFilter, iCur, addrNotFound, iReg, p4, p4t as i32);
        wi.lpm(lv).u.vtab.needFree = 0;
        wi.a[lv].p1 = iCur;
        wi.a[lv].op = if wi.eOnePass != 0 { OP_Noop } else { OP_VNext };
        wi.a[lv].p2 = sqlite3VdbeCurrentAddr(db.v());
        debug_assert!((pLoop.wsFlags & WHERE_MULTI_OR) == 0);

        for j in 0..nConstraint {
            let pTerm = pLoop.lterm(j as usize).unwrap();
            if j < 16 && ((pLoop.u.vtab.omitMask >> j) & 1) != 0 {
                disableTerm(db, wi, lv, pTerm);
                continue;
            }
            let t = *wi.t(pTerm);
            if (t.eOperator & WO_IN) != 0
                && (SMASKBIT32(j) & pLoop.u.vtab.mHandleIn) == 0
                && db.mallocFailed == 0
            {
                // Reload the constraint value into reg[iReg+j+2].  The same
                // value was loaded into the same register prior to the
                // OP_VFilter, but the xFilter implementation might have
                // changed the datatype or encoding of the value in the
                // register, so it *must* be reloaded.
                let nIn = wi.a[lv].u.r#in.aInLoop.len();
                for iIn in 0..nIn {
                    let top = wi.a[lv].u.r#in.aInLoop[iIn].addrInTop;
                    let (opc, p1, p2, p3) = {
                        let o = sqlite3VdbeGetOp(db.v(), top);
                        (o.opcode, o.p1, o.p2, o.p3)
                    };
                    if (opc == OP_Column && p3 == iReg + j + 2)
                        || (opc == OP_Rowid && p2 == iReg + j + 2)
                    {
                        sqlite3VdbeAddOp3(db.v(), opc, p1, p2, p3);
                        break;
                    }
                }

                // Generate code that will continue to the next row if
                // the IN constraint is not satisfied
                let pCompare = sqlite3PExpr(db, TK_EQ as i32, ExprId::NULL, ExprId::NULL);
                if db.mallocFailed == 0 {
                    let iFld = t.u.x.iField;
                    let pLeft = db.ex(t.pExpr).pLeft;
                    debug_assert!(pLeft.nn());
                    if iFld > 0 {
                        debug_assert!(db.ex(pLeft).op == TK_VECTOR);
                        let l = db.ex(pLeft).x.pList;
                        let e = db.el(l).a[(iFld - 1) as usize].pExpr;
                        db.exm(pCompare).pLeft = e;
                    } else {
                        db.exm(pCompare).pLeft = pLeft;
                    }
                    let pRight = sqlite3Expr(db, TK_REGISTER as i32, None);
                    db.exm(pCompare).pRight = pRight;
                    if pRight.nn() {
                        db.exm(pRight).iTable = iReg + j + 2;
                        let ac = wi.a[lv].addrCont;
                        sqlite3ExprIfFalse(db, pCompare, ac, SQLITE_JUMPIFNULL as i32);
                    }
                    db.exm(pCompare).pLeft = ExprId::NULL;
                }
                sqlite3ExprDelete(db, pCompare);
            }
        }

        // These registers need to be preserved in case there is an IN
        // operator loop.  So we could deallocate the registers here (and
        // potentially reuse them later) if (pLoop->wsFlags & WHERE_IN_ABLE)==0.
        // But it seems simpler and safer to simply not reuse the registers.
        //
        //    sqlite3ReleaseTempRange(pParse, iReg, nConstraint+2);
    } else if (wsFlags & WHERE_IPK) != 0 && (wsFlags & (WHERE_COLUMN_IN | WHERE_COLUMN_EQ)) != 0 {
        // Case 2:  We can directly reference a single row using an
        //          equality comparison against the ROWID field.  Or
        //          we reference multiple rows using a "rowid IN (...)"
        //          construct.
        debug_assert!(pLoop0.u.btree.nEq == 1);
        let pTerm = pLoop0.lterm(0).unwrap();
        db.pp().nMem += 1;
        iReleaseReg = db.ppr().nMem;
        iRowidReg = codeEqualityTerm(db, wi, pTerm, lv, 0, bRev, iReleaseReg);
        if iRowidReg != iReleaseReg {
            sqlite3ReleaseTempReg(db, iReleaseReg);
        }
        addrNxt = wi.a[lv].addrNxt;
        if wi.a[lv].regFilter != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_MustBeInt, iRowidReg, addrNxt);
            let rf = wi.a[lv].regFilter;
            sqlite3VdbeAddOp4Int(db.v(), OP_Filter, rf, addrNxt, iRowidReg, 1);
            filterPullDown(db, wi, lv, addrNxt, notReady);
        }
        sqlite3VdbeAddOp3(db.v(), OP_SeekRowid, iCur, addrNxt, iRowidReg);
        wi.a[lv].op = OP_Noop;
    } else if (wsFlags & WHERE_IPK) != 0 && (wsFlags & WHERE_COLUMN_RANGE) != 0 {
        // Case 3:  We have an inequality comparison against the ROWID field.
        let mut testOp = OP_Noop;
        let mut memEndValue = 0;
        let mut pStart: Option<TermId> = None;
        let mut pEnd: Option<TermId> = None;

        let mut j = 0usize;
        if wsFlags & WHERE_BTM_LIMIT != 0 {
            pStart = pLoop0.lterm(j);
            j += 1;
        }
        if wsFlags & WHERE_TOP_LIMIT != 0 {
            pEnd = pLoop0.lterm(j);
        }
        debug_assert!(pStart.is_some() || pEnd.is_some());
        if bRev != 0 {
            std::mem::swap(&mut pStart, &mut pEnd);
        }
        // codeCursorHint(pTabItem, pWInfo, pLevel, pEnd): SQLITE_ENABLE_CURSOR_HINTS is off
        if let Some(pStart) = pStart {
            let r1: i32;
            let mut rTemp: i32 = 0;
            let op: u8;

            // The following constant maps TK_xx codes into corresponding
            // seek opcodes.  It depends on a particular ordering of TK_xx
            const aMoveOp: [u8; 4] = [
                /* TK_GT */ OP_SeekGT, /* TK_LE */ OP_SeekLE, /* TK_LT */ OP_SeekLT,
                /* TK_GE */ OP_SeekGE,
            ];

            let st = *wi.t(pStart);
            debug_assert!((st.wtFlags & TERM_VNULL) == 0);
            let pX = st.pExpr;
            debug_assert!(pX.nn());
            let (xop, xr) = (db.ex(pX).op, db.ex(pX).pRight);
            if sqlite3ExprIsVector(db, xr) != 0 {
                rTemp = sqlite3GetTempReg(db);
                r1 = rTemp;
                codeExprOrVector(db, xr, r1, 1);
                op = aMoveOp[((((xop - TK_GT) as i32 - 1) & 0x3) | 0x1) as usize];
            } else {
                r1 = sqlite3ExprCodeTemp(db, xr, &mut rTemp);
                disableTerm(db, wi, lv, pStart);
                op = aMoveOp[(xop - TK_GT) as usize];
            }
            sqlite3VdbeAddOp3(db.v(), op, iCur, addrBrk, r1);
            sqlite3ReleaseTempReg(db, rTemp);
        } else {
            let h = wi.a[lv].addrHalt;
            sqlite3VdbeAddOp2(db.v(), if bRev != 0 { OP_Last } else { OP_Rewind }, iCur, h);
        }
        if let Some(pEnd) = pEnd {
            let et = *wi.t(pEnd);
            let pX = et.pExpr;
            debug_assert!(pX.nn());
            debug_assert!((et.wtFlags & TERM_VNULL) == 0);
            db.pp().nMem += 1;
            memEndValue = db.ppr().nMem;
            let (xop, xr) = (db.ex(pX).op, db.ex(pX).pRight);
            codeExprOrVector(db, xr, memEndValue, 1);
            if 0 == sqlite3ExprIsVector(db, xr) && (xop == TK_LT || xop == TK_GT) {
                testOp = if bRev != 0 { OP_Le } else { OP_Ge };
            } else {
                testOp = if bRev != 0 { OP_Lt } else { OP_Gt };
            }
            if 0 == sqlite3ExprIsVector(db, xr) {
                disableTerm(db, wi, lv, pEnd);
            }
        }
        let start = sqlite3VdbeCurrentAddr(db.v());
        wi.a[lv].op = if bRev != 0 { OP_Prev } else { OP_Next };
        wi.a[lv].p1 = iCur;
        wi.a[lv].p2 = start;
        debug_assert!(wi.a[lv].p5 == 0);
        if testOp != OP_Noop {
            db.pp().nMem += 1;
            iRowidReg = db.ppr().nMem;
            sqlite3VdbeAddOp2(db.v(), OP_Rowid, iCur, iRowidReg);
            sqlite3VdbeAddOp3(db.v(), testOp, memEndValue, addrBrk, iRowidReg);
            sqlite3VdbeChangeP5(db.v(), SQLITE_AFF_NUMERIC as u16 | SQLITE_JUMPIFNULL);
        }
    } else if wsFlags & WHERE_INDEXED != 0 {
        // Case 4: Search using an index.
        //
        // The WHERE clause may contain zero or more equality
        // terms ("==" or "IN" or "IS" operators) that refer to the N
        // left-most columns of the index. It may also contain
        // inequality constraints (>, <, >= or <=) on the indexed
        // column that immediately follows the N equalities. Only
        // the right-most column can be an inequality - the rest must
        // use the "==", "IN", or "IS" operators. For example, if the
        // index is on (x,y,z), then the following clauses are all
        // optimized:
        //
        //    x=5
        //    x=5 AND y=10
        //    x=5 AND y<10
        //    x=5 AND y>5 AND y<10
        //    x=5 AND y=5 AND z<=10
        //
        // The z<10 term of the following cannot be used, only
        // the x=5 term:
        //
        //    x=5 AND z<10
        //
        // N may be zero if there are inequality constraints.
        // If there are no inequality constraints, then N is at
        // least one.
        //
        // This case is also used when there are no WHERE clause
        // constraints but an index is selected anyway, in order
        // to force the output order to conform to an ORDER BY.
        const aStartOp: [u8; 8] = [
            0, 0, OP_Rewind, // 2: (!start_constraints && startEq &&  !bRev)
            OP_Last,   // 3: (!start_constraints && startEq &&   bRev)
            OP_SeekGT, // 4: (start_constraints  && !startEq && !bRev)
            OP_SeekLT, // 5: (start_constraints  && !startEq &&  bRev)
            OP_SeekGE, // 6: (start_constraints  &&  startEq && !bRev)
            OP_SeekLE, // 7: (start_constraints  &&  startEq &&  bRev)
        ];
        const aEndOp: [u8; 4] = [
            OP_IdxGE, // 0: (end_constraints && !bRev && !endEq)
            OP_IdxGT, // 1: (end_constraints && !bRev &&  endEq)
            OP_IdxLE, // 2: (end_constraints &&  bRev && !endEq)
            OP_IdxLT, // 3: (end_constraints &&  bRev &&  endEq)
        ];
        let pLoop = pLoop0.clone();
        let nEq = pLoop.u.btree.nEq as i32; // Number of == or IN terms
        let mut nBtm = pLoop.u.btree.nBtm as i32; // Length of BTM vector
        let mut nTop = pLoop.u.btree.nTop as i32; // Length of TOP vector
        let regBase: i32; // Base register holding constraint values
        let mut pRangeStart: Option<TermId> = None; // Inequality constraint at range start
        let mut pRangeEnd: Option<TermId> = None; // Inequality constraint at range end
        let mut startEq: i32; // True if range start uses ==, >= or <=
        let mut endEq: i32; // True if range end uses ==, >= or <=
        let mut start_constraints: i32; // Start of range is constrained
        let mut nConstraint: i32; // Number of constraint terms
        let mut nExtraReg = 0; // Number of extra registers needed
        let mut op: u8; // Instruction opcode
        let mut zStartAff: Option<Vec<u8>> = None; // Affinity for start of range constraint
        let mut zEndAff: Option<Vec<u8>> = None; // Affinity for end of range constraint
        let mut bSeekPastNull: u8 = 0; // True to seek past initial nulls
        let mut bStopAtNull: u8 = 0; // Add condition to terminate at NULLs
        let omitTable: bool; // True if we use the index only
        let mut regBignull = 0; // big-null flag register
        let mut addrSeekScan = 0; // Opcode of the OP_SeekScan, if any

        let pIx = pLoop.u.btree.pIndex.clone().unwrap();
        pIdx = Some(pIx.clone());
        let iIdxCur = wi.a[lv].iIdxCur;
        debug_assert!(nEq >= pLoop.nSkip as i32);
        let (aSortOrder, aiColumn, nColumn, aiRowLogEst0, pIdxTab) = {
            let p = pIx.borrow();
            (
                p.aSortOrder.clone(),
                p.aiColumn.clone(),
                p.nColumn as i32,
                p.aiRowLogEst[0],
                p.table(),
            )
        };

        // Find any inequality constraint terms for the start and end
        // of the range.
        let mut j = nEq as usize;
        if pLoop.wsFlags & WHERE_BTM_LIMIT != 0 {
            pRangeStart = pLoop.lterm(j);
            j += 1;
            nExtraReg = nExtraReg.max(pLoop.u.btree.nBtm as i32);
            // Like optimization range constraints always occur in pairs
            debug_assert!(
                (wi.t(pRangeStart.unwrap()).wtFlags & TERM_LIKEOPT) == 0
                    || (pLoop.wsFlags & WHERE_TOP_LIMIT) != 0
            );
        }
        if pLoop.wsFlags & WHERE_TOP_LIMIT != 0 {
            pRangeEnd = pLoop.lterm(j);
            nExtraReg = nExtraReg.max(pLoop.u.btree.nTop as i32);
            if (wi.t(pRangeEnd.unwrap()).wtFlags & TERM_LIKEOPT) != 0 {
                debug_assert!(pRangeStart.is_some()); // LIKE opt constraints
                debug_assert!(wi.t(pRangeStart.unwrap()).wtFlags & TERM_LIKEOPT != 0); // occur in pairs
                db.pp().nMem += 1;
                let cntr = db.ppr().nMem as u32;
                wi.a[lv].iLikeRepCntr = cntr;
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, cntr as i32);
                wi.a[lv].addrLikeRep = sqlite3VdbeCurrentAddr(db.v());
                // iLikeRepCntr actually stores 2x the counter register number.
                // The bottom bit indicates whether the search order is ASC or
                // DESC.
                debug_assert!((bRev & !1) == 0);
                wi.a[lv].iLikeRepCntr <<= 1;
                wi.a[lv].iLikeRepCntr |=
                    (bRev ^ (aSortOrder[nEq as usize] as i8 == SQLITE_SO_DESC) as i32) as u32;
            }
            if pRangeStart.is_none() {
                let jc = aiColumn[nEq as usize];
                if (jc >= 0 && pIdxTab.borrow().aCol[jc as usize].notNull == 0) || jc == XN_EXPR {
                    bSeekPastNull = 1;
                }
            }
        }
        debug_assert!(pRangeEnd.map_or(true, |t| (wi.t(t).wtFlags & TERM_VNULL) == 0));

        // If the WHERE_BIGNULL_SORT flag is set, then index column nEq uses
        // a non-default "big-null" sort (either ASC NULLS LAST or DESC NULLS
        // FIRST). In both cases separate ordered scans are made of those
        // index entries for which the column is null and for those for which
        // it is not. For an ASC sort, the non-NULL entries are scanned first.
        // For DESC, NULL entries are scanned first.
        if (pLoop.wsFlags & (WHERE_TOP_LIMIT | WHERE_BTM_LIMIT)) == 0
            && (pLoop.wsFlags & WHERE_BIGNULL_SORT) != 0
        {
            debug_assert!(bSeekPastNull == 0 && nExtraReg == 0 && nBtm == 0 && nTop == 0);
            debug_assert!(pRangeEnd.is_none() && pRangeStart.is_none());
            nExtraReg = 1;
            bSeekPastNull = 1;
            db.pp().nMem += 1;
            regBignull = db.ppr().nMem;
            wi.a[lv].regBignull = regBignull;
            if wi.a[lv].iLeftJoin != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regBignull);
            }
            wi.a[lv].addrBignull = sqlite3VdbeMakeLabel(db);
        }

        // If we are doing a reverse order scan on an ascending index, or
        // a forward order scan on a descending index, interchange the
        // start and end terms (pRangeStart and pRangeEnd).
        if nEq < nColumn && bRev == (aSortOrder[nEq as usize] as i8 == SQLITE_SO_ASC) as i32 {
            std::mem::swap(&mut pRangeEnd, &mut pRangeStart);
            std::mem::swap(&mut bSeekPastNull, &mut bStopAtNull);
            std::mem::swap(&mut nBtm, &mut nTop);
        }

        if iLevel > 0 && (pLoop.wsFlags & WHERE_IN_SEEKSCAN) != 0 {
            // In case OP_SeekScan is used, ensure that the index cursor does
            // not point to a valid row for the first iteration of this loop.
            sqlite3VdbeAddOp1(db.v(), OP_NullRow, iIdxCur);
        }

        // Generate code to evaluate all constraint terms using == or IN
        // and store the values of those terms in an array of registers
        // starting at regBase.
        // codeCursorHint(pTabItem, pWInfo, pLevel, pRangeEnd): no-op
        regBase = codeAllEqualityTerms(db, wi, lv, bRev, nExtraReg, &mut zStartAff);
        if let Some(z) = &zStartAff {
            if nTop != 0 {
                zEndAff = Some(z.get(nEq as usize..).map_or(Vec::new(), |s| s.to_vec()));
            }
        }
        addrNxt = if regBignull != 0 {
            wi.a[lv].addrBignull
        } else {
            wi.a[lv].addrNxt
        };

        startEq = match pRangeStart {
            None => 1,
            Some(t) => ((wi.t(t).eOperator & (WO_LE | WO_GE)) != 0) as i32,
        };
        endEq = match pRangeEnd {
            None => 1,
            Some(t) => ((wi.t(t).eOperator & (WO_LE | WO_GE)) != 0) as i32,
        };
        start_constraints = (pRangeStart.is_some() || nEq > 0) as i32;

        // Seek the index cursor to the start of the range.
        nConstraint = nEq;
        if let Some(rs) = pRangeStart {
            let st = *wi.t(rs);
            let pRight = db.ex(st.pExpr).pRight;
            codeExprOrVector(db, pRight, regBase + nEq, nBtm);
            let cntr = wi.a[lv].iLikeRepCntr;
            whereLikeOptimizationStringFixup(db, cntr, &st);
            if (st.wtFlags & TERM_VNULL) == 0 && sqlite3ExprCanBeNull(db, pRight) != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, regBase + nEq, addrNxt);
            }
            if let Some(z) = zStartAff.as_mut() {
                if (nEq as usize) <= z.len() {
                    updateRangeAffinityStr(db, pRight, nBtm, &mut z[nEq as usize..]);
                }
            }
            nConstraint += nBtm;
            if sqlite3ExprIsVector(db, pRight) == 0 {
                disableTerm(db, wi, lv, rs);
            } else {
                startEq = 1;
            }
            bSeekPastNull = 0;
        } else if bSeekPastNull != 0 {
            startEq = 0;
            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regBase + nEq);
            start_constraints = 1;
            nConstraint += 1;
        } else if regBignull != 0 {
            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regBase + nEq);
            start_constraints = 1;
            nConstraint += 1;
        }
        codeApplyAffinity(
            db,
            regBase,
            nConstraint - bSeekPastNull as i32,
            zStartAff.as_deref(),
        );
        if pLoop.nSkip > 0 && nConstraint == pLoop.nSkip as i32 {
            // The skip-scan logic inside the call to
            // codeAllEqualityConstraints() above has already left the cursor
            // sitting on the correct row, so no further seeking is needed
        } else {
            if regBignull != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, regBignull);
            }
            if wi.a[lv].regFilter != 0 {
                debug_assert!(sqlite3WhereLoopBloomable(&pLoop) != 0);
                let rf = wi.a[lv].regFilter;
                sqlite3VdbeAddOp4Int(db.v(), OP_Filter, rf, addrNxt, regBase, nEq);
                filterPullDown(db, wi, lv, addrNxt, notReady);
            }

            op = aStartOp[((start_constraints << 2) + (startEq << 1) + bRev) as usize];
            debug_assert!(op != 0);
            if (wi.lp(lv).wsFlags & WHERE_IN_SEEKSCAN) != 0 && op == OP_SeekGE {
                debug_assert!(regBignull == 0);
                // TUNING:  The OP_SeekScan opcode seeks to reduce the number
                // of expensive seek operations by replacing a single seek with
                // 1 or more step operations.  The question is, how many steps
                // should we try before giving up and going with a seek.  The
                // cost of a seek is proportional to the logarithm of the of
                // the number of entries in the tree, so basing the number of
                // steps to try on the estimated number of rows in the btree
                // seems like a good guess.
                addrSeekScan =
                    sqlite3VdbeAddOp1(db.v(), OP_SeekScan, (aiRowLogEst0 as i32 + 9) / 10);
                if pRangeStart.is_some() || pRangeEnd.is_some() {
                    sqlite3VdbeChangeP5(db.v(), 1);
                    let a = sqlite3VdbeCurrentAddr(db.v()) + 1;
                    sqlite3VdbeChangeP2(db.v(), addrSeekScan, a);
                    addrSeekScan = 0;
                }
            }
            sqlite3VdbeAddOp4Int(db.v(), op, iIdxCur, addrNxt, regBase, nConstraint);

            debug_assert!(bSeekPastNull == 0 || bStopAtNull == 0);
            if regBignull != 0 {
                debug_assert!(bSeekPastNull == 1 || bStopAtNull == 1);
                debug_assert!(bStopAtNull as i32 == startEq);
                let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
                sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, a);
                op = aStartOp[((nConstraint > 1) as i32 * 4 + 2 + bRev) as usize];
                sqlite3VdbeAddOp4Int(db.v(), op, iIdxCur, addrNxt, regBase, nConstraint - startEq);
            }
        }

        // Load the value for the inequality constraint at the end of the
        // range (if any).
        nConstraint = nEq;
        debug_assert!(wi.a[lv].p2 == 0);
        if let Some(re) = pRangeEnd {
            let et = *wi.t(re);
            let pRight = db.ex(et.pExpr).pRight;
            debug_assert!(addrSeekScan == 0);
            codeExprOrVector(db, pRight, regBase + nEq, nTop);
            let cntr = wi.a[lv].iLikeRepCntr;
            whereLikeOptimizationStringFixup(db, cntr, &et);
            if (et.wtFlags & TERM_VNULL) == 0 && sqlite3ExprCanBeNull(db, pRight) != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, regBase + nEq, addrNxt);
            }
            if let Some(z) = zEndAff.as_mut() {
                updateRangeAffinityStr(db, pRight, nTop, &mut z[..]);
                codeApplyAffinity(db, regBase + nEq, nTop, Some(&z[..]));
            } else {
                debug_assert!(db.mallocFailed != 0);
            }
            nConstraint += nTop;

            if sqlite3ExprIsVector(db, pRight) == 0 {
                disableTerm(db, wi, lv, re);
            } else {
                endEq = 1;
            }
        } else if bStopAtNull != 0 {
            if regBignull == 0 {
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regBase + nEq);
                endEq = 0;
            }
            nConstraint += 1;
        }

        // Top of the loop body
        wi.a[lv].p2 = sqlite3VdbeCurrentAddr(db.v());

        // Check if the index cursor is past the end of the range.
        if nConstraint != 0 {
            if regBignull != 0 {
                // Except, skip the end-of-range check while doing the
                // NULL-scan
                let a = sqlite3VdbeCurrentAddr(db.v()) + 3;
                sqlite3VdbeAddOp2(db.v(), OP_IfNot, regBignull, a);
            }
            op = aEndOp[(bRev * 2 + endEq) as usize];
            sqlite3VdbeAddOp4Int(db.v(), op, iIdxCur, addrNxt, regBase, nConstraint);
            if addrSeekScan != 0 {
                sqlite3VdbeJumpHere(db.v(), addrSeekScan);
            }
        }
        if regBignull != 0 {
            // During a NULL-scan, check to see if we have reached the end of
            // the NULLs
            debug_assert!(bSeekPastNull == (bStopAtNull == 0) as u8);
            debug_assert!(bSeekPastNull + bStopAtNull == 1);
            debug_assert!(nConstraint + bSeekPastNull as i32 > 0);
            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
            sqlite3VdbeAddOp2(db.v(), OP_If, regBignull, a);
            op = aEndOp[(bRev * 2 + bSeekPastNull as i32) as usize];
            sqlite3VdbeAddOp4Int(
                db.v(),
                op,
                iIdxCur,
                addrNxt,
                regBase,
                nConstraint + bSeekPastNull as i32,
            );
        }

        if (wi.lp(lv).wsFlags & WHERE_IN_EARLYOUT) != 0 {
            sqlite3VdbeAddOp3(db.v(), OP_SeekHit, iIdxCur, nEq, nEq);
        }

        // Seek the table cursor, if required
        omitTable = (wi.lp(lv).wsFlags & WHERE_IDX_ONLY) != 0
            && (wi.wctrlFlags & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) == 0;
        if omitTable {
            // pIdx is a covering index.  No need to access the main table.
        } else if pIdxTab.borrow().HasRowid() {
            codeDeferredSeek(db, wi, &pIx, iCur, iIdxCur);
        } else if iCur != iIdxCur {
            let pPk = crate::build::sqlite3PrimaryKeyIndex(&pIdxTab).unwrap();
            let (nk, pkCols) = {
                let p = pPk.borrow();
                (p.nKeyCol as i32, p.aiColumn.clone())
            };
            iRowidReg = sqlite3GetTempRange(db, nk);
            for j in 0..nk {
                let k = crate::build::sqlite3TableColumnToIndex(&pIx, pkCols[j as usize] as i32);
                sqlite3VdbeAddOp3(db.v(), OP_Column, iIdxCur, k, iRowidReg + j);
            }
            sqlite3VdbeAddOp4Int(db.v(), OP_NotFound, iCur, addrCont, iRowidReg, nk);
        }

        if wi.a[lv].iLeftJoin == 0 {
            // If a partial index is driving the loop, try to eliminate WHERE
            // clause terms from the query that must be true due to the WHERE
            // clause of the partial index.  This optimization does not work
            // on an outer join, as shown by:
            //
            // 2019-11-02 ticket 623eff57e76d45f6      (LEFT JOIN)
            // 2025-05-29 forum post 7dee41d32506c4ae  (RIGHT JOIN)
            if pIx.borrow().pPartIdxWhere.is_some() && wi.a[lv].pRJ.is_none() {
                let pw = idx_part_where(db, wi, &pIx);
                whereApplyPartialIndexConstraints(db, wi, pw, iCur, pWC);
            }
        } else {
            // The following assert() is not a requirement, merely an
            // observation: The OR-optimization doesn't work for the right
            // hand table of a LEFT JOIN:
            debug_assert!((wi.wctrlFlags & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) == 0);
        }

        // Record the instruction used to terminate the loop.
        let lpNow = wi.lp(lv).clone();
        if (lpNow.wsFlags & WHERE_ONEROW) != 0
            || (wi.a[lv].u.r#in.nIn() != 0 && regBignull == 0 && whereLoopIsOneRow(wi, &lpNow) != 0)
        {
            wi.a[lv].op = OP_Noop;
        } else if bRev != 0 {
            wi.a[lv].op = OP_Prev;
        } else {
            wi.a[lv].op = OP_Next;
        }
        wi.a[lv].p1 = iIdxCur;
        wi.a[lv].p3 = if (lpNow.wsFlags & WHERE_UNQ_WANTED) != 0 {
            1
        } else {
            0
        };
        if (lpNow.wsFlags & WHERE_CONSTRAINT) == 0 {
            wi.a[lv].p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        } else {
            debug_assert!(wi.a[lv].p5 == 0);
        }
        if omitTable {
            pIdx = None;
        }
    } else if wsFlags & WHERE_MULTI_OR != 0 {
        // Case 5:  Two or more separately indexed terms connected by OR
        //
        // Example:
        //
        //   CREATE TABLE t1(a,b,c,d);
        //   CREATE INDEX i1 ON t1(a);
        //   CREATE INDEX i2 ON t1(b);
        //   CREATE INDEX i3 ON t1(c);
        //
        //   SELECT * FROM t1 WHERE a=5 OR b=7 OR (c=11 AND d=13)
        //
        // In the example, there are three indexed terms connected by OR.
        // The top of the loop looks like this:
        //
        //          Null       1                # Zero the rowset in reg 1
        //
        // Then, for each indexed term, the following. The arguments to
        // RowSetTest are such that the rowid of the current row is inserted
        // into the RowSet. If it is already present, control skips the
        // Gosub opcode and jumps straight to the code generated by
        // WhereEnd().
        //
        //        sqlite3WhereBegin(<term>)
        //          RowSetTest                  # Insert rowid into rowset
        //          Gosub      2 A
        //        sqlite3WhereEnd()
        //
        // Following the above, code to terminate the loop. Label A, the
        // target of the Gosub above, jumps to the instruction right after
        // the Goto.
        //
        //          Null       1                # Zero the rowset in reg 1
        //          Goto       B                # The loop is finished.
        //
        //       A: <loop body>                 # Return data, whatever.
        //
        //          Return     2                # Jump back to the Gosub
        //
        //       B: <after the loop>
        //
        // Added 2014-05-26: If the table is a WITHOUT ROWID table, then
        // use an ephemeral index instead of a RowSet to record the primary
        // keys of the rows we have already seen.
        let pLoop = pLoop0.clone();
        let mut pCov: Option<IndexRef> = None; // Potential covering index (or NULL)
        let iCovCur = db.ppr().nTab; // Cursor used for index scans (if any)
        db.pp().nTab += 1;

        db.pp().nMem += 1;
        let regReturn = db.ppr().nMem; // Register used with OP_Gosub
        let mut regRowset = 0; // Register for RowSet object
        let mut regRowid = 0; // Register holding rowid
        let iLoopBody = sqlite3VdbeMakeLabel(db); // Start of loop body
        let mut untestedTerms = false; // Some terms not completely tested
        let mut pAndExpr = ExprId::NULL; // An ".. AND (...)" expression
        let pTab = item_tab(db, pTabItem);

        let pTerm = pLoop.lterm(0).unwrap();
        debug_assert!(wi.t(pTerm).eOperator & WO_OR != 0);
        debug_assert!((wi.t(pTerm).wtFlags & TERM_ORINFO) != 0);
        let pOrWc = wi.orinfo(pTerm).wc;
        wi.a[lv].op = OP_Return;
        wi.a[lv].p1 = regReturn;

        // Set up a new SrcList in pOrTab containing the table being scanned
        // by this loop in the a[0] slot and all notReady tables in a[1..]
        // slots.  This becomes the SrcList in the recursive call to
        // sqlite3WhereBegin().
        let pOrTab: SrcListId;
        if wi.nLevel > 1 || fg.fromExists != 0 {
            let nNotReady = wi.nLevel as usize - lv - 1; // The number of notReady tables
            let mut items: Vec<SrcItem> = Vec::with_capacity(nNotReady + 1);
            items.push(db.item(pTabItem).clone());
            for k in 1..=nNotReady {
                let it = db
                    .item(SrcItemId(wi.pTabList, wi.a[lv + k].iFrom as u32))
                    .clone();
                items.push(it);
            }

            // Clear the fromExists flag on the OR-optimized table entry so
            // that the calls to sqlite3WhereEnd() do not code early-exits
            // after the first row is visited. The early exit applies to this
            // table's overall loop - including the multiple OR branches and
            // any WHERE conditions not passed to the sub-loops - not to the
            // sub-loops.
            items[0].fg.fromExists = 0;
            pOrTab = db.arena.new_srclist(SrcList { a: items });
        } else {
            pOrTab = wi.pTabList;
        }

        // Initialize the rowset register to contain NULL. An SQL NULL is
        // equivalent to an empty rowset.  Or, create an ephemeral index
        // capable of holding primary keys in the case of a WITHOUT ROWID.
        //
        // Also initialize regReturn to contain the address of the
        // instruction immediately following the OP_Return at the bottom of
        // the loop. This is required in a few obscure LEFT JOIN cases where
        // control jumps over the top of the loop into the body of it. In
        // this case the correct response for the end-of-loop code (the
        // OP_Return) is to fall through to the next instruction, just as an
        // OP_Next does if called on an uninitialized cursor.
        if (wi.wctrlFlags & WHERE_DUPLICATES_OK) == 0 {
            if pTab.borrow().HasRowid() {
                db.pp().nMem += 1;
                regRowset = db.ppr().nMem;
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, regRowset);
            } else {
                let pPk = crate::build::sqlite3PrimaryKeyIndex(&pTab).unwrap();
                regRowset = db.ppr().nTab;
                db.pp().nTab += 1;
                let nk = pPk.borrow().nKeyCol as i32;
                sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, regRowset, nk);
                sqlite3VdbeSetP4KeyInfo(db, &pPk);
            }
            db.pp().nMem += 1;
            regRowid = db.ppr().nMem;
        }
        let iRetInit = sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, regReturn);

        // If the original WHERE clause is z of the form:  (x1 OR x2 OR ...)
        // AND y Then for every term xN, evaluate as the subexpression: xN
        // AND y That way, terms in y that are factored into the disjunction
        // will be picked up by the recursive calls to sqlite3WhereBegin()
        // below.
        //
        // Actually, each subexpression is converted to "xN AND w" where w is
        // the "interesting" terms of z - terms that did not originate in the
        // ON or USING clause of a LEFT JOIN, and terms that are usable as
        // indices.
        //
        // This optimization also only applies if the (x1 OR x2 OR ...) term
        // is not contained in the ON clause of a LEFT JOIN.
        // See ticket http://sqlite.org/src/info/f2369304e4
        //
        // 2022-02-04:  Do not push down slices of a row-value comparison.
        // In other words, "w" or "y" may not be a slice of a vector.
        // Otherwise, the initialization of the right-hand operand of the
        // vector comparison might not occur, or might occur only in an OR
        // branch that is not taken.  dbsqlfuzz
        // 80a9fade844b4fb43564efc972bcb2c68270f5d1.
        //
        // 2022-03-03:  Do not push down expressions that involve subqueries.
        // The subquery might get coded as a subroutine.  Any
        // table-references in the subquery might be resolved to
        // index-references for the index on the OR branch in which the
        // subroutine is coded.  But if the subroutine is invoked from a
        // different OR branch that uses a different index, such
        // index-references will not work.  tag-20220303a
        // https://sqlite.org/forum/forumpost/36937b197273d403
        let nWC = wi.wc(pWC).a.len();
        if nWC > 1 {
            for iTerm in 0..nWC {
                let t = wi.wc(pWC).a[iTerm];
                let pExpr = t.pExpr;
                if TermId::new(pWC, iTerm) == pTerm {
                    continue;
                }
                if (t.wtFlags & (TERM_VIRTUAL | TERM_CODED | TERM_SLICE)) != 0 {
                    continue;
                }
                if (t.eOperator & WO_ALL) == 0 {
                    continue;
                }
                if db.ex(pExpr).HasProperty(EP_Subquery) {
                    continue; // tag-20220303a
                }
                if whereExprIsReady(db, wi, lv, pTabItem, pExpr) == 0 {
                    continue;
                }
                let d = sqlite3ExprDup(db, pExpr, 0);
                pAndExpr = sqlite3ExprAnd(db, pAndExpr, d);
            }
            if pAndExpr.nn() {
                // The extra 0x10000 bit on the opcode is masked off and does
                // not become part of the new Expr.op.  However, it does make
                // the op==TK_AND comparison inside of sqlite3PExpr() false,
                // and this prevents sqlite3PExpr() from applying the AND
                // short-circuit optimization, which we do not want here.
                pAndExpr = sqlite3PExpr(db, TK_AND as i32 | 0x10000, ExprId::NULL, pAndExpr);
            }
        }

        // Run a separate WHERE clause for each term of the OR clause.  After
        // eliminating duplicates from other WHERE clauses, the action for
        // each sub-WHERE clause is to to invoke the main loop body as a
        // subroutine.
        sqlite3VdbeExplain(db, 1, b"MULTI-INDEX OR", &[]);
        let nOr = wi.wc(pOrWc).a.len();
        for ii in 0..nOr {
            let ot = wi.wc(pOrWc).a[ii];
            if ot.leftCursor == iCur || (ot.eOperator & WO_AND) != 0 {
                let mut jmp1 = 0; // Address of jump operation
                let mut pOrExpr = sqlite3ExprDup(db, ot.pExpr, 0); // Local copy of OR clause term
                let pDelete = pOrExpr;
                if db.mallocFailed != 0 {
                    sqlite3ExprDelete(db, pDelete);
                    continue;
                }
                if pAndExpr.nn() {
                    db.exm(pAndExpr).pLeft = pOrExpr;
                    pOrExpr = pAndExpr;
                }
                // Loop through table entries that match term pOrTerm.
                sqlite3VdbeExplain(db, 1, b"INDEX %d", &[((ii + 1) as i32).into()]);
                let pSubWInfo = sqlite3WhereBegin(
                    db,
                    pOrTab,
                    pOrExpr,
                    ExprListId::NULL,
                    ExprListId::NULL,
                    SelectId::NULL,
                    WHERE_OR_SUBCLAUSE,
                    iCovCur,
                );
                debug_assert!(pSubWInfo.is_some() || db.ppr().nErr != 0);
                if let Some(mut sub) = pSubWInfo {
                    let addrExplain = sqlite3WhereExplainOneScan(db, pOrTab, &sub, 0, 0);
                    sqlite3WhereAddScanStatus(db, pOrTab, &sub, 0, addrExplain);

                    // This is the sub-WHERE clause body.  First skip over
                    // duplicate rows from prior sub-WHERE clauses, and record
                    // the rowid (or PRIMARY KEY) for the current row so that
                    // the same row will be skipped in subsequent sub-WHERE
                    // clauses.
                    if (wi.wctrlFlags & WHERE_DUPLICATES_OK) == 0 {
                        let iSet: i32 = if ii == nOr - 1 { -1 } else { ii as i32 };
                        if pTab.borrow().HasRowid() {
                            sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, -1, regRowid);
                            jmp1 = sqlite3VdbeAddOp4Int(
                                db.v(),
                                OP_RowSetTest,
                                regRowset,
                                0,
                                regRowid,
                                iSet,
                            );
                        } else {
                            let pPk = crate::build::sqlite3PrimaryKeyIndex(&pTab).unwrap();
                            let (nPk, pkCols) = {
                                let p = pPk.borrow();
                                (p.nKeyCol as i32, p.aiColumn.clone())
                            };

                            // Read the PK into an array of temp registers.
                            let r = sqlite3GetTempRange(db, nPk);
                            for iPk in 0..nPk {
                                let iCol = pkCols[iPk as usize] as i32;
                                sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, iCol, r + iPk);
                            }

                            // Check if the temp table already contains this
                            // key. If so, the row has already been included in
                            // the result set and can be ignored (by jumping
                            // past the Gosub below). Otherwise, insert the key
                            // into the temp table and proceed with processing
                            // the row.
                            //
                            // Use some of the same optimizations as
                            // OP_RowSetTest: If iSet is zero, assume that the
                            // key cannot already be present in the temp table.
                            // And if iSet is -1, assume that there is no need
                            // to insert the key into the temp table, as it
                            // will never be tested for.
                            if iSet != 0 {
                                jmp1 = sqlite3VdbeAddOp4Int(db.v(), OP_Found, regRowset, 0, r, nPk);
                            }
                            if iSet >= 0 {
                                sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, r, nPk, regRowid);
                                sqlite3VdbeAddOp4Int(
                                    db.v(),
                                    OP_IdxInsert,
                                    regRowset,
                                    regRowid,
                                    r,
                                    nPk,
                                );
                                if iSet != 0 {
                                    sqlite3VdbeChangeP5(db.v(), OPFLAG_USESEEKRESULT);
                                }
                            }

                            // Release the array of temp registers
                            sqlite3ReleaseTempRange(db, r, nPk);
                        }
                    }

                    // Invoke the main loop body as a subroutine
                    sqlite3VdbeAddOp2(db.v(), OP_Gosub, regReturn, iLoopBody);

                    // Jump here (skipping the main loop body subroutine) if
                    // the current sub-WHERE row is a duplicate from prior
                    // sub-WHEREs.
                    if jmp1 != 0 {
                        sqlite3VdbeJumpHere(db.v(), jmp1);
                    }

                    // The pSubWInfo->untestedTerms flag means that this OR
                    // term contained one or more AND term from a notReady
                    // table.  The terms from the notReady table could not be
                    // tested and will need to be tested later.
                    if sub.untestedTerms != 0 {
                        untestedTerms = true;
                    }

                    // If all of the OR-connected terms are optimized using the
                    // same index, and the index is opened using the same
                    // cursor number by each call to sqlite3WhereBegin() made
                    // by this loop, it may be possible to use that index as a
                    // covering index.
                    //
                    // If the call to sqlite3WhereBegin() above resulted in a
                    // scan that uses an index, and this is either the first
                    // OR-connected term processed or the index is the same as
                    // that used by all previous terms, set pCov to the
                    // candidate covering index. Otherwise, set pCov to NULL to
                    // indicate that no candidate covering index will be
                    // available.
                    let pSubLoop = sub.lp(0).clone();
                    debug_assert!((pSubLoop.wsFlags & WHERE_AUTO_INDEX) == 0);
                    if (pSubLoop.wsFlags & WHERE_INDEXED) != 0
                        && (ii == 0 || idx_eq(&pSubLoop.u.btree.pIndex, &pCov))
                        && (pTab.borrow().HasRowid()
                            || !pSubLoop
                                .u
                                .btree
                                .pIndex
                                .as_ref()
                                .unwrap()
                                .borrow()
                                .IsPrimaryKeyIndex())
                    {
                        debug_assert!(sub.a[0].iIdxCur == iCovCur);
                        pCov = pSubLoop.u.btree.pIndex.clone();
                    } else {
                        pCov = None;
                    }
                    if crate::where_::sqlite3WhereUsesDeferredSeek(&mut sub) != 0 {
                        wi.bDeferredSeek = 1;
                    }

                    // Finish the loop through table entries that match term
                    // pOrTerm.
                    sqlite3WhereEnd(db, &mut sub);
                    crate::vdbeaux::sqlite3VdbeExplainPop(db);
                }
                sqlite3ExprDelete(db, pDelete);
            }
        }
        crate::vdbeaux::sqlite3VdbeExplainPop(db);
        debug_assert!((wi.lp(lv).wsFlags & WHERE_MULTI_OR) != 0);
        debug_assert!((wi.lp(lv).wsFlags & WHERE_IN_ABLE) == 0);
        wi.a[lv].u.pCoveringIdx = pCov.clone();
        if pCov.is_some() {
            wi.a[lv].iIdxCur = iCovCur;
        }
        if pAndExpr.nn() {
            db.exm(pAndExpr).pLeft = ExprId::NULL;
            sqlite3ExprDelete(db, pAndExpr);
        }
        let a = sqlite3VdbeCurrentAddr(db.v());
        sqlite3VdbeChangeP1(db.v(), iRetInit, a);
        let brk = wi.a[lv].addrBrk;
        sqlite3VdbeGoto(db.v(), brk);
        sqlite3VdbeResolveLabel(db.v(), iLoopBody);

        // Set the P2 operand of the OP_Return opcode that will end the
        // current loop to point to this spot, which is the top of the next
        // containing loop.  The byte-code formatter will use that P2 value
        // as a hint to indent everything in between the this point and the
        // final OP_Return.  See tag-20220407a in vdbe.c and shell.c
        debug_assert!(wi.a[lv].op == OP_Return);
        wi.a[lv].p2 = sqlite3VdbeCurrentAddr(db.v());

        if !untestedTerms {
            disableTerm(db, wi, lv, pTerm);
        }
    } else {
        // Case 6:  There is no usable index.  We must do a complete
        //          scan of the entire table.
        const aStep: [u8; 2] = [OP_Next, OP_Prev];
        const aStart: [u8; 2] = [OP_Rewind, OP_Last];
        debug_assert!(bRev == 0 || bRev == 1);
        if fg.isRecursive != 0 {
            // Tables marked isRecursive have only a single row that is stored
            // in a pseudo-cursor.  No need to Rewind or Next such cursors.
            wi.a[lv].op = OP_Noop;
        } else {
            // codeCursorHint(pTabItem, pWInfo, pLevel, 0): no-op
            wi.a[lv].op = aStep[bRev as usize];
            wi.a[lv].p1 = iCur;
            let h = wi.a[lv].addrHalt;
            wi.a[lv].p2 = 1 + sqlite3VdbeAddOp2(db.v(), aStart[bRev as usize], iCur, h);
            wi.a[lv].p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        }
    }

    // Insert code to test every subexpression that can be completely
    // computed using the current set of tables.
    //
    // This loop may run between one and three times, depending on the
    // constraints to be generated. The value of stack variable iLoop
    // determines the constraints coded by each iteration, as follows:
    //
    // iLoop==1: Code only expressions that are entirely covered by pIdx.
    // iLoop==2: Code remaining expressions that do not contain correlated
    //           sub-queries.
    // iLoop==3: Code all remaining expressions.
    //
    // An effort is made to skip unnecessary iterations of the loop.
    //
    // This optimization of causing simple query restrictions to occur
    // before more complex one is call the "push-down" optimization in
    // MySQL.  Here in SQLite, the name is "MySQL push-down", since there is
    // also another totally unrelated optimization called "WHERE-clause
    // push-down".  Sometimes the qualifier is omitted, resulting in an
    // ambiguity, so beware.
    iLoop = if pIdx.is_some() { 1 } else { 2 };
    loop {
        let mut iNext = 0; // Next value for iLoop
        let nT = wi.wc(pWC).a.len();
        for k in 0..nT {
            let t = wi.wc(pWC).a[k];
            let mut skipLikeAddr = 0;
            if t.wtFlags & (TERM_VIRTUAL | TERM_CODED) != 0 {
                continue;
            }
            if (t.prereqAll & wi.a[lv].notReady) != 0 {
                wi.untestedTerms = 1;
                continue;
            }
            let pE = t.pExpr;
            debug_assert!(pE.nn());
            if whereExprIsReady(db, wi, lv, pTabItem, pE) == 0 {
                continue;
            }
            if iLoop == 1 {
                let tc = wi.a[lv].iTabCur;
                if sqlite3ExprCoveredByIndex(db, pE, tc, pIdx.as_ref().unwrap()) == 0 {
                    iNext = 2;
                    continue;
                }
            }
            if iLoop < 3 && (t.wtFlags & TERM_VARSELECT) != 0 {
                if iNext == 0 {
                    iNext = 3;
                }
                continue;
            }

            if (t.wtFlags & TERM_LIKECOND) != 0 {
                // If the TERM_LIKECOND flag is set, that means that the range
                // search is sufficient to guarantee that the LIKE operator is
                // true, so we can skip the call to the like(A,B) function.
                // But this only works for strings.  So do not skip the call
                // to the function on the pass that compares BLOBs.
                let x = wi.a[lv].iLikeRepCntr;
                if x > 0 {
                    skipLikeAddr = sqlite3VdbeAddOp1(
                        db.v(),
                        if (x & 1) != 0 { OP_IfNot } else { OP_If },
                        (x >> 1) as i32,
                    );
                }
            }
            sqlite3ExprIfFalse(db, pE, addrCont, SQLITE_JUMPIFNULL as i32);
            if skipLikeAddr != 0 {
                sqlite3VdbeJumpHere(db.v(), skipLikeAddr);
            }
            wi.wcm(pWC).a[k].wtFlags |= TERM_CODED;
        }
        iLoop = iNext;
        if iLoop <= 0 {
            break;
        }
    }

    // Insert code to test for implied constraints based on transitivity
    // of the "==" operator.
    //
    // Example: If the WHERE clause contains "t1.a=t2.b" and "t2.b=123"
    // and we are coding the t1 loop and the t2 loop has not yet coded,
    // then we cannot use the "t1.a=t2.b" constraint, but we can code
    // the implied "t1.a=123" constraint.
    let nBase = wi.wc(pWC).nBase as usize;
    for k in 0..nBase {
        let t = wi.wc(pWC).a[k];
        if t.wtFlags & (TERM_VIRTUAL | TERM_CODED) != 0 {
            continue;
        }
        if (t.eOperator & (WO_EQ | WO_IS)) == 0 {
            continue;
        }
        if (t.eOperator & WO_EQUIV) == 0 {
            continue;
        }
        if t.leftCursor != iCur {
            continue;
        }
        if fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT) != 0 {
            continue;
        }
        let pE = t.pExpr;
        debug_assert!(!db.ex(pE).HasProperty(EP_OuterON));
        debug_assert!((t.prereqRight & wi.a[lv].notReady) != 0);
        debug_assert!((t.eOperator & (WO_OR | WO_AND)) == 0);
        let Some(pAlt) = sqlite3WhereFindTerm(
            db,
            wi,
            pWC,
            iCur,
            t.u.x.leftColumn,
            notReady,
            (WO_EQ | WO_IN | WO_IS) as u32,
            None,
        ) else {
            continue;
        };
        let at = *wi.t(pAlt);
        if at.wtFlags & TERM_CODED != 0 {
            continue;
        }
        if db.ex(at.pExpr).HasProperty(EP_Collate) {
            continue;
        }
        if (at.eOperator & WO_IN) != 0 && db.ex(at.pExpr).UseXSelect() && {
            let s = db.ex(at.pExpr).x.pSelect;
            db.el(db.sel(s).pEList).nExpr() > 1
        } {
            continue;
        }
        // sEAlt = *pAlt->pExpr; sEAlt.pLeft = pE->pLeft;
        let mut sEAlt = db.ex(at.pExpr).clone();
        sEAlt.pLeft = db.ex(pE).pLeft;
        let pEAlt = db.arena.new_expr(sEAlt);
        sqlite3ExprIfFalse(db, pEAlt, addrCont, SQLITE_JUMPIFNULL as i32);
        wi.tm(pAlt).wtFlags |= TERM_CODED;
    }

    // For a RIGHT OUTER JOIN, record the fact that the current row has
    // been matched at least once.
    if let Some(pRJ) = wi.a[lv].pRJ {
        let nPk: i32;
        let r: i32;

        // pTab is the right-hand table of the RIGHT JOIN.  Generate code that
        // will record that the current row of that table has been matched at
        // least once.  This is accomplished by storing the PK for the row in
        // both the iMatch index and the regBloom Bloom filter.
        let pTab = item_tab(db, pTabItem);
        if pTab.borrow().HasRowid() {
            r = sqlite3GetTempRange(db, 2);
            let tc = wi.a[lv].iTabCur;
            sqlite3ExprCodeGetColumnOfTable(db, &pTab, tc, -1, r + 1);
            nPk = 1;
        } else {
            let pPk = crate::build::sqlite3PrimaryKeyIndex(&pTab).unwrap();
            let pkCols = pPk.borrow().aiColumn.clone();
            nPk = pPk.borrow().nKeyCol as i32;
            r = sqlite3GetTempRange(db, nPk + 1);
            for iPk in 0..nPk {
                let iCol = pkCols[iPk as usize] as i32;
                sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, iCol, r + 1 + iPk);
            }
        }
        let jmp1 = sqlite3VdbeAddOp4Int(db.v(), OP_Found, pRJ.iMatch, 0, r + 1, nPk);
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, r + 1, nPk, r);
        sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, pRJ.iMatch, r, r + 1, nPk);
        if pRJ.regBloom != 0 {
            sqlite3VdbeAddOp4Int(db.v(), OP_FilterAdd, pRJ.regBloom, 0, r + 1, nPk);
            sqlite3VdbeChangeP5(db.v(), OPFLAG_USESEEKRESULT);
        }
        sqlite3VdbeJumpHere(db.v(), jmp1);
        sqlite3ReleaseTempRange(db, r, nPk + 1);
    }

    // For a LEFT OUTER JOIN, generate code that will record the fact that
    // at least one row of the right table has matched the left table.
    let mut codeOuterJoinConstraints = false;
    if wi.a[lv].iLeftJoin != 0 {
        wi.a[lv].addrFirst = sqlite3VdbeCurrentAddr(db.v());
        let lj = wi.a[lv].iLeftJoin;
        sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, lj);
        if wi.a[lv].pRJ.is_none() {
            codeOuterJoinConstraints = true; // WHERE clause constraints
        }
    }

    if let Some(mut pRJ) = wi.a[lv].pRJ {
        // Create a subroutine used to process all interior loops and code
        // of the RIGHT JOIN.  During normal operation, the subroutine will
        // be in-line with the rest of the code.  But at the end, a separate
        // loop will run that invokes this subroutine for unmatched rows
        // of pTab, with all tables to left begin set to NULL.
        sqlite3VdbeAddOp2(db.v(), OP_BeginSubrtn, 0, pRJ.regReturn);
        pRJ.addrSubrtn = sqlite3VdbeCurrentAddr(db.v());
        wi.a[lv].pRJ = Some(pRJ);
        debug_assert!(db.ppr().withinRJSubrtn < 255);
        db.pp().withinRJSubrtn += 1;

        // WHERE clause constraints must be deferred until after outer join
        // row elimination has completed, since WHERE clause constraints
        // apply to the results of the OUTER JOIN.  The following loop
        // generates the appropriate WHERE clause constraint checks.
        // tag-20220513a.
        codeOuterJoinConstraints = true;
    }
    if codeOuterJoinConstraints {
        // code_outer_join_constraints:
        let nBase = wi.wc(pWC).nBase as usize;
        for j in 0..nBase {
            let t = wi.wc(pWC).a[j];
            if t.wtFlags & (TERM_VIRTUAL | TERM_CODED) != 0 {
                continue;
            }
            if (t.prereqAll & wi.a[lv].notReady) != 0 {
                debug_assert!(wi.untestedTerms != 0);
                continue;
            }
            if fg.jointype & JT_LTORJ != 0 {
                continue;
            }
            debug_assert!(t.pExpr.nn());
            sqlite3ExprIfFalse(db, t.pExpr, addrCont, SQLITE_JUMPIFNULL as i32);
            wi.wcm(pWC).a[j].wtFlags |= TERM_CODED;
        }
    }

    wi.a[lv].notReady
}

/// Generate the code for the loop that finds all non-matched terms
/// for a RIGHT JOIN.
///
/// C: `SQLITE_NOINLINE void sqlite3WhereRightJoinLoop( WhereInfo *pWInfo, int iLevel, WhereLevel *pLevel )`
pub fn sqlite3WhereRightJoinLoop(
    db: &mut Sqlite3,
    pWInfo: &mut crate::where_::WhereInfo,
    iLevel: i32,
) {
    let wi = pWInfo;
    let lv = iLevel as usize;
    let pRJ = wi.a[lv].pRJ.unwrap();
    let mut pSubWhere = ExprId::NULL;
    let pWC = SWC;
    let pLoop = wi.lp(lv).clone();
    let pTabItem = SrcItemId(wi.pTabList, wi.a[lv].iFrom as u32);
    let mut mAll: Bitmask = 0;

    let zName = item_tab(db, pTabItem).borrow().zName.clone();
    sqlite3VdbeExplain(db, 1, b"RIGHT-JOIN %s", &[(&zName).into()]);
    sqlite3VdbeNoJumpsOutsideSubrtn(db.v(), pRJ.addrSubrtn, pRJ.endSubrtn, pRJ.regReturn);
    for k in 0..lv {
        debug_assert!(wi.lp(k).iTab == wi.a[k].iFrom);
        let pRight = SrcItemId(wi.pTabList, wi.a[k].iFrom as u32);
        mAll |= wi.lp(k).maskSelf;
        if db.item(pRight).fg.viaCoroutine != 0 {
            debug_assert!(db.item(pRight).fg.isSubquery != 0 && db.item(pRight).u4.pSubq.is_some());
            let pSubq = **db.item(pRight).u4.pSubq.as_ref().unwrap();
            debug_assert!(pSubq.pSelect.nn() && db.sel(pSubq.pSelect).pEList.nn());
            let n = db.el(db.sel(pSubq.pSelect).pEList).nExpr();
            sqlite3VdbeAddOp3(db.v(), OP_Null, 0, pSubq.regResult, pSubq.regResult + n - 1);
        }
        let tc = wi.a[k].iTabCur;
        sqlite3VdbeAddOp1(db.v(), OP_NullRow, tc);
        let iIdxCur = wi.a[k].iIdxCur;
        if iIdxCur != 0 {
            sqlite3VdbeAddOp1(db.v(), OP_NullRow, iIdxCur);
        }
    }
    if (db.item(pTabItem).fg.jointype & JT_LTORJ) == 0 {
        mAll |= pLoop.maskSelf;
        let n = wi.wc(pWC).a.len();
        for k in 0..n {
            let t = wi.wc(pWC).a[k];
            if (t.wtFlags & (TERM_VIRTUAL | TERM_SLICE)) != 0 && t.eOperator != WO_ROWVAL {
                break;
            }
            if t.prereqAll & !mAll != 0 {
                continue;
            }
            if db.ex(t.pExpr).HasProperty(EP_OuterON | EP_InnerON) {
                continue;
            }
            let d = sqlite3ExprDup(db, t.pExpr, 0);
            pSubWhere = sqlite3ExprAnd(db, pSubWhere, d);
        }
    }
    if wi.a[lv].iIdxCur != 0 {
        // pSubWhere may contain expressions that read from an index on the
        // table on the RHS of the right join. All such expressions first test
        // if the index is pointing at a NULL row, and if so, read from the
        // table cursor instead. So ensure that the index cursor really is
        // pointing at a NULL row here, so that no values are read from it
        // during the scan of the RHS of the RIGHT join below.
        let ic = wi.a[lv].iIdxCur;
        sqlite3VdbeAddOp1(db.v(), OP_NullRow, ic);
    }
    let mut it = db.item(pTabItem).clone();
    it.fg.jointype = 0;
    let pFrom = db.arena.new_srclist(SrcList { a: vec![it] });
    if db.ppr().withinRJSubrtn >= 100 {
        // This limit --------------^^^
        // is based on an historical assert().  It is not compile-time or
        // run-time configurable.  It could perhaps be raised as high as 254,
        // but only an attack robot would ever do even 100 RIGHT JOINS within
        // a single query, so we'll just leave it as it is.
        errmsg!(db, "too many RIGHT JOINs");
        return;
    }
    db.pp().withinRJSubrtn += 1;
    let pSubWInfo = sqlite3WhereBegin(
        db,
        pFrom,
        pSubWhere,
        ExprListId::NULL,
        ExprListId::NULL,
        SelectId::NULL,
        WHERE_RIGHT_JOIN,
        0,
    );
    if let Some(mut sub) = pSubWInfo {
        let iCur = wi.a[lv].iTabCur; // Table on RHS of RIGHT JOIN
        db.pp().nMem += 1;
        let r = db.ppr().nMem; // Register range to hold primary key
        let nPk: i32; // Number of values in the primary key
        let addrCont = sqlite3WhereContinueLabel(&mut sub);
        let pTab = item_tab(db, pTabItem); // Table on RHS of RIGHT JOIN
        if pTab.borrow().HasRowid() {
            sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, -1, r);
            nPk = 1;
        } else {
            let pPk = crate::build::sqlite3PrimaryKeyIndex(&pTab).unwrap();
            let pkCols = pPk.borrow().aiColumn.clone();
            nPk = pPk.borrow().nKeyCol as i32;
            db.pp().nMem += nPk - 1;
            for iPk in 0..nPk {
                let iCol = pkCols[iPk as usize] as i32;
                sqlite3ExprCodeGetColumnOfTable(db, &pTab, iCur, iCol, r + iPk);
            }
        }

        // Generate code that checks to see if the current row of the RHS
        // table has appeared in any prior output row.
        if pRJ.regBloom != 0 {
            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
            sqlite3VdbeAddOp4Int(db.v(), OP_Filter, pRJ.regBloom, a, r, nPk);
        }
        sqlite3VdbeAddOp4Int(db.v(), OP_Found, pRJ.iMatch, addrCont, r, nPk);
        let r2 = sqlite3GetTempReg(db);

        // Generate code that inserts the PK of the RHS table into the
        // pRH->iMatch index to indicate that the current row has appeared
        // in the output set.
        sqlite3VdbeAddOp3(db.v(), OP_MakeRecord, r, nPk, r2);
        sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, pRJ.iMatch, r2, r, nPk);
        sqlite3ReleaseTempReg(db, r2);
        if pRJ.regBloom != 0 {
            sqlite3VdbeAddOp4Int(db.v(), OP_FilterAdd, pRJ.regBloom, 0, r, nPk);
            sqlite3VdbeChangeP5(db.v(), OPFLAG_USESEEKRESULT);
        }

        // Invoke the subroutine that actually puts the current RHS table row
        // into the output set, with NULLs for the LHS.
        sqlite3VdbeAddOp2(db.v(), OP_Gosub, pRJ.regReturn, pRJ.addrSubrtn);
        sqlite3WhereEnd(db, &mut sub);
    }
    sqlite3ExprDelete(db, pSubWhere);
    crate::vdbeaux::sqlite3VdbeExplainPop(db);
    debug_assert!(db.ppr().withinRJSubrtn > 0);
    db.pp().withinRJSubrtn -= 1;
}
