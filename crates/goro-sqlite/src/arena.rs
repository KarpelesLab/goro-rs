//! The parse-tree arena.
//!
//! SQLite's parse trees are graphs of heap objects linked by pointers
//! (including back pointers such as Select.pNext and Window.ppThis).  In
//! this port every parse-tree node lives in an `Arena` and is referenced by
//! a typed 32-bit index.  Index 0 of every vector is a dummy node and the id
//! value 0 plays the role of the C NULL pointer.
//!
//! The arena of a connection (`Sqlite3::arena`) holds the nodes of all
//! parses in progress; it is reset when the outermost parse finishes.
//! Schema objects that must outlive a parse (CHECK constraints, column
//! defaults, view definitions, triggers, partial index WHERE clauses,
//! index expressions) keep their trees in a `Tree<I>`, which is either a
//! live id into the connection arena (while the object is being built) or
//! an owned private arena (once published in a schema).  `Tree::get()`
//! imports an owned tree into the connection arena by graph-copying it.

use crate::int::*;
use std::collections::HashMap;
use std::rc::Rc;

macro_rules! def_id {
    ($name:ident) => {
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
        pub struct $name(pub u32);
        impl $name {
            pub const NULL: $name = $name(0);
            #[inline]
            pub fn is_null(self) -> bool {
                self.0 == 0
            }
            #[inline]
            pub fn nn(self) -> bool {
                self.0 != 0
            }
        }
    };
}

def_id!(ExprId);
def_id!(ExprListId);
def_id!(SelectId);
def_id!(SrcListId);
def_id!(IdListId);
def_id!(WindowId);
def_id!(WithId);
def_id!(UpsertId);
def_id!(AggInfoId);
def_id!(NcId);
def_id!(CteUseId);
def_id!(TriggerStepId);

/// Storage for parse-tree nodes.
pub struct Arena {
    pub exprs: Vec<Expr>,
    pub elists: Vec<ExprList>,
    pub selects: Vec<Select>,
    pub srclists: Vec<SrcList>,
    pub idlists: Vec<IdList>,
    pub windows: Vec<Window>,
    pub withs: Vec<With>,
    pub upserts: Vec<Upsert>,
    pub aggs: Vec<AggInfo>,
    pub ncs: Vec<NameContext>,
    pub cteuses: Vec<CteUse>,
    pub tsteps: Vec<TriggerStep>,
}

impl Default for Arena {
    fn default() -> Self {
        Arena::new()
    }
}

macro_rules! accessors {
    ($get:ident, $getm:ident, $new:ident, $field:ident, $id:ident, $ty:ty) => {
        #[inline]
        pub fn $get(&self, id: $id) -> &$ty {
            debug_assert!(id.0 != 0, concat!("NULL ", stringify!($id)));
            &self.$field[id.0 as usize]
        }
        #[inline]
        pub fn $getm(&mut self, id: $id) -> &mut $ty {
            debug_assert!(id.0 != 0, concat!("NULL ", stringify!($id)));
            &mut self.$field[id.0 as usize]
        }
        #[inline]
        pub fn $new(&mut self, v: $ty) -> $id {
            self.$field.push(v);
            $id((self.$field.len() - 1) as u32)
        }
    };
}

impl Arena {
    pub fn new() -> Arena {
        Arena {
            exprs: vec![Expr::default()],
            elists: vec![ExprList::default()],
            selects: vec![Select::default()],
            srclists: vec![SrcList::default()],
            idlists: vec![IdList::default()],
            windows: vec![Window::default()],
            withs: vec![With::default()],
            upserts: vec![Upsert::default()],
            aggs: vec![AggInfo::default()],
            ncs: vec![NameContext::default()],
            cteuses: vec![CteUse::default()],
            tsteps: vec![TriggerStep::default()],
        }
    }

    /// Drop every node (called when the outermost parse finishes).
    pub fn reset(&mut self) {
        *self = Arena::new();
    }

    /// Total number of nodes (for diagnostics).
    pub fn len(&self) -> usize {
        self.exprs.len() + self.elists.len() + self.selects.len() + self.srclists.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 4
    }

    accessors!(ex, exm, new_expr, exprs, ExprId, Expr);
    accessors!(el, elm, new_elist, elists, ExprListId, ExprList);
    accessors!(sel, selm, new_select, selects, SelectId, Select);
    accessors!(src, srcm, new_srclist, srclists, SrcListId, SrcList);
    accessors!(idl, idlm, new_idlist, idlists, IdListId, IdList);
    accessors!(win, winm, new_window, windows, WindowId, Window);
    accessors!(with, withm, new_with, withs, WithId, With);
    accessors!(ups, upsm, new_upsert, upserts, UpsertId, Upsert);
    accessors!(agg, aggm, new_agg, aggs, AggInfoId, AggInfo);
    accessors!(nc, ncm, new_nc, ncs, NcId, NameContext);
    accessors!(cteuse, cteusem, new_cteuse, cteuses, CteUseId, CteUse);
    accessors!(tstep, tstepm, new_tstep, tsteps, TriggerStepId, TriggerStep);
}

// ---------------------------------------------------------------------------
// Graph copy between arenas
// ---------------------------------------------------------------------------

/// Old-id -> new-id maps used while copying a graph of nodes from one arena
/// to another.  Shared nodes are copied once, so sharing and back pointers
/// are preserved.
#[derive(Default)]
pub struct CopyMap {
    expr: HashMap<u32, u32>,
    elist: HashMap<u32, u32>,
    select: HashMap<u32, u32>,
    srclist: HashMap<u32, u32>,
    idlist: HashMap<u32, u32>,
    window: HashMap<u32, u32>,
    with: HashMap<u32, u32>,
    upsert: HashMap<u32, u32>,
    agg: HashMap<u32, u32>,
    cteuse: HashMap<u32, u32>,
    tstep: HashMap<u32, u32>,
}

/// Implemented by every arena id type.
pub trait ArenaId: Copy + Default + PartialEq + std::fmt::Debug {
    fn is_null_id(self) -> bool;
    /// Copy the node graph rooted at `self` from `src` into `dst`.
    fn copy_into(self, src: &Arena, dst: &mut Arena, map: &mut CopyMap) -> Self;
}

macro_rules! impl_copy {
    ($id:ident, $field:ident, $mapf:ident, $fix:ident) => {
        impl ArenaId for $id {
            fn is_null_id(self) -> bool {
                self.0 == 0
            }
            fn copy_into(self, src: &Arena, dst: &mut Arena, map: &mut CopyMap) -> Self {
                if self.0 == 0 {
                    return $id(0);
                }
                if let Some(&n) = map.$mapf.get(&self.0) {
                    return $id(n);
                }
                let node = src.$field[self.0 as usize].clone();
                dst.$field.push(node);
                let new = (dst.$field.len() - 1) as u32;
                map.$mapf.insert(self.0, new);
                $fix(new, src, dst, map);
                $id(new)
            }
        }
    };
}

fn fix_expr(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let e = dst.exprs[n as usize].clone();
    let l = e.pLeft.copy_into(src, dst, map);
    let r = e.pRight.copy_into(src, dst, map);
    let pl = e.x.pList.copy_into(src, dst, map);
    let ps = e.x.pSelect.copy_into(src, dst, map);
    let pw = e.y.pWin.copy_into(src, dst, map);
    let pa = e.pAggInfo.copy_into(src, dst, map);
    let d = &mut dst.exprs[n as usize];
    d.pLeft = l;
    d.pRight = r;
    d.x.pList = pl;
    d.x.pSelect = ps;
    d.y.pWin = pw;
    d.pAggInfo = pa;
}

fn fix_elist(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let cnt = dst.elists[n as usize].a.len();
    for i in 0..cnt {
        let e = dst.elists[n as usize].a[i].pExpr;
        let ne = e.copy_into(src, dst, map);
        dst.elists[n as usize].a[i].pExpr = ne;
    }
}

fn fix_select(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let s = dst.selects[n as usize].clone();
    let v = (
        s.pEList.copy_into(src, dst, map),
        s.pSrc.copy_into(src, dst, map),
        s.pWhere.copy_into(src, dst, map),
        s.pGroupBy.copy_into(src, dst, map),
        s.pHaving.copy_into(src, dst, map),
        s.pOrderBy.copy_into(src, dst, map),
        s.pPrior.copy_into(src, dst, map),
        s.pNext.copy_into(src, dst, map),
        s.pLimit.copy_into(src, dst, map),
        s.pWith.copy_into(src, dst, map),
        s.pWin.copy_into(src, dst, map),
        s.pWinDefn.copy_into(src, dst, map),
    );
    let d = &mut dst.selects[n as usize];
    d.pEList = v.0;
    d.pSrc = v.1;
    d.pWhere = v.2;
    d.pGroupBy = v.3;
    d.pHaving = v.4;
    d.pOrderBy = v.5;
    d.pPrior = v.6;
    d.pNext = v.7;
    d.pLimit = v.8;
    d.pWith = v.9;
    d.pWin = v.10;
    d.pWinDefn = v.11;
}

fn fix_srclist(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let cnt = dst.srclists[n as usize].a.len();
    for i in 0..cnt {
        let it = dst.srclists[n as usize].a[i].clone();
        let fa = it.u1.pFuncArg.copy_into(src, dst, map);
        let cu = it.u2.pCteUse.copy_into(src, dst, map);
        let on = it.u3.pOn.copy_into(src, dst, map);
        let us = it.u3.pUsing.copy_into(src, dst, map);
        let sq = it.u4.pSubq.as_ref().map(|q| {
            let mut q2 = **q;
            q2.pSelect = q.pSelect.copy_into(src, dst, map);
            Box::new(q2)
        });
        let d = &mut dst.srclists[n as usize].a[i];
        d.u1.pFuncArg = fa;
        d.u2.pCteUse = cu;
        d.u3.pOn = on;
        d.u3.pUsing = us;
        d.u4.pSubq = sq;
    }
}

fn fix_idlist(_n: u32, _src: &Arena, _dst: &mut Arena, _map: &mut CopyMap) {}

fn fix_window(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let w = dst.windows[n as usize].clone();
    let v = (
        w.pPartition.copy_into(src, dst, map),
        w.pOrderBy.copy_into(src, dst, map),
        w.pStart.copy_into(src, dst, map),
        w.pEnd.copy_into(src, dst, map),
        w.pNextWin.copy_into(src, dst, map),
        w.pFilter.copy_into(src, dst, map),
        w.pOwner.copy_into(src, dst, map),
    );
    let link = match w.ppThis {
        WinLink::None => WinLink::None,
        WinLink::Select(s) => WinLink::Select(s.copy_into(src, dst, map)),
        WinLink::Win(x) => WinLink::Win(x.copy_into(src, dst, map)),
    };
    let d = &mut dst.windows[n as usize];
    d.pPartition = v.0;
    d.pOrderBy = v.1;
    d.pStart = v.2;
    d.pEnd = v.3;
    d.pNextWin = v.4;
    d.pFilter = v.5;
    d.pOwner = v.6;
    d.ppThis = link;
}

fn fix_with(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let w = dst.withs[n as usize].clone();
    let outer = w.pOuter.copy_into(src, dst, map);
    let mut ctes = w.a.clone();
    for c in ctes.iter_mut() {
        c.pCols = c.pCols.copy_into(src, dst, map);
        c.pSelect = c.pSelect.copy_into(src, dst, map);
        c.pUse = c.pUse.copy_into(src, dst, map);
    }
    let d = &mut dst.withs[n as usize];
    d.pOuter = outer;
    d.a = ctes;
}

fn fix_upsert(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let u = dst.upserts[n as usize].clone();
    let v = (
        u.pUpsertTarget.copy_into(src, dst, map),
        u.pUpsertTargetWhere.copy_into(src, dst, map),
        u.pUpsertSet.copy_into(src, dst, map),
        u.pUpsertWhere.copy_into(src, dst, map),
        u.pNextUpsert.copy_into(src, dst, map),
        u.pUpsertSrc.copy_into(src, dst, map),
    );
    let d = &mut dst.upserts[n as usize];
    d.pUpsertTarget = v.0;
    d.pUpsertTargetWhere = v.1;
    d.pUpsertSet = v.2;
    d.pUpsertWhere = v.3;
    d.pNextUpsert = v.4;
    d.pUpsertSrc = v.5;
}

fn fix_agg(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let a = dst.aggs[n as usize].clone();
    let gb = a.pGroupBy.copy_into(src, dst, map);
    let mut cols = a.aCol.clone();
    for c in cols.iter_mut() {
        c.pCExpr = c.pCExpr.copy_into(src, dst, map);
    }
    let mut funcs = a.aFunc.clone();
    for f in funcs.iter_mut() {
        f.pFExpr = f.pFExpr.copy_into(src, dst, map);
    }
    let d = &mut dst.aggs[n as usize];
    d.pGroupBy = gb;
    d.aCol = cols;
    d.aFunc = funcs;
}

fn fix_cteuse(_n: u32, _src: &Arena, _dst: &mut Arena, _map: &mut CopyMap) {}

fn fix_tstep(n: u32, src: &Arena, dst: &mut Arena, map: &mut CopyMap) {
    let t = dst.tsteps[n as usize].clone();
    let v = (
        t.pSelect.copy_into(src, dst, map),
        t.pSrc.copy_into(src, dst, map),
        t.pWhere.copy_into(src, dst, map),
        t.pExprList.copy_into(src, dst, map),
        t.pIdList.copy_into(src, dst, map),
        t.pUpsert.copy_into(src, dst, map),
        t.pNext.copy_into(src, dst, map),
        t.pLast.copy_into(src, dst, map),
    );
    let d = &mut dst.tsteps[n as usize];
    d.pSelect = v.0;
    d.pSrc = v.1;
    d.pWhere = v.2;
    d.pExprList = v.3;
    d.pIdList = v.4;
    d.pUpsert = v.5;
    d.pNext = v.6;
    d.pLast = v.7;
}

impl_copy!(ExprId, exprs, expr, fix_expr);
impl_copy!(ExprListId, elists, elist, fix_elist);
impl_copy!(SelectId, selects, select, fix_select);
impl_copy!(SrcListId, srclists, srclist, fix_srclist);
impl_copy!(IdListId, idlists, idlist, fix_idlist);
impl_copy!(WindowId, windows, window, fix_window);
impl_copy!(WithId, withs, with, fix_with);
impl_copy!(UpsertId, upserts, upsert, fix_upsert);
impl_copy!(AggInfoId, aggs, agg, fix_agg);
impl_copy!(CteUseId, cteuses, cteuse, fix_cteuse);
impl_copy!(TriggerStepId, tsteps, tstep, fix_tstep);

/// A parse tree owned by a schema object.
pub struct Owned<I> {
    pub arena: Arena,
    pub root: I,
}

/// A parse tree that is either live in the connection arena or owned by a
/// schema object.  See the module documentation.
pub enum Tree<I> {
    None,
    Live(I),
    Owned(Rc<Owned<I>>),
}

impl<I: Clone> Clone for Tree<I> {
    fn clone(&self) -> Self {
        match self {
            Tree::None => Tree::None,
            Tree::Live(i) => Tree::Live(i.clone()),
            Tree::Owned(o) => Tree::Owned(o.clone()),
        }
    }
}

impl<I> Default for Tree<I> {
    fn default() -> Self {
        Tree::None
    }
}

impl<I: ArenaId> Tree<I> {
    /// C `p->pX != 0`.
    pub fn is_some(&self) -> bool {
        match self {
            Tree::None => false,
            Tree::Live(i) => !i.is_null_id(),
            Tree::Owned(_) => true,
        }
    }
    pub fn is_none(&self) -> bool {
        !self.is_some()
    }
    /// The tree as an id in arena `a`; owned trees are copied into `a`
    /// (each call makes a fresh copy).
    pub fn get(&self, a: &mut Arena) -> I {
        match self {
            Tree::None => I::default(),
            Tree::Live(i) => *i,
            Tree::Owned(o) => o.root.copy_into(&o.arena, a, &mut CopyMap::default()),
        }
    }
    /// The live id, if the tree is live (no copy).
    pub fn live(&self) -> Option<I> {
        match self {
            Tree::Live(i) => Some(*i),
            _ => None,
        }
    }
    /// Turn a live tree into an owned one (called when a schema object is
    /// published, i.e. inserted into a Schema).
    pub fn publish(&mut self, a: &Arena) {
        if let Tree::Live(i) = *self {
            if i.is_null_id() {
                *self = Tree::None;
            } else {
                let mut na = Arena::new();
                let root = i.copy_into(a, &mut na, &mut CopyMap::default());
                *self = Tree::Owned(Rc::new(Owned { arena: na, root }));
            }
        }
    }
    pub fn from_id(i: I) -> Tree<I> {
        if i.is_null_id() {
            Tree::None
        } else {
            Tree::Live(i)
        }
    }
}

impl Arena {
    /// Graph-copy a tree rooted at `id` in `src` into this arena.
    pub fn import<I: ArenaId>(&mut self, src: &Arena, id: I) -> I {
        id.copy_into(src, self, &mut CopyMap::default())
    }
}
