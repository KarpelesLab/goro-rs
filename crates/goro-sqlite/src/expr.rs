//! Port of expr.c.
//!
//! Routines used for analyzing expressions and for generating VDBE code
//! that evaluates expressions in SQLite.
//!
//! Notes on the port:
//! * `Parse*` is `db.pp()`; expression nodes live in `db.arena`.
//! * Stack-allocated temporary `Expr` objects of the C code (tempX,
//!   opCompare, exprAnd, ...) are allocated in the arena instead.
//! * "Delete" routines free nothing but keep the side effects of the C
//!   code (window objects are unlinked through sqlite3WindowDelete(),
//!   subqueries go through sqlite3SelectDelete()).
//! * Functions whose C version accepts a NULL `Parse*` take a
//!   `pParse: bool` argument (C `pParse!=0`).

use crate::int::*;
use crate::printf::PfArg;
use crate::vdbeaux::*;
use crate::walker::*;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Small helpers (C macros of sqliteInt.h and local conveniences)
// ---------------------------------------------------------------------------

/// ExprHasProperty(e, p) on an arena node.
#[inline]
fn hp(db: &Sqlite3, e: ExprId, p: u32) -> bool {
    (db.ex(e).flags & p) != 0
}

/// pParse->nErr
#[inline]
fn nerr(db: &Sqlite3) -> i32 {
    db.ppr().nErr
}

/// IN_RENAME_OBJECT
#[inline]
fn in_rename(db: &Sqlite3) -> bool {
    db.ppr().IN_RENAME_OBJECT()
}

/// pList ? pList->nExpr : 0
#[inline]
fn el_n(db: &Sqlite3, l: ExprListId) -> i32 {
    if l.is_null() { 0 } else { db.el(l).nExpr() }
}

/// pList->a[i].pExpr
#[inline]
fn el_e(db: &Sqlite3, l: ExprListId, i: i32) -> ExprId {
    db.el(l).a[i as usize].pExpr
}

/// sqlite3IsNumericAffinity(X)
#[inline]
fn sqlite3IsNumericAffinity(x: u8) -> bool {
    x >= SQLITE_AFF_NUMERIC
}

/// IsWindowFunc(p)
fn IsWindowFunc(db: &Sqlite3, p: ExprId) -> bool {
    let e = db.ex(p);
    e.HasProperty(EP_WinFunc) && db.win(e.y.pWin).eFrmType != TK_FILTER
}

/// sqlite3IsBinary() accepting a NULL collation (C `p==0 || ...`).
fn is_binary(p: &Option<CollSeqRef>) -> bool {
    match p {
        None => true,
        Some(c) => crate::mainapi::sqlite3IsBinary(Some(c)) != 0,
    }
}

/// The P4 operand for a collating sequence.  A NULL collation leaves the
/// P4 slot unused, as sqlite3VdbeChangeP4() does in C for a NULL pointer.
fn p4_coll(c: Option<CollSeqRef>) -> (P4, i32) {
    match c {
        Some(c) => (P4::Coll(c), P4_COLLSEQ as i32),
        None => (P4::None, P4_NOTUSED as i32),
    }
}

/// sqlite3VdbeAddOp4(v, op, p1, p2, p3, (char*)pColl, P4_COLLSEQ)
fn add_op4_coll(db: &mut Sqlite3, op: u8, p1: i32, p2: i32, p3: i32, c: Option<CollSeqRef>) -> i32 {
    let (p4, t) = p4_coll(c);
    sqlite3VdbeAddOp4(db.v(), op, p1, p2, p3, p4, t)
}

/// A zeroed Expr node of type `op` (C `memset(&tmp,0,sizeof(tmp)); tmp.op=op`).
fn tmp_expr(db: &mut Sqlite3, op: u8) -> ExprId {
    db.arena.new_expr(Expr {
        op,
        ..Expr::default()
    })
}

/// Run `f` with the statement under construction taken out of the parse
/// (for builder functions that need both `db` and the `Vdbe`).
fn with_parse_vdbe<R>(db: &mut Sqlite3, f: impl FnOnce(&mut Sqlite3, &mut Vdbe) -> R) -> Option<R> {
    let mut v = db.pp().pVdbe.take()?;
    let r = f(db, &mut v);
    db.pp().pVdbe = Some(v);
    Some(r)
}

/// sqlite3DbSpanDup(): the text between two offsets of the SQL being
/// parsed, with leading and trailing white-space removed.
fn span_dup(db: &Sqlite3, zStart: usize, zEnd: usize) -> ZStr {
    let Some(sql) = db.ppr().zSql.as_ref() else {
        return Vec::new();
    };
    let end = zEnd.min(sql.len());
    let mut s = zStart.min(end);
    while s < end && crate::global::sqlite3Isspace(sql[s]) {
        s += 1;
    }
    let mut n = end - s;
    while n > 0 && crate::global::sqlite3Isspace(sql[s + n - 1]) {
        n -= 1;
    }
    sql[s..s + n].to_vec()
}

// ---------------------------------------------------------------------------
// Affinity and collation
// ---------------------------------------------------------------------------

/// Return the affinity character for a single column of a table.
pub fn sqlite3TableColumnAffinity(pTab: &TableRef, iCol: i32) -> u8 {
    let t = pTab.borrow();
    if iCol < 0 || iCol >= t.nCol as i32 || iCol as usize >= t.aCol.len() {
        return SQLITE_AFF_INTEGER;
    }
    t.aCol[iCol as usize].affinity
}

fn exprAffinity(db: &Sqlite3, pExpr: ExprId) -> u8 {
    let mut pExpr = pExpr;
    let mut op = db.ex(pExpr).op;
    loop {
        let e = db.ex(pExpr);
        if op == TK_COLUMN || (op == TK_AGG_COLUMN && e.y.pTab.is_some()) {
            if let Some(t) = &e.y.pTab {
                return sqlite3TableColumnAffinity(t, e.iColumn as i32);
            }
            break;
        }
        if op == TK_SELECT {
            let pEList = db.sel(e.x.pSelect).pEList;
            return exprAffinity(db, el_e(db, pEList, 0));
        }
        if op == TK_CAST {
            debug_assert!(!e.HasProperty(EP_IntValue));
            return crate::build::sqlite3AffinityType(e.u.zToken.as_deref(), None);
        }
        if op == TK_SELECT_COLUMN {
            let pEList = db.sel(db.ex(e.pLeft).x.pSelect).pEList;
            return exprAffinity(db, el_e(db, pEList, e.iColumn as i32));
        }
        if op == TK_VECTOR || (op == TK_FUNCTION && e.affExpr == SQLITE_AFF_DEFER) {
            return exprAffinity(db, el_e(db, e.x.pList, 0));
        }
        if e.HasProperty(EP_Skip | EP_IfNullRow) {
            pExpr = e.pLeft;
            op = db.ex(pExpr).op;
            continue;
        }
        if op != TK_REGISTER {
            break;
        }
        op = e.op2;
        if op == TK_REGISTER {
            break;
        }
    }
    db.ex(pExpr).affExpr
}

/// Return the 'affinity' of the expression pExpr if any.
///
/// If pExpr is a column, a reference to a column via an 'AS' alias,
/// or a sub-select with a column as the return value, then the
/// affinity of that column is returned. Otherwise, 0x00 is returned,
/// indicating no affinity for the expression.
pub fn sqlite3ExprAffinity(db: &mut Sqlite3, pExpr: ExprId) -> u8 {
    exprAffinity(db, pExpr)
}

/// Make a guess at all the possible datatypes of the result that could
/// be returned by an expression.  Return a bitmask indicating the answer:
///
///     0x01         Numeric
///     0x02         Text
///     0x04         Blob
///
/// If the expression must return NULL, then 0x00 is returned.
pub fn sqlite3ExprDataType(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    exprDataType(db, pExpr)
}

fn exprDataType(db: &Sqlite3, pExpr: ExprId) -> i32 {
    let mut pExpr = pExpr;
    while pExpr.nn() {
        let e = db.ex(pExpr);
        match e.op {
            TK_COLLATE | TK_IF_NULL_ROW | TK_UPLUS => {
                pExpr = e.pLeft;
            }
            TK_NULL => {
                pExpr = ExprId::NULL;
            }
            TK_STRING => return 0x02,
            TK_BLOB => return 0x04,
            TK_CONCAT => return 0x06,
            TK_VARIABLE | TK_AGG_FUNCTION | TK_FUNCTION => return 0x07,
            TK_COLUMN | TK_AGG_COLUMN | TK_SELECT | TK_CAST | TK_SELECT_COLUMN | TK_VECTOR => {
                let aff = exprAffinity(db, pExpr);
                if aff >= SQLITE_AFF_NUMERIC {
                    return 0x05;
                }
                if aff == SQLITE_AFF_TEXT {
                    return 0x06;
                }
                return 0x07;
            }
            TK_CASE => {
                let mut res = 0;
                let pList = e.x.pList;
                let n = el_n(db, pList);
                let mut ii = 1;
                while ii < n {
                    res |= exprDataType(db, el_e(db, pList, ii));
                    ii += 2;
                }
                if n % 2 != 0 {
                    res |= exprDataType(db, el_e(db, pList, n - 1));
                }
                return res;
            }
            _ => return 0x01,
        }
    }
    0x00
}

/// Set the collating sequence for expression pExpr to be the collating
/// sequence named by pToken.   Return a pointer to a new Expr node that
/// implements the COLLATE operator.
pub fn sqlite3ExprAddCollateToken(
    db: &mut Sqlite3,
    pExpr: ExprId,
    pCollName: &Token,
    dequote: i32,
) -> ExprId {
    let mut pExpr = pExpr;
    if pCollName.n > 0 && nerr(db) == 0 {
        let pNew = sqlite3ExprAlloc(db, TK_COLLATE as i32, pCollName, dequote);
        debug_assert!(pExpr.nn());
        let h = db.ex(pExpr).nHeight + 1;
        {
            let n = db.exm(pNew);
            n.pLeft = pExpr;
            n.flags |= EP_Collate | EP_Skip;
            n.nHeight = h;
        }
        sqlite3ExprCheckHeight(db, h);
        pExpr = pNew;
    }
    pExpr
}

/// C: `Expr * sqlite3ExprAddCollateString( Parse *pParse, Expr *pExpr, const char *zC )`
pub fn sqlite3ExprAddCollateString(db: &mut Sqlite3, pExpr: ExprId, zC: Option<&[u8]>) -> ExprId {
    let s = Token::from_bytes(zC.unwrap_or(b""));
    sqlite3ExprAddCollateToken(db, pExpr, &s, 0)
}

fn skipCollate(db: &Sqlite3, pExpr: ExprId) -> ExprId {
    let mut pExpr = pExpr;
    while pExpr.nn() && hp(db, pExpr, EP_Skip) {
        debug_assert!(db.ex(pExpr).op == TK_COLLATE);
        pExpr = db.ex(pExpr).pLeft;
    }
    pExpr
}

/// Skip over any TK_COLLATE operators.
pub fn sqlite3ExprSkipCollate(db: &mut Sqlite3, pExpr: ExprId) -> ExprId {
    skipCollate(db, pExpr)
}

fn skipCollateAndLikely(db: &Sqlite3, pExpr: ExprId) -> ExprId {
    let mut pExpr = pExpr;
    while pExpr.nn() && hp(db, pExpr, EP_Skip | EP_Unlikely) {
        let e = db.ex(pExpr);
        if e.HasProperty(EP_Unlikely) {
            debug_assert!(e.op == TK_FUNCTION);
            pExpr = el_e(db, e.x.pList, 0);
        } else if e.op == TK_COLLATE {
            pExpr = e.pLeft;
        } else {
            break;
        }
    }
    pExpr
}

/// Skip over any TK_COLLATE operators and/or any unlikely()
/// or likelihood() or likely() functions at the root of an
/// expression.
pub fn sqlite3ExprSkipCollateAndLikely(db: &mut Sqlite3, pExpr: ExprId) -> ExprId {
    skipCollateAndLikely(db, pExpr)
}

/// Return the collation sequence for the expression pExpr. If
/// there is no defined collating sequence, return NULL.
///
/// See also: sqlite3ExprNNCollSeq()
///
/// The collating sequence might be determined by a COLLATE operator
/// or by the presence of a column with a defined collating sequence.
/// COLLATE operators take first precedence.  Left operands take
/// precedence over right operands.
pub fn sqlite3ExprCollSeq(db: &mut Sqlite3, pExpr: ExprId) -> Option<CollSeqRef> {
    let mut pColl: Option<CollSeqRef> = None;
    let mut p = pExpr;
    while p.nn() {
        let (mut op, op2, flags, pTab, iColumn, pLeft, pRight, pList, affExpr, zToken) = {
            let e = db.ex(p);
            (
                e.op,
                e.op2,
                e.flags,
                e.y.pTab.clone(),
                e.iColumn,
                e.pLeft,
                e.pRight,
                e.x.pList,
                e.affExpr,
                e.u.zToken.clone(),
            )
        };
        if op == TK_REGISTER {
            op = op2;
        }
        if (op == TK_AGG_COLUMN && pTab.is_some()) || op == TK_COLUMN || op == TK_TRIGGER {
            let j = iColumn as i32;
            if j >= 0 {
                if let Some(t) = &pTab {
                    let zColl = {
                        let tb = t.borrow();
                        match tb.aCol.get(j as usize) {
                            Some(c) => {
                                let mut c = c.clone();
                                crate::build::sqlite3ColumnColl(&mut c)
                            }
                            None => None,
                        }
                    };
                    let enc = db.ENC();
                    pColl = crate::callback::sqlite3FindCollSeq(db, enc, zColl.as_deref(), 0);
                }
            }
            break;
        }
        if op == TK_CAST || op == TK_UPLUS {
            p = pLeft;
            continue;
        }
        if op == TK_VECTOR || (op == TK_FUNCTION && affExpr == SQLITE_AFF_DEFER) {
            p = el_e(db, pList, 0);
            continue;
        }
        if op == TK_COLLATE {
            debug_assert!((flags & EP_IntValue) == 0);
            let enc = db.ENC();
            pColl = crate::callback::sqlite3GetCollSeq(db, enc, None, zToken.as_deref());
            break;
        }
        if (flags & EP_Collate) != 0 {
            if pLeft.nn() && hp(db, pLeft, EP_Collate) {
                p = pLeft;
            } else {
                let mut pNext = pRight;
                /* The Expr.x union is never used at the same time as Expr.pRight */
                if (flags & EP_xIsSelect) == 0 && pList.nn() && db.mallocFailed == 0 {
                    for i in 0..el_n(db, pList) {
                        let a = el_e(db, pList, i);
                        if hp(db, a, EP_Collate) {
                            pNext = a;
                            break;
                        }
                    }
                }
                p = pNext;
            }
        } else {
            break;
        }
    }
    if crate::callback::sqlite3CheckCollSeq(db, pColl.as_ref()) != 0 {
        pColl = None;
    }
    pColl
}

/// Return the collation sequence for the expression pExpr. If
/// there is no defined collating sequence, return a pointer to the
/// default collation sequence.
pub fn sqlite3ExprNNCollSeq(db: &mut Sqlite3, pExpr: ExprId) -> Option<CollSeqRef> {
    let p = sqlite3ExprCollSeq(db, pExpr);
    match p {
        Some(c) => Some(c),
        None => db.pDfltColl.clone(),
    }
}

/// Return TRUE if the two expressions have equivalent collating sequences.
pub fn sqlite3ExprCollSeqMatch(db: &mut Sqlite3, pE1: ExprId, pE2: ExprId) -> i32 {
    let pColl1 = sqlite3ExprNNCollSeq(db, pE1);
    let pColl2 = sqlite3ExprNNCollSeq(db, pE2);
    match (pColl1, pColl2) {
        (Some(a), Some(b)) => Rc::ptr_eq(&a, &b) as i32,
        (None, None) => 1,
        _ => 0,
    }
}

fn compareAffinity(db: &Sqlite3, pExpr: ExprId, aff2: u8) -> u8 {
    let aff1 = exprAffinity(db, pExpr);
    if aff1 > SQLITE_AFF_NONE && aff2 > SQLITE_AFF_NONE {
        /* Both sides of the comparison are columns. If one has numeric
         ** affinity, use that. Otherwise use no affinity.
         */
        if sqlite3IsNumericAffinity(aff1) || sqlite3IsNumericAffinity(aff2) {
            SQLITE_AFF_NUMERIC
        } else {
            SQLITE_AFF_BLOB
        }
    } else {
        /* One side is a column, the other is not. Use the columns affinity. */
        debug_assert!(aff1 <= SQLITE_AFF_NONE || aff2 <= SQLITE_AFF_NONE);
        (if aff1 <= SQLITE_AFF_NONE { aff2 } else { aff1 }) | SQLITE_AFF_NONE
    }
}

/// pExpr is an operand of a comparison operator.  aff2 is the
/// type affinity of the other operand.  This routine returns the
/// type affinity that should be used for the comparison operator.
pub fn sqlite3CompareAffinity(db: &mut Sqlite3, pExpr: ExprId, aff2: u8) -> u8 {
    compareAffinity(db, pExpr, aff2)
}

/// pExpr is a comparison operator.  Return the type affinity that should
/// be applied to both operands prior to doing the comparison.
fn comparisonAffinity(db: &Sqlite3, pExpr: ExprId) -> u8 {
    let e = db.ex(pExpr);
    debug_assert!(e.pLeft.nn());
    let mut aff = exprAffinity(db, e.pLeft);
    if e.pRight.nn() {
        aff = compareAffinity(db, e.pRight, aff);
    } else if e.UseXSelect() {
        let pEList = db.sel(e.x.pSelect).pEList;
        aff = compareAffinity(db, el_e(db, pEList, 0), aff);
    } else if aff == 0 {
        aff = SQLITE_AFF_BLOB;
    }
    aff
}

/// pExpr is a comparison expression, eg. '=', '<', IN(...) etc.
/// idx_affinity is the affinity of an indexed column. Return true
/// if the index with affinity idx_affinity may be used to implement
/// the comparison in pExpr.
pub fn sqlite3IndexAffinityOk(db: &mut Sqlite3, pExpr: ExprId, idx_affinity: u8) -> i32 {
    let aff = comparisonAffinity(db, pExpr);
    if aff < SQLITE_AFF_TEXT {
        return 1;
    }
    if aff == SQLITE_AFF_TEXT {
        return (idx_affinity == SQLITE_AFF_TEXT) as i32;
    }
    sqlite3IsNumericAffinity(idx_affinity) as i32
}

/// Return the P5 value that should be used for a binary comparison
/// opcode (OP_Eq, OP_Ge etc.) used to compare pExpr1 and pExpr2.
fn binaryCompareP5(db: &Sqlite3, pExpr1: ExprId, pExpr2: ExprId, jumpIfNull: i32) -> u8 {
    let aff = exprAffinity(db, pExpr2);
    compareAffinity(db, pExpr1, aff) | (jumpIfNull as u8)
}

/// Return a pointer to the collation sequence that should be used by
/// a binary comparison operator comparing pLeft and pRight.
///
/// If the left hand expression has a collating sequence type, then it is
/// used. Otherwise the collation sequence for the right hand expression
/// is used, or the default (BINARY) if neither expression has a collating
/// type.
///
/// Argument pRight (but not pLeft) may be a null pointer. In this case,
/// it is not considered.
pub fn sqlite3BinaryCompareCollSeq(
    db: &mut Sqlite3,
    pLeft: ExprId,
    pRight: ExprId,
) -> Option<CollSeqRef> {
    debug_assert!(pLeft.nn());
    if hp(db, pLeft, EP_Collate) {
        sqlite3ExprCollSeq(db, pLeft)
    } else if pRight.nn() && hp(db, pRight, EP_Collate) {
        sqlite3ExprCollSeq(db, pRight)
    } else {
        let pColl = sqlite3ExprCollSeq(db, pLeft);
        if pColl.is_none() {
            sqlite3ExprCollSeq(db, pRight)
        } else {
            pColl
        }
    }
}

/// Expression p is a comparison operator.  Return a collation sequence
/// appropriate for the comparison operator.
///
/// This is normally just a wrapper around sqlite3BinaryCompareCollSeq().
/// However, if the OP_Commuted flag is set, then the order of the operands
/// is reversed in the sqlite3BinaryCompareCollSeq() call so that the
/// correct collating sequence is found.
pub fn sqlite3ExprCompareCollSeq(db: &mut Sqlite3, p: ExprId) -> Option<CollSeqRef> {
    let (l, r, c) = {
        let e = db.ex(p);
        (e.pLeft, e.pRight, e.HasProperty(EP_Commuted))
    };
    if c {
        sqlite3BinaryCompareCollSeq(db, r, l)
    } else {
        sqlite3BinaryCompareCollSeq(db, l, r)
    }
}

/// Generate code for a comparison operator.
fn codeCompare(
    db: &mut Sqlite3,
    pLeft: ExprId,
    pRight: ExprId,
    opcode: u8,
    in1: i32,
    in2: i32,
    dest: i32,
    jumpIfNull: i32,
    isCommuted: bool,
) -> i32 {
    if nerr(db) != 0 {
        return 0;
    }
    let p4 = if isCommuted {
        sqlite3BinaryCompareCollSeq(db, pRight, pLeft)
    } else {
        sqlite3BinaryCompareCollSeq(db, pLeft, pRight)
    };
    let p5 = binaryCompareP5(db, pLeft, pRight, jumpIfNull);
    let addr = add_op4_coll(db, opcode, in2, dest, in1, p4);
    sqlite3VdbeChangeP5(db.v(), p5 as u16);
    addr
}

// ---------------------------------------------------------------------------
// Vectors
// ---------------------------------------------------------------------------

fn exprVectorSize(db: &Sqlite3, pExpr: ExprId) -> i32 {
    let e = db.ex(pExpr);
    let mut op = e.op;
    if op == TK_REGISTER {
        op = e.op2;
    }
    if op == TK_VECTOR {
        el_n(db, e.x.pList)
    } else if op == TK_SELECT {
        el_n(db, db.sel(e.x.pSelect).pEList)
    } else {
        1
    }
}

/// Return true if expression pExpr is a vector, or false otherwise.
///
/// A vector is defined as any expression that results in two or more
/// columns of result.  Every TK_VECTOR node is an vector because the
/// parser will not generate a TK_VECTOR with fewer than two entries.
/// But a TK_SELECT might be either a vector or a scalar. It is only
/// considered a vector if it has two or more result columns.
pub fn sqlite3ExprIsVector(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    (exprVectorSize(db, pExpr) > 1) as i32
}

/// If the expression passed as the only argument is of type TK_VECTOR
/// return the number of expressions in the vector. Or, if the expression
/// is a sub-select, return the number of columns in the sub-select. For
/// any other type of expression, return 1.
pub fn sqlite3ExprVectorSize(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    exprVectorSize(db, pExpr)
}

fn vectorFieldSubexpr(db: &Sqlite3, pVector: ExprId, i: i32) -> ExprId {
    if exprVectorSize(db, pVector) > 1 {
        let e = db.ex(pVector);
        if e.op == TK_SELECT || e.op2 == TK_SELECT {
            let pEList = db.sel(e.x.pSelect).pEList;
            return el_e(db, pEList, i);
        } else {
            return el_e(db, e.x.pList, i);
        }
    }
    pVector
}

/// Return a pointer to a subexpression of pVector that is the i-th
/// column of the vector (numbered starting with 0).  The caller must
/// ensure that i is within range.
///
/// If pVector is really a scalar (and "scalar" here includes subqueries
/// that return a single column!) then return pVector unmodified.
///
/// pVector retains ownership of the returned subexpression.
///
/// If the vector is a (SELECT ...) then the expression returned is
/// just the expression for the i-th term of the result set, and may
/// not be ready for evaluation because the table cursor has not yet
/// been positioned.
pub fn sqlite3VectorFieldSubexpr(db: &mut Sqlite3, pVector: ExprId, i: i32) -> ExprId {
    vectorFieldSubexpr(db, pVector, i)
}

/// Compute and return a new Expr object which when passed to
/// sqlite3ExprCode() will generate all necessary code to compute
/// the iField-th column of the vector expression pVector.
///
/// It is ok for pVector to be a scalar (as long as iField==0).
/// In that case, this routine works like sqlite3ExprDup().
///
/// The caller retains ownership of pVector.  If pVector is a TK_SELECT,
/// then the returned object will reference pVector and so pVector must remain
/// valid for the life of the returned object.  If pVector is a TK_VECTOR
/// or a scalar expression, then it can be deleted as soon as this routine
/// returns.
///
/// A trick to cause a TK_SELECT pVector to be deleted together with
/// the returned Expr object is to attach the pVector to the pRight field
/// of the returned TK_SELECT_COLUMN Expr object.
pub fn sqlite3ExprForVectorField(
    db: &mut Sqlite3,
    pVector: ExprId,
    iField: i32,
    nField: i32,
) -> ExprId {
    let mut pVector = pVector;
    let pRet;
    if db.ex(pVector).op == TK_SELECT {
        /* The TK_SELECT_COLUMN Expr node:
         **
         ** pLeft:           pVector containing TK_SELECT.  Not deleted.
         ** pRight:          not used.  But recursively deleted.
         ** iColumn:         Index of a column in pVector
         ** iTable:          0 or the number of columns on the LHS of an assignment
         ** pLeft->iTable:   First in an array of register holding result, or 0
         **                  if the result is not yet computed.
         **
         ** sqlite3ExprDelete() specifically skips the recursive delete of
         ** pLeft on TK_SELECT_COLUMN nodes.  But pRight is followed, so pVector
         ** can be attached to pRight to cause this node to take ownership of
         ** pVector.  Typically there will be multiple TK_SELECT_COLUMN nodes
         ** with the same pLeft pointer to the pVector, but only one of them
         ** will own the pVector.
         */
        pRet = sqlite3PExpr(db, TK_SELECT_COLUMN as i32, ExprId::NULL, ExprId::NULL);
        if pRet.nn() {
            let r = db.exm(pRet);
            r.flags |= EP_FullSize;
            r.iTable = nField;
            r.iColumn = iField as ynVar;
            r.pLeft = pVector;
        }
    } else {
        if db.ex(pVector).op == TK_VECTOR {
            let pList = db.ex(pVector).x.pList;
            let sub = el_e(db, pList, iField);
            pVector = sub;
            if in_rename(db) {
                /* This must be a vector UPDATE inside a trigger */
                db.elm(pList).a[iField as usize].pExpr = ExprId::NULL;
                return pVector;
            }
        }
        pRet = sqlite3ExprDup(db, pVector, 0);
    }
    pRet
}

/// If expression pExpr is of type TK_SELECT, generate code to evaluate
/// it. Return the register in which the result is stored (or, if the
/// sub-select returns more than one column, the first in an array
/// of registers in which the result is stored).
///
/// If pExpr is not a TK_SELECT expression, return 0.
fn exprCodeSubselect(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    let mut reg = 0;
    if db.ex(pExpr).op == TK_SELECT {
        reg = sqlite3CodeSubselect(db, pExpr);
    }
    reg
}

/// Argument pVector points to a vector expression - either a TK_VECTOR
/// or TK_SELECT that returns more than one column. This function returns
/// the register number of a register that contains the value of
/// element iField of the vector.
///
/// If pVector is a TK_SELECT expression, then code for it must have
/// already been generated using the exprCodeSubselect() routine. In this
/// case parameter regSelect should be the first in an array of registers
/// containing the results of the sub-select.
///
/// If pVector is of type TK_VECTOR, then code for the requested field
/// is generated. In this case (*pRegFree) may be set to the number of
/// a temporary register to be freed by the caller before returning.
///
/// Before returning, output parameter (*ppExpr) is set to point to the
/// Expr object corresponding to element iElem of the vector.
fn exprVectorRegister(
    db: &mut Sqlite3,
    pVector: ExprId,
    iField: i32,
    regSelect: i32,
    ppExpr: &mut ExprId,
    pRegFree: &mut i32,
) -> i32 {
    let (op, iTable, pList, pSelect) = {
        let e = db.ex(pVector);
        (e.op, e.iTable, e.x.pList, e.x.pSelect)
    };
    debug_assert!(op == TK_VECTOR || op == TK_REGISTER || op == TK_SELECT || op == TK_ERROR);
    if op == TK_REGISTER {
        *ppExpr = vectorFieldSubexpr(db, pVector, iField);
        return iTable + iField;
    }
    if op == TK_SELECT {
        /* Use the temporary expression node to wrap expression iField of the
         ** sub-select in a TK_SELECT_COLUMN node. This causes the caller to
         ** use the affinity of the expression in any comparison, but not the
         ** collation sequence.  */
        let n = el_n(db, db.sel(pSelect).pEList);
        let pTmp = tmp_expr(db, TK_SELECT_COLUMN);
        {
            let t = db.exm(pTmp);
            t.pLeft = pVector;
            t.iColumn = iField as ynVar;
            t.iTable = n;
        }
        *ppExpr = pTmp;
        return regSelect + iField;
    }
    if op == TK_VECTOR {
        *ppExpr = el_e(db, pList, iField);
        return sqlite3ExprCodeTemp(db, *ppExpr, pRegFree);
    }
    0
}

/// Expression pExpr is a comparison between two vector values. Compute
/// the result of the comparison (1, 0, or NULL) and write that
/// result into register dest.
///
/// The caller must satisfy the following preconditions:
///
///    if pExpr->op==TK_IS:      op==TK_EQ and p5==SQLITE_NULLEQ
///    if pExpr->op==TK_ISNOT:   op==TK_NE and p5==SQLITE_NULLEQ
///    otherwise:                op==pExpr->op and p5==0
fn codeVectorCompare(db: &mut Sqlite3, pExpr: ExprId, dest: i32, op: u8, p5: u8) {
    let (pLeft, pRight, isCommuted) = {
        let e = db.ex(pExpr);
        (e.pLeft, e.pRight, e.HasProperty(EP_Commuted))
    };
    let nLeft = exprVectorSize(db, pLeft);
    let regLeft;
    let regRight;
    let mut opx = op;
    let mut addrCmp = 0;
    let addrDone = sqlite3VdbeMakeLabel(db);

    if nerr(db) != 0 {
        return;
    }
    if nLeft != exprVectorSize(db, pRight) {
        errmsg!(db, "row value misused");
        return;
    }
    if op == TK_LE {
        opx = TK_LT;
    }
    if op == TK_GE {
        opx = TK_GT;
    }
    if op == TK_NE {
        opx = TK_EQ;
    }

    regLeft = exprCodeSubselect(db, pLeft);
    regRight = exprCodeSubselect(db, pRight);

    sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, dest);
    let mut i = 0;
    loop {
        let mut regFree1 = 0;
        let mut regFree2 = 0;
        let mut pL = ExprId::NULL;
        let mut pR = ExprId::NULL;
        debug_assert!(i >= 0 && i < nLeft);
        if addrCmp != 0 {
            sqlite3VdbeJumpHere(db.v(), addrCmp);
        }
        let r1 = exprVectorRegister(db, pLeft, i, regLeft, &mut pL, &mut regFree1);
        let r2 = exprVectorRegister(db, pRight, i, regRight, &mut pR, &mut regFree2);
        addrCmp = sqlite3VdbeCurrentAddr(db.v());
        codeCompare(db, pL, pR, opx, r1, r2, addrDone, p5 as i32, isCommuted);
        sqlite3ReleaseTempReg(db, regFree1);
        sqlite3ReleaseTempReg(db, regFree2);
        if (opx == TK_LT || opx == TK_GT) && i < nLeft - 1 {
            addrCmp = sqlite3VdbeAddOp0(db.v(), OP_ElseEq);
        }
        if p5 as u16 == SQLITE_NULLEQ {
            sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, dest);
        } else {
            sqlite3VdbeAddOp3(db.v(), OP_ZeroOrNull, r1, dest, r2);
        }
        if i == nLeft - 1 {
            break;
        }
        if opx == TK_EQ {
            sqlite3VdbeAddOp2(db.v(), OP_NotNull, dest, addrDone);
        } else {
            debug_assert!(op == TK_LT || op == TK_GT || op == TK_LE || op == TK_GE);
            sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, addrDone);
            if i == nLeft - 2 {
                opx = op;
            }
        }
        i += 1;
    }
    sqlite3VdbeJumpHere(db.v(), addrCmp);
    sqlite3VdbeResolveLabel(db.v(), addrDone);
    if op == TK_NE {
        sqlite3VdbeAddOp2(db.v(), OP_Not, dest, dest);
    }
}

// ---------------------------------------------------------------------------
// Expression height
// ---------------------------------------------------------------------------

/// Check that argument nHeight is less than or equal to the maximum
/// expression depth allowed. If it is not, leave an error message in
/// pParse.
pub fn sqlite3ExprCheckHeight(db: &mut Sqlite3, nHeight: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mxHeight = db.aLimit[SQLITE_LIMIT_EXPR_DEPTH as usize];
    if nHeight > mxHeight {
        errmsg!(
            db,
            "Expression tree is too large (maximum depth %d)",
            mxHeight
        );
        rc = SQLITE_ERROR;
    }
    rc
}

/* The following three functions, heightOfExpr(), heightOfExprList()
** and heightOfSelect(), are used to determine the maximum height
** of any expression tree referenced by the structure passed as the
** first argument.
**
** If this maximum height is greater than the current value pointed
** to by pnHeight, the second parameter, then set *pnHeight to that
** value.
*/
fn heightOfExpr(db: &Sqlite3, p: ExprId, pnHeight: &mut i32) {
    if p.nn() {
        let h = db.ex(p).nHeight;
        if h > *pnHeight {
            *pnHeight = h;
        }
    }
}
fn heightOfExprList(db: &Sqlite3, p: ExprListId, pnHeight: &mut i32) {
    if p.nn() {
        for it in db.el(p).a.iter() {
            heightOfExpr(db, it.pExpr, pnHeight);
        }
    }
}
fn heightOfSelect(db: &Sqlite3, pSelect: SelectId, pnHeight: &mut i32) {
    let mut p = pSelect;
    while p.nn() {
        let s = db.sel(p);
        heightOfExpr(db, s.pWhere, pnHeight);
        heightOfExpr(db, s.pHaving, pnHeight);
        heightOfExpr(db, s.pLimit, pnHeight);
        heightOfExprList(db, s.pEList, pnHeight);
        heightOfExprList(db, s.pGroupBy, pnHeight);
        heightOfExprList(db, s.pOrderBy, pnHeight);
        p = s.pPrior;
    }
}

/// Set the Expr.nHeight variable in the structure passed as an
/// argument. An expression with no children, Expr.pList or
/// Expr.pSelect member has a height of 1. Any other expression
/// has a height equal to the maximum height of any other
/// referenced Expr plus one.
///
/// Also propagate EP_Propagate flags up from Expr.x.pList to Expr.flags,
/// if appropriate.
fn exprSetHeight(db: &mut Sqlite3, p: ExprId) {
    let (pLeft, pRight, isSel, pSelect, pList) = {
        let e = db.ex(p);
        (e.pLeft, e.pRight, e.UseXSelect(), e.x.pSelect, e.x.pList)
    };
    let mut nHeight = if pLeft.nn() { db.ex(pLeft).nHeight } else { 0 };
    if pRight.nn() && db.ex(pRight).nHeight > nHeight {
        nHeight = db.ex(pRight).nHeight;
    }
    let mut addFlags = 0;
    if isSel {
        heightOfSelect(db, pSelect, &mut nHeight);
    } else if pList.nn() {
        heightOfExprList(db, pList, &mut nHeight);
        addFlags = EP_Propagate & sqlite3ExprListFlags(db, pList);
    }
    let e = db.exm(p);
    e.flags |= addFlags;
    e.nHeight = nHeight + 1;
}

/// Set the Expr.nHeight variable using the exprSetHeight() function. If
/// the height is greater than the maximum allowed expression depth,
/// leave an error in pParse.
///
/// Also propagate all EP_Propagate flags from the Expr.x.pList into
/// Expr.flags.
pub fn sqlite3ExprSetHeightAndFlags(db: &mut Sqlite3, p: ExprId) {
    if nerr(db) != 0 {
        return;
    }
    exprSetHeight(db, p);
    let h = db.ex(p).nHeight;
    sqlite3ExprCheckHeight(db, h);
}

/// Return the maximum height of any expression tree referenced
/// by the select statement passed as an argument.
pub fn sqlite3SelectExprHeight(db: &mut Sqlite3, p: SelectId) -> i32 {
    let mut nHeight = 0;
    heightOfSelect(db, p, &mut nHeight);
    nHeight
}

/// Set the error offset for an Expr node, if possible.
pub fn sqlite3ExprSetErrorOffset(db: &mut Sqlite3, pExpr: ExprId, iOfst: i32) {
    if pExpr.is_null() {
        return;
    }
    if db.ex(pExpr).UseWJoin() {
        return;
    }
    db.exm(pExpr).w.iOfst = iOfst;
}

// ---------------------------------------------------------------------------
// Expression constructors
// ---------------------------------------------------------------------------

/// The core of sqlite3ExprAlloc(): `tok` is None for a NULL token.
fn exprAlloc(db: &mut Sqlite3, op: i32, tok: Option<&[u8]>, dequote: i32) -> ExprId {
    let pNew = db.arena.new_expr(Expr {
        op: op as u8,
        iAgg: -1,
        nHeight: 1,
        u: ExprU {
            zToken: tok.map(|t| t.to_vec()),
            iValue: 0,
        },
        ..Expr::default()
    });
    if let Some(t) = tok {
        if dequote != 0 && !t.is_empty() && crate::global::sqlite3Isquote(t[0]) {
            crate::util::sqlite3DequoteExpr(db, pNew);
        }
    }
    pNew
}

/// This routine is the core allocator for Expr nodes.
///
/// Construct a new expression node and return a pointer to it.  Memory
/// for this node and for the pToken argument is a single allocation
/// obtained from sqlite3DbMalloc().  The calling function
/// is responsible for making sure the node eventually gets freed.
///
/// If dequote is true, then the token (if it exists) is dequoted.
/// If dequote is false, no dequoting is performed.  The deQuote
/// parameter is ignored if pToken is NULL or if the token does not
/// appear to be quoted.  If the quotes were of the form "..." (double-quotes)
/// then the EP_DblQuoted flag is set on the expression node.
///
/// A token whose `z` is None plays the role of the C NULL pToken (no
/// u.zToken at all).
pub fn sqlite3ExprAlloc(db: &mut Sqlite3, op: i32, pToken: &Token, dequote: i32) -> ExprId {
    if pToken.z.is_none() {
        exprAlloc(db, op, None, dequote)
    } else {
        let t = pToken.text().to_vec();
        exprAlloc(db, op, Some(&t), dequote)
    }
}

/// Allocate a new expression node from a zero-terminated token that has
/// already been dequoted.
pub fn sqlite3Expr(db: &mut Sqlite3, op: i32, zToken: Option<&[u8]>) -> ExprId {
    /* A NULL zToken still yields an empty (non-NULL) u.zToken, as in C */
    exprAlloc(db, op, Some(zToken.unwrap_or(b"")), 0)
}

/// Allocate an expression for a 32-bit signed integer literal.
pub fn sqlite3ExprInt32(db: &mut Sqlite3, iVal: i32) -> ExprId {
    db.arena.new_expr(Expr {
        op: TK_INTEGER,
        iAgg: -1,
        flags: EP_IntValue | EP_Leaf | (if iVal != 0 { EP_IsTrue } else { EP_IsFalse }),
        u: ExprU {
            zToken: None,
            iValue: iVal,
        },
        nHeight: 1,
        ..Expr::default()
    })
}

/// Attach subtrees pLeft and pRight to the Expr node pRoot.
///
/// If pRoot==NULL that means that a memory allocation error has occurred.
/// In that case, delete the subtrees pLeft and pRight.
pub fn sqlite3ExprAttachSubtrees(db: &mut Sqlite3, pRoot: ExprId, pLeft: ExprId, pRight: ExprId) {
    if pRoot.is_null() {
        sqlite3ExprDelete(db, pLeft);
        sqlite3ExprDelete(db, pRight);
    } else {
        if pRight.nn() {
            let (f, h) = {
                let r = db.ex(pRight);
                (r.flags, r.nHeight)
            };
            let e = db.exm(pRoot);
            e.pRight = pRight;
            e.flags |= EP_Propagate & f;
            e.nHeight = h + 1;
        } else {
            db.exm(pRoot).nHeight = 1;
        }
        if pLeft.nn() {
            let (f, h) = {
                let l = db.ex(pLeft);
                (l.flags, l.nHeight)
            };
            let e = db.exm(pRoot);
            e.pLeft = pLeft;
            e.flags |= EP_Propagate & f;
            if h >= e.nHeight {
                e.nHeight = h + 1;
            }
        }
    }
}

/// Allocate an Expr node which joins as many as two subtrees.
///
/// One or both of the subtrees can be NULL.  Return a pointer to the new
/// Expr node.
pub fn sqlite3PExpr(db: &mut Sqlite3, op: i32, pLeft: ExprId, pRight: ExprId) -> ExprId {
    let p = db.arena.new_expr(Expr {
        op: (op & 0xff) as u8,
        iAgg: -1,
        ..Expr::default()
    });
    sqlite3ExprAttachSubtrees(db, p, pLeft, pRight);
    let h = db.ex(p).nHeight;
    sqlite3ExprCheckHeight(db, h);
    p
}

/// Add pSelect to the Expr.x.pSelect field.  Or, if pExpr is NULL (due
/// do a memory allocation failure) then delete the pSelect object.
pub fn sqlite3PExprAddSelect(db: &mut Sqlite3, pExpr: ExprId, pSelect: SelectId) {
    if pExpr.nn() {
        {
            let e = db.exm(pExpr);
            e.x.pSelect = pSelect;
            e.flags |= EP_xIsSelect | EP_Subquery;
        }
        sqlite3ExprSetHeightAndFlags(db, pExpr);
    } else {
        crate::select::sqlite3SelectDelete(db, pSelect);
    }
}

/// Expression list pEList is a list of vector values. This function
/// converts the contents of pEList to a VALUES(...) Select statement
/// returning 1 row for each element of the list. For example, the
/// expression list:
///
///   ( (1,2), (3,4) (5,6) )
///
/// is translated to the equivalent of:
///
///   VALUES(1,2), (3,4), (5,6)
///
/// Each of the vector values in pEList must contain exactly nElem terms.
/// If a list element that is not a vector or does not contain nElem terms,
/// an error message is left in pParse.
///
/// This is used as part of processing IN(...) expressions with a list
/// of vectors on the RHS. e.g. "... IN ((1,2), (3,4), (5,6))".
pub fn sqlite3ExprListToValues(db: &mut Sqlite3, nElem: i32, pEList: ExprListId) -> SelectId {
    let mut pRet = SelectId::NULL;
    debug_assert!(nElem > 1);
    for ii in 0..el_n(db, pEList) {
        let pExpr = el_e(db, pEList, ii);
        let nExprElem = if db.ex(pExpr).op == TK_VECTOR {
            el_n(db, db.ex(pExpr).x.pList)
        } else {
            1
        };
        if nExprElem != nElem {
            errmsg!(
                db,
                "IN(...) element has %d term%s - expected %d",
                nExprElem,
                if nExprElem > 1 { "s" } else { "" },
                nElem
            );
            break;
        }
        let pList = db.ex(pExpr).x.pList;
        let pSel = crate::select::sqlite3SelectNew(
            db,
            pList,
            SrcListId::NULL,
            ExprId::NULL,
            ExprListId::NULL,
            ExprId::NULL,
            ExprListId::NULL,
            SF_Values,
            ExprId::NULL,
        );
        db.exm(pExpr).x.pList = ExprListId::NULL;
        if pSel.nn() {
            if pRet.nn() {
                let s = db.selm(pSel);
                s.op = TK_ALL;
                s.pPrior = pRet;
            }
            pRet = pSel;
        }
    }

    if pRet.nn() && db.sel(pRet).pPrior.nn() {
        db.selm(pRet).selFlags |= SF_MultiValue;
    }
    sqlite3ExprListDelete(db, pEList);
    pRet
}

/// Join two expressions using an AND operator.  If either expression is
/// NULL, then just return the other expression.
///
/// If one side or the other of the AND is known to be false, and neither side
/// is part of an ON clause, then instead of returning an AND expression,
/// just return a constant expression with a value of false.
pub fn sqlite3ExprAnd(db: &mut Sqlite3, pLeft: ExprId, pRight: ExprId) -> ExprId {
    if pLeft.is_null() {
        pRight
    } else if pRight.is_null() {
        pLeft
    } else {
        let f = db.ex(pLeft).flags | db.ex(pRight).flags;
        if (f & (EP_OuterON | EP_InnerON | EP_IsFalse | EP_HasFunc)) == EP_IsFalse && !in_rename(db)
        {
            sqlite3ExprDeferredDelete(db, pLeft);
            sqlite3ExprDeferredDelete(db, pRight);
            sqlite3ExprInt32(db, 0)
        } else {
            sqlite3PExpr(db, TK_AND as i32, pLeft, pRight)
        }
    }
}

/// Construct a new expression node for a function with multiple
/// arguments.
pub fn sqlite3ExprFunction(
    db: &mut Sqlite3,
    pList: ExprListId,
    pToken: &Token,
    eDistinct: i32,
) -> ExprId {
    let pNew = sqlite3ExprAlloc(db, TK_FUNCTION as i32, pToken, 1);
    debug_assert!(!hp(db, pNew, EP_InnerON | EP_OuterON));
    let zTail = db.ppr().zTail as i64;
    db.exm(pNew).w.iOfst = (pToken.off as i64 - zTail) as i32;
    if pList.nn()
        && el_n(db, pList) > db.aLimit[SQLITE_LIMIT_FUNCTION_ARG as usize]
        && db.ppr().nested == 0
    {
        errmsg!(db, "too many arguments on function %T", pToken);
    }
    {
        let e = db.exm(pNew);
        e.x.pList = pList;
        e.flags |= EP_HasFunc;
    }
    sqlite3ExprSetHeightAndFlags(db, pNew);
    if eDistinct == SF_Distinct as i32 {
        db.exm(pNew).flags |= EP_Distinct;
    }
    pNew
}

/// Report an error when attempting to use an ORDER BY clause within
/// the arguments of a non-aggregate function.
pub fn sqlite3ExprOrderByAggregateError(db: &mut Sqlite3, p: ExprId) {
    errmsg!(db, "ORDER BY may not be used with non-aggregate %#T()", p);
}

/// Attach an ORDER BY clause to a function call.
///
///     functionname( arguments ORDER BY sortlist )
///     \_____________________/          \______/
///             pExpr                    pOrderBy
///
/// The ORDER BY clause is inserted into a new Expr node of type TK_ORDER
/// and added to the Expr.pLeft field of the parent TK_FUNCTION node.
pub fn sqlite3ExprAddFunctionOrderBy(db: &mut Sqlite3, pExpr: ExprId, pOrderBy: ExprListId) {
    if pOrderBy.is_null() {
        return;
    }
    if pExpr.is_null() {
        sqlite3ExprListDelete(db, pOrderBy);
        return;
    }
    debug_assert!(db.ex(pExpr).op == TK_FUNCTION);
    debug_assert!(db.ex(pExpr).pLeft.is_null());
    let pList = db.ex(pExpr).x.pList;
    if pList.is_null() || el_n(db, pList) == 0 {
        /* Ignore ORDER BY on zero-argument aggregates */
        return;
    }
    if IsWindowFunc(db, pExpr) {
        sqlite3ExprOrderByAggregateError(db, pExpr);
        sqlite3ExprListDelete(db, pOrderBy);
        return;
    }
    if el_n(db, pOrderBy) > db.aLimit[SQLITE_LIMIT_COLUMN as usize] {
        errmsg!(db, "too many terms in ORDER BY clause");
        sqlite3ExprListDelete(db, pOrderBy);
        return;
    }

    let pOB = exprAlloc(db, TK_ORDER as i32, None, 0);
    {
        let o = db.exm(pOB);
        o.x.pList = pOrderBy;
        o.flags |= EP_FullSize;
    }
    db.exm(pExpr).pLeft = pOB;
}

/// Check to see if a function is usable according to current access
/// rules:
///
///    SQLITE_FUNC_DIRECT    -     Only usable from top-level SQL
///
///    SQLITE_FUNC_UNSAFE    -     Usable if TRUSTED_SCHEMA or from
///                                top-level SQL
///
/// If the function is not usable, create an error.
pub fn sqlite3ExprFunctionUsable(db: &mut Sqlite3, pExpr: ExprId, pDef: &FuncDefRef) {
    debug_assert!(!in_rename(db));
    debug_assert!((pDef.funcFlags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0);
    if hp(db, pExpr, EP_FromDDL) || (db.ppr().prepFlags as i32 & SQLITE_PREPARE_FROM_DDL) != 0 {
        if (pDef.funcFlags & SQLITE_FUNC_DIRECT) != 0 || (db.flags & SQLITE_TrustedSchema) == 0 {
            /* Functions prohibited in triggers and views if:
             **     (1) tagged with SQLITE_DIRECTONLY
             **     (2) not tagged with SQLITE_INNOCUOUS (which means it
             **         is tagged with SQLITE_FUNC_UNSAFE) and
             **         SQLITE_DBCONFIG_TRUSTED_SCHEMA is off (meaning
             **         that the schema is possibly tainted).
             */
            errmsg!(db, "unsafe use of %#T()", pExpr);
        }
    }
}

/// Assign a variable number to an expression that encodes a wildcard
/// in the original SQL statement.
///
/// Wildcards consisting of a single "?" are assigned the next sequential
/// variable number.
///
/// Wildcards of the form "?nnn" are assigned the number "nnn".  We make
/// sure "nnn" is not too big to avoid a denial of service attack when
/// the SQL statement comes from an external source.
///
/// Wildcards of the form ":aaa", "@aaa", or "$aaa" are assigned the same number
/// as the previous instance of the same wildcard.  Or if this is the first
/// instance of the wildcard, the next sequential variable number is
/// assigned.
pub fn sqlite3ExprAssignVarNumber(db: &mut Sqlite3, pExpr: ExprId, n: u32) {
    if pExpr.is_null() {
        return;
    }
    debug_assert!(!hp(db, pExpr, EP_IntValue | EP_Reduced | EP_TokenOnly));
    let z: ZStr = db.ex(pExpr).token().to_vec();
    debug_assert!(!z.is_empty());
    let x: ynVar;
    let mxVar = db.aLimit[SQLITE_LIMIT_VARIABLE_NUMBER as usize];
    if z.len() == 1 {
        /* Wildcard of the form "?".  Assign the next variable number */
        debug_assert!(z[0] == b'?');
        let pp = db.pp();
        pp.nVar = pp.nVar.wrapping_add(1);
        x = pp.nVar;
    } else {
        let mut doAdd = false;
        if z[0] == b'?' {
            /* Wildcard of the form "?nnn".  Convert "nnn" to an integer and
             ** use it as the variable number */
            let mut i: i64 = 0;
            let bOk;
            if n == 2 {
                /*OPTIMIZATION-IF-TRUE*/
                i = (z[1] as i64) - (b'0' as i64); /* The common case of ?N for a single digit N */
                bOk = true;
            } else {
                bOk = 0
                    == crate::util::sqlite3Atoi64(Some(&z[1..]), &mut i, n as i32 - 1, SQLITE_UTF8);
            }
            if !bOk || i < 1 || i > mxVar as i64 {
                errmsg!(db, "variable number must be between ?1 and ?%d", mxVar);
                crate::printf::sqlite3RecordErrorOffsetOfExpr(db, pExpr);
                return;
            }
            x = i as ynVar;
            if x > db.ppr().nVar {
                db.pp().nVar = x;
                doAdd = true;
            } else if (x as i32) < SQLITE_VNBMC * 64 {
                doAdd = (db.ppr().aVnbmc[(x >> 6) as usize] & (1u64 << (x & 63))) == 0;
            } else if crate::util::sqlite3VListNumToName(&mut db.pp().pVList, x as i32).is_none() {
                doAdd = true;
            }
        } else {
            /* Wildcards like ":aaa", "$aaa" or "@aaa".  Reuse the same variable
             ** number as the prior appearance of the same name, or if the name
             ** has never appeared before, reuse the same variable number
             */
            let mut xx = crate::util::sqlite3VListNameToNum(&mut db.pp().pVList, Some(&z), n as i32)
                as ynVar;
            if xx == 0 {
                let pp = db.pp();
                pp.nVar = pp.nVar.wrapping_add(1);
                xx = pp.nVar;
                doAdd = true;
            }
            x = xx;
        }
        if doAdd {
            let mut vl = std::mem::take(&mut db.pp().pVList);
            let r = crate::util::sqlite3VListAdd(db, &mut vl, Some(&z), n as i32, x as i32);
            db.pp().pVList = match r {
                Some(b) => *b,
                None => vl,
            };
            if (x as i32) < SQLITE_VNBMC * 64 {
                db.pp().aVnbmc[(x >> 6) as usize] |= 1u64 << (x & 63);
            }
        }
    }
    db.exm(pExpr).iColumn = x;
    if x as i32 > mxVar {
        errmsg!(db, "too many SQL variables");
        crate::printf::sqlite3RecordErrorOffsetOfExpr(db, pExpr);
    }
}

// ---------------------------------------------------------------------------
// Deletion.  Nothing is freed (the arena is reset at the end of the parse)
// but the side effects of the C routines are kept: window objects are
// unlinked from their SELECT and subqueries go through sqlite3SelectDelete().
// ---------------------------------------------------------------------------

/// Recursively delete an expression tree.
fn sqlite3ExprDeleteNN(db: &mut Sqlite3, p: ExprId) {
    let mut p = p;
    loop {
        let (flags, op, pLeft, pRight, pSelect, pList, pWin) = {
            let e = db.ex(p);
            (
                e.flags,
                e.op,
                e.pLeft,
                e.pRight,
                e.x.pSelect,
                e.x.pList,
                e.y.pWin,
            )
        };
        if (flags & (EP_TokenOnly | EP_Leaf)) == 0 {
            /* The Expr.x union is never used at the same time as Expr.pRight */
            if pRight.nn() {
                sqlite3ExprDeleteNN(db, pRight);
            } else if (flags & EP_xIsSelect) != 0 {
                crate::select::sqlite3SelectDelete(db, pSelect);
            } else {
                sqlite3ExprListDelete(db, pList);
                if (flags & EP_WinFunc) != 0 {
                    crate::window::sqlite3WindowDelete(db, pWin);
                }
            }
            if pLeft.nn() && op != TK_SELECT_COLUMN {
                if (flags & EP_Static) == 0 && !hp(db, pLeft, EP_Static) {
                    /* Avoid unnecessary recursion on unary operators */
                    p = pLeft;
                    continue;
                } else {
                    sqlite3ExprDeleteNN(db, pLeft);
                }
            }
        }
        break;
    }
}

/// C: `void sqlite3ExprDelete(sqlite3 *db, Expr *p)`
pub fn sqlite3ExprDelete(db: &mut Sqlite3, p: ExprId) {
    if p.nn() {
        sqlite3ExprDeleteNN(db, p);
    }
}

/// C: `void sqlite3ExprDeleteGeneric(sqlite3 *db, void *p)`
///
/// The generic (parser cleanup) form: runs when the parse ends, at which
/// point the arena is reset, so there is nothing left to do.
pub fn sqlite3ExprDeleteGeneric(db: &mut Sqlite3, p: &mut [u8]) {
    let _ = (db, p);
}

/// Clear both elements of an OnOrUsing object
pub fn sqlite3ClearOnOrUsing(db: &mut Sqlite3, p: &mut OnOrUsing) {
    if p.pOn.nn() {
        sqlite3ExprDeleteNN(db, p.pOn);
    } else if p.pUsing.nn() {
        crate::build::sqlite3IdListDelete(db, p.pUsing);
    }
}

/// Arrange to cause pExpr to be deleted when the pParse is deleted.
/// This is similar to sqlite3ExprDelete() except that the delete is
/// deferred until the pParse is deleted.
///
/// Return 0 if the delete was successfully deferred.  (In the arena, all
/// nodes die together with the parse, so the deferral always succeeds.)
pub fn sqlite3ExprDeferredDelete(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    let _ = (db, pExpr);
    0
}

/// Invoke sqlite3RenameExprUnmap() and sqlite3ExprDelete() on the
/// expression.
pub fn sqlite3ExprUnmapAndDelete(db: &mut Sqlite3, p: ExprId) {
    if p.nn() {
        if in_rename(db) {
            crate::alter::sqlite3RenameExprUnmap(db, p);
        }
        sqlite3ExprDeleteNN(db, p);
    }
}

// ---------------------------------------------------------------------------
// Duplication
// ---------------------------------------------------------------------------

/// The dupedExprStructSize() routine returns the EP_xxx flags that indicate
/// what the structure size of a copy should be:
///
///      0 (EXPR_FULLSIZE), EP_Reduced or EP_TokenOnly
///
/// Note that with flags==EXPRDUP_REDUCE, this routines works on full-size
/// (unreduced) Expr objects as they or originally constructed by the parser.
/// During expression analysis, extra information is computed and moved into
/// later parts of the Expr object and that extra information might get chopped
/// off if the expression is reduced.  Note also that it does not work to
/// make an EXPRDUP_REDUCE copy of a reduced expression.  It is only legal
/// to reduce a pristine expression tree from the parser.
fn dupedExprStructSize(p: &Expr, flags: i32) -> u32 {
    debug_assert!(flags == EXPRDUP_REDUCE || flags == 0);
    if 0 == flags || p.HasProperty(EP_FullSize) {
        0
    } else {
        debug_assert!(!p.HasProperty(EP_TokenOnly | EP_Reduced));
        debug_assert!(!p.HasProperty(EP_OuterON));
        if p.pLeft.nn() || p.x.pList.nn() || p.x.pSelect.nn() {
            EP_Reduced
        } else {
            debug_assert!(p.pRight.is_null());
            EP_TokenOnly
        }
    }
}

/// Clear the fields that do not exist in an EXPR_REDUCEDSIZE Expr (those
/// from iTable onwards).
fn truncateReduced(e: &mut Expr) {
    e.iTable = 0;
    e.iColumn = 0;
    e.iAgg = 0;
    e.w = ExprW::default();
    e.pAggInfo = AggInfoId::NULL;
    e.y = ExprY::default();
}

/// Clear the fields that do not exist in an EXPR_TOKENONLYSIZE Expr (those
/// from pLeft onwards).
fn truncateTokenOnly(e: &mut Expr) {
    e.pLeft = ExprId::NULL;
    e.pRight = ExprId::NULL;
    e.x = ExprX::default();
    e.nHeight = 0;
    truncateReduced(e);
}

/// This function is similar to sqlite3ExprDup().  In the arena every node
/// is a separate object, so the EdupBuf of the C code is not needed; the
/// EP_Reduced / EP_TokenOnly / EP_Static flags and the truncation of the
/// reduced structures are reproduced, as they influence later processing.
fn exprDup(db: &mut Sqlite3, p: ExprId, dupFlags: i32, isChild: bool) -> ExprId {
    debug_assert!(dupFlags == 0 || dupFlags == EXPRDUP_REDUCE);
    let src = db.ex(p).clone();
    let nStructSize = dupedExprStructSize(&src, dupFlags);
    let mut new = src.clone();
    if dupFlags != 0 {
        if nStructSize == EP_TokenOnly {
            truncateTokenOnly(&mut new);
        } else if nStructSize == EP_Reduced {
            truncateReduced(&mut new);
        }
    } else if src.HasProperty(EP_TokenOnly) {
        truncateTokenOnly(&mut new);
    } else if src.HasProperty(EP_Reduced) {
        truncateReduced(&mut new);
    }

    /* Set the EP_Reduced, EP_TokenOnly, and EP_Static flags appropriately. */
    new.flags &= !(EP_Reduced | EP_TokenOnly | EP_Static);
    new.flags |= nStructSize;
    if isChild {
        new.flags |= EP_Static;
    }
    let newFlags = new.flags;
    let pNew = db.arena.new_expr(new);

    if ((src.flags | newFlags) & (EP_TokenOnly | EP_Leaf)) == 0 {
        /* Fill in the pNew->x.pSelect or pNew->x.pList member. */
        if src.UseXSelect() {
            let s = sqlite3SelectDup(db, src.x.pSelect, dupFlags);
            db.exm(pNew).x.pSelect = s;
        } else {
            let l = sqlite3ExprListDup(
                db,
                src.x.pList,
                if src.op != TK_ORDER { dupFlags } else { 0 },
            );
            db.exm(pNew).x.pList = l;
        }

        if src.HasProperty(EP_WinFunc) {
            let w = crate::window::sqlite3WindowDup(db, pNew, src.y.pWin);
            db.exm(pNew).y.pWin = w;
        }

        /* Fill in pNew->pLeft and pNew->pRight. */
        let reduce = dupFlags != 0;
        let l = if src.op == TK_SELECT_COLUMN {
            src.pLeft
        } else if src.pLeft.nn() {
            exprDup(db, src.pLeft, dupFlags, reduce)
        } else {
            ExprId::NULL
        };
        let r = if src.pRight.nn() {
            exprDup(db, src.pRight, dupFlags, reduce)
        } else {
            ExprId::NULL
        };
        let n = db.exm(pNew);
        n.pLeft = l;
        n.pRight = r;
    }
    pNew
}

/// Create and return a deep copy of the object passed as the second
/// argument.
pub fn sqlite3WithDup(db: &mut Sqlite3, p: WithId) -> WithId {
    if p.is_null() {
        return WithId::NULL;
    }
    let ctes = db.arena.with(p).a.clone();
    let mut a = Vec::with_capacity(ctes.len());
    for c in ctes.iter() {
        let pSelect = sqlite3SelectDup(db, c.pSelect, 0);
        let pCols = sqlite3ExprListDup(db, c.pCols, 0);
        a.push(Cte {
            zName: c.zName.clone(),
            pCols,
            pSelect,
            zCteErr: None,
            pUse: CteUseId::NULL,
            eM10d: c.eM10d,
        });
    }
    db.arena.new_with(With {
        bView: 0,
        pOuter: WithId::NULL,
        a,
    })
}

/* The gatherSelectWindows() procedure and its helper routine
** gatherSelectWindowsCallback() are used to scan all the expressions
** an a newly duplicated SELECT statement and gather all of the Window
** objects found there, assembling them onto the linked list at Select->pWin.
*/
fn gatherSelectWindowsCallback(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, flags, pWin) = {
        let e = w.db.ex(pExpr);
        (e.op, e.flags, e.y.pWin)
    };
    if op == TK_FUNCTION && (flags & EP_WinFunc) != 0 {
        let pSelect = w.u.pSelect;
        debug_assert!(pWin.nn());
        crate::window::sqlite3WindowLink(w.db, pSelect, pWin);
    }
    WRC_Continue
}
fn gatherSelectWindowsSelectCallback(w: &mut Walker, p: SelectId) -> i32 {
    if p == w.u.pSelect {
        WRC_Continue
    } else {
        WRC_Prune
    }
}
fn gatherSelectWindows(db: &mut Sqlite3, p: SelectId) {
    let mut w = Walker::new(db);
    w.xExprCallback = Some(gatherSelectWindowsCallback);
    w.xSelectCallback = Some(gatherSelectWindowsSelectCallback);
    w.xSelectCallback2 = None;
    w.pParse = false;
    w.u.pSelect = p;
    sqlite3WalkSelect(&mut w, p);
}

/// The following group of routines make deep copies of expressions,
/// expression lists, ID lists, and select statements.  The copies can
/// be deleted (by being passed to their respective ...Delete() routines)
/// without effecting the originals.
///
/// Any tables that the SrcList might point to are not duplicated.
///
/// The flags parameter contains a combination of the EXPRDUP_XXX flags.
/// If the EXPRDUP_REDUCE flag is set, then the structure returned is a
/// truncated version of the usual Expr structure that will be stored as
/// part of the in-memory representation of the database schema.
pub fn sqlite3ExprDup(db: &mut Sqlite3, p: ExprId, flags: i32) -> ExprId {
    debug_assert!(flags == 0 || flags == EXPRDUP_REDUCE);
    if p.nn() {
        exprDup(db, p, flags, false)
    } else {
        ExprId::NULL
    }
}

/// C: `ExprList * sqlite3ExprListDup(sqlite3 *db, const ExprList *p, int flags)`
pub fn sqlite3ExprListDup(db: &mut Sqlite3, p: ExprListId, flags: i32) -> ExprListId {
    if p.is_null() {
        return ExprListId::NULL;
    }
    let old = db.el(p).a.clone();
    let mut a = Vec::with_capacity(old.len());
    let mut pPriorSelectColOld = ExprId::NULL;
    let mut pPriorSelectColNew = ExprId::NULL;
    for pOldItem in old.iter() {
        let pOldExpr = pOldItem.pExpr;
        let pNewExpr = sqlite3ExprDup(db, pOldExpr, flags);
        if pOldExpr.nn() && db.ex(pOldExpr).op == TK_SELECT_COLUMN && pNewExpr.nn() {
            if db.ex(pNewExpr).pRight.nn() {
                pPriorSelectColOld = db.ex(pOldExpr).pRight;
                pPriorSelectColNew = db.ex(pNewExpr).pRight;
                db.exm(pNewExpr).pLeft = pPriorSelectColNew;
            } else {
                let ol = db.ex(pOldExpr).pLeft;
                if ol != pPriorSelectColOld {
                    pPriorSelectColOld = ol;
                    pPriorSelectColNew = sqlite3ExprDup(db, pPriorSelectColOld, flags);
                    db.exm(pNewExpr).pRight = pPriorSelectColNew;
                }
                db.exm(pNewExpr).pLeft = pPriorSelectColNew;
            }
        }
        a.push(ExprListItem {
            pExpr: pNewExpr,
            zEName: pOldItem.zEName.clone(),
            fg: pOldItem.fg,
            u: pOldItem.u,
        });
    }
    db.arena.new_elist(ExprList { a })
}

/// C: `SrcList * sqlite3SrcListDup(sqlite3 *db, const SrcList *p, int flags)`
pub fn sqlite3SrcListDup(db: &mut Sqlite3, p: SrcListId, flags: i32) -> SrcListId {
    if p.is_null() {
        return SrcListId::NULL;
    }
    let old = db.src(p).a.clone();
    let mut a = Vec::with_capacity(old.len());
    for pOldItem in old.iter() {
        let mut pNewItem = SrcItem::default();
        pNewItem.fg = pOldItem.fg;
        if pOldItem.fg.isSubquery != 0 {
            match pOldItem.u4.pSubq.as_ref() {
                Some(q) => {
                    let mut pNewSubq: Subquery = **q;
                    pNewSubq.pSelect = sqlite3SelectDup(db, pNewSubq.pSelect, flags);
                    if pNewSubq.pSelect.is_null() {
                        pNewItem.fg.isSubquery = 0;
                        pNewItem.u4.pSubq = None;
                    } else {
                        pNewItem.u4.pSubq = Some(Box::new(pNewSubq));
                    }
                }
                None => {
                    pNewItem.fg.isSubquery = 0;
                }
            }
        } else if pOldItem.fg.fixedSchema != 0 {
            pNewItem.u4.pSchema = pOldItem.u4.pSchema;
        } else {
            pNewItem.u4.zDatabase = pOldItem.u4.zDatabase.clone();
        }
        pNewItem.zName = pOldItem.zName.clone();
        pNewItem.zAlias = pOldItem.zAlias.clone();
        pNewItem.iCursor = pOldItem.iCursor;
        if pNewItem.fg.isIndexedBy != 0 {
            pNewItem.u1.zIndexedBy = pOldItem.u1.zIndexedBy.clone();
        } else if pNewItem.fg.isTabFunc != 0 {
            pNewItem.u1.pFuncArg = sqlite3ExprListDup(db, pOldItem.u1.pFuncArg, flags);
        } else {
            pNewItem.u1.nRow = pOldItem.u1.nRow;
        }
        pNewItem.u2 = pOldItem.u2.clone();
        if pNewItem.fg.isCte != 0 && pNewItem.u2.pCteUse.nn() {
            db.arena.cteusem(pNewItem.u2.pCteUse).nUse += 1;
        }
        pNewItem.pSTab = pOldItem.pSTab.clone();
        if let Some(t) = &pNewItem.pSTab {
            t.borrow_mut().nTabRef += 1;
        }
        if pOldItem.fg.isUsing != 0 {
            pNewItem.u3.pUsing = sqlite3IdListDup(db, pOldItem.u3.pUsing);
        } else {
            pNewItem.u3.pOn = sqlite3ExprDup(db, pOldItem.u3.pOn, flags);
        }
        pNewItem.colUsed = pOldItem.colUsed;
        a.push(pNewItem);
    }
    db.arena.new_srclist(SrcList { a })
}

/// C: `IdList * sqlite3IdListDup(sqlite3 *db, const IdList *p)`
pub fn sqlite3IdListDup(db: &mut Sqlite3, p: IdListId) -> IdListId {
    if p.is_null() {
        return IdListId::NULL;
    }
    let a = db.idl(p).a.clone();
    db.arena.new_idlist(IdList { a })
}

/// C: `Select * sqlite3SelectDup(sqlite3 *db, const Select *pDup, int flags)`
pub fn sqlite3SelectDup(db: &mut Sqlite3, pDup: SelectId, flags: i32) -> SelectId {
    let mut pRet = SelectId::NULL;
    let mut pNext = SelectId::NULL;
    let mut pLast = SelectId::NULL; /* C: pp points at pLast->pPrior */
    let mut p = pDup;
    while p.nn() {
        let s = db.sel(p).clone();
        let pNew = db.arena.new_select(Select::default());
        let pEList = sqlite3ExprListDup(db, s.pEList, flags);
        let pSrc = sqlite3SrcListDup(db, s.pSrc, flags);
        let pWhere = sqlite3ExprDup(db, s.pWhere, flags);
        let pGroupBy = sqlite3ExprListDup(db, s.pGroupBy, flags);
        let pHaving = sqlite3ExprDup(db, s.pHaving, flags);
        let pOrderBy = sqlite3ExprListDup(db, s.pOrderBy, flags);
        let pLimit = sqlite3ExprDup(db, s.pLimit, flags);
        let pWith = sqlite3WithDup(db, s.pWith);
        {
            let n = db.selm(pNew);
            n.pEList = pEList;
            n.pSrc = pSrc;
            n.pWhere = pWhere;
            n.pGroupBy = pGroupBy;
            n.pHaving = pHaving;
            n.pOrderBy = pOrderBy;
            n.op = s.op;
            n.pNext = pNext;
            n.pPrior = SelectId::NULL;
            n.pLimit = pLimit;
            n.iLimit = 0;
            n.iOffset = 0;
            n.selFlags = s.selFlags;
            n.nSelectRow = s.nSelectRow;
            n.pWith = pWith;
            n.pWin = WindowId::NULL;
        }
        let pWinDefn = crate::window::sqlite3WindowListDup(db, s.pWinDefn);
        db.selm(pNew).pWinDefn = pWinDefn;
        if s.pWin.nn() && db.mallocFailed == 0 {
            gatherSelectWindows(db, pNew);
        }
        db.selm(pNew).selId = s.selId;
        if pLast.is_null() {
            pRet = pNew;
        } else {
            db.selm(pLast).pPrior = pNew;
        }
        pLast = pNew;
        pNext = pNew;
        p = s.pPrior;
    }
    pRet
}

// ---------------------------------------------------------------------------
// Expression lists
// ---------------------------------------------------------------------------

/// Add a new element to the end of an expression list.  If pList is
/// initially NULL, then create a new expression list.
pub fn sqlite3ExprListAppendNew(db: &mut Sqlite3, pExpr: ExprId) -> ExprListId {
    let mut a = Vec::with_capacity(4);
    a.push(ExprListItem {
        pExpr,
        ..ExprListItem::default()
    });
    db.arena.new_elist(ExprList { a })
}

/// C: `SQLITE_NOINLINE ExprList * sqlite3ExprListAppendGrow( sqlite3 *db, ExprList *pList, Expr *pExpr )`
pub fn sqlite3ExprListAppendGrow(db: &mut Sqlite3, pList: ExprListId, pExpr: ExprId) -> ExprListId {
    db.elm(pList).a.push(ExprListItem {
        pExpr,
        ..ExprListItem::default()
    });
    pList
}

/// C: `ExprList * sqlite3ExprListAppend( Parse *pParse, ExprList *pList, Expr *pExpr )`
pub fn sqlite3ExprListAppend(db: &mut Sqlite3, pList: ExprListId, pExpr: ExprId) -> ExprListId {
    if pList.is_null() {
        return sqlite3ExprListAppendNew(db, pExpr);
    }
    sqlite3ExprListAppendGrow(db, pList, pExpr)
}

/// pColumns and pExpr form a vector assignment which is part of the SET
/// clause of an UPDATE statement.  Like this:
///
///        (a,b,c) = (expr1,expr2,expr3)
/// Or:    (a,b,c) = (SELECT x,y,z FROM ....)
///
/// For each term of the vector assignment, append new entries to the
/// expression list pList.  In the case of a subquery on the RHS, append
/// TK_SELECT_COLUMN expressions.
pub fn sqlite3ExprListAppendVector(
    db: &mut Sqlite3,
    pList: ExprListId,
    pColumns: IdListId,
    pExpr: ExprId,
) -> ExprListId {
    let mut pList = pList;
    let mut pExpr = pExpr;
    let iFirst = el_n(db, pList);
    'error: {
        /* pColumns can only be NULL due to an OOM but an OOM will cause an
         ** exit prior to this routine being invoked */
        if pColumns.is_null() {
            break 'error;
        }
        if pExpr.is_null() {
            break 'error;
        }
        let nId = db.idl(pColumns).nId();

        /* If the RHS is a vector, then we can immediately check to see that
         ** the size of the RHS and LHS match.  But if the RHS is a SELECT,
         ** wildcards ("*") in the result set of the SELECT must be expanded before
         ** we can do the size check, so defer the size check until code generation.
         */
        if db.ex(pExpr).op != TK_SELECT {
            let n = exprVectorSize(db, pExpr);
            if nId != n {
                errmsg!(db, "%d columns assigned %d values", nId, n);
                break 'error;
            }
        }

        for i in 0..nId {
            let pSubExpr = sqlite3ExprForVectorField(db, pExpr, i, nId);
            if pSubExpr.is_null() {
                continue;
            }
            pList = sqlite3ExprListAppend(db, pList, pSubExpr);
            if pList.nn() {
                debug_assert!(el_n(db, pList) == iFirst + i + 1);
                let name = db.idlm(pColumns).a[i as usize].zName.take();
                db.elm(pList).a.last_mut().unwrap().zEName = name;
            }
        }

        if db.mallocFailed == 0 && db.ex(pExpr).op == TK_SELECT && pList.nn() {
            let pFirst = el_e(db, pList, iFirst);
            debug_assert!(pFirst.nn());
            debug_assert!(db.ex(pFirst).op == TK_SELECT_COLUMN);

            /* Store the SELECT statement in pRight so it will be deleted when
             ** sqlite3ExprListDelete() is called */
            let f = db.exm(pFirst);
            f.pRight = pExpr;
            pExpr = ExprId::NULL;

            /* Remember the size of the LHS in iTable so that we can check that
             ** the RHS and LHS sizes match during code generation. */
            f.iTable = nId;
        }
    }

    /* vector_append_error: */
    sqlite3ExprUnmapAndDelete(db, pExpr);
    crate::build::sqlite3IdListDelete(db, pColumns);
    pList
}

/// Set the sort order for the last element on the given ExprList.
pub fn sqlite3ExprListSetSortOrder(db: &mut Sqlite3, p: ExprListId, iSortOrder: i32, eNulls: i32) {
    if p.is_null() {
        return;
    }
    let mut iSortOrder = iSortOrder;
    debug_assert!(el_n(db, p) > 0);
    let pItem = db.elm(p).a.last_mut().unwrap();
    debug_assert!(pItem.fg.bNulls == 0);
    if iSortOrder == SQLITE_SO_UNDEFINED as i32 {
        iSortOrder = SQLITE_SO_ASC as i32;
    }
    pItem.fg.sortFlags = iSortOrder as u8;

    if eNulls != SQLITE_SO_UNDEFINED as i32 {
        pItem.fg.bNulls = 1;
        if iSortOrder != eNulls {
            pItem.fg.sortFlags |= KEYINFO_ORDER_BIGNULL;
        }
    }
}

/// Set the ExprList.a[].zEName element of the most recently added item
/// on the expression list.
///
/// pList might be NULL following an OOM error.  But pName should never be
/// NULL.
pub fn sqlite3ExprListSetName(db: &mut Sqlite3, pList: ExprListId, pName: &Token, dequote: i32) {
    if pList.nn() {
        debug_assert!(el_n(db, pList) > 0);
        let mut z = pName.text().to_vec();
        if dequote != 0 {
            /* If dequote==0, then pName->z does not point to part of a DDL
             ** statement handled by the parser. And so no token need be added
             ** to the token-map.  */
            crate::util::sqlite3Dequote(&mut z);
        }
        let rename = dequote != 0 && in_rename(db);
        if rename {
            let i = el_n(db, pList) - 1;
            crate::alter::sqlite3RenameTokenMap(
                db,
                crate::alter::rename_key_elist_item(pList, i as i32),
                pName,
            );
        }
        let pItem = db.elm(pList).a.last_mut().unwrap();
        debug_assert!(pItem.zEName.is_none());
        debug_assert!(pItem.fg.eEName == ENAME_NAME);
        pItem.zEName = Some(z);
    }
}

/// Set the ExprList.a[].zSpan element of the most recently added item
/// on the expression list.
///
/// The span is given as byte offsets into the SQL text of the current
/// parse (`db.ppr().zSql`): the C `zStart` and `zEnd` pointers.
pub fn sqlite3ExprListSetSpan(db: &mut Sqlite3, pList: ExprListId, zStart: usize, zEnd: usize) {
    if pList.nn() {
        debug_assert!(el_n(db, pList) > 0);
        if db.el(pList).a.last().unwrap().zEName.is_none() {
            let z = span_dup(db, zStart, zEnd);
            let pItem = db.elm(pList).a.last_mut().unwrap();
            pItem.zEName = Some(z);
            pItem.fg.eEName = ENAME_SPAN;
        }
    }
}

/// If the expression list pEList contains more than iLimit elements,
/// leave an error message in pParse.
pub fn sqlite3ExprListCheckLength(db: &mut Sqlite3, pEList: ExprListId, zObject: Option<&[u8]>) {
    let mx = db.aLimit[SQLITE_LIMIT_COLUMN as usize];
    if pEList.nn() && el_n(db, pEList) > mx {
        errmsg!(db, "too many columns in %s", zObject);
    }
}

/// Delete an entire expression list.
fn exprListDeleteNN(db: &mut Sqlite3, pList: ExprListId) {
    let n = db.el(pList).a.len();
    for i in 0..n {
        let e = db.el(pList).a[i].pExpr;
        sqlite3ExprDelete(db, e);
    }
}

/// C: `void sqlite3ExprListDelete(sqlite3 *db, ExprList *pList)`
pub fn sqlite3ExprListDelete(db: &mut Sqlite3, pList: ExprListId) {
    if pList.nn() {
        exprListDeleteNN(db, pList);
    }
}

/// C: `void sqlite3ExprListDeleteGeneric(sqlite3 *db, void *pList)`
///
/// Parser-cleanup form: runs when the arena is reset; nothing to do.
pub fn sqlite3ExprListDeleteGeneric(db: &mut Sqlite3, pList: &mut [u8]) {
    let _ = (db, pList);
}

/// Return the bitwise-OR of all Expr.flags fields in the given
/// ExprList.
pub fn sqlite3ExprListFlags(db: &mut Sqlite3, pList: ExprListId) -> u32 {
    exprListFlags(db, pList)
}

fn exprListFlags(db: &Sqlite3, pList: ExprListId) -> u32 {
    let mut m = 0u32;
    for it in db.el(pList).a.iter() {
        debug_assert!(it.pExpr.nn());
        if it.pExpr.nn() {
            m |= db.ex(it.pExpr).flags;
        }
    }
    m
}

/// This is a SELECT-node callback for the expression walker that
/// always "fails".  By "fail" in this case, we mean set
/// pWalker->eCode to zero and abort.
///
/// This callback is used by multiple expression walkers.
pub fn sqlite3SelectWalkFail(w: &mut Walker, NotUsed: SelectId) -> i32 {
    let _ = NotUsed;
    w.eCode = 0;
    WRC_Abort
}

/// Check the input string to see if it is "true" or "false" (in any case).
///
///       If the string is....           Return
///         "true"                         EP_IsTrue
///         "false"                        EP_IsFalse
///         anything else                  0
pub fn sqlite3IsTrueOrFalse(zIn: Option<&[u8]>) -> u32 {
    if crate::util::sqlite3StrICmp(zIn, Some(b"true")) == 0 {
        return EP_IsTrue;
    }
    if crate::util::sqlite3StrICmp(zIn, Some(b"false")) == 0 {
        return EP_IsFalse;
    }
    0
}

/// If the input expression is an ID with the name "true" or "false"
/// then convert it into an TK_TRUEFALSE term.  Return non-zero if
/// the conversion happened, and zero if the expression is unaltered.
pub fn sqlite3ExprIdToTrueFalse(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    let e = db.ex(pExpr);
    debug_assert!(e.op == TK_ID || e.op == TK_STRING);
    if !e.HasProperty(EP_Quoted | EP_IntValue) {
        let v = sqlite3IsTrueOrFalse(e.u.zToken.as_deref());
        if v != 0 {
            let e = db.exm(pExpr);
            e.op = TK_TRUEFALSE;
            e.flags |= v;
            return 1;
        }
    }
    0
}

fn truthValue(db: &Sqlite3, pExpr: ExprId) -> i32 {
    let p = skipCollateAndLikely(db, pExpr);
    let e = db.ex(p);
    debug_assert!(e.op == TK_TRUEFALSE);
    debug_assert!(!e.HasProperty(EP_IntValue));
    (e.token().len() == 4) as i32
}

/// The argument must be a TK_TRUEFALSE Expr node.  Return 1 if it is TRUE
/// and 0 if it is FALSE.
pub fn sqlite3ExprTruthValue(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    truthValue(db, pExpr)
}

fn simplifiedAndOr(db: &Sqlite3, pExpr: ExprId) -> ExprId {
    debug_assert!(pExpr.nn());
    let e = db.ex(pExpr);
    let mut pExpr = pExpr;
    if e.op == TK_AND || e.op == TK_OR {
        let isAnd = e.op == TK_AND;
        let pRight = simplifiedAndOr(db, e.pRight);
        let pLeft = simplifiedAndOr(db, e.pLeft);
        let (l, r) = (db.ex(pLeft), db.ex(pRight));
        if l.AlwaysTrue() || r.AlwaysFalse() {
            pExpr = if isAnd { pRight } else { pLeft };
        } else if r.AlwaysTrue() || l.AlwaysFalse() {
            pExpr = if isAnd { pLeft } else { pRight };
        }
    }
    pExpr
}

/// If pExpr is an AND or OR expression, try to simplify it by eliminating
/// terms that are always true or false.  Return the simplified expression.
/// Or return the original expression if no simplification is possible.
///
/// Examples:
///
///     (x<10) AND true                =>   (x<10)
///     (x<10) AND false               =>   false
///     (x<10) AND (y=22 OR false)     =>   (x<10) AND (y=22)
///     (x<10) AND (y=22 OR true)      =>   (x<10)
///     (y=22) OR true                 =>   true
pub fn sqlite3ExprSimplifiedAndOr(db: &mut Sqlite3, pExpr: ExprId) -> ExprId {
    simplifiedAndOr(db, pExpr)
}

/// Return true if it might be advantageous to compute the right operand
/// of expression pExpr first, before the left operand.
///
/// Normally the left operand is computed before the right operand.  But if
/// the left operand contains a subquery and the right does not, then it
/// might be more efficient to compute the right operand first.
fn exprEvalRhsFirst(db: &Sqlite3, pExpr: ExprId) -> bool {
    let e = db.ex(pExpr);
    hp(db, e.pLeft, EP_Subquery) && !hp(db, e.pRight, EP_Subquery)
}

/// Compute the two operands of a binary operator.
///
/// If either operand contains a subquery, then the code strives to
/// compute the operand containing the subquery second.  If the other
/// operand evalutes to NULL, then a jump is made.  The address of the
/// IsNull operand that does this jump is returned.  The caller can use
/// this to optimize the computation so as to avoid doing the potentially
/// expensive subquery.
///
/// If no optimization opportunities exist, return 0.
fn exprComputeOperands(
    db: &mut Sqlite3,
    pExpr: ExprId,
    pR1: &mut i32,
    pR2: &mut i32,
    pFree1: &mut i32,
    pFree2: &mut i32,
) -> i32 {
    let mut addrIsNull;
    let r1;
    let mut r2;
    let (pLeft, pRight) = {
        let e = db.ex(pExpr);
        (e.pLeft, e.pRight)
    };

    /*
     ** If the left operand contains a (possibly expensive) subquery and the
     ** right operand does not and the right operation might be NULL,
     ** then compute the right operand first and do an IsNull jump if the
     ** right operand evalutes to NULL.
     */
    if exprEvalRhsFirst(db, pExpr) && exprCanBeNull(db, pRight) {
        r2 = sqlite3ExprCodeTemp(db, pRight, pFree2);
        addrIsNull = sqlite3VdbeAddOp1(db.v(), OP_IsNull, r2);
    } else {
        r2 = 0; /* Silence a false-positive uninit-var warning in MSVC */
        addrIsNull = 0;
    }
    r1 = sqlite3ExprCodeTemp(db, pLeft, pFree1);
    if addrIsNull == 0 {
        /*
         ** If the right operand contains a subquery and the left operand does not
         ** and the left operand might be NULL, then do an IsNull check
         ** check on the left operand before computing the right operand.
         */
        if hp(db, pRight, EP_Subquery) && exprCanBeNull(db, pLeft) {
            addrIsNull = sqlite3VdbeAddOp1(db.v(), OP_IsNull, r1);
        }
        r2 = sqlite3ExprCodeTemp(db, pRight, pFree2);
    }
    *pR1 = r1;
    *pR2 = r2;
    addrIsNull
}

// ---------------------------------------------------------------------------
// Constant expressions
// ---------------------------------------------------------------------------

/// pExpr is a TK_FUNCTION node.  Try to determine whether or not the
/// function is a constant function.  A function is constant if all of
/// the following are true:
///
///    (1)  It is a scalar function (not an aggregate or window function)
///    (2)  It has either the SQLITE_FUNC_CONSTANT or SQLITE_FUNC_SLOCHNG
///         property.
///    (3)  All of its arguments are constants
///
/// This routine sets pWalker->eCode to 0 if pExpr is not a constant.
/// It makes no changes to pWalker->eCode if pExpr is constant.  In
/// every case, it returns WRC_Abort.
///
/// Called as a service subroutine from exprNodeIsConstant().
fn exprNodeIsConstantFunction(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (flags, pList, zToken) = {
        let e = w.db.ex(pExpr);
        (e.flags, e.x.pList, e.u.zToken.clone())
    };
    debug_assert!(w.db.ex(pExpr).op == TK_FUNCTION);
    let n;
    if (flags & EP_TokenOnly) != 0 || pList.is_null() {
        n = 0;
    } else {
        n = el_n(w.db, pList);
        sqlite3WalkExprList(w, pList);
        if w.eCode == 0 {
            return WRC_Abort;
        }
    }
    let enc = w.db.ENC();
    let pDef = crate::callback::sqlite3FindFunction(w.db, zToken.as_deref(), n, enc, 0);
    let bad = match &pDef {
        None => true,
        Some(d) => {
            d.xFinalize.is_some()
                || (d.funcFlags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) == 0
                || (flags & EP_WinFunc) != 0
        }
    };
    if bad {
        w.eCode = 0;
        return WRC_Abort;
    }
    WRC_Prune
}

/// These routines are Walker callbacks used to check expressions to
/// see if they are "constant" for some definition of constant.  The
/// Walker.eCode value determines the type of "constant" we are looking
/// for.
///
/// These callback routines are used to implement the following:
///
///     sqlite3ExprIsConstant()                  pWalker->eCode==1
///     sqlite3ExprIsConstantNotJoin()           pWalker->eCode==2
///     sqlite3ExprIsTableConstant()             pWalker->eCode==3
///     sqlite3ExprIsConstantOrFunction()        pWalker->eCode==4 or 5
///     sqlite3ExprListIsConstant()              pWalker->eCode==1 or 6
///
/// In all cases, the callbacks set Walker.eCode=0 and abort if the expression
/// is found to not be a constant.
///
/// The sqlite3ExprIsConstantOrFunction() is used for evaluating DEFAULT
/// expressions in a CREATE TABLE statement.  The Walker.eCode value is 5
/// when parsing an existing schema out of the sqlite_schema table and 4
/// when processing a new CREATE TABLE statement.  A bound parameter raises
/// an error for new statements, but is silently converted
/// to NULL for existing schemas.  This allows sqlite_schema tables that
/// contain a bound parameter because they were generated by older versions
/// of SQLite to be parsed by newer versions of SQLite without raising a
/// malformed schema error.
fn exprNodeIsConstant(w: &mut Walker, pExpr: ExprId) -> i32 {
    debug_assert!(w.eCode > 0);
    let (op, flags, iTable) = {
        let e = w.db.ex(pExpr);
        (e.op, e.flags, e.iTable)
    };

    /* If pWalker->eCode is 2 then any term of the expression that comes from
     ** the ON or USING clauses of an outer join disqualifies the expression
     ** from being considered constant. */
    if w.eCode == 2 && (flags & EP_OuterON) != 0 {
        w.eCode = 0;
        return WRC_Abort;
    }

    match op {
        TK_FUNCTION => {
            if (flags & EP_WinFunc) != 0 {
                w.eCode = 0; /* Window functions are never constant */
                WRC_Abort
            } else if w.eCode >= 4 && w.eCode <= 5 {
                /* Functions used within a CREATE TABLE statement are constant as
                 ** long as all of their arguments are constant */
                if w.eCode == 5 {
                    w.db.exm(pExpr).flags |= EP_FromDDL;
                }
                WRC_Continue
            } else if (flags & EP_ConstFunc) != 0 {
                /* Functions marked with EP_ConstFunc are constant as long as
                 ** all their arugments are constant and they are not window functions */
                WRC_Continue
            } else if w.pParse {
                /* Functions are constant if they have SQLITE_FUNC_CONSTANT or
                 ** SQLITE_FUNC_SLOCHNG and if all arguments are constant */
                exprNodeIsConstantFunction(w, pExpr)
            } else {
                w.eCode = 0; /* Function call is not constant */
                WRC_Abort
            }
        }
        TK_ID | TK_COLUMN | TK_AGG_FUNCTION | TK_AGG_COLUMN => {
            /* Convert "true" or "false" in a DEFAULT clause into the
             ** appropriate TK_TRUEFALSE operator */
            if op == TK_ID && sqlite3ExprIdToTrueFalse(w.db, pExpr) != 0 {
                return WRC_Prune;
            }
            if (flags & EP_FixedCol) != 0 && w.eCode != 2 {
                return WRC_Continue;
            }
            if w.eCode == 3 && iTable == w.u.iCur {
                return WRC_Continue;
            }
            w.eCode = 0;
            WRC_Abort
        }
        TK_IF_NULL_ROW | TK_REGISTER | TK_DOT | TK_RAISE => {
            w.eCode = 0;
            WRC_Abort
        }
        TK_IS | TK_ISNOT => {
            if w.eCode == 6 {
                w.eCode = 0;
                return WRC_Abort;
            }
            WRC_Continue
        }
        TK_VARIABLE => {
            if w.eCode == 5 {
                /* Silently convert bound parameters that appear inside of CREATE
                 ** statements into a NULL when parsing the CREATE statement text out
                 ** of the sqlite_schema table */
                w.db.exm(pExpr).op = TK_NULL;
            } else if w.eCode == 4 {
                /* A bound parameter in a CREATE statement that originates from
                 ** sqlite3_prepare() causes an error */
                w.eCode = 0;
                return WRC_Abort;
            }
            WRC_Continue
        }
        _ => WRC_Continue,
    }
}

fn exprIsConst(db: &mut Sqlite3, pParse: bool, p: ExprId, initFlag: u16) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = initFlag;
    w.pParse = pParse;
    w.xExprCallback = Some(exprNodeIsConstant);
    w.xSelectCallback = Some(sqlite3SelectWalkFail);
    sqlite3WalkExpr(&mut w, p);
    w.eCode as i32
}

/// Walk an expression tree.  Return non-zero if the expression is constant
/// or return zero if the expression involves variables or function calls.
///
/// For the purposes of this function, a double-quoted string (ex: "abc")
/// is considered a variable but a single-quoted string (ex: 'abc') is
/// a constant.
///
/// The pParse parameter may be false (C NULL).  But if it is false, there
/// is no way to determine if function calls are constant or not, and hence
/// all function calls will be considered to be non-constant.  If pParse is
/// true, then a function call might be constant, depending on the
/// function and on its parameters.
pub fn sqlite3ExprIsConstant(db: &mut Sqlite3, pParse: bool, p: ExprId) -> i32 {
    exprIsConst(db, pParse, p, 1)
}

/// Return true if all expressions in pList are constant. If parameter bNoIs
/// is true, do not consider expressions that contain "IS" operators to be
/// constant. IS operators are not always considered constant as expressions
/// like "x IS TRUE" need to be transformed to TK_TRUTH expression nodes,
/// which happens at the same time as column name resolution.
pub fn sqlite3ExprListIsConstant(db: &mut Sqlite3, pList: ExprListId, bNoIs: i32) -> i32 {
    let iInit = if bNoIs != 0 { 6 } else { 1 };
    for ii in 0..el_n(db, pList) {
        let e = el_e(db, pList, ii);
        if 0 == exprIsConst(db, true, e, iInit) {
            return 0;
        }
    }
    1
}

/// Walk an expression tree.  Return non-zero if
///
///   (1) the expression is constant, and
///   (2) the expression does not originate in the ON or USING clause
///       of a LEFT JOIN, and
///   (3) the expression does not contain any EP_FixedCol TK_COLUMN
///       operands created by the constant propagation optimization.
///
/// When this routine returns true, it indicates that the expression
/// can be added to the pParse->pConstExpr list and evaluated once when
/// the prepared statement starts up.  See sqlite3ExprCodeRunJustOnce().
fn sqlite3ExprIsConstantNotJoin(db: &mut Sqlite3, p: ExprId) -> i32 {
    exprIsConst(db, true, p, 2)
}

/// This routine examines sub-SELECT statements as an expression is being
/// walked as part of sqlite3ExprIsTableConstant().  Sub-SELECTs are considered
/// constant as long as they are uncorrelated - meaning that they do not
/// contain any terms from outer contexts.
fn exprSelectWalkTableConstant(w: &mut Walker, pSelect: SelectId) -> i32 {
    debug_assert!(w.eCode == 3 || w.eCode == 0);
    if (w.db.sel(pSelect).selFlags & SF_Correlated) != 0 {
        w.eCode = 0;
        return WRC_Abort;
    }
    WRC_Prune
}

/// Walk an expression tree.  Return non-zero if the expression is constant
/// for any single row of the table with cursor iCur.  In other words, the
/// expression must not refer to any non-deterministic function nor any
/// table other than iCur.
///
/// Consider uncorrelated subqueries to be constants if the bAllowSubq
/// parameter is true.
fn sqlite3ExprIsTableConstant(db: &mut Sqlite3, p: ExprId, iCur: i32, bAllowSubq: i32) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = 3;
    w.pParse = false;
    w.xExprCallback = Some(exprNodeIsConstant);
    if bAllowSubq != 0 {
        w.xSelectCallback = Some(exprSelectWalkTableConstant);
    } else {
        w.xSelectCallback = Some(sqlite3SelectWalkFail);
    }
    w.u.iCur = iCur;
    sqlite3WalkExpr(&mut w, p);
    w.eCode as i32
}

/// Check pExpr to see if it is an constraint on the single data source
/// pSrc = &pSrcList->a[iSrc].  In other words, check to see if pExpr
/// constrains pSrc but does not depend on any other tables or data
/// sources anywhere else in the query.  Return true (non-zero) if pExpr
/// is a constraint on pSrc only.
///
/// This is an optimization.  False negatives will perhaps cause slower
/// queries, but false positives will yield incorrect answers.  So when in
/// doubt, return 0.
///
/// To be an single-source constraint, the following must be true:
///
///   (1)  pExpr cannot refer to any table other than pSrc->iCursor.
///
///   (2a) pExpr cannot use subqueries unless the bAllowSubq parameter is
///        true and the subquery is non-correlated
///
///   (2b) pExpr cannot use non-deterministic functions.
///
///   (3)  pSrc cannot be part of the left operand for a RIGHT JOIN.
///        (Is there some way to relax this constraint?)
///
///   (4)  If pSrc is the right operand of a LEFT JOIN, then...
///         (4a)  pExpr must come from an ON clause..
///         (4b)  and specifically the ON clause associated with the LEFT JOIN.
///
///   (5)  If pSrc is the right operand of a LEFT JOIN or the left
///        operand of a RIGHT JOIN, then pExpr must be from the WHERE
///        clause, not an ON clause.
///
///   (6) Either:
///
///       (6a) pExpr does not originate in an ON or USING clause, or
///
///       (6b) The ON or USING clause from which pExpr is derived is
///            not to the left of a RIGHT JOIN (or FULL JOIN).
///
///       Without this restriction, accepting pExpr as a single-table
///       constraint might move the the ON/USING filter expression
///       from the left side of a RIGHT JOIN over to the right side,
///       which leads to incorrect answers.  See also restriction (9)
///       on push-down.
pub fn sqlite3ExprIsSingleTableConstraint(
    db: &mut Sqlite3,
    pExpr: ExprId,
    pSrcList: SrcListId,
    iSrc: i32,
    bAllowSubq: i32,
) -> i32 {
    let (jointype, iCursor) = {
        let it = &db.src(pSrcList).a[iSrc as usize];
        (it.fg.jointype, it.iCursor)
    };
    let (flags, iJoin) = {
        let e = db.ex(pExpr);
        (e.flags, e.w.iJoin)
    };
    if (jointype & JT_LTORJ) != 0 {
        return 0; /* rule (3) */
    }
    if (jointype & JT_LEFT) != 0 {
        if (flags & EP_OuterON) == 0 {
            return 0; /* rule (4a) */
        }
        if iJoin != iCursor {
            return 0; /* rule (4b) */
        }
    } else if (flags & EP_OuterON) != 0 {
        return 0; /* rule (5) */
    }
    if (flags & (EP_OuterON | EP_InnerON)) != 0 /* (6a) */
        && (db.src(pSrcList).a[0].fg.jointype & JT_LTORJ) != 0
    /* Fast pre-test of (6b) */
    {
        for jj in 0..iSrc as usize {
            let it = &db.src(pSrcList).a[jj];
            if iJoin == it.iCursor {
                if (it.fg.jointype & JT_LTORJ) != 0 {
                    return 0; /* restriction (6) */
                }
                break;
            }
        }
    }
    /* Rules (1), (2a), and (2b) handled by the following: */
    sqlite3ExprIsTableConstant(db, pExpr, iCursor, bAllowSubq)
}

/// sqlite3WalkExpr() callback used by sqlite3ExprIsConstantOrGroupBy().
fn exprNodeIsConstantOrGroupBy(w: &mut Walker, pExpr: ExprId) -> i32 {
    let pGroupBy = w.u.pGroupBy;

    /* Check if pExpr is identical to any GROUP BY term. If so, consider
     ** it constant.  */
    for i in 0..el_n(w.db, pGroupBy) {
        let p = el_e(w.db, pGroupBy, i);
        if sqlite3ExprCompare(w.db, false, pExpr, p, -1) < 2 {
            let pColl = sqlite3ExprNNCollSeq(w.db, p);
            if is_binary(&pColl) {
                return WRC_Prune;
            }
        }
    }

    /* Check if pExpr is a sub-select. If so, consider it variable. */
    if w.db.ex(pExpr).UseXSelect() {
        w.eCode = 0;
        return WRC_Abort;
    }

    exprNodeIsConstant(w, pExpr)
}

/// Walk the expression tree passed as the first argument. Return non-zero
/// if the expression consists entirely of constants or copies of terms
/// in pGroupBy that sort with the BINARY collation sequence.
///
/// This routine is used to determine if a term of the HAVING clause can
/// be promoted into the WHERE clause.  In order for such a promotion to work,
/// the value of the HAVING clause term must be the same for all members of
/// a "group".  The requirement that the GROUP BY term must be BINARY
/// assumes that no other collating sequence will have a finer-grained
/// grouping than binary.  In other words (A=B COLLATE binary) implies
/// A=B in every other collating sequence.  The requirement that the
/// GROUP BY be BINARY is stricter than necessary.  It would also work
/// to promote HAVING clauses that use the same alternative collating
/// sequence as the GROUP BY term, but that is much harder to check,
/// alternative collating sequences are uncommon, and this is only an
/// optimization, so we take the easy way out and simply require the
/// GROUP BY to use the BINARY collating sequence.
pub fn sqlite3ExprIsConstantOrGroupBy(db: &mut Sqlite3, p: ExprId, pGroupBy: ExprListId) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = 1;
    w.xExprCallback = Some(exprNodeIsConstantOrGroupBy);
    w.xSelectCallback = None;
    w.u.pGroupBy = pGroupBy;
    w.pParse = true;
    sqlite3WalkExpr(&mut w, p);
    w.eCode as i32
}

/// Walk an expression tree for the DEFAULT field of a column definition
/// in a CREATE TABLE statement.  Return non-zero if the expression is
/// acceptable for use as a DEFAULT.  That is to say, return non-zero if
/// the expression is constant or a function call with constant arguments.
/// Return and 0 if there are any variables.
///
/// isInit is true when parsing from sqlite_schema.  isInit is false when
/// processing a new CREATE TABLE statement.  When isInit is true, parameters
/// (such as ? or $abc) in the expression are converted into NULL.  When
/// isInit is false, parameters raise an error.  Parameters should not be
/// allowed in a CREATE TABLE statement, but some legacy versions of SQLite
/// allowed it, so we need to support it when reading sqlite_schema for
/// backwards compatibility.
///
/// If isInit is true, set EP_FromDDL on every TK_FUNCTION node.
///
/// For the purposes of this function, a double-quoted string (ex: "abc")
/// is considered a variable but a single-quoted string (ex: 'abc') is
/// a constant.
pub fn sqlite3ExprIsConstantOrFunction(db: &mut Sqlite3, p: ExprId, isInit: u8) -> i32 {
    debug_assert!(isInit == 0 || isInit == 1);
    exprIsConst(db, false, p, 4 + isInit as u16)
}

/// Walk an expression tree.  Return 1 if the expression contains a
/// subquery of some kind.  Return 0 if there are no subqueries.
/// (SQLITE_ENABLE_CURSOR_HINTS only in C.)
pub fn sqlite3ExprContainsSubquery(db: &mut Sqlite3, p: ExprId) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = 1;
    w.xExprCallback = Some(sqlite3ExprWalkNoop);
    w.xSelectCallback = Some(sqlite3SelectWalkFail);
    sqlite3WalkExpr(&mut w, p);
    (w.eCode == 0) as i32
}

/// If the expression p codes a constant integer between 0 and 0x7fffffff,
/// then return 1 and put the value of the integer in *pValue.  If the
/// expression is not an integer or if it is an integer that is out side
/// the range of 0...0x7fffffff, then return 0 and leave *pValue unchanged.
///
/// If pParse is true, then allow the expression p to be a parameter
/// (TK_VARIABLE) that is bound to an integer between 0 and 0x7fffffff.
/// Variables that hold anything other than integers, or that hold integers
/// outside the range of 0..0x7fffffff are not seen.  But if pParse is false
/// (C NULL), then p must be a pure integer literal between 0 and 0x7fffffff.
///
/// If pParse is true and expression p is a variable, then the variable
/// is marked so as to cause the statement to be reprepared each time a new
/// value is bound to it. Except, if parameter bRSI is true, then the statement
/// will only be reprepared if the rebind changes the value to or from a
/// "small integer" (either 0 or 1).
pub fn sqlite3ExprIsInteger(
    db: &mut Sqlite3,
    p: ExprId,
    pValue: &mut i32,
    pParse: bool,
    bRSI: i32,
) -> i32 {
    let mut iSign = 1; /* Either +1 or -1. */
    let mut p = p;
    let mut pParse = pParse;
    loop {
        if p.is_null() {
            return 0;
        }
        let (useU, iValue, op, pLeft, iColumn) = {
            let e = db.ex(p);
            (e.UseUValue(), e.u.iValue, e.op, e.pLeft, e.iColumn)
        };
        if useU {
            *pValue = iValue.wrapping_mul(iSign);
            return 1;
        }
        if op == TK_UPLUS {
            p = pLeft;
            pParse = false;
            continue;
        }
        if op == TK_UMINUS {
            iSign = -iSign;
            p = pLeft;
            pParse = false;
            continue;
        }
        if op == TK_VARIABLE && pParse {
            let mut rc = 0; /* 1 if successful, 0 if failed */
            debug_assert!(iSign == 1);
            if db.ppr().pVdbe.is_none() {
                break;
            }
            if (db.flags & SQLITE_EnableQPSG) != 0 {
                break;
            }
            let pVal = bound_value(db, iColumn as i32, SQLITE_AFF_BLOB);
            if let Some(mut pVal) = pVal {
                if crate::vdbeapi::sqlite3_value_type(&mut pVal) == SQLITE_INTEGER {
                    let vv = crate::vdbeapi::sqlite3_value_int64(&mut pVal);
                    if vv >= 0 && vv <= 0x7fffffff {
                        *pValue = vv as i32;
                        rc = 1;
                    }
                }
                crate::vdbemem::sqlite3ValueFree(db, &mut pVal);
            }
            with_parse_vdbe(db, |db, v| {
                sqlite3VdbeReprepareOnBind(db, v, iColumn as i32, bRSI)
            });
            return rc;
        }
        break;
    }
    0
}

/// sqlite3VdbeGetBoundValue(pParse->pReprepare, iVar, aff)
fn bound_value(db: &mut Sqlite3, iVar: i32, aff: u8) -> Option<Box<Mem>> {
    let id = db.ppr().pReprepare?;
    db.with_stmt(id, |db, v| sqlite3VdbeGetBoundValue(db, v, iVar, aff))
        .flatten()
}

fn exprCanBeNull(db: &Sqlite3, p: ExprId) -> bool {
    let mut p = p;
    debug_assert!(p.nn());
    while db.ex(p).op == TK_UPLUS || db.ex(p).op == TK_UMINUS {
        p = db.ex(p).pLeft;
        debug_assert!(p.nn());
    }
    let e = db.ex(p);
    let mut op = e.op;
    if op == TK_REGISTER {
        op = e.op2;
    }
    match op {
        TK_INTEGER | TK_STRING | TK_FLOAT | TK_BLOB => false,
        TK_COLUMN => {
            if e.HasProperty(EP_CanBeNull) {
                return true;
            }
            let Some(t) = &e.y.pTab else {
                return true; /* Reference to column of index on expr */
            };
            let t = t.borrow();
            e.iColumn >= 0
                && !t.aCol.is_empty() /* Possible due to prior error */
                && (e.iColumn as i32) < t.nCol as i32
                && (e.iColumn as usize) < t.aCol.len()
                && t.aCol[e.iColumn as usize].notNull == 0
        }
        _ => true,
    }
}

/// Return FALSE if there is no chance that the expression can be NULL.
///
/// If the expression might be NULL or if the expression is too complex
/// to tell return TRUE.
///
/// This routine is used as an optimization, to skip OP_IsNull opcodes
/// when we know that a value cannot be NULL.  Hence, a false positive
/// (returning TRUE when in fact the expression can never be NULL) might
/// be a small performance hit but is otherwise harmless.  On the other
/// hand, a false negative (returning FALSE when the result could be NULL)
/// will likely result in an incorrect answer.  So when in doubt, return
/// TRUE.
pub fn sqlite3ExprCanBeNull(db: &mut Sqlite3, p: ExprId) -> i32 {
    exprCanBeNull(db, p) as i32
}

/// Return TRUE if the given expression is a constant which would be
/// unchanged by OP_Affinity with the affinity given in the second
/// argument.
///
/// This routine is used to determine if the OP_Affinity operation
/// can be omitted.  When in doubt return FALSE.  A false negative
/// is harmless.  A false positive, however, can result in the wrong
/// answer.
pub fn sqlite3ExprNeedsNoAffinityChange(db: &mut Sqlite3, p: ExprId, aff: u8) -> i32 {
    let mut p = p;
    let mut unaryMinus = false;
    if aff == SQLITE_AFF_BLOB {
        return 1;
    }
    while db.ex(p).op == TK_UPLUS || db.ex(p).op == TK_UMINUS {
        if db.ex(p).op == TK_UMINUS {
            unaryMinus = true;
        }
        p = db.ex(p).pLeft;
    }
    let e = db.ex(p);
    let mut op = e.op;
    if op == TK_REGISTER {
        op = e.op2;
    }
    let r = match op {
        TK_INTEGER => aff >= SQLITE_AFF_NUMERIC,
        TK_FLOAT => aff >= SQLITE_AFF_NUMERIC,
        TK_STRING => !unaryMinus && aff == SQLITE_AFF_TEXT,
        TK_BLOB => !unaryMinus,
        TK_COLUMN => {
            debug_assert!(e.iTable >= 0); /* p cannot be part of a CHECK constraint */
            aff >= SQLITE_AFF_NUMERIC && e.iColumn < 0
        }
        _ => false,
    };
    r as i32
}

/// Return TRUE if the given string is a row-id column name.
pub fn sqlite3IsRowid(z: Option<&[u8]>) -> i32 {
    use crate::util::sqlite3StrICmp;
    if sqlite3StrICmp(z, Some(b"_ROWID_")) == 0 {
        return 1;
    }
    if sqlite3StrICmp(z, Some(b"ROWID")) == 0 {
        return 1;
    }
    if sqlite3StrICmp(z, Some(b"OID")) == 0 {
        return 1;
    }
    0
}

/// Return a pointer to a buffer containing a usable rowid alias for table
/// pTab. An alias is usable if there is not an explicit user-defined column
/// of the same name.
pub fn sqlite3RowidAlias(pTab: &TableRef) -> Option<ZStr> {
    const azOpt: [&[u8]; 3] = [b"_ROWID_", b"ROWID", b"OID"];
    debug_assert!(pTab.borrow().VisibleRowid());
    for z in azOpt.iter() {
        if crate::select::sqlite3ColumnIndex(pTab, Some(z)) < 0 {
            return Some(z.to_vec());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The IN operator
// ---------------------------------------------------------------------------

/// pX is the RHS of an IN operator.  If pX is a SELECT statement
/// that can be simplified to a direct table access, then return
/// a pointer to the SELECT statement.  If pX is not a SELECT statement,
/// or if the SELECT statement needs to be materialized into a transient
/// table, then return NULL.
fn isCandidateForInOpt(db: &Sqlite3, pX: ExprId) -> SelectId {
    let x = db.ex(pX);
    if !x.UseXSelect() {
        return SelectId::NULL; /* Not a subquery */
    }
    if x.HasProperty(EP_VarSelect) {
        return SelectId::NULL; /* Correlated subq */
    }
    let p = x.x.pSelect;
    let s = db.sel(p);
    if s.pPrior.nn() {
        return SelectId::NULL; /* Not a compound SELECT */
    }
    if (s.selFlags & SF_Aggregate) != 0 {
        return SelectId::NULL; /* No GROUP BY keyword or aggregate functions */
    }
    debug_assert!(s.pGroupBy.is_null()); /* Has no GROUP BY clause */
    if s.pLimit.nn() {
        return SelectId::NULL; /* Has no LIMIT clause */
    }
    if s.pWhere.nn() {
        return SelectId::NULL; /* Has no WHERE clause */
    }
    let pSrc = s.pSrc;
    debug_assert!(pSrc.nn());
    let src = db.src(pSrc);
    if src.nSrc() != 1 {
        return SelectId::NULL; /* Single term in FROM clause */
    }
    if src.a[0].fg.isSubquery != 0 {
        return SelectId::NULL; /* FROM is not a subquery or view */
    }
    let Some(pTab) = &src.a[0].pSTab else {
        return SelectId::NULL;
    };
    debug_assert!(!pTab.borrow().IsView()); /* FROM clause is not a view */
    if pTab.borrow().IsVirtual() {
        return SelectId::NULL; /* FROM clause not a virtual table */
    }
    let pEList = s.pEList;
    debug_assert!(pEList.nn());
    /* All SELECT results must be columns. */
    for it in db.el(pEList).a.iter() {
        if db.ex(it.pExpr).op != TK_COLUMN {
            return SelectId::NULL;
        }
        debug_assert!(db.ex(it.pExpr).iTable == src.a[0].iCursor); /* Not a correlated subquery */
    }
    p
}

/// Generate code that checks the left-most column of index table iCur to see if
/// it contains any NULL entries.  Cause the register at regHasNull to be set
/// to a non-NULL value if iCur contains no NULLs.  Cause register regHasNull
/// to be set to NULL if iCur contains one or more NULL values.
fn sqlite3SetHasNullFlag(v: &mut Vdbe, iCur: i32, regHasNull: i32, eSortOrder: i32) {
    sqlite3VdbeAddOp2(v, OP_Integer, 0, regHasNull);
    let op = if eSortOrder == SQLITE_SO_ASC as i32 {
        OP_Rewind
    } else {
        OP_Last
    };
    let addr1 = sqlite3VdbeAddOp1(v, op, iCur);
    sqlite3VdbeAddOp3(v, OP_Column, iCur, 0, regHasNull);
    sqlite3VdbeChangeP5(v, OPFLAG_TYPEOFARG);
    sqlite3VdbeJumpHere(v, addr1);
}

/// The argument is an IN operator with a list (not a subquery) on the
/// right-hand side.  Return TRUE if that list is constant.
fn sqlite3InRhsIsConstant(db: &mut Sqlite3, pIn: ExprId) -> i32 {
    debug_assert!(!hp(db, pIn, EP_xIsSelect));
    let pLHS = db.ex(pIn).pLeft;
    db.exm(pIn).pLeft = ExprId::NULL;
    let res = sqlite3ExprIsConstant(db, true, pIn);
    db.exm(pIn).pLeft = pLHS;
    res
}

/// This function is used by the implementation of the IN (...) operator.
/// The pX parameter is the expression on the RHS of the IN operator, which
/// might be either a list of expressions or a subquery.
///
/// The job of this routine is to find or create a b-tree object that can
/// be used either to test for membership in the RHS set or to iterate through
/// all members of the RHS set, skipping duplicates.
///
/// A cursor is opened on the b-tree object that is the RHS of the IN operator
/// and the *piTab parameter is set to the index of that cursor.
///
/// The returned value of this function indicates the b-tree type, as follows:
///
///   IN_INDEX_ROWID      - The cursor was opened on a database table.
///   IN_INDEX_INDEX_ASC  - The cursor was opened on an ascending index.
///   IN_INDEX_INDEX_DESC - The cursor was opened on a descending index.
///   IN_INDEX_EPH        - The cursor was opened on a specially created and
///                         populated ephemeral table.
///   IN_INDEX_NOOP       - No cursor was allocated.  The IN operator must be
///                         implemented as a sequence of comparisons.
///
/// An existing b-tree might be used if the RHS expression pX is a simple
/// subquery such as:
///
///     SELECT <column1>, <column2>... FROM <table>
///
/// If the RHS of the IN operator is a list or a more complex subquery, then
/// an ephemeral table might need to be generated from the RHS and then
/// pX->iTable made to point to the ephemeral table instead of an
/// existing table.  In this case, the creation and initialization of the
/// ephemeral table might be put inside of a subroutine, the EP_Subrtn flag
/// will be set on pX and the pX->y.sub fields will be set to show where
/// the subroutine is coded.
///
/// The inFlags parameter must contain, at a minimum, one of the bits
/// IN_INDEX_MEMBERSHIP or IN_INDEX_LOOP but not both.  If inFlags contains
/// IN_INDEX_MEMBERSHIP, then the generated table will be used for a fast
/// membership test.  When the IN_INDEX_LOOP bit is set, the IN index will
/// be used to loop over all values of the RHS of the IN operator.
///
/// When IN_INDEX_LOOP is used (and the b-tree will be used to iterate
/// through the set members) then the b-tree must not contain duplicates.
/// An ephemeral table will be created unless the selected columns are guaranteed
/// to be unique - either because it is an INTEGER PRIMARY KEY or due to
/// a UNIQUE constraint or index.
///
/// When IN_INDEX_MEMBERSHIP is used (and the b-tree will be used
/// for fast set membership tests) then an ephemeral table must
/// be used unless <columns> is a single INTEGER PRIMARY KEY column or an
/// index can be found with the specified <columns> as its left-most.
///
/// If the IN_INDEX_NOOP_OK and IN_INDEX_MEMBERSHIP are both set and
/// if the RHS of the IN operator is a list (not a subquery) then this
/// routine might decide that creating an ephemeral b-tree for membership
/// testing is too expensive and return IN_INDEX_NOOP.  In that case, the
/// calling routine should implement the IN operator using a sequence
/// of Eq or Ne comparison operations.
///
/// When the b-tree is being used for membership tests, the calling function
/// might need to know whether or not the RHS side of the IN operator
/// contains a NULL.  If prRhsHasNull is not a NULL pointer and
/// if there is any chance that the (...) might contain a NULL value at
/// runtime, then a register is allocated and the register number written
/// to *prRhsHasNull. If there is no chance that the (...) contains a
/// NULL value, then *prRhsHasNull is left unchanged.
///
/// If a register is allocated and its location stored in *prRhsHasNull, then
/// the value in that register will be NULL if the b-tree contains one or more
/// NULL values, and it will be some non-NULL value if the b-tree contains no
/// NULL values.
///
/// If the aiMap parameter is not NULL, it must point to an array containing
/// one element for each column returned by the SELECT statement on the RHS
/// of the IN(...) operator. The i'th entry of the array is populated with the
/// offset of the index column that matches the i'th column returned by the
/// SELECT. For example, if the expression and selected index are:
///
///   (?,?,?) IN (SELECT a, b, c FROM t1)
///   CREATE INDEX i1 ON t1(b, c, a);
///
/// then aiMap[] is populated with {2, 0, 1}.
pub fn sqlite3FindInIndex(
    db: &mut Sqlite3,
    pX: ExprId,
    inFlags: u32,
    prRhsHasNull: Option<&mut i32>,
    aiMap: Option<&mut [i32]>,
    piTab: &mut i32,
) -> i32 {
    let mut prRhsHasNull = prRhsHasNull;
    let mut aiMap = aiMap;
    let mut eType = 0; /* Type of RHS table. IN_INDEX_* */
    let inFlags = inFlags as i32;
    crate::select::sqlite3GetVdbe(db); /* Virtual machine being coded */

    debug_assert!(db.ex(pX).op == TK_IN);
    let mustBeUnique = (inFlags & IN_INDEX_LOOP) != 0;
    let mut iTab = db.ppr().nTab;
    db.pp().nTab += 1;

    /* If the RHS of this IN(...) operator is a SELECT, and if it matters
     ** whether or not the SELECT result contains NULL values, check whether
     ** or not NULL is actually possible (it may not be, for example, due
     ** to NOT NULL constraints in the schema). If no NULL values are possible,
     ** set prRhsHasNull to 0 before continuing.  */
    if prRhsHasNull.is_some() && hp(db, pX, EP_xIsSelect) {
        let pEList = db.sel(db.ex(pX).x.pSelect).pEList;
        let n = el_n(db, pEList);
        let mut i = 0;
        while i < n {
            if exprCanBeNull(db, el_e(db, pEList, i)) {
                break;
            }
            i += 1;
        }
        if i == n {
            prRhsHasNull = None;
        }
    }

    /* Check to see if an existing table or index can be used to
     ** satisfy the query.  This is preferable to generating a new
     ** ephemeral table.  */
    let p = if nerr(db) == 0 {
        isCandidateForInOpt(db, pX)
    } else {
        SelectId::NULL
    };
    if p.nn() {
        let pEList = db.sel(p).pEList;
        let nExpr = el_n(db, pEList);
        let pSrc = db.sel(p).pSrc;
        let pTab = db.src(pSrc).a[0].pSTab.clone().expect("IN subquery table");
        let pXLeft = db.ex(pX).pLeft;

        /* Code an OP_Transaction and OP_TableLock for <table>. */
        let (pSchema, tnum, zName) = {
            let t = pTab.borrow();
            (t.pSchema, t.tnum, t.zName.clone())
        };
        let iDb = db.schema_index(pSchema);
        debug_assert!(iDb >= 0);
        crate::build::sqlite3CodeVerifySchema(db, iDb);
        crate::build::sqlite3TableLock(db, iDb, tnum, 0, Some(&zName));

        if nExpr == 1 && db.ex(el_e(db, pEList, 0)).iColumn < 0 {
            /* The "x IN (SELECT rowid FROM table)" case */
            let iAddr = sqlite3VdbeAddOp0(db.v(), OP_Once);

            crate::insert::sqlite3OpenTable(db, iTab, iDb, &pTab, OP_OpenRead as i32);
            eType = IN_INDEX_ROWID;
            sqlite3VdbeExplain(
                db,
                0,
                b"USING ROWID SEARCH ON TABLE %s FOR IN-OPERATOR",
                &[PfArg::from(&zName)],
            );
            sqlite3VdbeJumpHere(db.v(), iAddr);
        } else {
            let mut affinity_ok = true;

            /* Check that the affinity that will be used to perform each
             ** comparison is the same as the affinity of each column in table
             ** on the RHS of the IN operator.  If it not, it is not possible to
             ** use any index of the RHS table.  */
            let mut i = 0;
            while i < nExpr && affinity_ok {
                let pLhs = vectorFieldSubexpr(db, pXLeft, i);
                let iCol = db.ex(el_e(db, pEList, i)).iColumn as i32;
                let idxaff = sqlite3TableColumnAffinity(&pTab, iCol); /* RHS table */
                let cmpaff = compareAffinity(db, pLhs, idxaff);
                match cmpaff {
                    SQLITE_AFF_BLOB => {}
                    SQLITE_AFF_TEXT => {
                        /* sqlite3CompareAffinity() only returns TEXT if one side or the
                         ** other has no affinity and the other side is TEXT.  Hence,
                         ** the only way for cmpaff to be TEXT is for idxaff to be TEXT
                         ** and for the term on the LHS of the IN to have no affinity. */
                        debug_assert!(idxaff == SQLITE_AFF_TEXT);
                    }
                    _ => {
                        affinity_ok = sqlite3IsNumericAffinity(idxaff);
                    }
                }
                i += 1;
            }

            if affinity_ok {
                /* Search for an existing index that will work for this IN operator */
                let indices: Vec<IndexRef> = pTab.borrow().pIndex.clone();
                for pIdx in indices.iter() {
                    if eType != 0 {
                        break;
                    }
                    let (
                        nColumn,
                        hasPartWhere,
                        nKeyCol,
                        isUnique,
                        aiColumn,
                        azColl,
                        tnumIdx,
                        sort0,
                    ) = {
                        let x = pIdx.borrow();
                        (
                            x.nColumn as i32,
                            x.pPartIdxWhere.is_some(),
                            x.nKeyCol as i32,
                            x.IsUniqueIndex(),
                            x.aiColumn.clone(),
                            x.azColl.clone(),
                            x.tnum,
                            x.aSortOrder.first().copied().unwrap_or(0),
                        )
                    };
                    if nColumn < nExpr {
                        continue;
                    }
                    if hasPartWhere {
                        continue;
                    }
                    /* Maximum nColumn is BMS-2, not BMS-1, so that we can compute
                     ** BITMASK(nExpr) without overflowing */
                    if nColumn >= BMS - 1 {
                        continue;
                    }
                    if mustBeUnique && (nKeyCol > nExpr || (nColumn > nExpr && !isUnique)) {
                        continue; /* This index is not unique over the IN RHS columns */
                    }

                    let mut colUsed: Bitmask = 0; /* Columns of index used so far */
                    let mut i = 0;
                    while i < nExpr {
                        let pLhs = vectorFieldSubexpr(db, pXLeft, i);
                        let pRhs = el_e(db, pEList, i);
                        let pReq = sqlite3BinaryCompareCollSeq(db, pLhs, pRhs);
                        let rhsCol = db.ex(pRhs).iColumn;
                        let mut j = 0;
                        while j < nExpr {
                            if aiColumn[j as usize] != rhsCol {
                                j += 1;
                                continue;
                            }
                            if let Some(req) = &pReq {
                                if crate::util::sqlite3StrICmp(
                                    Some(&req.zName),
                                    Some(&azColl[j as usize]),
                                ) != 0
                                {
                                    j += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        if j == nExpr {
                            break;
                        }
                        let mCol = MASKBIT(j);
                        if (mCol & colUsed) != 0 {
                            break; /* Each column used only once */
                        }
                        colUsed |= mCol;
                        if let Some(m) = aiMap.as_deref_mut() {
                            m[i as usize] = j;
                        }
                        i += 1;
                    }

                    debug_assert!(nExpr > 0 && nExpr < BMS);
                    if colUsed == (MASKBIT(nExpr) - 1) {
                        /* If we reach this point, that means the index pIdx is usable */
                        let iAddr = sqlite3VdbeAddOp0(db.v(), OP_Once);
                        let zIdxName = pIdx.borrow().zName.clone();
                        sqlite3VdbeExplain(
                            db,
                            0,
                            b"USING INDEX %s FOR IN-OPERATOR",
                            &[PfArg::from(&zIdxName)],
                        );
                        sqlite3VdbeAddOp3(db.v(), OP_OpenRead, iTab, tnumIdx as i32, iDb);
                        sqlite3VdbeSetP4KeyInfo(db, pIdx);
                        eType = IN_INDEX_INDEX_ASC + sort0 as i32;

                        if let Some(r) = prRhsHasNull.as_deref_mut() {
                            db.pp().nMem += 1;
                            *r = db.ppr().nMem;
                            if nExpr == 1 {
                                let reg = *r;
                                sqlite3SetHasNullFlag(db.v(), iTab, reg, sort0 as i32);
                            }
                        }
                        sqlite3VdbeJumpHere(db.v(), iAddr);
                    }
                } /* End loop over indexes */
            } /* End if( affinity_ok ) */
        } /* End if not an rowid index */
    } /* End attempt to optimize using an index */

    /* If no preexisting index is available for the IN clause
     ** and IN_INDEX_NOOP is an allowed reply
     ** and the RHS of the IN operator is a list, not a subquery
     ** and the RHS is not constant or has two or fewer terms,
     ** then it is not worth creating an ephemeral table to evaluate
     ** the IN operator so return IN_INDEX_NOOP.
     */
    if eType == 0
        && (inFlags & IN_INDEX_NOOP_OK) != 0
        && db.ex(pX).UseXList()
        && (sqlite3InRhsIsConstant(db, pX) == 0 || el_n(db, db.ex(pX).x.pList) <= 2)
    {
        db.pp().nTab -= 1; /* Back out the allocation of the unused cursor */
        iTab = -1; /* Cursor is not allocated */
        eType = IN_INDEX_NOOP;
    }

    if eType == 0 {
        /* Could not find an existing table or index to use as the RHS b-tree.
         ** We will have to generate an ephemeral table to do the job.
         */
        let savedNQueryLoop = db.ppr().nQueryLoop;
        let mut rMayHaveNull = 0;
        let mut bloomOk = (inFlags & IN_INDEX_MEMBERSHIP) != 0;
        eType = IN_INDEX_EPH;
        if (inFlags & IN_INDEX_LOOP) != 0 {
            db.pp().nQueryLoop = 0;
        } else if let Some(r) = prRhsHasNull.as_deref_mut() {
            db.pp().nMem += 1;
            rMayHaveNull = db.ppr().nMem;
            *r = rMayHaveNull;
        }
        debug_assert!(db.ex(pX).op == TK_IN);
        if !bloomOk
            && hp(db, pX, EP_xIsSelect)
            && (db.sel(db.ex(pX).x.pSelect).selFlags & SF_ClonedRhsIn) != 0
        {
            bloomOk = true;
        }
        sqlite3CodeRhsOfIN(db, pX, iTab, bloomOk as i32);
        if rMayHaveNull != 0 {
            sqlite3SetHasNullFlag(db.v(), iTab, rMayHaveNull, SQLITE_SO_ASC as i32);
        }
        db.pp().nQueryLoop = savedNQueryLoop;
    }

    if let Some(m) = aiMap.as_deref_mut() {
        if eType != IN_INDEX_INDEX_ASC && eType != IN_INDEX_INDEX_DESC {
            let n = exprVectorSize(db, db.ex(pX).pLeft);
            for i in 0..n {
                m[i as usize] = i;
            }
        }
    }
    *piTab = iTab;
    eType
}

/// Argument pExpr is an (?, ?...) IN(...) expression. This
/// function allocates and returns a nul-terminated string containing
/// the affinities to be used for each column of the comparison.
fn exprINAffinity(db: &Sqlite3, pExpr: ExprId) -> ZStr {
    let e = db.ex(pExpr);
    let pLeft = e.pLeft;
    let nVal = exprVectorSize(db, pLeft);
    let pSelect = if e.UseXSelect() {
        e.x.pSelect
    } else {
        SelectId::NULL
    };
    debug_assert!(e.op == TK_IN);
    let mut zRet = Vec::with_capacity(nVal as usize);
    for i in 0..nVal {
        let pA = vectorFieldSubexpr(db, pLeft, i);
        let a = exprAffinity(db, pA);
        if pSelect.nn() {
            let pEList = db.sel(pSelect).pEList;
            zRet.push(compareAffinity(db, el_e(db, pEList, i), a));
        } else {
            zRet.push(a);
        }
    }
    zRet
}

/// Load the Parse object passed as the first argument with an error
/// message of the form:
///
///   "sub-select returns N columns - expected M"
pub fn sqlite3SubselectError(db: &mut Sqlite3, nActual: i32, nExpect: i32) {
    if nerr(db) == 0 {
        errmsg!(
            db,
            "sub-select returns %d columns - expected %d",
            nActual,
            nExpect
        );
    }
}

/// Expression pExpr is a vector that has been used in a context where
/// it is not permitted. If pExpr is a sub-select vector, this routine
/// loads the Parse object with a message of the form:
///
///   "sub-select returns N columns - expected 1"
///
/// Or, if it is a regular scalar vector:
///
///   "row value misused"
pub fn sqlite3VectorErrorMsg(db: &mut Sqlite3, pExpr: ExprId) {
    if db.ex(pExpr).UseXSelect() {
        let n = el_n(db, db.sel(db.ex(pExpr).x.pSelect).pEList);
        sqlite3SubselectError(db, n, 1);
    } else {
        errmsg!(db, "row value misused");
    }
}

/// Scan all previously generated bytecode looking for an OP_BeginSubrtn
/// that is compatible with pExpr.  If found, add the y.sub values
/// to pExpr and return true.  If not found, return false.
fn findCompatibleInRhsSubrtn(db: &mut Sqlite3, pExpr: ExprId, pNewSig: Option<&SubrtnSig>) -> bool {
    let Some(pNewSig) = pNewSig else {
        return false;
    };
    if (db.ppr().mSubrtnSig as i32 & (1 << (pNewSig.selId & 7))) == 0 {
        return false;
    }
    debug_assert!(db.ex(pExpr).op == TK_IN);
    debug_assert!(!db.ex(pExpr).UseYSub());
    debug_assert!(db.ex(pExpr).UseXSelect());
    let mut found = None;
    {
        let v = db.v();
        let nOp = v.aOp.len();
        /* pOp runs from sqlite3VdbeGetOp(v,1) up to (not including) the last op */
        let mut i = 1;
        while i + 1 < nOp {
            let pOp = &v.aOp[i];
            i += 1;
            if pOp.p4type != P4_SUBRTNSIG {
                continue;
            }
            debug_assert!(pOp.opcode == OP_BeginSubrtn);
            let P4::SubrtnSig(pSig) = &pOp.p4 else {
                continue;
            };
            if pSig.bComplete == 0 {
                continue;
            }
            if pNewSig.selId != pSig.selId {
                continue;
            }
            if pNewSig.zAff != pSig.zAff {
                continue;
            }
            found = Some((pSig.iAddr, pSig.regReturn, pSig.iTable));
            break;
        }
    }
    if let Some((iAddr, regReturn, iTable)) = found {
        let e = db.exm(pExpr);
        e.y.sub.iAddr = iAddr;
        e.y.sub.regReturn = regReturn;
        e.iTable = iTable;
        e.flags |= EP_Subrtn;
        return true;
    }
    false
}

/// Generate code that will construct an ephemeral table containing all terms
/// in the RHS of an IN operator.  The IN operator can be in either of two
/// forms:
///
///     x IN (4,5,11)              -- IN operator with list on right-hand side
///     x IN (SELECT a FROM b)     -- IN operator with subquery on the right
///
/// The pExpr parameter is the IN operator.  The cursor number for the
/// constructed ephemeral table is returned.  The first time the ephemeral
/// table is computed, the cursor number is also stored in pExpr->iTable,
/// however the cursor number returned might not be the same, as it might
/// have been duplicated using OP_OpenDup.
///
/// If the LHS expression ("x" in the examples) is a column value, or
/// the SELECT statement returns a column value, then the affinity of that
/// column is used to build the index keys. If both 'x' and the
/// SELECT... statement are columns, then numeric affinity is used
/// if either column has NUMERIC or INTEGER affinity. If neither
/// 'x' nor the SELECT... statement are columns, then numeric affinity
/// is used.
pub fn sqlite3CodeRhsOfIN(db: &mut Sqlite3, pExpr: ExprId, iTab: i32, allowBloom: i32) {
    let mut addrOnce = 0; /* Address of the OP_Once instruction at top */
    let mut allowBloom = allowBloom;
    let mut pSig: Option<SubrtnSig> = None; /* Signature for this subroutine */
    let mut sigAddr: i32 = -1; /* Address of the OP_BeginSubrtn holding pSig */

    debug_assert!(db.ppr().pVdbe.is_some());

    /* The evaluation of the IN must be repeated every time it
     ** is encountered if any of the following is true:
     **
     **    *  The right-hand side is a correlated subquery
     **    *  The right-hand side is an expression list containing variables
     **    *  We are inside a trigger
     **
     ** If all of the above are false, then we can compute the RHS just once
     ** and reuse it many names.
     */
    if !hp(db, pExpr, EP_VarSelect) && db.ppr().iSelfTab == 0 {
        /* Reuse of the RHS is allowed
         **
         ** Compute a signature for the RHS of the IN operator to facility
         ** finding and reusing prior instances of the same IN operator.
         */
        if hp(db, pExpr, EP_xIsSelect) {
            let s = db.ex(pExpr).x.pSelect;
            if (db.sel(s).selFlags & SF_All) == 0 {
                pSig = Some(SubrtnSig {
                    selId: db.sel(s).selId as i32,
                    zAff: Some(exprINAffinity(db, pExpr)),
                    ..SubrtnSig::default()
                });
            }
        }

        /* Check to see if there is a prior materialization of the RHS of
         ** this IN operator.  If there is, then make use of that prior
         ** materialization rather than recomputing it.
         */
        if hp(db, pExpr, EP_Subrtn) || findCompatibleInRhsSubrtn(db, pExpr, pSig.as_ref()) {
            addrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
            if hp(db, pExpr, EP_xIsSelect) {
                let selId = db.sel(db.ex(pExpr).x.pSelect).selId;
                sqlite3VdbeExplain(db, 0, b"REUSE LIST SUBQUERY %d", &[PfArg::from(selId)]);
            }
            let (regReturn, iAddr, iTable) = {
                let e = db.ex(pExpr);
                (e.y.sub.regReturn, e.y.sub.iAddr, e.iTable)
            };
            sqlite3VdbeAddOp2(db.v(), OP_Gosub, regReturn, iAddr);
            debug_assert!(iTab != iTable);
            sqlite3VdbeAddOp2(db.v(), OP_OpenDup, iTab, iTable);
            sqlite3VdbeJumpHere(db.v(), addrOnce);
            return;
        }

        /* Begin coding the subroutine */
        debug_assert!(!db.ex(pExpr).UseYWin());
        debug_assert!(!hp(db, pExpr, EP_TokenOnly | EP_Reduced));
        db.exm(pExpr).flags |= EP_Subrtn;
        db.pp().nMem += 1;
        let regReturn = db.ppr().nMem;
        let iAddr = sqlite3VdbeAddOp2(db.v(), OP_BeginSubrtn, 0, regReturn) + 1;
        {
            let e = db.exm(pExpr);
            e.y.sub.regReturn = regReturn;
            e.y.sub.iAddr = iAddr;
        }
        if let Some(sig) = pSig.as_mut() {
            sig.bComplete = 0;
            sig.iAddr = iAddr;
            sig.regReturn = regReturn;
            sig.iTable = iTab;
            db.pp().mSubrtnSig = (1 << (sig.selId & 7)) as u8;
            sigAddr = iAddr - 1;
            sqlite3VdbeChangeP4(
                db.v(),
                -1,
                P4::SubrtnSig(Box::new(sig.clone())),
                P4_SUBRTNSIG as i32,
            );
        }
        addrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
    }

    /* Check to see if this is a vector IN operator */
    let pLeft = db.ex(pExpr).pLeft;
    let nVal = exprVectorSize(db, pLeft);

    /* Construct the ephemeral table that will contain the content of
     ** RHS of the IN operator.
     */
    db.exm(pExpr).iTable = iTab;
    let addr = sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, iTab, nVal);
    let mut pKeyInfo = crate::select::sqlite3KeyInfoAlloc(db, nVal, 1);
    let Some(kinfo) = pKeyInfo.clone() else {
        return;
    };

    if hp(db, pExpr, EP_xIsSelect) {
        /* Case 1:     expr IN (SELECT ...)
         **
         ** Generate code to write the results of the select into the temporary
         ** table allocated and opened above.
         */
        let pSelect = db.ex(pExpr).x.pSelect;
        let pEList = db.sel(pSelect).pEList;
        let selId = db.sel(pSelect).selId;

        sqlite3VdbeExplain(
            db,
            1,
            b"%sLIST SUBQUERY %d",
            &[
                PfArg::from(if addrOnce != 0 { "" } else { "CORRELATED " }),
                PfArg::from(selId),
            ],
        );
        /* If the LHS and RHS of the IN operator do not match, that
         ** error will have been caught long before we reach this point. */
        if el_n(db, pEList) == nVal {
            let mut dest = SelectDest::default();
            let mut addrBloom = 0;
            crate::select::sqlite3SelectDestInit(&mut dest, SRT_Set as i32, iTab);
            dest.zAffSdst = Some(exprINAffinity(db, pExpr));
            db.selm(pSelect).iLimit = 0;
            for i in 0..nVal {
                let p = vectorFieldSubexpr(db, pLeft, i);
                let pColl = sqlite3BinaryCompareCollSeq(db, p, el_e(db, pEList, i));
                if !is_binary(&pColl) {
                    allowBloom = 0; /* tag-202607231411 */
                }
                let mut k = kinfo.borrow_mut();
                if (i as usize) < k.aColl.len() {
                    k.aColl[i as usize] = pColl;
                }
            }
            if addrOnce != 0 && allowBloom != 0 && db.OptimizationEnabled(SQLITE_BloomFilter) {
                db.pp().nMem += 1;
                let regBloom = db.ppr().nMem;
                addrBloom = sqlite3VdbeAddOp2(db.v(), OP_Blob, 10000, regBloom);
                dest.iSDParm2 = regBloom;
                sqlite3VdbeChangeP4(db.v(), addr, P4::KeyInfo(kinfo.clone()), P4_KEYINFO as i32);
                pKeyInfo = None;
            }
            let pCopy = sqlite3SelectDup(db, pSelect, 0);
            let rc = if db.mallocFailed != 0 {
                1
            } else {
                crate::select::sqlite3Select(db, pCopy, &mut dest)
            };
            crate::select::sqlite3SelectDelete(db, pCopy);
            if addrBloom != 0 {
                /* Remember that location of the Bloom filter in the P3 operand
                 ** of the OP_Once that began this subroutine. tag-202407032019 */
                let p2 = dest.iSDParm2;
                sqlite3VdbeGetOp(db.v(), addrOnce).p3 = p2;
                if p2 == 0 {
                    /* If the Bloom filter won't actually be used, keep it small */
                    sqlite3VdbeGetOp(db.v(), addrBloom).p1 = 10;
                }
            }
            if rc != 0 {
                if let Some(k) = &pKeyInfo {
                    crate::select::sqlite3KeyInfoUnref(db, k);
                }
                return;
            }
        }
    } else if db.ex(pExpr).x.pList.nn() {
        /* Case 2:     expr IN (exprlist)
         **
         ** For each expression, build an index key from the evaluation and
         ** store it in the temporary table. If <expr> is a column, then use
         ** that columns affinity when building index keys. If <expr> is not
         ** a column, use numeric affinity.
         */
        let pList = db.ex(pExpr).x.pList;
        let mut affinity = exprAffinity(db, pLeft); /* Affinity of the LHS of the IN */
        if affinity <= SQLITE_AFF_NONE {
            affinity = SQLITE_AFF_BLOB;
        } else if affinity == SQLITE_AFF_REAL {
            affinity = SQLITE_AFF_NUMERIC;
        }
        let c = sqlite3ExprCollSeq(db, pLeft);
        {
            let mut k = kinfo.borrow_mut();
            if !k.aColl.is_empty() {
                k.aColl[0] = c;
            }
        }

        /* Loop through each expression in <exprlist>. */
        let r1 = sqlite3GetTempReg(db);
        let r2 = sqlite3GetTempReg(db);
        for i in 0..el_n(db, pList) {
            let pE2 = el_e(db, pList, i);

            /* If the expression is not constant then we will need to
             ** disable the test that was generated above that makes sure
             ** this code only executes once.  Because for a non-constant
             ** expression we need to rerun this code each time.
             */
            if addrOnce != 0 && sqlite3ExprIsConstant(db, true, pE2) == 0 {
                sqlite3VdbeChangeToNoop(db.v(), addrOnce - 1);
                sqlite3VdbeChangeToNoop(db.v(), addrOnce);
                db.exm(pExpr).flags &= !EP_Subrtn;
                addrOnce = 0;
            }

            /* Evaluate the expression and insert it into the temp table */
            sqlite3ExprCode(db, pE2, r1);
            sqlite3VdbeAddOp4(db.v(), OP_MakeRecord, r1, 1, r2, P4::Z(vec![affinity]), 1);
            sqlite3VdbeAddOp4Int(db.v(), OP_IdxInsert, iTab, r2, r1, 1);
        }
        sqlite3ReleaseTempReg(db, r1);
        sqlite3ReleaseTempReg(db, r2);
    }
    if pSig.is_some() && sigAddr >= 0 {
        if let P4::SubrtnSig(s) = &mut sqlite3VdbeGetOp(db.v(), sigAddr).p4 {
            s.bComplete = 1;
        }
    }
    if let Some(k) = pKeyInfo {
        sqlite3VdbeChangeP4(db.v(), addr, P4::KeyInfo(k), P4_KEYINFO as i32);
    }
    if addrOnce != 0 {
        sqlite3VdbeAddOp1(db.v(), OP_NullRow, iTab);
        sqlite3VdbeJumpHere(db.v(), addrOnce);
        /* Subroutine return */
        let (regReturn, iAddr) = {
            let e = db.ex(pExpr);
            (e.y.sub.regReturn, e.y.sub.iAddr)
        };
        sqlite3VdbeAddOp3(db.v(), OP_Return, regReturn, iAddr, 1);
        sqlite3ClearTempRegCache(db);
    }
}

/// Generate code for scalar subqueries used as a subquery expression
/// or EXISTS operator:
///
///     (SELECT a FROM b)          -- subquery
///     EXISTS (SELECT a FROM b)   -- EXISTS subquery
///
/// The pExpr parameter is the SELECT or EXISTS operator to be coded.
///
/// Return the register that holds the result.  For a multi-column SELECT,
/// the result is stored in a contiguous array of registers and the
/// return value is the register of the left-most result column.
/// Return 0 if an error occurs.
pub fn sqlite3CodeSubselect(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    let mut addrOnce = 0; /* Address of OP_Once at top of subroutine */
    let rReg; /* Register storing resulting */
    let mut dest = SelectDest::default(); /* How to deal with SELECT result */

    debug_assert!(db.ppr().pVdbe.is_some());
    if nerr(db) != 0 {
        return 0;
    }
    let (op, pSel) = {
        let e = db.ex(pExpr);
        (e.op, e.x.pSelect)
    };
    debug_assert!(op == TK_EXISTS || op == TK_SELECT);
    debug_assert!(hp(db, pExpr, EP_xIsSelect));
    let selId = db.sel(pSel).selId;

    /* If this routine has already been coded, then invoke it as a
     ** subroutine. */
    if hp(db, pExpr, EP_Subrtn) {
        sqlite3VdbeExplain(db, 0, b"REUSE SUBQUERY %d", &[PfArg::from(selId)]);
        let (regReturn, iAddr, iTable) = {
            let e = db.ex(pExpr);
            (e.y.sub.regReturn, e.y.sub.iAddr, e.iTable)
        };
        sqlite3VdbeAddOp2(db.v(), OP_Gosub, regReturn, iAddr);
        return iTable;
    }

    /* Begin coding the subroutine */
    debug_assert!(!db.ex(pExpr).UseYWin());
    debug_assert!(!hp(db, pExpr, EP_Reduced | EP_TokenOnly));
    db.exm(pExpr).flags |= EP_Subrtn;
    db.pp().nMem += 1;
    let regReturn = db.ppr().nMem;
    let iAddr = sqlite3VdbeAddOp2(db.v(), OP_BeginSubrtn, 0, regReturn) + 1;
    {
        let e = db.exm(pExpr);
        e.y.sub.regReturn = regReturn;
        e.y.sub.iAddr = iAddr;
    }

    /* The evaluation of the EXISTS/SELECT must be repeated every time it
     ** is encountered if any of the following is true:
     **
     **    *  The right-hand side is a correlated subquery
     **    *  The right-hand side is an expression list containing variables
     **    *  We are inside a trigger
     **
     ** If all of the above are false, then we can run this code just once
     ** save the results, and reuse the same result on subsequent invocations.
     */
    if !hp(db, pExpr, EP_VarSelect) {
        addrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
    }

    /* For a SELECT, generate code to put the values for all columns of
     ** the first row into an array of registers and return the index of
     ** the first register.
     **
     ** If this is an EXISTS, write an integer 0 (not exists) or 1 (exists)
     ** into a register and return that register number.
     **
     ** In both cases, the query is augmented with "LIMIT 1".  Any
     ** preexisting limit is discarded in place of the new LIMIT 1.
     */
    sqlite3VdbeExplain(
        db,
        1,
        b"%sSCALAR SUBQUERY %d",
        &[
            PfArg::from(if addrOnce != 0 { "" } else { "CORRELATED " }),
            PfArg::from(selId),
        ],
    );
    let nReg = if op == TK_SELECT {
        el_n(db, db.sel(pSel).pEList)
    } else {
        1
    };
    let nMem1 = db.ppr().nMem + 1;
    crate::select::sqlite3SelectDestInit(&mut dest, 0, nMem1);
    db.pp().nMem += nReg;
    if op == TK_SELECT {
        dest.eDest = SRT_Mem;
        let (selFlags, pLimit) = {
            let s = db.sel(pSel);
            (s.selFlags, s.pLimit)
        };
        if (selFlags & SF_Distinct) != 0 && pLimit.nn() && db.ex(pLimit).pRight.nn() {
            /* If there is both a DISTINCT and an OFFSET clause, then allocate
             ** a separate dest.iSdst array for sqlite3Select() and other
             ** routines to populate. In this case results will be copied over
             ** into the dest.iSDParm array only after OFFSET processing. This
             ** ensures that in the case where OFFSET excludes all rows, the
             ** dest.iSDParm array is not left populated with the contents of the
             ** last row visited - it should be all NULLs if all rows were
             ** excluded by OFFSET.  */
            dest.iSdst = db.ppr().nMem + 1;
            db.pp().nMem += nReg;
        } else {
            dest.iSdst = dest.iSDParm;
        }
        dest.nSdst = nReg;
        let nMem = db.ppr().nMem;
        sqlite3VdbeAddOp3(db.v(), OP_Null, 0, dest.iSDParm, nMem);
    } else {
        dest.eDest = SRT_Exists;
        sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, dest.iSDParm);
    }
    let pSelLimit = db.sel(pSel).pLimit;
    if pSelLimit.nn() {
        /* The subquery already has a limit.  If the pre-existing limit X is
         ** not already integer value 1 or 0, then make the new limit X<>0 so that
         ** the new limit is either 1 or 0 */
        let pLeft = db.ex(pSelLimit).pLeft;
        let (isInt, iv) = {
            let l = db.ex(pLeft);
            (l.HasProperty(EP_IntValue), l.u.iValue)
        };
        if !isInt || (iv != 1 && iv != 0) {
            let mut pLimit = sqlite3ExprInt32(db, 0);
            if pLimit.nn() {
                db.exm(pLimit).affExpr = SQLITE_AFF_NUMERIC;
                let d = sqlite3ExprDup(db, pLeft, 0);
                pLimit = sqlite3PExpr(db, TK_NE as i32, d, pLimit);
            }
            sqlite3ExprDeferredDelete(db, pLeft);
            db.exm(pSelLimit).pLeft = pLimit;
        }
    } else {
        /* If there is no pre-existing limit add a limit of 1 */
        let pLimit = sqlite3ExprInt32(db, 1);
        let l = sqlite3PExpr(db, TK_LIMIT as i32, pLimit, ExprId::NULL);
        db.selm(pSel).pLimit = l;
    }
    db.selm(pSel).iLimit = 0;
    if crate::select::sqlite3Select(db, pSel, &mut dest) != 0 {
        let e = db.exm(pExpr);
        e.op2 = e.op;
        e.op = TK_ERROR;
        return 0;
    }
    rReg = dest.iSDParm;
    db.exm(pExpr).iTable = rReg;
    if addrOnce != 0 {
        sqlite3VdbeJumpHere(db.v(), addrOnce);
    }

    if dest.eDest == SRT_Mem {
        let e0 = el_e(db, db.sel(pSel).pEList, 0);
        if sqlite3ExprCanReturnSubtype(db, e0) != 0 {
            sqlite3VdbeAddOp1(db.v(), OP_ClrSubtype, rReg);
        }
    }

    /* Subroutine return */
    let (regReturn, iAddr) = {
        let e = db.ex(pExpr);
        (e.y.sub.regReturn, e.y.sub.iAddr)
    };
    sqlite3VdbeAddOp3(db.v(), OP_Return, regReturn, iAddr, 1);
    sqlite3ClearTempRegCache(db);
    rReg
}

/// Expr pIn is an IN(...) expression. This function checks that the
/// sub-select on the RHS of the IN() operator has the same number of
/// columns as the vector on the LHS. Or, if the RHS of the IN() is not
/// a sub-query, that the LHS is a vector of size 1.
pub fn sqlite3ExprCheckIN(db: &mut Sqlite3, pIn: ExprId) -> i32 {
    let pLeft = db.ex(pIn).pLeft;
    let nVector = exprVectorSize(db, pLeft);
    if hp(db, pIn, EP_xIsSelect) && db.mallocFailed == 0 {
        let n = el_n(db, db.sel(db.ex(pIn).x.pSelect).pEList);
        if nVector != n {
            sqlite3SubselectError(db, n, nVector);
            return 1;
        }
    } else if nVector != 1 {
        sqlite3VectorErrorMsg(db, pLeft);
        return 1;
    }
    0
}

/// Generate code for an IN expression.
///
///      x IN (SELECT ...)
///      x IN (value, value, ...)
///
/// The left-hand side (LHS) is a scalar or vector expression.  The
/// right-hand side (RHS) is an array of zero or more scalar values, or a
/// subquery.  If the RHS is a subquery, the number of result columns must
/// match the number of columns in the vector on the LHS.  If the RHS is
/// a list of values, the LHS must be a scalar.
///
/// The IN operator is true if the LHS value is contained within the RHS.
/// The result is false if the LHS is definitely not in the RHS.  The
/// result is NULL if the presence of the LHS in the RHS cannot be
/// determined due to NULLs.
///
/// This routine generates code that jumps to destIfFalse if the LHS is not
/// contained within the RHS.  If due to NULLs we cannot determine if the LHS
/// is contained in the RHS then jump to destIfNull.  If the LHS is contained
/// within the RHS then fall through.
///
/// See the separate in-operator.md documentation file in the canonical
/// SQLite source tree for additional information.
fn sqlite3ExprCodeIN(db: &mut Sqlite3, pExpr: ExprId, destIfFalse: i32, destIfNull: i32) {
    let mut rRhsHasNull = 0; /* Register that is true if RHS contains NULL values */
    let mut iDummy = 0; /* Dummy parameter to exprCodeVector() */
    let mut destStep6 = 0; /* Start of code for Step 6 */
    let mut iTab = 0; /* Index to use */
    let okConstFactor = db.ppr().okConstFactor;

    let pLeft = db.ex(pExpr).pLeft;
    if sqlite3ExprCheckIN(db, pExpr) != 0 {
        return;
    }
    let zAff = exprINAffinity(db, pExpr);
    let nVector = exprVectorSize(db, pLeft);
    let mut aiMap = vec![0i32; nVector.max(0) as usize];

    /* Attempt to compute the RHS. After this step, if anything other than
     ** IN_INDEX_NOOP is returned, the table opened with cursor iTab
     ** contains the values that make up the RHS. If IN_INDEX_NOOP is returned,
     ** the RHS has not yet been coded.  */
    debug_assert!(db.ppr().pVdbe.is_some());
    let eType = sqlite3FindInIndex(
        db,
        pExpr,
        (IN_INDEX_MEMBERSHIP | IN_INDEX_NOOP_OK) as u32,
        if destIfFalse == destIfNull {
            None
        } else {
            Some(&mut rRhsHasNull)
        },
        Some(&mut aiMap[..]),
        &mut iTab,
    );

    debug_assert!(
        nerr(db) != 0
            || nVector == 1
            || eType == IN_INDEX_EPH
            || eType == IN_INDEX_INDEX_ASC
            || eType == IN_INDEX_INDEX_DESC
    );

    /* Code the LHS, the <expr> from "<expr> IN (...)". If the LHS is a
     ** vector, then it is stored in an array of nVector registers starting
     ** at r1.
     **
     ** sqlite3FindInIndex() might have reordered the fields of the LHS vector
     ** so that the fields are in the same order as an existing index.   The
     ** aiMap[] array contains a mapping from the original LHS field order to
     ** the field order that matches the RHS index.
     **
     ** Avoid factoring the LHS of the IN(...) expression out of the loop,
     ** even if it is constant, as OP_Affinity may be used on the register
     ** by code generated below.  */
    db.pp().okConstFactor = 0;
    let mut rLhs = exprCodeVector(db, pLeft, &mut iDummy);
    db.pp().okConstFactor = okConstFactor;

    'finished: {
        /* If sqlite3FindInIndex() did not find or create an index that is
         ** suitable for evaluating the IN operator, then evaluate using a
         ** sequence of comparisons.
         **
         ** This is step (1) in the in-operator.md optimized algorithm.
         */
        if eType == IN_INDEX_NOOP {
            let labelOk = sqlite3VdbeMakeLabel(db);
            let mut regCkNull = 0;
            debug_assert!(nVector == 1);
            let pList = db.ex(pExpr).x.pList;
            let pColl = sqlite3ExprCollSeq(db, pLeft);
            if destIfNull != destIfFalse {
                regCkNull = sqlite3GetTempReg(db);
                sqlite3VdbeAddOp3(db.v(), OP_BitAnd, rLhs, rLhs, regCkNull);
            }
            let n = el_n(db, pList);
            for ii in 0..n {
                let mut regToFree = 0;
                let pE = el_e(db, pList, ii);
                let r2 = sqlite3ExprCodeTemp(db, pE, &mut regToFree);
                if regCkNull != 0 && exprCanBeNull(db, pE) {
                    sqlite3VdbeAddOp3(db.v(), OP_BitAnd, regCkNull, r2, regCkNull);
                }
                sqlite3ReleaseTempReg(db, regToFree);
                if ii < n - 1 || destIfNull != destIfFalse {
                    let op = if rLhs != r2 { OP_Eq } else { OP_NotNull };
                    add_op4_coll(db, op, rLhs, labelOk, r2, pColl.clone());
                    sqlite3VdbeChangeP5(db.v(), zAff[0] as u16);
                } else {
                    let op = if rLhs != r2 { OP_Ne } else { OP_IsNull };
                    debug_assert!(destIfNull == destIfFalse);
                    add_op4_coll(db, op, rLhs, destIfFalse, r2, pColl.clone());
                    sqlite3VdbeChangeP5(db.v(), zAff[0] as u16 | SQLITE_JUMPIFNULL);
                }
            }
            if regCkNull != 0 {
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, regCkNull, destIfNull);
                sqlite3VdbeGoto(db.v(), destIfFalse);
            }
            sqlite3VdbeResolveLabel(db.v(), labelOk);
            sqlite3ReleaseTempReg(db, regCkNull);
            break 'finished;
        }

        if eType != IN_INDEX_ROWID {
            /* If this IN operator will use an index, then the order of columns in the
             ** vector might be different from the order in the index.  In that case,
             ** we need to reorder the LHS values to be in index order.  Run Affinity
             ** before reordering the columns, so that the affinity is correct.
             */
            if nVector == 1 {
                let aff = zAff[0];
                if aff >= SQLITE_AFF_TEXT && aff != exprAffinity(db, pLeft) {
                    /* The OP_Affinity below may change the value. In this case, create a
                     ** copy of rLhs to run OP_Affinity on, in case the original register
                     ** is used again (e.g. if it is TK_AGG_COLUMN).  */
                    let rTmp = sqlite3GetTempReg(db);
                    sqlite3VdbeAddOp3(db.v(), OP_Copy, rLhs, rTmp, 0);
                    rLhs = rTmp;
                    sqlite3VdbeAddOp4(
                        db.v(),
                        OP_Affinity,
                        rLhs,
                        1,
                        0,
                        P4::Z(zAff[..1].to_vec()),
                        1,
                    );
                }
            } else {
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Affinity,
                    rLhs,
                    nVector,
                    0,
                    P4::Z(zAff.clone()),
                    nVector,
                );
            }

            let mut i = 0;
            while i < nVector && aiMap[i as usize] == i {
                i += 1;
            } /* Are LHS fields reordered? */
            if i != nVector {
                /* Need to reorder the LHS fields according to aiMap */
                let rLhsOrig = rLhs;
                rLhs = sqlite3GetTempRange(db, nVector);
                for i in 0..nVector {
                    sqlite3VdbeAddOp3(db.v(), OP_Copy, rLhsOrig + i, rLhs + aiMap[i as usize], 0);
                }
            }
        }

        /* Step 2: Check to see if the LHS contains any NULL columns.  If the
         ** LHS does contain NULLs then the result must be either FALSE or NULL.
         ** We will then skip the binary search of the RHS.
         */
        let destStep2;
        if destIfNull == destIfFalse {
            destStep2 = destIfFalse;
        } else {
            destStep6 = sqlite3VdbeMakeLabel(db);
            destStep2 = destStep6;
        }
        for i in 0..nVector {
            let p = vectorFieldSubexpr(db, pLeft, i);
            if nerr(db) != 0 {
                return; /* goto sqlite3ExprCodeIN_oom_error */
            }
            if exprCanBeNull(db, p) {
                sqlite3VdbeAddOp2(db.v(), OP_IsNull, rLhs + aiMap[i as usize], destStep2);
            }
        }

        /* Step 3.  The LHS is now known to be non-NULL.  Do the binary search
         ** of the RHS using the LHS as a probe.  If found, the result is
         ** true.
         */
        let addrTruthOp;
        if eType == IN_INDEX_ROWID {
            /* In this case, the RHS is the ROWID of table b-tree and so we also
             ** know that the RHS is non-NULL.  Hence, we combine steps 3 and 4
             ** into a single opcode. */
            debug_assert!(nVector == 1);
            sqlite3VdbeAddOp3(db.v(), OP_SeekRowid, iTab, destIfFalse, rLhs);
            addrTruthOp = sqlite3VdbeAddOp0(db.v(), OP_Goto); /* Return True */
        } else {
            if destIfFalse == destIfNull {
                /* Combine Step 3 and Step 5 into a single opcode */
                if hp(db, pExpr, EP_Subrtn) {
                    let iAddr = db.ex(pExpr).y.sub.iAddr;
                    let p3 = sqlite3VdbeGetOp(db.v(), iAddr).p3;
                    if p3 > 0 {
                        /* tag-202407032019 */
                        sqlite3VdbeAddOp4Int(db.v(), OP_Filter, p3, destIfFalse, rLhs, nVector);
                    }
                }
                sqlite3VdbeAddOp4Int(db.v(), OP_NotFound, iTab, destIfFalse, rLhs, nVector);
                break 'finished;
            }
            /* Ordinary Step 3, for the case where FALSE and NULL are distinct */
            addrTruthOp = sqlite3VdbeAddOp4Int(db.v(), OP_Found, iTab, 0, rLhs, nVector);
        }

        /* Step 4.  If the RHS is known to be non-NULL and we did not find
         ** an match on the search above, then the result must be FALSE.
         */
        if rRhsHasNull != 0 && nVector == 1 {
            sqlite3VdbeAddOp2(db.v(), OP_NotNull, rRhsHasNull, destIfFalse);
        }

        /* Step 5.  If we do not care about the difference between NULL and
         ** FALSE, then just return false.
         */
        if destIfFalse == destIfNull {
            sqlite3VdbeGoto(db.v(), destIfFalse);
        }

        /* Step 6: Loop through rows of the RHS.  Compare each row to the LHS.
         ** If any comparison is NULL, then the result is NULL.  If all
         ** comparisons are FALSE then the final result is FALSE.
         **
         ** For a scalar LHS, it is sufficient to check just the first row
         ** of the RHS.
         */
        if destStep6 != 0 {
            sqlite3VdbeResolveLabel(db.v(), destStep6);
        }
        let addrTop = if eType == IN_INDEX_INDEX_DESC {
            sqlite3VdbeAddOp2(db.v(), OP_Last, iTab, destIfFalse)
        } else {
            sqlite3VdbeAddOp2(db.v(), OP_Rewind, iTab, destIfFalse)
        };
        let destNotNull = if nVector > 1 {
            sqlite3VdbeMakeLabel(db)
        } else {
            /* For nVector==1, combine steps 6 and 7 by immediately returning
             ** FALSE if the first comparison is not NULL */
            destIfFalse
        };
        for i in 0..nVector {
            let r3 = sqlite3GetTempReg(db);
            let p = vectorFieldSubexpr(db, pLeft, i);
            let pColl = if hp(db, pExpr, EP_xIsSelect) {
                let pRhs = el_e(db, db.sel(db.ex(pExpr).x.pSelect).pEList, i);
                sqlite3BinaryCompareCollSeq(db, p, pRhs)
            } else {
                /* If the RHS of the IN(...) expression are scalar expressions, do
                 ** not consider their collation sequences. The documentation says
                 ** "The collating sequence used for expressions of the form "x IN (y, z,
                 ** ...)" is the collating sequence of x.".  */
                sqlite3ExprCollSeq(db, p)
            };
            let m = aiMap[i as usize];
            sqlite3VdbeAddOp3(db.v(), OP_Column, iTab, m, r3);
            add_op4_coll(db, OP_Ne, rLhs + m, destNotNull, r3, pColl);
            sqlite3ReleaseTempReg(db, r3);
        }
        sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, destIfNull);
        if nVector > 1 {
            sqlite3VdbeResolveLabel(db.v(), destNotNull);
            sqlite3VdbeAddOp2(db.v(), OP_Next, iTab, addrTop + 1);

            /* Step 7:  If we reach this point, we know that the result must
             ** be false. */
            sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, destIfFalse);
        }

        /* Jumps here in order to return true. */
        sqlite3VdbeJumpHere(db.v(), addrTruthOp);
    }
    /* sqlite3ExprCodeIN_finished: */
}

/// Generate an instruction that will put the floating point
/// value described by z[0..n-1] into register iMem.
fn codeReal(v: &mut Vdbe, z: Option<&[u8]>, negateFlag: bool, iMem: i32) {
    if let Some(z) = z {
        let mut value = 0.0f64;
        crate::util::sqlite3AtoF(Some(z), &mut value);
        if negateFlag {
            value = -value;
        }
        sqlite3VdbeAddDouble(v, iMem, value);
    }
}

/// Generate an instruction that will put the integer describe by
/// text z[0..n-1] into register iMem.
///
/// Expr.u.zToken is always UTF8 and zero-terminated.
fn codeInteger(db: &mut Sqlite3, pExpr: ExprId, negFlag: bool, iMem: i32) {
    let (flags, iValue, z) = {
        let e = db.ex(pExpr);
        (e.flags, e.u.iValue, e.u.zToken.clone())
    };
    if (flags & EP_IntValue) != 0 {
        let mut i = iValue;
        debug_assert!(i >= 0);
        if negFlag {
            i = -i;
        }
        sqlite3VdbeAddOp2(db.v(), OP_Integer, i, iMem);
    } else {
        let mut value: i64 = 0;
        let z = z.unwrap_or_default();
        let c = crate::util::sqlite3DecOrHexToI64(Some(&z), &mut value);
        if (c == 3 && !negFlag) || (c == 2) || (negFlag && value == SMALLEST_INT64) {
            if crate::util::sqlite3_strnicmp(Some(&z), Some(b"0x"), 2) == 0 {
                errmsg!(
                    db,
                    "hex literal too big: %s%#T",
                    if negFlag { "-" } else { "" },
                    pExpr
                );
            } else {
                codeReal(db.v(), Some(&z), negFlag, iMem);
            }
        } else {
            if negFlag {
                value = if c == 3 {
                    SMALLEST_INT64
                } else {
                    value.wrapping_neg()
                };
            }
            sqlite3VdbeAddInt64(db.v(), iMem, value);
        }
    }
}

/// Generate code that will load into register regOut a value that is
/// appropriate for the iIdxCol-th column of index pIdx.
pub fn sqlite3ExprCodeLoadIndexColumn(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    iTabCur: i32,
    iIdxCol: i32,
    regOut: i32,
) {
    let iTabCol = pIdx.borrow().aiColumn[iIdxCol as usize];
    if iTabCol == XN_EXPR {
        let t = pIdx.borrow().aColExpr.clone();
        debug_assert!(t.is_some());
        let pList = db.tree(&t);
        debug_assert!(el_n(db, pList) > iIdxCol);
        let e = el_e(db, pList, iIdxCol);
        db.pp().iSelfTab = iTabCur + 1;
        sqlite3ExprCodeCopy(db, e, regOut);
        db.pp().iSelfTab = 0;
    } else {
        let pTab = pIdx.borrow().table();
        sqlite3ExprCodeGetColumnOfTable(db, &pTab, iTabCur, iTabCol as i32, regOut);
    }
}

/// Generate code that will compute the value of generated column pCol
/// and store the result in register regOut
pub fn sqlite3ExprCodeGeneratedColumn(db: &mut Sqlite3, pTab: &TableRef, pCol: usize, regOut: i32) {
    let nErr = nerr(db);
    debug_assert!(db.ppr().pVdbe.is_some());
    let iSelfTab = db.ppr().iSelfTab;
    debug_assert!(iSelfTab != 0);
    let iAddr = if iSelfTab > 0 {
        sqlite3VdbeAddOp3(db.v(), OP_IfNullRow, iSelfTab - 1, 0, regOut)
    } else {
        0
    };
    let e = crate::build::sqlite3ColumnExpr(db, pTab, pCol);
    sqlite3ExprCodeCopy(db, e, regOut);
    let (colFlags, affinity, tabFlags) = {
        let t = pTab.borrow();
        (t.aCol[pCol].colFlags, t.aCol[pCol].affinity, t.tabFlags)
    };
    if (colFlags & COLFLAG_VIRTUAL) != 0 && (tabFlags & TF_Strict) != 0 {
        let p3 = 2 + pCol as i32;
        sqlite3VdbeAddOp4(
            db.v(),
            OP_TypeCheck,
            regOut,
            1,
            p3,
            P4::Tab(pTab.clone()),
            P4_TABLE as i32,
        );
    } else if affinity >= SQLITE_AFF_TEXT {
        sqlite3VdbeAddOp4(db.v(), OP_Affinity, regOut, 1, 0, P4::Z(vec![affinity]), 1);
    }
    if iAddr != 0 {
        sqlite3VdbeJumpHere(db.v(), iAddr);
    }
    if nerr(db) > nErr {
        db.errByteOffset = -1;
    }
}

/// Generate code to extract the value of the iCol-th column of a table.
///
/// (C takes the Vdbe; the generated-column case needs the Parse, so the
/// port takes the connection and codes into `db.v()`.)
pub fn sqlite3ExprCodeGetColumnOfTable(
    db: &mut Sqlite3,
    pTab: &TableRef,
    iTabCur: i32,
    iCol: i32,
    regOut: i32,
) {
    debug_assert!(iCol != XN_EXPR as i32);
    let (iPKey, isVirtual, hasRowid) = {
        let t = pTab.borrow();
        (t.iPKey as i32, t.IsVirtual(), t.HasRowid())
    };
    if iCol < 0 || iCol == iPKey {
        sqlite3VdbeAddOp2(db.v(), OP_Rowid, iTabCur, regOut);
    } else {
        let x;
        if isVirtual {
            x = iCol;
            sqlite3VdbeAddOp3(db.v(), OP_VColumn, iTabCur, x, regOut);
            return;
        }
        let (colFlags, zCnName) = {
            let t = pTab.borrow();
            (
                t.aCol[iCol as usize].colFlags,
                t.aCol[iCol as usize].zCnName.clone(),
            )
        };
        if (colFlags & COLFLAG_VIRTUAL) != 0 {
            if (colFlags & COLFLAG_BUSY) != 0 {
                errmsg!(db, "generated column loop on \"%s\"", &zCnName);
            } else {
                let savedSelfTab = db.ppr().iSelfTab;
                pTab.borrow_mut().aCol[iCol as usize].colFlags |= COLFLAG_BUSY;
                db.pp().iSelfTab = iTabCur + 1;
                sqlite3ExprCodeGeneratedColumn(db, pTab, iCol as usize, regOut);
                db.pp().iSelfTab = savedSelfTab;
                pTab.borrow_mut().aCol[iCol as usize].colFlags &= !COLFLAG_BUSY;
            }
            return;
        } else if !hasRowid {
            let pk = crate::build::sqlite3PrimaryKeyIndex(pTab).expect("WITHOUT ROWID without PK");
            x = crate::build::sqlite3TableColumnToIndex(&pk, iCol);
        } else {
            x = crate::build::sqlite3TableColumnToStorage(pTab, iCol as i16) as i32;
        }
        sqlite3VdbeAddOp3(db.v(), OP_Column, iTabCur, x, regOut);
        crate::update::sqlite3ColumnDefault(db, pTab, iCol, regOut);
    }
}

/// Generate code that will extract the iColumn-th column from
/// table pTab and store the column value in register iReg.
///
/// There must be an open cursor to pTab in iTable when this routine
/// is called.  If iColumn<0 then code is generated that extracts the rowid.
pub fn sqlite3ExprCodeGetColumn(
    db: &mut Sqlite3,
    pTab: &TableRef,
    iColumn: i32,
    iTable: i32,
    iReg: i32,
    p5: u8,
) -> i32 {
    debug_assert!(db.ppr().pVdbe.is_some());
    sqlite3ExprCodeGetColumnOfTable(db, pTab, iTable, iColumn, iReg);
    if p5 != 0 {
        if let Some(pOp) = sqlite3VdbeGetLastOp(db.v()) {
            if pOp.opcode == OP_Column {
                pOp.p5 = p5 as u16;
            }
            if pOp.opcode == OP_VColumn {
                pOp.p5 = p5 as u16 & OPFLAG_NOCHNG;
            }
        }
    }
    iReg
}

/// Generate code to move content from registers iFrom...iFrom+nReg-1
/// over to iTo..iTo+nReg-1.
pub fn sqlite3ExprCodeMove(db: &mut Sqlite3, iFrom: i32, iTo: i32, nReg: i32) {
    sqlite3VdbeAddOp3(db.v(), OP_Move, iFrom, iTo, nReg);
}

/// Convert a scalar expression node to a TK_REGISTER referencing
/// register iReg.  The caller must ensure that iReg already contains
/// the correct value for the expression.
pub fn sqlite3ExprToRegister(db: &mut Sqlite3, pExpr: ExprId, iReg: i32) {
    let p = skipCollateAndLikely(db, pExpr);
    if p.is_null() {
        return;
    }
    let e = db.exm(p);
    if e.op == TK_REGISTER {
        debug_assert!(e.iTable == iReg);
    } else {
        e.op2 = e.op;
        e.op = TK_REGISTER;
        e.iTable = iReg;
        e.flags &= !EP_Skip;
    }
}

/// Evaluate an expression (either a vector or a scalar expression) and store
/// the result in contiguous temporary registers.  Return the index of
/// the first register used to store the result.
///
/// If the returned result register is a temporary scalar, then also write
/// that register number into *piFreeable.  If the returned result register
/// is not a temporary or if the expression is a vector set *piFreeable
/// to 0.
fn exprCodeVector(db: &mut Sqlite3, p: ExprId, piFreeable: &mut i32) -> i32 {
    let iResult;
    let nResult = exprVectorSize(db, p);
    if nResult == 1 {
        iResult = sqlite3ExprCodeTemp(db, p, piFreeable);
    } else {
        *piFreeable = 0;
        if db.ex(p).op == TK_SELECT {
            iResult = sqlite3CodeSubselect(db, p);
        } else {
            iResult = db.ppr().nMem + 1;
            db.pp().nMem += nResult;
            let pList = db.ex(p).x.pList;
            for i in 0..nResult {
                let e = el_e(db, pList, i);
                sqlite3ExprCodeFactorable(db, e, i + iResult);
            }
        }
    }
    iResult
}

/// If the last opcode is a OP_Copy, then set the do-not-merge flag (p5)
/// so that a subsequent copy will not be merged into this one.
fn setDoNotMergeFlagOnCopy(v: &mut Vdbe) {
    let isCopy = matches!(sqlite3VdbeGetLastOp(v), Some(op) if op.opcode == OP_Copy);
    if isCopy {
        sqlite3VdbeChangeP5(v, 1); /* Tag trailing OP_Copy as not mergeable */
    }
}

/// Generate code to implement special SQL functions that are implemented
/// in-line rather than by using the usual callbacks.
fn exprCodeInlineFunction(db: &mut Sqlite3, pFarg: ExprListId, iFuncId: i32, target: i32) -> i32 {
    let mut target = target;
    debug_assert!(db.ppr().pVdbe.is_some());
    debug_assert!(pFarg.nn());
    let nFarg = el_n(db, pFarg);
    debug_assert!(nFarg > 0); /* All in-line functions have at least one argument */
    match iFuncId {
        INLINEFUNC_coalesce => {
            /* Attempt a direct implementation of the built-in COALESCE() and
             ** IFNULL() functions.  This avoids unnecessary evaluation of
             ** arguments past the first non-NULL argument.
             */
            let endCoalesce = sqlite3VdbeMakeLabel(db);
            debug_assert!(nFarg >= 2);
            let e0 = el_e(db, pFarg, 0);
            sqlite3ExprCode(db, e0, target);
            for i in 1..nFarg {
                sqlite3VdbeAddOp2(db.v(), OP_NotNull, target, endCoalesce);
                let ei = el_e(db, pFarg, i);
                sqlite3ExprCode(db, ei, target);
            }
            setDoNotMergeFlagOnCopy(db.v());
            sqlite3VdbeResolveLabel(db.v(), endCoalesce);
        }
        INLINEFUNC_iif => {
            let caseExpr = tmp_expr(db, TK_CASE);
            db.exm(caseExpr).x.pList = pFarg;
            return sqlite3ExprCodeTarget(db, caseExpr, target);
        }
        INLINEFUNC_expr_compare => {
            /* Compare two expressions using sqlite3ExprCompare() */
            debug_assert!(nFarg == 2);
            let (a, b) = (el_e(db, pFarg, 0), el_e(db, pFarg, 1));
            let r = sqlite3ExprCompare(db, false, a, b, -1);
            sqlite3VdbeAddOp2(db.v(), OP_Integer, r, target);
        }
        INLINEFUNC_expr_implies_expr => {
            /* Compare two expressions using sqlite3ExprImpliesExpr() */
            debug_assert!(nFarg == 2);
            let (a, b) = (el_e(db, pFarg, 0), el_e(db, pFarg, 1));
            let r = sqlite3ExprImpliesExpr(db, a, b, -1);
            sqlite3VdbeAddOp2(db.v(), OP_Integer, r, target);
        }
        INLINEFUNC_implies_nonnull_row => {
            /* Result of sqlite3ExprImpliesNonNullRow() */
            debug_assert!(nFarg == 2);
            let pA1 = el_e(db, pFarg, 1);
            if db.ex(pA1).op == TK_COLUMN {
                let a0 = el_e(db, pFarg, 0);
                let iTable = db.ex(pA1).iTable;
                let r = sqlite3ExprImpliesNonNullRow(db, a0, iTable, 1);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, r, target);
            } else {
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
            }
        }
        INLINEFUNC_affinity => {
            /* The AFFINITY() function evaluates to a string that describes
             ** the type affinity of the argument.  This is used for testing of
             ** the SQLite type logic.
             */
            const azAff: [&[u8]; 6] = [
                b"blob", b"text", b"numeric", b"integer", b"real", b"flexnum",
            ];
            debug_assert!(nFarg == 1);
            let aff = exprAffinity(db, el_e(db, pFarg, 0));
            debug_assert!(
                aff <= SQLITE_AFF_NONE || (aff >= SQLITE_AFF_BLOB && aff <= SQLITE_AFF_FLEXNUM)
            );
            let z: &[u8] = if aff <= SQLITE_AFF_NONE {
                b"none"
            } else {
                azAff[(aff - SQLITE_AFF_BLOB) as usize]
            };
            sqlite3VdbeLoadString(db.v(), target, z);
        }
        _ => {
            /* The UNLIKELY() function is a no-op.  The result is the value
             ** of the first argument.
             */
            debug_assert!(nFarg == 1 || nFarg == 2);
            let e0 = el_e(db, pFarg, 0);
            target = sqlite3ExprCodeTarget(db, e0, target);
        }
    }
    target
}

/// Expression Node callback for sqlite3ExprCanReturnSubtype().  If
/// pExpr is able to return a subtype, set pWalker->eCode and abort
/// the search.  If pExpr can never return a subtype, prune search.
///
/// The only expressions that can return a subtype are:
///
///    1.  A function, including an aggregate function
///    2.  The no-op "+" operator
///    3.  A CASE...END expression
///    4.  A CAST() expression
///    5.  A "expr COLLATE colseq" expression.
///
/// For any other kind of expression, prune the search.
///
/// For case 1, the expression can yield a subtype if the function has
/// the SQLITE_RESULT_SUBTYPE property.  Functions can also return
/// a subtype (via sqlite3_result_value()) if any of the arguments can
/// return a subtype.
///
/// In all cases 1 through 5, the expression might also return a subtype
/// if any operand can return a subtype.
fn exprNodeCanReturnSubtype(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, pList, zToken) = {
        let e = w.db.ex(pExpr);
        (e.op, e.x.pList, e.u.zToken.clone())
    };
    if op == TK_CASE || op == TK_UPLUS || op == TK_COLLATE || op == TK_CAST {
        return WRC_Continue;
    }
    if op != TK_FUNCTION && op != TK_AGG_FUNCTION {
        return WRC_Prune;
    }
    let n = el_n(w.db, pList);
    let enc = w.db.ENC();
    let pDef = crate::callback::sqlite3FindFunction(w.db, zToken.as_deref(), n, enc, 0);
    let Some(pDef) = pDef else {
        return WRC_Continue;
    };
    if (pDef.funcFlags & SQLITE_RESULT_SUBTYPE) != 0 {
        w.eCode = 1;
        return WRC_Abort;
    }
    WRC_Continue
}

/// Return TRUE if expression pExpr is able to return a subtype.
///
/// A TRUE return does not guarantee that a subtype will be returned.
/// It only indicates that a subtype return is possible.  False positives
/// are acceptable as they only disable an optimization.  False negatives,
/// on the other hand, can lead to incorrect answers.
pub fn sqlite3ExprCanReturnSubtype(db: &mut Sqlite3, pExpr: ExprId) -> i32 {
    let mut w = Walker::new(db);
    w.pParse = true;
    w.xExprCallback = Some(exprNodeCanReturnSubtype);
    sqlite3WalkExpr(&mut w, pExpr);
    w.eCode as i32
}

/// Check to see if pExpr is one of the indexed expressions on pParse->pIdxEpr.
/// If it is, then resolve the expression by reading from the index and
/// return the register into which the value has been read.  If pExpr is
/// not an indexed expression, then return negative.
///
/// The C list is a singly linked list with new entries pushed at the head;
/// the Vec keeps push order, so it is scanned from the end.
fn sqlite3IndexedExprLookup(db: &mut Sqlite3, pExpr: ExprId, target: i32) -> i32 {
    let list: Vec<IndexedExpr> = db.ppr().pIdxEpr.clone();
    for p in list.iter().rev() {
        let mut iDataCur = p.iDataCur;
        if iDataCur < 0 {
            continue;
        }
        let iSelfTab = db.ppr().iSelfTab;
        if iSelfTab != 0 {
            if p.iDataCur != iSelfTab - 1 {
                continue;
            }
            iDataCur = -1;
        }
        if sqlite3ExprCompare(db, false, pExpr, p.pExpr, iDataCur) != 0 {
            continue;
        }
        debug_assert!(p.aff >= SQLITE_AFF_BLOB && p.aff <= SQLITE_AFF_NUMERIC);
        let exprAff = exprAffinity(db, pExpr);
        if (exprAff <= SQLITE_AFF_BLOB && p.aff != SQLITE_AFF_BLOB)
            || (exprAff == SQLITE_AFF_TEXT && p.aff != SQLITE_AFF_TEXT)
            || (exprAff >= SQLITE_AFF_NUMERIC && p.aff != SQLITE_AFF_NUMERIC)
        {
            /* Affinity mismatch on a generated column */
            continue;
        }

        /* Functions that might set a subtype should not be replaced by the
         ** value taken from an expression index if they are themselves an
         ** argument to another scalar function or aggregate.
         ** https://sqlite.org/forum/forumpost/68d284c86b082c3e */
        if hp(db, pExpr, EP_SubtArg) && sqlite3ExprCanReturnSubtype(db, pExpr) != 0 {
            continue;
        }

        if p.bMaybeNullRow != 0 || sqlite3ExprIsConstant(db, true, pExpr) != 0 {
            /* The value of pExpr should be computed from the original expression,
             ** not from the index, if either:
             **
             **    1.   The index might be on a NULL row due to an outer join.
             **
             **    2.   The expression is a constant.  This prevents an attempt to
             **         use an index value outside of the loop that positions the
             **         index.  (Bug 2026-10-11T19:26:40Z).
             */
            let addr = sqlite3VdbeCurrentAddr(db.v());
            sqlite3VdbeAddOp3(db.v(), OP_IfNullRow, p.iIdxCur, addr + 3, target);
            sqlite3VdbeAddOp3(db.v(), OP_Column, p.iIdxCur, p.iIdxCol, target);
            sqlite3VdbeGoto(db.v(), 0);
            let saved = std::mem::take(&mut db.pp().pIdxEpr);
            sqlite3ExprCode(db, pExpr, target);
            db.pp().pIdxEpr = saved;
            sqlite3VdbeJumpHere(db.v(), addr + 2);
        } else {
            sqlite3VdbeAddOp3(db.v(), OP_Column, p.iIdxCur, p.iIdxCol, target);
        }
        return target;
    }
    -1 /* Not found */
}

/// Expression pExpr is guaranteed to be a TK_COLUMN or equivalent. This
/// function checks the Parse.pIdxPartExpr list to see if this column
/// can be replaced with a constant value. If so, it generates code to
/// put the constant value in a register (ideally, but not necessarily,
/// register iTarget) and returns the register number.
///
/// Or, if the TK_COLUMN cannot be replaced by a constant, zero is
/// returned.
fn exprPartidxExprLookup(db: &mut Sqlite3, pExpr: ExprId, iTarget: i32) -> i32 {
    let list: Vec<IndexedExpr> = db.ppr().pIdxPartExpr.clone();
    let (iColumn, iTable) = {
        let e = db.ex(pExpr);
        (e.iColumn as i32, e.iTable)
    };
    for p in list.iter().rev() {
        if iColumn == p.iIdxCol && iTable == p.iDataCur {
            let mut addr = 0;

            if p.bMaybeNullRow != 0 {
                addr = sqlite3VdbeAddOp1(db.v(), OP_IfNullRow, p.iIdxCur);
            }
            let ret = sqlite3ExprCodeTarget(db, p.pExpr, iTarget);
            sqlite3VdbeAddOp4(db.v(), OP_Affinity, ret, 1, 0, P4::Z(vec![p.aff]), 1);
            if sqlite3ExprCanReturnSubtype(db, p.pExpr) != 0 {
                /* If the expression value may have a sub-type, clear it. Values
                 ** read from columns do not have subtypes. */
                sqlite3VdbeAddOp1(db.v(), OP_ClrSubtype, ret);
            }
            if addr != 0 {
                sqlite3VdbeJumpHere(db.v(), addr);
                sqlite3VdbeChangeP3(db.v(), addr, ret);
            }
            return ret;
        }
    }
    0
}

/// Generate code that evaluates an AND or OR operator leaving a
/// boolean result in a register.  pExpr is the AND/OR expression.
/// Store the result in the "target" register.  Use short-circuit
/// evaluation to avoid computing both operands, if possible.
///
/// The code generated might require the use of a temporary register.
/// If it does, then write the number of that temporary register
/// into *pTmpReg.  If not, leave *pTmpReg unchanged.
fn exprCodeTargetAndOr(db: &mut Sqlite3, pExpr: ExprId, target: i32, pTmpReg: &mut i32) -> i32 {
    let addrSkip;
    let mut regSS = 0; /* Register holding computed operand when other omitted */
    let r1;
    let r2;
    debug_assert!(pExpr.nn());
    let (op, pLeft, pRight) = {
        let e = db.ex(pExpr);
        (e.op, e.pLeft, e.pRight)
    };
    debug_assert!(op == TK_AND || op == TK_OR);
    let pAlt = simplifiedAndOr(db, pExpr);
    if pAlt != pExpr {
        let r1 = sqlite3ExprCodeTarget(db, pAlt, target);
        sqlite3VdbeAddOp3(db.v(), OP_And, r1, r1, target);
        return target;
    }
    let skipOp = if op == TK_AND { OP_IfNot } else { OP_If };
    if exprEvalRhsFirst(db, pExpr) {
        /* Compute the right operand first.  Skip the computation of the left
         ** operand if the right operand fully determines the result */
        r2 = sqlite3ExprCodeTarget(db, pRight, target);
        regSS = r2;
        addrSkip = sqlite3VdbeAddOp1(db.v(), skipOp, r2);
        r1 = sqlite3ExprCodeTemp(db, pLeft, pTmpReg);
    } else {
        /* Compute the left operand first */
        r1 = sqlite3ExprCodeTarget(db, pLeft, target);
        if hp(db, pRight, EP_Subquery) {
            /* Skip over the computation of the right operand if the right
             ** operand is a subquery and the left operand completely determines
             ** the result */
            regSS = r1;
            addrSkip = sqlite3VdbeAddOp1(db.v(), skipOp, r1);
        } else {
            addrSkip = 0;
        }
        r2 = sqlite3ExprCodeTemp(db, pRight, pTmpReg);
    }
    sqlite3VdbeAddOp3(db.v(), op, r2, r1, target);
    if addrSkip != 0 {
        let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
        sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, a);
        sqlite3VdbeJumpHere(db.v(), addrSkip);
        sqlite3VdbeAddOp3(db.v(), OP_Or, regSS, regSS, target);
    }
    target
}

// ---------------------------------------------------------------------------
// The code generator
// ---------------------------------------------------------------------------

/// The TK_COLUMN case of sqlite3ExprCodeTarget() (also reached by
/// fall-through from TK_AGG_COLUMN).  Always returns the result register.
fn exprCodeColumn(db: &mut Sqlite3, pExpr: ExprId, target: i32) -> i32 {
    let (flags, iTable, iColumn, pLeft, pTab, op2) = {
        let e = db.ex(pExpr);
        (
            e.flags,
            e.iTable,
            e.iColumn as i32,
            e.pLeft,
            e.y.pTab.clone(),
            e.op2,
        )
    };
    let mut iTab = iTable;
    if (flags & EP_FixedCol) != 0 {
        /* This COLUMN expression is really a constant due to WHERE clause
         ** constraints, and that constant is coded by the pExpr->pLeft
         ** expression.  However, make sure the constant has the correct
         ** datatype by applying the Affinity of the table column to the
         ** constant.
         */
        let iReg = sqlite3ExprCodeTarget(db, pLeft, target);
        let pTab = pTab.expect("EP_FixedCol column without table");
        let aff = sqlite3TableColumnAffinity(&pTab, iColumn);
        if aff > SQLITE_AFF_BLOB {
            sqlite3VdbeAddOp4(
                db.v(),
                OP_Affinity,
                iReg,
                1,
                0,
                P4::Z(vec![aff]),
                P4_STATIC as i32,
            );
        }
        return iReg;
    }
    if iTab < 0 {
        let iSelfTab = db.ppr().iSelfTab;
        if iSelfTab < 0 {
            /* Other columns in the same row for CHECK constraints or
             ** generated columns or for inserting into partial index.
             ** The row is unpacked into registers beginning at
             ** 0-(pParse->iSelfTab).  The rowid (if any) is in a register
             ** immediately prior to the first column.
             */
            let iCol = iColumn;
            let pTab = pTab.expect("column without table");
            debug_assert!(iCol >= XN_ROWID as i32);
            if iCol < 0 {
                return -1 - iSelfTab;
            }
            let iSrc =
                crate::build::sqlite3TableColumnToStorage(&pTab, iCol as i16) as i32 - iSelfTab;
            let (colFlags, affinity, zCnName) = {
                let t = pTab.borrow();
                let c = &t.aCol[iCol as usize];
                (c.colFlags, c.affinity, c.zCnName.clone())
            };
            if (colFlags & COLFLAG_GENERATED) != 0 {
                if (colFlags & COLFLAG_BUSY) != 0 {
                    errmsg!(db, "generated column loop on \"%s\"", &zCnName);
                    return 0;
                }
                pTab.borrow_mut().aCol[iCol as usize].colFlags |= COLFLAG_BUSY;
                if (colFlags & COLFLAG_NOTAVAIL) != 0 {
                    sqlite3ExprCodeGeneratedColumn(db, &pTab, iCol as usize, iSrc);
                }
                pTab.borrow_mut().aCol[iCol as usize].colFlags &=
                    !(COLFLAG_BUSY | COLFLAG_NOTAVAIL);
                return iSrc;
            } else if affinity == SQLITE_AFF_REAL {
                sqlite3VdbeAddOp2(db.v(), OP_SCopy, iSrc, target);
                sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, target);
                return target;
            } else {
                return iSrc;
            }
        } else {
            /* Coding an expression that is part of an index where column names
             ** in the index refer to the table to which the index belongs */
            iTab = iSelfTab - 1;
        }
    } else if !db.ppr().pIdxPartExpr.is_empty() {
        let r1 = exprPartidxExprLookup(db, pExpr, target);
        if r1 != 0 {
            return r1;
        }
    }
    let pTab = pTab.expect("column without table");
    sqlite3ExprCodeGetColumn(db, &pTab, iColumn, iTab, target, op2)
}

/// Generate code into the current Vdbe to evaluate the given
/// expression.  Attempt to store the results in register "target".
/// Return the register where results are stored.
///
/// With this routine, there is no guarantee that results will
/// be stored in target.  The result might be stored in some other
/// register if it is convenient to do so.  The calling function
/// must check the return code and move the results to the desired
/// register.
pub fn sqlite3ExprCodeTarget(db: &mut Sqlite3, pExpr: ExprId, target: i32) -> i32 {
    let mut pExpr = pExpr;
    let mut inReg = target; /* Results stored in register inReg */
    let mut regFree1 = 0; /* If non-zero free this temporary register */
    let mut regFree2 = 0; /* If non-zero free this temporary register */
    let mut r1 = 0;
    let mut r2 = 0;
    let mut p5: u16 = 0;

    debug_assert!(target > 0 && target <= db.ppr().nMem);
    debug_assert!(db.ppr().pVdbe.is_some());

    /* expr_code_doover: */
    loop {
        let mut op;
        if pExpr.is_null() {
            op = TK_NULL;
        } else if !db.ppr().pIdxEpr.is_empty() && !hp(db, pExpr, EP_Leaf) {
            let r = sqlite3IndexedExprLookup(db, pExpr, target);
            if r >= 0 {
                return r;
            }
            op = db.ex(pExpr).op;
        } else {
            op = db.ex(pExpr).op;
        }
        debug_assert!(op != TK_ORDER);
        match op {
            TK_AGG_COLUMN => {
                let (pAggInfo, iAgg, hasTab, iTable, iColumn) = {
                    let e = db.ex(pExpr);
                    (
                        e.pAggInfo,
                        e.iAgg as i32,
                        e.y.pTab.is_some(),
                        e.iTable,
                        e.iColumn as i32,
                    )
                };
                debug_assert!(pAggInfo.nn());
                debug_assert!(iAgg >= 0);
                let (nColumn, directMode, useSortingIdx, sortingIdxPTab) = {
                    let a = db.arena.agg(pAggInfo);
                    (a.nColumn(), a.directMode, a.useSortingIdx, a.sortingIdxPTab)
                };
                if iAgg >= nColumn {
                    /* Happens when the left table of a RIGHT JOIN is null and
                     ** is using an expression index */
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
                    break;
                }
                if directMode == 0 {
                    return db.arena.agg(pAggInfo).AggInfoColumnReg(iAgg);
                } else if useSortingIdx != 0 {
                    let (pTab, iSorterColumn, colIColumn) = {
                        let c = &db.arena.agg(pAggInfo).aCol[iAgg as usize];
                        (c.pTab.clone(), c.iSorterColumn, c.iColumn)
                    };
                    sqlite3VdbeAddOp3(db.v(), OP_Column, sortingIdxPTab, iSorterColumn, target);
                    if let Some(t) = pTab {
                        if colIColumn >= 0 {
                            let aff = t.borrow().aCol[colIColumn as usize].affinity;
                            if aff == SQLITE_AFF_REAL {
                                sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, target);
                            }
                        }
                    }
                    return target;
                } else if !hasTab {
                    /* This case happens when the argument to an aggregate function
                     ** is rewritten by aggregateConvertIndexedExprRefToColumn() */
                    sqlite3VdbeAddOp3(db.v(), OP_Column, iTable, iColumn, target);
                    return target;
                }
                /* Otherwise, fall thru into the TK_COLUMN case */
                return exprCodeColumn(db, pExpr, target);
            }
            TK_COLUMN => {
                return exprCodeColumn(db, pExpr, target);
            }
            TK_INTEGER => {
                codeInteger(db, pExpr, false, target);
                return target;
            }
            TK_TRUEFALSE => {
                let t = truthValue(db, pExpr);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, t, target);
                return target;
            }
            TK_FLOAT => {
                debug_assert!(!hp(db, pExpr, EP_IntValue));
                let z = db.ex(pExpr).u.zToken.clone();
                codeReal(db.v(), z.as_deref(), false, target);
                return target;
            }
            TK_STRING => {
                debug_assert!(!hp(db, pExpr, EP_IntValue));
                let z = db.ex(pExpr).token().to_vec();
                sqlite3VdbeLoadString(db.v(), target, &z);
                return target;
            }
            TK_NULLS => {
                /* Set a range of registers to NULL.  pExpr->y.nReg registers starting
                 ** with target */
                let nReg = db.ex(pExpr).y.nReg;
                sqlite3VdbeAddOp3(db.v(), OP_Null, 0, target, target + nReg - 1);
                return target;
            }
            TK_BLOB => {
                debug_assert!(!hp(db, pExpr, EP_IntValue));
                let tok = db.ex(pExpr).token().to_vec();
                debug_assert!(tok[0] == b'x' || tok[0] == b'X');
                debug_assert!(tok[1] == b'\'');
                let z = &tok[2..];
                let n = z.len() as i32 - 1;
                debug_assert!(z[n as usize] == b'\'');
                let zBlob = crate::util::sqlite3HexToBlob(db, Some(z), n).unwrap_or_default();
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_Blob,
                    n / 2,
                    target,
                    0,
                    P4::Z(zBlob),
                    P4_DYNAMIC as i32,
                );
                return target;
            }
            TK_VARIABLE => {
                debug_assert!(!hp(db, pExpr, EP_IntValue));
                let iColumn = db.ex(pExpr).iColumn as i32;
                sqlite3VdbeAddOp2(db.v(), OP_Variable, iColumn, target);
                return target;
            }
            TK_REGISTER => {
                return db.ex(pExpr).iTable;
            }
            TK_CAST => {
                /* Expressions of the form:   CAST(pLeft AS token) */
                let (pLeft, z) = {
                    let e = db.ex(pExpr);
                    (e.pLeft, e.u.zToken.clone())
                };
                sqlite3ExprCode(db, pLeft, target);
                debug_assert!(inReg == target);
                let aff = crate::build::sqlite3AffinityType(z.as_deref(), None);
                sqlite3VdbeAddOp2(db.v(), OP_Cast, target, aff as i32);
                return inReg;
            }
            TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
                if op == TK_IS || op == TK_ISNOT {
                    op = if op == TK_IS { TK_EQ } else { TK_NE };
                    p5 = SQLITE_NULLEQ;
                }
                let (pLeft, pRight, flags) = {
                    let e = db.ex(pExpr);
                    (e.pLeft, e.pRight, e.flags)
                };
                let mut addrIsNull = 0;
                if exprVectorSize(db, pLeft) > 1 {
                    codeVectorCompare(db, pExpr, target, op, p5 as u8);
                } else {
                    if (flags & EP_Subquery) != 0 && p5 != SQLITE_NULLEQ {
                        addrIsNull = exprComputeOperands(
                            db,
                            pExpr,
                            &mut r1,
                            &mut r2,
                            &mut regFree1,
                            &mut regFree2,
                        );
                    } else {
                        r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                        r2 = sqlite3ExprCodeTemp(db, pRight, &mut regFree2);
                    }
                    sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, inReg);
                    let dest = sqlite3VdbeCurrentAddr(db.v()) + 2;
                    codeCompare(
                        db,
                        pLeft,
                        pRight,
                        op,
                        r1,
                        r2,
                        dest,
                        p5 as i32,
                        (flags & EP_Commuted) != 0,
                    );
                    if p5 == SQLITE_NULLEQ {
                        sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, inReg);
                    } else {
                        sqlite3VdbeAddOp3(db.v(), OP_ZeroOrNull, r1, inReg, r2);
                        if addrIsNull != 0 {
                            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
                            sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, a);
                            sqlite3VdbeJumpHere(db.v(), addrIsNull);
                            sqlite3VdbeAddOp2(db.v(), OP_Null, 0, inReg);
                        }
                    }
                }
                break;
            }
            TK_AND | TK_OR => {
                inReg = exprCodeTargetAndOr(db, pExpr, target, &mut regFree1);
                break;
            }
            TK_PLUS | TK_STAR | TK_MINUS | TK_REM | TK_BITAND | TK_BITOR | TK_SLASH | TK_LSHIFT
            | TK_RSHIFT | TK_CONCAT => {
                let addrIsNull;
                let (pLeft, pRight, flags) = {
                    let e = db.ex(pExpr);
                    (e.pLeft, e.pRight, e.flags)
                };
                if (flags & EP_Subquery) != 0 {
                    addrIsNull = exprComputeOperands(
                        db,
                        pExpr,
                        &mut r1,
                        &mut r2,
                        &mut regFree1,
                        &mut regFree2,
                    );
                } else {
                    r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                    r2 = sqlite3ExprCodeTemp(db, pRight, &mut regFree2);
                    addrIsNull = 0;
                }
                sqlite3VdbeAddOp3(db.v(), op, r2, r1, target);
                if addrIsNull != 0 {
                    let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
                    sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, a);
                    sqlite3VdbeJumpHere(db.v(), addrIsNull);
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
                }
                break;
            }
            TK_UMINUS => {
                let pLeft = db.ex(pExpr).pLeft;
                debug_assert!(pLeft.nn());
                let lop = db.ex(pLeft).op;
                if lop == TK_INTEGER {
                    codeInteger(db, pLeft, true, target);
                    return target;
                } else if lop == TK_FLOAT {
                    let z = db.ex(pLeft).u.zToken.clone();
                    codeReal(db.v(), z.as_deref(), true, target);
                    return target;
                } else {
                    let tempX = db.arena.new_expr(Expr {
                        op: TK_INTEGER,
                        flags: EP_IntValue | EP_TokenOnly,
                        ..Expr::default()
                    });
                    r1 = sqlite3ExprCodeTemp(db, tempX, &mut regFree1);
                    r2 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree2);
                    sqlite3VdbeAddOp3(db.v(), OP_Subtract, r2, r1, target);
                }
                break;
            }
            TK_BITNOT | TK_NOT => {
                let pLeft = db.ex(pExpr).pLeft;
                r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                sqlite3VdbeAddOp2(db.v(), op, r1, inReg);
                break;
            }
            TK_TRUTH => {
                let (pLeft, pRight, op2) = {
                    let e = db.ex(pExpr);
                    (e.pLeft, e.pRight, e.op2)
                };
                r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                let isTrue = truthValue(db, pRight); /* IS TRUE or IS NOT TRUE */
                let bNormal = (op2 == TK_IS) as i32; /* IS TRUE or IS FALSE */
                sqlite3VdbeAddOp4Int(
                    db.v(),
                    OP_IsTrue,
                    r1,
                    inReg,
                    (isTrue == 0) as i32,
                    isTrue ^ bNormal,
                );
                break;
            }
            TK_ISNULL | TK_NOTNULL => {
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, target);
                let pLeft = db.ex(pExpr).pLeft;
                r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                let addr = sqlite3VdbeAddOp1(db.v(), op, r1);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, target);
                sqlite3VdbeJumpHere(db.v(), addr);
                break;
            }
            TK_AGG_FUNCTION => {
                let (pInfo, iAgg) = {
                    let e = db.ex(pExpr);
                    (e.pAggInfo, e.iAgg as i32)
                };
                if pInfo.is_null() || iAgg < 0 || iAgg >= db.arena.agg(pInfo).nFunc() {
                    debug_assert!(!hp(db, pExpr, EP_IntValue));
                    errmsg!(db, "misuse of aggregate: %#T()", pExpr);
                } else {
                    return db.arena.agg(pInfo).AggInfoFuncReg(iAgg);
                }
                break;
            }
            TK_FUNCTION => {
                let mut constMask: u32 = 0; /* Mask of function arguments that are constant */
                let enc = db.ENC(); /* The text encoding used by this database */
                let mut pColl: Option<CollSeqRef> = None; /* A collating sequence */
                let (flags, pWin, pFarg, zId, op2) = {
                    let e = db.ex(pExpr);
                    (e.flags, e.y.pWin, e.x.pList, e.u.zToken.clone(), e.op2)
                };

                if (flags & EP_WinFunc) != 0 {
                    return db.win(pWin).regResult;
                }

                if db.ppr().ConstFactorOk() && sqlite3ExprIsConstantNotJoin(db, pExpr) != 0 {
                    /* SQL functions can be expensive. So try to avoid running them
                     ** multiple times if we know they always give the same result */
                    return sqlite3ExprCodeRunJustOnce(db, pExpr, -1);
                }
                debug_assert!((flags & EP_TokenOnly) == 0);
                let nFarg = el_n(db, pFarg);
                debug_assert!((flags & EP_IntValue) == 0);
                let pDef0 = crate::callback::sqlite3FindFunction(db, zId.as_deref(), nFarg, enc, 0);
                let mut pDef = match pDef0 {
                    Some(d)
                        if d.xFinalize.is_none()
                            && !((d.funcFlags & SQLITE_FUNC_INTERNAL) != 0
                                && db.ppr().nested == 0
                                && (db.mDbFlags & DBFLAG_InternalFunc) == 0) =>
                    {
                        d
                    }
                    _ => {
                        errmsg!(db, "unknown function: %#T()", pExpr);
                        break;
                    }
                };
                if (pDef.funcFlags & SQLITE_FUNC_INLINE) != 0 && pFarg.nn() {
                    debug_assert!((pDef.funcFlags & SQLITE_FUNC_UNSAFE) == 0);
                    debug_assert!((pDef.funcFlags & SQLITE_FUNC_DIRECT) == 0);
                    let iFuncId = pDef.pUserData.as_int() as i32;
                    return exprCodeInlineFunction(db, pFarg, iFuncId, target);
                } else if (pDef.funcFlags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0 {
                    sqlite3ExprFunctionUsable(db, pExpr, &pDef);
                }

                for i in 0..nFarg {
                    let a = el_e(db, pFarg, i);
                    if i < 32 && sqlite3ExprIsConstant(db, true, a) != 0 {
                        constMask |= MASKBIT32(i);
                    }
                    if (pDef.funcFlags & SQLITE_FUNC_NEEDCOLL) != 0 && pColl.is_none() {
                        pColl = sqlite3ExprCollSeq(db, a);
                    }
                }
                if pFarg.nn() {
                    if constMask != 0 {
                        r1 = db.ppr().nMem + 1;
                        db.pp().nMem += nFarg;
                    } else {
                        r1 = sqlite3GetTempRange(db, nFarg);
                    }

                    /* For length() and typeof() and octet_length() functions,
                     ** set the P5 parameter to the OP_Column opcode to OPFLAG_LENGTHARG
                     ** or OPFLAG_TYPEOFARG or OPFLAG_BYTELENARG respectively, to avoid
                     ** unnecessary data loading.
                     */
                    if (pDef.funcFlags & (SQLITE_FUNC_LENGTH | SQLITE_FUNC_TYPEOF)) != 0 {
                        debug_assert!(nFarg == 1);
                        let a0 = el_e(db, pFarg, 0);
                        let exprOp = db.ex(a0).op;
                        if exprOp == TK_COLUMN || exprOp == TK_AGG_COLUMN {
                            db.exm(a0).op2 = (pDef.funcFlags & OPFLAG_BYTELENARG as u32) as u8;
                        }
                    }

                    sqlite3ExprCodeExprList(db, pFarg, r1, 0, SQLITE_ECEL_FACTOR);
                } else {
                    r1 = 0;
                }
                /* Possibly overload the function if the first argument is
                 ** a virtual table column.
                 **
                 ** For infix functions (LIKE, GLOB, REGEXP, and MATCH) use the
                 ** second argument, not the first, as the argument to test to
                 ** see if it is a column in a virtual table.  This is done because
                 ** the left operand of infix functions (the operand we want to
                 ** control overloading) ends up as the second argument to the
                 ** function.  The expression "A glob B" is equivalent to
                 ** "glob(B,A).  We want to use the A in "A glob B" to test
                 ** for function overloading.  But we use the B term in "glob(B,A)".
                 */
                if nFarg >= 2 && (flags & EP_InfixFunc) != 0 {
                    let a1 = el_e(db, pFarg, 1);
                    if let Some(d) = crate::vtab::sqlite3VtabOverloadFunction(db, &pDef, nFarg, a1)
                    {
                        pDef = d;
                    }
                } else if nFarg > 0 {
                    let a0 = el_e(db, pFarg, 0);
                    if let Some(d) = crate::vtab::sqlite3VtabOverloadFunction(db, &pDef, nFarg, a0)
                    {
                        pDef = d;
                    }
                }
                if (pDef.funcFlags & SQLITE_FUNC_NEEDCOLL) != 0 {
                    if pColl.is_none() {
                        pColl = db.pDfltColl.clone();
                    }
                    add_op4_coll(db, OP_CollSeq, 0, 0, 0, pColl);
                }
                sqlite3VdbeAddFunctionCall(
                    db,
                    constMask as i32,
                    r1,
                    target,
                    nFarg,
                    &pDef,
                    op2 as i32,
                );
                if nFarg != 0 && constMask == 0 {
                    sqlite3ReleaseTempRange(db, r1, nFarg);
                }
                return target;
            }
            TK_EXISTS | TK_SELECT => {
                if db.mallocFailed != 0 {
                    return 0;
                }
                let nCol = if op == TK_SELECT {
                    el_n(db, db.sel(db.ex(pExpr).x.pSelect).pEList)
                } else {
                    1
                };
                if op == TK_SELECT && nCol != 1 {
                    sqlite3SubselectError(db, nCol, 1);
                } else {
                    return sqlite3CodeSubselect(db, pExpr);
                }
                break;
            }
            TK_SELECT_COLUMN => {
                let (pLeft, iTable, iColumn) = {
                    let e = db.ex(pExpr);
                    (e.pLeft, e.iTable, e.iColumn as i32)
                };
                let withinRJSubrtn = db.ppr().withinRJSubrtn;
                if db.ex(pLeft).iTable == 0 || withinRJSubrtn > db.ex(pLeft).op2 {
                    let r = sqlite3CodeSubselect(db, pLeft);
                    let l = db.exm(pLeft);
                    l.iTable = r;
                    l.op2 = withinRJSubrtn;
                }
                debug_assert!(db.ex(pLeft).op == TK_SELECT || db.ex(pLeft).op == TK_ERROR);
                let n = exprVectorSize(db, pLeft);
                if iTable != n {
                    errmsg!(db, "%d columns assigned %d values", iTable, n);
                }
                return db.ex(pLeft).iTable + iColumn;
            }
            TK_IN => {
                let destIfFalse = sqlite3VdbeMakeLabel(db);
                let destIfNull = sqlite3VdbeMakeLabel(db);
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
                sqlite3ExprCodeIN(db, pExpr, destIfFalse, destIfNull);
                sqlite3VdbeAddOp2(db.v(), OP_Integer, 1, target);
                sqlite3VdbeResolveLabel(db.v(), destIfFalse);
                sqlite3VdbeAddOp2(db.v(), OP_AddImm, target, 0);
                sqlite3VdbeResolveLabel(db.v(), destIfNull);
                return target;
            }

            /*
             **    x BETWEEN y AND z
             **
             ** This is equivalent to
             **
             **    x>=y AND x<=z
             **
             ** X is stored in pExpr->pLeft.
             ** Y is stored in pExpr->pList->a[0].pExpr.
             ** Z is stored in pExpr->pList->a[1].pExpr.
             */
            TK_BETWEEN => {
                exprCodeBetween(db, pExpr, target, None, 0);
                return target;
            }
            TK_COLLATE => {
                let (flags, pLeft) = {
                    let e = db.ex(pExpr);
                    (e.flags, e.pLeft)
                };
                if (flags & EP_Collate) == 0 {
                    /* A TK_COLLATE Expr node without the EP_Collate tag is a so-called
                     ** "SOFT-COLLATE" that is added to constraints that are pushed down
                     ** from outer queries into sub-queries by the WHERE-clause push-down
                     ** optimization. Clear subtypes as subtypes may not cross a subquery
                     ** boundary.
                     */
                    debug_assert!(pLeft.nn());
                    sqlite3ExprCode(db, pLeft, target);
                    sqlite3VdbeAddOp1(db.v(), OP_ClrSubtype, target);
                    return target;
                } else {
                    pExpr = pLeft;
                    continue; /* 2018-04-28: Prevent deep recursion. */
                }
            }
            TK_SPAN | TK_UPLUS => {
                pExpr = db.ex(pExpr).pLeft;
                continue; /* 2018-04-28: Prevent deep recursion. OSSFuzz. */
            }

            TK_TRIGGER => {
                /* If the opcode is TK_TRIGGER, then the expression is a reference
                 ** to a column in the new.* or old.* pseudo-tables available to
                 ** trigger programs. In this case Expr.iTable is set to 1 for the
                 ** new.* pseudo-table, or 0 for the old.* pseudo-table. Expr.iColumn
                 ** is set to the column of the pseudo-table to read, or to -1 to
                 ** read the rowid field.
                 **
                 ** The expression is implemented using an OP_Param opcode. The p1
                 ** parameter is set to 0 for an old.rowid reference, or to (i+1)
                 ** to reference another column of the old.* pseudo-table, where
                 ** i is the index of the column. For a new.rowid reference, p1 is
                 ** set to (n+1), where n is the number of columns in each pseudo-table.
                 ** For a reference to any other column in the new.* pseudo-table, p1
                 ** is set to (n+2+i), where n and i are as defined previously. For
                 ** example, if the table on which triggers are being fired is
                 ** declared as:
                 **
                 **   CREATE TABLE t1(a, b);
                 **
                 ** Then p1 is interpreted as follows:
                 **
                 **   p1==0   ->    old.rowid     p1==3   ->    new.rowid
                 **   p1==1   ->    old.a         p1==4   ->    new.a
                 **   p1==2   ->    old.b         p1==5   ->    new.b
                 */
                let (pTab, iCol, iTable) = {
                    let e = db.ex(pExpr);
                    (
                        e.y.pTab.clone().expect("TK_TRIGGER without table"),
                        e.iColumn,
                        e.iTable,
                    )
                };
                let nCol = pTab.borrow().nCol as i32;
                let p1 = iTable * (nCol + 1)
                    + 1
                    + crate::build::sqlite3TableColumnToStorage(&pTab, iCol) as i32;

                debug_assert!(iTable == 0 || iTable == 1);
                debug_assert!(iCol >= -1 && (iCol as i32) < nCol);
                debug_assert!(p1 >= 0 && p1 < (nCol * 2 + 2));

                sqlite3VdbeAddOp2(db.v(), OP_Param, p1, target);

                /* If the column has REAL affinity, it may currently be stored as an
                 ** integer. Use OP_RealAffinity to make sure it is really real.
                 **
                 ** EVIDENCE-OF: R-60985-57662 SQLite will convert the value back to
                 ** floating point when extracting it from the record.  */
                if iCol >= 0 && pTab.borrow().aCol[iCol as usize].affinity == SQLITE_AFF_REAL {
                    sqlite3VdbeAddOp1(db.v(), OP_RealAffinity, target);
                }
                break;
            }

            TK_VECTOR => {
                errmsg!(db, "row value misused");
                break;
            }

            /* TK_IF_NULL_ROW Expr nodes are inserted ahead of expressions
             ** that derive from the right-hand table of a LEFT JOIN.  The
             ** Expr.iTable value is the table number for the right-hand table.
             ** The expression is only evaluated if that table is not currently
             ** on a LEFT JOIN NULL row.
             */
            TK_IF_NULL_ROW => {
                let okConstFactor = db.ppr().okConstFactor;
                let (pAggInfo, iAgg, iTable, pLeft) = {
                    let e = db.ex(pExpr);
                    (e.pAggInfo, e.iAgg as i32, e.iTable, e.pLeft)
                };
                if pAggInfo.nn() {
                    let (directMode, useSortingIdx, sortingIdxPTab) = {
                        let a = db.arena.agg(pAggInfo);
                        debug_assert!(iAgg >= 0 && iAgg < a.nColumn());
                        (a.directMode, a.useSortingIdx, a.sortingIdxPTab)
                    };
                    if directMode == 0 {
                        inReg = db.arena.agg(pAggInfo).AggInfoColumnReg(iAgg);
                        break;
                    }
                    if useSortingIdx != 0 {
                        let iSorterColumn =
                            db.arena.agg(pAggInfo).aCol[iAgg as usize].iSorterColumn;
                        sqlite3VdbeAddOp3(db.v(), OP_Column, sortingIdxPTab, iSorterColumn, target);
                        inReg = target;
                        break;
                    }
                }
                let addrINR = sqlite3VdbeAddOp3(db.v(), OP_IfNullRow, iTable, 0, target);
                /* The OP_IfNullRow opcode above can overwrite the result register with
                 ** NULL.  So we have to ensure that the result register is not a value
                 ** that is suppose to be a constant.  Two defenses are needed:
                 **   (1)  Temporarily disable factoring of constant expressions
                 **   (2)  Make sure the computed value really is stored in register
                 **        "target" and not someplace else.
                 */
                db.pp().okConstFactor = 0; /* note (1) above */
                sqlite3ExprCode(db, pLeft, target);
                debug_assert!(target == inReg);
                db.pp().okConstFactor = okConstFactor;
                sqlite3VdbeJumpHere(db.v(), addrINR);
                break;
            }

            /*
             ** Form A:
             **   CASE x WHEN e1 THEN r1 WHEN e2 THEN r2 ... WHEN eN THEN rN ELSE y END
             **
             ** Form B:
             **   CASE WHEN e1 THEN r1 WHEN e2 THEN r2 ... WHEN eN THEN rN ELSE y END
             **
             ** Form A is can be transformed into the equivalent form B as follows:
             **   CASE WHEN x=e1 THEN r1 WHEN x=e2 THEN r2 ...
             **        WHEN x=eN THEN rN ELSE y END
             **
             ** X (if it exists) is in pExpr->pLeft.
             ** Y is in the last element of pExpr->x.pList if pExpr->x.pList->nExpr is
             ** odd.  The Y is also optional.  If the number of elements in x.pList
             ** is even, then Y is omitted and the "otherwise" result is NULL.
             ** Ei is in pExpr->pList->a[i*2] and Ri is pExpr->pList->a[i*2+1].
             **
             ** The result of the expression is the Ri for the first matching Ei,
             ** or if there is no matching Ei, the ELSE term Y, or if there is
             ** no ELSE term, NULL.
             */
            TK_CASE => {
                let (pEList, pX) = {
                    let e = db.ex(pExpr);
                    (e.x.pList, e.pLeft)
                };
                debug_assert!(pEList.nn());
                let nExpr = el_n(db, pEList);
                debug_assert!(nExpr > 0);
                let endLabel = sqlite3VdbeMakeLabel(db);
                let mut pDel = ExprId::NULL;
                let mut opCompare = ExprId::NULL; /* The X==Ei expression */
                let mut pTest = ExprId::NULL; /* X==Ei (form A) or just Ei (form B) */
                if pX.nn() {
                    pDel = sqlite3ExprDup(db, pX, 0);
                    if db.mallocFailed != 0 {
                        sqlite3ExprDelete(db, pDel);
                        break;
                    }
                    let r = exprCodeVector(db, pDel, &mut regFree1);
                    sqlite3ExprToRegister(db, pDel, r);
                    opCompare = tmp_expr(db, TK_EQ);
                    db.exm(opCompare).pLeft = pDel;
                    pTest = opCompare;
                    /* Ticket b351d95f9cd5ef17e9d9dbae18f5ca8611190001:
                     ** The value in regFree1 might get SCopy-ed into the file result.
                     ** So make sure that the regFree1 register is not reused for other
                     ** purposes and possibly overwritten.  */
                    regFree1 = 0;
                }
                let mut i = 0;
                while i < nExpr - 1 {
                    if pX.nn() {
                        debug_assert!(pTest.nn());
                        let ai = el_e(db, pEList, i);
                        db.exm(opCompare).pRight = ai;
                    } else {
                        pTest = el_e(db, pEList, i);
                    }
                    let nextCase = sqlite3VdbeMakeLabel(db);
                    sqlite3ExprIfFalse(db, pTest, nextCase, SQLITE_JUMPIFNULL as i32);
                    let r = el_e(db, pEList, i + 1);
                    sqlite3ExprCode(db, r, target);
                    sqlite3VdbeGoto(db.v(), endLabel);
                    sqlite3VdbeResolveLabel(db.v(), nextCase);
                    i += 2;
                }
                if (nExpr & 1) != 0 {
                    let e = el_e(db, pEList, nExpr - 1);
                    sqlite3ExprCode(db, e, target);
                } else {
                    sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
                }
                sqlite3ExprDelete(db, pDel);
                setDoNotMergeFlagOnCopy(db.v());
                sqlite3VdbeResolveLabel(db.v(), endLabel);
                break;
            }
            TK_RAISE => {
                let (affExpr, pLeft) = {
                    let e = db.ex(pExpr);
                    (e.affExpr, e.pLeft)
                };
                debug_assert!(
                    affExpr == OE_Rollback
                        || affExpr == OE_Abort
                        || affExpr == OE_Fail
                        || affExpr == OE_Ignore
                );
                let hasTriggerTab = db.ppr().pTriggerTab.is_some();
                if !hasTriggerTab && db.ppr().nested == 0 {
                    errmsg!(db, "RAISE() may only be used within a trigger-program");
                    return 0;
                }
                if affExpr == OE_Abort {
                    crate::build::sqlite3MayAbort(db);
                }
                debug_assert!(!hp(db, pExpr, EP_IntValue));
                if affExpr == OE_Ignore {
                    sqlite3VdbeAddOp2(db.v(), OP_Halt, SQLITE_OK, OE_Ignore as i32);
                } else {
                    r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                    let rc = if hasTriggerTab {
                        SQLITE_CONSTRAINT_TRIGGER
                    } else {
                        SQLITE_ERROR
                    };
                    sqlite3VdbeAddOp3(db.v(), OP_Halt, rc, affExpr as i32, r1);
                }
                break;
            }
            _ => {
                /* Make NULL the default case so that if a bug causes an illegal
                 ** Expr node to be passed into this function, it will be handled
                 ** sanely and not crash.  But keep the assert() to bring the problem
                 ** to the attention of the developers. */
                debug_assert!(op == TK_NULL || op == TK_ERROR || db.mallocFailed != 0);
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, target);
                return target;
            }
        }
    }
    sqlite3ReleaseTempReg(db, regFree1);
    sqlite3ReleaseTempReg(db, regFree2);
    inReg
}

/// Generate code that will evaluate expression pExpr just one time
/// per prepared statement execution.
///
/// If the expression uses functions (that might throw an exception) then
/// guard them with an OP_Once opcode to ensure that the code is only executed
/// once. If no functions are involved, then factor the code out and put it at
/// the end of the prepared statement in the initialization section.
///
/// If regDest>0 then the result is always stored in that register and the
/// result is not reusable.  If regDest<0 then this routine is free to
/// store the value wherever it wants.  The register where the expression
/// is stored is returned.  When regDest<0, two identical expressions might
/// code to the same register, if they do not contain function calls and hence
/// are factored out into the initialization section at the end of the
/// prepared statement.
pub fn sqlite3ExprCodeRunJustOnce(db: &mut Sqlite3, pExpr: ExprId, regDest: i32) -> i32 {
    let mut regDest = regDest;
    debug_assert!(db.ppr().ConstFactorOk());
    debug_assert!(regDest != 0);
    let p = db.ppr().pConstExpr;
    if regDest < 0 && p.nn() {
        let items: Vec<(u8, ExprId, i32)> = db
            .el(p)
            .a
            .iter()
            .map(|it| (it.fg.reusable, it.pExpr, it.u.iConstExprReg))
            .collect();
        for (reusable, e, reg) in items {
            if reusable != 0 && sqlite3ExprCompare(db, false, e, pExpr, -1) == 0 {
                return reg;
            }
        }
    }
    let pExpr = sqlite3ExprDup(db, pExpr, 0);
    if pExpr.nn() && hp(db, pExpr, EP_HasFunc) {
        let addr = sqlite3VdbeAddOp0(db.v(), OP_Once);
        db.pp().okConstFactor = 0;
        if db.mallocFailed == 0 {
            if regDest < 0 {
                db.pp().nMem += 1;
                regDest = db.ppr().nMem;
            }
            sqlite3ExprCode(db, pExpr, regDest);
        }
        db.pp().okConstFactor = 1;
        sqlite3ExprDelete(db, pExpr);
        sqlite3VdbeJumpHere(db.v(), addr);
    } else {
        let p = sqlite3ExprListAppend(db, p, pExpr);
        if p.nn() {
            let reusable = (regDest < 0) as u8;
            if regDest < 0 {
                db.pp().nMem += 1;
                regDest = db.ppr().nMem;
            }
            let pItem = db.elm(p).a.last_mut().unwrap();
            pItem.fg.reusable = reusable;
            pItem.u.iConstExprReg = regDest;
        }
        db.pp().pConstExpr = p;
    }
    regDest
}

/// Make arrangements to invoke OP_Null on a range of registers
/// during initialization.
pub fn sqlite3ExprNullRegisterRange(db: &mut Sqlite3, iReg: i32, nReg: i32) {
    let okConstFactor = db.ppr().okConstFactor;
    let t = tmp_expr(db, TK_NULLS);
    db.exm(t).y.nReg = nReg;
    db.pp().okConstFactor = 1;
    sqlite3ExprCodeRunJustOnce(db, t, iReg);
    db.pp().okConstFactor = okConstFactor;
}

/// Generate code to evaluate an expression and store the results
/// into a register.  Return the register number where the results
/// are stored.
///
/// If the register is a temporary register that can be deallocated,
/// then write its number into *pReg.  If the result register is not
/// a temporary, then set *pReg to zero.
///
/// If pExpr is a constant, then this routine might generate this
/// code to fill the register in the initialization section of the
/// VDBE program, in order to factor it out of the evaluation loop.
pub fn sqlite3ExprCodeTemp(db: &mut Sqlite3, pExpr: ExprId, pReg: &mut i32) -> i32 {
    let r2;
    let pExpr = skipCollateAndLikely(db, pExpr);
    if db.ppr().ConstFactorOk()
        && pExpr.nn()
        && db.ex(pExpr).op != TK_REGISTER
        && sqlite3ExprIsConstantNotJoin(db, pExpr) != 0
    {
        *pReg = 0;
        r2 = sqlite3ExprCodeRunJustOnce(db, pExpr, -1);
    } else {
        let r1 = sqlite3GetTempReg(db);
        r2 = sqlite3ExprCodeTarget(db, pExpr, r1);
        if r2 == r1 {
            *pReg = r1;
        } else {
            sqlite3ReleaseTempReg(db, r1);
            *pReg = 0;
        }
    }
    r2
}

/// Generate code that will evaluate expression pExpr and store the
/// results in register target.  The results are guaranteed to appear
/// in register target.
pub fn sqlite3ExprCode(db: &mut Sqlite3, pExpr: ExprId, target: i32) {
    debug_assert!(target > 0 && target <= db.ppr().nMem);
    if db.ppr().pVdbe.is_none() {
        return;
    }
    let inReg = sqlite3ExprCodeTarget(db, pExpr, target);
    if inReg != target {
        let pX = skipCollateAndLikely(db, pExpr);
        let op = if pX.nn() && (hp(db, pX, EP_Subquery) || db.ex(pX).op == TK_REGISTER) {
            OP_Copy
        } else {
            OP_SCopy
        };
        sqlite3VdbeAddOp2(db.v(), op, inReg, target);
    }
}

/// Make a transient copy of expression pExpr and then code it using
/// sqlite3ExprCode().  This routine works just like sqlite3ExprCode()
/// except that the input expression is guaranteed to be unchanged.
pub fn sqlite3ExprCodeCopy(db: &mut Sqlite3, pExpr: ExprId, target: i32) {
    let pExpr = sqlite3ExprDup(db, pExpr, 0);
    if db.mallocFailed == 0 {
        sqlite3ExprCode(db, pExpr, target);
    }
    sqlite3ExprDelete(db, pExpr);
}

/// Generate code that will evaluate expression pExpr and store the
/// results in register target.  The results are guaranteed to appear
/// in register target.  If the expression is constant, then this routine
/// might choose to code the expression at initialization time.
pub fn sqlite3ExprCodeFactorable(db: &mut Sqlite3, pExpr: ExprId, target: i32) {
    if db.ppr().okConstFactor != 0 && sqlite3ExprIsConstantNotJoin(db, pExpr) != 0 {
        sqlite3ExprCodeRunJustOnce(db, pExpr, target);
    } else {
        sqlite3ExprCodeCopy(db, pExpr, target);
    }
}

/// Generate code that pushes the value of every element of the given
/// expression list into a sequence of registers beginning at target.
///
/// Return the number of elements evaluated.  The number returned will
/// usually be pList->nExpr but might be reduced if SQLITE_ECEL_OMITREF
/// is defined.
///
/// The SQLITE_ECEL_DUP flag prevents the arguments from being
/// filled using OP_SCopy.  OP_Copy must be used instead.
///
/// The SQLITE_ECEL_FACTOR argument allows constant arguments to be
/// factored out into initialization code.
///
/// The SQLITE_ECEL_REF flag means that expressions in the list with
/// ExprList.a[].u.x.iOrderByCol>0 have already been evaluated and stored
/// in registers at srcReg, and so the value can be copied from there.
/// If SQLITE_ECEL_OMITREF is also set, then the values with u.x.iOrderByCol>0
/// are simply omitted rather than being copied from srcReg.
pub fn sqlite3ExprCodeExprList(
    db: &mut Sqlite3,
    pList: ExprListId,
    target: i32,
    srcReg: i32,
    flags: u8,
) -> i32 {
    let mut flags = flags;
    let copyOp = if (flags & SQLITE_ECEL_DUP) != 0 {
        OP_Copy
    } else {
        OP_SCopy
    };
    debug_assert!(pList.nn());
    debug_assert!(target > 0);
    debug_assert!(db.ppr().pVdbe.is_some()); /* Never gets this far otherwise */
    let nExpr = el_n(db, pList);
    let mut n = nExpr;
    if !db.ppr().ConstFactorOk() {
        flags &= !SQLITE_ECEL_FACTOR;
    }
    let mut i = 0;
    for k in 0..nExpr as usize {
        let (pExpr, j) = {
            let it = &db.el(pList).a[k];
            (it.pExpr, it.u.x.iOrderByCol as i32)
        };
        if (flags & SQLITE_ECEL_REF) != 0 && j > 0 {
            if (flags & SQLITE_ECEL_OMITREF) != 0 {
                n -= 1;
                continue;
            } else {
                sqlite3VdbeAddOp2(db.v(), copyOp, j + srcReg - 1, target + i);
            }
        } else if (flags & SQLITE_ECEL_FACTOR) != 0 && sqlite3ExprIsConstantNotJoin(db, pExpr) != 0
        {
            sqlite3ExprCodeRunJustOnce(db, pExpr, target + i);
        } else {
            let inReg = sqlite3ExprCodeTarget(db, pExpr, target + i);
            if inReg != target + i {
                let mut merged = false;
                if copyOp == OP_Copy {
                    if let Some(pOp) = sqlite3VdbeGetLastOp(db.v()) {
                        if pOp.opcode == OP_Copy
                            && pOp.p1 + pOp.p3 + 1 == inReg
                            && pOp.p2 + pOp.p3 + 1 == target + i
                            && pOp.p5 == 0
                        /* The do-not-merge flag must be clear */
                        {
                            pOp.p3 += 1;
                            merged = true;
                        }
                    }
                }
                if !merged {
                    sqlite3VdbeAddOp2(db.v(), copyOp, inReg, target + i);
                }
            }
        }
        i += 1;
    }
    n
}

/// The type of the xJump argument of exprCodeBetween()
type XJump = fn(&mut Sqlite3, ExprId, i32, i32);

/// Generate code for a BETWEEN operator.
///
///    x BETWEEN y AND z
///
/// The above is equivalent to
///
///    x>=y AND x<=z
///
/// Code it as such, taking care to do the common subexpression
/// elimination of x.
///
/// The xJumpIf parameter determines details:
///
///    NULL:                   Store the boolean result in reg[dest]
///    sqlite3ExprIfTrue:      Jump to dest if true
///    sqlite3ExprIfFalse:     Jump to dest if false
///
/// The jumpIfNull parameter is ignored if xJumpIf is NULL.
fn exprCodeBetween(
    db: &mut Sqlite3,
    pExpr: ExprId,
    dest: i32,
    xJump: Option<XJump>,
    jumpIfNull: i32,
) {
    let mut regFree1 = 0; /* Temporary use register */
    debug_assert!(db.ex(pExpr).UseXList());
    let (pLeft, pList) = {
        let e = db.ex(pExpr);
        (e.pLeft, e.x.pList)
    };
    let pDel = sqlite3ExprDup(db, pLeft, 0);
    if db.mallocFailed == 0 {
        let exprAnd = tmp_expr(db, TK_AND); /* The AND operator in  x>=y AND x<=z  */
        let compLeft = tmp_expr(db, TK_GE); /* The  x>=y  term */
        let compRight = tmp_expr(db, TK_LE); /* The  x<=z  term */
        let (a0, a1) = (el_e(db, pList, 0), el_e(db, pList, 1));
        {
            let e = db.exm(exprAnd);
            e.pLeft = compLeft;
            e.pRight = compRight;
        }
        {
            let e = db.exm(compLeft);
            e.pLeft = pDel;
            e.pRight = a0;
        }
        {
            let e = db.exm(compRight);
            e.pLeft = pDel;
            e.pRight = a1;
        }
        let r = exprCodeVector(db, pDel, &mut regFree1);
        sqlite3ExprToRegister(db, pDel, r);
        if let Some(xJump) = xJump {
            xJump(db, exprAnd, dest, jumpIfNull);
        } else {
            /* Mark the expression is being from the ON or USING clause of a join
             ** so that the sqlite3ExprCodeTarget() routine will not attempt to move
             ** it into the Parse.pConstExpr list.  We should use a new bit for this,
             ** for clarity, but we are out of bits in the Expr.flags field so we
             ** have to reuse the EP_OuterON bit.  Bummer. */
            db.exm(pDel).flags |= EP_OuterON;
            sqlite3ExprCodeTarget(db, exprAnd, dest);
        }
        sqlite3ReleaseTempReg(db, regFree1);
    }
    sqlite3ExprDelete(db, pDel);
}

/// Generate code for a boolean expression such that a jump is made
/// to the label "dest" if the expression is true but execution
/// continues straight thru if the expression is false.
///
/// If the expression evaluates to NULL (neither true nor false), then
/// take the jump if the jumpIfNull flag is SQLITE_JUMPIFNULL.
///
/// This code depends on the fact that certain token values (ex: TK_EQ)
/// are the same as opcode values (ex: OP_Eq) that implement the corresponding
/// operation.  Special comments in vdbe.c and the mkopcodeh.awk script in
/// the make process cause these values to align.  Assert()s in the code
/// below verify that the numbers are aligned correctly.
pub fn sqlite3ExprIfTrue(db: &mut Sqlite3, pExpr: ExprId, dest: i32, jumpIfNull: i32) {
    let mut jumpIfNull = jumpIfNull;
    let mut regFree1 = 0;
    let mut regFree2 = 0;
    let mut r1 = 0;
    let mut r2 = 0;

    debug_assert!(jumpIfNull == SQLITE_JUMPIFNULL as i32 || jumpIfNull == 0);
    if db.ppr().pVdbe.is_none() {
        return; /* Existence of VDBE checked by caller */
    }
    if pExpr.is_null() {
        return; /* No way this can happen */
    }
    let (eop, pLeft, pRight, op2, flags) = {
        let e = db.ex(pExpr);
        (e.op, e.pLeft, e.pRight, e.op2, e.flags)
    };
    let mut op = eop;
    let mut doDefault = false;
    match op {
        TK_AND | TK_OR => {
            let pAlt = simplifiedAndOr(db, pExpr);
            if pAlt != pExpr {
                sqlite3ExprIfTrue(db, pAlt, dest, jumpIfNull);
            } else {
                let (pFirst, pSecond) = if exprEvalRhsFirst(db, pExpr) {
                    (pRight, pLeft)
                } else {
                    (pLeft, pRight)
                };
                if op == TK_AND {
                    let d2 = sqlite3VdbeMakeLabel(db);
                    sqlite3ExprIfFalse(db, pFirst, d2, jumpIfNull ^ SQLITE_JUMPIFNULL as i32);
                    sqlite3ExprIfTrue(db, pSecond, dest, jumpIfNull);
                    sqlite3VdbeResolveLabel(db.v(), d2);
                } else {
                    sqlite3ExprIfTrue(db, pFirst, dest, jumpIfNull);
                    sqlite3ExprIfTrue(db, pSecond, dest, jumpIfNull);
                }
            }
        }
        TK_NOT => {
            sqlite3ExprIfFalse(db, pLeft, dest, jumpIfNull);
        }
        TK_TRUTH => {
            let isNot = op2 == TK_ISNOT; /* IS NOT TRUE or IS NOT FALSE */
            let isTrue = truthValue(db, pRight) != 0; /* IS TRUE or IS NOT TRUE */
            if isTrue ^ isNot {
                sqlite3ExprIfTrue(
                    db,
                    pLeft,
                    dest,
                    if isNot { SQLITE_JUMPIFNULL as i32 } else { 0 },
                );
            } else {
                sqlite3ExprIfFalse(
                    db,
                    pLeft,
                    dest,
                    if isNot { SQLITE_JUMPIFNULL as i32 } else { 0 },
                );
            }
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            if op == TK_IS || op == TK_ISNOT {
                op = if op == TK_IS { TK_EQ } else { TK_NE };
                jumpIfNull = SQLITE_NULLEQ as i32;
            }
            if exprVectorSize(db, pLeft) > 1 {
                doDefault = true;
            } else {
                let addrIsNull;
                if (flags & EP_Subquery) != 0 && jumpIfNull != SQLITE_NULLEQ as i32 {
                    addrIsNull = exprComputeOperands(
                        db,
                        pExpr,
                        &mut r1,
                        &mut r2,
                        &mut regFree1,
                        &mut regFree2,
                    );
                } else {
                    r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                    r2 = sqlite3ExprCodeTemp(db, pRight, &mut regFree2);
                    addrIsNull = 0;
                }
                codeCompare(
                    db,
                    pLeft,
                    pRight,
                    op,
                    r1,
                    r2,
                    dest,
                    jumpIfNull,
                    (flags & EP_Commuted) != 0,
                );
                if addrIsNull != 0 {
                    if jumpIfNull != 0 {
                        sqlite3VdbeChangeP2(db.v(), addrIsNull, dest);
                    } else {
                        sqlite3VdbeJumpHere(db.v(), addrIsNull);
                    }
                }
            }
        }
        TK_ISNULL | TK_NOTNULL => {
            r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
            debug_assert!(regFree1 == 0 || regFree1 == r1);
            if regFree1 != 0 {
                sqlite3VdbeTypeofColumn(db.v(), r1);
            }
            sqlite3VdbeAddOp2(db.v(), op, r1, dest);
        }
        TK_BETWEEN => {
            exprCodeBetween(db, pExpr, dest, Some(sqlite3ExprIfTrue), jumpIfNull);
        }
        TK_IN => {
            let destIfFalse = sqlite3VdbeMakeLabel(db);
            let destIfNull = if jumpIfNull != 0 { dest } else { destIfFalse };
            sqlite3ExprCodeIN(db, pExpr, destIfFalse, destIfNull);
            sqlite3VdbeGoto(db.v(), dest);
            sqlite3VdbeResolveLabel(db.v(), destIfFalse);
        }
        _ => {
            doDefault = true;
        }
    }
    if doDefault {
        /* default_expr: */
        let (alwaysTrue, alwaysFalse) = {
            let e = db.ex(pExpr);
            (e.AlwaysTrue(), e.AlwaysFalse())
        };
        if alwaysTrue {
            sqlite3VdbeGoto(db.v(), dest);
        } else if alwaysFalse {
            /* No-op */
        } else {
            r1 = sqlite3ExprCodeTemp(db, pExpr, &mut regFree1);
            sqlite3VdbeAddOp3(db.v(), OP_If, r1, dest, (jumpIfNull != 0) as i32);
        }
    }
    sqlite3ReleaseTempReg(db, regFree1);
    sqlite3ReleaseTempReg(db, regFree2);
}

/// Generate code for a boolean expression such that a jump is made
/// to the label "dest" if the expression is false but execution
/// continues straight thru if the expression is true.
///
/// If the expression evaluates to NULL (neither true nor false) then
/// jump if jumpIfNull is SQLITE_JUMPIFNULL or fall through if jumpIfNull
/// is 0.
pub fn sqlite3ExprIfFalse(db: &mut Sqlite3, pExpr: ExprId, dest: i32, jumpIfNull: i32) {
    let mut jumpIfNull = jumpIfNull;
    let mut regFree1 = 0;
    let mut regFree2 = 0;
    let mut r1 = 0;
    let mut r2 = 0;

    debug_assert!(jumpIfNull == SQLITE_JUMPIFNULL as i32 || jumpIfNull == 0);
    if db.ppr().pVdbe.is_none() {
        return; /* Existence of VDBE checked by caller */
    }
    if pExpr.is_null() {
        return;
    }
    let (eop, pLeft, pRight, op2, flags) = {
        let e = db.ex(pExpr);
        (e.op, e.pLeft, e.pRight, e.op2, e.flags)
    };

    /* The value of pExpr->op and op are related as follows:
     **
     **       pExpr->op            op
     **       ---------          ----------
     **       TK_ISNULL          OP_NotNull
     **       TK_NOTNULL         OP_IsNull
     **       TK_NE              OP_Eq
     **       TK_EQ              OP_Ne
     **       TK_GT              OP_Le
     **       TK_LE              OP_Gt
     **       TK_GE              OP_Lt
     **       TK_LT              OP_Ge
     **
     ** For other values of pExpr->op, op is undefined and unused.
     ** The value of TK_ and OP_ constants are arranged such that we
     ** can compute the mapping above using the following expression.
     ** Assert()s verify that the computation is correct.
     */
    let k = (TK_ISNULL & 1) as i32;
    let mut op = ((((eop as i32) + k) ^ 1) - k) as u8;

    /* Verify correct alignment of TK_ and OP_ constants
     */
    debug_assert!(eop != TK_ISNULL || op == OP_NotNull);
    debug_assert!(eop != TK_NOTNULL || op == OP_IsNull);
    debug_assert!(eop != TK_NE || op == OP_Eq);
    debug_assert!(eop != TK_EQ || op == OP_Ne);
    debug_assert!(eop != TK_LT || op == OP_Ge);
    debug_assert!(eop != TK_LE || op == OP_Gt);
    debug_assert!(eop != TK_GT || op == OP_Le);
    debug_assert!(eop != TK_GE || op == OP_Lt);

    let mut doDefault = false;
    match eop {
        TK_AND | TK_OR => {
            let pAlt = simplifiedAndOr(db, pExpr);
            if pAlt != pExpr {
                sqlite3ExprIfFalse(db, pAlt, dest, jumpIfNull);
            } else {
                let (pFirst, pSecond) = if exprEvalRhsFirst(db, pExpr) {
                    (pRight, pLeft)
                } else {
                    (pLeft, pRight)
                };
                if eop == TK_AND {
                    sqlite3ExprIfFalse(db, pFirst, dest, jumpIfNull);
                    sqlite3ExprIfFalse(db, pSecond, dest, jumpIfNull);
                } else {
                    let d2 = sqlite3VdbeMakeLabel(db);
                    sqlite3ExprIfTrue(db, pFirst, d2, jumpIfNull ^ SQLITE_JUMPIFNULL as i32);
                    sqlite3ExprIfFalse(db, pSecond, dest, jumpIfNull);
                    sqlite3VdbeResolveLabel(db.v(), d2);
                }
            }
        }
        TK_NOT => {
            sqlite3ExprIfTrue(db, pLeft, dest, jumpIfNull);
        }
        TK_TRUTH => {
            let isNot = op2 == TK_ISNOT; /* IS NOT TRUE or IS NOT FALSE */
            let isTrue = truthValue(db, pRight) != 0; /* IS TRUE or IS NOT TRUE */
            if isTrue ^ isNot {
                /* IS TRUE and IS NOT FALSE */
                sqlite3ExprIfFalse(
                    db,
                    pLeft,
                    dest,
                    if isNot { 0 } else { SQLITE_JUMPIFNULL as i32 },
                );
            } else {
                /* IS FALSE and IS NOT TRUE */
                sqlite3ExprIfTrue(
                    db,
                    pLeft,
                    dest,
                    if isNot { 0 } else { SQLITE_JUMPIFNULL as i32 },
                );
            }
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            if eop == TK_IS || eop == TK_ISNOT {
                op = if eop == TK_IS { TK_NE } else { TK_EQ };
                jumpIfNull = SQLITE_NULLEQ as i32;
            }
            if exprVectorSize(db, pLeft) > 1 {
                doDefault = true;
            } else {
                let addrIsNull;
                if (flags & EP_Subquery) != 0 && jumpIfNull != SQLITE_NULLEQ as i32 {
                    addrIsNull = exprComputeOperands(
                        db,
                        pExpr,
                        &mut r1,
                        &mut r2,
                        &mut regFree1,
                        &mut regFree2,
                    );
                } else {
                    r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
                    r2 = sqlite3ExprCodeTemp(db, pRight, &mut regFree2);
                    addrIsNull = 0;
                }
                codeCompare(
                    db,
                    pLeft,
                    pRight,
                    op,
                    r1,
                    r2,
                    dest,
                    jumpIfNull,
                    (flags & EP_Commuted) != 0,
                );
                if addrIsNull != 0 {
                    if jumpIfNull != 0 {
                        sqlite3VdbeChangeP2(db.v(), addrIsNull, dest);
                    } else {
                        sqlite3VdbeJumpHere(db.v(), addrIsNull);
                    }
                }
            }
        }
        TK_ISNULL | TK_NOTNULL => {
            r1 = sqlite3ExprCodeTemp(db, pLeft, &mut regFree1);
            debug_assert!(regFree1 == 0 || regFree1 == r1);
            if regFree1 != 0 {
                sqlite3VdbeTypeofColumn(db.v(), r1);
            }
            sqlite3VdbeAddOp2(db.v(), op, r1, dest);
        }
        TK_BETWEEN => {
            exprCodeBetween(db, pExpr, dest, Some(sqlite3ExprIfFalse), jumpIfNull);
        }
        TK_IN => {
            if jumpIfNull != 0 {
                sqlite3ExprCodeIN(db, pExpr, dest, dest);
            } else {
                let destIfNull = sqlite3VdbeMakeLabel(db);
                sqlite3ExprCodeIN(db, pExpr, dest, destIfNull);
                sqlite3VdbeResolveLabel(db.v(), destIfNull);
            }
        }
        _ => {
            doDefault = true;
        }
    }
    if doDefault {
        /* default_expr: */
        let (alwaysTrue, alwaysFalse) = {
            let e = db.ex(pExpr);
            (e.AlwaysTrue(), e.AlwaysFalse())
        };
        if alwaysFalse {
            sqlite3VdbeGoto(db.v(), dest);
        } else if alwaysTrue {
            /* no-op */
        } else {
            r1 = sqlite3ExprCodeTemp(db, pExpr, &mut regFree1);
            sqlite3VdbeAddOp3(db.v(), OP_IfNot, r1, dest, (jumpIfNull != 0) as i32);
        }
    }
    sqlite3ReleaseTempReg(db, regFree1);
    sqlite3ReleaseTempReg(db, regFree2);
}

/// Like sqlite3ExprIfFalse() except that a copy is made of pExpr before
/// code generation, and that copy is deleted after code generation. This
/// ensures that the original pExpr is unchanged.
pub fn sqlite3ExprIfFalseDup(db: &mut Sqlite3, pExpr: ExprId, dest: i32, jumpIfNull: i32) {
    let pCopy = sqlite3ExprDup(db, pExpr, 0);
    if db.mallocFailed == 0 {
        sqlite3ExprIfFalse(db, pCopy, dest, jumpIfNull);
    }
    sqlite3ExprDelete(db, pCopy);
}

// ---------------------------------------------------------------------------
// Expression comparison
// ---------------------------------------------------------------------------

/// Expression pVar is guaranteed to be an SQL variable. pExpr may be any
/// type of expression.
///
/// If pExpr is a simple SQL value - an integer, real, string, blob
/// or NULL value - then the VDBE currently being prepared is configured
/// to re-prepare each time a new value is bound to variable pVar.
///
/// Additionally, if pExpr is a simple SQL value and the value is the
/// same as that currently bound to variable pVar, non-zero is returned.
/// Otherwise, if the values are not the same or if pExpr is not a simple
/// SQL value, zero is returned.
///
/// If the SQLITE_EnableQPSG flag is set on the database connection, then
/// this routine always returns false.
fn exprCompareVariable(db: &mut Sqlite3, pVar: ExprId, pExpr: ExprId) -> i32 {
    let mut res = 2;
    let iVar = db.ex(pVar).iColumn as i32;
    if db.ex(pExpr).op == TK_VARIABLE && iVar == db.ex(pExpr).iColumn as i32 {
        return 0;
    }
    if (db.flags & SQLITE_EnableQPSG) != 0 {
        return 2;
    }
    let mut pR: Option<Box<Mem>> = None;
    crate::vdbemem::sqlite3ValueFromExpr(db, pExpr, SQLITE_UTF8, SQLITE_AFF_BLOB, &mut pR);
    if let Some(mut pR) = pR {
        with_parse_vdbe(db, |db, v| sqlite3VdbeReprepareOnBind(db, v, iVar, 0));
        let pL = bound_value(db, iVar, SQLITE_AFF_BLOB);
        if let Some(mut pL) = pL {
            if crate::vdbeapi::sqlite3_value_type(&mut pL) == SQLITE_TEXT {
                crate::vdbeapi::sqlite3_value_text(&mut pL); /* Make sure the encoding is UTF-8 */
            }
            /* A NULL collating sequence compares with memcmp(), as BINARY does */
            res = if crate::vdbeaux::sqlite3MemCompare(db, &pL, &pR, None) != 0 {
                2
            } else {
                0
            };
            crate::vdbemem::sqlite3ValueFree(db, &mut pL);
        }
        crate::vdbemem::sqlite3ValueFree(db, &mut pR);
    }
    res
}

/// Do a deep comparison of two expression trees.  Return 0 if the two
/// expressions are completely identical.  Return 1 if they differ only
/// by a COLLATE operator at the top level.  Return 2 if there are differences
/// other than the top-level COLLATE operator.
///
/// If any subelement of pB has Expr.iTable==(-1) then it is allowed
/// to compare equal to an equivalent element in pA with Expr.iTable==iTab.
///
/// The pA side might be using TK_REGISTER.  If that is the case and pB is
/// not using TK_REGISTER but is otherwise equivalent, then still return 0.
///
/// Sometimes this routine will return 2 even if the two expressions
/// really are equivalent.  If we cannot prove that the expressions are
/// identical, we return 2 just to be safe.  So if this routine
/// returns 2, then you do not really know for certain if the two
/// expressions are the same.  But if you get a 0 or 1 return, then you
/// can be sure the expressions are the same.  In the places where
/// this routine is used, it does not hurt to get an extra 2 - that
/// just might result in some slightly slower code.  But returning
/// an incorrect 0 or 1 could lead to a malfunction.
///
/// If pParse is true (C non-NULL) and SQLITE_EnableQPSG is off then
/// TK_VARIABLE terms in pA with bindings in pParse->pReprepare can be
/// matched against literals in pB.  The pParse->pVdbe->expmask bitmask is
/// updated for each variable referenced.
pub fn sqlite3ExprCompare(
    db: &mut Sqlite3,
    pParse: bool,
    pA: ExprId,
    pB: ExprId,
    iTab: i32,
) -> i32 {
    if pA.is_null() || pB.is_null() {
        return if pB == pA { 0 } else { 2 };
    }
    if pParse && db.ex(pA).op == TK_VARIABLE {
        return exprCompareVariable(db, pA, pB);
    }
    let (fa, opA, op2A, ivA, zA, lA, rA, listA, colA, tabA, winA) = {
        let a = db.ex(pA);
        (
            a.flags,
            a.op,
            a.op2,
            a.u.iValue,
            a.u.zToken.clone(),
            a.pLeft,
            a.pRight,
            a.x.pList,
            a.iColumn,
            a.iTable,
            a.y.pWin,
        )
    };
    let (fb, opB, op2B, ivB, zB, lB, rB, listB, colB, tabB, winB) = {
        let b = db.ex(pB);
        (
            b.flags,
            b.op,
            b.op2,
            b.u.iValue,
            b.u.zToken.clone(),
            b.pLeft,
            b.pRight,
            b.x.pList,
            b.iColumn,
            b.iTable,
            b.y.pWin,
        )
    };
    let combinedFlags = fa | fb;
    if (combinedFlags & EP_IntValue) != 0 {
        if (fa & fb & EP_IntValue) != 0 && ivA == ivB {
            return 0;
        }
        return 2;
    }
    if opA != opB || opA == TK_RAISE {
        if opA == TK_COLLATE && sqlite3ExprCompare(db, pParse, lA, pB, iTab) < 2 {
            return 1;
        }
        if opB == TK_COLLATE && sqlite3ExprCompare(db, pParse, pA, lB, iTab) < 2 {
            return 1;
        }
        if opA == TK_AGG_COLUMN && opB == TK_COLUMN && tabB < 0 && tabA == iTab {
            /* fall through */
        } else {
            return 2;
        }
    }
    if let Some(za) = &zA {
        if opA == TK_FUNCTION || opA == TK_AGG_FUNCTION {
            if crate::util::sqlite3StrICmp(Some(za), zB.as_deref()) != 0 {
                return 2;
            }
            debug_assert!(opA == opB);
            if (fa & EP_WinFunc) != (fb & EP_WinFunc) {
                return 2;
            }
            if (fa & EP_WinFunc) != 0 && crate::window::sqlite3WindowCompare(db, winA, winB, 1) != 0
            {
                return 2;
            }
        } else if opA == TK_NULL {
            return 0;
        } else if opA == TK_COLLATE {
            if crate::util::sqlite3_stricmp(Some(za), zB.as_deref()) != 0 {
                return 2;
            }
        } else if let Some(zb) = &zB {
            if opA != TK_COLUMN && opA != TK_AGG_COLUMN && za != zb {
                return 2;
            }
        }
    }
    if (fa & (EP_Distinct | EP_Commuted)) != (fb & (EP_Distinct | EP_Commuted)) {
        return 2;
    }
    if (combinedFlags & EP_TokenOnly) == 0 {
        if (combinedFlags & EP_xIsSelect) != 0 {
            return 2;
        }
        if (combinedFlags & EP_FixedCol) == 0 && sqlite3ExprCompare(db, pParse, lA, lB, iTab) != 0 {
            return 2;
        }
        if sqlite3ExprCompare(db, pParse, rA, rB, iTab) != 0 {
            return 2;
        }
        if sqlite3ExprListCompare(db, listA, listB, iTab) != 0 {
            return 2;
        }
        if opA != TK_STRING && opA != TK_TRUEFALSE && (combinedFlags & EP_Reduced) == 0 {
            if colA != colB {
                return 2;
            }
            if op2A != op2B && opA == TK_TRUTH {
                return 2;
            }
            if opA != TK_IN && tabA != tabB && tabA != iTab {
                return 2;
            }
        }
    }
    0
}

/// Compare two ExprList objects.  Return 0 if they are identical, 1
/// if they are certainly different, or 2 if it is not possible to
/// determine if they are identical or not.
///
/// If any subelement of pB has Expr.iTable==(-1) then it is allowed
/// to compare equal to an equivalent element in pA with Expr.iTable==iTab.
///
/// This routine might return non-zero for equivalent ExprLists.  The
/// only consequence will be disabled optimizations.  But this routine
/// must never return 0 if the two ExprList objects are different, or
/// a malfunction will result.
///
/// Two NULL pointers are considered to be the same.  But a NULL pointer
/// always differs from a non-NULL pointer.
pub fn sqlite3ExprListCompare(db: &mut Sqlite3, pA: ExprListId, pB: ExprListId, iTab: i32) -> i32 {
    if pA.is_null() && pB.is_null() {
        return 0;
    }
    if pA.is_null() || pB.is_null() {
        return 1;
    }
    let n = el_n(db, pA);
    if n != el_n(db, pB) {
        return 1;
    }
    for i in 0..n as usize {
        let (sa, ea) = {
            let it = &db.el(pA).a[i];
            (it.fg.sortFlags, it.pExpr)
        };
        let (sb, eb) = {
            let it = &db.el(pB).a[i];
            (it.fg.sortFlags, it.pExpr)
        };
        if sa != sb {
            return 1;
        }
        let res = sqlite3ExprCompare(db, false, ea, eb, iTab);
        if res != 0 {
            return res;
        }
    }
    0
}

/// Like sqlite3ExprCompare() except COLLATE operators at the top-level
/// are ignored.
pub fn sqlite3ExprCompareSkip(db: &mut Sqlite3, pA: ExprId, pB: ExprId, iTab: i32) -> i32 {
    let a = skipCollate(db, pA);
    let b = skipCollate(db, pB);
    sqlite3ExprCompare(db, false, a, b, iTab)
}

/// Return non-zero if Expr p can only be true if pNN is not NULL.
///
/// Or if seenNot is true, return non-zero if Expr p can only be
/// non-NULL if pNN is not NULL
fn exprImpliesNotNull(db: &mut Sqlite3, p: ExprId, pNN: ExprId, iTab: i32, seenNot: bool) -> bool {
    let mut seenNot = seenNot;
    debug_assert!(p.nn());
    debug_assert!(pNN.nn());
    if sqlite3ExprCompare(db, true, p, pNN, iTab) == 0 {
        return db.ex(pNN).op != TK_NULL;
    }
    let (op, op2, flags, pLeft, pRight, pList) = {
        let e = db.ex(p);
        (e.op, e.op2, e.flags, e.pLeft, e.pRight, e.x.pList)
    };
    match op {
        TK_IN => {
            if seenNot && (flags & EP_xIsSelect) != 0 {
                return false;
            }
            exprImpliesNotNull(db, pLeft, pNN, iTab, true)
        }
        TK_BETWEEN => {
            debug_assert!(el_n(db, pList) == 2);
            if seenNot {
                return false;
            }
            let (a0, a1) = (el_e(db, pList, 0), el_e(db, pList, 1));
            if exprImpliesNotNull(db, a0, pNN, iTab, true)
                || exprImpliesNotNull(db, a1, pNN, iTab, true)
            {
                return true;
            }
            exprImpliesNotNull(db, pLeft, pNN, iTab, true)
        }
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE | TK_PLUS | TK_MINUS | TK_BITOR
        | TK_LSHIFT | TK_RSHIFT | TK_CONCAT | TK_STAR | TK_REM | TK_BITAND | TK_SLASH => {
            if matches!(
                op,
                TK_EQ
                    | TK_NE
                    | TK_LT
                    | TK_LE
                    | TK_GT
                    | TK_GE
                    | TK_PLUS
                    | TK_MINUS
                    | TK_BITOR
                    | TK_LSHIFT
                    | TK_RSHIFT
                    | TK_CONCAT
            ) {
                seenNot = true;
            }
            if exprImpliesNotNull(db, pRight, pNN, iTab, seenNot) {
                return true;
            }
            exprImpliesNotNull(db, pLeft, pNN, iTab, seenNot)
        }
        TK_SPAN | TK_COLLATE | TK_UPLUS | TK_UMINUS => {
            exprImpliesNotNull(db, pLeft, pNN, iTab, seenNot)
        }
        TK_TRUTH => {
            if seenNot {
                return false;
            }
            if op2 != TK_IS {
                return false;
            }
            exprImpliesNotNull(db, pLeft, pNN, iTab, true)
        }
        TK_BITNOT | TK_NOT => exprImpliesNotNull(db, pLeft, pNN, iTab, true),
        _ => false,
    }
}

/// Return true if the boolean value of the expression is always either
/// FALSE or NULL.
fn sqlite3ExprIsNotTrue(db: &mut Sqlite3, pExpr: ExprId) -> bool {
    let op = db.ex(pExpr).op;
    if op == TK_NULL {
        return true;
    }
    if op == TK_TRUEFALSE && truthValue(db, pExpr) == 0 {
        return true;
    }
    let mut v = 1;
    if sqlite3ExprIsInteger(db, pExpr, &mut v, false, 0) != 0 && v == 0 {
        return true;
    }
    false
}

/// Return true if the expression is one of the following:
///
///    CASE WHEN x THEN y END
///    CASE WHEN x THEN y ELSE NULL END
///    CASE WHEN x THEN y ELSE false END
///    iif(x,y)
///    iif(x,y,NULL)
///    iif(x,y,false)
fn sqlite3ExprIsIIF(db: &mut Sqlite3, pExpr: ExprId) -> bool {
    let (op, z, pList, pLeft) = {
        let e = db.ex(pExpr);
        (e.op, e.u.zToken.clone(), e.x.pList, e.pLeft)
    };
    if op == TK_FUNCTION {
        let z = z.unwrap_or_default();
        if z.is_empty() || (z[0] != b'i' && z[0] != b'I') {
            return false;
        }
        if pList.is_null() {
            return false;
        }
        let n = el_n(db, pList);
        let enc = db.ENC();
        let Some(pDef) = crate::callback::sqlite3FindFunction(db, Some(&z), n, enc, 0) else {
            return false;
        };
        if (pDef.funcFlags & SQLITE_FUNC_INLINE) == 0 {
            return false;
        }
        if pDef.pUserData.as_int() as i32 != INLINEFUNC_iif {
            return false;
        }
    } else if op == TK_CASE {
        if pLeft.nn() {
            return false;
        }
    } else {
        return false;
    }
    debug_assert!(pList.nn());
    let n = el_n(db, pList);
    if n == 2 {
        return true;
    }
    if n == 3 {
        let a2 = el_e(db, pList, 2);
        if sqlite3ExprIsNotTrue(db, a2) {
            return true;
        }
    }
    false
}

/// Return true if we can prove the pE2 will always be true if pE1 is
/// true.  Return false if we cannot complete the proof or if pE2 might
/// be false.  Examples:
///
///     pE1: x==5        pE2: x==5             Result: true
///     pE1: x>0         pE2: x==5             Result: false
///     pE1: x=21        pE2: x=21 OR y=43     Result: true
///     pE1: x!=123      pE2: x IS NOT NULL    Result: true
///     pE1: x!=?1       pE2: x IS NOT NULL    Result: true
///     pE1: x IS NULL   pE2: x IS NOT NULL    Result: false
///     pE1: x IS ?2     pE2: x IS NOT NULL    Result: false
///     pE1: iif(x,y)    pE2: x                Result: true
///     PE1: iif(x,y,0)  pE2: x                Result: true
///
/// When comparing TK_COLUMN nodes between pE1 and pE2, if pE2 has
/// Expr.iTable<0 then assume a table number given by iTab.
///
/// The values of bound variables in pE1 are compared against literal
/// values in pE2 and pParse->pVdbe->expmask is modified to record which
/// bound variables are referenced.
///
/// When in doubt, return false.  Returning true might give a performance
/// improvement.  Returning false might cause a performance reduction, but
/// it will always give the correct answer and is hence always safe.
pub fn sqlite3ExprImpliesExpr(db: &mut Sqlite3, pE1: ExprId, pE2: ExprId, iTab: i32) -> i32 {
    if sqlite3ExprCompare(db, true, pE1, pE2, iTab) == 0 {
        return 1;
    }
    let (op2, l2, r2) = {
        let e = db.ex(pE2);
        (e.op, e.pLeft, e.pRight)
    };
    if op2 == TK_OR
        && (sqlite3ExprImpliesExpr(db, pE1, l2, iTab) != 0
            || sqlite3ExprImpliesExpr(db, pE1, r2, iTab) != 0)
    {
        return 1;
    }
    if op2 == TK_NOTNULL && exprImpliesNotNull(db, pE1, l2, iTab, false) {
        return 1;
    }
    if sqlite3ExprIsIIF(db, pE1) {
        let a0 = el_e(db, db.ex(pE1).x.pList, 0);
        return sqlite3ExprImpliesExpr(db, a0, pE2, iTab);
    }
    0
}

/// This is a helper function to impliesNotNullRow().  In this routine,
/// set pWalker->eCode to one only if *both* of the input expressions
/// separately have the implies-not-null-row property.
fn bothImplyNotNullRow(w: &mut Walker, pE1: ExprId, pE2: ExprId) {
    if w.eCode == 0 {
        sqlite3WalkExpr(w, pE1);
        if w.eCode != 0 {
            w.eCode = 0;
            sqlite3WalkExpr(w, pE2);
        }
    }
}

/// This is the Expr node callback for sqlite3ExprImpliesNonNullRow().
/// If the expression node requires that the table at pWalker->iCur
/// have one or more non-NULL column, then set pWalker->eCode to 1 and abort.
///
/// pWalker->mWFlags is non-zero if this inquiry is being undertaking on
/// behalf of a RIGHT JOIN (or FULL JOIN).  That makes a difference when
/// evaluating terms in the ON clause of an inner join.
///
/// This routine controls an optimization.  False positives (setting
/// pWalker->eCode to 1 when it should not be) are deadly, but false-negatives
/// (never setting pWalker->eCode) is a harmless missed optimization.
fn impliesNotNullRow(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, flags, iTable, pLeft, pRight, pList) = {
        let e = w.db.ex(pExpr);
        (e.op, e.flags, e.iTable, e.pLeft, e.pRight, e.x.pList)
    };
    if (flags & EP_OuterON) != 0 {
        return WRC_Prune;
    }
    if (flags & EP_InnerON) != 0 && w.mWFlags != 0 {
        /* If iCur is used in an inner-join ON clause to the left of a
         ** RIGHT JOIN, that does *not* mean that the table must be non-null.
         ** But it is difficult to check for that condition precisely.
         ** To keep things simple, any use of iCur from any inner-join is
         ** ignored while attempting to simplify a RIGHT JOIN. */
        return WRC_Prune;
    }
    match op {
        TK_ISNOT | TK_ISNULL | TK_NOTNULL | TK_IS | TK_VECTOR | TK_FUNCTION | TK_TRUTH
        | TK_CASE => WRC_Prune,

        TK_COLUMN => {
            if w.u.iCur == iTable {
                w.eCode = 1;
                return WRC_Abort;
            }
            WRC_Prune
        }

        TK_OR | TK_AND => {
            /* Both sides of an AND or OR must separately imply non-null-row.
             ** Consider these cases:
             **    1.  NOT (x AND y)
             **    2.  x OR y
             ** If only one of x or y is non-null-row, then the overall expression
             ** can be true if the other arm is false (case 1) or true (case 2).
             */
            bothImplyNotNullRow(w, pLeft, pRight);
            WRC_Prune
        }

        TK_IN => {
            /* Beware of "x NOT IN ()" and "x NOT IN (SELECT 1 WHERE false)",
             ** both of which can be true.  But apart from these cases, if
             ** the left-hand side of the IN is NULL then the IN itself will be
             ** NULL. */
            if (flags & EP_xIsSelect) == 0 && el_n(w.db, pList) > 0 {
                sqlite3WalkExpr(w, pLeft);
            }
            WRC_Prune
        }

        TK_BETWEEN => {
            /* In "x NOT BETWEEN y AND z" either x must be non-null-row or else
             ** both y and z must be non-null row */
            debug_assert!(el_n(w.db, pList) == 2);
            sqlite3WalkExpr(w, pLeft);
            let (a0, a1) = (el_e(w.db, pList, 0), el_e(w.db, pList, 1));
            bothImplyNotNullRow(w, a0, a1);
            WRC_Prune
        }

        /* Virtual tables are allowed to use constraints like x=NULL.  So
         ** a term of the form x=y does not prove that y is not null if x
         ** is the column of a virtual table */
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
            let isVtabCol = |db: &Sqlite3, e: ExprId| -> bool {
                let x = db.ex(e);
                x.op == TK_COLUMN
                    && x.y
                        .pTab
                        .as_ref()
                        .map(|t| t.borrow().IsVirtual())
                        .unwrap_or(false)
            };
            if isVtabCol(w.db, pLeft) || isVtabCol(w.db, pRight) {
                return WRC_Prune;
            }
            WRC_Continue
        }
        _ => WRC_Continue,
    }
}

/// Return true (non-zero) if expression p can only be true if at least
/// one column of table iTab is non-null.  In other words, return true
/// if expression p will always be NULL or false if every column of iTab
/// is NULL.
///
/// False negatives are acceptable.  In other words, it is ok to return
/// zero even if expression p will never be true of every column of iTab
/// is NULL.  A false negative is merely a missed optimization opportunity.
///
/// False positives are not allowed, however.  A false positive may result
/// in an incorrect answer.
///
/// Terms of p that are marked with EP_OuterON (and hence that come from
/// the ON or USING clauses of OUTER JOINS) are excluded from the analysis.
///
/// This routine is used to check if a LEFT JOIN can be converted into
/// an ordinary JOIN.  The p argument is the WHERE clause.  If the WHERE
/// clause requires that some column of the right table of the LEFT JOIN
/// be non-NULL, then the LEFT JOIN can be safely converted into an
/// ordinary join.
pub fn sqlite3ExprImpliesNonNullRow(db: &mut Sqlite3, p: ExprId, iTab: i32, isRJ: i32) -> i32 {
    let mut p = skipCollateAndLikely(db, p);
    if p.is_null() {
        return 0;
    }
    if db.ex(p).op == TK_NOTNULL {
        p = db.ex(p).pLeft;
    } else {
        while db.ex(p).op == TK_AND {
            let l = db.ex(p).pLeft;
            if sqlite3ExprImpliesNonNullRow(db, l, iTab, isRJ) != 0 {
                return 1;
            }
            p = db.ex(p).pRight;
        }
    }
    let mut w = Walker::new(db);
    w.xExprCallback = Some(impliesNotNullRow);
    w.xSelectCallback = None;
    w.xSelectCallback2 = None;
    w.eCode = 0;
    w.mWFlags = (isRJ != 0) as u16;
    w.u.iCur = iTab;
    sqlite3WalkExpr(&mut w, p);
    w.eCode as i32
}

/// An instance of the following structure is used by the tree walker
/// to determine if an expression can be evaluated by reference to the
/// index only, without having to do a search for the corresponding
/// table entry.  The IdxCover.pIdx field is the index.  IdxCover.iCur
/// is the cursor for the table.
struct IdxCover {
    pIdx: IndexRef, /* The index to be tested for coverage */
    iCur: i32,      /* Cursor number for the table corresponding to the index */
}

/// Check to see if there are references to columns in table
/// pWalker->u.pIdxCover->iCur can be satisfied using the index
/// pWalker->u.pIdxCover->pIdx.
fn exprIdxCover(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, iTable, iColumn) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iTable, e.iColumn as i32)
    };
    if op == TK_COLUMN {
        let cov =
            w.u.any
                .as_ref()
                .and_then(|a| a.downcast_ref::<IdxCover>())
                .expect("IdxCover");
        if iTable == cov.iCur && crate::build::sqlite3TableColumnToIndex(&cov.pIdx, iColumn) < 0 {
            w.eCode = 1;
            return WRC_Abort;
        }
    }
    WRC_Continue
}

/// Determine if an index pIdx on table with cursor iCur contains will
/// the expression pExpr.  Return true if the index does cover the
/// expression and false if the pExpr expression references table columns
/// that are not found in the index pIdx.
///
/// An index covering an expression means that the expression can be
/// evaluated using only the index and without having to lookup the
/// corresponding table entry.
pub fn sqlite3ExprCoveredByIndex(
    db: &mut Sqlite3,
    pExpr: ExprId,
    iCur: i32,
    pIdx: &IndexRef,
) -> i32 {
    let mut w = Walker::new(db);
    w.xExprCallback = Some(exprIdxCover);
    w.u.any = Some(Box::new(IdxCover {
        pIdx: pIdx.clone(),
        iCur,
    }));
    sqlite3WalkExpr(&mut w, pExpr);
    (w.eCode == 0) as i32
}

/// Structure used to pass information throughout the Walker in order to
/// implement sqlite3ReferencesSrcList().
struct RefSrcList {
    pRef: SrcListId,     /* Looking for references to these tables */
    aiExclude: Vec<i32>, /* Cursor IDs for tables to exclude from the search */
}

fn refsrclist<'a>(w: &'a mut Walker) -> &'a mut RefSrcList {
    w.u.any
        .as_mut()
        .and_then(|a| a.downcast_mut::<RefSrcList>())
        .expect("RefSrcList")
}

/// Walker SELECT callbacks for sqlite3ReferencesSrcList().
///
/// When entering a new subquery on the pExpr argument, add all FROM clause
/// entries for that subquery to the exclude list.
///
/// When leaving the subquery, remove those entries from the exclude list.
fn selectRefEnter(w: &mut Walker, pSelect: SelectId) -> i32 {
    let pSrc = w.db.sel(pSelect).pSrc;
    if pSrc.is_null() || w.db.src(pSrc).nSrc() == 0 {
        return WRC_Continue;
    }
    let cursors: Vec<i32> = w.db.src(pSrc).a.iter().map(|it| it.iCursor).collect();
    refsrclist(w).aiExclude.extend(cursors);
    WRC_Continue
}
fn selectRefLeave(w: &mut Walker, pSelect: SelectId) {
    let pSrc = w.db.sel(pSelect).pSrc;
    let nSrc = if pSrc.is_null() {
        0
    } else {
        w.db.src(pSrc).a.len()
    };
    let p = refsrclist(w);
    if !p.aiExclude.is_empty() {
        debug_assert!(p.aiExclude.len() >= nSrc);
        let n = p.aiExclude.len().saturating_sub(nSrc);
        p.aiExclude.truncate(n);
    }
}

/// This is the Walker EXPR callback for sqlite3ReferencesSrcList().
///
/// Set the 0x01 bit of pWalker->eCode if there is a reference to any
/// of the tables shown in RefSrcList.pRef.
///
/// Set the 0x02 bit of pWalker->eCode if there is a reference to a
/// table is in neither RefSrcList.pRef nor RefSrcList.aiExclude.
fn exprRefToSrcList(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (op, iTable) = {
        let e = w.db.ex(pExpr);
        (e.op, e.iTable)
    };
    if op == TK_COLUMN || op == TK_AGG_COLUMN {
        let pSrc = refsrclist(w).pRef;
        let inRef = if pSrc.nn() {
            w.db.src(pSrc).a.iter().any(|it| it.iCursor == iTable)
        } else {
            false
        };
        if inRef {
            w.eCode |= 1;
            return WRC_Continue;
        }
        let excluded = refsrclist(w).aiExclude.iter().any(|&c| c == iTable);
        if !excluded {
            w.eCode |= 2;
        }
    }
    WRC_Continue
}

/// Check to see if pExpr references any tables in pSrcList.
/// Possible return values:
///
///    1         pExpr does references a table in pSrcList.
///
///    0         pExpr references some table that is not defined in either
///              pSrcList or in subqueries of pExpr itself.
///
///   -1         pExpr only references no tables at all, or it only
///              references tables defined in subqueries of pExpr itself.
///
/// As currently used, pExpr is always an aggregate function call.  That
/// fact is exploited for efficiency.
pub fn sqlite3ReferencesSrcList(db: &mut Sqlite3, pExpr: ExprId, pSrcList: SrcListId) -> i32 {
    let (pList, pLeft, flags, pWin) = {
        let e = db.ex(pExpr);
        (e.x.pList, e.pLeft, e.flags, e.y.pWin)
    };
    let pLeftList = if pLeft.nn() {
        db.ex(pLeft).x.pList
    } else {
        ExprListId::NULL
    };
    let pFilter = if (flags & EP_WinFunc) != 0 {
        db.win(pWin).pFilter
    } else {
        ExprId::NULL
    };
    let mut w = Walker::new(db);
    w.xExprCallback = Some(exprRefToSrcList);
    w.xSelectCallback = Some(selectRefEnter);
    w.xSelectCallback2 = Some(selectRefLeave);
    w.u.any = Some(Box::new(RefSrcList {
        pRef: pSrcList,
        aiExclude: Vec::new(),
    }));
    debug_assert!(w.db.ex(pExpr).op == TK_AGG_FUNCTION);
    sqlite3WalkExprList(&mut w, pList);
    if pLeft.nn() {
        debug_assert!(w.db.ex(pLeft).op == TK_ORDER);
        sqlite3WalkExprList(&mut w, pLeftList);
    }
    if (flags & EP_WinFunc) != 0 {
        sqlite3WalkExpr(&mut w, pFilter);
    }
    if (w.eCode & 0x01) != 0 {
        1
    } else if w.eCode != 0 {
        0
    } else {
        -1
    }
}

// ---------------------------------------------------------------------------
// Aggregates
// ---------------------------------------------------------------------------

/// This is a Walker expression node callback.
///
/// For Expr nodes that contain pAggInfo pointers, make sure the AggInfo
/// object that is referenced does not refer directly to the Expr.  If
/// it does, make a copy.  This is done because the pExpr argument is
/// subject to change.
///
/// The copy is scheduled for deletion using the sqlite3ExprDeferredDelete()
/// which builds on the sqlite3ParserAddCleanup() mechanism.
fn agginfoPersistExprCb(w: &mut Walker, pExpr: ExprId) -> i32 {
    let (flags, pAggInfo, iAgg, op) = {
        let e = w.db.ex(pExpr);
        (e.flags, e.pAggInfo, e.iAgg as i32, e.op)
    };
    if (flags & (EP_TokenOnly | EP_Reduced)) == 0 && pAggInfo.nn() {
        debug_assert!(iAgg >= 0);
        if op != TK_AGG_FUNCTION {
            let matches = {
                let a = w.db.arena.agg(pAggInfo);
                iAgg < a.nColumn() && a.aCol[iAgg as usize].pCExpr == pExpr
            };
            if matches {
                let d = sqlite3ExprDup(w.db, pExpr, 0);
                if d.nn() && sqlite3ExprDeferredDelete(w.db, d) == 0 {
                    w.db.arena.aggm(pAggInfo).aCol[iAgg as usize].pCExpr = d;
                }
            }
        } else {
            let matches = {
                let a = w.db.arena.agg(pAggInfo);
                iAgg < a.nFunc() && a.aFunc[iAgg as usize].pFExpr == pExpr
            };
            if matches {
                let d = sqlite3ExprDup(w.db, pExpr, 0);
                if d.nn() && sqlite3ExprDeferredDelete(w.db, d) == 0 {
                    w.db.arena.aggm(pAggInfo).aFunc[iAgg as usize].pFExpr = d;
                }
            }
        }
    }
    WRC_Continue
}

/// Initialize a Walker object so that will persist AggInfo entries referenced
/// by the tree that is walked.
pub fn sqlite3AggInfoPersistWalkerInit(w: &mut Walker) {
    w.xExprCallback = Some(agginfoPersistExprCb);
    w.xSelectCallback = Some(sqlite3SelectWalkNoop);
    w.xSelectCallback2 = None;
    w.pParse = true;
    w.walkerDepth = 0;
    w.eCode = 0;
    w.mWFlags = 0;
    w.u = WalkerU::default();
}

/// Add a new element to the pAggInfo->aCol[] array.  Return the index of
/// the new element.
fn addAggInfoColumn(db: &mut Sqlite3, pInfo: AggInfoId) -> i32 {
    let a = db.arena.aggm(pInfo);
    a.aCol.push(AggInfoCol::default());
    a.aCol.len() as i32 - 1
}

/// Add a new element to the pAggInfo->aFunc[] array.  Return the index of
/// the new element.
fn addAggInfoFunc(db: &mut Sqlite3, pInfo: AggInfoId) -> i32 {
    let a = db.arena.aggm(pInfo);
    a.aFunc.push(AggInfoFunc::default());
    a.aFunc.len() as i32 - 1
}

/// Search the AggInfo object for an aCol[] entry that has iTable and iColumn.
/// Return the index in aCol[] of the entry that describes that column.
///
/// If no prior entry is found, create a new one and return -1.  The
/// new column will have an index of pAggInfo->nColumn-1.
fn findOrCreateAggInfoColumn(db: &mut Sqlite3, pAggInfo: AggInfoId, pExpr: ExprId) {
    let mxTerm = db.aLimit[SQLITE_LIMIT_COLUMN as usize];
    let (eTable, eColumn, eop, pTab) = {
        let e = db.ex(pExpr);
        (e.iTable, e.iColumn as i32, e.op, e.y.pTab.clone())
    };
    debug_assert!(db.arena.agg(pAggInfo).iFirstReg == 0);
    let mut found: Option<i32> = None;
    for (k, pCol) in db.arena.agg(pAggInfo).aCol.iter().enumerate() {
        if pCol.pCExpr == pExpr {
            return;
        }
        if pCol.iTable == eTable && pCol.iColumn == eColumn && eop != TK_IF_NULL_ROW {
            found = Some(k as i32);
            break;
        }
    }
    let k = match found {
        Some(k) => k,
        None => {
            let mut k = addAggInfoColumn(db, pAggInfo);
            if k > mxTerm {
                errmsg!(db, "more than %d aggregate terms", mxTerm);
                k = mxTerm;
            }
            let pGroupBy = db.arena.agg(pAggInfo).pGroupBy;
            let mut iSorterColumn = -1;
            if pGroupBy.nn() && eop != TK_IF_NULL_ROW {
                for j in 0..el_n(db, pGroupBy) {
                    let pE = db.ex(el_e(db, pGroupBy, j));
                    if pE.op == TK_COLUMN && pE.iTable == eTable && pE.iColumn as i32 == eColumn {
                        iSorterColumn = j;
                        break;
                    }
                }
            }
            let a = db.arena.aggm(pAggInfo);
            if iSorterColumn < 0 {
                iSorterColumn = a.nSortingColumn as i32;
                a.nSortingColumn += 1;
            }
            let pCol = &mut a.aCol[k as usize];
            pCol.pTab = pTab;
            pCol.iTable = eTable;
            pCol.iColumn = eColumn;
            pCol.pCExpr = pExpr;
            pCol.iSorterColumn = iSorterColumn;
            k
        }
    };
    /* fix_up_expr: */
    let e = db.exm(pExpr);
    debug_assert!(e.pAggInfo.is_null() || e.pAggInfo == pAggInfo);
    e.pAggInfo = pAggInfo;
    if e.op == TK_COLUMN {
        e.op = TK_AGG_COLUMN;
    }
    e.iAgg = k as i16;
}

/// This is the xExprCallback for a tree walker.  It is used to
/// implement sqlite3ExprAnalyzeAggregates().  See sqlite3ExprAnalyzeAggregates
/// for additional information.
fn analyzeAggregate(w: &mut Walker, pExpr: ExprId) -> i32 {
    let pNC = w.u.pNC;
    let nc = *w.db.arena.nc(pNC);
    let pSrcList = nc.pSrcList;
    let pAggInfo = nc.uNC.pAggInfo;

    debug_assert!((nc.ncFlags & NC_UAggInfo) != 0);
    debug_assert!(w.db.arena.agg(pAggInfo).iFirstReg == 0);
    let (op, op2, flags, iTable, ePAggInfo) = {
        let e = w.db.ex(pExpr);
        (e.op, e.op2, e.flags, e.iTable, e.pAggInfo)
    };
    match op {
        TK_IF_NULL_ROW | TK_AGG_COLUMN | TK_COLUMN => {
            /* Check to see if the column is in one of the tables in the FROM
             ** clause of the aggregate query */
            if pSrcList.nn() {
                let n = w.db.src(pSrcList).a.len();
                for i in 0..n {
                    debug_assert!((flags & (EP_TokenOnly | EP_Reduced)) == 0);
                    if iTable == w.db.src(pSrcList).a[i].iCursor {
                        findOrCreateAggInfoColumn(w.db, pAggInfo, pExpr);
                        break;
                    } /* endif pExpr->iTable==pItem->iCursor */
                } /* end loop over pSrcList */
            }
            WRC_Continue
        }
        TK_AGG_FUNCTION => {
            if (nc.ncFlags & NC_InAggFunc) == 0
                && w.walkerDepth == op2 as i32
                && ePAggInfo.is_null()
            {
                /* Check to see if pExpr is a duplicate of another aggregate
                 ** function that is already in the pAggInfo structure
                 */
                let mxTerm = w.db.aLimit[SQLITE_LIMIT_COLUMN as usize];
                let fexprs: Vec<ExprId> =
                    w.db.arena
                        .agg(pAggInfo)
                        .aFunc
                        .iter()
                        .map(|f| f.pFExpr)
                        .collect();
                let nFunc = fexprs.len() as i32;
                let mut i = 0;
                while i < nFunc {
                    let f = fexprs[i as usize];
                    if f == pExpr {
                        break;
                    }
                    if sqlite3ExprCompare(w.db, false, f, pExpr, -1) == 0 {
                        break;
                    }
                    i += 1;
                }
                if i > mxTerm {
                    errmsg!(w.db, "more than %d aggregate terms", mxTerm);
                    i = mxTerm;
                    debug_assert!(i < nFunc);
                } else if i >= nFunc {
                    /* pExpr is original.  Make a new entry in pAggInfo->aFunc[]
                     */
                    let enc = w.db.ENC();
                    i = addAggInfoFunc(w.db, pAggInfo);
                    if i >= 0 {
                        debug_assert!((flags & EP_xIsSelect) == 0);
                        let (pList, zToken, pLeft) = {
                            let e = w.db.ex(pExpr);
                            (e.x.pList, e.u.zToken.clone(), e.pLeft)
                        };
                        let nArg = el_n(w.db, pList);
                        let pFunc = crate::callback::sqlite3FindFunction(
                            w.db,
                            zToken.as_deref(),
                            nArg,
                            enc,
                            0,
                        );
                        let funcFlags = pFunc.as_ref().map(|f| f.funcFlags).unwrap_or(0);
                        let mut iOBTab = -1;
                        let mut bOBPayload = 0;
                        let mut bOBUnique = 0;
                        let mut bUseSubtype = 0;
                        if pLeft.nn() && (funcFlags & SQLITE_FUNC_NEEDCOLL) == 0 {
                            /* The NEEDCOLL test above causes any ORDER BY clause on
                             ** aggregate min() or max() to be ignored. */
                            debug_assert!(nArg > 0);
                            debug_assert!(w.db.ex(pLeft).op == TK_ORDER);
                            iOBTab = w.db.ppr().nTab;
                            w.db.pp().nTab += 1;
                            let pOBList = w.db.ex(pLeft).x.pList;
                            debug_assert!(el_n(w.db, pOBList) > 0);
                            if el_n(w.db, pOBList) == 1 && nArg == 1 && {
                                let (o0, a0) = (el_e(w.db, pOBList, 0), el_e(w.db, pList, 0));
                                sqlite3ExprCompare(w.db, false, o0, a0, -1) == 0
                            } {
                                bOBPayload = 0;
                                bOBUnique = ((flags & EP_Distinct) != 0) as u8;
                            } else {
                                bOBPayload = 1;
                            }
                            bUseSubtype = ((funcFlags & SQLITE_SUBTYPE) != 0) as u8;
                        }
                        let iDistinct = if (flags & EP_Distinct) != 0 && bOBUnique == 0 {
                            let t = w.db.ppr().nTab;
                            w.db.pp().nTab += 1;
                            t
                        } else {
                            -1
                        };
                        let pItem = &mut w.db.arena.aggm(pAggInfo).aFunc[i as usize];
                        pItem.pFExpr = pExpr;
                        pItem.pFunc = pFunc;
                        pItem.iOBTab = iOBTab;
                        pItem.bOBPayload = bOBPayload;
                        pItem.bOBUnique = bOBUnique;
                        pItem.bUseSubtype = bUseSubtype;
                        pItem.iDistinct = iDistinct;
                    }
                }
                /* Make pExpr point to the appropriate pAggInfo->aFunc[] entry
                 */
                debug_assert!((flags & (EP_TokenOnly | EP_Reduced)) == 0);
                let e = w.db.exm(pExpr);
                e.iAgg = i as i16;
                e.pAggInfo = pAggInfo;
                WRC_Prune
            } else {
                WRC_Continue
            }
        }
        _ => {
            debug_assert!(w.db.ppr().iSelfTab == 0);
            if (nc.ncFlags & NC_InAggFunc) == 0 {
                return WRC_Continue;
            }
            if w.db.ppr().pIdxEpr.is_empty() {
                return WRC_Continue;
            }
            let list: Vec<IndexedExpr> = w.db.ppr().pIdxEpr.clone();
            let mut pIEpr: Option<&IndexedExpr> = None;
            for p in list.iter().rev() {
                let iDataCur = p.iDataCur;
                if iDataCur < 0 {
                    continue;
                }
                if sqlite3ExprCompare(w.db, false, pExpr, p.pExpr, iDataCur) == 0 {
                    pIEpr = Some(p);
                    break;
                }
            }
            let Some(pIEpr) = pIEpr else {
                return WRC_Continue;
            };
            if !w.db.ex(pExpr).UseYTab() {
                return WRC_Continue;
            }
            let nSrc = if pSrcList.nn() {
                w.db.src(pSrcList).a.len()
            } else {
                0
            };
            let mut i = 0;
            while i < nSrc {
                if w.db.src(pSrcList).a[i].iCursor == pIEpr.iDataCur {
                    break;
                }
                i += 1;
            }
            if i >= nSrc {
                return WRC_Continue;
            }
            if ePAggInfo.nn() {
                return WRC_Continue; /* Resolved by outer context */
            }
            if nerr(w.db) != 0 {
                return WRC_Abort;
            }

            /* If we reach this point, it means that expression pExpr can be
             ** translated into a reference to an index column as described by
             ** pIEpr.
             */
            let tmp = tmp_expr(w.db, TK_AGG_COLUMN);
            {
                let t = w.db.exm(tmp);
                t.iTable = pIEpr.iIdxCur;
                t.iColumn = pIEpr.iIdxCol as ynVar;
            }
            findOrCreateAggInfoColumn(w.db, pAggInfo, tmp);
            if nerr(w.db) != 0 {
                return WRC_Abort;
            }
            let tAgg = w.db.ex(tmp).iAgg;
            debug_assert!((tAgg as i32) < w.db.arena.agg(pAggInfo).nColumn());
            w.db.arena.aggm(pAggInfo).aCol[tAgg as usize].pCExpr = pExpr;
            let e = w.db.exm(pExpr);
            e.pAggInfo = pAggInfo;
            e.iAgg = tAgg;
            WRC_Prune
        }
    }
}

/// Analyze the pExpr expression looking for aggregate functions and
/// for variables that need to be added to AggInfo object that pNC->pAggInfo
/// points to.  Additional entries are made on the AggInfo object as
/// necessary.
///
/// This routine should only be called after the expression has been
/// analyzed by sqlite3ResolveExprNames().
pub fn sqlite3ExprAnalyzeAggregates(db: &mut Sqlite3, pNC: NcId, pExpr: ExprId) {
    debug_assert!(db.arena.nc(pNC).pSrcList.nn());
    let mut w = Walker::new(db);
    w.xExprCallback = Some(analyzeAggregate);
    w.xSelectCallback = Some(sqlite3WalkerDepthIncrease);
    w.xSelectCallback2 = Some(sqlite3WalkerDepthDecrease);
    w.walkerDepth = 0;
    w.u.pNC = pNC;
    w.pParse = false;
    sqlite3WalkExpr(&mut w, pExpr);
}

/// Call sqlite3ExprAnalyzeAggregates() for every expression in an
/// expression list.  Return the number of errors.
///
/// If an error is found, the analysis is cut short.
pub fn sqlite3ExprAnalyzeAggList(db: &mut Sqlite3, pNC: NcId, pList: ExprListId) {
    if pList.nn() {
        for i in 0..el_n(db, pList) {
            let e = el_e(db, pList, i);
            sqlite3ExprAnalyzeAggregates(db, pNC, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Register allocation
// ---------------------------------------------------------------------------

/// Allocate a single new register for use to hold some intermediate result.
pub fn sqlite3GetTempReg(db: &mut Sqlite3) -> i32 {
    let pParse = db.pp();
    if pParse.nTempReg == 0 {
        pParse.nMem += 1;
        return pParse.nMem;
    }
    pParse.nTempReg -= 1;
    pParse.aTempReg[pParse.nTempReg as usize]
}

/// Deallocate a register, making available for reuse for some other
/// purpose.
pub fn sqlite3ReleaseTempReg(db: &mut Sqlite3, iReg: i32) {
    if iReg != 0 {
        let pParse = db.pp();
        debug_assert!(!pParse.aTempReg[..pParse.nTempReg as usize].contains(&iReg));
        if (pParse.nTempReg as usize) < pParse.aTempReg.len() {
            pParse.aTempReg[pParse.nTempReg as usize] = iReg;
            pParse.nTempReg += 1;
        }
    }
}

/// Allocate or deallocate a block of nReg consecutive registers.
pub fn sqlite3GetTempRange(db: &mut Sqlite3, nReg: i32) -> i32 {
    if nReg == 1 {
        return sqlite3GetTempReg(db);
    }
    let pParse = db.pp();
    let mut i = pParse.iRangeReg;
    let n = pParse.nRangeReg;
    if nReg <= n {
        pParse.iRangeReg += nReg;
        pParse.nRangeReg -= nReg;
    } else {
        i = pParse.nMem + 1;
        pParse.nMem += nReg;
    }
    i
}

/// C: `void sqlite3ReleaseTempRange(Parse *pParse, int iReg, int nReg)`
pub fn sqlite3ReleaseTempRange(db: &mut Sqlite3, iReg: i32, nReg: i32) {
    if nReg == 1 {
        sqlite3ReleaseTempReg(db, iReg);
        return;
    }
    let pParse = db.pp();
    if nReg > pParse.nRangeReg {
        pParse.nRangeReg = nReg;
        pParse.iRangeReg = iReg;
    }
}

/// Mark all temporary registers as being unavailable for reuse.
///
/// Always invoke this procedure after coding a subroutine or co-routine
/// that might be invoked from other parts of the code, to ensure that
/// the sub/co-routine does not use registers in common with the code that
/// invokes the sub/co-routine.
pub fn sqlite3ClearTempRegCache(db: &mut Sqlite3) {
    let pParse = db.pp();
    pParse.nTempReg = 0;
    pParse.nRangeReg = 0;
}

/// Make sure sufficient registers have been allocated so that
/// iReg is a valid register number.
pub fn sqlite3TouchRegister(db: &mut Sqlite3, iReg: i32) {
    let pParse = db.pp();
    if pParse.nMem < iReg {
        pParse.nMem = iReg;
    }
}

/// Return the latest reusable register in the set of all registers.
/// The value returned is no less than iMin.  If any register iMin or
/// greater is in permanent use, then return one more than that last
/// permanent register.
pub fn sqlite3FirstAvailableRegister(db: &mut Sqlite3, iMin: i32) -> i32 {
    let mut iMin = iMin;
    let pList = db.ppr().pConstExpr;
    if pList.nn() {
        for it in db.el(pList).a.iter() {
            if it.u.iConstExprReg >= iMin {
                iMin = it.u.iConstExprReg + 1;
            }
        }
    }
    let pParse = db.pp();
    pParse.nTempReg = 0;
    pParse.nRangeReg = 0;
    iMin
}

/// Validate that no temporary register falls within the range of
/// iFirst..iLast, inclusive.  This routine is only call from within assert()
/// statements.
pub fn sqlite3NoTempsInRange(db: &mut Sqlite3, iFirst: i32, iLast: i32) -> i32 {
    let pParse = db.ppr();
    if pParse.nRangeReg > 0
        && pParse.iRangeReg + pParse.nRangeReg > iFirst
        && pParse.iRangeReg <= iLast
    {
        return 0;
    }
    for i in 0..pParse.nTempReg as usize {
        if pParse.aTempReg[i] >= iFirst && pParse.aTempReg[i] <= iLast {
            return 0;
        }
    }
    let pList = pParse.pConstExpr;
    if pList.nn() {
        for it in db.el(pList).a.iter() {
            let iReg = it.u.iConstExprReg;
            if iReg == 0 {
                continue;
            }
            if iReg >= iFirst && iReg <= iLast {
                return 0;
            }
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dequote() {
        let mut z = b"\"a\"\"b\"".to_vec();
        crate::util::sqlite3Dequote(&mut z);
        assert_eq!(z, b"a\"b");
        let mut z = b"[x y]".to_vec();
        crate::util::sqlite3Dequote(&mut z);
        assert_eq!(z, b"x y");
        let mut z = b"'it''s'".to_vec();
        crate::util::sqlite3Dequote(&mut z);
        assert_eq!(z, b"it's");
        let mut z = b"plain".to_vec();
        crate::util::sqlite3Dequote(&mut z);
        assert_eq!(z, b"plain");
    }

    #[test]
    fn struct_size() {
        let mut e = Expr::default();
        assert_eq!(dupedExprStructSize(&e, 0), 0);
        assert_eq!(dupedExprStructSize(&e, EXPRDUP_REDUCE), EP_TokenOnly);
        e.pLeft = ExprId(1);
        assert_eq!(dupedExprStructSize(&e, EXPRDUP_REDUCE), EP_Reduced);
        e.flags = EP_FullSize;
        assert_eq!(dupedExprStructSize(&e, EXPRDUP_REDUCE), 0);
    }
}
