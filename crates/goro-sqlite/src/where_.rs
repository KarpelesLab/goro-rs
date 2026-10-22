//! Port of where.c (and the structures of whereInt.h).
//!
//! # Data structure design
//!
//! The C query planner is built out of objects that point at each other
//! freely (WhereTerm.pWC, WhereClause.pOuter, WhereOrInfo/WhereAndInfo
//! sub-clauses, WhereLoop.aLTerm[] pointing at terms, WherePath.aLoop[]
//! and WhereLevel.pWLoop pointing at loops).  In this port everything is
//! owned by the [`WhereInfo`] and addressed by index:
//!
//! * `WhereInfo::aWC` holds every WhereClause of the analysis.  Index 0
//!   is the top-level clause (C `pWInfo->sWC`); the sub-clauses of OR
//!   terms (C `WhereOrInfo.wc`) and of AND sub-terms of OR terms (C
//!   `WhereAndInfo.wc`) are appended to it.  A clause is designated by a
//!   [`WcId`] (its index), `WhereClause::pOuter` is an `Option<WcId>`.
//! * a WhereTerm is designated by a [`TermId`] = (clause id, index in
//!   that clause's `a` vector).  C pointer arithmetic such as
//!   `&pTerm[1]` becomes `TermId { i: t.i + 1, ..t }`, and
//!   `pTerm->pWC->a[pTerm->iParent]` becomes `TermId { wc: t.wc, i:
//!   iParent }`.  `WhereTerm::pWC` stores the id of the owning clause.
//! * `WhereTerm::u.pOrInfo` is an index into `WhereInfo::aOrInfo`
//!   (a [`WhereOrInfo`] = sub-clause id + `indexable` mask);
//!   `WhereTerm::u.pAndInfo` is directly the id of the AND sub-clause.
//! * `WhereTerm::pExpr` is an `ExprId` into `db.arena`: terms point INTO
//!   the WHERE clause tree; virtual terms are new arena nodes.
//! * all WhereLoop objects (C `pWInfo->pLoops` list) live in
//!   `WhereInfo::pLoops`, a Vec kept in the C list order (new loops are
//!   appended at the end of the list, replaced loops are overwritten in
//!   place and supplanted ones are removed, exactly as whereLoopInsert()
//!   does on the linked list).  Loop indices are only taken after all
//!   loops have been built (by the path solver), so they are stable.
//!   `WherePath::aLoop` and `WhereLevel::pWLoop` are indices into it.
//! * the WhereLoopBuilder owns its template loop (`pNew`); a sub-builder
//!   (OR processing) temporarily takes the template from its parent, so
//!   that both share it as in C.
//! * `WhereLevel::u.in.aInLoop` is a Vec (nIn is its length).
//!
//! Functions of where.c/wherecode.c/whereexpr.c that take a `WhereInfo*`
//! take `wi: &mut WhereInfo` plus the ids of the clause/level/term they
//! work on instead of the C pointers.

#![allow(unused_variables)]

use crate::expr::*;
use crate::int::*;
use crate::util::*;
use crate::vdbeaux::*;
use crate::wherecode::*;
use crate::whereexpr::*;
use std::rc::Rc;

/// Index of a WhereClause in `WhereInfo::aWC`.
pub type WcId = usize;

/// Identifies one WhereTerm: clause `wc`, term `i` of that clause.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct TermId {
    pub wc: u32,
    pub i: u32,
}

impl TermId {
    #[inline]
    pub fn new(wc: WcId, i: usize) -> TermId {
        TermId {
            wc: wc as u32,
            i: i as u32,
        }
    }
}

/// Extra information needed for RIGHT JOIN processing (WhereRightJoin).
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereRightJoin {
    pub iMatch: i32,
    pub regBloom: i32,
    pub regReturn: i32,
    pub addrSubrtn: i32,
    pub endSubrtn: i32,
}

/// Information about each nested IN operator (struct InLoop).
#[derive(Clone, Copy, Default, Debug)]
pub struct InLoop {
    pub iCur: i32,
    pub addrInTop: i32,
    pub iBase: i32,
    pub nPrefix: i32,
    pub eEndLoopOp: u8,
}

/// WhereLevel.u.in
#[derive(Clone, Default)]
pub struct WhereLevelIn {
    /// nIn is aInLoop.len()
    pub aInLoop: Vec<InLoop>,
}

impl WhereLevelIn {
    #[inline]
    pub fn nIn(&self) -> i32 {
        self.aInLoop.len() as i32
    }
}

/// WhereLevel.u (flattened union)
#[derive(Clone, Default)]
pub struct WhereLevelU {
    pub r#in: WhereLevelIn,
    pub pCoveringIdx: Option<IndexRef>,
}

/// For each nested loop in a WHERE clause implementation, the WhereInfo
/// structure contains a single instance of this structure.
#[derive(Clone, Default)]
pub struct WhereLevel {
    pub iLeftJoin: i32,
    pub iTabCur: i32,
    pub iIdxCur: i32,
    pub addrBrk: i32,
    pub addrHalt: i32,
    pub addrNxt: i32,
    pub addrSkip: i32,
    pub addrCont: i32,
    pub addrFirst: i32,
    pub addrBody: i32,
    pub regBignull: i32,
    pub addrBignull: i32,
    pub iLikeRepCntr: u32,
    pub addrLikeRep: i32,
    pub regFilter: i32,
    pub pRJ: Option<WhereRightJoin>,
    pub iFrom: u8,
    pub op: u8,
    pub p3: u8,
    pub p5: u8,
    pub p1: i32,
    pub p2: i32,
    pub u: WhereLevelU,
    /// Index of the selected WhereLoop in WhereInfo::pLoops.
    pub pWLoop: Option<usize>,
    pub notReady: Bitmask,
}

/// WhereLoop.u.btree
#[derive(Clone, Default)]
pub struct WhereLoopBtree {
    pub nEq: u16,
    pub nBtm: u16,
    pub nTop: u16,
    pub nDistinctCol: u16,
    pub pIndex: Option<IndexRef>,
    pub pOrderBy: ExprListId,
}

/// WhereLoop.u.vtab
#[derive(Clone, Default)]
pub struct WhereLoopVtab {
    pub idxNum: i32,
    pub needFree: u8,
    pub bOmitOffset: u8,
    pub bIdxNumHex: u8,
    pub isOrdered: i8,
    pub omitMask: u16,
    pub idxStr: Option<ZStr>,
    pub mHandleIn: u32,
}

/// WhereLoop.u (flattened union)
#[derive(Clone, Default)]
pub struct WhereLoopU {
    pub btree: WhereLoopBtree,
    pub vtab: WhereLoopVtab,
}

/// Each instance of this object represents an algorithm for evaluating one
/// term of a join.
#[derive(Clone, Default)]
pub struct WhereLoop {
    pub prereq: Bitmask,
    pub maskSelf: Bitmask,
    pub iTab: u8,
    pub iSortIdx: u8,
    pub rSetup: LogEst,
    pub rRun: LogEst,
    pub nOut: LogEst,
    pub u: WhereLoopU,
    pub wsFlags: u32,
    pub nLTerm: u16,
    pub nSkip: u16,
    /// The WhereTerms used (None entries for skip-scan columns).  The
    /// vector length is the C nLSlot; only the first nLTerm entries are
    /// meaningful.
    pub aLTerm: Vec<Option<TermId>>,
}

/// Number of entries in the C aLTermSpace[] array.
pub const N_LTERM_SPACE: usize = 3;

impl WhereLoop {
    /// The i-th used term (C `pLoop->aLTerm[i]`).
    #[inline]
    pub fn lterm(&self, i: usize) -> Option<TermId> {
        self.aLTerm.get(i).copied().flatten()
    }
    /// `pLoop->aLTerm[i] = t`, growing the array as needed.
    #[inline]
    pub fn set_lterm(&mut self, i: usize, t: Option<TermId>) {
        if self.aLTerm.len() <= i {
            self.aLTerm.resize(i + 1, None);
        }
        self.aLTerm[i] = t;
    }
}

/// This object holds the prerequisites and the cost of running a subquery
/// on one operand of an OR operator in the WHERE clause.
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereOrCost {
    pub prereq: Bitmask,
    pub rRun: LogEst,
    pub nOut: LogEst,
}

/// The WhereOrSet object holds a set of possible WhereOrCosts.
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereOrSet {
    pub n: u16,
    pub a: [WhereOrCost; N_OR_COST as usize],
}

/// Each instance of this object holds a sequence of WhereLoop objects that
/// implement some or all of a query plan.
#[derive(Clone, Default)]
pub struct WherePath {
    pub maskLoop: Bitmask,
    pub revLoop: Bitmask,
    pub nRow: LogEst,
    pub rCost: LogEst,
    pub rUnsort: LogEst,
    pub isOrdered: i8,
    /// Indices into WhereInfo::pLoops
    pub aLoop: Vec<usize>,
}

/// WhereTerm.u.x
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereTermX {
    pub leftColumn: i32,
    pub iField: i32,
}

/// WhereTerm.u (flattened union)
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereTermU {
    pub x: WhereTermX,
    /// Index into WhereInfo::aOrInfo when (eOperator & WO_OR)!=0
    pub pOrInfo: Option<usize>,
    /// Id of the AND sub-clause when (eOperator & WO_AND)!=0
    pub pAndInfo: Option<WcId>,
}

/// The query generator uses an array of instances of this structure to
/// help it analyze the subexpressions of the WHERE clause.
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereTerm {
    pub pExpr: ExprId,
    pub pWC: WcId,
    pub truthProb: LogEst,
    pub wtFlags: u16,
    pub eOperator: u16,
    pub nChild: u8,
    pub eMatchOp: u8,
    pub iParent: i32,
    pub leftCursor: i32,
    pub u: WhereTermU,
    pub prereqRight: Bitmask,
    pub prereqAll: Bitmask,
}

/// An instance of the WhereScan object is used as an iterator for locating
/// terms in the WHERE clause that are useful to the query planner.
#[derive(Clone, Default)]
pub struct WhereScan {
    pub pOrigWC: WcId,
    pub pWC: WcId,
    pub zCollName: Option<ZStr>,
    pub pIdxExpr: ExprId,
    pub k: usize,
    pub opMask: u32,
    pub idxaff: u8,
    pub iEquiv: u8,
    pub nEquiv: u8,
    pub aiCur: [i32; 11],
    pub aiColumn: [i16; 11],
}

/// An instance of the following structure holds all information about a
/// WHERE clause.
#[derive(Clone, Default)]
pub struct WhereClause {
    pub pOuter: Option<WcId>,
    pub op: u8,
    pub hasOr: u8,
    pub nBase: i32,
    /// nTerm is a.len()
    pub a: Vec<WhereTerm>,
}

impl WhereClause {
    #[inline]
    pub fn nTerm(&self) -> i32 {
        self.a.len() as i32
    }
}

/// A WhereTerm with eOperator==WO_OR has its u.pOrInfo pointer set to an
/// instance of the following structure.
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereOrInfo {
    /// Decomposition into subterms (a clause of WhereInfo::aWC)
    pub wc: WcId,
    pub indexable: Bitmask,
}

/// A WhereTerm with eOperator==WO_AND has its u.pAndInfo pointer set to
/// the id of the sub-clause (C struct WhereAndInfo { WhereClause wc; }).
#[derive(Clone, Copy, Default, Debug)]
pub struct WhereAndInfo {
    pub wc: WcId,
}

/// An instance of the following structure keeps track of a mapping between
/// VDBE cursor numbers and bits of the bitmasks in WhereTerm.
#[derive(Clone, Copy)]
pub struct WhereMaskSet {
    pub bVarSelect: i32,
    pub n: i32,
    pub ix: [i32; BMS as usize],
}

impl Default for WhereMaskSet {
    fn default() -> Self {
        WhereMaskSet {
            bVarSelect: 0,
            n: 0,
            ix: [0; BMS as usize],
        }
    }
}

/// This object is a convenience wrapper holding all information needed to
/// construct WhereLoop objects for a particular query.
#[derive(Clone, Default)]
pub struct WhereLoopBuilder {
    pub pWC: WcId,
    /// Template WhereLoop
    pub pNew: WhereLoop,
    /// Record best loops here, if not None
    pub pOrSet: Option<WhereOrSet>,
    pub bldFlags1: u8,
    pub bldFlags2: u8,
    pub iPlanLimit: u32,
}

/// The WHERE clause processing routine has two halves.  The first part
/// does the start of the WHERE loop and the second half does the tail of
/// the WHERE loop.  An instance of the following structure is returned by
/// the first half and passed into the second half.
#[derive(Clone, Default)]
pub struct WhereInfo {
    pub pTabList: SrcListId,
    pub pOrderBy: ExprListId,
    pub pResultSet: ExprListId,
    pub pSelect: SelectId,
    pub aiCurOnePass: [i32; 2],
    pub iContinue: i32,
    pub iBreak: i32,
    pub savedNQueryLoop: i32,
    pub wctrlFlags: u16,
    pub iLimit: LogEst,
    pub nLevel: u8,
    pub nOBSat: i8,
    pub eOnePass: u8,
    pub eDistinct: u8,
    pub bDeferredSeek: u8,
    pub untestedTerms: u8,
    pub bOrderedInnerLoop: u8,
    pub sorted: u8,
    pub bStarDone: u8,
    pub bStarUsed: u8,
    pub nRowOut: LogEst,
    pub iTop: i32,
    pub iEndWhere: i32,
    /// All WhereLoop objects, in the order of the C pLoops list.
    pub pLoops: Vec<WhereLoop>,
    pub revMask: Bitmask,
    /// All WhereClauses; aWC[0] is the C sWC.
    pub aWC: Vec<WhereClause>,
    /// WhereOrInfo objects (WhereTerm.u.pOrInfo indexes this).
    pub aOrInfo: Vec<WhereOrInfo>,
    pub sMaskSet: WhereMaskSet,
    /// Information about each nest loop (a.len() == nLevel).
    pub a: Vec<WhereLevel>,
    /// Index expression lists / partial index WHERE clauses imported into
    /// the arena (cache, keyed by the Index object address).
    /// (The Index is kept alive by the cache so that identities are stable.)
    pub idxTreeCache: Vec<(IndexRef, ExprListId, ExprId)>,
}

/// The id of the top-level clause (C pWInfo->sWC).
pub const SWC: WcId = 0;

impl WhereInfo {
    /// The term `t` (C `*pTerm`).
    #[inline]
    pub fn t(&self, t: TermId) -> &WhereTerm {
        &self.aWC[t.wc as usize].a[t.i as usize]
    }
    /// The term `t`, mutable.
    #[inline]
    pub fn tm(&mut self, t: TermId) -> &mut WhereTerm {
        &mut self.aWC[t.wc as usize].a[t.i as usize]
    }
    /// The clause `wc`.
    #[inline]
    pub fn wc(&self, wc: WcId) -> &WhereClause {
        &self.aWC[wc]
    }
    /// The clause `wc`, mutable.
    #[inline]
    pub fn wcm(&mut self, wc: WcId) -> &mut WhereClause {
        &mut self.aWC[wc]
    }
    /// The loop of level `i` (C `pWInfo->a[i].pWLoop`).
    #[inline]
    pub fn lp(&self, iLevel: usize) -> &WhereLoop {
        &self.pLoops[self.a[iLevel].pWLoop.expect("level without loop")]
    }
    /// The loop of level `i`, mutable.
    #[inline]
    pub fn lpm(&mut self, iLevel: usize) -> &mut WhereLoop {
        let i = self.a[iLevel].pWLoop.expect("level without loop");
        &mut self.pLoops[i]
    }
    /// The WhereOrInfo of OR term `t`.
    #[inline]
    pub fn orinfo(&self, t: TermId) -> WhereOrInfo {
        self.aOrInfo[self.t(t).u.pOrInfo.expect("not an OR term")]
    }
}

/// C `pX==pY` for two `Index*` values.
#[inline]
pub(crate) fn idx_eq(a: &Option<IndexRef>, b: &Option<IndexRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// The ExprList of the index expressions of `pIdx` (C `pIdx->aColExpr`),
/// as an id in db.arena (owned trees are imported once per WhereInfo).
pub(crate) fn idx_col_exprs(db: &mut Sqlite3, wi: &mut WhereInfo, pIdx: &IndexRef) -> ExprListId {
    for e in wi.idxTreeCache.iter() {
        if Rc::ptr_eq(&e.0, pIdx) {
            return e.1;
        }
    }
    let (t1, t2) = {
        let p = pIdx.borrow();
        (p.aColExpr.clone(), p.pPartIdxWhere.clone())
    };
    let l = db.tree(&t1);
    let w = db.tree(&t2);
    wi.idxTreeCache.push((pIdx.clone(), l, w));
    l
}

/// The partial index WHERE clause of `pIdx` (C `pIdx->pPartIdxWhere`) as
/// an id in db.arena.
pub(crate) fn idx_part_where(db: &mut Sqlite3, wi: &mut WhereInfo, pIdx: &IndexRef) -> ExprId {
    idx_col_exprs(db, wi, pIdx);
    for e in wi.idxTreeCache.iter() {
        if Rc::ptr_eq(&e.0, pIdx) {
            return e.2;
        }
    }
    ExprId::NULL
}

/// `pIdx->aColExpr->a[j].pExpr`
pub(crate) fn idx_col_expr(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pIdx: &IndexRef,
    j: i32,
) -> ExprId {
    let l = idx_col_exprs(db, wi, pIdx);
    if l.is_null() {
        return ExprId::NULL;
    }
    let el = db.el(l);
    if (j as usize) < el.a.len() {
        el.a[j as usize].pExpr
    } else {
        ExprId::NULL
    }
}

/// The table of FROM-clause item `it` (C `pItem->pSTab`).
#[inline]
pub(crate) fn item_tab(db: &Sqlite3, it: SrcItemId) -> TableRef {
    db.item(it).pSTab.clone().expect("FROM item without table")
}

/// Run `f` with the Vdbe under construction taken out of the parse, for
/// the functions that need both `db` and a `&mut Vdbe` (C code that
/// passes pParse->pVdbe to a function that also uses v->db).
pub(crate) fn with_vdbe<R>(
    db: &mut Sqlite3,
    f: impl FnOnce(&mut Sqlite3, &mut Vdbe) -> R,
) -> Option<R> {
    let mut v = db.pp().pVdbe.take()?;
    let r = f(db, &mut v);
    db.pp().pVdbe = Some(v);
    Some(r)
}

/// sqlite3IsBinary() accepting a NULL collating sequence.
#[inline]
pub(crate) fn isBinary(p: &Option<CollSeqRef>) -> bool {
    match p {
        None => true,
        Some(c) => crate::mainapi::sqlite3IsBinary(Some(c)) != 0,
    }
}

/// The name of a collating sequence returned by one of the
/// sqlite3...CollSeq() functions (C `pColl ? pColl->zName : "BINARY"`).
#[inline]
pub(crate) fn coll_name(p: &Option<CollSeqRef>) -> ZStr {
    match p {
        Some(c) => c.zName.clone(),
        None => sqlite3StrBINARY.to_vec(),
    }
}

pub use crate::global::sqlite3StrBINARY;

// ---------------------------------------------------------------------------
// Public accessors
// ---------------------------------------------------------------------------

/// Return the estimated number of output rows from a WHERE clause
///
/// C: `LogEst sqlite3WhereOutputRowCount(WhereInfo *pWInfo)`
pub fn sqlite3WhereOutputRowCount(pWInfo: &mut crate::where_::WhereInfo) -> LogEst {
    pWInfo.nRowOut
}

/// Return one of the WHERE_DISTINCT_xxxxx values to indicate how this
/// WHERE clause returns outputs for DISTINCT processing.
///
/// C: `int sqlite3WhereIsDistinct(WhereInfo *pWInfo)`
pub fn sqlite3WhereIsDistinct(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    pWInfo.eDistinct as i32
}

/// Return the number of ORDER BY terms that are satisfied by the
/// WHERE clause.  A return of 0 means that the output must be
/// completely sorted.  A return equal to the number of ORDER BY
/// terms means that no sorting is needed at all.  A return that
/// is positive but less than the number of ORDER BY terms means that
/// block sorting is required.
///
/// C: `int sqlite3WhereIsOrdered(WhereInfo *pWInfo)`
pub fn sqlite3WhereIsOrdered(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    if pWInfo.nOBSat < 0 {
        0
    } else {
        pWInfo.nOBSat as i32
    }
}

/// In the ORDER BY LIMIT optimization, if the inner-most loop is known
/// to emit rows in increasing order, and if the last row emitted by the
/// inner-most loop did not fit within the sorter, then we can skip all
/// subsequent rows for the current iteration of the inner loop (because
/// they will not fit in the sorter either) and continue with the second
/// inner loop - the loop immediately outside the inner-most.
///
/// When a row does not fit in the sorter (because the sorter already
/// holds LIMIT+OFFSET rows that are smaller), then a jump is made to the
/// label returned by this function.
///
/// If the ORDER BY LIMIT optimization applies, the jump destination should
/// be the continuation for the second-inner-most loop.  If the ORDER BY
/// LIMIT optimization does not apply, then the jump destination should
/// be the continuation for the inner-most loop.
///
/// C: `int sqlite3WhereOrderByLimitOptLabel(WhereInfo *pWInfo)`
pub fn sqlite3WhereOrderByLimitOptLabel(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    if pWInfo.bOrderedInnerLoop == 0 {
        // The ORDER BY LIMIT optimization does not apply.  Jump to the
        // continuation of the inner-most loop.
        return pWInfo.iContinue;
    }
    let pInner = &pWInfo.a[pWInfo.nLevel as usize - 1];
    debug_assert!(pInner.addrNxt != 0);
    if pInner.pRJ.is_some() {
        pWInfo.iContinue
    } else {
        pInner.addrNxt
    }
}

/// While generating code for the min/max optimization, after handling
/// the aggregate-step call to min() or max(), check to see if any
/// additional looping is required.  If the output order is such that
/// we are certain that the correct answer has already been found, then
/// code an OP_Goto to by pass subsequent processing.
///
/// Any extra OP_Goto that is coded here is an optimization.  The
/// correct answer should be obtained regardless.  This OP_Goto just
/// makes the answer appear faster.
///
/// C: `void sqlite3WhereMinMaxOptEarlyOut(Vdbe *v, WhereInfo *pWInfo)`
pub fn sqlite3WhereMinMaxOptEarlyOut(v: &mut Vdbe, pWInfo: &mut crate::where_::WhereInfo) {
    if pWInfo.bOrderedInnerLoop == 0 {
        return;
    }
    if pWInfo.nOBSat == 0 {
        return;
    }
    let mut i = pWInfo.nLevel as i32 - 1;
    while i >= 0 {
        if (pWInfo.lp(i as usize).wsFlags & WHERE_COLUMN_IN) != 0 {
            sqlite3VdbeGoto(v, pWInfo.a[i as usize].addrNxt);
            return;
        }
        i -= 1;
    }
    sqlite3VdbeGoto(v, pWInfo.iBreak);
}

/// Return the VDBE address or label to jump to in order to continue
/// immediately with the next row of a WHERE clause.
///
/// C: `int sqlite3WhereContinueLabel(WhereInfo *pWInfo)`
pub fn sqlite3WhereContinueLabel(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    debug_assert!(pWInfo.iContinue != 0);
    pWInfo.iContinue
}

/// Return the VDBE address or label to jump to in order to break
/// out of a WHERE loop.
///
/// C: `int sqlite3WhereBreakLabel(WhereInfo *pWInfo)`
pub fn sqlite3WhereBreakLabel(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    pWInfo.iBreak
}

/// Return ONEPASS_OFF (0) if an UPDATE or DELETE statement is unable to
/// operate directly on the rowids returned by a WHERE clause.  Return
/// ONEPASS_SINGLE (1) if the statement can operation directly because only
/// a single row is to be changed.  Return ONEPASS_MULTI (2) if the one-pass
/// optimization can be used on multiple
///
/// If the ONEPASS optimization is used (if this routine returns true)
/// then also write the indices of open cursors used by ONEPASS
/// into aiCur[0] and aiCur[1].  iaCur[0] gets the cursor of the data
/// table and aiCur[1] gets the cursor used by an auxiliary index.
/// Either value may be -1, indicating that cursor is not used.
/// Any cursors returned will have been opened for writing.
///
/// aiCur[0] and aiCur[1] both get -1 if the where-clause logic is
/// unable to use the ONEPASS optimization.
///
/// C: `int sqlite3WhereOkOnePass(WhereInfo *pWInfo, int *aiCur)`
pub fn sqlite3WhereOkOnePass(pWInfo: &mut crate::where_::WhereInfo, aiCur: &mut [i32; 2]) -> i32 {
    *aiCur = pWInfo.aiCurOnePass;
    pWInfo.eOnePass as i32
}

/// Return TRUE if the WHERE loop uses the OP_DeferredSeek opcode to move
/// the data cursor to the row selected by the index cursor.
///
/// C: `int sqlite3WhereUsesDeferredSeek(WhereInfo *pWInfo)`
pub fn sqlite3WhereUsesDeferredSeek(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    pWInfo.bDeferredSeek as i32
}

/// Move the content of pSrc into pDest
fn whereOrMove(pDest: &mut WhereOrSet, pSrc: &WhereOrSet) {
    pDest.n = pSrc.n;
    for i in 0..pDest.n as usize {
        pDest.a[i] = pSrc.a[i];
    }
}

/// Try to insert a new prerequisite/cost entry into the WhereOrSet pSet.
///
/// The new entry might overwrite an existing entry, or it might be
/// appended, or it might be discarded.  Do whatever is the right thing
/// so that pSet keeps the N_OR_COST best entries seen so far.
fn whereOrInsert(pSet: &mut WhereOrSet, prereq: Bitmask, rRun: LogEst, nOut: LogEst) -> i32 {
    let mut found: Option<usize> = None;
    for i in 0..pSet.n as usize {
        let p = pSet.a[i];
        if rRun <= p.rRun && (prereq & p.prereq) == prereq {
            found = Some(i);
            break;
        }
        if p.rRun <= rRun && (p.prereq & prereq) == p.prereq {
            return 0;
        }
    }
    let ip: usize = match found {
        Some(i) => i,
        None => {
            if (pSet.n as i32) < N_OR_COST {
                let i = pSet.n as usize;
                pSet.n += 1;
                pSet.a[i].nOut = nOut;
                i
            } else {
                let mut ip = 0usize;
                for i in 1..pSet.n as usize {
                    if pSet.a[ip].rRun > pSet.a[i].rRun {
                        ip = i;
                    }
                }
                if pSet.a[ip].rRun <= rRun {
                    return 0;
                }
                ip
            }
        }
    };
    // whereOrInsert_done:
    let p = &mut pSet.a[ip];
    p.prereq = prereq;
    p.rRun = rRun;
    if p.nOut > nOut {
        p.nOut = nOut;
    }
    1
}

/// Return the bitmask for the given cursor number.  Return 0 if
/// iCursor is not in the set.
///
/// C: `Bitmask sqlite3WhereGetMask(WhereMaskSet *pMaskSet, int iCursor)`
pub fn sqlite3WhereGetMask(pMaskSet: &mut crate::where_::WhereMaskSet, iCursor: i32) -> Bitmask {
    debug_assert!(pMaskSet.n <= BMS);
    debug_assert!(pMaskSet.n > 0 || pMaskSet.ix[0] < 0);
    if pMaskSet.ix[0] == iCursor {
        return 1;
    }
    for i in 1..pMaskSet.n {
        if pMaskSet.ix[i as usize] == iCursor {
            return MASKBIT(i);
        }
    }
    0
}

/// Allocate memory that is automatically freed when pWInfo is freed.
/// (Rust: plain heap memory; kept for API completeness.)
///
/// C: `void * sqlite3WhereMalloc(WhereInfo *pWInfo, u64 nByte)`
pub fn sqlite3WhereMalloc(
    db: &mut Sqlite3,
    pWInfo: &mut crate::where_::WhereInfo,
    nByte: u64,
) -> Option<Vec<u8>> {
    Some(vec![0u8; nByte as usize])
}

/// C: `void * sqlite3WhereRealloc(WhereInfo *pWInfo, void *pOld, u64 nByte)`
pub fn sqlite3WhereRealloc(
    pWInfo: &mut crate::where_::WhereInfo,
    pOld: &mut [u8],
    nByte: u64,
) -> Option<Vec<u8>> {
    let mut v = vec![0u8; nByte as usize];
    let n = pOld.len().min(v.len());
    v[..n].copy_from_slice(&pOld[..n]);
    Some(v)
}

/// Create a new mask for cursor iCursor.
///
/// There is one cursor per table in the FROM clause.  The number of
/// tables in the FROM clause is limited by a test early in the
/// sqlite3WhereBegin() routine.  So we know that the pMaskSet->ix[]
/// array will never overflow.
fn createMask(pMaskSet: &mut WhereMaskSet, iCursor: i32) {
    debug_assert!((pMaskSet.n as usize) < pMaskSet.ix.len());
    pMaskSet.ix[pMaskSet.n as usize] = iCursor;
    pMaskSet.n += 1;
}

/// If the right-hand branch of the expression is a TK_COLUMN, then return
/// a pointer to the right-hand branch.  Otherwise, return NULL.
fn whereRightSubexprIsColumn(db: &mut Sqlite3, p: ExprId) -> ExprId {
    let r = db.ex(p).pRight;
    let p = sqlite3ExprSkipCollateAndLikely(db, r);
    if p.nn() && db.ex(p).op == TK_COLUMN && !db.ex(p).HasProperty(EP_FixedCol) {
        return p;
    }
    ExprId::NULL
}

/// Term pTerm is guaranteed to be a WO_IN term. It may be a component term
/// of a vector IN expression of the form "(x, y, ...) IN (SELECT ...)".
/// This function checks to see if the term is compatible with an index
/// column with affinity idxaff (one of the SQLITE_AFF_XYZ values). If so,
/// it returns a pointer to the name of the collation sequence (e.g. "BINARY"
/// or "NOCASE") used by the comparison in pTerm. If it is not compatible
/// with affinity idxaff, NULL is returned.
fn indexInAffinityOk(db: &mut Sqlite3, pTerm: &WhereTerm, idxaff: u8) -> Option<ZStr> {
    let mut pX = pTerm.pExpr;

    debug_assert!(pTerm.eOperator & WO_IN != 0);

    let l = db.ex(pX).pLeft;
    if sqlite3ExprIsVector(db, l) != 0 {
        let iField = (pTerm.u.x.iField - 1) as usize;
        let ll = db.ex(l).x.pList;
        let lhs = db.el(ll).a[iField].pExpr;
        let sel = db.ex(pX).x.pSelect;
        let el = db.sel(sel).pEList;
        let rhs = db.el(el).a[iField].pExpr;
        // A transient "inexpr" (C uses a stack Expr)
        pX = db.arena.new_expr(Expr {
            op: TK_EQ,
            flags: 0,
            pLeft: lhs,
            pRight: rhs,
            ..Default::default()
        });
    }

    if sqlite3IndexAffinityOk(db, pX, idxaff) != 0 {
        let pRet = sqlite3ExprCompareCollSeq(db, pX);
        return Some(coll_name(&pRet));
    }
    None
}

/// Advance to the next WhereTerm that matches according to the criteria
/// established when the pScan object was initialized by whereScanInit().
/// Return NULL if there are no more matching WhereTerms.
pub(crate) fn whereScanNext(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pScan: &mut WhereScan,
) -> Option<TermId> {
    let mut k = pScan.k; // Where to start scanning

    debug_assert!(pScan.iEquiv <= pScan.nEquiv);
    let mut pWC: Option<WcId> = Some(pScan.pWC);
    loop {
        let iColumn = pScan.aiColumn[pScan.iEquiv as usize - 1];
        let iCur = pScan.aiCur[pScan.iEquiv as usize - 1];
        debug_assert!(pWC.is_some());
        debug_assert!(iCur >= 0);
        loop {
            let wc = pWC.unwrap();
            while k < wi.wc(wc).a.len() {
                let pTerm = TermId::new(wc, k);
                let t = *wi.t(pTerm);
                debug_assert!((t.eOperator & (WO_OR | WO_AND)) == 0 || t.leftCursor < 0);
                if t.leftCursor == iCur
                    && t.u.x.leftColumn == iColumn as i32
                    && (iColumn != XN_EXPR || {
                        let l = db.ex(t.pExpr).pLeft;
                        sqlite3ExprCompareSkip(db, l, pScan.pIdxExpr, iCur) == 0
                    })
                    && (pScan.iEquiv <= 1 || !db.ex(t.pExpr).HasProperty(EP_OuterON))
                {
                    if (t.eOperator & WO_EQUIV) != 0 && (pScan.nEquiv as usize) < pScan.aiCur.len()
                    {
                        let pX = whereRightSubexprIsColumn(db, t.pExpr);
                        if pX.nn() {
                            let (xt, xc) = (db.ex(pX).iTable, db.ex(pX).iColumn);
                            let mut j = 0usize;
                            while j < pScan.nEquiv as usize {
                                if pScan.aiCur[j] == xt && pScan.aiColumn[j] == xc {
                                    break;
                                }
                                j += 1;
                            }
                            if j == pScan.nEquiv as usize {
                                pScan.aiCur[j] = xt;
                                pScan.aiColumn[j] = xc;
                                pScan.nEquiv += 1;
                            }
                        }
                    }
                    if (t.eOperator as u32 & pScan.opMask) != 0 {
                        // Verify the affinity and collating sequence match
                        if pScan.zCollName.is_some() && (t.eOperator & WO_ISNULL) == 0 {
                            let zCollName: ZStr;
                            let pX = t.pExpr;

                            if t.eOperator & WO_IN != 0 {
                                match indexInAffinityOk(db, &t, pScan.idxaff) {
                                    Some(z) => zCollName = z,
                                    None => {
                                        k += 1;
                                        continue;
                                    }
                                }
                            } else {
                                if sqlite3IndexAffinityOk(db, pX, pScan.idxaff) == 0 {
                                    k += 1;
                                    continue;
                                }
                                debug_assert!(db.ex(pX).pLeft.nn());
                                let pColl = sqlite3ExprCompareCollSeq(db, pX);
                                zCollName = coll_name(&pColl);
                            }

                            if sqlite3StrICmp(Some(&zCollName[..]), pScan.zCollName.as_deref()) != 0
                            {
                                k += 1;
                                continue;
                            }
                        }
                        if (t.eOperator & (WO_EQ | WO_IS)) != 0 {
                            let pX = db.ex(t.pExpr).pRight;
                            if pX.nn() {
                                let x = db.ex(pX);
                                if x.op == TK_COLUMN
                                    && x.iTable == pScan.aiCur[0]
                                    && x.iColumn == pScan.aiColumn[0]
                                {
                                    k += 1;
                                    continue;
                                }
                            }
                        }
                        pScan.pWC = wc;
                        pScan.k = k + 1;
                        return Some(pTerm);
                    }
                }
                k += 1;
            }
            pWC = wi.wc(wc).pOuter;
            k = 0;
            if pWC.is_none() {
                break;
            }
        }
        if pScan.iEquiv >= pScan.nEquiv {
            break;
        }
        pWC = Some(pScan.pOrigWC);
        k = 0;
        pScan.iEquiv += 1;
    }
    None
}

/// This is whereScanInit() for the case of an index on an expression.
/// It is factored out into a separate tail-recursion subroutine so that
/// the normal whereScanInit() routine, which is a high-runner, does not
/// need to push registers onto the stack as part of its prologue.
fn whereScanInitIndexExpr(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pScan: &mut WhereScan,
) -> Option<TermId> {
    pScan.idxaff = sqlite3ExprAffinity(db, pScan.pIdxExpr);
    whereScanNext(db, wi, pScan)
}

/// Initialize a WHERE clause scanner object.  Return a pointer to the
/// first match.  Return NULL if there are no matches.
///
/// The scanner will be searching the WHERE clause pWC.  It will look
/// for terms of the form "X <op> <expr>" where X is column iColumn of table
/// iCur.   Or if pIdx!=0 then X is column iColumn of index pIdx.  pIdx
/// must be one of the indexes of table iCur.
///
/// The <op> must be one of the operators described by opMask.
///
/// If the search is for X and the WHERE clause contains terms of the
/// form X=Y then this routine might also return terms of the form
/// "Y <op> <expr>".  The number of levels of transitivity is limited,
/// but is enough to handle most commonly occurring SQL statements.
///
/// If X is not the INTEGER PRIMARY KEY then X must be compatible with
/// index pIdx.
pub(crate) fn whereScanInit(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pScan: &mut WhereScan,
    pWC: WcId,
    iCur: i32,
    iColumn: i32,
    opMask: u32,
    pIdx: Option<&IndexRef>,
) -> Option<TermId> {
    let mut iColumn = iColumn;
    pScan.pOrigWC = pWC;
    pScan.pWC = pWC;
    pScan.pIdxExpr = ExprId::NULL;
    pScan.idxaff = 0;
    pScan.zCollName = None;
    pScan.opMask = opMask;
    pScan.k = 0;
    pScan.aiCur[0] = iCur;
    pScan.nEquiv = 1;
    pScan.iEquiv = 1;
    if let Some(pIdx) = pIdx {
        let j = iColumn;
        let (col, iPKey, tab, coll) = {
            let p = pIdx.borrow();
            let t = p.table();
            let iPKey = t.borrow().iPKey;
            (
                p.aiColumn[j as usize] as i32,
                iPKey as i32,
                t,
                p.azColl.get(j as usize).cloned(),
            )
        };
        iColumn = col;
        if iColumn == iPKey {
            iColumn = XN_ROWID as i32;
        } else if iColumn >= 0 {
            pScan.idxaff = tab.borrow().aCol[iColumn as usize].affinity;
            pScan.zCollName = coll;
        } else if iColumn == XN_EXPR as i32 {
            pScan.pIdxExpr = idx_col_expr(db, wi, pIdx, j);
            pScan.zCollName = coll;
            pScan.aiColumn[0] = XN_EXPR;
            return whereScanInitIndexExpr(db, wi, pScan);
        }
    } else if iColumn == XN_EXPR as i32 {
        return None;
    }
    pScan.aiColumn[0] = iColumn as i16;
    whereScanNext(db, wi, pScan)
}

/// Search for a term in the WHERE clause that is of the form "X <op> <expr>"
/// where X is a reference to the iColumn of table iCur or of index pIdx
/// if pIdx!=0 and <op> is one of the WO_xx operator codes specified by
/// the op parameter.  Return a pointer to the term.  Return 0 if not found.
///
/// If pIdx!=0 then it must be one of the indexes of table iCur.
/// Search for terms matching the iColumn-th column of pIdx
/// rather than the iColumn-th column of table iCur.
///
/// The term returned might by Y=<expr> if there is another constraint in
/// the WHERE clause that specifies that X=Y.  Any such constraints will be
/// identified by the WO_EQUIV bit in the pTerm->eOperator field.  The
/// aiCur[]/iaColumn[] arrays hold X and all its equivalents. There are 11
/// slots in aiCur[]/aiColumn[] so that means we can look for X plus up to 10
/// other equivalent values.  Hence a search for X will return <expr> if X=A1
/// and A1=A2 and A2=A3 and ... and A9=A10 and A10=<expr>.
///
/// If there are multiple terms in the WHERE clause of the form "X <op> <expr>"
/// then try for the one with no dependencies on <expr> - in other words where
/// <expr> is a constant expression of some kind.  Only return entries of
/// the form "X <op> Y" where Y is a column in another table if no terms of
/// the form "X <op> <const-expr>" exist.   If no terms with a constant RHS
/// exist, try to return a term that does not use WO_EQUIV.
///
/// C: `WhereTerm * sqlite3WhereFindTerm( WhereClause *pWC, int iCur, int iColumn, Bitmask notReady, u32 op, Index *pIdx )`
pub fn sqlite3WhereFindTerm(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pWC: WcId,
    iCur: i32,
    iColumn: i32,
    notReady: Bitmask,
    op: u32,
    pIdx: Option<&IndexRef>,
) -> Option<TermId> {
    let mut pResult: Option<TermId> = None;
    let mut scan = WhereScan::default();

    let mut p = whereScanInit(db, wi, &mut scan, pWC, iCur, iColumn, op, pIdx);
    let op = op & (WO_EQ | WO_IS) as u32;
    while let Some(pt) = p {
        let t = *wi.t(pt);
        if (t.prereqRight & notReady) == 0 {
            if t.prereqRight == 0 && (t.eOperator as u32 & op) != 0 {
                return Some(pt);
            }
            if pResult.is_none() {
                pResult = Some(pt);
            }
        }
        p = whereScanNext(db, wi, &mut scan);
    }
    pResult
}

/// This function searches pList for an entry that matches the iCol-th
/// column of index pIdx.
///
/// If such an expression is found, its index in pList->a[] is returned. If
/// no expression is found, -1 is returned.
fn findIndexCol(
    db: &mut Sqlite3,
    pList: ExprListId,
    iBase: i32,
    pIdx: &IndexRef,
    iCol: i32,
) -> i32 {
    let (zColl, aic) = {
        let p = pIdx.borrow();
        (p.azColl[iCol as usize].clone(), p.aiColumn[iCol as usize])
    };
    let n = db.el(pList).nExpr();
    for i in 0..n {
        let e = db.el(pList).a[i as usize].pExpr;
        let p = sqlite3ExprSkipCollateAndLikely(db, e);
        if p.nn() && {
            let x = db.ex(p);
            (x.op == TK_COLUMN || x.op == TK_AGG_COLUMN) && x.iColumn == aic && x.iTable == iBase
        } {
            let pColl = sqlite3ExprNNCollSeq(db, e);
            if 0 == sqlite3StrICmp(Some(&coll_name(&pColl)[..]), Some(&zColl[..])) {
                return i;
            }
        }
    }
    -1
}

/// Return TRUE if the iCol-th column of index pIdx is NOT NULL
fn indexColumnNotNull(pIdx: &IndexRef, iCol: i32) -> i32 {
    let p = pIdx.borrow();
    debug_assert!(iCol >= 0 && iCol < p.nColumn as i32);
    let j = p.aiColumn[iCol as usize];
    if j >= 0 {
        p.table().borrow().aCol[j as usize].notNull as i32
    } else if j == -1 {
        1
    } else {
        debug_assert!(j == -2);
        0 // Assume an indexed expression can always yield a NULL
    }
}

/// Return true if the DISTINCT expression-list passed as the third argument
/// is redundant.
///
/// A DISTINCT list is redundant if any subset of the columns in the
/// DISTINCT list are collectively unique and individually non-null.
fn isDistinctRedundant(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pTabList: SrcListId,
    pWC: WcId,
    pDistinct: ExprListId,
) -> i32 {
    // If there is more than one table or sub-select in the FROM clause of
    // this query, then it will not be possible to show that the DISTINCT
    // clause is redundant.
    if db.src(pTabList).nSrc() != 1 {
        return 0;
    }
    let iBase = db.src(pTabList).a[0].iCursor;
    let pTab = item_tab(db, SrcItemId(pTabList, 0));

    // If any of the expressions is an IPK column on table iBase, then return
    // true. Note: The (p->iTable==iBase) part of this test may be false if
    // the current SELECT is a correlated sub-query.
    let n = db.el(pDistinct).nExpr();
    for i in 0..n as usize {
        let e = db.el(pDistinct).a[i].pExpr;
        let p = sqlite3ExprSkipCollateAndLikely(db, e);
        if p.is_null() {
            continue;
        }
        let x = db.ex(p);
        if x.op != TK_COLUMN && x.op != TK_AGG_COLUMN {
            continue;
        }
        if x.iTable == iBase && x.iColumn < 0 {
            return 1;
        }
    }

    // Loop through all indices on the table, checking each to see if it
    // makes the DISTINCT qualifier redundant. It does so if:
    //
    //   1. The index is itself UNIQUE, and
    //
    //   2. All of the columns in the index are either part of the pDistinct
    //      list, or else the WHERE clause contains a term of the form
    //      "col=X", where X is a constant value. The collation sequences of
    //      the comparison and select-list expressions must match those of
    //      the index.
    //
    //   3. All of those index columns for which the WHERE clause does not
    //      contain a "col=X" term are subject to a NOT NULL constraint.
    let idxs: Vec<IndexRef> = pTab.borrow().pIndex.clone();
    for pIdx in idxs.iter() {
        if !pIdx.borrow().IsUniqueIndex() {
            continue;
        }
        if pIdx.borrow().pPartIdxWhere.is_some() {
            continue;
        }
        let nKeyCol = pIdx.borrow().nKeyCol as i32;
        let mut i = 0;
        while i < nKeyCol {
            if sqlite3WhereFindTerm(db, wi, pWC, iBase, i, !0, WO_EQ as u32, Some(pIdx)).is_none() {
                if findIndexCol(db, pDistinct, iBase, pIdx, i) < 0 {
                    break;
                }
                if indexColumnNotNull(pIdx, i) == 0 {
                    break;
                }
            }
            i += 1;
        }
        if i == nKeyCol {
            // This index implies that the DISTINCT qualifier is redundant.
            return 1;
        }
    }

    0
}

/// Estimate the logarithm of the input value to base 2.
fn estLog(N: LogEst) -> LogEst {
    if N <= 10 {
        0
    } else {
        sqlite3LogEst(N as u64) - 33
    }
}

/// Convert OP_Column opcodes to OP_Copy in previously generated code.
///
/// This routine runs over generated VDBE code and translates OP_Column
/// opcodes into OP_Copy when the table is being accessed via co-routine
/// instead of via table lookup.
///
/// If the iAutoidxCur is not zero, then any OP_Rowid instructions on
/// cursor iTabCur are transformed into OP_Sequence opcode for the
/// iAutoidxCur cursor, in order to generate unique rowids for the
/// automatic index being generated.
fn translateColumnToCopy(
    db: &mut Sqlite3,
    iStart: i32,
    iTabCur: i32,
    iRegister: i32,
    iAutoidxCur: i32,
) {
    if db.mallocFailed != 0 {
        return;
    }
    let v = db.v();
    let iEnd = sqlite3VdbeCurrentAddr(v);
    let mut i = iStart;
    while i < iEnd {
        let pOp = &mut v.aOp[i as usize];
        i += 1;
        if pOp.p1 != iTabCur {
            continue;
        }
        if pOp.opcode == OP_Column {
            pOp.opcode = OP_Copy;
            pOp.p1 = pOp.p2 + iRegister;
            pOp.p2 = pOp.p3;
            pOp.p3 = 0;
            pOp.p5 = 2; // Cause the MEM_Subtype flag to be cleared
        } else if pOp.opcode == OP_Rowid {
            pOp.opcode = OP_Sequence;
            pOp.p1 = iAutoidxCur;
        }
    }
}

/// We know that pSrc is an operand of an outer join.  Return true if
/// pTerm is a constraint that is compatible with that join.
///
/// pTerm must be EP_OuterON if pSrc is the right operand of an
/// outer join.  pTerm can be either EP_OuterON or EP_InnerON if pSrc
/// is the left operand of a RIGHT join.
///
/// See https://sqlite.org/forum/forumpost/206d99a16dd9212f
/// for an example of a WHERE clause constraints that may not be used on
/// the right table of a RIGHT JOIN because the constraint implies a
/// not-NULL condition on the left table of the RIGHT JOIN.
fn constraintCompatibleWithOuterJoin(db: &Sqlite3, pTerm: &WhereTerm, pSrc: SrcItemId) -> i32 {
    let it = db.item(pSrc);
    let e = db.ex(pTerm.pExpr);
    debug_assert!((it.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0); // By caller
    if !e.HasProperty(EP_OuterON | EP_InnerON) || e.w.iJoin != it.iCursor {
        return 0;
    }
    if (it.fg.jointype & (JT_LEFT | JT_RIGHT)) != 0 && e.HasProperty(EP_InnerON) {
        return 0;
    }
    1
}

/// Return true if column iCol of table pTab seem like it might be a
/// good column to use as part of a query-time index.
///
/// Current algorithm (subject to improvement!):
///
///   1.   If iCol is already the left-most column of some other index,
///        then return false.
///
///   2.   If iCol is part of an existing index that has an aiRowLogEst of
///        more than 20, then return false.
///
///   3.   If no disqualifying conditions above are found, return true.
fn columnIsGoodIndexCandidate(pTab: &TableRef, iCol: i32) -> i32 {
    let t = pTab.borrow();
    for pIdx in t.pIndex.iter() {
        let p = pIdx.borrow();
        for j in 0..p.nKeyCol as usize {
            if p.aiColumn[j] as i32 == iCol {
                if j == 0 {
                    return 0;
                }
                if p.hasStat1 != 0 && p.aiRowLogEst[j + 1] > 20 {
                    return 0;
                }
                break;
            }
        }
    }
    1
}

/// Return TRUE if the WHERE clause term pTerm is of a form where it
/// could be used with an index to access pSrc, assuming an appropriate
/// index existed.
fn termCanDriveIndex(
    db: &mut Sqlite3,
    pTerm: &WhereTerm,
    pSrc: SrcItemId,
    notReady: Bitmask,
) -> i32 {
    let (iCursor, jointype) = {
        let it = db.item(pSrc);
        (it.iCursor, it.fg.jointype)
    };
    if pTerm.leftCursor != iCursor {
        return 0;
    }
    if (pTerm.eOperator & (WO_EQ | WO_IS)) == 0 {
        return 0;
    }
    debug_assert!((jointype & JT_RIGHT) == 0);
    if (jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
        && constraintCompatibleWithOuterJoin(db, pTerm, pSrc) == 0
    {
        return 0; // See https://sqlite.org/forum/forumpost/51e6959f61
    }
    if (pTerm.prereqRight & notReady) != 0 {
        return 0;
    }
    debug_assert!((pTerm.eOperator & (WO_OR | WO_AND)) == 0);
    let leftCol = pTerm.u.x.leftColumn;
    if leftCol < 0 {
        return 0;
    }
    let tab = item_tab(db, pSrc);
    let aff = tab.borrow().aCol[leftCol as usize].affinity;
    if sqlite3IndexAffinityOk(db, pTerm.pExpr, aff) == 0 {
        return 0;
    }
    columnIsGoodIndexCandidate(&tab, leftCol)
}

/// Generate code to construct the Index object for an automatic index
/// and to set up the WhereLevel object pLevel so that the code generator
/// makes use of the automatic index.
fn constructAutomaticIndex(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pWC: WcId,
    notReady: Bitmask,
    iLevel: usize,
) {
    let mut nKeyCol: i32; // Number of columns in the constructed index
    let mut idxCols: Bitmask; // Bitmap of columns used for indexing
    let mut extraCols: Bitmask; // Bitmap of additional columns
    let mut sentWarning = false; // True if a warning has been issued
    let mut useBloomFilter = true; // True to also add a Bloom filter
    let mut pPartial = ExprId::NULL; // Partial Index Expression
    let mut iContinue = 0; // Jump here to skip excluded rows
    let mut addrCounter = 0; // Address where integer counter is initialized
    let addrTop: i32;

    // Generate code to skip over the creation and initialization of the
    // transient index on 2nd and subsequent iterations of the loop.
    let addrInit = sqlite3VdbeAddOp0(db.v(), OP_Once);

    // Count the number of columns that will be added to the index
    // and used to match WHERE clause constraints
    nKeyCol = 0;
    let pTabList = wi.pTabList;
    let iFrom = wi.a[iLevel].iFrom as u32;
    let pSrc = SrcItemId(pTabList, iFrom);
    let pTable = item_tab(db, pSrc);
    let nTerm = wi.wc(pWC).a.len();
    let iLoop = wi.a[iLevel].pWLoop.unwrap();
    idxCols = 0;
    'end: {
        for k in 0..nTerm {
            let t = wi.wc(pWC).a[k];
            let pExpr = t.pExpr;
            // Make the automatic index a partial index if there are terms in
            // the WHERE clause (or the ON clause of a LEFT join) that
            // constrain which rows of the target table (pSrc) that can be
            // used.
            if (t.wtFlags & TERM_VIRTUAL) == 0
                && sqlite3ExprIsSingleTableConstraint(db, pExpr, pTabList, iFrom as i32, 0) != 0
            {
                let d = sqlite3ExprDup(db, pExpr, 0);
                pPartial = sqlite3ExprAnd(db, pPartial, d);
            }
            if termCanDriveIndex(db, &t, pSrc, notReady) != 0 {
                debug_assert!((t.eOperator & (WO_OR | WO_AND)) == 0);
                let iCol = t.u.x.leftColumn;
                let cMask: Bitmask = if iCol >= BMS {
                    MASKBIT(BMS - 1)
                } else {
                    MASKBIT(iCol)
                };
                if !sentWarning {
                    let (zName, zCol) = {
                        let tb = pTable.borrow();
                        (tb.zName.clone(), tb.aCol[iCol as usize].zCnName.clone())
                    };
                    crate::printf::sqlite3_log(
                        SQLITE_WARNING_AUTOINDEX,
                        b"automatic index on %s(%s)",
                        &[(&zName).into(), (&zCol).into()],
                    );
                    sentWarning = true;
                }
                if (idxCols & cMask) == 0 {
                    wi.pLoops[iLoop].set_lterm(nKeyCol as usize, Some(TermId::new(pWC, k)));
                    nKeyCol += 1;
                    idxCols |= cMask;
                }
            }
        }
        debug_assert!(nKeyCol > 0 || db.mallocFailed != 0);
        {
            let pLoop = &mut wi.pLoops[iLoop];
            pLoop.u.btree.nEq = nKeyCol as u16;
            pLoop.nLTerm = nKeyCol as u16;
            pLoop.wsFlags = WHERE_COLUMN_EQ | WHERE_IDX_ONLY | WHERE_INDEXED | WHERE_AUTO_INDEX;
        }

        // Count the number of additional columns needed to create a
        // covering index.  A "covering index" is an index that contains all
        // columns that are needed by the query.  With a covering index, the
        // original table never needs to be accessed.  Automatic indices must
        // be a covering index because the index will not be updated if the
        // original table changes and the index and table cannot both be used
        // if they go out of sync.
        let colUsed = db.item(pSrc).colUsed;
        if pTable.borrow().IsView() {
            extraCols = ALLBITS & !idxCols;
        } else {
            extraCols = colUsed & (!idxCols | MASKBIT(BMS - 1));
        }
        let (hasRowid, nCol) = {
            let tb = pTable.borrow();
            (tb.HasRowid(), tb.nCol as i32)
        };
        if !hasRowid {
            // For WITHOUT ROWID tables, ensure that all PRIMARY KEY columns
            // are either in the idxCols mask or in the extraCols mask
            for i in 0..nCol {
                if (pTable.borrow().aCol[i as usize].colFlags & COLFLAG_PRIMKEY) == 0 {
                    continue;
                }
                if i >= BMS - 1 {
                    extraCols |= MASKBIT(BMS - 1);
                    break;
                }
                if idxCols & MASKBIT(i) != 0 {
                    continue;
                }
                extraCols |= MASKBIT(i);
            }
        }
        let mxBitCol = (BMS - 1).min(nCol);
        for i in 0..mxBitCol {
            if extraCols & MASKBIT(i) != 0 {
                nKeyCol += 1;
            }
        }
        if colUsed & MASKBIT(BMS - 1) != 0 {
            nKeyCol += nCol - BMS + 1;
        }

        // Construct the Index object to describe this index
        let mut zNotUsed: Option<ZStr> = None;
        let pIdx = match crate::build::sqlite3AllocateIndexObject(
            db,
            nKeyCol + hasRowid as i32,
            0,
            &mut zNotUsed,
        ) {
            Some(p) => p,
            None => break 'end,
        };
        wi.pLoops[iLoop].u.btree.pIndex = Some(pIdx.clone());
        {
            let mut p = pIdx.borrow_mut();
            p.zName = b"auto-index".to_vec();
            p.pTable = Rc::downgrade(&pTable);
            let nc = (nKeyCol + hasRowid as i32) as usize;
            if p.aiColumn.len() < nc {
                p.aiColumn.resize(nc, 0);
            }
            if p.azColl.len() < nc {
                p.azColl.resize(nc, Vec::new());
            }
        }
        let mut n: usize = 0;
        idxCols = 0;
        for k in 0..nTerm {
            let t = wi.wc(pWC).a[k];
            if termCanDriveIndex(db, &t, pSrc, notReady) != 0 {
                debug_assert!((t.eOperator & (WO_OR | WO_AND)) == 0);
                let iCol = t.u.x.leftColumn;
                let cMask: Bitmask = if iCol >= BMS {
                    MASKBIT(BMS - 1)
                } else {
                    MASKBIT(iCol)
                };
                if (idxCols & cMask) == 0 {
                    let pX = t.pExpr;
                    idxCols |= cMask;
                    pIdx.borrow_mut().aiColumn[n] = t.u.x.leftColumn as i16;
                    let pColl = sqlite3ExprCompareCollSeq(db, pX);
                    debug_assert!(pColl.is_some() || db.ppr().nErr > 0); // TH3 collate01.800
                    if !isBinary(&pColl) {
                        // Disallow the use of a Bloom filter if any non-BINARY
                        // collating sequence is involved.  tag-202607231411
                        useBloomFilter = false;
                    }
                    pIdx.borrow_mut().azColl[n] = coll_name(&pColl);
                    n += 1;
                }
            }
        }
        debug_assert!(n as u16 == wi.pLoops[iLoop].u.btree.nEq);

        // Add additional columns needed to make the automatic index into
        // a covering index
        for i in 0..mxBitCol {
            if extraCols & MASKBIT(i) != 0 {
                let mut p = pIdx.borrow_mut();
                p.aiColumn[n] = i as i16;
                p.azColl[n] = sqlite3StrBINARY.to_vec();
                n += 1;
            }
        }
        if colUsed & MASKBIT(BMS - 1) != 0 {
            for i in (BMS - 1)..nCol {
                let mut p = pIdx.borrow_mut();
                p.aiColumn[n] = i as i16;
                p.azColl[n] = sqlite3StrBINARY.to_vec();
                n += 1;
            }
        }
        debug_assert!(n as i32 == nKeyCol);
        if hasRowid {
            let mut p = pIdx.borrow_mut();
            p.aiColumn[n] = XN_ROWID;
            p.azColl[n] = sqlite3StrBINARY.to_vec();
        }

        // Create the automatic index
        debug_assert!(wi.a[iLevel].iIdxCur >= 0);
        let iIdxCur = db.pp().nTab;
        db.pp().nTab += 1;
        wi.a[iLevel].iIdxCur = iIdxCur;
        sqlite3VdbeAddOp2(db.v(), OP_OpenAutoindex, iIdxCur, nKeyCol + 1);
        sqlite3VdbeSetP4KeyInfo(db, &pIdx);
        if db.OptimizationEnabled(SQLITE_BloomFilter) && useBloomFilter {
            crate::wherecode::sqlite3WhereExplainBloomFilter(db, wi, iLevel);
            db.pp().nMem += 1;
            let r = db.pp().nMem;
            wi.a[iLevel].regFilter = r;
            sqlite3VdbeAddOp2(db.v(), OP_Blob, 10000, r);
        }

        // Fill the automatic index with content
        let iTabCur = wi.a[iLevel].iTabCur;
        let viaCoroutine = db.item(pSrc).fg.viaCoroutine != 0;
        if viaCoroutine {
            debug_assert!(db.item(pSrc).fg.isSubquery != 0);
            let pSubq = **db.item(pSrc).u4.pSubq.as_ref().unwrap();
            let regYield = pSubq.regReturn;
            addrCounter = sqlite3VdbeAddOp2(db.v(), OP_Integer, 0, 0);
            sqlite3VdbeAddOp3(db.v(), OP_InitCoroutine, regYield, 0, pSubq.addrFillSub);
            addrTop = sqlite3VdbeAddOp1(db.v(), OP_Yield, regYield);
        } else {
            debug_assert!(wi.a[iLevel].addrHalt != 0);
            let h = wi.a[iLevel].addrHalt;
            addrTop = sqlite3VdbeAddOp2(db.v(), OP_Rewind, iTabCur, h);
        }
        if pPartial.nn() {
            iContinue = sqlite3VdbeMakeLabel(db);
            sqlite3ExprIfFalse(db, pPartial, iContinue, SQLITE_JUMPIFNULL as i32);
            wi.pLoops[iLoop].wsFlags |= WHERE_PARTIALIDX;
        }
        let regRecord = sqlite3GetTempReg(db);
        let regBase =
            crate::delete::sqlite3GenerateIndexKey(db, &pIdx, iTabCur, regRecord, 0, None, None, 0);
        let regFilter = wi.a[iLevel].regFilter;
        if regFilter != 0 {
            debug_assert!(crate::whereexpr::sqlite3WhereLoopBloomable(&wi.pLoops[iLoop]) != 0);
            let nEq = wi.pLoops[iLoop].u.btree.nEq as i32;
            sqlite3VdbeAddOp4Int(db.v(), OP_FilterAdd, regFilter, 0, regBase, nEq);
        }
        sqlite3VdbeAddOp2(db.v(), OP_IdxInsert, iIdxCur, regRecord);
        sqlite3VdbeChangeP5(db.v(), OPFLAG_USESEEKRESULT);
        if pPartial.nn() {
            sqlite3VdbeResolveLabel(db.v(), iContinue);
        }
        if viaCoroutine {
            debug_assert!(db.item(pSrc).fg.isSubquery != 0);
            sqlite3VdbeChangeP2(db.v(), addrCounter, regBase + n as i32);
            debug_assert!(iIdxCur > 0);
            let regResult = db.item(pSrc).u4.pSubq.as_ref().unwrap().regResult;
            translateColumnToCopy(db, addrTop, iTabCur, regResult, iIdxCur);
            sqlite3VdbeGoto(db.v(), addrTop);
            db.itemm(pSrc).fg.viaCoroutine = 0;
            sqlite3VdbeJumpHere(db.v(), addrTop);
        } else {
            sqlite3VdbeAddOp2(db.v(), OP_Next, iTabCur, addrTop + 1);
            sqlite3VdbeChangeP5(db.v(), SQLITE_STMTSTATUS_AUTOINDEX as u16);
            if (db.item(pSrc).fg.jointype & JT_LEFT) != 0 {
                sqlite3VdbeJumpHere(db.v(), addrTop);
            }
        }
        sqlite3ReleaseTempReg(db, regRecord);

        // Jump here when skipping the initialization
        sqlite3VdbeJumpHere(db.v(), addrInit);
    }

    // end_auto_index_create:
    sqlite3ExprDelete(db, pPartial);
}

/// Generate bytecode that will initialize a Bloom filter that is
/// appropriate for pLevel.
///
/// If there are inner loops within pLevel that have the WHERE_BLOOMFILTER
/// flag set, initialize a Bloomfilter for them as well.
///
/// When the Bloom filter is initialized, the WHERE_BLOOMFILTER flag is
/// cleared from the loop, but the regFilter value is set to a register
/// that implements the Bloom filter.  When regFilter is positive, the
/// sqlite3WhereCodeOneLoopStart() will generate code to test the Bloom
/// filter and skip the subsequence B-Tree seek if the Bloom filter
/// indicates that no matching rows exist.
///
/// This routine may only be called if it has previously been determined
/// that the loop would benefit from a Bloom filter, and the
/// WHERE_BLOOMFILTER bit is set.
fn sqlite3ConstructBloomFilter(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    iLevel: usize,
    notReady: Bitmask,
) {
    let mut iLevel = iLevel;
    let saved_pIdxEpr = std::mem::take(&mut db.pp().pIdxEpr);
    let saved_pIdxPartExpr = std::mem::take(&mut db.pp().pIdxPartExpr);

    debug_assert!(wi.lp(iLevel).wsFlags & WHERE_BLOOMFILTER != 0);
    debug_assert!((wi.lp(iLevel).wsFlags & WHERE_IDX_ONLY) == 0);

    let addrOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
    loop {
        crate::wherecode::sqlite3WhereExplainBloomFilter(db, wi, iLevel);
        let addrCont = sqlite3VdbeMakeLabel(db);
        let iCur = wi.a[iLevel].iTabCur;
        db.pp().nMem += 1;
        let regFilter = db.pp().nMem;
        wi.a[iLevel].regFilter = regFilter;

        // The Bloom filter is a Blob held in a register.  Initialize it
        // to zero-filled blob of at least 80K bits, but maybe more if the
        // estimated size of the table is larger.  We could actually
        // measure the size of the table at run-time using OP_Count with
        // P3==1 and use that value to initialize the blob.  But that makes
        // testing complicated.  By basing the blob size on the value in the
        // sqlite_stat1 table, testing is much easier.
        let pTabList = wi.pTabList;
        let iSrc = wi.a[iLevel].iFrom as i32;
        let pItem = SrcItemId(pTabList, iSrc as u32);
        let pTab = item_tab(db, pItem);
        let mut sz = sqlite3LogEstToInt(pTab.borrow().nRowLogEst);
        if sz < 10000 {
            sz = 10000;
        } else if sz > 10000000 {
            sz = 10000000;
        }
        debug_assert!(crate::whereexpr::sqlite3WhereLoopBloomable(wi.lp(iLevel)) != 0);
        sqlite3VdbeAddOp2(db.v(), OP_Blob, sz as i32, regFilter);

        let addrTop = sqlite3VdbeAddOp1(db.v(), OP_Rewind, iCur);
        let nTerm = wi.wc(SWC).a.len();
        for k in 0..nTerm {
            let t = wi.wc(SWC).a[k];
            let pExpr = t.pExpr;
            if (t.wtFlags & TERM_VIRTUAL) == 0
                && sqlite3ExprIsSingleTableConstraint(db, pExpr, pTabList, iSrc, 0) != 0
            {
                sqlite3ExprIfFalse(db, t.pExpr, addrCont, SQLITE_JUMPIFNULL as i32);
            }
        }
        if wi.lp(iLevel).wsFlags & WHERE_IPK != 0 {
            let r1 = sqlite3GetTempReg(db);
            sqlite3VdbeAddOp2(db.v(), OP_Rowid, iCur, r1);
            sqlite3VdbeAddOp4Int(db.v(), OP_FilterAdd, regFilter, 0, r1, 1);
            sqlite3ReleaseTempReg(db, r1);
        } else {
            let pIdx = wi.lp(iLevel).u.btree.pIndex.clone().unwrap();
            let n = wi.lp(iLevel).u.btree.nEq as i32;
            let r1 = sqlite3GetTempRange(db, n);
            for jj in 0..n {
                sqlite3ExprCodeLoadIndexColumn(db, &pIdx, iCur, jj, r1 + jj);
            }
            sqlite3VdbeAddOp4Int(db.v(), OP_FilterAdd, regFilter, 0, r1, n);
            sqlite3ReleaseTempRange(db, r1, n);
        }
        sqlite3VdbeResolveLabel(db.v(), addrCont);
        let tc = wi.a[iLevel].iTabCur;
        sqlite3VdbeAddOp2(db.v(), OP_Next, tc, addrTop + 1);
        sqlite3VdbeJumpHere(db.v(), addrTop);
        wi.lpm(iLevel).wsFlags &= !WHERE_BLOOMFILTER;
        loop {
            iLevel += 1;
            if iLevel >= wi.nLevel as usize {
                break;
            }
            let jt = db
                .item(SrcItemId(wi.pTabList, wi.a[iLevel].iFrom as u32))
                .fg
                .jointype;
            if jt & (JT_LEFT | JT_LTORJ) != 0 {
                continue;
            }
            if wi.a[iLevel].pWLoop.is_none() {
                continue;
            }
            let pLoop = wi.lp(iLevel);
            if pLoop.prereq & notReady != 0 {
                continue;
            }
            if (pLoop.wsFlags & (WHERE_BLOOMFILTER | WHERE_COLUMN_IN)) == WHERE_BLOOMFILTER {
                // This is a candidate for bloom-filter pull-down (early
                // evaluation).  The test that WHERE_COLUMN_IN is omitted is
                // important, as we are not able to do early evaluation of
                // bloom filters that make use of the IN operator
                break;
            }
        }
        if iLevel >= wi.nLevel as usize {
            break;
        }
    }
    sqlite3VdbeJumpHere(db.v(), addrOnce);
    db.pp().pIdxEpr = saved_pIdxEpr;
    db.pp().pIdxPartExpr = saved_pIdxPartExpr;
}

// ---------------------------------------------------------------------------
// Virtual table interfaces.  The virtual table module (vtab.c) is not
// ported yet, so these are minimal: no sqlite3_index_info object is ever
// built and whereLoopAddVirtual() produces no plans.
// ---------------------------------------------------------------------------

/// C: `const char * sqlite3_vtab_collation(sqlite3_index_info *pIdxInfo, int iCons)`
/// (virtual tables not available: returns NULL)
pub fn sqlite3_vtab_collation(pIdxInfo: /*sqlite3_index_info**/ (), iCons: i32) -> Option<ZStr> {
    None
}

/// C: `int sqlite3_vtab_in(sqlite3_index_info *pIdxInfo, int iCons, int bHandle)`
/// (virtual tables not available: returns 0)
pub fn sqlite3_vtab_in(pIdxInfo: /*sqlite3_index_info**/ (), iCons: i32, bHandle: i32) -> i32 {
    0
}

/// C: `int sqlite3_vtab_rhs_value( sqlite3_index_info *pIdxInfo, int iCons, sqlite3_value **ppVal )`
/// (virtual tables not available: SQLITE_MISUSE, as for an out-of-range iCons)
pub fn sqlite3_vtab_rhs_value(
    db: &mut Sqlite3,
    pIdxInfo: /*sqlite3_index_info**/ (),
    iCons: i32,
    ppVal: &mut [Mem],
) -> i32 {
    SQLITE_MISUSE
}

/// C: `int sqlite3_vtab_distinct(sqlite3_index_info *pIdxInfo)`
/// (virtual tables not available: returns 0)
pub fn sqlite3_vtab_distinct(pIdxInfo: /*sqlite3_index_info**/ ()) -> i32 {
    0
}

/// Cause the prepared statement that is associated with a call to
/// xBestIndex to potentially use all schemas.  If the statement being
/// prepared is read-only, then just start read transactions on all
/// schemas.  But if this is a write operation, start writes on all
/// schemas.
///
/// This is used by the (built-in) sqlite_dbpage virtual table.
///
/// C: `void sqlite3VtabUsesAllSchemas(Parse *pParse)`
pub fn sqlite3VtabUsesAllSchemas(db: &mut Sqlite3) {
    let nDb = db.nDb();
    for i in 0..nDb {
        crate::build::sqlite3CodeVerifySchema(db, i);
    }
    if db.ppr().writeMask != 0 {
        for i in 0..nDb {
            crate::build::sqlite3BeginWriteOperation(db, 0, i);
        }
    }
}

/// Add all WhereLoop objects for a table of the join identified by
/// pBuilder->pNew->iTab.  That table is guaranteed to be a virtual table.
///
/// NOTE: the virtual table module is not ported yet; no xBestIndex call is
/// possible, so no WhereLoop is added.
fn whereLoopAddVirtual(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pBuilder: &mut WhereLoopBuilder,
    mPrereq: Bitmask,
    mUnusable: Bitmask,
) -> i32 {
    SQLITE_OK
}

/// If it is not NULL, pTerm is a term that provides an upper or lower
/// bound on a range scan. Without considering pTerm, it is estimated
/// that the scan will visit nNew rows. This function returns the number
/// estimated to be visited after taking pTerm into account.
///
/// If the user explicitly specified a likelihood() value for this term,
/// then the return value is the likelihood multiplied by the number of
/// input rows. Otherwise, this function assumes that an "IS NOT NULL" term
/// has a likelihood of 0.50, and any other term a likelihood of 0.25.
fn whereRangeAdjust(pTerm: Option<&WhereTerm>, nNew: LogEst) -> LogEst {
    let mut nRet = nNew as i32;
    if let Some(t) = pTerm {
        if t.truthProb <= 0 {
            nRet += t.truthProb as i32;
        } else if (t.wtFlags & TERM_VNULL) == 0 {
            nRet -= 20;
        }
    }
    nRet as LogEst
}

/// Return the affinity for a single column of an index.
///
/// C: `char sqlite3IndexColumnAffinity(sqlite3 *db, Index *pIdx, int iCol)`
pub fn sqlite3IndexColumnAffinity(db: &mut Sqlite3, pIdx: &IndexRef, iCol: i32) -> u8 {
    debug_assert!(iCol >= 0 && iCol < pIdx.borrow().nColumn as i32);
    if pIdx.borrow().zColAff.is_none() {
        if crate::insert::sqlite3IndexAffinityStr(db, pIdx).is_none() {
            return SQLITE_AFF_BLOB;
        }
    }
    let p = pIdx.borrow();
    let z = p.zColAff.as_ref().unwrap();
    debug_assert!((iCol as usize) < z.len());
    z.get(iCol as usize).copied().unwrap_or(SQLITE_AFF_BLOB)
}

/// This function is used to estimate the number of rows that will be
/// visited by scanning an index for a range of values. The range may have
/// an upper bound, a lower bound, or both. The WHERE clause terms that set
/// the upper and lower bounds are represented by pLower and pUpper
/// respectively.
///
/// In the absence of sqlite_stat4 ANALYZE data, or if such data cannot be
/// used, a single range inequality reduces the search space by a factor of
/// 4.  and a pair of constraints (x>? AND x<?) reduces the expected number
/// of rows visited by a factor of 64.
fn whereRangeScanEst(
    wi: &WhereInfo,
    pLower: Option<TermId>,
    pUpper: Option<TermId>,
    pLoop: &mut WhereLoop,
) -> i32 {
    let rc = SQLITE_OK;
    let mut nOut = pLoop.nOut as i32;

    let pLower = pLower.map(|t| *wi.t(t));
    let pUpper = pUpper.map(|t| *wi.t(t));
    debug_assert!(pLower.is_some() || pUpper.is_some());
    let mut nNew = whereRangeAdjust(pLower.as_ref(), nOut as LogEst) as i32;
    nNew = whereRangeAdjust(pUpper.as_ref(), nNew as LogEst) as i32;

    // TUNING: If there is both an upper and lower limit and neither limit
    // has an application-defined likelihood(), assume the range is
    // reduced by an additional 75%. This means that, by default, an
    // open-ended range query (e.g. col > ?) is assumed to match 1/4 of the
    // rows in the index. While a closed range (e.g. col BETWEEN ? AND ?) is
    // estimated to match 1/64 of the index.
    if let (Some(l), Some(u)) = (&pLower, &pUpper) {
        if l.truthProb > 0 && u.truthProb > 0 {
            nNew -= 20;
        }
    }

    nOut -= pLower.is_some() as i32 + pUpper.is_some() as i32;
    if nNew < 10 {
        nNew = 10;
    }
    if nNew < nOut {
        nOut = nNew;
    }
    pLoop.nOut = nOut as LogEst;
    rc
}

/// Print the content of a WhereTerm object (debugging aid: no-op).
///
/// C: `void sqlite3WhereTermPrint(WhereTerm *pTerm, int iTerm)`
pub fn sqlite3WhereTermPrint(pTerm: &mut crate::where_::WhereTerm, iTerm: i32) {}

/// C: `void sqlite3ShowWhereTerm(WhereTerm *pTerm)` (debugging aid: no-op)
pub fn sqlite3ShowWhereTerm(pTerm: &mut crate::where_::WhereTerm) {}

/// C: `void sqlite3WhereClausePrint(WhereClause *pWC)` (debugging aid: no-op)
pub fn sqlite3WhereClausePrint(pWC: &mut crate::where_::WhereClause) {}

/// C: `void sqlite3WhereLoopPrint(const WhereLoop *p, const WhereClause *pWC)`
/// (debugging aid: no-op)
pub fn sqlite3WhereLoopPrint(p: &crate::where_::WhereLoop, pWC: &crate::where_::WhereClause) {}

/// C: `void sqlite3ShowWhereLoop(const WhereLoop *p)` (debugging aid: no-op)
pub fn sqlite3ShowWhereLoop(p: &crate::where_::WhereLoop) {}

/// C: `void sqlite3ShowWhereLoopList(const WhereLoop *p)` (debugging aid: no-op)
pub fn sqlite3ShowWhereLoopList(p: &crate::where_::WhereLoop) {}

/// Convert bulk memory into a valid WhereLoop that can be passed
/// to whereLoopClear harmlessly.
fn whereLoopInit(p: &mut WhereLoop) {
    p.aLTerm = vec![None; N_LTERM_SPACE];
    p.nLTerm = 0;
    p.wsFlags = 0;
}

/// Clear the WhereLoop.u union.  Leave WhereLoop.pLTerm intact.
fn whereLoopClearUnion(p: &mut WhereLoop) {
    if p.wsFlags & (WHERE_VIRTUALTABLE | WHERE_AUTO_INDEX) != 0 {
        if (p.wsFlags & WHERE_VIRTUALTABLE) != 0 && p.u.vtab.needFree != 0 {
            p.u.vtab.needFree = 0;
            p.u.vtab.idxStr = None;
        } else if (p.wsFlags & WHERE_AUTO_INDEX) != 0 && p.u.btree.pIndex.is_some() {
            p.u.btree.pIndex = None;
        }
    }
}

/// Deallocate internal memory used by a WhereLoop object.  Leave the
/// object in an initialized state, as if it had been newly allocated.
fn whereLoopClear(p: &mut WhereLoop) {
    if p.aLTerm.len() != N_LTERM_SPACE {
        p.aLTerm = vec![None; N_LTERM_SPACE];
    }
    whereLoopClearUnion(p);
    p.nLTerm = 0;
    p.wsFlags = 0;
}

/// Increase the memory allocation for pLoop->aLTerm[] to be at least n.
fn whereLoopResize(p: &mut WhereLoop, n: i32) -> i32 {
    if p.aLTerm.len() >= n as usize {
        return SQLITE_OK;
    }
    let n = ((n + 7) & !7) as usize;
    p.aLTerm.resize(n, None);
    SQLITE_OK
}

/// Transfer content from the second pLoop into the first.
fn whereLoopXfer(pTo: &mut WhereLoop, pFrom: &mut WhereLoop) -> i32 {
    whereLoopClearUnion(pTo);
    if pFrom.nLTerm as usize > pTo.aLTerm.len() {
        whereLoopResize(pTo, pFrom.nLTerm as i32);
    }
    pTo.prereq = pFrom.prereq;
    pTo.maskSelf = pFrom.maskSelf;
    pTo.iTab = pFrom.iTab;
    pTo.iSortIdx = pFrom.iSortIdx;
    pTo.rSetup = pFrom.rSetup;
    pTo.rRun = pFrom.rRun;
    pTo.nOut = pFrom.nOut;
    pTo.u = pFrom.u.clone();
    pTo.wsFlags = pFrom.wsFlags;
    pTo.nLTerm = pFrom.nLTerm;
    pTo.nSkip = pFrom.nSkip;
    for i in 0..pTo.nLTerm as usize {
        pTo.aLTerm[i] = pFrom.lterm(i);
    }
    if pFrom.wsFlags & WHERE_VIRTUALTABLE != 0 {
        pFrom.u.vtab.needFree = 0;
    } else if (pFrom.wsFlags & WHERE_AUTO_INDEX) != 0 {
        pFrom.u.btree.pIndex = None;
    }
    SQLITE_OK
}

/// Return TRUE if X is a proper subset of Y but is of equal or less cost.
/// In other words, return true if all constraints of X are also part of Y
/// and Y has additional constraints that might speed the search that X
/// lacks but the cost of running X is not more than the cost of running Y.
///
/// In other words, return true if the cost relationship between X and Y
/// is inverted and needs to be adjusted.
///
/// Case 1:
///
///   (1a)  X and Y use the same index.
///   (1b)  X has fewer == terms than Y
///   (1c)  Neither X nor Y use skip-scan
///   (1d)  X does not have a a greater cost than Y
///
/// Case 2:
///
///   (2a)  X has the same or lower cost, or returns the same or fewer rows,
///         than Y.
///   (2b)  X uses fewer WHERE clause terms than Y
///   (2c)  Every WHERE clause term used by X is also used by Y
///   (2d)  X skips at least as many columns as Y
///   (2e)  If X is a covering index, than Y is too
fn whereLoopCheaperProperSubset(pX: &WhereLoop, pY: &WhereLoop) -> i32 {
    if pX.rRun > pY.rRun && pX.nOut > pY.nOut {
        return 0; // (1d) and (2a)
    }
    debug_assert!((pX.wsFlags & WHERE_VIRTUALTABLE) == 0);
    debug_assert!((pY.wsFlags & WHERE_VIRTUALTABLE) == 0);
    if pX.u.btree.nEq < pY.u.btree.nEq // (1b)
        && idx_eq(&pX.u.btree.pIndex, &pY.u.btree.pIndex) // (1a)
        && pX.nSkip == 0
        && pY.nSkip == 0
    // (1c)
    {
        return 1; // Case 1 is true
    }
    if pX.nLTerm as i32 - pX.nSkip as i32 >= pY.nLTerm as i32 - pY.nSkip as i32 {
        return 0; // (2b)
    }
    if pY.nSkip > pX.nSkip {
        return 0; // (2d)
    }
    let mut i = pX.nLTerm as i32 - 1;
    while i >= 0 {
        if let Some(xt) = pX.lterm(i as usize) {
            let mut j = pY.nLTerm as i32 - 1;
            while j >= 0 {
                if pY.lterm(j as usize) == Some(xt) {
                    break;
                }
                j -= 1;
            }
            if j < 0 {
                return 0; // (2c)
            }
        }
        i -= 1;
    }
    if (pX.wsFlags & WHERE_IDX_ONLY) != 0 && (pY.wsFlags & WHERE_IDX_ONLY) == 0 {
        return 0; // (2e)
    }
    1 // Case 2 is true
}

/// Try to adjust the cost and number of output rows of WhereLoop pTemplate
/// upwards or downwards so that:
///
///   (1) pTemplate costs less than any other WhereLoops that are a proper
///       subset of pTemplate
///
///   (2) pTemplate costs more than any other WhereLoops for which pTemplate
///       is a proper subset.
///
/// To say "WhereLoop X is a proper subset of Y" means that X uses fewer
/// WHERE clause terms than Y and that every WHERE clause term used by X is
/// also used by Y.
fn whereLoopAdjustCost(pLoops: &[WhereLoop], pTemplate: &mut WhereLoop) {
    if (pTemplate.wsFlags & WHERE_INDEXED) == 0 {
        return;
    }
    for p in pLoops.iter() {
        if p.iTab != pTemplate.iTab {
            continue;
        }
        if (p.wsFlags & WHERE_INDEXED) == 0 {
            continue;
        }
        if whereLoopCheaperProperSubset(p, pTemplate) != 0 {
            // Adjust pTemplate cost downward so that it is cheaper than its
            // subset p.
            pTemplate.rRun = p.rRun.min(pTemplate.rRun);
            pTemplate.nOut = ((p.nOut as i32 - 1).min(pTemplate.nOut as i32)) as LogEst;
        } else if whereLoopCheaperProperSubset(pTemplate, p) != 0 {
            // Adjust pTemplate cost upward so that it is costlier than p
            // since pTemplate is a proper subset of p
            pTemplate.rRun = p.rRun.max(pTemplate.rRun);
            pTemplate.nOut = ((p.nOut as i32 + 1).max(pTemplate.nOut as i32)) as LogEst;
        }
    }
}

/// Search the list of WhereLoops starting at index `start` looking for one
/// that can be replaced by pTemplate.
///
/// Return None if pTemplate does not belong on the WhereLoop list.
/// In other words if pTemplate ought to be dropped from further
/// consideration.
///
/// If pX is a WhereLoop that pTemplate can replace, then return
/// Some(index of pX).
///
/// If pTemplate cannot replace any existing element of the list but needs
/// to be added to the list as a new entry, then return Some(pLoops.len())
/// (the tail of the list).
fn whereLoopFindLesser(pLoops: &[WhereLoop], start: usize, pTemplate: &WhereLoop) -> Option<usize> {
    let mut i = start;
    while i < pLoops.len() {
        let p = &pLoops[i];
        if p.iTab != pTemplate.iTab || p.iSortIdx != pTemplate.iSortIdx {
            // If either the iTab or iSortIdx values for two WhereLoop are
            // different then those WhereLoops need to be considered
            // separately.  Neither is a candidate to replace the other.
            i += 1;
            continue;
        }
        // In the current implementation, the rSetup value is either zero
        // or the cost of building an automatic index (NlogN) and the NlogN
        // is the same for compatible WhereLoops.
        debug_assert!(p.rSetup == 0 || pTemplate.rSetup == 0 || p.rSetup == pTemplate.rSetup);

        // whereLoopAddBtree() always generates and inserts the automatic
        // index case first.  Hence compatible candidate WhereLoops never have
        // a larger rSetup. Call this SETUP-INVARIANT
        debug_assert!(p.rSetup >= pTemplate.rSetup);

        // Any loop using an application-defined index (or PRIMARY KEY or
        // UNIQUE constraint) with one or more == constraints is better
        // than an automatic index. Unless it is a skip-scan.
        if (p.wsFlags & WHERE_AUTO_INDEX) != 0
            && pTemplate.nSkip == 0
            && (pTemplate.wsFlags & WHERE_INDEXED) != 0
            && (pTemplate.wsFlags & WHERE_COLUMN_EQ) != 0
            && (p.prereq & pTemplate.prereq) == pTemplate.prereq
        {
            break;
        }

        // If existing WhereLoop p is better than pTemplate, pTemplate can be
        // discarded.  WhereLoop p is better if:
        //   (1)  p has no more dependencies than pTemplate, and
        //   (2)  p has an equal or lower cost than pTemplate
        if (p.prereq & pTemplate.prereq) == p.prereq // (1)
            && p.rSetup <= pTemplate.rSetup // (2a)
            && p.rRun <= pTemplate.rRun // (2b)
            && p.nOut <= pTemplate.nOut
        // (2c)
        {
            return None; // Discard pTemplate
        }

        // If pTemplate is always better than p, then cause p to be
        // overwritten with pTemplate.  pTemplate is better than p if:
        //   (1)  pTemplate has no more dependencies than p, and
        //   (2)  pTemplate has an equal or lower cost than p.
        if (p.prereq & pTemplate.prereq) == pTemplate.prereq // (1)
            && p.rRun >= pTemplate.rRun // (2a)
            && p.nOut >= pTemplate.nOut
        // (2b)
        {
            debug_assert!(p.rSetup >= pTemplate.rSetup); // SETUP-INVARIANT above
            break; // Cause p to be overwritten by pTemplate
        }
        i += 1;
    }
    Some(i)
}

/// Insert or replace a WhereLoop entry using the template supplied
/// (pBuilder->pNew).
///
/// An existing WhereLoop entry might be overwritten if the new template
/// is better and has fewer dependencies.  Or the template will be ignored
/// and no insert will occur if an existing WhereLoop is faster and has
/// fewer dependencies than the template.  Otherwise a new WhereLoop is
/// added based on the template.
///
/// If pBuilder->pOrSet is not NULL then we care about only the
/// prerequisites and rRun and nOut costs of the N best loops.  That
/// information is gathered in the pBuilder->pOrSet object.  This special
/// processing mode is used only for OR clause processing.
///
/// When accumulating multiple loops (when pBuilder->pOrSet is NULL) we
/// still might overwrite similar loops with the new template if the
/// new template is better.  Loops may be overwritten if the following
/// conditions are met:
///
///    (1)  They have the same iTab.
///    (2)  They have the same iSortIdx.
///    (3)  The template has same or fewer dependencies than the current loop
///    (4)  The template has the same or lower cost than the current loop
fn whereLoopInsert(db: &mut Sqlite3, wi: &mut WhereInfo, pBuilder: &mut WhereLoopBuilder) -> i32 {
    // Stop the search once we hit the query planner search limit
    if pBuilder.iPlanLimit == 0 {
        if let Some(s) = pBuilder.pOrSet.as_mut() {
            s.n = 0;
        }
        return SQLITE_DONE;
    }
    pBuilder.iPlanLimit -= 1;

    whereLoopAdjustCost(&wi.pLoops, &mut pBuilder.pNew);

    // If pBuilder->pOrSet is defined, then only keep track of the costs
    // and prereqs.
    if let Some(set) = pBuilder.pOrSet.as_mut() {
        let t = &pBuilder.pNew;
        if t.nLTerm != 0 {
            whereOrInsert(set, t.prereq, t.rRun, t.nOut);
        }
        return SQLITE_OK;
    }

    // Look for an existing WhereLoop to replace with pTemplate
    let ip = match whereLoopFindLesser(&wi.pLoops, 0, &pBuilder.pNew) {
        // There already exists a WhereLoop on the list that is better
        // than pTemplate, so just ignore pTemplate
        None => return SQLITE_OK,
        Some(i) => i,
    };

    // If we reach this point it means that either p[] should be overwritten
    // with pTemplate[] if p[] exists, or if p==NULL then allocate a new
    // WhereLoop and insert it.
    if ip == wi.pLoops.len() {
        // Allocate a new WhereLoop to add to the end of the list
        let mut p = WhereLoop::default();
        whereLoopInit(&mut p);
        wi.pLoops.push(p);
    } else {
        // We will be overwriting WhereLoop p[].  But before we do, first
        // go through the rest of the list and delete any other entries
        // besides p[] that are also supplanted by pTemplate
        let mut iTail = ip + 1;
        while iTail < wi.pLoops.len() {
            match whereLoopFindLesser(&wi.pLoops, iTail, &pBuilder.pNew) {
                None => break,
                Some(k) => {
                    if k >= wi.pLoops.len() {
                        break;
                    }
                    wi.pLoops.remove(k);
                    iTail = k;
                }
            }
        }
    }
    let rc = whereLoopXfer(&mut wi.pLoops[ip], &mut pBuilder.pNew);
    let p = &mut wi.pLoops[ip];
    if (p.wsFlags & WHERE_VIRTUALTABLE) == 0 {
        let isIpk = match &p.u.btree.pIndex {
            Some(x) => x.borrow().idxType == SQLITE_IDXTYPE_IPK,
            None => false,
        };
        if isIpk {
            p.u.btree.pIndex = None;
        }
    }
    rc
}

/// Callback for estLikePatternLength().
///
/// If this node is a string literal that is longer pWalker->sz, then set
/// pWalker->sz to the byte length of that string literal.
///
/// pWalker->eCode indicates how to count characters:
///
///    eCode==0     Count as a GLOB pattern
///    eCode==1     Count as a LIKE pattern
fn exprNodePatternLengthEst(pWalker: &mut Walker, pExpr: ExprId) -> i32 {
    if pWalker.db.ex(pExpr).op == TK_STRING {
        let mut sz = 0; // Pattern size in bytes
        let z: Vec<u8> = pWalker.db.ex(pExpr).token().to_vec(); // The pattern
        let (c1, c2, c3): (u8, u8, u8);
        if pWalker.eCode != 0 {
            c1 = b'%';
            c2 = b'_';
            c3 = 0;
        } else {
            c1 = b'*';
            c2 = b'?';
            c3 = b'[';
        }
        let at = |i: usize| -> u8 { z.get(i).copied().unwrap_or(0) };
        let mut i = 0usize;
        loop {
            let c = at(i);
            i += 1;
            if c == 0 {
                break;
            }
            if c == c3 {
                if at(i) != 0 {
                    i += 1;
                }
                while at(i) != 0 && at(i) != b']' {
                    i += 1;
                }
            } else if c != c1 && c != c2 {
                sz += 1;
            }
        }
        if sz > pWalker.u.sz {
            pWalker.u.sz = sz;
        }
    }
    WRC_Continue
}

/// Return the length of the longest string literal in the given
/// expression.
///
/// eCode indicates how to count characters:
///
///    eCode==0     Count as a GLOB pattern
///    eCode==1     Count as a LIKE pattern
fn estLikePatternLength(db: &mut Sqlite3, p: ExprId, eCode: u16) -> i32 {
    let mut w = Walker::new(db);
    w.u.sz = 0;
    w.eCode = eCode;
    w.xExprCallback = Some(exprNodePatternLengthEst);
    w.xSelectCallback = Some(sqlite3SelectWalkFail);
    crate::walker::sqlite3WalkExpr(&mut w, p);
    w.u.sz
}

/// Adjust the WhereLoop.nOut value downward to account for terms of the
/// WHERE clause that reference the loop but which are not used by an
/// index.
///
/// For every WHERE clause term that is not used by the index
/// and which has a truth probability assigned by one of the likelihood(),
/// likely(), or unlikely() SQL functions, reduce the estimated number
/// of output rows by the probability specified.
///
/// TUNING:  For every WHERE clause term that is not used by the index
/// and which does not have an assigned truth probability, heuristics
/// described below are used to try to estimate the truth probability.
///
/// Heuristic 1:  Estimate the truth probability as 93.75%.  The 93.75%
/// value corresponds to -1 in LogEst notation, so this means decrement
/// the WhereLoop.nOut field for every such WHERE clause term.
///
/// Heuristic 2:  If there exists one or more WHERE clause terms of the
/// form "x==EXPR" and EXPR is not a constant 0 or 1, then make sure the
/// final output row estimate is no greater than 1/4 of the total number
/// of rows in the table.  In other words, assume that x==EXPR will filter
/// out at least 3 out of 4 rows.  If EXPR is -1 or 0 or 1, then maybe the
/// "x" column is boolean or else -1 or 0 or 1 is a common default value
/// on the "x" column and so in that case only cap the output row estimate
/// at 1/2 instead of 1/4.
///
/// Heuristic 3:  If there is a LIKE or GLOB (or REGEXP or MATCH) operator
/// with a large constant pattern, then reduce the size of the search
/// space according to the length of the pattern, under the theory that
/// longer patterns are less likely to match.
fn whereLoopOutputAdjust(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pWC: WcId,
    pLoop: &mut WhereLoop,
    nRow: LogEst,
) {
    let notAllowed: Bitmask = !(pLoop.prereq | pLoop.maskSelf);
    let mut iReduce: i32 = 0; // pLoop->nOut should not exceed nRow-iReduce

    // Skip all this if the FROM clause of the query is a single table and
    // there is no ORDER BY. In this case it doesn't matter how accurate
    // the WhereLoop.nOut values are.
    if db.src(wi.pTabList).nSrc() <= 1 && wi.pOrderBy.is_null() {
        return;
    }

    debug_assert!((pLoop.wsFlags & WHERE_AUTO_INDEX) == 0);
    let nBase = wi.wc(pWC).nBase;
    let mut nOut = pLoop.nOut as i32;
    for k in 0..nBase as usize {
        let pTerm = TermId::new(pWC, k);
        let t = *wi.t(pTerm);
        if (t.prereqAll & notAllowed) != 0 {
            continue;
        }
        if (t.prereqAll & pLoop.maskSelf) == 0 {
            continue;
        }
        if (t.wtFlags & TERM_VIRTUAL) != 0 {
            continue;
        }
        let mut j = pLoop.nLTerm as i32 - 1;
        while j >= 0 {
            match pLoop.lterm(j as usize) {
                None => {}
                Some(pX) => {
                    if pX == pTerm {
                        break;
                    }
                    let px = wi.t(pX);
                    if px.iParent >= 0 && pX.wc as usize == pWC && px.iParent as usize == k {
                        break;
                    }
                }
            }
            j -= 1;
        }
        if j < 0 {
            sqlite3ProgressCheck(db);
            if pLoop.maskSelf == t.prereqAll {
                // If there are extra terms in the WHERE clause not used by an
                // index that depend only on the table being scanned, and that
                // will tend to cause many rows to be omitted, then mark that
                // table as "self-culling".
                //
                // 2022-03-24:  Self-culling only applies if either the extra
                // terms are straight comparison operators that are non-true
                // with NULL operand, or if the loop is not an OUTER JOIN.
                if (t.eOperator & 0x3f) != 0
                    || (db.src(wi.pTabList).a[pLoop.iTab as usize].fg.jointype
                        & (JT_LEFT | JT_LTORJ))
                        == 0
                {
                    pLoop.wsFlags |= WHERE_SELFCULL;
                }
            }
            if t.truthProb <= 0 {
                // If a truth probability is specified using the likelihood()
                // hints, then use the probability provided by the
                // application.
                nOut += t.truthProb as i32;
            } else {
                // In the absence of explicit truth probabilities, use
                // heuristics to guess a reasonable truth probability.
                let pOpExpr = t.pExpr;
                nOut -= 1;
                if (t.eOperator & (WO_EQ | WO_IS)) != 0 && (t.wtFlags & TERM_HIGHTRUTH) == 0
                // tag-20200224-1
                {
                    let pRight = db.ex(pOpExpr).pRight;
                    let mut k2 = 0;
                    if sqlite3ExprIsInteger(db, pRight, &mut k2, true, 1) != 0
                        && k2 >= -1
                        && k2 <= 1
                    {
                        k2 = 10;
                    } else {
                        k2 = 20;
                    }
                    if iReduce < k2 {
                        wi.tm(pTerm).wtFlags |= TERM_HEURTRUTH;
                        iReduce = k2;
                    }
                } else if db.ex(pOpExpr).HasProperty(EP_InfixFunc)
                    && db.ex(pOpExpr).op == TK_FUNCTION
                {
                    let mut eOp = crate::whereexpr::sqlite3ExprIsLikeOperator(db, pOpExpr);
                    if eOp > 0 {
                        let l = db.ex(pOpExpr).x.pList;
                        let pRHS = db.el(l).a[0].pExpr;
                        eOp = (eOp == SQLITE_INDEX_CONSTRAINT_LIKE) as i32;
                        let szPattern = estLikePatternLength(db, pRHS, eOp as u16);
                        if szPattern > 0 {
                            nOut -= szPattern * 2;
                        }
                    }
                }
            }
        }
    }
    if nOut > nRow as i32 - iReduce {
        nOut = nRow as i32 - iReduce;
    }
    pLoop.nOut = nOut as LogEst;
}

/// Term pTerm is a vector range comparison operation. The first comparison
/// in the vector can be optimized using column nEq of the index. This
/// function returns the total number of vector elements that can be used
/// as part of the range comparison.
///
/// For example, if the query is:
///
///   WHERE a = ? AND (b, c, d) > (?, ?, ?)
///
/// and the index:
///
///   CREATE INDEX ... ON (a, b, c, d, e)
///
/// then this function would be invoked with nEq=1. The value returned in
/// this case is 3.
fn whereRangeVectorLen(
    db: &mut Sqlite3,
    iCur: i32,
    pIdx: &IndexRef,
    nEq: i32,
    pTerm: &WhereTerm,
) -> i32 {
    let (pl, pr) = (db.ex(pTerm.pExpr).pLeft, db.ex(pTerm.pExpr).pRight);
    let mut nCmp = sqlite3ExprVectorSize(db, pl);
    let (nColumn, aiColumn, aSortOrder, azColl, pTab) = {
        let p = pIdx.borrow();
        (
            p.nColumn as i32,
            p.aiColumn.clone(),
            p.aSortOrder.clone(),
            p.azColl.clone(),
            p.table(),
        )
    };

    nCmp = nCmp.min(nColumn - nEq);
    let mut i = 1;
    while i < nCmp {
        // Test if comparison i of pTerm is compatible with column (i+nEq) of
        // the index. If not, exit the loop.
        let ll = db.ex(pl).x.pList;
        let mut pLhs = db.el(ll).a[i as usize].pExpr;
        let mut pRhs = pr;
        if db.ex(pRhs).UseXSelect() {
            let s = db.ex(pRhs).x.pSelect;
            let el = db.sel(s).pEList;
            pRhs = db.el(el).a[i as usize].pExpr;
        } else {
            let l = db.ex(pRhs).x.pList;
            pRhs = db.el(l).a[i as usize].pExpr;
        }

        // Check that the LHS of the comparison is a column reference to
        // the right column of the right source table. And that the sort
        // order of the index column is the same as the sort order of the
        // leftmost index column.
        {
            let x = db.ex(pLhs);
            let k = (i + nEq) as usize;
            if x.op != TK_COLUMN
                || x.iTable != iCur
                || x.iColumn != aiColumn[k]
                || aSortOrder[k] != aSortOrder[nEq as usize]
            {
                break;
            }
        }

        let lc = db.ex(pLhs).iColumn as i32;
        let la = sqlite3ExprAffinity(db, pLhs);
        let aff = sqlite3CompareAffinity(db, pRhs, la);
        let idxaff = sqlite3TableColumnAffinity(&pTab, lc);
        if aff != idxaff {
            break;
        }

        if db.ex(pTerm.pExpr).HasProperty(EP_Commuted) {
            std::mem::swap(&mut pRhs, &mut pLhs);
        }
        let pColl = sqlite3BinaryCompareCollSeq(db, pLhs, pRhs);
        let Some(pColl) = pColl else { break };
        if sqlite3StrICmp(
            Some(&pColl.zName[..]),
            Some(&azColl[(i + nEq) as usize][..]),
        ) != 0
        {
            break;
        }
        i += 1;
    }
    i
}

/// We have so far matched pBuilder->pNew->u.btree.nEq terms of the
/// index pIndex. Try to match one more.
///
/// When this function is called, pBuilder->pNew->nOut contains the
/// number of rows expected to be visited by filtering using the nEq
/// terms only. If it is modified, this value is restored before this
/// function returns.
///
/// If pProbe->idxType==SQLITE_IDXTYPE_IPK, that means pIndex is
/// a fake index used for the INTEGER PRIMARY KEY.
fn whereLoopAddBtreeIndex(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pBuilder: &mut WhereLoopBuilder,
    pSrc: SrcItemId,
    pProbe: &IndexRef,
    nInMul: LogEst,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut opMask: u32;
    let mut scan = WhereScan::default();
    let mut pTop: Option<TermId> = None;
    let mut pBtm: Option<TermId> = None;

    debug_assert!(db.mallocFailed == 0 || db.ppr().nErr > 0);
    if db.ppr().nErr != 0 {
        return db.ppr().rc;
    }

    let (
        aiRowLogEst,
        aiColumn,
        nKeyCol,
        nColumn,
        bUnordered,
        idxType,
        onError,
        uniqNotNull,
        hasStat1,
        noSkipScan,
        szIdxRow,
    ) = {
        let p = pProbe.borrow();
        (
            p.aiRowLogEst.clone(),
            p.aiColumn.clone(),
            p.nKeyCol as i32,
            p.nColumn as i32,
            p.bUnordered,
            p.idxType,
            p.onError,
            p.uniqNotNull,
            p.hasStat1,
            p.noSkipScan,
            p.szIdxRow,
        )
    };
    let isUnique = onError != OE_None;

    {
        let pNew = &pBuilder.pNew;
        debug_assert!((pNew.wsFlags & WHERE_VIRTUALTABLE) == 0);
        debug_assert!((pNew.wsFlags & WHERE_TOP_LIMIT) == 0);
        if pNew.wsFlags & WHERE_BTM_LIMIT != 0 {
            opMask = (WO_LT | WO_LE) as u32;
        } else {
            debug_assert!(pNew.u.btree.nBtm == 0);
            opMask = (WO_EQ | WO_IN | WO_GT | WO_GE | WO_LT | WO_LE | WO_ISNULL | WO_IS) as u32;
        }
    }
    if bUnordered != 0 {
        opMask &= !((WO_GT | WO_GE | WO_LT | WO_LE) as u32);
    }

    debug_assert!((pBuilder.pNew.u.btree.nEq as i32) < nColumn);
    debug_assert!(
        (pBuilder.pNew.u.btree.nEq as i32) < nKeyCol || idxType != SQLITE_IDXTYPE_PRIMARYKEY
    );

    let saved_nEq = pBuilder.pNew.u.btree.nEq;
    let saved_nBtm = pBuilder.pNew.u.btree.nBtm;
    let saved_nTop = pBuilder.pNew.u.btree.nTop;
    let saved_nSkip = pBuilder.pNew.nSkip;
    let saved_nLTerm = pBuilder.pNew.nLTerm;
    let saved_wsFlags = pBuilder.pNew.wsFlags;
    let saved_prereq = pBuilder.pNew.prereq;
    let saved_nOut = pBuilder.pNew.nOut;
    let iCursor = db.item(pSrc).iCursor;
    let pWC = pBuilder.pWC;
    let mut next = whereScanInit(
        db,
        wi,
        &mut scan,
        pWC,
        iCursor,
        saved_nEq as i32,
        opMask,
        Some(pProbe),
    );
    pBuilder.pNew.rSetup = 0;
    let rSize: LogEst = aiRowLogEst[0];
    let rLogSize: LogEst = estLog(rSize);
    let (jointype, fromExists) = {
        let it = db.item(pSrc);
        (it.fg.jointype, it.fg.fromExists)
    };
    let szTabRow = item_tab(db, pSrc).borrow().szTabRow;
    while rc == SQLITE_OK {
        let Some(pTerm) = next else { break };
        'body: {
            let t = *wi.t(pTerm);
            let eOp: u16 = t.eOperator; // Shorthand for pTerm->eOperator
            let mut rCostIdx: i32;
            let nOutUnadjusted: LogEst; // nOut before IN() and WHERE adjustments
            let mut nIn: i32 = 0;
            if (eOp == WO_ISNULL || (t.wtFlags & TERM_VNULL) != 0)
                && indexColumnNotNull(pProbe, saved_nEq as i32) != 0
            {
                break 'body; // ignore IS [NOT] NULL constraints on NOT NULL columns
            }
            if t.prereqRight & pBuilder.pNew.maskSelf != 0 {
                break 'body;
            }

            // Do not allow the upper bound of a LIKE optimization range
            // constraint to mix with a lower range bound from some other
            // source
            if (t.wtFlags & TERM_LIKEOPT) != 0 && t.eOperator == WO_LT {
                break 'body;
            }

            if (jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
                && constraintCompatibleWithOuterJoin(db, &t, pSrc) == 0
            {
                break 'body;
            }
            if isUnique && saved_nEq as i32 == nKeyCol - 1 {
                pBuilder.bldFlags1 |= SQLITE_BLDF1_UNIQUE as u8;
            } else {
                pBuilder.bldFlags1 |= SQLITE_BLDF1_INDEXED as u8;
            }
            {
                let pNew = &mut pBuilder.pNew;
                pNew.wsFlags = saved_wsFlags;
                pNew.u.btree.nEq = saved_nEq;
                pNew.u.btree.nBtm = saved_nBtm;
                pNew.u.btree.nTop = saved_nTop;
                pNew.nLTerm = saved_nLTerm;
                let n = pNew.nLTerm as usize;
                pNew.set_lterm(n, Some(pTerm));
                pNew.nLTerm += 1;
                pNew.prereq = (saved_prereq | t.prereqRight) & !pNew.maskSelf;
            }

            debug_assert!(
                nInMul == 0
                    || (pBuilder.pNew.wsFlags & WHERE_COLUMN_NULL) != 0
                    || (pBuilder.pNew.wsFlags & WHERE_COLUMN_IN) != 0
                    || (pBuilder.pNew.wsFlags & WHERE_SKIPSCAN) != 0
            );

            if eOp & WO_IN != 0 {
                let pExpr = t.pExpr;
                if db.ex(pExpr).UseXSelect() {
                    // "x IN (SELECT ...)":  TUNING: the SELECT returns 25 rows
                    let mut bRedundant = false;
                    nIn = 46;
                    debug_assert!(46 == sqlite3LogEst(25));

                    // The expression may actually be of the form (x, y) IN
                    // (SELECT...).  In this case there is a separate term for
                    // each of (x) and (y).  However, the nIn multiplier should
                    // only be applied once, not once for each such term. The
                    // following loop checks that pTerm is the first such term
                    // in use, and sets nIn back to 0 if it is not.
                    let nl = pBuilder.pNew.nLTerm as usize;
                    for i in 0..nl - 1 {
                        if let Some(o) = pBuilder.pNew.lterm(i) {
                            let ot = wi.t(o);
                            if ot.pExpr == pExpr {
                                nIn = 0;
                                if ot.u.x.iField == t.u.x.iField {
                                    // Detect when two or more columns of an
                                    // index match the same column of a vector
                                    // IN operater, and avoid adding the column
                                    // to the WhereLoop more than once.  See
                                    // tag-20250707-01 in test/rowvalue.test
                                    bRedundant = true;
                                }
                            }
                        }
                    }
                    if bRedundant {
                        pBuilder.pNew.nLTerm -= 1;
                        break 'body;
                    }
                } else {
                    let l = db.ex(pExpr).x.pList;
                    if l.nn() && db.el(l).nExpr() != 0 {
                        // "x IN (value, value, ...)"
                        nIn = sqlite3LogEst(db.el(l).nExpr() as u64) as i32;
                    }
                }
                if hasStat1 != 0 && rLogSize >= 10 {
                    // Let:
                    //   N = the total number of rows in the table
                    //   K = the number of entries on the RHS of the IN operator
                    //   M = the number of rows in the table that match terms to
                    //       the to the left in the same index.  If the IN
                    //       operator is on the left-most index column, M==N.
                    //
                    // Given the definitions above, it is better to omit the IN
                    // operator from the index lookup and instead do a scan of
                    // the M elements, testing each scanned row against the IN
                    // operator separately, if:
                    //
                    //        M*log(K) < K*log(N)
                    //
                    // Our estimates for M, K, and N might be inaccurate, so we
                    // build in a safety margin of 2 (LogEst: 10) that favors
                    // using the IN operator with the index, as using an index
                    // has better worst-case behavior.  If we do not have real
                    // sqlite_stat1 data, always prefer to use the index.  Do
                    // not bother with this optimization on very small tables
                    // (less than 2 rows) as it is pointless in that case.
                    let M = aiRowLogEst[saved_nEq as usize] as i32;
                    let logK = estLog(nIn as LogEst) as i32;
                    // TUNING      v-----  10 to bias toward indexed IN
                    let x = M + logK + 10 - (nIn + rLogSize as i32);
                    if x >= 0 {
                        // prefers indexed lookup
                    } else if nInMul < 2 && db.OptimizationEnabled(SQLITE_SeekScan) {
                        // prefers skip-scan
                        pBuilder.pNew.wsFlags |= WHERE_IN_SEEKSCAN;
                    } else {
                        // prefers normal scan
                        break 'body;
                    }
                }
                pBuilder.pNew.wsFlags |= WHERE_COLUMN_IN;
            } else if eOp & (WO_EQ | WO_IS) != 0 {
                let iCol = aiColumn[saved_nEq as usize];
                pBuilder.pNew.wsFlags |= WHERE_COLUMN_EQ;
                debug_assert!(saved_nEq == pBuilder.pNew.u.btree.nEq);
                if iCol == XN_ROWID || (iCol >= 0 && nInMul == 0 && saved_nEq as i32 == nKeyCol - 1)
                {
                    if iCol == XN_ROWID
                        || uniqNotNull != 0
                        || (nKeyCol == 1 && onError != 0 && (eOp & WO_EQ) != 0)
                    {
                        pBuilder.pNew.wsFlags |= WHERE_ONEROW;
                    } else {
                        pBuilder.pNew.wsFlags |= WHERE_UNQ_WANTED;
                    }
                }
                if scan.iEquiv > 1 {
                    pBuilder.pNew.wsFlags |= WHERE_TRANSCONS;
                }
            } else if eOp & WO_ISNULL != 0 {
                pBuilder.pNew.wsFlags |= WHERE_COLUMN_NULL;
            } else {
                let nVecLen = whereRangeVectorLen(db, iCursor, pProbe, saved_nEq as i32, &t);
                if eOp & (WO_GT | WO_GE) != 0 {
                    pBuilder.pNew.wsFlags |= WHERE_COLUMN_RANGE | WHERE_BTM_LIMIT;
                    pBuilder.pNew.u.btree.nBtm = nVecLen as u16;
                    pBtm = Some(pTerm);
                    pTop = None;
                    if t.wtFlags & TERM_LIKEOPT != 0 {
                        // Range constraints that come from the LIKE
                        // optimization are always used in pairs.
                        let tp = TermId {
                            i: pTerm.i + 1,
                            ..pTerm
                        };
                        pTop = Some(tp);
                        debug_assert!((tp.i as usize) < wi.wc(tp.wc as usize).a.len());
                        debug_assert!(wi.t(tp).wtFlags & TERM_LIKEOPT != 0);
                        debug_assert!(wi.t(tp).eOperator == WO_LT);
                        let pNew = &mut pBuilder.pNew;
                        let n = pNew.nLTerm as usize;
                        pNew.set_lterm(n, Some(tp));
                        pNew.nLTerm += 1;
                        pNew.wsFlags |= WHERE_TOP_LIMIT;
                        pNew.u.btree.nTop = 1;
                    }
                } else {
                    debug_assert!(eOp & (WO_LT | WO_LE) != 0);
                    let pNew = &mut pBuilder.pNew;
                    pNew.wsFlags |= WHERE_COLUMN_RANGE | WHERE_TOP_LIMIT;
                    pNew.u.btree.nTop = nVecLen as u16;
                    pTop = Some(pTerm);
                    pBtm = if (pNew.wsFlags & WHERE_BTM_LIMIT) != 0 {
                        pNew.lterm(pNew.nLTerm as usize - 2)
                    } else {
                        None
                    };
                }
            }

            // At this point pNew->nOut is set to the number of rows expected
            // to be visited by the index scan before considering term pTerm,
            // or the values of nIn and nInMul. In other words, assuming that
            // all "x IN(...)" terms are replaced with "x = ?". This block
            // updates the value of pNew->nOut to account for pTerm (but not
            // nIn/nInMul).
            debug_assert!(pBuilder.pNew.nOut == saved_nOut);
            if pBuilder.pNew.wsFlags & WHERE_COLUMN_RANGE != 0 {
                // Adjust nOut using stat4 data. Or, if there is no stat4
                // data, using some other estimate.
                whereRangeScanEst(wi, pBtm, pTop, &mut pBuilder.pNew);
            } else {
                let pNew = &mut pBuilder.pNew;
                pNew.u.btree.nEq += 1;
                let nEq = pNew.u.btree.nEq as usize;
                debug_assert!(eOp & (WO_ISNULL | WO_EQ | WO_IN | WO_IS) != 0);

                debug_assert!(pNew.nOut == saved_nOut);
                if t.truthProb <= 0 && aiColumn[saved_nEq as usize] >= 0 {
                    debug_assert!((eOp & WO_IN) != 0 || nIn == 0);
                    pNew.nOut = (pNew.nOut as i32 + t.truthProb as i32 - nIn) as LogEst;
                } else {
                    pNew.nOut = (pNew.nOut as i32
                        + (aiRowLogEst[nEq] as i32 - aiRowLogEst[nEq - 1] as i32))
                        as LogEst;
                    if eOp & WO_ISNULL != 0 {
                        // TUNING: If there is no likelihood() value, assume
                        // that a "col IS NULL" expression matches twice as
                        // many rows as (col=?).
                        pNew.nOut += 10;
                    }
                }
            }

            // Set rCostIdx to the estimated cost of visiting selected rows in
            // the index.  The estimate is the sum of two values:
            //   1.  The cost of doing one search-by-key to find the first
            //       matching entry
            //   2.  Stepping forward in the index pNew->nOut times to find all
            //       additional matching entries.
            debug_assert!(szTabRow > 0);
            if idxType == SQLITE_IDXTYPE_IPK {
                // The pProbe->szIdxRow is low for an IPK table since the
                // interior pages are small.  Thus szIdxRow gives a good
                // estimate of seek cost.  But the leaf pages are full-size,
                // so pProbe->szIdxRow would badly under-estimate the scanning
                // cost.
                rCostIdx = pBuilder.pNew.nOut as i32 + 16;
            } else {
                rCostIdx = pBuilder.pNew.nOut as i32 + 1 + (15 * szIdxRow as i32) / szTabRow as i32;
            }
            rCostIdx = sqlite3LogEstAdd(rLogSize, rCostIdx as LogEst) as i32;

            // Estimate the cost of running the loop.  If all data is coming
            // from the index, then this is just the cost of doing the index
            // lookup and scan.  But if some data is coming out of the main
            // table, we also have to add in the cost of doing pNew->nOut
            // searches to locate the row in the main table that corresponds
            // to the index entry.
            {
                let pNew = &mut pBuilder.pNew;
                pNew.rRun = rCostIdx as LogEst;
                if (pNew.wsFlags & (WHERE_IDX_ONLY | WHERE_IPK | WHERE_EXPRIDX)) == 0 {
                    pNew.rRun = sqlite3LogEstAdd(pNew.rRun, (pNew.nOut as i32 + 16) as LogEst);
                }

                nOutUnadjusted = pNew.nOut;
                pNew.rRun = (pNew.rRun as i32 + nInMul as i32 + nIn) as LogEst;
                pNew.nOut = (pNew.nOut as i32 + nInMul as i32 + nIn) as LogEst;
            }
            whereLoopOutputAdjust(db, wi, pWC, &mut pBuilder.pNew, rSize);
            if fromExists != 0 {
                pBuilder.pNew.nOut = 0;
            }
            rc = whereLoopInsert(db, wi, pBuilder);

            if pBuilder.pNew.wsFlags & WHERE_COLUMN_RANGE != 0 {
                pBuilder.pNew.nOut = saved_nOut;
            } else {
                pBuilder.pNew.nOut = nOutUnadjusted;
            }

            {
                let pNew = &pBuilder.pNew;
                if (pNew.wsFlags & WHERE_TOP_LIMIT) == 0
                    && (pNew.u.btree.nEq as i32) < nColumn
                    && ((pNew.u.btree.nEq as i32) < nKeyCol || idxType != SQLITE_IDXTYPE_PRIMARYKEY)
                {
                    if pNew.u.btree.nEq > 3 {
                        sqlite3ProgressCheck(db);
                    }
                    whereLoopAddBtreeIndex(
                        db,
                        wi,
                        pBuilder,
                        pSrc,
                        pProbe,
                        (nInMul as i32 + nIn) as LogEst,
                    );
                }
            }
            pBuilder.pNew.nOut = saved_nOut;
        }
        next = whereScanNext(db, wi, &mut scan);
    }
    {
        let pNew = &mut pBuilder.pNew;
        pNew.prereq = saved_prereq;
        pNew.u.btree.nEq = saved_nEq;
        pNew.u.btree.nBtm = saved_nBtm;
        pNew.u.btree.nTop = saved_nTop;
        pNew.nSkip = saved_nSkip;
        pNew.wsFlags = saved_wsFlags;
        pNew.nOut = saved_nOut;
        pNew.nLTerm = saved_nLTerm;
    }

    // Consider using a skip-scan if there are no WHERE clause constraints
    // available for the left-most terms of the index, and if the average
    // number of repeats in the left-most terms is at least 18.
    //
    // The magic number 18 is selected on the basis that scanning 17 rows
    // is almost always quicker than an index seek (even though if the index
    // contains fewer than 2^17 rows we assume otherwise in other parts of
    // the code). And, even if it is not, it should not be too much slower.
    // On the other hand, the extra seeks could end up being significantly
    // more expensive.
    debug_assert!(42 == sqlite3LogEst(18));
    if saved_nEq == saved_nSkip
        && (saved_nEq as i32 + 1) < nKeyCol
        && saved_nEq == pBuilder.pNew.nLTerm
        && noSkipScan == 0
        && hasStat1 != 0
        && db.OptimizationEnabled(SQLITE_SkipScan)
        && aiRowLogEst[saved_nEq as usize + 1] >= 42 // TUNING: Minimum for skip-scan
        && fromExists == 0
        && {
            let n = pBuilder.pNew.nLTerm as i32 + 1;
            rc = whereLoopResize(&mut pBuilder.pNew, n);
            rc == SQLITE_OK
        }
    {
        let mut nIter: i32;
        {
            let pNew = &mut pBuilder.pNew;
            pNew.u.btree.nEq += 1;
            pNew.nSkip += 1;
            let n = pNew.nLTerm as usize;
            pNew.set_lterm(n, None);
            pNew.nLTerm += 1;
            pNew.wsFlags |= WHERE_SKIPSCAN;
            nIter =
                aiRowLogEst[saved_nEq as usize] as i32 - aiRowLogEst[saved_nEq as usize + 1] as i32;
            pNew.nOut = (pNew.nOut as i32 - nIter) as LogEst;
        }
        // TUNING:  Because uncertainties in the estimates for skip-scan
        // queries, add a 1.375 fudge factor to make skip-scan slightly less
        // likely.
        nIter += 5;
        whereLoopAddBtreeIndex(
            db,
            wi,
            pBuilder,
            pSrc,
            pProbe,
            (nIter + nInMul as i32) as LogEst,
        );
        let pNew = &mut pBuilder.pNew;
        pNew.nOut = saved_nOut;
        pNew.u.btree.nEq = saved_nEq;
        pNew.nSkip = saved_nSkip;
        pNew.wsFlags = saved_wsFlags;
    }

    rc
}

/// Return True if it is possible that pIndex might be useful in
/// implementing the ORDER BY clause in pBuilder.
///
/// Return False if pBuilder does not contain an ORDER BY clause or
/// if there is no way for pIndex to be useful in implementing that
/// ORDER BY clause.
fn indexMightHelpWithOrderBy(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pIndex: &IndexRef,
    iCursor: i32,
) -> i32 {
    if pIndex.borrow().bUnordered != 0 {
        return 0;
    }
    let pOB = wi.pOrderBy;
    if pOB.is_null() {
        return 0;
    }
    let (aiColumn, nKeyCol, hasColExpr) = {
        let p = pIndex.borrow();
        (p.aiColumn.clone(), p.nKeyCol as usize, p.aColExpr.is_some())
    };
    let n = db.el(pOB).nExpr();
    for ii in 0..n as usize {
        let e = db.el(pOB).a[ii].pExpr;
        let pExpr = sqlite3ExprSkipCollateAndLikely(db, e);
        if pExpr.is_null() {
            continue;
        }
        let (op, iTable, iColumn) = {
            let x = db.ex(pExpr);
            (x.op, x.iTable, x.iColumn)
        };
        if (op == TK_COLUMN || op == TK_AGG_COLUMN) && iTable == iCursor {
            if iColumn < 0 {
                return 1;
            }
            for jj in 0..nKeyCol {
                if iColumn == aiColumn[jj] {
                    return 1;
                }
            }
        } else if hasColExpr {
            for jj in 0..nKeyCol {
                if aiColumn[jj] != XN_EXPR {
                    continue;
                }
                let ce = idx_col_expr(db, wi, pIndex, jj as i32);
                if sqlite3ExprCompareSkip(db, pExpr, ce, iCursor) == 0 {
                    return 1;
                }
            }
        }
    }
    0
}

/// Check to see if a partial index with pPartIndexWhere can be used
/// in the current query.  Return true if it can be and false if not.
fn whereUsablePartialIndex(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    iTab: i32,
    jointype: u8,
    pWC: WcId,
    pWhere: ExprId,
) -> i32 {
    let mut pWhere = pWhere;
    if jointype & JT_LTORJ != 0 {
        return 0;
    }
    while db.ex(pWhere).op == TK_AND {
        let l = db.ex(pWhere).pLeft;
        if whereUsablePartialIndex(db, wi, iTab, jointype, pWC, l) == 0 {
            return 0;
        }
        pWhere = db.ex(pWhere).pRight;
    }
    let n = wi.wc(pWC).a.len();
    for i in 0..n {
        let t = wi.wc(pWC).a[i];
        let pExpr = t.pExpr;
        let (outer, iJoin) = (db.ex(pExpr).HasProperty(EP_OuterON), db.ex(pExpr).w.iJoin);
        if (!outer || iJoin == iTab)
            && ((jointype & JT_OUTER) == 0 || outer)
            && sqlite3ExprImpliesExpr(db, pExpr, pWhere, iTab) != 0
            && sqlite3ExprImpliesExpr(db, pExpr, pWhere, -1) == 0
            && (t.wtFlags & TERM_VNULL) == 0
        {
            return 1;
        }
    }
    0
}

/// pIdx is an index containing expressions.  Check it see if any of the
/// expressions in the index match the pExpr expression.
fn exprIsCoveredByIndex(
    db: &mut Sqlite3,
    pExpr: ExprId,
    aiColumn: &[i16],
    aColExpr: ExprListId,
    iTabCur: i32,
) -> i32 {
    for i in 0..aiColumn.len() {
        if aiColumn[i] == XN_EXPR {
            let e = db.el(aColExpr).a[i].pExpr;
            if sqlite3ExprCompare(db, false, pExpr, e, iTabCur) == 0 {
                return 1;
            }
        }
    }
    0
}

/// Structure passed to the whereIsCoveringIndex Walker callback.
struct CoveringIndexCheck {
    /// The index columns (C pIdx->aiColumn, nColumn entries)
    aiColumn: Vec<i16>,
    /// The index expressions (C pIdx->aColExpr) imported into the arena
    aColExpr: ExprListId,
    /// C pIdx->bHasExpr
    bHasExpr: u8,
    /// Cursor number for the corresponding table
    iTabCur: i32,
    /// Uses an indexed expression
    bExpr: u8,
    /// Uses an unindexed column not within an indexed expr
    bUnidx: u8,
}

/// Information passed in is pWalk->u.pCovIdxCk.  Call it pCk.
///
/// If the Expr node references the table with cursor pCk->iTabCur, then
/// make sure that column is covered by the index pCk->pIdx.  We know that
/// all columns less than 63 (really BMS-1) are covered, so we don't need
/// to check them.  But we do need to check any column at 63 or greater.
///
/// If the index does not cover the column, then set pWalk->eCode to
/// non-zero and return WRC_Abort to stop the search.
///
/// If this node does not disprove that the index can be a covering index,
/// then just return WRC_Continue, to continue the search.
///
/// If pCk->pIdx contains indexed expressions and one of those expressions
/// matches pExpr, then prune the search.
fn whereIsCoveringIndexWalkCallback(pWalk: &mut Walker, pExpr: ExprId) -> i32 {
    let (aiColumn, aColExpr, bHasExpr, iTabCur) = {
        let ck = pWalk
            .u
            .any
            .as_ref()
            .unwrap()
            .downcast_ref::<CoveringIndexCheck>()
            .unwrap();
        (ck.aiColumn.clone(), ck.aColExpr, ck.bHasExpr, ck.iTabCur)
    };
    let (op, iTable, iColumn) = {
        let x = pWalk.db.ex(pExpr);
        (x.op, x.iTable, x.iColumn)
    };
    if op == TK_COLUMN || op == TK_AGG_COLUMN {
        // if( pExpr->iColumn<(BMS-1) && pIdx->bHasExpr==0 ) return WRC_Continue;
        if iTable != iTabCur {
            return WRC_Continue;
        }
        for i in 0..aiColumn.len() {
            if aiColumn[i] == iColumn {
                return WRC_Continue;
            }
        }
        pWalk
            .u
            .any
            .as_mut()
            .unwrap()
            .downcast_mut::<CoveringIndexCheck>()
            .unwrap()
            .bUnidx = 1;
        return WRC_Abort;
    } else if bHasExpr != 0
        && exprIsCoveredByIndex(pWalk.db, pExpr, &aiColumn, aColExpr, iTabCur) != 0
    {
        pWalk
            .u
            .any
            .as_mut()
            .unwrap()
            .downcast_mut::<CoveringIndexCheck>()
            .unwrap()
            .bExpr = 1;
        return WRC_Prune;
    }
    WRC_Continue
}

/// pIdx is an index that covers all of the low-number columns used by
/// pWInfo->pSelect (columns from 0 through 62) or an index that has
/// expressions terms.  Hence, we cannot determine whether or not it is
/// a covering index by using the colUsed bitmasks.  We have to do a search
/// to see if the index is covering.  This routine does that search.
///
/// The return value is one of these:
///
///      0                The index is definitely not a covering index
///
///      WHERE_IDX_ONLY   The index is definitely a covering index
///
///      WHERE_EXPRIDX    The index is likely a covering index, but it is
///                       difficult to determine precisely because of the
///                       expressions that are indexed.  Score it as a
///                       covering index, but still keep the main table open
///                       just in case we need it.
///
/// This routine is an optimization.  It is always safe to return zero.
/// But returning one of the other two values when zero should have been
/// returned can lead to incorrect bytecode and assertion faults.
fn whereIsCoveringIndex(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pIdx: &IndexRef,
    iTabCur: i32,
) -> u32 {
    if wi.pSelect.is_null() {
        // We don't have access to the full query, so we cannot check to see
        // if pIdx is covering.  Assume it is not.
        return 0;
    }
    let (aiColumn, bHasExpr, nColumn) = {
        let p = pIdx.borrow();
        (
            p.aiColumn[..p.nColumn as usize].to_vec(),
            p.bHasExpr,
            p.nColumn as usize,
        )
    };
    if bHasExpr == 0 {
        let mut i = 0;
        while i < nColumn {
            if aiColumn[i] as i32 >= BMS - 1 {
                break;
            }
            i += 1;
        }
        if i >= nColumn {
            // pIdx does not index any columns greater than 62, but we know
            // from colMask that columns greater than 62 are used, so this is
            // not a covering index
            return 0;
        }
    }
    let aColExpr = idx_col_exprs(db, wi, pIdx);
    let pSelect = wi.pSelect;
    let ck = CoveringIndexCheck {
        aiColumn,
        aColExpr,
        bHasExpr,
        iTabCur,
        bExpr: 0,
        bUnidx: 0,
    };
    let mut w = Walker::new(db);
    w.xExprCallback = Some(whereIsCoveringIndexWalkCallback);
    w.xSelectCallback = Some(crate::walker::sqlite3SelectWalkNoop);
    w.u.any = Some(Box::new(ck));
    crate::walker::sqlite3WalkSelect(&mut w, pSelect);
    let ck =
        w.u.any
            .take()
            .unwrap()
            .downcast::<CoveringIndexCheck>()
            .unwrap();
    if ck.bUnidx != 0 {
        0
    } else if ck.bExpr != 0 {
        WHERE_EXPRIDX
    } else {
        WHERE_IDX_ONLY
    }
}

/// This function is called for a partial index - one with a WHERE clause -
/// in two scenarios. In both cases, it determines whether or not the WHERE
/// clause on the index implies that a column of the table may be safely
/// replaced by a constant expression. For example, in the following
/// SELECT:
///
///   CREATE INDEX i1 ON t1(b, c) WHERE a=<expr>;
///   SELECT a, b, c FROM t1 WHERE a=<expr> AND b=?;
///
/// The "a" in the select-list may be replaced by <expr>, iff:
///
///    (a) <expr> is a constant expression, and
///    (b) The (a=<expr>) comparison uses the BINARY collation sequence, and
///    (c) Column "a" has an affinity other than NONE or BLOB.
///
/// If argument pItem is NULL, then pMask must not be NULL. In this case
/// this function is being called as part of determining whether or not
/// pIdx is a covering index. This function clears any bits in (*pMask)
/// corresponding to columns that may be replaced by constants as described
/// above.
///
/// Otherwise, if pItem is not NULL, then this function is being called
/// as part of coding a loop that uses index pIdx. In this case, add entries
/// to the Parse.pIdxPartExpr list for each column that can be replaced
/// by a constant.
fn wherePartIdxExpr(
    db: &mut Sqlite3,
    pIdx: &IndexRef,
    pPart: ExprId,
    pMask: &mut Option<&mut Bitmask>,
    iIdxCur: i32,
    pItem: Option<SrcItemId>,
) {
    let mut pPart = pPart;
    debug_assert!(pItem.map_or(true, |it| (db.item(it).fg.jointype & JT_RIGHT) == 0));
    debug_assert!((pItem.is_none() || pMask.is_none()) && (pMask.is_some() || pItem.is_some()));

    if db.ex(pPart).op == TK_AND {
        let r = db.ex(pPart).pRight;
        wherePartIdxExpr(db, pIdx, r, pMask, iIdxCur, pItem);
        pPart = db.ex(pPart).pLeft;
    }

    let op = db.ex(pPart).op;
    if op == TK_EQ || op == TK_IS {
        let pLeft = db.ex(pPart).pLeft;
        let pRight = db.ex(pPart).pRight;

        if db.ex(pLeft).op != TK_COLUMN {
            return;
        }
        if sqlite3ExprIsConstant(db, false, pRight) == 0 {
            return;
        }
        let c = sqlite3ExprCompareCollSeq(db, pPart);
        if !isBinary(&c) {
            return;
        }
        let iColumn = db.ex(pLeft).iColumn;
        if iColumn < 0 {
            return;
        }
        let aff = pIdx.borrow().table().borrow().aCol[iColumn as usize].affinity;
        if aff >= SQLITE_AFF_TEXT {
            if let Some(it) = pItem {
                let bNullRow = (db.item(it).fg.jointype & (JT_LEFT | JT_LTORJ)) != 0;
                let e = sqlite3ExprDup(db, pRight, 0);
                let iDataCur = db.item(it).iCursor;
                let p = IndexedExpr {
                    pExpr: e,
                    iDataCur,
                    iIdxCur,
                    iIdxCol: iColumn as i32,
                    bMaybeNullRow: bNullRow as u8,
                    aff,
                };
                // C prepends to the pIdxPartExpr linked list; the Vec keeps push order
                // (C list head = last element), as expr.rs scans it from the end.
                db.pp().pIdxPartExpr.push(p);
            } else if (iColumn as i32) < BMS - 1 {
                if let Some(m) = pMask.as_mut() {
                    **m &= !((1 as Bitmask) << iColumn);
                }
            }
        }
    }
}

/// Add all WhereLoop objects for a single table of the join where the table
/// is identified by pBuilder->pNew->iTab.  That table is guaranteed to be
/// a b-tree table, not a virtual table.
///
/// The costs (WhereLoop.rRun) of the b-tree loops added by this function
/// are calculated as follows:
///
/// For a full scan, assuming the table (or index) contains nRow rows:
///
///     cost = nRow * 3.0                    // full-table scan
///     cost = nRow * K                      // scan of covering index
///     cost = nRow * (K+3.0)                // scan of non-covering index
///
/// where K is a value between 1.1 and 3.0 set based on the relative
/// estimated average size of the index and table records.
///
/// For an index scan, where nVisit is the number of index rows visited
/// by the scan, and nSeek is the number of seek operations required on
/// the index b-tree:
///
///     cost = nSeek * (log(nRow) + K * nVisit)          // covering index
///     cost = nSeek * (log(nRow) + (K+3.0) * nVisit)    // non-covering index
///
/// Normally, nSeek is 1. nSeek values greater than 1 come about if the
/// WHERE clause includes "x IN (....)" terms used in place of "x=?". Or
/// when implicit "x IN (SELECT x FROM tbl)" terms are added for skip-scans.
///
/// The estimated values (nRow, nVisit, nSeek) often contain a large amount
/// of uncertainty.  For this reason, scoring is designed to pick plans that
/// "do the least harm" if the estimates are inaccurate.  For example, a
/// log(nRow) factor is omitted from a non-covering index scan in order to
/// bias the scoring in favor of using an index, since the worst-case
/// performance of using an index is far better than the worst-case
/// performance of a full table scan.
fn whereLoopAddBtree(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pBuilder: &mut WhereLoopBuilder,
    mPrereq: Bitmask,
) -> i32 {
    let mut rc = SQLITE_OK; // Return code
    let mut iSortIdx: i32 = 1; // Index number
    let mut rSize: LogEst; // number of rows in the table

    let pTabList = wi.pTabList;
    let pSrc = SrcItemId(pTabList, pBuilder.pNew.iTab as u32);
    let pTab = item_tab(db, pSrc);
    let pWC = pBuilder.pWC;
    debug_assert!(!pTab.borrow().IsVirtual());

    let fg = db.item(pSrc).fg;
    let iCursor = db.item(pSrc).iCursor;
    let probes: Vec<IndexRef>;
    if fg.isIndexedBy != 0 {
        debug_assert!(fg.isCte == 0);
        // An INDEXED BY clause specifies a particular index to use
        probes = db.item(pSrc).u2.pIBIndex.clone().into_iter().collect();
    } else if !pTab.borrow().HasRowid() {
        probes = pTab.borrow().pIndex.clone();
    } else {
        // There is no INDEXED BY clause.  Create a fake Index object in
        // local variable sPk to represent the rowid primary key index.  Make
        // this fake index the first in a chain of Index objects with all of
        // the real indices to follow
        let sPk = Index {
            nKeyCol: 1,
            nColumn: 1,
            aiColumn: vec![-1],
            aiRowLogEst: vec![pTab.borrow().nRowLogEst, 0],
            aSortOrder: vec![0],
            onError: OE_Replace,
            pTable: Rc::downgrade(&pTab),
            szIdxRow: 3, // TUNING: Interior rows of IPK table are very small
            idxType: SQLITE_IDXTYPE_IPK,
            ..Default::default()
        };
        let mut v = vec![Rc::new(std::cell::RefCell::new(sPk))];
        if fg.notIndexed == 0 {
            // The real indices of the table are only considered if the
            // NOT INDEXED qualifier is omitted from the FROM clause
            v.extend(pTab.borrow().pIndex.iter().cloned());
        }
        probes = v;
    }
    rSize = pTab.borrow().nRowLogEst;

    // Automatic indexes
    if pBuilder.pOrSet.is_none() // Not part of an OR optimization
        && (wi.wctrlFlags & (WHERE_RIGHT_JOIN | WHERE_OR_SUBCLAUSE)) == 0
        && (db.flags & SQLITE_AutoIndex) != 0
        && fg.isIndexedBy == 0 // Has no INDEXED BY clause
        && fg.notIndexed == 0 // Has no NOT INDEXED clause
        && fg.isCorrelated == 0 // Not a correlated subquery
        && fg.isRecursive == 0 // Not a recursive common table expression.
        && (fg.jointype & JT_RIGHT) == 0
    // Not the right tab of a RIGHT JOIN
    {
        // Generate auto-index WhereLoops
        let rLogSize = estLog(rSize);
        let nTerm = wi.wc(pWC).a.len();
        let mut k = 0usize;
        while rc == SQLITE_OK && k < nTerm {
            let t = wi.wc(pWC).a[k];
            let pTerm = TermId::new(pWC, k);
            k += 1;
            if t.prereqRight & pBuilder.pNew.maskSelf != 0 {
                continue;
            }
            if termCanDriveIndex(db, &t, pSrc, 0) != 0 {
                let (isView, ephem) = {
                    let tb = pTab.borrow();
                    (tb.IsView(), (tb.tabFlags & TF_Ephemeral) != 0)
                };
                let pNew = &mut pBuilder.pNew;
                pNew.u.btree.nEq = 1;
                pNew.nSkip = 0;
                pNew.u.btree.pIndex = None;
                pNew.nLTerm = 1;
                pNew.set_lterm(0, Some(pTerm));
                // TUNING: One-time cost for computing the automatic index is
                // estimated to be X*N*log2(N) where N is the number of rows in
                // the table being indexed and where X is 7 (LogEst=28) for
                // normal tables or 0.5 (LogEst=-10) for views and subqueries.
                // The value of X is smaller for views and subqueries so that
                // the query planner will be more aggressive about generating
                // automatic indexes for those objects, since there is no
                // opportunity to add schema indexes on subqueries and views.
                let mut rSetup = rLogSize as i32 + rSize as i32;
                if !isView && !ephem {
                    rSetup += 28;
                } else {
                    rSetup -= 25; // Greatly reduced setup cost for auto indexes
                    // on ephemeral materializations of views
                }
                if rSetup < 0 {
                    rSetup = 0;
                }
                pNew.rSetup = rSetup as LogEst;
                // TUNING: Each index lookup yields 20 rows in the table.  This
                // is more than the usual guess of 10 rows, since we have no
                // way of knowing how selective the index will ultimately be.
                // It would not be unreasonable to make this value much larger.
                pNew.nOut = 43;
                debug_assert!(43 == sqlite3LogEst(20));
                pNew.rRun = sqlite3LogEstAdd(rLogSize, pNew.nOut);
                pNew.wsFlags = WHERE_AUTO_INDEX;
                pNew.prereq = mPrereq | t.prereqRight;
                rc = whereLoopInsert(db, wi, pBuilder);
            }
        }
    }

    // Loop over all indices. If there was an INDEXED BY clause, then only
    // consider index pProbe.
    'probe: for pProbe in probes.iter() {
        if rc != SQLITE_OK {
            break;
        }
        'next: {
            let hasPart = pProbe.borrow().pPartIdxWhere.is_some();
            if hasPart {
                let pw = idx_part_where(db, wi, pProbe);
                if whereUsablePartialIndex(db, wi, iCursor, fg.jointype, pWC, pw) == 0 {
                    break 'next; // Partial index inappropriate for this query
                }
            }
            if pProbe.borrow().bNoQuery != 0 {
                break 'next;
            }
            rSize = pProbe.borrow().aiRowLogEst[0];
            {
                let pNew = &mut pBuilder.pNew;
                pNew.u.btree.nEq = 0;
                pNew.u.btree.nBtm = 0;
                pNew.u.btree.nTop = 0;
                pNew.u.btree.nDistinctCol = 0;
                pNew.nSkip = 0;
                pNew.nLTerm = 0;
                pNew.iSortIdx = 0;
                pNew.rSetup = 0;
                pNew.prereq = mPrereq;
                pNew.nOut = rSize;
                pNew.u.btree.pIndex = Some(pProbe.clone());
                pNew.u.btree.pOrderBy = ExprListId::NULL;
            }
            let b = indexMightHelpWithOrderBy(db, wi, pProbe, iCursor);

            // The ONEPASS_DESIRED flags never occurs together with ORDER BY
            debug_assert!((wi.wctrlFlags & WHERE_ONEPASS_DESIRED) == 0 || b == 0);
            let idxType = pProbe.borrow().idxType;
            if idxType == SQLITE_IDXTYPE_IPK {
                // Integer primary key index
                pBuilder.pNew.wsFlags = WHERE_IPK;

                // Full table scan
                pBuilder.pNew.iSortIdx = if b != 0 { iSortIdx as u8 } else { 0 };
                // TUNING: Cost of full table scan is 3.0*N.  The 3.0 factor is
                // an extra cost designed to discourage the use of full table
                // scans, since index lookups have better worst-case
                // performance if our stat guesses are wrong.
                pBuilder.pNew.rRun = (rSize as i32 + 16) as LogEst;
                whereLoopOutputAdjust(db, wi, pWC, &mut pBuilder.pNew, rSize);
                if fg.isSubquery != 0 {
                    let it = db.item(pSrc);
                    let viaCo = it.fg.viaCoroutine != 0;
                    let sel = it.u4.pSubq.as_ref().unwrap().pSelect;
                    if viaCo {
                        pBuilder.pNew.wsFlags |= WHERE_COROUTINE;
                    }
                    // Do not set btree.pOrderBy for a recursive CTE. In this
                    // case the ORDER BY clause does not determine the overall
                    // order that rows are emitted from the CTE in.
                    let s = db.sel(sel);
                    if (s.selFlags & SF_Recursive) == 0 {
                        pBuilder.pNew.u.btree.pOrderBy = s.pOrderBy;
                    }
                } else if fg.fromExists != 0 {
                    pBuilder.pNew.nOut = 0;
                }
                rc = whereLoopInsert(db, wi, pBuilder);
                pBuilder.pNew.nOut = rSize;
                if rc != 0 {
                    break 'probe;
                }
            } else {
                let mut m: Bitmask;
                let (isCovering, colNotIdxed, bHasExpr, bHasVCol, bUnordered, szIdxRow) = {
                    let p = pProbe.borrow();
                    (
                        p.isCovering,
                        p.colNotIdxed,
                        p.bHasExpr,
                        p.bHasVCol,
                        p.bUnordered,
                        p.szIdxRow,
                    )
                };
                if isCovering != 0 {
                    m = 0;
                    pBuilder.pNew.wsFlags = WHERE_IDX_ONLY | WHERE_INDEXED;
                } else {
                    m = db.item(pSrc).colUsed & colNotIdxed;
                    if hasPart {
                        let pw = idx_part_where(db, wi, pProbe);
                        wherePartIdxExpr(db, pProbe, pw, &mut Some(&mut m), 0, None);
                    }
                    pBuilder.pNew.wsFlags = WHERE_INDEXED;
                    if m == TOPBIT || (bHasExpr != 0 && bHasVCol == 0 && m != 0) {
                        let isCov = whereIsCoveringIndex(db, wi, pProbe, iCursor);
                        if isCov == 0 {
                            debug_assert!(m != 0);
                        } else {
                            m = 0;
                            pBuilder.pNew.wsFlags |= isCov;
                        }
                    } else if m == 0
                        && (pTab.borrow().HasRowid()
                            || wi.pSelect.nn()
                            || sqlite3FaultSim(700) != 0)
                    {
                        pBuilder.pNew.wsFlags = WHERE_IDX_ONLY | WHERE_INDEXED;
                    }
                }

                // Full scan via index
                let szTabRow = pTab.borrow().szTabRow;
                if b != 0
                    || !pTab.borrow().HasRowid()
                    || hasPart
                    || fg.isIndexedBy != 0
                    || (m == 0
                        && bUnordered == 0
                        && (szIdxRow < szTabRow)
                        && (wi.wctrlFlags & WHERE_ONEPASS_DESIRED) == 0
                        && crate::global::sqlite3GlobalConfig().bUseCis != 0
                        && db.OptimizationEnabled(SQLITE_CoverIdxScan))
                {
                    pBuilder.pNew.iSortIdx = if b != 0 { iSortIdx as u8 } else { 0 };

                    // The cost of visiting the index rows is N*K, where K is
                    // between 1.1 and 3.0, depending on the relative sizes of
                    // the index and table rows.
                    pBuilder.pNew.rRun =
                        (rSize as i32 + 1 + (15 * szIdxRow as i32) / szTabRow as i32) as LogEst;
                    if m != 0 {
                        // If this is a non-covering index scan, add in the
                        // cost of doing table lookups.  The cost will be 3x
                        // the number of lookups.  Take into account WHERE
                        // clause terms that can be satisfied using just the
                        // index, and that do not require a table lookup.
                        let mut nLookup: i32 = rSize as i32 + 16; // Base cost:  N*3
                        let iCur = iCursor;
                        let n2 = wi.wc(SWC).a.len();
                        for ii in 0..n2 {
                            let t = wi.wc(SWC).a[ii];
                            if sqlite3ExprCoveredByIndex(db, t.pExpr, iCur, pProbe) == 0 {
                                break;
                            }
                            // pTerm can be evaluated using just the index.  So
                            // reduce the expected number of table lookups
                            // accordingly
                            if t.truthProb <= 0 {
                                nLookup += t.truthProb as i32;
                            } else {
                                nLookup -= 1;
                                if t.eOperator & (WO_EQ | WO_IS) != 0 {
                                    nLookup -= 19;
                                }
                            }
                        }

                        pBuilder.pNew.rRun =
                            sqlite3LogEstAdd(pBuilder.pNew.rRun, nLookup as LogEst);
                    }
                    whereLoopOutputAdjust(db, wi, pWC, &mut pBuilder.pNew, rSize);
                    if (fg.jointype & JT_RIGHT) != 0 && pProbe.borrow().aColExpr.is_some() {
                        // Do not do an SCAN of a index-on-expression in a
                        // RIGHT JOIN because the cursor used to access the
                        // index might not be positioned to the correct row
                        // during the right-join no-match loop.
                    } else {
                        if fg.fromExists != 0 {
                            pBuilder.pNew.nOut = 0;
                        }
                        rc = whereLoopInsert(db, wi, pBuilder);
                    }
                    pBuilder.pNew.nOut = rSize;
                    if rc != 0 {
                        break 'probe;
                    }
                }
            }

            pBuilder.bldFlags1 = 0;
            rc = whereLoopAddBtreeIndex(db, wi, pBuilder, pSrc, pProbe, 0);
            if pBuilder.bldFlags1 == SQLITE_BLDF1_INDEXED as u8 {
                // If a non-unique index is used, or if a prefix of the key for
                // unique index is used (making the index functionally
                // non-unique) then the sqlite_stat1 data becomes important for
                // scoring the plan
                pTab.borrow_mut().tabFlags |= TF_MaybeReanalyze;
            }
        }
        if fg.isIndexedBy != 0 {
            break;
        }
        iSortIdx += 1;
    }
    rc
}

/// Add WhereLoop entries to handle OR terms.  This works for either
/// btrees or virtual tables.
fn whereLoopAddOr(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pBuilder: &mut WhereLoopBuilder,
    mPrereq: Bitmask,
    mUnusable: Bitmask,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut sSum = WhereOrSet::default();

    let pWC = pBuilder.pWC;
    let nTerm = wi.wc(pWC).a.len();
    let pItem = SrcItemId(wi.pTabList, pBuilder.pNew.iTab as u32);
    let iCur = db.item(pItem).iCursor;

    // The multi-index OR optimization does not work for RIGHT and FULL JOIN
    if db.item(pItem).fg.jointype & JT_RIGHT != 0 {
        return SQLITE_OK;
    }
    let isVirtual = item_tab(db, pItem).borrow().IsVirtual();

    let mut k = 0usize;
    while k < nTerm && rc == SQLITE_OK {
        let pTerm = TermId::new(pWC, k);
        k += 1;
        let t = *wi.t(pTerm);
        if (t.eOperator & WO_OR) != 0 && (wi.orinfo(pTerm).indexable & pBuilder.pNew.maskSelf) != 0
        {
            let pOrWC = wi.orinfo(pTerm).wc;
            let nOr = wi.wc(pOrWC).a.len();
            let mut once = true;

            let mut sSubBuild = WhereLoopBuilder {
                pWC: pBuilder.pWC,
                pNew: std::mem::take(&mut pBuilder.pNew),
                pOrSet: Some(WhereOrSet::default()),
                bldFlags1: pBuilder.bldFlags1,
                bldFlags2: pBuilder.bldFlags2,
                iPlanLimit: pBuilder.iPlanLimit,
            };

            for io in 0..nOr {
                let ot = wi.wc(pOrWC).a[io];
                let nWcSaved = wi.aWC.len();
                if (ot.eOperator & WO_AND) != 0 {
                    sSubBuild.pWC = ot.u.pAndInfo.unwrap();
                } else if ot.leftCursor == iCur {
                    // tempWC: a clause made of the single term pOrTerm
                    wi.aWC.push(WhereClause {
                        pOuter: Some(pWC),
                        op: TK_AND,
                        hasOr: 0,
                        nBase: 1,
                        a: vec![ot],
                    });
                    sSubBuild.pWC = nWcSaved;
                } else {
                    continue;
                }
                sSubBuild.pOrSet = Some(WhereOrSet::default()); // sCur.n = 0
                if isVirtual {
                    rc = whereLoopAddVirtual(db, wi, &mut sSubBuild, mPrereq, mUnusable);
                } else {
                    rc = whereLoopAddBtree(db, wi, &mut sSubBuild, mPrereq);
                }
                if rc == SQLITE_OK {
                    rc = whereLoopAddOr(db, wi, &mut sSubBuild, mPrereq, mUnusable);
                }
                wi.aWC.truncate(nWcSaved);
                let sCur = sSubBuild.pOrSet.unwrap();
                if sCur.n == 0 {
                    sSum.n = 0;
                    break;
                } else if once {
                    whereOrMove(&mut sSum, &sCur);
                    once = false;
                } else {
                    let mut sPrev = WhereOrSet::default();
                    whereOrMove(&mut sPrev, &sSum);
                    sSum.n = 0;
                    for i in 0..sPrev.n as usize {
                        for j in 0..sCur.n as usize {
                            whereOrInsert(
                                &mut sSum,
                                sPrev.a[i].prereq | sCur.a[j].prereq,
                                sqlite3LogEstAdd(sPrev.a[i].rRun, sCur.a[j].rRun),
                                sqlite3LogEstAdd(sPrev.a[i].nOut, sCur.a[j].nOut),
                            );
                        }
                    }
                }
            }
            pBuilder.pNew = std::mem::take(&mut sSubBuild.pNew);
            {
                let pNew = &mut pBuilder.pNew;
                pNew.nLTerm = 1;
                pNew.set_lterm(0, Some(pTerm));
                pNew.wsFlags = WHERE_MULTI_OR;
                pNew.rSetup = 0;
                pNew.iSortIdx = 0;
                pNew.u = WhereLoopU::default();
            }
            let mut i = 0;
            while rc == SQLITE_OK && i < sSum.n as usize {
                // TUNING: Currently sSum.a[i].rRun is set to the sum of the
                // costs of all sub-scans required by the OR-scan. However, due
                // to rounding errors, it may be that the cost of the OR-scan
                // is equal to its most expensive sub-scan. Add the smallest
                // possible penalty (equivalent to multiplying the cost by
                // 1.07) to ensure that this does not happen. Otherwise, for
                // WHERE clauses such as the following where there is an index
                // on "y":
                //
                //     WHERE likelihood(x=?, 0.99) OR y=?
                //
                // the planner may elect to "OR" together a full-table scan and
                // an index lookup. And other similarly odd results.
                let pNew = &mut pBuilder.pNew;
                pNew.rRun = (sSum.a[i].rRun as i32 + 1) as LogEst;
                pNew.nOut = sSum.a[i].nOut;
                pNew.prereq = sSum.a[i].prereq;
                rc = whereLoopInsert(db, wi, pBuilder);
                i += 1;
            }
        }
    }
    rc
}

/// Add all WhereLoop objects for all tables
fn whereLoopAddAll(db: &mut Sqlite3, wi: &mut WhereInfo, pBuilder: &mut WhereLoopBuilder) -> i32 {
    let mut mPrereq: Bitmask = 0;
    let mut mPrior: Bitmask = 0;
    let pTabList = wi.pTabList;
    let nEnd = wi.nLevel as usize;
    let mut rc = SQLITE_OK;
    let mut bFirstPastRJ = false;
    let mut hasRightCrossJoin = false;

    // Verify that pNew has already been initialized
    debug_assert!(pBuilder.pNew.nLTerm == 0);
    debug_assert!(pBuilder.pNew.wsFlags == 0);

    pBuilder.iPlanLimit = SQLITE_QUERY_PLANNER_LIMIT as u32;
    for iTab in 0..nEnd {
        let pItem = SrcItemId(pTabList, iTab as u32);
        let mut mUnusable: Bitmask = 0;
        pBuilder.pNew.iTab = iTab as u8;
        pBuilder.iPlanLimit += SQLITE_QUERY_PLANNER_LIMIT_INCR as u32;
        let (iCursor, jointype, fromExists) = {
            let it = db.item(pItem);
            (it.iCursor, it.fg.jointype, it.fg.fromExists)
        };
        pBuilder.pNew.maskSelf = sqlite3WhereGetMask(&mut wi.sMaskSet, iCursor);
        if bFirstPastRJ || (jointype & (JT_OUTER | JT_CROSS | JT_LTORJ)) != 0 {
            // Add prerequisites to prevent reordering of FROM clause terms
            // across CROSS joins and outer joins.  The bFirstPastRJ boolean
            // prevents the right operand of a RIGHT JOIN from being swapped
            // with other elements even further to the right.
            //
            // The hasRightCrossJoin flag prevent FROM-clause terms from
            // moving from the right side of a LEFT JOIN or CROSS JOIN over to
            // the left side of that same join.  This is a required
            // restriction in the case of LEFT JOIN - an incorrect answer may
            // results if it is not enforced.  This restriction is not required
            // for CROSS JOIN.  It is provided merely as a means of controlling
            // join order, under the theory that no real-world queries that
            // care about performance actually use the CROSS JOIN syntax.
            if jointype & (JT_LTORJ | JT_CROSS) != 0 {
                hasRightCrossJoin = true;
            }
            mPrereq |= mPrior;
            bFirstPastRJ = (jointype & JT_RIGHT) != 0;
        } else if fromExists != 0 {
            // joins that result from the EXISTS-to-JOIN optimization should
            // not be moved to the left of any of their dependencies
            let nBase = wi.wc(SWC).nBase as usize;
            let maskSelf = pBuilder.pNew.maskSelf;
            for i in 0..nBase {
                let pa = wi.wc(SWC).a[i].prereqAll;
                if (maskSelf & pa) != 0 {
                    mPrereq |= pa & (maskSelf.wrapping_sub(1));
                }
            }
        } else if !hasRightCrossJoin {
            mPrereq = 0;
        }
        if item_tab(db, pItem).borrow().IsVirtual() {
            for p in (iTab + 1)..nEnd {
                let (jt, ic) = {
                    let it = db.item(SrcItemId(pTabList, p as u32));
                    (it.fg.jointype, it.iCursor)
                };
                if jt & (JT_OUTER | JT_CROSS) != 0 {
                    mUnusable |= sqlite3WhereGetMask(&mut wi.sMaskSet, ic);
                }
            }
            rc = whereLoopAddVirtual(db, wi, pBuilder, mPrereq, mUnusable);
        } else {
            rc = whereLoopAddBtree(db, wi, pBuilder, mPrereq);
        }
        if rc == SQLITE_OK && wi.wc(pBuilder.pWC).hasOr != 0 {
            rc = whereLoopAddOr(db, wi, pBuilder, mPrereq, mUnusable);
        }
        mPrior |= pBuilder.pNew.maskSelf;
        if rc != 0 || db.mallocFailed != 0 {
            if rc == SQLITE_DONE {
                // We hit the query planner search limit set by iPlanLimit
                crate::printf::sqlite3_log(
                    SQLITE_WARNING,
                    b"abbreviated query algorithm search",
                    &[],
                );
                rc = SQLITE_OK;
            } else {
                break;
            }
        }
    }

    whereLoopClear(&mut pBuilder.pNew);
    rc
}

/// Implementation of the order-by-subquery optimization:
///
/// WhereLoop pLoop, which the iLoop-th term of the nested loop, is really
/// a subquery or CTE that has an ORDER BY clause.  See if any of the terms
/// in the subquery ORDER BY clause will satisfy pOrderBy from the outer
/// query.  Mark off all satisfied terms (by setting bits in *pOBSat) and
/// return TRUE if they do.  If not, return false.
///
/// Example:
///
///    CREATE TABLE t1(a,b,c, PRIMARY KEY(a,b));
///    CREATE TABLE t2(x,y);
///    WITH t3(p,q) AS MATERIALIZED (SELECT x+y, x-y FROM t2 ORDER BY x+y)
///       SELECT * FROM t3 JOIN t1 ON a=q ORDER BY p, b;
///
/// The CTE named "t3" comes out in the natural order of "p", so the first
/// first them of "ORDER BY p,b" is satisfied by a sequential scan of "t3"
/// and sorting only needs to occur on the second term "b".
///
/// Limitations:
///
/// (1)  The optimization is not applied if the outer ORDER BY contains
///      a COLLATE clause.  The optimization might be applied if the
///      outer ORDER BY uses NULLS FIRST, NULLS LAST, ASC, and/or DESC as
///      long as the subquery ORDER BY does the same.  But if the
///      outer ORDER BY uses COLLATE, even a redundant COLLATE, the
///      optimization is bypassed.
///
/// (2)  The subquery ORDER BY terms must exactly match subquery result
///      columns, including any COLLATE annotations.  This routine relies
///      on iOrderByCol to do matching between order by terms and result
///      columns, and iOrderByCol will not be set if the result column
///      and ORDER BY collations differ.
///
/// (3)  The subquery and outer ORDER BY can be in opposite directions as
///      long as  the subquery is materialized.  If the subquery is
///      implemented as a co-routine, the sort orders must be in the same
///      direction because there is no way to run a co-routine backwards.
fn wherePathMatchSubqueryOB(
    db: &mut Sqlite3,
    pLoop: &WhereLoop,
    iLoop: i32,
    iCur: i32,
    pOrderBy: ExprListId,
    pRevMask: &mut Bitmask,
    pOBSat: &mut Bitmask,
    wctrlFlags: u16,
) -> i32 {
    let mut rev: u8 = 0; // True if iOB and jSub sort in opposite directions
    let mut revIdx: u8; // Sort direction for jSub

    let pSubOB = pLoop.u.btree.pOrderBy;
    debug_assert!(pSubOB.nn());
    let mut iOB: i32 = 0;
    while (MASKBIT(iOB) & *pOBSat) != 0 {
        iOB += 1;
    }
    let nSub = db.el(pSubOB).nExpr();
    let nOB = db.el(pOrderBy).nExpr();
    let mut jSub = 0;
    while jSub < nSub && iOB < nOB {
        let sub = db.el(pSubOB).a[jSub as usize].clone();
        if sub.u.x.iOrderByCol == 0 {
            break;
        }
        let ob = db.el(pOrderBy).a[iOB as usize].clone();
        let pOBExpr = ob.pExpr;
        {
            let x = db.ex(pOBExpr);
            if x.op != TK_COLUMN && x.op != TK_AGG_COLUMN {
                break;
            }
            if x.iTable != iCur {
                break;
            }
            if x.iColumn as i32 != sub.u.x.iOrderByCol as i32 - 1 {
                break;
            }
        }
        if (wctrlFlags & WHERE_GROUPBY) == 0 {
            let sfOB = ob.fg.sortFlags; // sortFlags for iOB
            let sfSub = sub.fg.sortFlags; // sortFlags for jSub
            if (sfSub & KEYINFO_ORDER_BIGNULL) != (sfOB & KEYINFO_ORDER_BIGNULL) {
                break;
            }
            revIdx = sfSub & KEYINFO_ORDER_DESC;
            if jSub > 0 {
                if (rev ^ revIdx) != (sfOB & KEYINFO_ORDER_DESC) {
                    break;
                }
            } else {
                rev = revIdx ^ (sfOB & KEYINFO_ORDER_DESC);
                if rev != 0 {
                    if (pLoop.wsFlags & WHERE_COROUTINE) != 0 {
                        // Cannot run a co-routine in reverse order
                        break;
                    }
                    *pRevMask |= MASKBIT(iLoop);
                }
            }
        }
        *pOBSat |= MASKBIT(iOB);
        jSub += 1;
        iOB += 1;
    }
    (jSub > 0) as i32
}

/// Examine a WherePath (with the addition of the extra WhereLoop of the 6th
/// parameters) to see if it outputs rows in the requested ORDER BY
/// (or GROUP BY) without requiring a separate sort operation.  Return N:
///
///   N>0:   N terms of the ORDER BY clause are satisfied
///   N==0:  No terms of the ORDER BY clause are satisfied
///   N<0:   Unknown yet how many terms of ORDER BY might be satisfied.
///
/// Note that processing for WHERE_GROUPBY and WHERE_DISTINCTBY is not as
/// strict.  With GROUP BY and DISTINCT the only requirement is that
/// equivalent rows appear immediately adjacent to one another.  GROUP BY
/// and DISTINCT do not require rows to appear in any particular order as
/// long as equivalent rows are grouped together.  Thus for GROUP BY and
/// DISTINCT the pOrderBy terms can be matched in any order.  With ORDER BY,
/// the pOrderBy terms must be matched in strict left-to-right order.
///
/// `aLoop` is pPath->aLoop (indices into wi.pLoops), `pLast` the index of
/// the extra WhereLoop.
fn wherePathSatisfiesOrderBy(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pOrderBy: ExprListId,
    aLoop: &[usize],
    wctrlFlags: u16,
    nLoop: u16,
    pLast: usize,
    pRevMask: &mut Bitmask,
) -> i8 {
    let mut revSet: u8; // True if rev is known
    let mut rev: u8; // Composite sort order
    let mut revIdx: u8; // Index sort order
    let mut isOrderDistinct: u8; // All prior WhereLoops are order-distinct
    let mut distinctColumns: u8; // True if the loop has UNIQUE NOT NULL columns
    let mut isMatch: u8; // iColumn matches a term of the ORDER BY clause
    let mut eqOpMask: u16; // Allowed equality operators
    let mut nKeyCol: u16; // Number of key columns in pIndex
    let mut nColumn: u16; // Total number of ordered columns in the index
    let nOrderBy: u16; // Number terms in the ORDER BY clause
    let mut iColumn: i32; // A column number within table iCur
    let mut pLoop: usize = 0; // Current WhereLoop being processed.
    let mut pIndex: Option<IndexRef>; // The index associated with pLoop
    let mut obSat: Bitmask = 0; // Mask of ORDER BY terms satisfied so far
    let obDone: Bitmask; // Mask of all ORDER BY terms
    let mut orderDistinctMask: Bitmask; // Mask of all well-ordered loops
    let mut ready: Bitmask; // Mask of inner loops

    // We say the WhereLoop is "one-row" if it generates no more than one
    // row of output.  A WhereLoop is one-row if all of the following are
    // true:
    //  (a) All index columns match with WHERE_COLUMN_EQ.
    //  (b) The index is unique
    // Any WhereLoop with an WHERE_COLUMN_EQ constraint on the rowid is
    // one-row.  Every one-row WhereLoop will have the WHERE_ONEROW bit set
    // in wsFlags.
    //
    // We say the WhereLoop is "order-distinct" if the set of columns from
    // that WhereLoop that are in the ORDER BY clause are different for every
    // row of the WhereLoop.  Every one-row WhereLoop is automatically
    // order-distinct.   A WhereLoop that has no columns in the ORDER BY
    // clause is not order-distinct. To be order-distinct is not quite the
    // same as being UNIQUE since a UNIQUE column or index can have multiple
    // rows that are NULL and NULL values are equivalent for the purpose of
    // order-distinct.  To be order-distinct, the columns must be UNIQUE and
    // NOT NULL.
    //
    // The rowid for a table is always UNIQUE and NOT NULL so whenever the
    // rowid appears in the ORDER BY clause, the corresponding WhereLoop is
    // automatically order-distinct.

    debug_assert!(pOrderBy.nn());
    if nLoop != 0 && db.OptimizationDisabled(SQLITE_OrderByIdxJoin) {
        return 0;
    }

    nOrderBy = db.el(pOrderBy).nExpr() as u16;
    if nOrderBy as i32 > BMS - 1 {
        return 0; // Cannot optimize overly large ORDER BYs
    }
    isOrderDistinct = 1;
    obDone = MASKBIT(nOrderBy as i32) - 1;
    orderDistinctMask = 0;
    ready = 0;
    eqOpMask = WO_EQ | WO_IS | WO_ISNULL;
    if wctrlFlags & (WHERE_ORDERBY_LIMIT | WHERE_ORDERBY_MAX | WHERE_ORDERBY_MIN) != 0 {
        eqOpMask |= WO_IN;
    }
    let mut iLoop: i32 = 0;
    'oloop: while isOrderDistinct != 0 && obSat < obDone && iLoop <= nLoop as i32 {
        'loopbody: {
            if iLoop > 0 {
                ready |= wi.pLoops[pLoop].maskSelf;
            }
            if iLoop < nLoop as i32 {
                pLoop = aLoop[iLoop as usize];
                if wctrlFlags & WHERE_ORDERBY_LIMIT != 0 {
                    break 'loopbody;
                }
            } else {
                pLoop = pLast;
            }
            let lp = wi.pLoops[pLoop].clone();
            if lp.wsFlags & WHERE_VIRTUALTABLE != 0 {
                if lp.u.vtab.isOrdered != 0 && wi.pOrderBy == pOrderBy {
                    obSat = obDone;
                } else {
                    // No further ORDER BY terms may be matched. So this call
                    // should return >=0, not -1. Clear isOrderDistinct to
                    // ensure it does so.
                    isOrderDistinct = 0;
                }
                break 'oloop;
            }
            let iCur = db.src(wi.pTabList).a[lp.iTab as usize].iCursor;

            // Mark off any ORDER BY term X that is a column in the table of
            // the current loop for which there is term in the WHERE
            // clause of the form X IS NULL or X=? that reference only outer
            // loops.
            for i in 0..nOrderBy as i32 {
                if MASKBIT(i) & obSat != 0 {
                    continue;
                }
                let e = db.el(pOrderBy).a[i as usize].pExpr;
                let pOBExpr = sqlite3ExprSkipCollateAndLikely(db, e);
                if pOBExpr.is_null() {
                    continue;
                }
                let (op, iTable, iCol) = {
                    let x = db.ex(pOBExpr);
                    (x.op, x.iTable, x.iColumn)
                };
                if op != TK_COLUMN && op != TK_AGG_COLUMN {
                    continue;
                }
                if iTable != iCur {
                    continue;
                }
                let Some(pTerm) = sqlite3WhereFindTerm(
                    db,
                    wi,
                    SWC,
                    iCur,
                    iCol as i32,
                    !ready,
                    eqOpMask as u32,
                    None,
                ) else {
                    continue;
                };
                let t = *wi.t(pTerm);
                if t.eOperator == WO_IN {
                    // IN terms are only valid for sorting in the ORDER BY LIMIT
                    // optimization, and then only if they are actually used
                    // by the query plan
                    debug_assert!(
                        wctrlFlags & (WHERE_ORDERBY_LIMIT | WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)
                            != 0
                    );
                    let mut j = 0;
                    while j < lp.nLTerm as usize && Some(pTerm) != lp.lterm(j) {
                        j += 1;
                    }
                    if j >= lp.nLTerm as usize {
                        continue;
                    }
                }
                if (t.eOperator & (WO_EQ | WO_IS)) != 0 && iCol >= 0 {
                    let pColl1 = sqlite3ExprNNCollSeq(db, e);
                    let pCExpr = t.pExpr;
                    let pColl2 = sqlite3ExprCompareCollSeq(db, pCExpr);
                    debug_assert!(pColl1.is_some());
                    match &pColl2 {
                        None => continue,
                        Some(c2) => {
                            if sqlite3StrICmp(Some(&coll_name(&pColl1)[..]), Some(&c2.zName[..]))
                                != 0
                            {
                                continue;
                            }
                        }
                    }
                    let (cl, cr) = (db.ex(pCExpr).pLeft, db.ex(pCExpr).pRight);
                    let affRight = sqlite3ExprAffinity(db, cr);
                    // Left operand must be a column, hence always has affinity
                    if affRight != 0 && affRight != sqlite3ExprAffinity(db, cl) {
                        continue; // An affinity conversion would be required
                    }
                }
                obSat |= MASKBIT(i);
            }

            if (lp.wsFlags & WHERE_ONEROW) == 0 {
                if lp.wsFlags & WHERE_IPK != 0 {
                    if lp.u.btree.pOrderBy.nn()
                        && db.OptimizationEnabled(SQLITE_OrderBySubq)
                        && wherePathMatchSubqueryOB(
                            db, &lp, iLoop, iCur, pOrderBy, pRevMask, &mut obSat, wctrlFlags,
                        ) != 0
                    {
                        nColumn = 0;
                        isOrderDistinct = 0;
                    } else {
                        nColumn = 1;
                    }
                    pIndex = None;
                    nKeyCol = 0;
                } else {
                    pIndex = lp.u.btree.pIndex.clone();
                    match &pIndex {
                        None => return 0,
                        Some(p) => {
                            if p.borrow().bUnordered != 0 {
                                return 0;
                            }
                            let pb = p.borrow();
                            nKeyCol = pb.nKeyCol;
                            nColumn = pb.nColumn;
                            // All relevant terms of the index must also be
                            // non-NULL in order for isOrderDistinct to be true.
                            // So the isOrderDistinct value computed here might
                            // be a false positive.  Corrections will be made at
                            // tag-20210426-1 below
                            isOrderDistinct =
                                (pb.IsUniqueIndex() && (lp.wsFlags & WHERE_SKIPSCAN) == 0) as u8;
                        }
                    }
                }

                // Loop through all columns of the index and deal with the ones
                // that are not constrained by == or IN.
                rev = 0;
                revSet = 0;
                distinctColumns = 0;
                let mut j: i32 = 0;
                while j < nColumn as i32 {
                    let mut bOnce: u8 = 1; // True to run the ORDER BY search loop

                    if j < lp.u.btree.nEq as i32 && j >= lp.nSkip as i32 {
                        let lt = lp.lterm(j as usize).unwrap();
                        let eOp: u16 = wi.t(lt).eOperator;

                        // Skip over == and IS and ISNULL terms.  (Also skip IN
                        // terms when doing WHERE_ORDERBY_LIMIT processing).
                        // Except, IS and ISNULL terms imply that the index is
                        // not UNIQUE NOT NULL in which case the loop need to be
                        // marked as not order-distinct because it can have
                        // repeated NULL rows.
                        //
                        // If the current term is a column of an ((?,?) IN
                        // (SELECT...)) expression for which the SELECT returns
                        // more than one column, check that it is the only
                        // column used by this loop. Otherwise, if it is one of
                        // two or more, none of the columns can be considered
                        // to match an ORDER BY term.
                        if (eOp & eqOpMask) != 0 {
                            if eOp & (WO_ISNULL | WO_IS) != 0 {
                                isOrderDistinct = 0;
                            }
                            j += 1;
                            continue;
                        } else if eOp & WO_IN != 0 {
                            // ALWAYS() justification: eOp is an equality
                            // operator due to the j<pLoop->u.btree.nEq
                            // constraint above.  Any equality other than WO_IN
                            // is captured by the previous "if".  So this one
                            // always has to be WO_IN.
                            let pX = wi.t(lt).pExpr;
                            for i in (j + 1)..lp.u.btree.nEq as i32 {
                                let li = lp.lterm(i as usize).unwrap();
                                if wi.t(li).pExpr == pX {
                                    debug_assert!(wi.t(li).eOperator & WO_IN != 0);
                                    bOnce = 0;
                                    break;
                                }
                            }
                        }
                    }

                    // Get the column number in the table (iColumn) and sort
                    // order (revIdx) for the j-th column of the index.
                    if let Some(p) = &pIndex {
                        let pb = p.borrow();
                        iColumn = pb.aiColumn[j as usize] as i32;
                        revIdx = pb.aSortOrder[j as usize] & KEYINFO_ORDER_DESC;
                        if iColumn == pb.table().borrow().iPKey as i32 {
                            iColumn = XN_ROWID as i32;
                        }
                    } else {
                        iColumn = XN_ROWID as i32;
                        revIdx = 0;
                    }

                    // An unconstrained column that might be NULL means that
                    // this WhereLoop is not well-ordered.  tag-20210426-1
                    if isOrderDistinct != 0 {
                        if iColumn >= 0
                            && j >= lp.u.btree.nEq as i32
                            && pIndex.as_ref().unwrap().borrow().table().borrow().aCol
                                [iColumn as usize]
                                .notNull
                                == 0
                        {
                            isOrderDistinct = 0;
                        }
                        if iColumn == XN_EXPR as i32 {
                            isOrderDistinct = 0;
                        }
                    }

                    // Find the ORDER BY term that corresponds to the j-th
                    // column of the index and mark that ORDER BY term having
                    // been satisfied.
                    isMatch = 0;
                    let mut i: i32 = 0;
                    while bOnce != 0 && i < nOrderBy as i32 {
                        if MASKBIT(i) & obSat != 0 {
                            i += 1;
                            continue;
                        }
                        let e = db.el(pOrderBy).a[i as usize].pExpr;
                        let pOBExpr = sqlite3ExprSkipCollateAndLikely(db, e);
                        if pOBExpr.is_null() {
                            i += 1;
                            continue;
                        }
                        if (wctrlFlags & (WHERE_GROUPBY | WHERE_DISTINCTBY)) == 0 {
                            bOnce = 0;
                        }
                        if iColumn >= XN_ROWID as i32 {
                            let x = db.ex(pOBExpr);
                            if (x.op != TK_COLUMN && x.op != TK_AGG_COLUMN)
                                || x.iTable != iCur
                                || x.iColumn as i32 != iColumn
                            {
                                i += 1;
                                continue;
                            }
                        } else {
                            let pIxExpr = idx_col_expr(db, wi, pIndex.as_ref().unwrap(), j);
                            if sqlite3ExprCompareSkip(db, pOBExpr, pIxExpr, iCur) != 0 {
                                i += 1;
                                continue;
                            }
                        }
                        if iColumn != XN_ROWID as i32 {
                            let pColl = sqlite3ExprNNCollSeq(db, e);
                            let zc = pIndex.as_ref().unwrap().borrow().azColl[j as usize].clone();
                            if sqlite3StrICmp(Some(&coll_name(&pColl)[..]), Some(&zc[..])) != 0 {
                                i += 1;
                                continue;
                            }
                        }
                        if wctrlFlags & WHERE_DISTINCTBY != 0 {
                            wi.pLoops[pLoop].u.btree.nDistinctCol = (j + 1) as u16;
                        }
                        isMatch = 1;
                        break;
                    }
                    let sf = if (i as u16) < nOrderBy {
                        db.el(pOrderBy).a[i as usize].fg.sortFlags
                    } else {
                        0
                    };
                    if isMatch != 0 && (wctrlFlags & WHERE_GROUPBY) == 0 {
                        // Make sure the sort order is compatible in an ORDER BY
                        // clause.  Sort order is irrelevant for a GROUP BY
                        // clause.
                        if revSet != 0 {
                            if (rev ^ revIdx) != (sf & KEYINFO_ORDER_DESC) {
                                isMatch = 0;
                            }
                        } else {
                            rev = revIdx ^ (sf & KEYINFO_ORDER_DESC);
                            if rev != 0 {
                                *pRevMask |= MASKBIT(iLoop);
                            }
                            revSet = 1;
                        }
                    }
                    if isMatch != 0 && (sf & KEYINFO_ORDER_BIGNULL) != 0 {
                        if j == lp.u.btree.nEq as i32 {
                            wi.pLoops[pLoop].wsFlags |= WHERE_BIGNULL_SORT;
                        } else {
                            isMatch = 0;
                        }
                    }
                    if isMatch != 0 {
                        if iColumn == XN_ROWID as i32 {
                            distinctColumns = 1;
                        }
                        obSat |= MASKBIT(i);
                    } else {
                        // No match found
                        if j == 0 || j < nKeyCol as i32 {
                            isOrderDistinct = 0;
                        }
                        break;
                    }
                    j += 1;
                } // end Loop over all index columns
                if distinctColumns != 0 {
                    isOrderDistinct = 1;
                }
            } // end-if not one-row

            // Mark off any other ORDER BY terms that reference pLoop
            if isOrderDistinct != 0 {
                orderDistinctMask |= wi.pLoops[pLoop].maskSelf;
                for i in 0..nOrderBy as i32 {
                    if MASKBIT(i) & obSat != 0 {
                        continue;
                    }
                    let p = db.el(pOrderBy).a[i as usize].pExpr;
                    let mTerm = sqlite3WhereExprUsage(db, &mut wi.sMaskSet, p);
                    if mTerm == 0 && sqlite3ExprIsConstant(db, false, p) == 0 {
                        continue;
                    }
                    if (mTerm & !orderDistinctMask) == 0 {
                        obSat |= MASKBIT(i);
                    }
                }
            }
        }
        iLoop += 1;
    } // End the loop over all WhereLoops from outer-most down to inner-most
    if obSat == obDone {
        return nOrderBy as i8;
    }
    if isOrderDistinct == 0 {
        let mut i = nOrderBy as i32 - 1;
        while i > 0 {
            let m: Bitmask = if i < BMS { MASKBIT(i) - 1 } else { 0 };
            if (obSat & m) == m {
                return i as i8;
            }
            i -= 1;
        }
        return 0;
    }
    -1
}

/// If the WHERE_GROUPBY flag is set in the mask passed to
/// sqlite3WhereBegin(), the planner assumes that the specified pOrderBy
/// list is actually a GROUP BY clause - and so any order that groups rows
/// as required satisfies the request.
///
/// Normally, in this case it is not possible for the caller to determine
/// whether or not the rows are really being delivered in sorted order, or
/// just in some other order that provides the required grouping. However,
/// if the WHERE_SORTBYGROUP flag is also passed to sqlite3WhereBegin(),
/// then this function may be called on the returned WhereInfo object. It
/// returns true if the rows really will be sorted in the specified order,
/// or false otherwise.
///
/// For example, assuming:
///
///   CREATE INDEX i1 ON t1(x, Y);
///
/// then
///
///   SELECT * FROM t1 GROUP BY x,y ORDER BY x,y;   -- IsSorted()==1
///   SELECT * FROM t1 GROUP BY y,x ORDER BY y,x;   -- IsSorted()==0
///
/// C: `int sqlite3WhereIsSorted(WhereInfo *pWInfo)`
pub fn sqlite3WhereIsSorted(pWInfo: &mut crate::where_::WhereInfo) -> i32 {
    debug_assert!(pWInfo.wctrlFlags & (WHERE_GROUPBY | WHERE_DISTINCTBY) != 0);
    debug_assert!(pWInfo.wctrlFlags & WHERE_SORTBYGROUP != 0);
    pWInfo.sorted as i32
}

/// Return the cost of sorting nRow rows, assuming that the keys have
/// nOrderby columns and that the first nSorted columns are already in
/// order.
fn whereSortingCost(
    db: &mut Sqlite3,
    wi: &WhereInfo,
    nRow: LogEst,
    nOrderBy: i32,
    nSorted: i32,
) -> LogEst {
    // Estimated cost of a full external sort, where N is
    // the number of rows to sort is:
    //
    //   cost = (K * N * log(N)).
    //
    // Or, if the order-by clause has X terms but only the last Y
    // terms are out of order, then block-sorting will reduce the
    // sorting cost to:
    //
    //   cost = (K * N * log(N)) * (Y/X)
    //
    // The constant K is at least 2.0 but will be larger if there are a
    // large number of columns to be sorted, as the sorting time is
    // proportional to the amount of content to be sorted.  The algorithm
    // does not currently distinguish between fat columns (BLOBs and TEXTs)
    // and skinny columns (INTs).  It just uses the number of columns as
    // an approximation for the row width.
    //
    // And extra factor of 2.0 or 3.0 is added to the sorting cost if the
    // sort is built using OP_IdxInsert and OP_Sort rather than with
    // OP_SorterInsert.
    let mut nRow = nRow as i32;
    debug_assert!(wi.pSelect.nn());
    let el = db.sel(wi.pSelect).pEList;
    debug_assert!(el.nn());
    // TUNING: sorting cost proportional to the number of output columns:
    let nCol = sqlite3LogEst(((db.el(el).nExpr() + 59) / 30) as u64) as i32;
    let mut rSortCost: i32 = nRow + nCol;
    if nSorted > 0 {
        // Scale the result by (Y/X)
        rSortCost += sqlite3LogEst(((nOrderBy - nSorted) * 100 / nOrderBy) as u64) as i32 - 66;
    }

    // Multiple by log(M) where M is the number of output rows.
    // Use the LIMIT for M if it is smaller.  Or if this sort is for
    // a DISTINCT operator, M will be the number of distinct output
    // rows, so fudge it downwards a bit.
    if (wi.wctrlFlags & WHERE_USE_LIMIT) != 0 {
        rSortCost += 10; // TUNING: Extra 2.0x if using LIMIT
        if nSorted != 0 {
            rSortCost += 6; // TUNING: Extra 1.5x if also using partial sort
        }
        if (wi.iLimit as i32) < nRow {
            nRow = wi.iLimit as i32;
        }
    } else if (wi.wctrlFlags & WHERE_WANT_DISTINCT) != 0 {
        // TUNING: In the sort for a DISTINCT operator, assume that the
        // DISTINCT reduces the number of output rows by a factor of 2
        if nRow > 10 {
            nRow -= 10;
        }
    }
    rSortCost += estLog(nRow as LogEst) as i32;
    rSortCost as LogEst
}

/// Compute the maximum number of paths in the solver algorithm, for
/// queries that have three or more terms in the FROM clause.  Queries with
/// two or fewer FROM clause terms are handled by the caller.
///
/// Query planning is NP-hard.  We must limit the number of paths at
/// each step of the solver search algorithm to avoid exponential behavior.
///
/// The value returned is a tuning parameter.  Currently the value is:
///
///     18    for star queries
///     12    otherwise
///
/// For the purposes of this heuristic, a star-query is defined as a query
/// with a central "fact" table that is joined against multiple
/// "dimension" tables, subject to the following constraints:
///
///   (aa)  Only a five-way or larger join is considered for this
///         optimization.  If there are fewer than four terms in the FROM
///         clause, this heuristic does not apply.
///
///   (bb)  The join between the fact table and the dimension tables must
///         be an INNER join.  CROSS and OUTER JOINs do not qualify.
///
///   (cc)  A table must have 3 or more dimension tables in order to be
///         considered a fact table. (Was 4 prior to 2026-02-10.)
///
///   (dd)  A table that is a self-join cannot be a dimension table.
///         Dimension tables are joined against fact tables.
///
/// SIDE EFFECT:  (and really the whole point of this subroutine)
///
/// If pWInfo describes a star-query, then the cost for SCANs of dimension
/// WhereLoops is increased to be slightly larger than the cost of a SCAN
/// in the fact table.  Only SCAN costs are increased.  SEARCH costs are
/// unchanged. This heuristic helps keep fact tables in outer loops. Without
/// this heuristic, paths with fact tables in outer loops tend to get pruned
/// by the mxChoice limit on the number of paths, resulting in poor query
/// plans.
fn computeMxChoice(db: &mut Sqlite3, wi: &mut WhereInfo) -> i32 {
    let nLoop = wi.nLevel as i32; // Number of terms in the join

    if nLoop >= 4 // Constraint (aa)
        && wi.bStarDone == 0
        && db.OptimizationEnabled(SQLITE_StarQuery)
    {
        let mut mSelfJoin: Bitmask = 0; // Tables that cannot be dimension tables
        let mut pStart: usize = 0; // Where to start searching for dimension-tables

        wi.bStarDone = 1; // Only do this computation once

        // Look for fact tables with three or more dimensions where the
        // dimension tables are not separately from the fact tables by an
        // outer or cross join.  Adjust cost weights if found.
        debug_assert!(wi.bStarUsed == 0);
        let pTabList = wi.pTabList;
        let nL = wi.pLoops.len();
        let mut m: Bitmask = 1;
        'fact: for iFromIdx in 0..nLoop {
            let mut nDep = 0; // Number of dimension tables
            let mut mSeen: Bitmask = 0; // Mask of dimension tables
            let pFactJt = db.src(pTabList).a[iFromIdx as usize].fg.jointype;
            let pFactTab = db.src(pTabList).a[iFromIdx as usize].pSTab.clone();

            'cont: {
                if (pFactJt & (JT_OUTER | JT_CROSS)) != 0 {
                    // If the candidate fact-table is the right table of an
                    // outer join restrict the search for dimension-tables to
                    // be tables to the right of the fact-table.  Constraint
                    // (bb)
                    if iFromIdx + 3 > nLoop {
                        break 'fact; // ^-- Impossible to reach nDep>=2 - Constraint (cc)
                    }
                    while pStart < nL && (wi.pLoops[pStart].iTab as i32) <= iFromIdx {
                        pStart += 1;
                    }
                }
                for pWLoop in pStart..nL {
                    let l = &wi.pLoops[pWLoop];
                    let jt = db.src(pTabList).a[l.iTab as usize].fg.jointype;
                    if (jt & (JT_OUTER | JT_CROSS)) != 0 {
                        break; // Constraint (bb)
                    }
                    if (l.prereq & m) != 0 // pWInfo depends on iFromIdx
                        && (l.maskSelf & mSeen) == 0 // pWInfo not already a dependency
                        && (l.maskSelf & mSelfJoin) == 0
                    // Not a self-join
                    {
                        let dimTab = &db.src(pTabList).a[l.iTab as usize].pSTab;
                        let same = match (dimTab, &pFactTab) {
                            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                            (None, None) => true,
                            _ => false,
                        };
                        if same {
                            mSelfJoin |= m;
                        } else {
                            nDep += 1;
                            mSeen |= l.maskSelf;
                        }
                    }
                }
                if nDep <= 2 {
                    break 'cont; // Constraint (cc)
                }

                // If we reach this point, it means that pFactTab is a fact
                // table with four or more dimensions connected by inner joins.
                // Proceed to make cost adjustments.
                wi.bStarUsed = 1;

                // Compute the maximum cost of any WhereLoop for the
                // fact table plus one epsilon
                let mut mxRun: i32 = LOGEST_MIN;
                for pWLoop in pStart..nL {
                    let l = &wi.pLoops[pWLoop];
                    if (l.iTab as i32) < iFromIdx {
                        continue;
                    }
                    if (l.iTab as i32) > iFromIdx {
                        break;
                    }
                    if l.rRun as i32 > mxRun {
                        mxRun = l.rRun as i32;
                    }
                }
                if mxRun < LOGEST_MAX {
                    mxRun += 1;
                }

                // Increase the cost of table scans for dimension tables to be
                // slightly more than the maximum cost of the fact table
                for pWLoop in pStart..nL {
                    let l = &mut wi.pLoops[pWLoop];
                    if (l.maskSelf & mSeen) == 0 {
                        continue;
                    }
                    if l.nLTerm != 0 {
                        continue;
                    }
                    if (l.rRun as i32) < mxRun {
                        l.rRun = mxRun as LogEst;
                    }
                }
            }
            m <<= 1;
        }
    }
    if wi.bStarUsed != 0 { 18 } else { 12 }
}

/// Two WhereLoop objects, pCandidate and pBaseline, are known to have the
/// same cost.  Look deep into each to see if pCandidate is even slightly
/// better than pBaseline.  Return false if it is, if pCandidate is is
/// preferred.  Return true if pBaseline is preferred or if we cannot tell
/// the difference.
///
///    Result       Meaning
///    --------     ----------------------------------------------------------
///    true         We cannot tell the difference in pCandidate and pBaseline
///    false        pCandidate seems like a better choice than pBaseline
fn whereLoopIsNoBetter(pCandidate: &WhereLoop, pBaseline: &WhereLoop) -> i32 {
    if (pCandidate.wsFlags & WHERE_INDEXED) == 0 {
        return 1;
    }
    if (pBaseline.wsFlags & WHERE_INDEXED) == 0 {
        return 1;
    }
    let a = pCandidate
        .u
        .btree
        .pIndex
        .as_ref()
        .map_or(0, |p| p.borrow().szIdxRow);
    let b = pBaseline
        .u
        .btree
        .pIndex
        .as_ref()
        .map_or(0, |p| p.borrow().szIdxRow);
    if a < b {
        return 0;
    }
    1
}

/// Given the list of WhereLoop objects at pWInfo->pLoops, this routine
/// attempts to find the lowest cost path that visits each WhereLoop
/// once.  This path is then loaded into the pWInfo->a[].pWLoop fields.
///
/// Assume that the total number of output rows that will need to be sorted
/// will be nRowEst (in the 10*log2 representation).  Or, ignore sorting
/// costs if nRowEst==0.
///
/// Return SQLITE_OK on success or SQLITE_NOMEM of a memory allocation
/// error occurs.
fn wherePathSolver(db: &mut Sqlite3, wi: &mut WhereInfo, nRowEst: LogEst) -> i32 {
    let mxChoice: i32; // Maximum number of simultaneous paths tracked
    let nLoop: i32; // Number of terms in the join
    let mut mxI: usize = 0; // Index of next entry to replace
    let nOrderBy: i32; // Number of ORDER BY clause terms
    let mut mxCost: LogEst = 0; // Maximum cost of a set of paths
    let mut mxUnsort: LogEst = 0; // Maximum unsorted cost of a set of path
    let mut nTo: usize; // Number of valid entries in aTo[]
    let mut nFrom: usize; // Number of valid entries in aFrom[]

    nLoop = wi.nLevel as i32;
    // TUNING: mxChoice is the maximum number of possible paths to preserve
    // at each step.  Based on the number of loops in the FROM clause:
    //
    //     nLoop      mxChoice
    //     -----      --------
    //       1            1            // the most common case
    //       2            5
    //       3+        12 or 18        // see computeMxChoice()
    if nLoop <= 1 {
        mxChoice = 1;
    } else if nLoop == 2 {
        mxChoice = 5;
    } else if db.ppr().nErr != 0 {
        mxChoice = 1;
    } else {
        mxChoice = computeMxChoice(db, wi);
    }
    debug_assert!(nLoop <= db.src(wi.pTabList).nSrc());

    // If nRowEst is zero and there is an ORDER BY clause, ignore it. In this
    // case the purpose of this call is to estimate the number of rows
    // returned by the overall query. Once this estimate has been obtained,
    // the caller will invoke this function a second time, passing the
    // estimate as the nRowEst parameter.
    if wi.pOrderBy.is_null() || nRowEst == 0 {
        nOrderBy = 0;
    } else {
        nOrderBy = db.el(wi.pOrderBy).nExpr();
    }

    // Allocate and initialize space for aTo, aFrom and aSortCost[]
    let blank = WherePath {
        aLoop: vec![0usize; nLoop as usize],
        ..Default::default()
    };
    let mut aTo: Vec<WherePath> = vec![blank.clone(); mxChoice as usize];
    let mut aFrom: Vec<WherePath> = vec![blank.clone(); mxChoice as usize];
    // If there is an ORDER BY clause and it is not being ignored, set up
    // space for the aSortCost[] array. Each element of the aSortCost array
    // is either zero - meaning it has not yet been initialized - or the
    // cost of sorting nRowEst rows of data where the first X terms of
    // the ORDER BY clause are already in order, where X is the array
    // index.
    let mut aSortCost: Vec<LogEst> = vec![0; nOrderBy as usize];

    // Seed the search with a single WherePath containing zero WhereLoops.
    //
    // TUNING: Do not let the number of iterations go above 28.  If the cost
    // of computing an automatic index is not paid back within the first 28
    // rows, then do not use the automatic index.
    aFrom[0].nRow = db.ppr().nQueryLoop.min(48);
    debug_assert!(48 == sqlite3LogEst(28));
    nFrom = 1;
    debug_assert!(aFrom[0].isOrdered == 0);
    if nOrderBy != 0 {
        // If nLoop is zero, then there are no FROM terms in the query. Since
        // in this case the query may return a maximum of one row, the
        // results are already in the requested order. Set isOrdered to
        // nOrderBy to indicate this. Or, if nLoop is greater than zero, set
        // isOrdered to -1, indicating that the result set may or may not be
        // ordered, depending on the loops added to the current plan.
        aFrom[0].isOrdered = if nLoop > 0 { -1 } else { nOrderBy as i8 };
    }

    // Compute successively longer WherePaths using the previous generation
    // of WherePaths as the basis for the next.  Keep track of the mxChoice
    // best paths at each generation
    let nLoops = wi.pLoops.len();
    for iLoop in 0..nLoop {
        nTo = 0;
        for ii in 0..nFrom {
            for iW in 0..nLoops {
                let (prereq, maskSelf, wsFlags, rRunW, rSetup, nOutW) = {
                    let l = &wi.pLoops[iW];
                    (l.prereq, l.maskSelf, l.wsFlags, l.rRun, l.rSetup, l.nOut)
                };
                let pFrom = &aFrom[ii];
                if (prereq & !pFrom.maskLoop) != 0 {
                    continue;
                }
                if (maskSelf & pFrom.maskLoop) != 0 {
                    continue;
                }
                if (wsFlags & WHERE_AUTO_INDEX) != 0 && pFrom.nRow < 3 {
                    // Do not use an automatic index if the this loop is
                    // expected to run less than 1.25 times.  It is tempting to
                    // also exclude automatic index usage on an outer loop, but
                    // sometimes an automatic index is useful in the outer loop
                    // of a correlated subquery.
                    continue;
                }

                // At this point, pWLoop is a candidate to be the next loop.
                // Compute its cost
                let mut rUnsort: LogEst = (rRunW as i32 + pFrom.nRow as i32) as LogEst;
                if rSetup != 0 {
                    rUnsort = sqlite3LogEstAdd(rSetup, rUnsort);
                }
                rUnsort = sqlite3LogEstAdd(rUnsort, pFrom.rUnsort);
                let nOut: LogEst = (pFrom.nRow as i32 + nOutW as i32) as LogEst;
                let maskNew: Bitmask = pFrom.maskLoop | maskSelf;
                let mut isOrdered: i8 = pFrom.isOrdered;
                let revMask: Bitmask;
                let rCost: LogEst;
                if isOrdered < 0 {
                    let mut rm: Bitmask = 0;
                    let fromLoops = aFrom[ii].aLoop.clone();
                    let ob = wi.pOrderBy;
                    let wf = wi.wctrlFlags;
                    isOrdered = wherePathSatisfiesOrderBy(
                        db,
                        wi,
                        ob,
                        &fromLoops,
                        wf,
                        iLoop as u16,
                        iW,
                        &mut rm,
                    );
                    revMask = rm;
                } else {
                    revMask = aFrom[ii].revLoop;
                }
                if isOrdered >= 0 && (isOrdered as i32) < nOrderBy {
                    if aSortCost[isOrdered as usize] == 0 {
                        aSortCost[isOrdered as usize] =
                            whereSortingCost(db, wi, nRowEst, nOrderBy, isOrdered as i32);
                    }
                    // TUNING:  Add a small extra penalty (3) to sorting as an
                    // extra encouragement to the query planner to select a
                    // plan where the rows emerge in the correct order without
                    // any sorting required.
                    rCost = (sqlite3LogEstAdd(rUnsort, aSortCost[isOrdered as usize]) as i32 + 3)
                        as LogEst;
                } else {
                    rCost = rUnsort;
                    rUnsort -= 2; // TUNING:  Slight bias in favor of no-sort plans
                }

                // Check to see if pWLoop should be added to the set of
                // mxChoice best-so-far paths.
                //
                // First look for an existing path among best-so-far paths
                // that:
                //     (1) covers the same set of loops, and
                //     (2) has a compatible isOrdered value.
                //
                // "Compatible isOrdered value" means either
                //     (A) both have isOrdered==-1, or
                //     (B) both have isOrder>=0, or
                //     (C) ordering does not matter because this is the last
                //         round of the solver.
                //
                // The term "((pTo->isOrdered^isOrdered)&0x80)==0" is
                // equivalent to (pTo->isOrdered==(-1))==(isOrdered==(-1))" for
                // the range of legal values for isOrdered, -1..64.
                let mut jj = 0usize;
                while jj < nTo {
                    let pTo = &aTo[jj];
                    if pTo.maskLoop == maskNew
                        && ((((pTo.isOrdered ^ isOrdered) as u8) & 0x80) == 0 || iLoop == nLoop - 1)
                    {
                        break;
                    }
                    jj += 1;
                }
                if jj >= nTo {
                    // None of the existing best-so-far paths match the
                    // candidate.
                    if nTo >= mxChoice as usize
                        && (rCost > mxCost || (rCost == mxCost && rUnsort >= mxUnsort))
                    {
                        // The current candidate is no better than any of the
                        // mxChoice paths currently in the best-so-far buffer.
                        // So discard this candidate as not viable.
                        continue;
                    }
                    // If we reach this points it means that the new candidate
                    // path needs to be added to the set of best-so-far paths.
                    if nTo < mxChoice as usize {
                        // Increase the size of the aTo set by one
                        jj = nTo;
                        nTo += 1;
                    } else {
                        // New path replaces the prior worst to keep count below
                        // mxChoice
                        jj = mxI;
                    }
                } else {
                    // Control reaches here if best-so-far path pTo=aTo[jj]
                    // covers the same set of loops and has the same isOrdered
                    // setting as the candidate path.  Check to see if the
                    // candidate should replace pTo or if the candidate should
                    // be skipped.
                    //
                    // The conditional is an expanded vector comparison
                    // equivalent to:
                    //   (pTo->rCost,pTo->nRow,pTo->rUnsort) <= (rCost,nOut,rUnsort)
                    let pTo = &aTo[jj];
                    if (pTo.rCost < rCost)
                        || (pTo.rCost == rCost && pTo.nRow < nOut)
                        || (pTo.rCost == rCost && pTo.nRow == nOut && pTo.rUnsort < rUnsort)
                        || (pTo.rCost == rCost
                            && pTo.nRow == nOut
                            && pTo.rUnsort == rUnsort
                            && whereLoopIsNoBetter(
                                &wi.pLoops[iW],
                                &wi.pLoops[pTo.aLoop[iLoop as usize]],
                            ) != 0)
                    {
                        // Discard the candidate path from further consideration
                        continue;
                    }
                    // Control reaches here if the candidate path is better than
                    // the pTo path.  Replace pTo with the candidate.
                }
                // pWLoop is a winner.  Add it to the set of best so far
                {
                    let fromMask = aFrom[ii].maskLoop;
                    let fromLoops: Vec<usize> = aFrom[ii].aLoop[..iLoop as usize].to_vec();
                    let pTo = &mut aTo[jj];
                    pTo.maskLoop = fromMask | maskSelf;
                    pTo.revLoop = revMask;
                    pTo.nRow = nOut;
                    pTo.rCost = rCost;
                    pTo.rUnsort = rUnsort;
                    pTo.isOrdered = isOrdered;
                    pTo.aLoop[..iLoop as usize].copy_from_slice(&fromLoops);
                    pTo.aLoop[iLoop as usize] = iW;
                }
                if nTo >= mxChoice as usize {
                    mxI = 0;
                    mxCost = aTo[0].rCost;
                    mxUnsort = aTo[0].nRow;
                    for jj in 1..mxChoice as usize {
                        let pTo = &aTo[jj];
                        if pTo.rCost > mxCost || (pTo.rCost == mxCost && pTo.rUnsort > mxUnsort) {
                            mxCost = pTo.rCost;
                            mxUnsort = pTo.rUnsort;
                            mxI = jj;
                        }
                    }
                }
            }
        }

        // Swap the roles of aFrom and aTo for the next generation
        std::mem::swap(&mut aFrom, &mut aTo);
        nFrom = nTo;
    }

    if nFrom == 0 {
        errmsg!(db, "no query solution");
        return SQLITE_ERROR;
    }

    // Only one path is available, which is the best path
    debug_assert!(nFrom == 1);
    let pFrom = aFrom[0].clone();

    debug_assert!(wi.nLevel as i32 == nLoop);
    // Load the lowest cost path into pWInfo
    for iLoop in 0..nLoop as usize {
        let iW = pFrom.aLoop[iLoop];
        let iTab = wi.pLoops[iW].iTab;
        let iCursor = db.src(wi.pTabList).a[iTab as usize].iCursor;
        let pLevel = &mut wi.a[iLoop];
        pLevel.pWLoop = Some(iW);
        pLevel.iFrom = iTab;
        pLevel.iTabCur = iCursor;
    }
    if (wi.wctrlFlags & WHERE_WANT_DISTINCT) != 0
        && (wi.wctrlFlags & WHERE_DISTINCTBY) == 0
        && wi.eDistinct == WHERE_DISTINCT_NOOP
        && nRowEst != 0
    {
        let mut notUsed: Bitmask = 0;
        let rs = wi.pResultSet;
        let last = pFrom.aLoop[nLoop as usize - 1];
        let rc = wherePathSatisfiesOrderBy(
            db,
            wi,
            rs,
            &pFrom.aLoop,
            WHERE_DISTINCTBY,
            (nLoop - 1) as u16,
            last,
            &mut notUsed,
        );
        if rc as i32 == db.el(rs).nExpr() {
            wi.eDistinct = WHERE_DISTINCT_ORDERED;
        }
    }
    wi.bOrderedInnerLoop = 0;
    if wi.pOrderBy.nn() {
        wi.nOBSat = pFrom.isOrdered;
        let nOB = db.el(wi.pOrderBy).nExpr();
        if wi.wctrlFlags & WHERE_DISTINCTBY != 0 {
            if pFrom.isOrdered as i32 == nOB {
                wi.eDistinct = WHERE_DISTINCT_ORDERED;
            }
            // vvv--- See check-in [12ad822d9b827777] on 2023-03-16 ---vvv
        } else {
            wi.revMask = pFrom.revLoop;
            if wi.nOBSat <= 0 {
                wi.nOBSat = 0;
                if nLoop > 0 {
                    let wsFlags = wi.pLoops[pFrom.aLoop[nLoop as usize - 1]].wsFlags;
                    if (wsFlags & WHERE_ONEROW) == 0
                        && (wsFlags & (WHERE_IPK | WHERE_COLUMN_IN))
                            != (WHERE_IPK | WHERE_COLUMN_IN)
                    {
                        let mut m: Bitmask = 0;
                        let ob = wi.pOrderBy;
                        let last = pFrom.aLoop[nLoop as usize - 1];
                        let rc = wherePathSatisfiesOrderBy(
                            db,
                            wi,
                            ob,
                            &pFrom.aLoop,
                            WHERE_ORDERBY_LIMIT,
                            (nLoop - 1) as u16,
                            last,
                            &mut m,
                        );
                        if rc as i32 == nOB {
                            wi.bOrderedInnerLoop = 1;
                            wi.revMask = m;
                        }
                    }
                }
            } else if nLoop != 0
                && wi.nOBSat == 1
                && (wi.wctrlFlags & (WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)) != 0
            {
                wi.bOrderedInnerLoop = 1;
            }
        }
        if (wi.wctrlFlags & WHERE_SORTBYGROUP) != 0 && wi.nOBSat as i32 == nOB && nLoop > 0 {
            let mut revMask: Bitmask = 0;
            let ob = wi.pOrderBy;
            let last = pFrom.aLoop[nLoop as usize - 1];
            let nOrder = wherePathSatisfiesOrderBy(
                db,
                wi,
                ob,
                &pFrom.aLoop,
                0,
                (nLoop - 1) as u16,
                last,
                &mut revMask,
            );
            debug_assert!(wi.sorted == 0);
            if nOrder as i32 == nOB {
                wi.sorted = 1;
                wi.revMask = revMask;
            }
        }
    }

    wi.nRowOut = pFrom.nRow;

    SQLITE_OK
}

/// This routine implements a heuristic designed to improve query planning.
/// This routine is called in between the first and second call to
/// wherePathSolver().  Hence the name "Interstage" "Heuristic".
///
/// The first call to wherePathSolver() (hereafter just "solver()") computes
/// the best path without regard to the order of the outputs.  The second
/// call to the solver() builds upon the first call to try to find an
/// alternative path that satisfies the ORDER BY clause.
///
/// This routine looks at the results of the first solver() run, and for
/// every FROM clause term in the resulting query plan that uses an
/// equality constraint against an index, disable other WhereLoops for that
/// same FROM clause term that would try to do a full-table scan.  This
/// prevents an index search from being converted into a full-table scan in
/// order to satisfy an ORDER BY clause, since even though we might get
/// slightly better performance using the full-scan without sorting if the
/// output size estimates are very precise, we might also get severe
/// performance degradation using the full-scan if the output size estimate
/// is too large.  It is better to err on the side of caution.
///
/// Except, if the first solver() call generated a full-table scan in an
/// outer loop then stop this analysis at the first full-scan, since the
/// second solver() run might try to swap that full-scan for another in
/// order to get the output into the correct order.
fn whereInterstageHeuristic(wi: &mut WhereInfo) {
    for i in 0..wi.nLevel as usize {
        let Some(ip) = wi.a[i].pWLoop else { break };
        let (wsFlags, iTab) = {
            let p = &wi.pLoops[ip];
            (p.wsFlags, p.iTab)
        };
        if (wsFlags & WHERE_VIRTUALTABLE) != 0 {
            // Treat a vtab scan as similar to a full-table scan
            break;
        }
        if (wsFlags & (WHERE_COLUMN_EQ | WHERE_COLUMN_NULL | WHERE_COLUMN_IN)) != 0 {
            for pLoop in wi.pLoops.iter_mut() {
                if pLoop.iTab != iTab {
                    continue;
                }
                if (pLoop.wsFlags & (WHERE_CONSTRAINT | WHERE_AUTO_INDEX)) != 0 {
                    // Auto-index and index-constrained loops allowed to remain
                    continue;
                }
                pLoop.prereq = ALLBITS; // Prevent 2nd solver() from using this one
            }
        } else {
            break;
        }
    }
}

/// Most queries use only a single table (they are not joins) and have
/// simple == constraints against indexed fields.  This routine attempts
/// to plan those simple cases using much less ceremony than the
/// general-purpose query planner, and thereby yield faster sqlite3_prepare()
/// times for the common case.
///
/// Return non-zero on success, if this query can be handled by this
/// no-frills query planner.  Return zero if this query needs the
/// general-purpose query planner.
fn whereShortCut(db: &mut Sqlite3, wi: &mut WhereInfo, pBuilder: &mut WhereLoopBuilder) -> i32 {
    let mut scan = WhereScan::default();

    if wi.wctrlFlags & WHERE_OR_SUBCLAUSE != 0 {
        return 0;
    }
    debug_assert!(db.src(wi.pTabList).nSrc() >= 1);
    let pItem = SrcItemId(wi.pTabList, 0);
    let pTab = item_tab(db, pItem);
    if pTab.borrow().IsVirtual() {
        return 0;
    }
    if db.item(pItem).fg.isIndexedBy != 0 || db.item(pItem).fg.notIndexed != 0 {
        return 0;
    }
    let iCur = db.item(pItem).iCursor;
    let pWC = SWC;
    pBuilder.pNew.wsFlags = 0;
    pBuilder.pNew.nSkip = 0;
    let mut pTerm = whereScanInit(
        db,
        wi,
        &mut scan,
        pWC,
        iCur,
        -1,
        (WO_EQ | WO_IS) as u32,
        None,
    );
    while let Some(t) = pTerm {
        if wi.t(t).prereqRight == 0 {
            break;
        }
        pTerm = whereScanNext(db, wi, &mut scan);
    }
    if let Some(t) = pTerm {
        let pLoop = &mut pBuilder.pNew;
        pLoop.wsFlags = WHERE_COLUMN_EQ | WHERE_IPK | WHERE_ONEROW;
        pLoop.set_lterm(0, Some(t));
        pLoop.nLTerm = 1;
        pLoop.u.btree.nEq = 1;
        // TUNING: Cost of a rowid lookup is 10
        pLoop.rRun = 33; // 33==sqlite3LogEst(10)
    } else {
        let idxs: Vec<IndexRef> = pTab.borrow().pIndex.clone();
        for pIdx in idxs.iter() {
            let (isUnique, hasPart, nKeyCol, uniqNotNull, isCovering, colNotIdxed) = {
                let p = pIdx.borrow();
                (
                    p.IsUniqueIndex(),
                    p.pPartIdxWhere.is_some(),
                    p.nKeyCol as i32,
                    p.uniqNotNull,
                    p.isCovering,
                    p.colNotIdxed,
                )
            };
            if !isUnique || hasPart || nKeyCol > N_LTERM_SPACE as i32 {
                continue;
            }
            let opMask: u32 = if uniqNotNull != 0 {
                (WO_EQ | WO_IS) as u32
            } else {
                WO_EQ as u32
            };
            let mut j = 0;
            while j < nKeyCol {
                let mut pt = whereScanInit(db, wi, &mut scan, pWC, iCur, j, opMask, Some(pIdx));
                while let Some(t) = pt {
                    if wi.t(t).prereqRight == 0 {
                        break;
                    }
                    pt = whereScanNext(db, wi, &mut scan);
                }
                let Some(t) = pt else { break };
                pBuilder.pNew.set_lterm(j as usize, Some(t));
                j += 1;
            }
            if j != nKeyCol {
                continue;
            }
            let colUsed = db.item(pItem).colUsed;
            let pLoop = &mut pBuilder.pNew;
            pLoop.wsFlags = WHERE_COLUMN_EQ | WHERE_ONEROW | WHERE_INDEXED;
            if isCovering != 0 || (colUsed & colNotIdxed) == 0 {
                pLoop.wsFlags |= WHERE_IDX_ONLY;
            }
            pLoop.nLTerm = j as u16;
            pLoop.u.btree.nEq = j as u16;
            pLoop.u.btree.pIndex = Some(pIdx.clone());
            // TUNING: Cost of a unique index lookup is 15
            pLoop.rRun = 39; // 39==sqlite3LogEst(15)
            break;
        }
    }
    if pBuilder.pNew.wsFlags != 0 {
        pBuilder.pNew.nOut = 1;
        debug_assert!(wi.sMaskSet.n == 1 && iCur == wi.sMaskSet.ix[0]);
        pBuilder.pNew.maskSelf = 1; // sqlite3WhereGetMask(&pWInfo->sMaskSet, iCur);
        wi.a[0].iTabCur = iCur;
        wi.nRowOut = 1;
        if wi.pOrderBy.nn() {
            wi.nOBSat = db.el(wi.pOrderBy).nExpr() as i8;
        }
        if wi.wctrlFlags & WHERE_WANT_DISTINCT != 0 {
            wi.eDistinct = WHERE_DISTINCT_UNIQUE;
        }
        if scan.iEquiv > 1 {
            pBuilder.pNew.wsFlags |= WHERE_TRANSCONS;
        }
        // pWInfo->a[0].pWLoop = pLoop: the template becomes the only loop
        // of the solution.
        let l = std::mem::take(&mut pBuilder.pNew);
        wi.pLoops.push(l);
        wi.a[0].pWLoop = Some(wi.pLoops.len() - 1);
        return 1;
    }
    0
}

/// Helper function for exprIsDeterministic().
fn exprNodeIsDeterministic(pWalker: &mut Walker, pExpr: ExprId) -> i32 {
    let e = pWalker.db.ex(pExpr);
    if e.op == TK_FUNCTION && !e.HasProperty(EP_ConstFunc) {
        pWalker.eCode = 0;
        return WRC_Abort;
    }
    WRC_Continue
}

/// Return true if the expression contains no non-deterministic SQL
/// functions. Do not consider non-deterministic SQL functions that are
/// part of sub-select statements.
fn exprIsDeterministic(db: &mut Sqlite3, p: ExprId) -> i32 {
    let mut w = Walker::new(db);
    w.eCode = 1;
    w.xExprCallback = Some(exprNodeIsDeterministic);
    w.xSelectCallback = Some(sqlite3SelectWalkFail);
    crate::walker::sqlite3WalkExpr(&mut w, p);
    w.eCode as i32
}

/// Attempt to omit tables from a join that do not affect the result.
/// For a table to not affect the result, the following must be true:
///
///   1) The query must not be an aggregate.
///   2) The table must be the RHS of a LEFT JOIN.
///   3) Either the query must be DISTINCT, or else the ON or USING clause
///      must contain a constraint that limits the scan of the table to
///      at most a single row.
///   4) The table must not be referenced by any part of the query apart
///      from its own USING or ON clause.
///   5) The table must not have an inner-join ON or USING clause if there
///      is a RIGHT JOIN anywhere in the query.  Otherwise the ON/USING
///      clause might move from the right side to the left side of the
///      RIGHT JOIN.  Note: Due to (2), this condition can only arise if the
///      table is the right-most table of a subquery that was flattened into
///      the main query and that subquery was the right-hand operand of an
///      inner join that held an ON or USING clause.
///   6) The ORDER BY clause has 63 or fewer terms
///   7) The omit-noop-join optimization is enabled.
///
/// Items (1), (6), and (7) are checked by the caller.
///
/// For example, given:
///
///     CREATE TABLE t1(ipk INTEGER PRIMARY KEY, v1);
///     CREATE TABLE t2(ipk INTEGER PRIMARY KEY, v2);
///     CREATE TABLE t3(ipk INTEGER PRIMARY KEY, v3);
///
/// then table t2 can be omitted from the following:
///
///     SELECT v1, v3 FROM t1
///       LEFT JOIN t2 ON (t1.ipk=t2.ipk)
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
///
/// or from:
///
///     SELECT DISTINCT v1, v3 FROM t1
///       LEFT JOIN t2
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
fn whereOmitNoopJoin(db: &mut Sqlite3, wi: &mut WhereInfo, notReady: Bitmask) -> Bitmask {
    let mut notReady = notReady;

    // Preconditions checked by the caller
    debug_assert!(wi.nLevel >= 2);
    debug_assert!(db.OptimizationEnabled(SQLITE_OmitNoopJoin));

    // These two preconditions checked by the caller combine to guarantee
    // condition (1) of the header comment
    debug_assert!(wi.pResultSet.nn());
    debug_assert!(0 == (wi.wctrlFlags & WHERE_AGG_DISTINCT));

    let rs = wi.pResultSet;
    let mut tabUsed = sqlite3WhereExprListUsage(db, &mut wi.sMaskSet, rs);
    if wi.pOrderBy.nn() {
        let ob = wi.pOrderBy;
        tabUsed |= sqlite3WhereExprListUsage(db, &mut wi.sMaskSet, ob);
    }
    let hasRightJoin = (db.src(wi.pTabList).a[0].fg.jointype & JT_LTORJ) != 0;
    let mut i = wi.nLevel as i32 - 1;
    while i >= 1 {
        'cont: {
            let lp = wi.lp(i as usize).clone();
            let pItem = SrcItemId(wi.pTabList, lp.iTab as u32);
            let (jt, iCursor) = (db.item(pItem).fg.jointype, db.item(pItem).iCursor);
            if (jt & (JT_LEFT | JT_RIGHT)) != JT_LEFT {
                break 'cont;
            }
            if (wi.wctrlFlags & WHERE_WANT_DISTINCT) == 0 && (lp.wsFlags & WHERE_ONEROW) == 0 {
                break 'cont;
            }
            if (tabUsed & lp.maskSelf) != 0 {
                break 'cont;
            }
            let nEnd = wi.wc(SWC).a.len();
            let mut k = 0usize;
            while k < nEnd {
                let t = wi.wc(SWC).a[k];
                let e = db.ex(t.pExpr);
                if (t.prereqAll & lp.maskSelf) != 0 {
                    if !e.HasProperty(EP_OuterON) || e.w.iJoin != iCursor {
                        break;
                    }
                }
                if hasRightJoin && e.HasProperty(EP_InnerON) && e.w.iJoin == iCursor {
                    break; // restriction (5)
                }
                k += 1;
            }
            if k < nEnd {
                break 'cont;
            }
            let m1: Bitmask = MASKBIT(i) - 1;
            wi.revMask = (m1 & wi.revMask) | ((wi.revMask >> 1) & !m1);
            notReady &= !lp.maskSelf;
            for k in 0..nEnd {
                let t = &mut wi.wcm(SWC).a[k];
                if (t.prereqAll & lp.maskSelf) != 0 {
                    t.wtFlags |= TERM_CODED;
                    t.prereqAll = 0;
                }
            }
            wi.a.remove(i as usize);
            wi.nLevel -= 1;
            debug_assert!(wi.nLevel > 0);
        }
        i -= 1;
    }
    notReady
}

/// Check to see if there are any SEARCH loops that might benefit from
/// using a Bloom filter.  Consider a Bloom filter if:
///
///   (1)  The SEARCH happens more than N times where N is the number
///        of rows in the table that is being considered for the Bloom
///        filter.
///   (2)  Some searches are expected to find zero rows.  (This is
///        determined by the WHERE_SELFCULL flag on the term.)
///   (3)  Bloom-filter processing is not disabled.  (Checked by the
///        caller.)
///   (4)  The size of the table being searched is known by ANALYZE.
///
/// This block of code merely checks to see if a Bloom filter would be
/// appropriate, and if so sets the WHERE_BLOOMFILTER flag on the
/// WhereLoop.  The implementation of the Bloom filter comes further
/// down where the code for each WhereLoop is generated.
fn whereCheckIfBloomFilterIsUseful(db: &mut Sqlite3, wi: &mut WhereInfo) {
    let mut nSearch: i32 = 0;

    debug_assert!(wi.nLevel >= 2);
    debug_assert!(db.OptimizationEnabled(SQLITE_BloomFilter));
    for i in 0..wi.nLevel as usize {
        let reqFlags: u32 = WHERE_SELFCULL | WHERE_COLUMN_EQ;
        let iTab = wi.lp(i).iTab;
        let pTab = item_tab(db, SrcItemId(wi.pTabList, iTab as u32));
        if (pTab.borrow().tabFlags & TF_HasStat1) == 0 {
            break;
        }
        pTab.borrow_mut().tabFlags |= TF_MaybeReanalyze;
        let nRowLogEst = pTab.borrow().nRowLogEst as i32;
        let pLoop = wi.lpm(i);
        if i >= 1
            && (pLoop.wsFlags & reqFlags) == reqFlags
            && crate::whereexpr::sqlite3WhereLoopBloomable(pLoop) != 0
            && nSearch > nRowLogEst
        {
            pLoop.wsFlags |= WHERE_BLOOMFILTER;
            pLoop.wsFlags &= !WHERE_IDX_ONLY;
        }
        nSearch += pLoop.nOut as i32;
        nSearch = nSearch as LogEst as i32;
    }
}

/// The index pIdx is used by a query and contains one or more expressions.
/// In other words pIdx is an index on an expression.  iIdxCur is the cursor
/// number for the index and iDataCur is the cursor number for the
/// corresponding table.
///
/// This routine adds IndexedExpr entries to the Parse->pIdxEpr field for
/// each of the expressions in the index so that the expression code
/// generator will know to replace occurrences of the indexed expression
/// with references to the corresponding column of the index.
fn whereAddIndexedExpr(
    db: &mut Sqlite3,
    wi: &mut WhereInfo,
    pIdx: &IndexRef,
    iIdxCur: i32,
    pTabItem: SrcItemId,
) {
    debug_assert!(pIdx.borrow().bHasExpr != 0);
    let pTab = pIdx.borrow().table();
    let nColumn = pIdx.borrow().nColumn as i32;
    for i in 0..nColumn {
        let pExpr: ExprId;
        let j = pIdx.borrow().aiColumn[i as usize];
        if j == XN_EXPR {
            pExpr = idx_col_expr(db, wi, pIdx, i);
        } else if j >= 0 && (pTab.borrow().aCol[j as usize].colFlags & COLFLAG_VIRTUAL) != 0 {
            pExpr = crate::build::sqlite3ColumnExpr(db, &pTab, j as usize);
        } else {
            continue;
        }
        if sqlite3ExprIsConstant(db, false, pExpr) != 0 {
            continue;
        }
        let e = sqlite3ExprDup(db, pExpr, 0);
        let (iCursor, jt) = {
            let it = db.item(pTabItem);
            (it.iCursor, it.fg.jointype)
        };
        let mut aff = 0u8;
        if crate::insert::sqlite3IndexAffinityStr(db, pIdx).is_some() {
            aff = pIdx
                .borrow()
                .zColAff
                .as_ref()
                .map_or(0, |z| z.get(i as usize).copied().unwrap_or(0));
        }
        let p = IndexedExpr {
            pExpr: e,
            iDataCur: iCursor,
            iIdxCur,
            iIdxCol: i,
            bMaybeNullRow: ((jt & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0) as u8,
            aff,
        };
        // C prepends to the pIdxEpr linked list; the Vec keeps push order
        // (C list head = last element), as expr.rs scans it from the end.
        db.pp().pIdxEpr.push(p);
    }
}

/// Set the reverse-scan order mask to one for all tables in the query
/// with the exception of MATERIALIZED common table expressions that have
/// their own internal ORDER BY clauses.
///
/// This implements the PRAGMA reverse_unordered_selects=ON setting.
/// (Also SQLITE_DBCONFIG_REVERSE_SCANORDER).
fn whereReverseScanOrder(db: &mut Sqlite3, wi: &mut WhereInfo) {
    let n = db.src(wi.pTabList).nSrc();
    for ii in 0..n {
        let it = &db.src(wi.pTabList).a[ii as usize];
        let cte_mat = it.fg.isCte != 0
            && it.u2.pCteUse.nn()
            && db.arena.cteuse(it.u2.pCteUse).eM10d == M10d_Yes
            && it.fg.isSubquery != 0
            && it
                .u4
                .pSubq
                .as_ref()
                .map_or(false, |q| db.sel(q.pSelect).pOrderBy.nn());
        if !cte_mat {
            wi.revMask |= MASKBIT(ii);
        }
    }
}

/// Generate the beginning of the loop used for WHERE clause processing.
/// The return value is a pointer to an opaque structure that contains
/// information needed to terminate the loop.  Later, the calling routine
/// should invoke sqlite3WhereEnd() with the return value of this function
/// in order to complete the WHERE clause processing.
///
/// If an error occurs, this routine returns NULL.
///
/// The basic idea is to do a nested loop, one loop for each table in
/// the FROM clause of a select.  (INSERT and UPDATE statements are the
/// same as a SELECT with only a single table in the FROM clause.)  For
/// example, if the SQL is this:
///
///       SELECT * FROM t1, t2, t3 WHERE ...;
///
/// Then the code generated is conceptually like the following:
///
///      foreach row1 in t1 do       \    Code generated
///        foreach row2 in t2 do      |-- by sqlite3WhereBegin()
///          foreach row3 in t3 do   /
///            ...
///          end                     \    Code generated
///        end                        |-- by sqlite3WhereEnd()
///      end                         /
///
/// Note that the loops might not be nested in the order in which they
/// appear in the FROM clause if a different order is better able to make
/// use of indices.  Note also that when the IN operator appears in
/// the WHERE clause, it might result in additional nested loops for
/// scanning through all values on the right-hand side of the IN.
///
/// There are Btree cursors associated with each table.  t1 uses cursor
/// number pTabList->a[0].iCursor.  t2 uses the cursor pTabList->a[1].iCursor.
/// And so forth.  This routine generates code to open those VDBE cursors
/// and sqlite3WhereEnd() generates the code to close them.
///
/// The code that sqlite3WhereBegin() generates leaves the cursors named
/// in pTabList pointing at their appropriate entries.  The [...] code
/// can use OP_Column and OP_Rowid opcodes on these cursors to extract
/// data from the various tables of the loop.
///
/// If the WHERE clause is empty, the foreach loops must each scan their
/// entire tables.  Thus a three-way join is an O(N^3) operation.  But if
/// the tables have indices and there are terms in the WHERE clause that
/// refer to those indices, a complete table scan can be avoided and the
/// code will run much faster.  Most of the work of this routine is checking
/// to see if there are indices that can be used to speed up the loop.
///
/// Terms of the WHERE clause are also used to limit which rows actually
/// make it to the "..." in the middle of the loop.  After each "foreach",
/// terms of the WHERE clause that use only terms in that loop and outer
/// loops are evaluated and if false a jump is made around all subsequent
/// inner loops (or around the "..." if the test occurs within the inner-
/// most loop)
///
/// OUTER JOINS
///
/// An outer join of tables t1 and t2 is conceptually coded as follows:
///
///    foreach row1 in t1 do
///      flag = 0
///      foreach row2 in t2 do
///        start:
///          ...
///          flag = 1
///      end
///      if flag==0 then
///        move the row2 cursor to a null row
///        goto start
///      fi
///    end
///
/// ORDER BY CLAUSE PROCESSING
///
/// pOrderBy is a pointer to the ORDER BY clause (or the GROUP BY clause
/// if the WHERE_GROUPBY flag is set in wctrlFlags) of a SELECT statement
/// if there is one.  If there is no ORDER BY clause or if this routine
/// is called from an UPDATE or DELETE statement, then pOrderBy is NULL.
///
/// The iIdxCur parameter is the cursor number of an index.  If
/// WHERE_OR_SUBCLAUSE is set, iIdxCur is the cursor number of an index
/// to use for OR clause processing.  The WHERE clause should use this
/// specific cursor.  If WHERE_ONEPASS_DESIRED is set, then iIdxCur is
/// the first cursor in an array of cursors for all indices.  iIdxCur should
/// be used to compute the appropriate cursor depending on which index is
/// used.
///
/// C: `WhereInfo * sqlite3WhereBegin( Parse *pParse, SrcList *pTabList, Expr *pWhere, ExprList *pOrderBy, ExprList *pResultSet, Select *pSelect, u16 wctrlFlags, int iAuxArg )`
pub fn sqlite3WhereBegin(
    db: &mut Sqlite3,
    pTabList: SrcListId,
    pWhere: ExprId,
    pOrderBy: ExprListId,
    pResultSet: ExprListId,
    pSelect: SelectId,
    wctrlFlags: u16,
    iAuxArg: i32,
) -> Option<Box<crate::where_::WhereInfo>> {
    let mut pOrderBy = pOrderBy;
    let mut wctrlFlags = wctrlFlags;
    let mut nTabList: i32; // Number of elements in pTabList
    let mut notReady: Bitmask; // Cursors that are not yet positioned
    let mut sWLB = WhereLoopBuilder::default(); // The WhereLoop builder
    let mut bFordelete: u16 = 0; // OPFLAG_FORDELETE or zero, as appropriate

    debug_assert!(
        (wctrlFlags & WHERE_ONEPASS_MULTIROW) == 0
            || ((wctrlFlags & WHERE_ONEPASS_DESIRED) != 0
                && (wctrlFlags & WHERE_OR_SUBCLAUSE) == 0)
    );

    // Only one of WHERE_OR_SUBCLAUSE or WHERE_USE_LIMIT
    debug_assert!((wctrlFlags & WHERE_OR_SUBCLAUSE) == 0 || (wctrlFlags & WHERE_USE_LIMIT) == 0);

    // An ORDER/GROUP BY clause of more than 63 terms cannot be optimized
    if pOrderBy.nn() && db.el(pOrderBy).nExpr() >= BMS {
        pOrderBy = ExprListId::NULL;
        wctrlFlags &= !WHERE_WANT_DISTINCT;
        wctrlFlags |= WHERE_KEEP_ALL_JOINS; // Disable omit-noop-join opt
    }

    // The number of tables in the FROM clause is limited by the number of
    // bits in a Bitmask
    if db.src(pTabList).nSrc() > BMS {
        errmsg!(db, "at most %d tables in a join", BMS);
        return None;
    }

    // This function normally generates a nested loop for all tables in
    // pTabList.  But if the WHERE_OR_SUBCLAUSE flag is set, then we should
    // only generate code for the first table in pTabList and assume that
    // any cursors associated with subsequent tables are uninitialized.
    nTabList = if (wctrlFlags & WHERE_OR_SUBCLAUSE) != 0 {
        1
    } else {
        db.src(pTabList).nSrc()
    };

    // Allocate and initialize the WhereInfo structure that will become the
    // return value.
    let mut wi = Box::new(WhereInfo::default());
    wi.pTabList = pTabList;
    wi.pOrderBy = pOrderBy;
    wi.pResultSet = pResultSet;
    wi.aiCurOnePass = [-1, -1];
    wi.nLevel = nTabList as u8;
    let lbl = sqlite3VdbeMakeLabel(db);
    wi.iBreak = lbl;
    wi.iContinue = lbl;
    wi.wctrlFlags = wctrlFlags;
    wi.iLimit = iAuxArg as LogEst;
    wi.savedNQueryLoop = db.ppr().nQueryLoop as i32;
    wi.pSelect = pSelect;
    wi.a = vec![WhereLevel::default(); nTabList as usize];
    debug_assert!(wi.eOnePass as i32 == ONEPASS_OFF); // ONEPASS defaults to OFF
    wi.sMaskSet.n = 0;
    wi.sMaskSet.ix[0] = -99; // Initialize ix[0] to a value that can never be
    // a valid cursor number, to avoid an initial
    // test for pMaskSet->n==0 in sqlite3WhereGetMask()
    sWLB.pWC = SWC;
    whereLoopInit(&mut sWLB.pNew);

    // Split the WHERE clause into separate subexpressions where each
    // subexpression is separated by an AND operator.
    let mut sWC = WhereClause::default();
    sqlite3WhereClauseInit(&mut sWC);
    wi.aWC.push(sWC);
    wi.aOrInfo.clear();
    sqlite3WhereSplit(db, &mut wi, SWC, pWhere, TK_AND);

    'error: {
        // Special case: No FROM clause
        if nTabList == 0 {
            if pOrderBy.nn() {
                wi.nOBSat = db.el(pOrderBy).nExpr() as i8;
            }
            if (wctrlFlags & WHERE_WANT_DISTINCT) != 0 && db.OptimizationEnabled(SQLITE_DistinctOpt)
            {
                wi.eDistinct = WHERE_DISTINCT_UNIQUE;
            }
            if wi.pSelect.nn() && (db.sel(wi.pSelect).selFlags & SF_MultiValue) == 0 {
                sqlite3VdbeExplain(db, 0, b"SCAN CONSTANT ROW", &[]);
            }
        } else {
            // Assign a bit from the bitmask to every term in the FROM clause.
            //
            // The N-th term of the FROM clause is assigned a bitmask of 1<<N.
            //
            // The rule of the previous sentence ensures that if X is the
            // bitmask for a table T, then X-1 is the bitmask for all other
            // tables to the left of T.  Knowing the bitmask for all tables to
            // the left of a left join is important.  Ticket #3015.
            //
            // Note that bitmasks are created for all pTabList->nSrc tables in
            // pTabList, not just the first nTabList tables.  nTabList is
            // normally equal to pTabList->nSrc but might be shortened to 1 if
            // the WHERE_OR_SUBCLAUSE flag is set.
            let nSrc = db.src(pTabList).nSrc();
            let mut ii = 0;
            loop {
                let iCursor = db.src(pTabList).a[ii as usize].iCursor;
                createMask(&mut wi.sMaskSet, iCursor);
                sqlite3WhereTabFuncArgs(db, &mut wi, SrcItemId(pTabList, ii as u32), SWC);
                ii += 1;
                if ii >= nSrc {
                    break;
                }
            }
        }

        // Analyze all of the subexpressions.
        sqlite3WhereExprAnalyze(db, &mut wi, pTabList, SWC);
        if pSelect.nn() && db.sel(pSelect).pLimit.nn() {
            sqlite3WhereAddLimit(db, &mut wi, SWC, pSelect);
        }
        if db.ppr().nErr != 0 {
            break 'error;
        }

        // The False-WHERE-Term-Bypass optimization:
        //
        // If there are WHERE terms that are false, then no rows will be
        // output, so skip over all of the code generated here.
        //
        // Conditions:
        //
        //   (1)  The WHERE term must not refer to any tables in the join.
        //   (2)  The term must not come from an ON clause on the
        //        right-hand side of a LEFT or FULL JOIN.
        //   (3)  The term must not come from an ON clause, or there must be
        //        no RIGHT or FULL OUTER joins in pTabList.
        //   (4)  If the expression contains non-deterministic functions
        //        that are not within a sub-select. This is not required
        //        for correctness but rather to preserves SQLite's legacy
        //        behaviour in the following two cases:
        //
        //          WHERE random()>0;           -- eval random() once per row
        //          WHERE (SELECT random())>0;  -- eval random() just once overall
        //
        // Note that the Where term need not be a constant in order for this
        // optimization to apply, though it does need to be constant relative
        // to the current subquery (condition 1).  The term might include
        // variables from outer queries so that the value of the term changes
        // from one invocation of the current subquery to the next.
        let nBase = wi.wc(SWC).nBase;
        for ii in 0..nBase as usize {
            let pT = wi.wc(SWC).a[ii];
            if pT.wtFlags & TERM_VIRTUAL != 0 {
                continue;
            }
            let pX = pT.pExpr;
            debug_assert!(pX.nn());
            debug_assert!(pT.prereqAll != 0 || !db.ex(pX).HasProperty(EP_OuterON));
            if pT.prereqAll == 0 // Conditions (1) and (2)
                && (nTabList == 0 || exprIsDeterministic(db, pX) != 0) // Condition (4)
                && !(db.ex(pX).HasProperty(EP_InnerON) // Condition (3)
                    && (db.src(pTabList).a[0].fg.jointype & JT_LTORJ) != 0)
            {
                let brk = wi.iBreak;
                sqlite3ExprIfFalse(db, pX, brk, SQLITE_JUMPIFNULL as i32);
                wi.wcm(SWC).a[ii].wtFlags |= TERM_CODED;
            }
        }

        if wctrlFlags & WHERE_WANT_DISTINCT != 0 {
            if db.OptimizationDisabled(SQLITE_DistinctOpt) {
                // Disable the DISTINCT optimization if SQLITE_DistinctOpt is
                // set via sqlite3_test_ctrl(SQLITE_TESTCTRL_OPTIMIZATIONS,...)
                wctrlFlags &= !WHERE_WANT_DISTINCT;
                wi.wctrlFlags &= !WHERE_WANT_DISTINCT;
            } else if isDistinctRedundant(db, &mut wi, pTabList, SWC, pResultSet) != 0 {
                // The DISTINCT marking is pointless.  Ignore it.
                wi.eDistinct = WHERE_DISTINCT_UNIQUE;
            } else if pOrderBy.is_null() {
                // Try to ORDER BY the result set to make distinct processing
                // easier
                wi.wctrlFlags |= WHERE_DISTINCTBY;
                wi.pOrderBy = pResultSet;
            }
        }

        // Construct the WhereLoop objects
        if nTabList != 1 || whereShortCut(db, &mut wi, &mut sWLB) == 0 {
            let rc = whereLoopAddAll(db, &mut wi, &mut sWLB);
            if rc != 0 {
                break 'error;
            }

            wherePathSolver(db, &mut wi, 0);
            if db.mallocFailed != 0 {
                break 'error;
            }
            if wi.pOrderBy.nn() {
                whereInterstageHeuristic(&mut wi);
                let est = if wi.nRowOut < 0 { 1 } else { wi.nRowOut + 1 };
                wherePathSolver(db, &mut wi, est);
                if db.mallocFailed != 0 {
                    break 'error;
                }
            }

            // TUNING:  Assume that a DISTINCT clause on a subquery reduces
            // the output size by a factor of 8 (LogEst -30).  Search for
            // tag-20250414a to see other cases.
            if (wi.wctrlFlags & WHERE_WANT_DISTINCT) != 0 {
                wi.nRowOut -= 30;
            }
        }
        debug_assert!(wi.pTabList.nn());
        if wi.pOrderBy.is_null() && (db.flags & SQLITE_ReverseOrder) != 0 {
            whereReverseScanOrder(db, &mut wi);
        }
        if db.ppr().nErr != 0 {
            break 'error;
        }
        debug_assert!(db.mallocFailed == 0);

        // Attempt to omit tables from a join that do not affect the result.
        // See the comment on whereOmitNoopJoin() for further information.
        //
        // This query optimization is factored out into a separate
        // "no-inline" procedure to keep the sqlite3WhereBegin() procedure
        // from becoming too large.
        notReady = !0;
        if wi.nLevel >= 2 // Must be a join, or this opt8n is pointless
            && pResultSet.nn() // Condition (1)
            && 0 == (wctrlFlags & (WHERE_AGG_DISTINCT | WHERE_KEEP_ALL_JOINS)) // (1),(6)
            && db.OptimizationEnabled(SQLITE_OmitNoopJoin)
        // (7)
        {
            notReady = whereOmitNoopJoin(db, &mut wi, notReady);
            nTabList = wi.nLevel as i32;
            debug_assert!(nTabList > 0);
        }

        // Check to see if there are any SEARCH loops that might benefit from
        // using a Bloom filter.
        if wi.nLevel >= 2 && db.OptimizationEnabled(SQLITE_BloomFilter) {
            whereCheckIfBloomFilterIsUseful(db, &mut wi);
        }

        let nql = (db.ppr().nQueryLoop as i32 + wi.nRowOut as i32) as LogEst;
        db.pp().nQueryLoop = nql;

        // If the caller is an UPDATE or DELETE statement that is requesting
        // to use a one-pass algorithm, determine if this is appropriate.
        //
        // A one-pass approach can be used if the caller has requested one
        // and either (a) the scan visits at most one row or (b) each
        // of the following are true:
        //
        //   * the caller has indicated that a one-pass approach can be used
        //     with multiple rows (by setting WHERE_ONEPASS_MULTIROW), and
        //   * the table is not a virtual table, and
        //   * either the scan does not use the OR optimization or the caller
        //     is a DELETE operation (WHERE_DUPLICATES_OK is only specified
        //     for DELETE).
        //
        // The last qualification is because an UPDATE statement uses
        // WhereInfo.aiCurOnePass[1] to determine whether or not it really
        // can use a one-pass approach, and this is not set accurately for
        // scans that use the OR optimization.
        debug_assert!((wctrlFlags & WHERE_ONEPASS_DESIRED) == 0 || wi.nLevel == 1);
        if (wctrlFlags & WHERE_ONEPASS_DESIRED) != 0 {
            let wsFlags = wi.lp(0).wsFlags;
            let bOnerow = (wsFlags & WHERE_ONEROW) != 0;
            let tab0 = item_tab(db, SrcItemId(pTabList, 0));
            let isVirt = tab0.borrow().IsVirtual();
            if bOnerow
                || (0 != (wctrlFlags & WHERE_ONEPASS_MULTIROW)
                    && !isVirt
                    && (0 == (wsFlags & WHERE_MULTI_OR) || (wctrlFlags & WHERE_DUPLICATES_OK) != 0)
                    && db.OptimizationEnabled(SQLITE_OnePass))
            {
                wi.eOnePass = if bOnerow {
                    ONEPASS_SINGLE as u8
                } else {
                    ONEPASS_MULTI as u8
                };
                if tab0.borrow().HasRowid() && (wsFlags & WHERE_IDX_ONLY) != 0 {
                    if wctrlFlags & WHERE_ONEPASS_MULTIROW != 0 {
                        bFordelete = OPFLAG_FORDELETE;
                    }
                    wi.lpm(0).wsFlags = wsFlags & !WHERE_IDX_ONLY;
                }
            }
        }

        // Open all tables in the pTabList and any indices selected for
        // searching those tables.
        for ii in 0..nTabList as usize {
            let iFrom = wi.a[ii].iFrom as u32;
            let pTabItem = SrcItemId(pTabList, iFrom);
            let pTab = item_tab(db, pTabItem);
            let iDb = db.schema_index(pTab.borrow().pSchema);
            let lbl = sqlite3VdbeMakeLabel(db);
            wi.a[ii].addrBrk = lbl;
            let jt = db.item(pTabItem).fg.jointype;
            if ii == 0 || (jt & JT_LEFT) != 0 {
                wi.a[ii].addrHalt = wi.a[ii].addrBrk;
            } else if wi.a[ii - 1].pRJ.is_some() {
                wi.a[ii].addrHalt = wi.a[ii - 1].addrBrk;
            } else {
                wi.a[ii].addrHalt = wi.a[ii - 1].addrHalt;
            }
            let wsFlags = wi.lp(ii).wsFlags;
            let (tabFlags, isView, isVirtual, nCol, tnum, zName) = {
                let t = pTab.borrow();
                (
                    t.tabFlags,
                    t.IsView(),
                    t.IsVirtual(),
                    t.nCol as i32,
                    t.tnum,
                    t.zName.clone(),
                )
            };
            let iCursor = db.item(pTabItem).iCursor;
            if (tabFlags & TF_Ephemeral) != 0 || isView {
                // Do nothing
            } else if (wsFlags & WHERE_VIRTUALTABLE) != 0 {
                // NOTE: virtual tables are not supported yet (no VTable
                // object to pass as P4_VTAB).
                sqlite3VdbeAddOp4(
                    db.v(),
                    OP_VOpen,
                    iCursor,
                    0,
                    0,
                    P4::Tab(pTab.clone()),
                    P4_VTAB as i32,
                );
            } else if isVirtual {
                // noop
            } else if ((wsFlags & WHERE_IDX_ONLY) == 0 && (wctrlFlags & WHERE_OR_SUBCLAUSE) == 0)
                || (jt & (JT_LTORJ | JT_RIGHT)) != 0
            {
                let mut op = OP_OpenRead;
                if wi.eOnePass as i32 != ONEPASS_OFF {
                    op = OP_OpenWrite;
                    wi.aiCurOnePass[0] = iCursor;
                }
                crate::insert::sqlite3OpenTable(db, iCursor, iDb, &pTab, op as i32);
                debug_assert!(iCursor == wi.a[ii].iTabCur);
                if wi.eOnePass as i32 == ONEPASS_OFF
                    && nCol < BMS
                    && (tabFlags & (TF_HasGenerated | TF_WithoutRowid)) == 0
                    && (wsFlags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) == 0
                {
                    // If we know that only a prefix of the record will be
                    // used, it is advantageous to reduce the "column count"
                    // field in the P4 operand of the OP_OpenRead/Write opcode.
                    let mut b: Bitmask = db.item(pTabItem).colUsed;
                    let mut n = 0;
                    while b != 0 {
                        b >>= 1;
                        n += 1;
                    }
                    sqlite3VdbeChangeP4(db.v(), -1, P4::I(n), P4_INT32 as i32);
                    debug_assert!(n <= nCol);
                }
                sqlite3VdbeChangeP5(db.v(), bFordelete);
                if ii >= 2
                    && (jt & (JT_LTORJ | JT_LEFT)) == 0
                    && wi.a[ii].addrHalt == wi.a[0].addrHalt
                {
                    let brk = wi.iBreak;
                    sqlite3VdbeAddOp2(db.v(), OP_IfEmpty, iCursor, brk);
                }
            } else {
                crate::build::sqlite3TableLock(db, iDb, tnum, 0, Some(&zName[..]));
            }
            if wsFlags & WHERE_INDEXED != 0 {
                let pIx = wi.lp(ii).u.btree.pIndex.clone().unwrap();
                let iIndexCur: i32;
                let mut op = OP_OpenRead;
                // iAuxArg is always set to a positive value if ONEPASS is
                // possible
                debug_assert!(iAuxArg != 0 || (wi.wctrlFlags & WHERE_ONEPASS_DESIRED) == 0);
                let (isPk, bHasExpr, hasPart, ixTnum) = {
                    let p = pIx.borrow();
                    (
                        p.IsPrimaryKeyIndex(),
                        p.bHasExpr,
                        p.pPartIdxWhere.is_some(),
                        p.tnum,
                    )
                };
                if !pTab.borrow().HasRowid() && isPk && (wctrlFlags & WHERE_OR_SUBCLAUSE) != 0 {
                    // This is one term of an OR-optimization using the PRIMARY
                    // KEY of a WITHOUT ROWID table.  No need for a separate
                    // index
                    iIndexCur = wi.a[ii].iTabCur;
                    op = 0;
                } else if wi.eOnePass as i32 != ONEPASS_OFF {
                    let mut c = iAuxArg;
                    debug_assert!(wctrlFlags & WHERE_ONEPASS_DESIRED != 0);
                    for pJ in pTab.borrow().pIndex.iter() {
                        if Rc::ptr_eq(pJ, &pIx) {
                            break;
                        }
                        c += 1;
                    }
                    iIndexCur = c;
                    op = OP_OpenWrite;
                    wi.aiCurOnePass[1] = iIndexCur;
                } else if iAuxArg != 0 && (wctrlFlags & WHERE_OR_SUBCLAUSE) != 0 {
                    iIndexCur = iAuxArg;
                    op = OP_ReopenIdx;
                } else {
                    iIndexCur = db.pp().nTab;
                    db.pp().nTab += 1;
                    if bHasExpr != 0 && db.OptimizationEnabled(SQLITE_IndexedExpr) {
                        whereAddIndexedExpr(db, &mut wi, &pIx, iIndexCur, pTabItem);
                    }
                    if hasPart && (jt & JT_RIGHT) == 0 {
                        let pw = idx_part_where(db, &mut wi, &pIx);
                        wherePartIdxExpr(db, &pIx, pw, &mut None, iIndexCur, Some(pTabItem));
                    }
                }
                wi.a[ii].iIdxCur = iIndexCur;
                debug_assert!(iIndexCur >= 0);
                if op != 0 {
                    sqlite3VdbeAddOp3(db.v(), op, iIndexCur, ixTnum as i32, iDb);
                    sqlite3VdbeSetP4KeyInfo(db, &pIx);
                    let wsFlags = wi.lp(ii).wsFlags;
                    if (wsFlags & WHERE_CONSTRAINT) != 0
                        && (wsFlags & (WHERE_COLUMN_RANGE | WHERE_SKIPSCAN)) == 0
                        && (wsFlags & WHERE_BIGNULL_SORT) == 0
                        && (wsFlags & WHERE_IN_SEEKSCAN) == 0
                        && (wi.wctrlFlags & WHERE_ORDERBY_MIN) == 0
                        && wi.eDistinct != WHERE_DISTINCT_ORDERED
                    {
                        sqlite3VdbeChangeP5(db.v(), OPFLAG_SEEKEQ);
                    }
                }
            }
            if iDb >= 0 {
                crate::build::sqlite3CodeVerifySchema(db, iDb);
            }
            if (jt & JT_RIGHT) != 0 {
                let mut pRJ = WhereRightJoin::default();
                let bBloomable;
                pRJ.iMatch = db.pp().nTab;
                db.pp().nTab += 1;
                db.pp().nMem += 1;
                pRJ.regReturn = db.pp().nMem;
                sqlite3VdbeAddOp2(db.v(), OP_Null, 0, pRJ.regReturn);
                if pTab.borrow().HasRowid() {
                    sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, pRJ.iMatch, 1);
                    bBloomable = 1;
                    let pInfo = crate::select::sqlite3KeyInfoAlloc(db, 1, 0);
                    if let Some(pInfo) = pInfo {
                        {
                            let mut k = pInfo.borrow_mut();
                            if k.aColl.is_empty() {
                                k.aColl.push(None);
                            }
                            k.aColl[0] = None;
                            if k.aSortFlags.is_empty() {
                                k.aSortFlags.push(0);
                            }
                            k.aSortFlags[0] = 0;
                        }
                        sqlite3VdbeAppendP4(db.v(), P4::KeyInfo(pInfo), P4_KEYINFO as i32);
                    }
                } else {
                    let pPk = crate::build::sqlite3PrimaryKeyIndex(&pTab).unwrap();
                    let nk = pPk.borrow().nKeyCol as i32;
                    sqlite3VdbeAddOp2(db.v(), OP_OpenEphemeral, pRJ.iMatch, nk);
                    sqlite3VdbeSetP4KeyInfo(db, &pPk);
                    bBloomable = crate::build::sqlite3IndexBloomable(&pPk, nk);
                }
                if bBloomable != 0 {
                    db.pp().nMem += 1;
                    pRJ.regBloom = db.pp().nMem;
                    sqlite3VdbeAddOp2(db.v(), OP_Blob, 65536, pRJ.regBloom);
                } else {
                    pRJ.regBloom = 0;
                }
                wi.a[ii].pRJ = Some(pRJ);
                wi.lpm(ii).wsFlags &= !WHERE_IDX_ONLY;
                // The nature of RIGHT JOIN processing is such that it messes
                // up the output order.  So omit any ORDER BY/GROUP BY
                // elimination optimizations.  We need to do an actual sort for
                // RIGHT JOIN.
                wi.nOBSat = 0;
                wi.eDistinct = WHERE_DISTINCT_UNORDERED;
            }
        }
        wi.iTop = sqlite3VdbeCurrentAddr(db.v());
        if db.mallocFailed != 0 {
            break 'error;
        }

        // Generate the code to do the search.  Each iteration of the for
        // loop below generates code for a single nested loop of the VM
        // program.
        for ii in 0..nTabList as usize {
            if db.ppr().nErr != 0 {
                break 'error;
            }
            let wsFlags = wi.lp(ii).wsFlags;
            let pSrc = SrcItemId(pTabList, wi.a[ii].iFrom as u32);
            if db.item(pSrc).fg.isMaterialized != 0 {
                debug_assert!(db.item(pSrc).fg.isSubquery != 0);
                let pSubq = **db.item(pSrc).u4.pSubq.as_ref().unwrap();
                let iOnce;
                if db.item(pSrc).fg.isCorrelated == 0 {
                    iOnce = sqlite3VdbeAddOp0(db.v(), OP_Once);
                } else {
                    iOnce = 0;
                }
                sqlite3VdbeAddOp2(db.v(), OP_Gosub, pSubq.regReturn, pSubq.addrFillSub);
                if iOnce != 0 {
                    sqlite3VdbeJumpHere(db.v(), iOnce);
                }
            }
            debug_assert!(pTabList == wi.pTabList);
            if (wsFlags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) != 0 {
                if (wsFlags & WHERE_AUTO_INDEX) != 0 {
                    constructAutomaticIndex(db, &mut wi, SWC, notReady, ii);
                } else {
                    sqlite3ConstructBloomFilter(db, &mut wi, ii, notReady);
                }
                if db.mallocFailed != 0 {
                    break 'error;
                }
            }
            let addrExplain = sqlite3WhereExplainOneScan(db, pTabList, &wi, ii, wctrlFlags);
            wi.a[ii].addrBody = sqlite3VdbeCurrentAddr(db.v());
            notReady = sqlite3WhereCodeOneLoopStart(db, &mut wi, ii as i32, notReady);
            wi.iContinue = wi.a[ii].addrCont;
            if (wsFlags & WHERE_MULTI_OR) == 0 && (wctrlFlags & WHERE_OR_SUBCLAUSE) == 0 {
                sqlite3WhereAddScanStatus(db, pTabList, &wi, ii, addrExplain);
            }
        }

        // Done.
        wi.iEndWhere = sqlite3VdbeCurrentAddr(db.v());
        return Some(wi);
    }

    // Jump here if malloc fails
    // whereBeginError:
    db.pp().nQueryLoop = wi.savedNQueryLoop as LogEst;
    whereInfoFree(db, &mut wi);
    None
}

/// Free a WhereInfo structure
fn whereInfoFree(db: &mut Sqlite3, wi: &mut WhereInfo) {
    sqlite3WhereClauseClear(db, wi, SWC);
    let _ = &wi.pLoops;
}

/// Generate the end of the WHERE loop.  See comments on
/// sqlite3WhereBegin() for additional information.
///
/// C: `void sqlite3WhereEnd(WhereInfo *pWInfo)`
pub fn sqlite3WhereEnd(db: &mut Sqlite3, pWInfo: &mut crate::where_::WhereInfo) {
    let wi = pWInfo;
    let pTabList = wi.pTabList;
    let iEnd = sqlite3VdbeCurrentAddr(db.v());
    let mut nRJ = 0;
    let mut addrSeek = 0;

    // Generate loop termination code.
    let mut i = wi.nLevel as i32 - 1;
    while i >= 0 {
        let iu = i as usize;
        if let Some(mut pRJ) = wi.a[iu].pRJ {
            // Terminate the subroutine that forms the interior of the loop of
            // the RIGHT JOIN table
            let ac = wi.a[iu].addrCont;
            sqlite3VdbeResolveLabel(db.v(), ac);
            // Replace addrCont with a new label that will never be used, just
            // so the subsequent call to resolve pLevel->addrCont will have
            // something to resolve.
            wi.a[iu].addrCont = sqlite3VdbeMakeLabel(db);
            pRJ.endSubrtn = sqlite3VdbeCurrentAddr(db.v());
            sqlite3VdbeAddOp3(db.v(), OP_Return, pRJ.regReturn, pRJ.addrSubrtn, 1);
            wi.a[iu].pRJ = Some(pRJ);
            nRJ += 1;
        }
        let lp = wi.lp(iu).clone();
        let lv = wi.a[iu].clone();
        if lv.op != OP_Noop {
            if wi.eDistinct == WHERE_DISTINCT_ORDERED
                && i == wi.nLevel as i32 - 1 // Ticket [ef9318757b152e3] 2017-10-21
                && (lp.wsFlags & WHERE_INDEXED) != 0
            {
                let pIdx = lp.u.btree.pIndex.clone().unwrap();
                let (hasStat1, n, rle) = {
                    let p = pIdx.borrow();
                    let n = lp.u.btree.nDistinctCol as usize;
                    (p.hasStat1, n, if n > 0 { p.aiRowLogEst[n] } else { 0 })
                };
                if hasStat1 != 0 && n > 0 && rle >= 36 {
                    let n = n as i32;
                    let r1 = db.ppr().nMem + 1;
                    let mut addrIfNull = 0; // Init to avoid false-positive compiler warning
                    if lv.iLeftJoin != 0 {
                        addrIfNull = sqlite3VdbeAddOp2(db.v(), OP_IfNullRow, lv.iIdxCur, r1);
                    }
                    for j in 0..n {
                        sqlite3VdbeAddOp3(db.v(), OP_Column, lv.iIdxCur, j, r1 + j);
                    }
                    db.pp().nMem += n + 1;
                    let op = if lv.op == OP_Prev {
                        OP_SeekLT
                    } else {
                        OP_SeekGT
                    };
                    addrSeek = sqlite3VdbeAddOp4Int(db.v(), op, lv.iIdxCur, 0, r1, n);
                    sqlite3VdbeAddOp2(db.v(), OP_Goto, 1, lv.p2);
                    if lv.iLeftJoin != 0 {
                        sqlite3VdbeJumpHere(db.v(), addrIfNull);
                    }
                }
            }
        }
        if db.item(SrcItemId(pTabList, lv.iFrom as u32)).fg.fromExists != 0 {
            // This is an EXISTS-to-JOIN optimization loop. If this loop sees
            // a successful row, it should break out of itself.
            sqlite3VdbeAddOp2(db.v(), OP_Goto, 0, lv.addrBrk);
        }
        let ac = wi.a[iu].addrCont;
        sqlite3VdbeResolveLabel(db.v(), ac);
        if lv.op != OP_Noop {
            sqlite3VdbeAddOp3(db.v(), lv.op, lv.p1, lv.p2, lv.p3 as i32);
            sqlite3VdbeChangeP5(db.v(), lv.p5 as u16);
            if addrSeek != 0 {
                sqlite3VdbeJumpHere(db.v(), addrSeek);
                addrSeek = 0;
            }
            if lv.regBignull != 0 {
                sqlite3VdbeResolveLabel(db.v(), lv.addrBignull);
                sqlite3VdbeAddOp2(db.v(), OP_DecrJumpZero, lv.regBignull, lv.p2 - 1);
            }
        }
        if (lp.wsFlags & WHERE_IN_ABLE) != 0 && lv.u.r#in.nIn() > 0 {
            sqlite3VdbeResolveLabel(db.v(), lv.addrNxt);
            let mut j = lv.u.r#in.nIn() as usize;
            while j > 0 {
                let pIn = lv.u.r#in.aInLoop[j - 1];
                debug_assert!(
                    sqlite3VdbeGetOp(db.v(), pIn.addrInTop + 1).opcode == OP_IsNull
                        || db.mallocFailed != 0
                );
                sqlite3VdbeJumpHere(db.v(), pIn.addrInTop + 1);
                if pIn.eEndLoopOp != OP_Noop {
                    if pIn.nPrefix != 0 {
                        let bEarlyOut = (lp.wsFlags & WHERE_VIRTUALTABLE) == 0
                            && (lp.wsFlags & WHERE_IN_EARLYOUT) != 0;
                        if lv.iLeftJoin != 0 {
                            // For LEFT JOIN queries, cursor pIn->iCur might have
                            // been disabled by OP_NullRow.  This occurs for
                            // WHERE clauses such as "a = ? AND b IN (...)",
                            // where the index is on (a, b).  When the
                            // OP_IfNullRow jump is taken, the pIn->iBase
                            // register is NULLed out as a side effect.  But as
                            // that register is not used when the jump is taken,
                            // the side-effect is harmless.
                            let a = sqlite3VdbeCurrentAddr(db.v()) + 2 + bEarlyOut as i32;
                            sqlite3VdbeAddOp3(db.v(), OP_IfNullRow, pIn.iCur, a, pIn.iBase);
                        }
                        if bEarlyOut {
                            let a = sqlite3VdbeCurrentAddr(db.v()) + 2;
                            sqlite3VdbeAddOp4Int(
                                db.v(),
                                OP_IfNoHope,
                                lv.iIdxCur,
                                a,
                                pIn.iBase,
                                pIn.nPrefix,
                            );
                            // Retarget the OP_IsNull against the left operand of
                            // IN so it jumps past the OP_IfNoHope.  This is
                            // because the OP_IsNull also bypasses the
                            // OP_Affinity opcode that is required by
                            // OP_IfNoHope.
                            sqlite3VdbeJumpHere(db.v(), pIn.addrInTop + 1);
                        }
                    }
                    sqlite3VdbeAddOp2(db.v(), pIn.eEndLoopOp, pIn.iCur, pIn.addrInTop);
                }
                sqlite3VdbeJumpHere(db.v(), pIn.addrInTop - 1);
                j -= 1;
            }
        }
        sqlite3VdbeResolveLabel(db.v(), lv.addrBrk);
        if let Some(pRJ) = wi.a[iu].pRJ {
            sqlite3VdbeAddOp3(db.v(), OP_Return, pRJ.regReturn, 0, 1);
        }
        if lv.addrSkip != 0 {
            sqlite3VdbeGoto(db.v(), lv.addrSkip);
            sqlite3VdbeJumpHere(db.v(), lv.addrSkip);
            sqlite3VdbeJumpHere(db.v(), lv.addrSkip - 2);
        }
        if lv.addrLikeRep != 0 {
            sqlite3VdbeAddOp2(
                db.v(),
                OP_DecrJumpZero,
                (lv.iLikeRepCntr >> 1) as i32,
                lv.addrLikeRep,
            );
        }
        if lv.iLeftJoin != 0 {
            let ws = lp.wsFlags;
            let addr = sqlite3VdbeAddOp1(db.v(), OP_IfPos, lv.iLeftJoin);
            debug_assert!((ws & WHERE_IDX_ONLY) == 0 || (ws & WHERE_INDEXED) != 0);
            if (ws & WHERE_IDX_ONLY) == 0 {
                let pSrc = SrcItemId(pTabList, lv.iFrom as u32);
                debug_assert!(lv.iTabCur == db.item(pSrc).iCursor);
                if db.item(pSrc).fg.viaCoroutine != 0 {
                    debug_assert!(db.item(pSrc).fg.isSubquery != 0);
                    let n = db.item(pSrc).u4.pSubq.as_ref().unwrap().regResult;
                    let m = item_tab(db, pSrc).borrow().nCol as i32;
                    sqlite3VdbeAddOp3(db.v(), OP_Null, 0, n, n + m - 1);
                }
                sqlite3VdbeAddOp1(db.v(), OP_NullRow, lv.iTabCur);
            }
            if (ws & WHERE_INDEXED) != 0
                || ((ws & WHERE_MULTI_OR) != 0 && lv.u.pCoveringIdx.is_some())
            {
                if ws & WHERE_MULTI_OR != 0 {
                    let pIx = lv.u.pCoveringIdx.clone().unwrap();
                    let (sch, tn) = {
                        let p = pIx.borrow();
                        (p.pSchema, p.tnum)
                    };
                    let iDb = db.schema_index(sch);
                    sqlite3VdbeAddOp3(db.v(), OP_ReopenIdx, lv.iIdxCur, tn as i32, iDb);
                    sqlite3VdbeSetP4KeyInfo(db, &pIx);
                }
                sqlite3VdbeAddOp1(db.v(), OP_NullRow, lv.iIdxCur);
            }
            if ws & WHERE_IN_ABLE != 0 {
                for m in 0..lv.u.r#in.aInLoop.len() {
                    let pIn = lv.u.r#in.aInLoop[m];
                    if pIn.eEndLoopOp != OP_Noop {
                        sqlite3VdbeAddOp1(db.v(), OP_NullRow, pIn.iCur);
                    }
                }
            }
            if lv.op == OP_Return {
                sqlite3VdbeAddOp2(db.v(), OP_Gosub, lv.p1, lv.addrFirst);
            } else {
                sqlite3VdbeGoto(db.v(), lv.addrFirst);
            }
            sqlite3VdbeJumpHere(db.v(), addr);
        }
        i -= 1;
    }

    debug_assert!(wi.nLevel as i32 <= db.src(pTabList).nSrc());
    for i in 0..wi.nLevel as usize {
        let pTabItem = SrcItemId(pTabList, wi.a[i].iFrom as u32);
        let pTab = item_tab(db, pTabItem);
        let lp = wi.lp(i).clone();

        // Do RIGHT JOIN processing.  Generate code that will output the
        // unmatched rows of the right operand of the RIGHT JOIN with
        // all of the columns of the left operand set to NULL.
        if wi.a[i].pRJ.is_some() {
            sqlite3WhereRightJoinLoop(db, wi, i as i32);
            continue;
        }

        // For a co-routine, change all OP_Column references to the table of
        // the co-routine into OP_Copy of result contained in a register.
        // OP_Rowid becomes OP_Null.
        if db.item(pTabItem).fg.viaCoroutine != 0 {
            debug_assert!(db.item(pTabItem).fg.isSubquery != 0);
            let regResult = db.item(pTabItem).u4.pSubq.as_ref().unwrap().regResult;
            debug_assert!(regResult >= 0);
            let (ab, tc) = (wi.a[i].addrBody, wi.a[i].iTabCur);
            translateColumnToCopy(db, ab, tc, regResult, 0);
            continue;
        }

        // If this scan uses an index, make VDBE code substitutions to read
        // data from the index instead of from the table where possible.  In
        // some cases this optimization prevents the table from ever being
        // read, which can yield a significant performance boost.
        //
        // Calls to the code generator in between sqlite3WhereBegin and
        // sqlite3WhereEnd will have created code that references the table
        // directly.  This loop scans all that code looking for opcodes
        // that reference the table and converts them into opcodes that
        // reference the index.
        let mut pIdx: Option<IndexRef> = None;
        if lp.wsFlags & (WHERE_INDEXED | WHERE_IDX_ONLY) != 0 {
            pIdx = lp.u.btree.pIndex.clone();
        } else if lp.wsFlags & WHERE_MULTI_OR != 0 {
            pIdx = wi.a[i].u.pCoveringIdx.clone();
        }
        if let Some(pIdx) = pIdx {
            if db.mallocFailed != 0 {
                continue;
            }
            let last: i32;
            let idxTabHasRowid = pIdx.borrow().table().borrow().HasRowid();
            if wi.eOnePass as i32 == ONEPASS_OFF || !idxTabHasRowid {
                last = iEnd;
            } else {
                last = wi.iEndWhere;
            }
            let iIdxCur = wi.a[i].iIdxCur;
            let iTabCur = wi.a[i].iTabCur;
            if pIdx.borrow().bHasExpr != 0 {
                for p in db.pp().pIdxEpr.iter_mut() {
                    if p.iIdxCur == iIdxCur {
                        p.iDataCur = -1;
                        p.iIdxCur = -1;
                    }
                }
            }
            let tabHasRowid = pTab.borrow().HasRowid();
            let pPk = if !tabHasRowid {
                crate::build::sqlite3PrimaryKeyIndex(&pTab)
            } else {
                None
            };
            let mut k = wi.a[i].addrBody + 1;
            let mut lastOp = last;
            if lastOp <= k {
                // C uses a do{}while loop: the first opcode is always visited
                lastOp = k + 1;
            }
            while k < lastOp {
                if (k as usize) >= db.v().aOp.len() {
                    break;
                }
                let (opcode, p1, p2) = {
                    let o = &db.v().aOp[k as usize];
                    (o.opcode, o.p1, o.p2)
                };
                if p1 != iTabCur {
                    // no-op
                } else if opcode == OP_Column {
                    let mut x = p2;
                    if !tabHasRowid {
                        x = pPk.as_ref().unwrap().borrow().aiColumn[x as usize] as i32;
                        debug_assert!(x >= 0);
                    } else {
                        x = crate::build::sqlite3StorageColumnToTable(&pTab, x as i16) as i32;
                    }
                    x = crate::build::sqlite3TableColumnToIndex(&pIdx, x);
                    if x >= 0 {
                        let o = &mut db.v().aOp[k as usize];
                        o.p2 = x;
                        o.p1 = iIdxCur;
                    } else if wi.lp(i).wsFlags & (WHERE_IDX_ONLY | WHERE_EXPRIDX) != 0 {
                        if wi.lp(i).wsFlags & WHERE_IDX_ONLY != 0 {
                            // An error. pLoop is supposed to be a covering
                            // index loop, and yet the VM code refers to a column
                            // of the table that is not part of the index.
                            errmsg!(db, "internal query planner error");
                            db.pp().rc = SQLITE_INTERNAL;
                        } else {
                            // The WHERE_EXPRIDX flag is set by the planner when
                            // it is likely that pLoop is a covering index loop,
                            // but it is not possible to be 100% sure. In this
                            // case, any OP_Explain opcode corresponding to this
                            // loop describes the index as a "COVERING INDEX".
                            // But, pOp proves that pLoop is not actually a
                            // covering index loop. So clear the WHERE_EXPRIDX
                            // flag and rewrite the text that accompanies the
                            // OP_Explain opcode, if any.
                            wi.lpm(i).wsFlags &= !WHERE_EXPRIDX;
                            let ab = wi.a[i].addrBody;
                            let wf = wi.wctrlFlags;
                            sqlite3WhereAddExplainText(db, ab - 1, pTabList, wi, i, wf);
                        }
                    }
                } else if opcode == OP_Rowid {
                    let o = &mut db.v().aOp[k as usize];
                    o.p1 = iIdxCur;
                    o.opcode = OP_IdxRowid;
                } else if opcode == OP_IfNullRow {
                    let o = &mut db.v().aOp[k as usize];
                    o.p1 = iIdxCur;
                }
                k += 1;
            }
        }
    }

    // The "break" point is here, just past the end of the outer loop.
    // Set it.
    let brk = wi.iBreak;
    sqlite3VdbeResolveLabel(db.v(), brk);

    // Final cleanup
    db.pp().nQueryLoop = wi.savedNQueryLoop as LogEst;
    whereInfoFree(db, wi);
    db.pp().withinRJSubrtn -= nRJ;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lp(iTab: u8, prereq: Bitmask, rRun: LogEst, nOut: LogEst) -> WhereLoop {
        let mut p = WhereLoop::default();
        whereLoopInit(&mut p);
        p.iTab = iTab;
        p.prereq = prereq;
        p.rRun = rRun;
        p.nOut = nOut;
        p
    }

    #[test]
    fn or_set_keeps_best() {
        let mut s = WhereOrSet::default();
        assert_eq!(whereOrInsert(&mut s, 0, 50, 20), 1);
        // dominated (more prereqs, higher cost): discarded
        assert_eq!(whereOrInsert(&mut s, 1, 60, 10), 0);
        assert_eq!(s.n, 1);
        // cheaper with same prereqs: overwrites, nOut keeps the minimum
        assert_eq!(whereOrInsert(&mut s, 0, 40, 30), 1);
        assert_eq!(s.n, 1);
        assert_eq!((s.a[0].rRun, s.a[0].nOut), (40, 20));
        // incomparable prereqs and costs: appended
        assert_eq!(whereOrInsert(&mut s, 4, 20, 5), 1);
        assert_eq!(s.n, 2);
    }

    #[test]
    fn find_lesser() {
        let loops = vec![lp(0, 0, 50, 40), lp(1, 1, 30, 20)];
        // a better loop for table 0 replaces loop 0
        assert_eq!(whereLoopFindLesser(&loops, 0, &lp(0, 0, 40, 30)), Some(0));
        // a worse loop for table 0 is discarded
        assert_eq!(whereLoopFindLesser(&loops, 0, &lp(0, 0, 60, 50)), None);
        // a loop for another table is appended
        assert_eq!(whereLoopFindLesser(&loops, 0, &lp(2, 0, 60, 50)), Some(2));
        // more prerequisites but cheaper: appended (cannot replace)
        assert_eq!(whereLoopFindLesser(&loops, 0, &lp(0, 2, 10, 10)), Some(2));
    }

    #[test]
    fn cheaper_proper_subset() {
        let t1 = TermId::new(0, 1);
        let t2 = TermId::new(0, 2);
        let mut x = lp(0, 0, 30, 20);
        x.wsFlags = WHERE_INDEXED;
        x.nLTerm = 1;
        x.set_lterm(0, Some(t1));
        let mut y = lp(0, 0, 30, 20);
        y.wsFlags = WHERE_INDEXED;
        y.nLTerm = 2;
        y.set_lterm(0, Some(t1));
        y.set_lterm(1, Some(t2));
        assert_eq!(whereLoopCheaperProperSubset(&x, &y), 1);
        assert_eq!(whereLoopCheaperProperSubset(&y, &x), 0);
        let mut ys = y.clone();
        whereLoopAdjustCost(&[x.clone()], &mut ys);
        assert_eq!((ys.rRun, ys.nOut), (30, 19));
    }
}
