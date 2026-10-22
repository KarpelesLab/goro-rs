//! Port of whereexpr.c.
//!
//! This file contains utility routines for analyzing Expr objects in the
//! WHERE clause.  See the module documentation of `where_` for the
//! index-based representation of WhereClause/WhereTerm used here: the
//! functions take the WhereInfo plus the id of the clause they work on.

#![allow(unused_variables)]

use crate::expr::*;
use crate::global::*;
use crate::int::*;
use crate::util::*;
use crate::vdbeaux::*;
use crate::where_::*;
use std::rc::Rc;

/// Deallocate all memory associated with a WhereOrInfo object.
fn whereOrInfoDelete(db: &mut Sqlite3, wi: &mut WhereInfo, p: usize) {
    let wc = wi.aOrInfo[p].wc;
    sqlite3WhereClauseClear(db, wi, wc);
}

/// Deallocate all memory associated with a WhereAndInfo object.
fn whereAndInfoDelete(db: &mut Sqlite3, wi: &mut WhereInfo, wc: WcId) {
    sqlite3WhereClauseClear(db, wi, wc);
}

/// Add a single new WhereTerm entry to the WhereClause object pWC.
/// The new WhereTerm object is constructed from Expr p and with wtFlags.
/// The index in pWC->a[] of the new WhereTerm is returned.
pub(crate) fn whereClauseInsert(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    wc: WcId,
    p: ExprId,
    wtFlags: u16,
) -> i32 {
    let truthProb: LogEst;
    if p.nn() && db.ex(p).HasProperty(EP_Unlikely) {
        truthProb = sqlite3LogEst(db.ex(p).iTable as u64).wrapping_sub(270);
    } else {
        truthProb = 1;
    }
    let pExpr = sqlite3ExprSkipCollateAndLikely(db, p);
    let pWC = wi.wcm(wc);
    let idx = pWC.a.len();
    // memset(&pTerm->eOperator, 0, ...) clears every field from eOperator
    // to the end of the structure, including iParent.
    pWC.a.push(WhereTerm {
        pExpr,
        pWC: wc,
        truthProb,
        wtFlags,
        eOperator: 0,
        nChild: 0,
        eMatchOp: 0,
        iParent: 0,
        leftCursor: 0,
        u: WhereTermU::default(),
        prereqRight: 0,
        prereqAll: 0,
    });
    if (wtFlags & TERM_VIRTUAL) == 0 {
        pWC.nBase = pWC.a.len() as i32;
    }
    idx as i32
}

/// Return TRUE if the given operator is one of the operators that is
/// allowed for an indexable WHERE clause term.  The allowed operators are
/// "=", "<", ">", "<=", ">=", "IN", "IS", and "IS NULL"
pub(crate) fn allowedOp(op: u8) -> bool {
    if op > TK_GE {
        return false;
    }
    if op >= TK_EQ {
        return true;
    }
    op == TK_IN || op == TK_ISNULL || op == TK_IS
}

/// Commute a comparison operator.  Expressions of the form "X op Y"
/// are converted into "Y op X".
fn exprCommute(db: &mut Sqlite3, pExpr: ExprId) -> u16 {
    let (l, r) = {
        let e = db.ex(pExpr);
        (e.pLeft, e.pRight)
    };
    if db.ex(l).op == TK_VECTOR || db.ex(r).op == TK_VECTOR || {
        let c1 = sqlite3BinaryCompareCollSeq(db, l, r);
        let c2 = sqlite3BinaryCompareCollSeq(db, r, l);
        !coll_ptr_eq(&c1, &c2)
    } {
        db.exm(pExpr).flags ^= EP_Commuted;
    }
    let e = db.exm(pExpr);
    std::mem::swap(&mut e.pRight, &mut e.pLeft);
    if e.op >= TK_GT {
        debug_assert!(e.op >= TK_GT && e.op <= TK_GE);
        e.op = ((e.op - TK_GT) ^ 2) + TK_GT;
    }
    0
}

/// C pointer equality of two `CollSeq*`.
pub(crate) fn coll_ptr_eq(a: &Option<CollSeqRef>, b: &Option<CollSeqRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Translate from TK_xx operator to WO_xx bitmask.
pub(crate) fn operatorMask(op: u8) -> u16 {
    let c: u16;
    debug_assert!(allowedOp(op));
    if op >= TK_EQ {
        c = WO_EQ << (op - TK_EQ);
    } else if op == TK_IN {
        c = WO_IN;
    } else if op == TK_ISNULL {
        c = WO_ISNULL;
    } else {
        debug_assert!(op == TK_IS);
        c = WO_IS;
    }
    c
}

/// Lookup table of sqlite3Utf8Read().
static sqlite3Utf8Trans1: [u8; 64] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x00, 0x01, 0x02, 0x03, 0x00, 0x01, 0x00, 0x00,
];

/// Local equivalent of sqlite3Utf8Read() on a zero-terminated buffer: read
/// one UTF-8 character starting at `z[*pos]` and advance `*pos`.  (The
/// skeleton signature of utf.rs's sqlite3Utf8Read is a placeholder.)
fn utf8_read(z: &[u8], pos: &mut usize) -> u32 {
    let at = |i: usize| -> u8 { z.get(i).copied().unwrap_or(0) };
    let mut c = at(*pos) as u32;
    *pos += 1;
    if c >= 0xc0 {
        c = sqlite3Utf8Trans1[(c - 0xc0) as usize] as u32;
        while (at(*pos) & 0xc0) == 0x80 {
            c = (c << 6) + (0x3f & at(*pos) as u32);
            *pos += 1;
        }
        if c < 0x80 || (c & 0xFFFFF800) == 0xD800 {
            c = 0xFFFD;
        }
    }
    c
}

/// Check to see if the given expression is a LIKE or GLOB operator that
/// can be optimized using inequality constraints.  Return TRUE if it is
/// so and false if not.
///
/// In order for the operator to be optimizible, the RHS must be a string
/// literal that does not begin with a wildcard.  The LHS must be a column
/// that may only be NULL, a string, or a BLOB, never a number. (This means
/// that virtual tables cannot participate in the LIKE optimization.)  The
/// collating sequence for the column on the LHS must be appropriate for
/// the operator.
fn isLikeOrGlob(
    db: &mut Sqlite3,
    pExpr: ExprId,
    ppPrefix: &mut ExprId,
    pisComplete: &mut i32,
    pnoCase: &mut i32,
) -> i32 {
    let mut z: Option<Vec<u8>> = None; // String on RHS of LIKE operator (NUL-terminated)
    let mut wc = [0u8; 4]; // Wildcard characters
    let mut pVal: Option<Box<Mem>> = None;
    let rc: i32;

    if crate::func::sqlite3IsLikeFunction(db, pExpr, pnoCase, &mut wc[..]) == 0 {
        return 0;
    }
    let pList = db.ex(pExpr).x.pList;
    let pLeft = db.el(pList).a[1].pExpr;

    let a0 = db.el(pList).a[0].pExpr;
    let pRight = sqlite3ExprSkipCollate(db, a0);
    let op = db.ex(pRight).op;
    if op == TK_VARIABLE && (db.flags & SQLITE_EnableQPSG) == 0 {
        let iCol = db.ex(pRight).iColumn as i32;
        if let Some(id) = db.ppr().pReprepare {
            pVal = db
                .with_stmt(id, |db, v| {
                    sqlite3VdbeGetBoundValue(db, v, iCol, SQLITE_AFF_BLOB)
                })
                .flatten();
        }
        if let Some(m) = pVal.as_deref_mut() {
            if crate::vdbeapi::sqlite3_value_type(m) == SQLITE_TEXT {
                if let Some(t) = crate::vdbeapi::sqlite3_value_text(m) {
                    let mut v = t.to_vec();
                    if let Some(p) = v.iter().position(|&c| c == 0) {
                        v.truncate(p);
                    }
                    v.push(0);
                    z = Some(v);
                }
            }
        }
        with_vdbe(db, |db, v| sqlite3VdbeReprepareOnBind(db, v, iCol, 0));
    } else if op == TK_STRING {
        let mut v = db.ex(pRight).token().to_vec();
        v.push(0);
        z = Some(v);
    }
    if let Some(zz) = z.as_ref() {
        let at = |i: usize| -> u8 { zz.get(i).copied().unwrap_or(0) };
        // Count the number of prefix bytes prior to the first wildcard,
        // U+fffd character, or malformed utf-8. If the underlying database
        // has a UTF16LE encoding, then only consider ASCII characters.
        let mut cnt: usize = 0;
        let mut c: u8;
        loop {
            c = at(cnt);
            if c == 0 || c == wc[0] || c == wc[1] || c == wc[2] {
                break;
            }
            cnt += 1;
            if c == wc[3] && at(cnt) > 0 && at(cnt) < 0x80 {
                cnt += 1;
            } else if c >= 0x80 {
                let mut z2 = cnt - 1;
                if c == 0xff || utf8_read(zz, &mut z2) == 0xfffd || db.ENC() == SQLITE_UTF16LE {
                    cnt -= 1;
                    break;
                } else {
                    cnt = z2;
                }
            }
        }

        // The optimization is possible only if (1) the pattern does not
        // begin with a wildcard and if (2) the non-wildcard prefix does not
        // end with an (illegal 0xff) character, or (3) the pattern does not
        // consist of a single escape character.
        if (cnt > 1 || (cnt > 0 && at(0) != wc[3])) && 255 != at(cnt - 1) {
            // A "complete" match if the pattern ends with "*" or "%"
            *pisComplete = (c == wc[0] && at(cnt + 1) == 0 && db.ENC() != SQLITE_UTF16LE) as i32;

            // Get the pattern prefix.  Remove all escapes from the prefix.
            let zstr: Vec<u8> = zz[..zz.len() - 1].to_vec();
            let pPrefix = sqlite3Expr(db, TK_STRING as i32, Some(&zstr[..]));
            if pPrefix.nn() {
                let mut zNew: Vec<u8> = db.ex(pPrefix).token().to_vec();
                zNew.resize(zNew.len().max(cnt) + 2, 0);
                zNew[cnt] = 0;
                let mut iTo = 0usize;
                let mut iFrom = 0usize;
                while iFrom < cnt {
                    if zNew[iFrom] == wc[3] {
                        iFrom += 1;
                    }
                    zNew[iTo] = zNew[iFrom];
                    iTo += 1;
                    iFrom += 1;
                }
                zNew[iTo] = 0;
                debug_assert!(iTo > 0);
                // The C string ends at the first NUL
                let mut zTok: Vec<u8> = zNew[..iTo].to_vec();
                if let Some(p) = zTok.iter().position(|&c| c == 0) {
                    zTok.truncate(p);
                }
                db.exm(pPrefix).u.zToken = Some(zTok.clone());

                // If the LHS is not an ordinary column with TEXT affinity,
                // then the pattern prefix boundaries (both the start and end
                // boundaries) must not look like a number.  Otherwise the
                // pattern might be treated as a number, which will
                // invalidate the LIKE optimization.
                let notTextCol = {
                    let lop = db.ex(pLeft).op;
                    lop != TK_COLUMN
                        || sqlite3ExprAffinity(db, pLeft) != SQLITE_AFF_TEXT
                        || (db.ex(pLeft).UseYTab()
                            && db.ex(pLeft).y.pTab.is_some()
                            && db.ex(pLeft).y.pTab.as_ref().unwrap().borrow().IsVirtual())
                };
                if notTextCol {
                    let mut rDummy = 0.0f64;
                    let mut isNum = sqlite3AtoF(Some(&zTok[..]), &mut rDummy);
                    if isNum <= 0 {
                        if iTo == 1 && zTok.first() == Some(&b'-') {
                            isNum = 1;
                        } else {
                            let mut z3 = zTok.clone();
                            if let Some(last) = z3.last_mut() {
                                *last = last.wrapping_add(1);
                            }
                            isNum = sqlite3AtoF(Some(&z3[..]), &mut rDummy);
                        }
                    }
                    if isNum > 0 {
                        sqlite3ExprDelete(db, pPrefix);
                        if let Some(mut m) = pVal {
                            crate::vdbemem::sqlite3ValueFree(db, &mut m);
                        }
                        return 0;
                    }
                }
            }
            *ppPrefix = pPrefix;

            // If the RHS pattern is a bound parameter, make arrangements to
            // reprepare the statement when that parameter is rebound
            if op == TK_VARIABLE {
                let iCol = db.ex(pRight).iColumn as i32;
                with_vdbe(db, |db, v| sqlite3VdbeReprepareOnBind(db, v, iCol, 0));
                if *pisComplete != 0 && db.ex(pRight).token().len() > 1 {
                    // If the rhs of the LIKE expression is a variable, and the
                    // current value of the variable means there is no need to
                    // invoke the LIKE function, then no OP_Variable will be
                    // added to the program.  This causes problems for the
                    // sqlite3_bind_parameter_name() API. To work around them,
                    // add a dummy OP_Variable here.
                    let r1 = sqlite3GetTempReg(db);
                    sqlite3ExprCodeTarget(db, pRight, r1);
                    let a = sqlite3VdbeCurrentAddr(db.v()) - 1;
                    sqlite3VdbeChangeP3(db.v(), a, 0);
                    sqlite3ReleaseTempReg(db, r1);
                }
            }
        } else {
            z = None;
        }
    }

    rc = z.is_some() as i32;
    if let Some(mut m) = pVal {
        crate::vdbemem::sqlite3ValueFree(db, &mut m);
    }
    rc
}

/// If pExpr is one of "like", "glob", "match", or "regexp", then
/// return the corresponding SQLITE_INDEX_CONSTRAINT_xxxx value.
/// If not, return 0.
///
/// pExpr is guaranteed to be a TK_FUNCTION.
pub fn sqlite3ExprIsLikeOperator(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    static aOp: [(&[u8], i32); 4] = [
        (b"match", SQLITE_INDEX_CONSTRAINT_MATCH),
        (b"glob", SQLITE_INDEX_CONSTRAINT_GLOB),
        (b"like", SQLITE_INDEX_CONSTRAINT_LIKE),
        (b"regexp", SQLITE_INDEX_CONSTRAINT_REGEXP),
    ];
    debug_assert!(db.ex(pExpr).op == TK_FUNCTION);
    let z = db.ex(pExpr).u.zToken.clone();
    for (zOp, eOp) in aOp.iter() {
        if sqlite3StrICmp(z.as_deref(), Some(zOp)) == 0 {
            return *eOp;
        }
    }
    0
}

/// ExprIsVtab(X): X is a column of a virtual table.
fn ExprIsVtab(db: &Sqlite3, x: ExprId) -> bool {
    let e = db.ex(x);
    e.op == TK_COLUMN
        && e.y
            .pTab
            .as_ref()
            .map_or(false, |t| t.borrow().eTabType == TABTYP_VTAB)
}

/// Check to see if the pExpr expression is a form that needs to be passed
/// to the xBestIndex method of virtual tables.  Forms of interest include:
///
///          Expression                   Virtual Table Operator
///          -----------------------      ---------------------------------
///      1.  column MATCH expr            SQLITE_INDEX_CONSTRAINT_MATCH
///      2.  column GLOB expr             SQLITE_INDEX_CONSTRAINT_GLOB
///      3.  column LIKE expr             SQLITE_INDEX_CONSTRAINT_LIKE
///      4.  column REGEXP expr           SQLITE_INDEX_CONSTRAINT_REGEXP
///      5.  column != expr               SQLITE_INDEX_CONSTRAINT_NE
///      6.  expr != column               SQLITE_INDEX_CONSTRAINT_NE
///      7.  column IS NOT expr           SQLITE_INDEX_CONSTRAINT_ISNOT
///      8.  expr IS NOT column           SQLITE_INDEX_CONSTRAINT_ISNOT
///      9.  column IS NOT NULL           SQLITE_INDEX_CONSTRAINT_ISNOTNULL
///
/// In every case, "column" must be a column of a virtual table.  If there
/// is a match, set *ppLeft to the "column" expression, set *ppRight to the
/// "expr" expression (even though in forms (6) and (8) the column is on the
/// right and the expression is on the left).  Also set *peOp2 to the
/// appropriate virtual table operator.  The return value is 1 or 2 if there
/// is a match.  The usual return is 1, but if the RHS is also a column
/// of virtual table in forms (5) or (7) then return 2.
///
/// If the expression matches none of the patterns above, return 0.
fn isAuxiliaryVtabOperator(
    db: &mut Sqlite3,
    pExpr: ExprId,
    peOp2: &mut u8,
    ppLeft: &mut ExprId,
    ppRight: &mut ExprId,
) -> i32 {
    let op = db.ex(pExpr).op;
    if op == TK_FUNCTION {
        let pList = db.ex(pExpr).x.pList;
        if pList.is_null() || db.el(pList).nExpr() != 2 {
            return 0;
        }

        // Built-in operators MATCH, GLOB, LIKE, and REGEXP attach to a
        // virtual table on their second argument, which is the same as
        // the left-hand side operand in their in-fix form.
        //
        //       vtab_column MATCH expression
        //       MATCH(expression,vtab_column)
        let pCol = db.el(pList).a[1].pExpr;
        if ExprIsVtab(db, pCol) {
            let i = sqlite3ExprIsLikeOperator(db, pExpr);
            if i != 0 {
                *peOp2 = i as u8;
                *ppRight = db.el(pList).a[0].pExpr;
                *ppLeft = pCol;
                return 1;
            }
        }

        // We can also match against the first column of overloaded
        // functions where xFindFunction returns a value of at least
        // SQLITE_INDEX_CONSTRAINT_FUNCTION.
        //
        //      OVERLOADED(vtab_column,expression)
        //
        // NOTE: requires the virtual table module (xFindFunction), which
        // is not available yet in this port; such functions are not
        // reported as auxiliary operators.
        let pCol = db.el(pList).a[0].pExpr;
        if ExprIsVtab(db, pCol) {
            // sqlite3GetVTable(db, pCol->y.pTab)->pVtab->pModule->xFindFunction
        }
    } else if op >= TK_EQ {
        // Comparison operators are a common case.  Save a few comparisons
        // for that common case by terminating early.
        return 0;
    } else if op == TK_NE || op == TK_ISNOT || op == TK_NOTNULL {
        let mut res = 0;
        let mut pLeft = db.ex(pExpr).pLeft;
        let mut pRight = db.ex(pExpr).pRight;
        if ExprIsVtab(db, pLeft) {
            res += 1;
        }
        if pRight.nn() && ExprIsVtab(db, pRight) {
            res += 1;
            std::mem::swap(&mut pLeft, &mut pRight);
        }
        *ppLeft = pLeft;
        *ppRight = pRight;
        if op == TK_NE {
            *peOp2 = SQLITE_INDEX_CONSTRAINT_NE as u8;
        }
        if op == TK_ISNOT {
            *peOp2 = SQLITE_INDEX_CONSTRAINT_ISNOT as u8;
        }
        if op == TK_NOTNULL {
            *peOp2 = SQLITE_INDEX_CONSTRAINT_ISNOTNULL as u8;
        }
        return res;
    }
    0
}

/// If the pBase expression originated in the ON or USING clause of
/// a join, then transfer the appropriate markings over to derived.
fn transferJoinMarkings(db: &mut Sqlite3, pDerived: ExprId, pBase: ExprId) {
    if pDerived.nn() && db.ex(pBase).HasProperty(EP_OuterON | EP_InnerON) {
        let (f, j) = {
            let b = db.ex(pBase);
            (b.flags & (EP_OuterON | EP_InnerON), b.w.iJoin)
        };
        let d = db.exm(pDerived);
        d.flags |= f;
        d.w.iJoin = j;
    }
}

/// Mark term iChild as being a child of term iParent
fn markTermAsChild(wi: &mut WhereInfo, wc: WcId, iChild: i32, iParent: i32) {
    let tp = wi.wc(wc).a[iParent as usize].truthProb;
    {
        let c = &mut wi.wcm(wc).a[iChild as usize];
        c.iParent = iParent;
        c.truthProb = tp;
    }
    let p = &mut wi.wcm(wc).a[iParent as usize];
    debug_assert!(p.nChild < u8::MAX);
    p.nChild = p.nChild.wrapping_add(1);
}

/// Return the N-th AND-connected subterm of pTerm.  Or if pTerm is not
/// a conjunction, then return just pTerm when N==0.  If N is exceeds
/// the number of available subterms, return NULL.
fn whereNthSubterm(wi: &WhereInfo, pTerm: TermId, N: i32) -> Option<TermId> {
    let t = wi.t(pTerm);
    if t.eOperator != WO_AND {
        return if N == 0 { Some(pTerm) } else { None };
    }
    let wc = t.u.pAndInfo.expect("AND term without info");
    if N < wi.wc(wc).nTerm() {
        return Some(TermId::new(wc, N as usize));
    }
    None
}

/// Subterms pOne and pTwo are contained within WHERE clause pWC.  The
/// two subterms are in disjunction - they are OR-ed together.
///
/// If these two terms are both of the form:  "A op B" with the same
/// A and B values but different operators and if the operators are
/// compatible (if one is = and the other is <, for example) then
/// add a new virtual AND term to pWC that is the combination of the
/// two.
///
/// Some examples:
///
///    x<y OR x=y    -->     x<=y
///    x=y OR x=y    -->     x=y
///    x<=y OR x<y   -->     x<=y
///
/// The following is NOT generated:
///
///    x<y OR x>y    -->     x!=y
fn whereCombineDisjuncts(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pSrc: SrcListId,
    wc: WcId,
    pOne: TermId,
    pTwo: TermId,
) {
    let (o, t) = (*wi.t(pOne), *wi.t(pTwo));
    let mut eOp: u16 = o.eOperator | t.eOperator;

    if ((o.wtFlags | t.wtFlags) & TERM_VNULL) != 0 {
        return;
    }
    if (o.eOperator & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (t.eOperator & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (eOp & (WO_EQ | WO_LT | WO_LE)) != eOp && (eOp & (WO_EQ | WO_GT | WO_GE)) != eOp {
        return;
    }
    let pA = o.pExpr;
    let pB = t.pExpr;
    let (al, ar) = (db.ex(pA).pLeft, db.ex(pA).pRight);
    let (bl, br) = (db.ex(pB).pLeft, db.ex(pB).pRight);
    if sqlite3ExprCompare(db, false, al, bl, -1) != 0 {
        return;
    }
    if sqlite3ExprCompare(db, false, ar, br, -1) != 0 {
        return;
    }
    if db.ex(pA).HasProperty(EP_Commuted) != db.ex(pB).HasProperty(EP_Commuted) {
        return;
    }
    // If we reach this point, it means the two subterms can be combined
    if (eOp & eOp.wrapping_sub(1)) != 0 {
        if eOp & (WO_LT | WO_LE) != 0 {
            eOp = WO_LE;
        } else {
            debug_assert!(eOp & (WO_GT | WO_GE) != 0);
            eOp = WO_GE;
        }
    }
    let pNew = sqlite3ExprDup(db, pA, 0);
    if pNew.is_null() {
        return;
    }
    let mut op = TK_EQ;
    while eOp != (WO_EQ << (op - TK_EQ)) {
        debug_assert!(op < TK_GE);
        op += 1;
    }
    db.exm(pNew).op = op;
    let idxNew = whereClauseInsert(db, wi, wc, pNew, TERM_VIRTUAL | TERM_DYNAMIC);
    exprAnalyze(db, wi, pSrc, wc, idxNew);
}

/// Analyze a term that consists of two or more OR-connected
/// subterms.  So in:
///
///     ... WHERE  (a=5) AND (b=7 OR c=9 OR d=13) AND (d=13)
///                          ^^^^^^^^^^^^^^^^^^^^
///
/// This routine analyzes terms such as the middle term in the above
/// example.  A WhereOrTerm object is computed and attached to the term
/// under analysis, regardless of the outcome of the analysis.  Hence:
///
///     WhereTerm.wtFlags   |=  TERM_ORINFO
///     WhereTerm.u.pOrInfo  =  a dynamically allocated WhereOrTerm object
///
/// The term being analyzed must have two or more of OR-connected subterms.
/// A single subterm might be a set of AND-connected sub-subterms.
/// Examples of terms under analysis:
///
///     (A)     t1.x=t2.y OR t1.x=t2.z OR t1.y=15 OR t1.z=t3.a+5
///     (B)     x=expr1 OR expr2=x OR x=expr3
///     (C)     t1.x=t2.y OR (t1.x=t2.z AND t1.y=15)
///     (D)     x=expr1 OR (y>11 AND y<22 AND z LIKE '*hello*')
///     (E)     (p.a=1 AND q.b=2 AND r.c=3) OR (p.x=4 AND q.y=5 AND r.z=6)
///     (F)     x>A OR (x=A AND y>=B)
///
/// CASE 1:
///
/// If all subterms are of the form T.C=expr for some single column of C and
/// a single table T (as shown in example B above) then create a new virtual
/// term that is an equivalent IN expression.  In other words, if the term
/// being analyzed is:
///
///      x = expr1  OR  expr2 = x  OR  x = expr3
///
/// then create a new virtual term like this:
///
///      x IN (expr1,expr2,expr3)
///
/// CASE 2:
///
/// If there are exactly two disjuncts and one side has x>A and the other
/// side has x=A (for the same x and A) then add a new virtual conjunct term
/// to the WHERE clause of the form "x>=A".  Example:
///
///      x>A OR (x=A AND y>B)    adds:    x>=A
///
/// The added conjunct can sometimes be helpful in query planning.
///
/// CASE 3:
///
/// If all subterms are indexable by a single table T, then set
///
///     WhereTerm.eOperator              =  WO_OR
///     WhereTerm.u.pOrInfo->indexable  |=  the cursor number for table T
///
/// A subterm is "indexable" if it is of the form
/// "T.C <op> <expr>" where C is any column of table T and
/// <op> is one of "=", "<", "<=", ">", ">=", "IS NULL", or "IN".
/// A subterm is also indexable if it is an AND of two or more
/// subsubterms at least one of which is indexable.  Indexable AND
/// subterms have their eOperator set to WO_AND and they have
/// u.pAndInfo set to a dynamically allocated WhereAndTerm object.
///
/// From another point of view, "indexable" means that the subterm could
/// potentially be used with an index if an appropriate index exists.
/// This analysis does not consider whether or not the index exists; that
/// is decided elsewhere.  This analysis only looks at whether subterms
/// appropriate for indexing exist.
///
/// All examples A through E above satisfy case 3.  But if a term
/// also satisfies case 1 (such as B) we know that the optimizer will
/// always prefer case 1, so in that case we pretend that case 3 is not
/// satisfied.
///
/// It might be the case that multiple tables are indexable.  For example,
/// (E) above is indexable on tables P, Q, and R.
///
/// Terms that satisfy case 3 are candidates for lookup by using
/// separate indices to find rowids for each subterm and composing
/// the union of all rowids using a RowSet object.  This is similar
/// to "bitmap indices" in other database engines.
///
/// OTHERWISE:
///
/// If none of cases 1, 2, or 3 apply, then leave the eOperator set to
/// zero.  This term is not useful for search.
fn exprAnalyzeOrTerm(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pSrc: SrcListId,
    wc: WcId,
    idxTerm: i32,
) {
    let tTerm = TermId::new(wc, idxTerm as usize);
    let pExpr = wi.t(tTerm).pExpr;

    // Break the OR clause into its separate subterms.  The subterms are
    // stored in a WhereClause structure containing within the WhereOrInfo
    // object that is attached to the original OR clause term.
    debug_assert!((wi.t(tTerm).wtFlags & (TERM_DYNAMIC | TERM_ORINFO | TERM_ANDINFO)) == 0);
    debug_assert!(db.ex(pExpr).op == TK_OR);
    let pOrWc: WcId = wi.aWC.len();
    let mut nwc = WhereClause::default();
    sqlite3WhereClauseInit(&mut nwc);
    wi.aWC.push(nwc);
    let iOrInfo = wi.aOrInfo.len();
    wi.aOrInfo.push(WhereOrInfo {
        wc: pOrWc,
        indexable: 0,
    });
    wi.tm(tTerm).u.pOrInfo = Some(iOrInfo);
    wi.tm(tTerm).wtFlags |= TERM_ORINFO;
    sqlite3WhereSplit(db, wi, pOrWc, pExpr, TK_OR);
    sqlite3WhereExprAnalyze(db, wi, pSrc, pOrWc);
    if db.mallocFailed != 0 {
        return;
    }
    debug_assert!(wi.wc(pOrWc).nTerm() >= 2);

    // Compute the set of tables that might satisfy cases 1 or 3.
    let mut indexable: Bitmask = !0;
    let mut chngToIN: Bitmask = !0;
    let nOr = wi.wc(pOrWc).nTerm();
    let mut i = nOr - 1;
    let mut k = 0usize; // index of pOrTerm
    while i >= 0 && indexable != 0 {
        let pOrTerm = TermId::new(pOrWc, k);
        let ot = *wi.t(pOrTerm);
        if (ot.eOperator & WO_SINGLE) == 0 {
            debug_assert!((ot.wtFlags & (TERM_ANDINFO | TERM_ORINFO)) == 0);
            chngToIN = 0;
            let mut b: Bitmask = 0;
            let pAndWC: WcId = wi.aWC.len();
            let mut nwc = WhereClause::default();
            sqlite3WhereClauseInit(&mut nwc);
            wi.aWC.push(nwc);
            {
                let t = wi.tm(pOrTerm);
                t.u.pAndInfo = Some(pAndWC);
                t.wtFlags |= TERM_ANDINFO;
                t.eOperator = WO_AND;
                t.leftCursor = -1;
            }
            sqlite3WhereSplit(db, wi, pAndWC, ot.pExpr, TK_AND);
            sqlite3WhereExprAnalyze(db, wi, pSrc, pAndWC);
            wi.wcm(pAndWC).pOuter = Some(wc);
            if db.mallocFailed == 0 {
                let n = wi.wc(pAndWC).nTerm();
                for j in 0..n as usize {
                    let at = wi.wc(pAndWC).a[j];
                    debug_assert!(at.pExpr.nn());
                    if allowedOp(db.ex(at.pExpr).op) || at.eOperator == WO_AUX {
                        b |= sqlite3WhereGetMask(&mut wi.sMaskSet, at.leftCursor);
                    }
                }
            }
            indexable &= b;
        } else if ot.wtFlags & TERM_COPIED != 0 {
            // Skip this term for now.  We revisit it when we process the
            // corresponding TERM_VIRTUAL term
        } else {
            let mut b = sqlite3WhereGetMask(&mut wi.sMaskSet, ot.leftCursor);
            if ot.wtFlags & TERM_VIRTUAL != 0 {
                let pOther = wi.wc(pOrWc).a[ot.iParent as usize];
                b |= sqlite3WhereGetMask(&mut wi.sMaskSet, pOther.leftCursor);
            }
            indexable &= b;
            if (ot.eOperator & WO_EQ) == 0 {
                chngToIN = 0;
            } else {
                chngToIN &= b;
            }
        }
        i -= 1;
        k += 1;
    }

    // Record the set of tables that satisfy case 3.  The set might be
    // empty.
    wi.aOrInfo[iOrInfo].indexable = indexable;
    wi.tm(tTerm).eOperator = WO_OR;
    wi.tm(tTerm).leftCursor = -1;
    if indexable != 0 {
        wi.wcm(wc).hasOr = 1;
    }

    // For a two-way OR, attempt to implementation case 2.
    if indexable != 0 && wi.wc(pOrWc).nTerm() == 2 {
        let mut iOne = 0;
        while let Some(pOne) = whereNthSubterm(wi, TermId::new(pOrWc, 0), iOne) {
            iOne += 1;
            let mut iTwo = 0;
            while let Some(pTwo) = whereNthSubterm(wi, TermId::new(pOrWc, 1), iTwo) {
                iTwo += 1;
                whereCombineDisjuncts(db, wi, pSrc, wc, pOne, pTwo);
            }
        }
    }

    // chngToIN holds a set of tables that *might* satisfy case 1.  But
    // we have to do some additional checking to see if case 1 really
    // is satisfied.
    //
    // chngToIN will hold either 0, 1, or 2 bits.  The 0-bit case means
    // that there is no possibility of transforming the OR clause into an
    // IN operator because one or more terms in the OR clause contain
    // something other than == on a column in the single table.  The 1-bit
    // case means that every term of the OR clause is of the form
    // "table.column=expr" for some single table.  The one bit that is set
    // will correspond to the common table.  We still need to check to make
    // sure the same column is used on all terms.  The 2-bit case is when
    // the all terms are of the form "table1.column=table2.column".  It
    // might be possible to form an IN operator with either table1.column
    // or table2.column as the LHS if either is common to every term of
    // the OR clause.
    //
    // Note that terms of the form "table.column1=table.column2" (the
    // same table on both sizes of the ==) cannot be optimized.
    if chngToIN != 0 {
        let mut okToChngToIN = 0; // True if the conversion to IN is valid
        let mut iColumn = -1; // Column index on lhs of IN operator
        let mut iCursor = -1; // Table cursor common to all terms
        let nOr = wi.wc(pOrWc).nTerm();

        // Search for a table and column that appears on one side or the
        // other of the == operator in every subterm.  That table and column
        // will be recorded in iCursor and iColumn.  There might not be any
        // such table and column.  Set okToChngToIN if an appropriate table
        // and column is found but leave okToChngToIN false if not found.
        let mut j = 0;
        while j < 2 && okToChngToIN == 0 {
            let mut pLeft = ExprId::NULL;
            let mut k = 0usize;
            let mut i = nOr - 1;
            while i >= 0 {
                let pOrTerm = TermId::new(pOrWc, k);
                debug_assert!(wi.t(pOrTerm).eOperator & WO_EQ != 0);
                wi.tm(pOrTerm).wtFlags &= !TERM_OK;
                let ot = *wi.t(pOrTerm);
                if ot.leftCursor == iCursor {
                    // This is the 2-bit case and we are on the second
                    // iteration and current term is from the first
                    // iteration.  So skip this term.
                    debug_assert!(j == 1);
                    i -= 1;
                    k += 1;
                    continue;
                }
                if (chngToIN & sqlite3WhereGetMask(&mut wi.sMaskSet, ot.leftCursor)) == 0 {
                    // This term must be of the form t1.a==t2.b where t2 is
                    // in the chngToIN set but t1 is not.  This term will be
                    // either preceded or followed by an inverted copy
                    // (t2.b==t1.a).  Skip this term and use its inversion.
                    debug_assert!(ot.wtFlags & (TERM_COPIED | TERM_VIRTUAL) != 0);
                    i -= 1;
                    k += 1;
                    continue;
                }
                debug_assert!((ot.eOperator & (WO_OR | WO_AND)) == 0);
                iColumn = ot.u.x.leftColumn;
                iCursor = ot.leftCursor;
                pLeft = db.ex(ot.pExpr).pLeft;
                break;
            }
            if i < 0 {
                // No candidate table+column was found.  This can only
                // occur on the second iteration
                debug_assert!(j == 1);
                break;
            }

            // We have found a candidate table and column.  Check to see if
            // that table and column is common to every term in the OR clause
            okToChngToIN = 1;
            while i >= 0 && okToChngToIN != 0 {
                let pOrTerm = TermId::new(pOrWc, k);
                let ot = *wi.t(pOrTerm);
                debug_assert!(ot.eOperator & WO_EQ != 0);
                debug_assert!((ot.eOperator & (WO_OR | WO_AND)) == 0);
                if ot.leftCursor != iCursor {
                    wi.tm(pOrTerm).wtFlags &= !TERM_OK;
                } else if ot.u.x.leftColumn != iColumn
                    || (iColumn == XN_EXPR as i32 && {
                        let l = db.ex(ot.pExpr).pLeft;
                        sqlite3ExprCompare(db, true, l, pLeft, -1) != 0
                    })
                {
                    okToChngToIN = 0;
                } else {
                    // If the right-hand side is also a column, then the
                    // affinities of both right and left sides must be such
                    // that no type conversions are required on the right.
                    // (Ticket #2249)
                    let (l, r) = (db.ex(ot.pExpr).pLeft, db.ex(ot.pExpr).pRight);
                    let affRight = sqlite3ExprAffinity(db, r);
                    let affLeft = sqlite3ExprAffinity(db, l);
                    if affRight != 0 && affRight != affLeft {
                        okToChngToIN = 0;
                    } else {
                        wi.tm(pOrTerm).wtFlags |= TERM_OK;
                    }
                }
                i -= 1;
                k += 1;
            }
            j += 1;
        }

        // At this point, okToChngToIN is true if original pTerm is a
        // candidate to satisfy case 1, though we are not yet certain that
        // the collating sequences are all compatible.  Try to construct a
        // new virtual term that is pTerm converted from an OR operator
        // into an IN operator.
        //
        // During construction, verify that the collating sequences on all
        // subterms of the OR are compatible.  Omit the construction of the
        // new IN operator if there are any collating sequence mismatches.
        if okToChngToIN != 0 {
            let mut pList = ExprListId::NULL; // The RHS of the IN operator
            let mut pLeft = ExprId::NULL; // The LHS of the IN operator
            let mut pCollSeq: Option<CollSeqRef> = None; // Collating sequence to use
            let pNew: ExprId; // The complete IN operator

            for k in 0..nOr as usize {
                let ot = wi.wc(pOrWc).a[k];
                if (ot.wtFlags & TERM_OK) == 0 {
                    continue;
                }
                debug_assert!(ot.eOperator & WO_EQ != 0);
                debug_assert!(ot.leftCursor == iCursor);
                debug_assert!(ot.u.x.leftColumn == iColumn);
                let pThis = ot.pExpr;
                let r = db.ex(pThis).pRight;
                let pDup = sqlite3ExprDup(db, r, 0);
                pList = sqlite3ExprListAppend(db, pList, pDup);
                if pLeft.is_null() {
                    pLeft = db.ex(pThis).pLeft;
                    pCollSeq = sqlite3ExprCompareCollSeq(db, pThis);
                } else {
                    let c = sqlite3ExprCompareCollSeq(db, pThis);
                    if !coll_ptr_eq(&pCollSeq, &c) {
                        pLeft = ExprId::NULL; // Collating sequence mismatch
                        break;
                    }
                }
            }
            if pLeft.is_null() {
                pNew = ExprId::NULL; // Collating sequence mismatch
            } else {
                let mut pDup = sqlite3ExprDup(db, pLeft, 0);
                let c = sqlite3ExprCollSeq(db, pDup);
                if !coll_ptr_eq(&c, &pCollSeq) && pCollSeq.is_some() {
                    let zName = pCollSeq.as_ref().unwrap().zName.clone();
                    pDup = sqlite3ExprAddCollateString(db, pDup, Some(&zName[..]));
                }
                pNew = sqlite3PExpr(db, TK_IN as i32, pDup, ExprId::NULL);
            }
            if pNew.nn() {
                transferJoinMarkings(db, pNew, pExpr);
                db.exm(pNew).x.pList = pList;
                let idxNew = whereClauseInsert(db, wi, wc, pNew, TERM_VIRTUAL | TERM_DYNAMIC);
                exprAnalyze(db, wi, pSrc, wc, idxNew);
                // pTerm = &pWC->a[idxTerm]; // would be needed if pTerm where reused
                markTermAsChild(wi, wc, idxNew, idxTerm);
            } else {
                sqlite3ExprListDelete(db, pList);
            }
        }
    }
}

/// We already know that pExpr is a binary operator where both operands are
/// column references.  This routine checks to see if pExpr is an
/// equivalence relation:
///   1.  The SQLITE_Transitive optimization must be enabled
///   2.  Must be either an == or an IS operator
///   3.  Not originating in the ON clause of an OUTER JOIN
///   4.  The operator is not IS or else the query does not contain RIGHT JOIN
///   5.  The affinities of A and B must be compatible
///   6.  Both operands use the same collating sequence, and they must not
///       use explicit COLLATE clauses.
/// If this routine returns TRUE, that means that the RHS can be substituted
/// for the LHS anyplace else in the WHERE clause where the LHS column occurs.
/// This is an optimization.  No harm comes from returning 0.  But if 1 is
/// returned when it should not be, then incorrect answers might result.
fn termIsEquivalence(db: &mut Sqlite3, pExpr: ExprId, pSrc: SrcListId) -> i32 {
    if !db.OptimizationEnabled(SQLITE_Transitive) {
        return 0; // (1)
    }
    let (op, l, r) = {
        let e = db.ex(pExpr);
        (e.op, e.pLeft, e.pRight)
    };
    if op != TK_EQ && op != TK_IS {
        return 0; // (2)
    }
    if db.ex(pExpr).HasProperty(EP_OuterON | EP_Collate) {
        return 0; // (3)
    }
    debug_assert!(pSrc.nn());
    if op == TK_IS && db.src(pSrc).nSrc() >= 2 && (db.src(pSrc).a[0].fg.jointype & JT_LTORJ) != 0 {
        return 0; // (4)
    }
    let aff1 = sqlite3ExprAffinity(db, l);
    let aff2 = sqlite3ExprAffinity(db, r);
    if aff1 != aff2 && (!sqlite3IsNumericAffinity(aff1) || !sqlite3IsNumericAffinity(aff2)) {
        return 0; // (5)
    }
    if sqlite3ExprCollSeqMatch(db, l, r) == 0 {
        return 0; // (6)
    }
    1
}

/// sqlite3IsNumericAffinity(X)
#[inline]
pub(crate) fn sqlite3IsNumericAffinity(x: u8) -> bool {
    x >= SQLITE_AFF_NUMERIC
}

/// Recursively walk the expressions of a SELECT statement and generate
/// a bitmask indicating which tables are used in that expression
/// tree.
fn exprSelectUsage(db: &mut Sqlite3, pMaskSet: &mut WhereMaskSet, pS: SelectId) -> Bitmask {
    let mut mask: Bitmask = 0;
    let mut pS = pS;
    while pS.nn() {
        let s = db.sel(pS).clone();
        let pSrc = s.pSrc;
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, s.pEList);
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, s.pGroupBy);
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, s.pOrderBy);
        mask |= sqlite3WhereExprUsage(db, pMaskSet, s.pWhere);
        mask |= sqlite3WhereExprUsage(db, pMaskSet, s.pHaving);
        if pSrc.nn() {
            let n = db.src(pSrc).nSrc();
            for i in 0..n as usize {
                let (isSub, sel, isUsing, pOn, isTabFunc, pFuncArg) = {
                    let it = &db.src(pSrc).a[i];
                    (
                        it.fg.isSubquery,
                        it.u4.pSubq.as_ref().map_or(SelectId::NULL, |q| q.pSelect),
                        it.fg.isUsing,
                        it.u3.pOn,
                        it.fg.isTabFunc,
                        it.u1.pFuncArg,
                    )
                };
                if isSub != 0 {
                    mask |= exprSelectUsage(db, pMaskSet, sel);
                }
                if isUsing == 0 {
                    mask |= sqlite3WhereExprUsage(db, pMaskSet, pOn);
                }
                if isTabFunc != 0 {
                    mask |= sqlite3WhereExprListUsage(db, pMaskSet, pFuncArg);
                }
            }
        }
        pS = s.pPrior;
    }
    mask
}

/// Expression pExpr is one operand of a comparison operator that might
/// be useful for indexing.  This routine checks to see if pExpr appears
/// in any index.  Return TRUE (1) if pExpr is an indexed term and return
/// FALSE (0) if not.  If TRUE is returned, also set aiCurCol[0] to the
/// cursor number of the table that is indexed and aiCurCol[1] to the
/// column number of the column that is indexed, or XN_EXPR (-2) if an
/// expression is being indexed.
///
/// If pExpr is a TK_COLUMN column reference, then this routine always
/// returns true even if that particular column is not indexed, because the
/// column might be added to an automatic index later.
fn exprMightBeIndexed2(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pFrom: SrcListId,
    aiCurCol: &mut [i32; 2],
    pExpr: ExprId,
    j: i32,
) -> i32 {
    let mut j = j;
    let nSrc = db.src(pFrom).nSrc();
    loop {
        let iCur = db.src(pFrom).a[j as usize].iCursor;
        let tab = item_tab(db, SrcItemId(pFrom, j as u32));
        let idxs: Vec<IndexRef> = tab.borrow().pIndex.clone();
        for pIdx in idxs.iter() {
            if pIdx.borrow().aColExpr.is_none() {
                continue;
            }
            let nKeyCol = pIdx.borrow().nKeyCol as i32;
            for i in 0..nKeyCol {
                if pIdx.borrow().aiColumn[i as usize] != XN_EXPR {
                    continue;
                }
                let e = idx_col_expr(db, wi, pIdx, i);
                if sqlite3ExprCompareSkip(db, pExpr, e, iCur) == 0 && sqlite3ExprIsConstant(db, false, e) == 0 {
                    aiCurCol[0] = iCur;
                    aiCurCol[1] = XN_EXPR as i32;
                    return 1;
                }
            }
        }
        j += 1;
        if j >= nSrc {
            break;
        }
    }
    0
}

fn exprMightBeIndexed(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pFrom: SrcListId,
    aiCurCol: &mut [i32; 2],
    pExpr: ExprId,
    op: u8,
) -> i32 {
    let mut pExpr = pExpr;
    // If this expression is a vector to the left or right of a
    // inequality constraint (>, <, >= or <=), perform the processing
    // on the first element of the vector.
    debug_assert!(op <= TK_GE);
    if db.ex(pExpr).op == TK_VECTOR && (op >= TK_GT && op <= TK_GE) {
        let l = db.ex(pExpr).x.pList;
        pExpr = db.el(l).a[0].pExpr;
    }

    if db.ex(pExpr).op == TK_COLUMN {
        aiCurCol[0] = db.ex(pExpr).iTable;
        aiCurCol[1] = db.ex(pExpr).iColumn as i32;
        return 1;
    }

    let nSrc = db.src(pFrom).nSrc();
    for i in 0..nSrc {
        let tab = item_tab(db, SrcItemId(pFrom, i as u32));
        let has = tab
            .borrow()
            .pIndex
            .iter()
            .any(|x| x.borrow().aColExpr.is_some());
        if has {
            return exprMightBeIndexed2(db, wi, pFrom, aiCurCol, pExpr, i);
        }
    }
    0
}

/// The input to this routine is an WhereTerm structure with only the
/// "pExpr" field filled in.  The job of this routine is to analyze the
/// subexpression and populate all the other fields of the WhereTerm
/// structure.
///
/// If the expression is of the form "<expr> <op> X" it gets commuted
/// to the standard form of "X <op> <expr>".
///
/// If the expression is of the form "X <op> Y" where both X and Y are
/// columns, then the original expression is unchanged and a new virtual
/// term of the form "Y <op> X" is added to the WHERE clause and
/// analyzed separately.  The original term is marked with TERM_COPIED
/// and the new term is marked with TERM_DYNAMIC (because it's pExpr
/// needs to be freed with the WhereClause) and TERM_VIRTUAL (because it
/// is a commuted copy of a prior term.)  The original term has nChild=1
/// and the copy has idxParent set to the index of the original term.
pub(crate) fn exprAnalyze(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pSrc: SrcListId,
    wc: WcId,
    idxTerm: i32,
) {
    let tTerm = TermId::new(wc, idxTerm as usize);
    let mut prereqLeft: Bitmask;
    let mut prereqAll: Bitmask;
    let mut extraRight: Bitmask = 0;
    let mut pStr1 = ExprId::NULL;
    let mut isComplete = 0;
    let mut noCase = 0;
    let mut op: u8;
    let mut eOp2: u8 = 0;
    let nLeft: i32;

    if db.mallocFailed != 0 {
        return;
    }
    debug_assert!(wi.wc(wc).nTerm() > idxTerm);
    let mut pExpr = wi.t(tTerm).pExpr;
    debug_assert!(pExpr.nn());
    debug_assert!(db.ex(pExpr).op != TK_AS && db.ex(pExpr).op != TK_COLLATE);
    'restart: loop {
        wi.sMaskSet.bVarSelect = 0;
        let eLeft = db.ex(pExpr).pLeft;
        prereqLeft = sqlite3WhereExprUsage(db, &mut wi.sMaskSet, eLeft);
        op = db.ex(pExpr).op;
        if op == TK_IN {
            debug_assert!(db.ex(pExpr).pRight.is_null());
            if sqlite3ExprCheckIN(db, pExpr) != 0 {
                return;
            }
            let (useSel, sel, list) = {
                let e = db.ex(pExpr);
                (e.UseXSelect(), e.x.pSelect, e.x.pList)
            };
            let pr = if useSel {
                exprSelectUsage(db, &mut wi.sMaskSet, sel)
            } else {
                sqlite3WhereExprListUsage(db, &mut wi.sMaskSet, list)
            };
            wi.tm(tTerm).prereqRight = pr;
            prereqAll = prereqLeft | pr;
        } else {
            let r = db.ex(pExpr).pRight;
            let pr = sqlite3WhereExprUsage(db, &mut wi.sMaskSet, r);
            wi.tm(tTerm).prereqRight = pr;
            let complex = {
                let e = db.ex(pExpr);
                e.pLeft.is_null()
                    || e.HasProperty(EP_xIsSelect | EP_IfNullRow)
                    || e.x.pList.nn()
                    || e.x.pSelect.nn()
            };
            if complex {
                prereqAll = sqlite3WhereExprUsageNN(db, &mut wi.sMaskSet, pExpr);
            } else {
                prereqAll = prereqLeft | pr;
            }
        }
        if wi.sMaskSet.bVarSelect != 0 {
            wi.tm(tTerm).wtFlags |= TERM_VARSELECT;
        }

        if db.ex(pExpr).HasProperty(EP_OuterON | EP_InnerON) {
            let iJoin = db.ex(pExpr).w.iJoin;
            let x = sqlite3WhereGetMask(&mut wi.sMaskSet, iJoin);
            if db.ex(pExpr).HasProperty(EP_OuterON) {
                prereqAll |= x;
                extraRight = x.wrapping_sub(1); // ON clause terms may not be used with an index
            // on left table of a LEFT JOIN.  Ticket #3015
            } else if (prereqAll >> 1) >= x {
                db.exm(pExpr).ClearProperty(EP_InnerON);
            }
        }
        {
            let t = wi.tm(tTerm);
            t.prereqAll = prereqAll;
            t.leftCursor = -1;
            t.iParent = -1;
            t.eOperator = 0;
        }
        if allowedOp(op) {
            let mut aiCurCol = [0i32; 2];
            let (el, er) = {
                let e = db.ex(pExpr);
                (e.pLeft, e.pRight)
            };
            let mut pLeft = sqlite3ExprSkipCollate(db, el);
            let pRight = if er.nn() {
                sqlite3ExprSkipCollate(db, er)
            } else {
                ExprId::NULL
            };
            let opMask: u16 = if (wi.t(tTerm).prereqRight & prereqLeft) == 0 {
                WO_ALL
            } else {
                WO_EQUIV
            };

            let iField = wi.t(tTerm).u.x.iField;
            if iField > 0 {
                debug_assert!(op == TK_IN);
                debug_assert!(db.ex(pLeft).op == TK_VECTOR);
                let l = db.ex(pLeft).x.pList;
                pLeft = db.el(l).a[(iField - 1) as usize].pExpr;
            }

            if exprMightBeIndexed(db, wi, pSrc, &mut aiCurCol, pLeft, op) != 0 {
                let t = wi.tm(tTerm);
                t.leftCursor = aiCurCol[0];
                debug_assert!((t.eOperator & (WO_OR | WO_AND)) == 0);
                t.u.x.leftColumn = aiCurCol[1];
                t.eOperator = operatorMask(op) & opMask;
            }
            if op == TK_IS {
                wi.tm(tTerm).wtFlags |= TERM_IS;
            }
            if pRight.nn()
                && exprMightBeIndexed(db, wi, pSrc, &mut aiCurCol, pRight, op) != 0
                && !db.ex(pRight).HasProperty(EP_FixedCol)
            {
                let pNew: TermId;
                let pDup: ExprId;
                let mut eExtraOp: u16 = 0; // Extra bits for pNew->eOperator
                debug_assert!(wi.t(tTerm).u.x.iField == 0);
                if wi.t(tTerm).leftCursor >= 0 {
                    pDup = sqlite3ExprDup(db, pExpr, 0);
                    if db.mallocFailed != 0 {
                        sqlite3ExprDelete(db, pDup);
                        return;
                    }
                    let idxNew = whereClauseInsert(db, wi, wc, pDup, TERM_VIRTUAL | TERM_DYNAMIC);
                    if idxNew == 0 {
                        return;
                    }
                    pNew = TermId::new(wc, idxNew as usize);
                    markTermAsChild(wi, wc, idxNew, idxTerm);
                    if op == TK_IS {
                        wi.tm(pNew).wtFlags |= TERM_IS;
                    }
                    wi.tm(tTerm).wtFlags |= TERM_COPIED;
                    debug_assert!(wi.pTabList.nn());
                    let tl = wi.pTabList;
                    if termIsEquivalence(db, pDup, tl) != 0 {
                        wi.tm(tTerm).eOperator |= WO_EQUIV;
                        eExtraOp = WO_EQUIV;
                    }
                } else {
                    pDup = pExpr;
                    pNew = tTerm;
                }
                let f = exprCommute(db, pDup);
                let dop = db.ex(pDup).op;
                let t = wi.tm(pNew);
                t.wtFlags |= f;
                t.leftCursor = aiCurCol[0];
                t.u.x.leftColumn = aiCurCol[1];
                t.prereqRight = prereqLeft | extraRight;
                t.prereqAll = prereqAll;
                t.eOperator = (operatorMask(dop).wrapping_add(eExtraOp)) & opMask;
            } else if op == TK_ISNULL
                && !db.ex(pExpr).HasProperty(EP_OuterON)
                && 0 == sqlite3ExprCanBeNull(db, pLeft)
            {
                debug_assert!(!db.ex(pExpr).HasProperty(EP_IntValue));
                {
                    let e = db.exm(pExpr);
                    e.op = TK_TRUEFALSE; // See tag-20230504-1
                    e.u.zToken = Some(b"false".to_vec());
                    e.SetProperty(EP_IsFalse);
                }
                let t = wi.tm(tTerm);
                t.prereqAll = 0;
                t.eOperator = 0;
            }
        }
        // If a term is the BETWEEN operator, create two new virtual terms
        // that define the range that the BETWEEN implements.  For example:
        //
        //      a BETWEEN b AND c
        //
        // is converted into:
        //
        //      (a BETWEEN b AND c) AND (a>=b) AND (a<=c)
        //
        // The two new terms are added onto the end of the WhereClause
        // object.  The new terms are "dynamic" and are children of the
        // original BETWEEN term.  That means that if the BETWEEN term is
        // coded, the children are skipped.  Or, if the children are
        // satisfied by an index, the original BETWEEN term is skipped.
        else if db.ex(pExpr).op == TK_BETWEEN && wi.wc(wc).op == TK_AND {
            static ops: [u8; 2] = [TK_GE, TK_LE];
            let pList = db.ex(pExpr).x.pList;
            debug_assert!(pList.nn());
            debug_assert!(db.el(pList).nExpr() == 2);
            debug_assert!(wi.wc(wc).a[idxTerm as usize].nChild == 0);
            for i in 0..2usize {
                let l = db.ex(pExpr).pLeft;
                let a = sqlite3ExprDup(db, l, 0);
                let li = db.el(pList).a[i].pExpr;
                let b = sqlite3ExprDup(db, li, 0);
                let pNewExpr = sqlite3PExpr(db, ops[i] as i32, a, b);
                transferJoinMarkings(db, pNewExpr, pExpr);
                let idxNew = whereClauseInsert(db, wi, wc, pNewExpr, TERM_VIRTUAL | TERM_DYNAMIC);
                exprAnalyze(db, wi, pSrc, wc, idxNew);
                markTermAsChild(wi, wc, idxNew, idxTerm);
            }
        }
        // Analyze a term that is composed of two or more subterms connected
        // by an OR operator.
        else if db.ex(pExpr).op == TK_OR && !db.ex(pExpr).HasProperty(EP_Collate) {
            let pAlt = sqlite3ExprSimplifiedAndOr(db, pExpr);
            if pAlt != pExpr {
                pExpr = sqlite3ExprSkipCollateAndLikely(db, pAlt);
                wi.tm(tTerm).pExpr = pExpr;
                continue 'restart;
            }
            debug_assert!(wi.wc(wc).op == TK_AND);
            exprAnalyzeOrTerm(db, wi, pSrc, wc, idxTerm);
        }
        // The form "x IS NOT NULL" can sometimes be evaluated more
        // efficiently as "x>NULL" if x is not an INTEGER PRIMARY KEY.  So
        // construct a virtual term of that form.
        //
        // The virtual term must be tagged with TERM_VNULL.
        else if db.ex(pExpr).op == TK_NOTNULL {
            let pLeft = db.ex(pExpr).pLeft;
            if db.ex(pLeft).op == TK_COLUMN
                && db.ex(pLeft).iColumn >= 0
                && !db.ex(pExpr).HasProperty(EP_OuterON)
            {
                let a = sqlite3ExprDup(db, pLeft, 0);
                let b = sqlite3ExprAlloc(db, TK_NULL as i32, &Token::default(), 0);
                let pNewExpr = sqlite3PExpr(db, TK_GT as i32, a, b);

                let idxNew = whereClauseInsert(
                    db,
                    wi,
                    wc,
                    pNewExpr,
                    TERM_VIRTUAL | TERM_DYNAMIC | TERM_VNULL,
                );
                if idxNew != 0 {
                    let (lt, lc) = (db.ex(pLeft).iTable, db.ex(pLeft).iColumn as i32);
                    let pNewTerm = TermId::new(wc, idxNew as usize);
                    {
                        let t = wi.tm(pNewTerm);
                        t.prereqRight = 0;
                        t.leftCursor = lt;
                        t.u.x.leftColumn = lc;
                        t.eOperator = WO_GT;
                    }
                    markTermAsChild(wi, wc, idxNew, idxTerm);
                    wi.tm(tTerm).wtFlags |= TERM_COPIED;
                    let pa = wi.t(tTerm).prereqAll;
                    wi.tm(pNewTerm).prereqAll = pa;
                }
            }
        }
        // Add constraints to reduce the search space on a LIKE or GLOB
        // operator.
        //
        // A like pattern of the form "x LIKE 'aBc%'" is changed into
        // constraints
        //
        //          x>='ABC' AND x<'abd' AND x LIKE 'aBc%'
        //
        // The last character of the prefix "abc" is incremented to form the
        // termination condition "abd".  If case is not significant (the
        // default for LIKE) then the lower-bound is made all uppercase and
        // the upper-bound is made all lowercase so that the bounds also work
        // when comparing BLOBs.
        else if db.ex(pExpr).op == TK_FUNCTION
            && wi.wc(wc).op == TK_AND
            && isLikeOrGlob(db, pExpr, &mut pStr1, &mut isComplete, &mut noCase) != 0
        {
            let wtFlags: u16 = TERM_LIKEOPT | TERM_VIRTUAL | TERM_DYNAMIC;

            let l = db.ex(pExpr).x.pList;
            let pLeft = db.el(l).a[1].pExpr;
            let pStr2 = sqlite3ExprDup(db, pStr1, 0);

            // Convert the lower bound to upper-case and the upper bound to
            // lower-case (upper-case is less than lower-case in ASCII) so
            // that the range constraints also work for BLOBs
            if noCase != 0 && db.mallocFailed == 0 {
                wi.tm(tTerm).wtFlags |= TERM_LIKE;
                let z1: Vec<u8> = db.ex(pStr1).token().to_vec();
                let mut u1 = z1.clone();
                let mut u2 = db.ex(pStr2).token().to_vec();
                for (i, &c) in z1.iter().enumerate() {
                    if c == 0 {
                        break;
                    }
                    u1[i] = sqlite3Toupper(c);
                    u2[i] = sqlite3Tolower(c);
                }
                db.exm(pStr1).u.zToken = Some(u1);
                db.exm(pStr2).u.zToken = Some(u2);
            }

            if db.mallocFailed == 0 {
                let mut z2: Vec<u8> = db.ex(pStr2).token().to_vec();
                // pC: Last character before the first wildcard
                let mut pC: usize = z2.len().wrapping_sub(1);
                if !z2.is_empty() {
                    if noCase != 0 {
                        // The point is to increment the last character before
                        // the first wildcard.  But if we increment '@', that
                        // will push it into the alphabetic range where case
                        // conversions will mess up the inequality.  To avoid
                        // this, make sure to also run the full LIKE on all
                        // candidate expressions by clearing the isComplete flag
                        if z2[pC] == b'A' - 1 {
                            isComplete = 0;
                        }
                        z2[pC] = sqlite3UpperToLower[z2[pC] as usize];
                    }

                    // Increment the value of the last utf8 character in the
                    // prefix.
                    while z2[pC] == 0xBF && pC > 0 {
                        z2[pC] = 0x80;
                        pC -= 1;
                    }
                    debug_assert!(z2[pC] != 0xFF); // isLikeOrGlob() guarantees this
                    z2[pC] = z2[pC].wrapping_add(1);
                }
                db.exm(pStr2).u.zToken = Some(z2);
            }
            let zCollSeqName: &[u8] = if noCase != 0 {
                b"NOCASE"
            } else {
                sqlite3StrBINARY
            };
            let mut pNewExpr1 = sqlite3ExprDup(db, pLeft, 0);
            let c1 = sqlite3ExprAddCollateString(db, pNewExpr1, Some(zCollSeqName));
            pNewExpr1 = sqlite3PExpr(db, TK_GE as i32, c1, pStr1);
            transferJoinMarkings(db, pNewExpr1, pExpr);
            let idxNew1 = whereClauseInsert(db, wi, wc, pNewExpr1, wtFlags);
            let mut pNewExpr2 = sqlite3ExprDup(db, pLeft, 0);
            let c2 = sqlite3ExprAddCollateString(db, pNewExpr2, Some(zCollSeqName));
            pNewExpr2 = sqlite3PExpr(db, TK_LT as i32, c2, pStr2);
            transferJoinMarkings(db, pNewExpr2, pExpr);
            let idxNew2 = whereClauseInsert(db, wi, wc, pNewExpr2, wtFlags);
            exprAnalyze(db, wi, pSrc, wc, idxNew1);
            exprAnalyze(db, wi, pSrc, wc, idxNew2);
            if isComplete != 0 {
                markTermAsChild(wi, wc, idxNew1, idxTerm);
                markTermAsChild(wi, wc, idxNew2, idxTerm);
            }
        }
        break;
    }

    // If there is a vector == or IS term - e.g. "(a, b) == (?, ?)" - create
    // new terms for each component comparison - "a = ?" and "b = ?".  The
    // new terms completely replace the original vector comparison, which is
    // no longer used.
    //
    // This is only required if at least one side of the comparison
    // operation is not a sub-select.
    //
    // tag-20220128a
    let eop = db.ex(pExpr).op;
    let (el, er) = {
        let e = db.ex(pExpr);
        (e.pLeft, e.pRight)
    };
    if (eop == TK_EQ || eop == TK_IS)
        && {
            nLeft = sqlite3ExprVectorSize(db, el);
            nLeft > 1
        }
        && sqlite3ExprVectorSize(db, er) == nLeft
        && ((db.ex(el).flags & EP_xIsSelect) == 0 || (db.ex(er).flags & EP_xIsSelect) == 0)
        && wi.wc(wc).op == TK_AND
    {
        for i in 0..nLeft {
            let pLeft = sqlite3ExprForVectorField(db, el, i, nLeft);
            let pRight = sqlite3ExprForVectorField(db, er, i, nLeft);

            let pNew = sqlite3PExpr(db, eop as i32, pLeft, pRight);
            transferJoinMarkings(db, pNew, pExpr);
            let idxNew = whereClauseInsert(db, wi, wc, pNew, TERM_DYNAMIC | TERM_SLICE);
            exprAnalyze(db, wi, pSrc, wc, idxNew);
        }
        let t = wi.tm(tTerm);
        t.wtFlags |= TERM_CODED | TERM_VIRTUAL; // Disable the original
        t.eOperator = WO_ROWVAL;
    }
    // If there is a vector IN term - e.g. "(a, b) IN (SELECT ...)" - create
    // a virtual term for each vector component. The expression object
    // used by each such virtual term is pExpr (the full vector IN(...)
    // expression). The WhereTerm.u.x.iField variable identifies the index
    // within the vector on the LHS that the virtual term represents.
    //
    // This only works if the RHS is a simple SELECT (not a compound) that
    // does not use window functions.
    else if eop == TK_IN
        && wi.t(tTerm).u.x.iField == 0
        && db.ex(el).op == TK_VECTOR
        && db.ex(pExpr).UseXSelect()
        && {
            let s = db.sel(db.ex(pExpr).x.pSelect);
            (s.pPrior.is_null() || (s.selFlags & SF_Values) != 0)
                && s.pWin.is_null()
                && (s.selFlags & SF_MinMaxAgg) == 0
        }
        && wi.wc(wc).op == TK_AND
        && {
            let s = db.sel(db.ex(pExpr).x.pSelect);
            db.el(s.pEList).nExpr() <= u8::MAX as i32
        }
    // ^-- See bug 2026-06-04T10:00:49Z
    {
        debug_assert!(wi.t(tTerm).nChild == 0);
        let mut i = 0;
        while i < sqlite3ExprVectorSize(db, el) {
            let idxNew = whereClauseInsert(db, wi, wc, pExpr, TERM_VIRTUAL | TERM_SLICE);
            wi.wcm(wc).a[idxNew as usize].u.x.iField = i + 1;
            exprAnalyze(db, wi, pSrc, wc, idxNew);
            markTermAsChild(wi, wc, idxNew, idxTerm);
            i += 1;
        }
    }
    // Add a WO_AUX auxiliary term to the constraint set if the
    // current expression is of the form "column OP expr" where OP
    // is an operator that gets passed into virtual tables but which is
    // not normally optimized for ordinary tables.  In other words, OP
    // is one of MATCH, LIKE, GLOB, REGEXP, !=, IS, IS NOT, or NOT NULL.
    // This information is used by the xBestIndex methods of
    // virtual tables.  The native query optimizer does not attempt
    // to do anything with MATCH functions.
    else if wi.wc(wc).op == TK_AND {
        let mut pRight = ExprId::NULL;
        let mut pLeft = ExprId::NULL;
        let mut res = isAuxiliaryVtabOperator(db, pExpr, &mut eOp2, &mut pLeft, &mut pRight);
        while res > 0 {
            res -= 1;
            let prereqExpr = sqlite3WhereExprUsage(db, &mut wi.sMaskSet, pRight);
            let prereqColumn = sqlite3WhereExprUsage(db, &mut wi.sMaskSet, pLeft);
            if (prereqExpr & prereqColumn) == 0 {
                let d = sqlite3ExprDup(db, pRight, 0);
                let pNewExpr = sqlite3PExpr(db, TK_MATCH as i32, ExprId::NULL, d);
                if db.ex(pExpr).HasProperty(EP_OuterON) && pNewExpr.nn() {
                    let j = db.ex(pExpr).w.iJoin;
                    let n = db.exm(pNewExpr);
                    n.SetProperty(EP_OuterON);
                    n.w.iJoin = j;
                }
                let idxNew = whereClauseInsert(db, wi, wc, pNewExpr, TERM_VIRTUAL | TERM_DYNAMIC);
                let pNewTerm = TermId::new(wc, idxNew as usize);
                let (lt, lc) = (db.ex(pLeft).iTable, db.ex(pLeft).iColumn as i32);
                {
                    let t = wi.tm(pNewTerm);
                    t.prereqRight = prereqExpr | extraRight;
                    t.leftCursor = lt;
                    t.u.x.leftColumn = lc;
                    t.eOperator = WO_AUX;
                    t.eMatchOp = eOp2;
                }
                markTermAsChild(wi, wc, idxNew, idxTerm);
                wi.tm(tTerm).wtFlags |= TERM_COPIED;
                let pa = wi.t(tTerm).prereqAll;
                wi.tm(pNewTerm).prereqAll = pa;
            }
            std::mem::swap(&mut pLeft, &mut pRight);
        }
    }

    // Prevent ON clause terms of a LEFT JOIN from being used to drive
    // an index for tables to the left of the join.
    wi.tm(tTerm).prereqRight |= extraRight;
}

// ***************************************************************************
// Routines with file scope above.  Interface to the rest of the where.c
// subsystem follows.
// ***************************************************************************

/// This routine identifies subexpressions in the WHERE clause where
/// each subexpression is separated by the AND operator or some other
/// operator specified in the op parameter.  The WhereClause structure
/// is filled with pointers to subexpressions.  For example:
///
///    WHERE  a=='hello' AND coalesce(b,11)<10 AND (c+12!=d OR c==22)
///           \________/     \_______________/     \________________/
///            slot[0]            slot[1]               slot[2]
///
/// The original WHERE clause in pExpr is unaltered.  All this routine
/// does is make slot[] entries point to substructure within pExpr.
///
/// C: `void sqlite3WhereSplit(WhereClause *pWC, Expr *pExpr, u8 op)`
pub fn sqlite3WhereSplit(db: &mut Sqlite3, wi: &mut WhereInfo, pWC: WcId, pExpr: ExprId, op: u8) {
    let pE2 = if pExpr.nn() {
        sqlite3ExprSkipCollateAndLikely(db, pExpr)
    } else {
        ExprId::NULL
    };
    wi.wcm(pWC).op = op;
    if pE2.is_null() {
        return;
    }
    if db.ex(pE2).op != op {
        whereClauseInsert(db, wi, pWC, pExpr, 0);
    } else {
        let (l, r) = (db.ex(pE2).pLeft, db.ex(pE2).pRight);
        sqlite3WhereSplit(db, wi, pWC, l, op);
        sqlite3WhereSplit(db, wi, pWC, r, op);
    }
}

/// Add either a LIMIT (if eMatchOp==SQLITE_INDEX_CONSTRAINT_LIMIT) or
/// OFFSET (if eMatchOp==SQLITE_INDEX_CONSTRAINT_OFFSET) term to the
/// where-clause passed as the first argument. The value for the term
/// is found in register iReg.
///
/// In the common case where the value is a simple integer
/// (example: "LIMIT 5 OFFSET 10") then the expression codes as a
/// TK_INTEGER so that it will be available to sqlite3_vtab_rhs_value().
/// If not, then it codes as a TK_REGISTER expression.
fn whereAddLimitExpr(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pWC: WcId,
    iReg: i32,
    pExpr: ExprId,
    iCsr: i32,
    eMatchOp: i32,
) {
    let pNew: ExprId;
    let mut iVal = 0;

    if sqlite3ExprIsInteger(db, pExpr, &mut iVal, true, 0) != 0 && iVal >= 0 {
        let pVal = sqlite3ExprInt32(db, iVal);
        if pVal.is_null() {
            return;
        }
        pNew = sqlite3PExpr(db, TK_MATCH as i32, ExprId::NULL, pVal);
    } else {
        let pVal = sqlite3ExprAlloc(db, TK_REGISTER as i32, &Token::default(), 0);
        if pVal.is_null() {
            return;
        }
        db.exm(pVal).iTable = iReg;
        pNew = sqlite3PExpr(db, TK_MATCH as i32, ExprId::NULL, pVal);
    }
    if pNew.nn() {
        let idx = whereClauseInsert(db, wi, pWC, pNew, TERM_DYNAMIC | TERM_VIRTUAL);
        let t = &mut wi.wcm(pWC).a[idx as usize];
        t.leftCursor = iCsr;
        t.eOperator = WO_AUX;
        t.eMatchOp = eMatchOp as u8;
    }
}

/// Possibly add terms corresponding to the LIMIT and OFFSET clauses of the
/// SELECT statement passed as the second argument. These terms are only
/// added if:
///
///   1. The SELECT statement has a LIMIT clause, and
///   2. The SELECT statement is not an aggregate or DISTINCT query, and
///   3. The SELECT statement has exactly one object in its FROM clause, and
///      that object is a virtual table, and
///   4. There are no terms in the WHERE clause that will not be passed
///      to the virtual table xBestIndex method.
///   5. The ORDER BY clause, if any, will be made available to the
///      xBestIndex method.
///
/// LIMIT and OFFSET terms are ignored by most of the planner code. They
/// exist only so that they may be passed to the xBestIndex method of the
/// single virtual table in the FROM clause of the SELECT.
///
/// C: `void SQLITE_NOINLINE sqlite3WhereAddLimit(WhereClause *pWC, Select *p)`
pub fn sqlite3WhereAddLimit(db: &mut Sqlite3, wi: &mut WhereInfo, pWC: WcId, p: SelectId) {
    let s = db.sel(p).clone();
    debug_assert!(s.pLimit.nn()); // 1 -- checked by caller
    if s.pGroupBy.is_null()
        && (s.selFlags & (SF_Distinct | SF_Aggregate)) == 0 // 2
        && (db.src(s.pSrc).nSrc() == 1 && item_tab(db, SrcItemId(s.pSrc, 0)).borrow().IsVirtual())
    // 3
    {
        let pOrderBy = s.pOrderBy;
        let iCsr = db.src(s.pSrc).a[0].iCursor;

        // Check condition (4). Return early if it is not met.
        let n = wi.wc(pWC).nTerm();
        for ii in 0..n as usize {
            let t = wi.wc(pWC).a[ii];
            if t.wtFlags & TERM_CODED != 0 {
                // This term is a vector operation that has been decomposed
                // into other, subsequent terms.  It can be ignored. See
                // tag-20220128a
                debug_assert!(t.wtFlags & TERM_VIRTUAL != 0);
                debug_assert!(t.eOperator == WO_ROWVAL);
                continue;
            }
            if t.nChild != 0 {
                // If this term has child terms, then they are also part of
                // the pWC->a[] array. So this term can be ignored, as a LIMIT
                // clause will only be added if each of the child terms passes
                // the (leftCursor==iCsr) test below.
                continue;
            }
            if t.leftCursor == iCsr && t.prereqRight == 0 {
                continue;
            }

            // If this term has a parent with exactly one child, and the
            // parent will be passed through to xBestIndex, then this term
            // can be ignored.
            if t.iParent >= 0 {
                let pParent = wi.wc(pWC).a[t.iParent as usize];
                if pParent.leftCursor == iCsr && pParent.prereqRight == 0 && pParent.nChild == 1 {
                    continue;
                }
            }

            // This term will not be passed through. Do not add a LIMIT
            // clause.
            return;
        }

        // Check condition (5). Return early if it is not met.
        if pOrderBy.nn() {
            let n = db.el(pOrderBy).nExpr();
            for ii in 0..n as usize {
                let it = &db.el(pOrderBy).a[ii];
                let pExpr = it.pExpr;
                let sf = it.fg.sortFlags;
                if db.ex(pExpr).op != TK_COLUMN {
                    return;
                }
                if db.ex(pExpr).iTable != iCsr {
                    return;
                }
                if sf & KEYINFO_ORDER_BIGNULL != 0 {
                    return;
                }
            }
        }

        // All conditions are met. Add the terms to the where-clause object.
        debug_assert!(db.ex(s.pLimit).op == TK_LIMIT);
        let (ll, lr) = (db.ex(s.pLimit).pLeft, db.ex(s.pLimit).pRight);
        if s.iOffset != 0 && (s.selFlags & SF_Compound) == 0 {
            whereAddLimitExpr(
                db,
                wi,
                pWC,
                s.iOffset,
                lr,
                iCsr,
                SQLITE_INDEX_CONSTRAINT_OFFSET,
            );
        }
        if s.iOffset == 0 || (s.selFlags & SF_Compound) == 0 {
            whereAddLimitExpr(
                db,
                wi,
                pWC,
                s.iLimit,
                ll,
                iCsr,
                SQLITE_INDEX_CONSTRAINT_LIMIT,
            );
        }
    }
}

/// Initialize a preallocated WhereClause structure.
///
/// C: `void sqlite3WhereClauseInit( WhereClause *pWC, WhereInfo *pWInfo )`
/// (Rust: the clause does not point back at its WhereInfo.)
pub fn sqlite3WhereClauseInit(pWC: &mut WhereClause) {
    pWC.hasOr = 0;
    pWC.pOuter = None;
    pWC.nBase = 0;
    pWC.a.clear();
}

/// Deallocate a WhereClause structure.  The WhereClause structure
/// itself is not freed.  This routine is the inverse of
/// sqlite3WhereClauseInit().
///
/// C: `void sqlite3WhereClauseClear(WhereClause *pWC)`
pub fn sqlite3WhereClauseClear(db: &mut Sqlite3, wi: &mut WhereInfo, pWC: WcId) {
    debug_assert!(wi.wc(pWC).nTerm() >= wi.wc(pWC).nBase);
    let n = wi.wc(pWC).a.len();
    for i in 0..n {
        let a = wi.wc(pWC).a[i];
        debug_assert!(a.eMatchOp == 0 || a.eOperator == WO_AUX);
        if a.wtFlags & TERM_DYNAMIC != 0 {
            sqlite3ExprDelete(db, a.pExpr);
        }
        if a.wtFlags & (TERM_ORINFO | TERM_ANDINFO) != 0 {
            if a.wtFlags & TERM_ORINFO != 0 {
                debug_assert!((a.wtFlags & TERM_ANDINFO) == 0);
                if let Some(o) = a.u.pOrInfo {
                    whereOrInfoDelete(db, wi, o);
                }
            } else {
                debug_assert!((a.wtFlags & TERM_ANDINFO) != 0);
                if let Some(w) = a.u.pAndInfo {
                    whereAndInfoDelete(db, wi, w);
                }
            }
        }
    }
}

/// These routines walk (recursively) an expression tree and generate
/// a bitmask indicating which tables are used in that expression
/// tree.
///
/// sqlite3WhereExprUsage(MaskSet, Expr) ->
///
///       Return a Bitmask of all tables referenced by Expr.  Expr can be
///       be NULL, in which case 0 is returned.
///
/// sqlite3WhereExprUsageNN(MaskSet, Expr) ->
///
///       Same as sqlite3WhereExprUsage() except that Expr must not be
///       NULL.  The "NN" suffix on the name stands for "Not Null".
///
/// sqlite3WhereExprListUsage(MaskSet, ExprList) ->
///
///       Return a Bitmask of all tables referenced by every expression
///       in the expression list ExprList.  ExprList can be NULL, in which
///       case 0 is returned.
///
/// sqlite3WhereExprUsageFull(MaskSet, ExprList) ->
///
///       Internal use only.  Called only by sqlite3WhereExprUsageNN() for
///       complex expressions that require pushing register values onto
///       the stack.
fn sqlite3WhereExprUsageFull(db: &mut Sqlite3, pMaskSet: &mut WhereMaskSet, p: ExprId) -> Bitmask {
    let e = db.ex(p).clone();
    let mut mask: Bitmask = if e.op == TK_IF_NULL_ROW {
        sqlite3WhereGetMask(pMaskSet, e.iTable)
    } else {
        0
    };
    if e.pLeft.nn() {
        mask |= sqlite3WhereExprUsageNN(db, pMaskSet, e.pLeft);
    }
    if e.pRight.nn() {
        mask |= sqlite3WhereExprUsageNN(db, pMaskSet, e.pRight);
        debug_assert!(e.x.pList.is_null());
    } else if e.UseXSelect() {
        if e.HasProperty(EP_VarSelect) {
            pMaskSet.bVarSelect = 1;
        }
        mask |= exprSelectUsage(db, pMaskSet, e.x.pSelect);
    } else if e.x.pList.nn() {
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, e.x.pList);
    }
    if (e.op == TK_FUNCTION || e.op == TK_AGG_FUNCTION) && e.UseYWin() {
        debug_assert!(e.y.pWin.nn());
        let w = db.win(e.y.pWin).clone();
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, w.pPartition);
        mask |= sqlite3WhereExprListUsage(db, pMaskSet, w.pOrderBy);
        mask |= sqlite3WhereExprUsage(db, pMaskSet, w.pFilter);
    }
    mask
}

/// C: `Bitmask sqlite3WhereExprUsageNN(WhereMaskSet *pMaskSet, Expr *p)`
pub fn sqlite3WhereExprUsageNN(
    db: &mut Sqlite3,
    pMaskSet: &mut crate::where_::WhereMaskSet,
    p: ExprId,
) -> Bitmask {
    let (op, fixed, leaf, iTable) = {
        let e = db.ex(p);
        (
            e.op,
            e.HasProperty(EP_FixedCol),
            e.HasProperty(EP_TokenOnly | EP_Leaf),
            e.iTable,
        )
    };
    if op == TK_COLUMN && !fixed {
        return sqlite3WhereGetMask(pMaskSet, iTable);
    } else if leaf {
        debug_assert!(op != TK_IF_NULL_ROW);
        return 0;
    }
    sqlite3WhereExprUsageFull(db, pMaskSet, p)
}

/// C: `Bitmask sqlite3WhereExprUsage(WhereMaskSet *pMaskSet, Expr *p)`
pub fn sqlite3WhereExprUsage(
    db: &mut Sqlite3,
    pMaskSet: &mut crate::where_::WhereMaskSet,
    p: ExprId,
) -> Bitmask {
    if p.nn() {
        sqlite3WhereExprUsageNN(db, pMaskSet, p)
    } else {
        0
    }
}

/// C: `Bitmask sqlite3WhereExprListUsage(WhereMaskSet *pMaskSet, ExprList *pList)`
pub fn sqlite3WhereExprListUsage(
    db: &mut Sqlite3,
    pMaskSet: &mut crate::where_::WhereMaskSet,
    pList: ExprListId,
) -> Bitmask {
    let mut mask: Bitmask = 0;
    if pList.nn() {
        let n = db.el(pList).nExpr();
        for i in 0..n as usize {
            let e = db.el(pList).a[i].pExpr;
            mask |= sqlite3WhereExprUsage(db, pMaskSet, e);
        }
    }
    mask
}

/// Call exprAnalyze on all terms in a WHERE clause.
///
/// Note that exprAnalyze() might add new virtual terms onto the
/// end of the WHERE clause.  We do not want to analyze these new
/// virtual terms, so start analyzing at the end and work forward
/// so that the added virtual terms are never processed.
///
/// C: `void sqlite3WhereExprAnalyze( SrcList *pTabList, WhereClause *pWC )`
pub fn sqlite3WhereExprAnalyze(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pTabList: SrcListId,
    pWC: WcId,
) {
    let mut i = wi.wc(pWC).nTerm() - 1;
    while i >= 0 {
        exprAnalyze(db, wi, pTabList, pWC, i);
        i -= 1;
    }
}

/// For table-valued-functions, transform the function arguments into
/// new WHERE clause terms.
///
/// Each function argument translates into an equality constraint against
/// a HIDDEN column in the table.
///
/// C: `void sqlite3WhereTabFuncArgs( Parse *pParse, SrcItem *pItem, WhereClause *pWC )`
pub fn sqlite3WhereTabFuncArgs(db: &mut Sqlite3, wi: &mut WhereInfo, pItem: SrcItemId, pWC: WcId) {
    if db.item(pItem).fg.isTabFunc == 0 {
        return;
    }
    let pTab = item_tab(db, pItem);
    if !pTab.borrow().IsVirtual() {
        let zName = db.item(pItem).zName.clone();
        errmsg!(db, "'%s' is not a function", zName.as_deref());
        return;
    }
    let pArgs = db.item(pItem).u1.pFuncArg;
    if pArgs.is_null() {
        return;
    }
    let nArg = db.el(pArgs).nExpr();
    let mut k: i32 = 0;
    for j in 0..nArg {
        let nCol = pTab.borrow().nCol as i32;
        while k < nCol && (pTab.borrow().aCol[k as usize].colFlags & COLFLAG_HIDDEN) == 0 {
            k += 1;
        }
        if k >= nCol {
            let zName = pTab.borrow().zName.clone();
            errmsg!(db, "too many arguments on %s() - max %d", &zName, j);
            return;
        }
        let pColRef = sqlite3ExprAlloc(db, TK_COLUMN as i32, &Token::default(), 0);
        if pColRef.is_null() {
            return;
        }
        let iCursor = db.item(pItem).iCursor;
        {
            let e = db.exm(pColRef);
            e.iTable = iCursor;
            e.iColumn = k as ynVar;
            e.y.pTab = Some(pTab.clone());
        }
        k += 1;
        let cu = crate::resolve::sqlite3ExprColUsed(db, pColRef);
        db.itemm(pItem).colUsed |= cu;
        let a = db.el(pArgs).a[j as usize].pExpr;
        let d = sqlite3ExprDup(db, a, 0);
        let pRhs = sqlite3PExpr(db, TK_UPLUS as i32, d, ExprId::NULL);
        let pTerm = sqlite3PExpr(db, TK_EQ as i32, pColRef, pRhs);
        let joinType: u32;
        if db.item(pItem).fg.jointype & (JT_LEFT | JT_RIGHT) != 0 {
            joinType = EP_OuterON;
        } else {
            joinType = EP_InnerON;
        }
        crate::select::sqlite3SetJoinExpr(db, pTerm, iCursor, joinType);
        whereClauseInsert(db, wi, pWC, pTerm, TERM_DYNAMIC);
    }
}

/// Return true if the WhereLoop pLoop can be use a Bloom filter.
/// tag-202607231411
///
/// C: `int sqlite3WhereLoopBloomable(const WhereLoop *pLoop)`
pub fn sqlite3WhereLoopBloomable(pLoop: &crate::where_::WhereLoop) -> i32 {
    if pLoop.wsFlags & WHERE_IPK != 0 {
        return 1;
    }
    if (pLoop.wsFlags & WHERE_INDEXED) == 0 {
        return 0;
    }
    match &pLoop.u.btree.pIndex {
        Some(p) => crate::build::sqlite3IndexBloomable(p, pLoop.u.btree.nEq as i32),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_masks() {
        assert!(allowedOp(TK_EQ) && allowedOp(TK_GE) && allowedOp(TK_IN) && allowedOp(TK_IS));
        assert!(allowedOp(TK_ISNULL) && !allowedOp(TK_NE) && !allowedOp(TK_AND));
        assert_eq!(operatorMask(TK_EQ), WO_EQ);
        assert_eq!(operatorMask(TK_LT), WO_LT);
        assert_eq!(operatorMask(TK_LE), WO_LE);
        assert_eq!(operatorMask(TK_GT), WO_GT);
        assert_eq!(operatorMask(TK_GE), WO_GE);
        assert_eq!(operatorMask(TK_IN), WO_IN);
        assert_eq!(operatorMask(TK_IS), WO_IS);
        assert_eq!(operatorMask(TK_ISNULL), WO_ISNULL);
    }

    #[test]
    fn utf8_reader() {
        let z = "aé€😀\u{0}".as_bytes();
        let mut p = 0;
        assert_eq!(utf8_read(z, &mut p), 'a' as u32);
        assert_eq!(utf8_read(z, &mut p), 'é' as u32);
        assert_eq!(utf8_read(z, &mut p), '€' as u32);
        assert_eq!(utf8_read(z, &mut p), '😀' as u32);
        assert_eq!(p, z.len() - 1);
        // overlong encoding and surrogate
        let mut p = 0;
        assert_eq!(utf8_read(&[0xc0, 0x80, 0], &mut p), 0xFFFD);
        let mut p = 0;
        assert_eq!(utf8_read(&[0xed, 0xa0, 0x80, 0], &mut p), 0xFFFD);
    }
}
