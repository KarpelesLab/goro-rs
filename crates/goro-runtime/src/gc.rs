//! Cycle collection.
//!
//! PHP manual (Garbage Collection, "Collecting Cycles"): reference counting
//! frees values as soon as their count reaches zero, but a cycle keeps its
//! own counts above zero. PHP records *possible roots* — containers whose
//! refcount was decremented to a non-zero value — in a buffer, and when the
//! buffer is full (10 000 roots) or `gc_collect_cycles()` is called, it
//! looks for garbage reachable from those roots.
//!
//! goro uses the same root-buffer idea with a trial-counting scan over the
//! subgraph reachable from the roots:
//!
//! 1. Every container reachable from a root is a node (arrays' data,
//!    reference cells, objects). For each node we know its total refcount
//!    (from `Rc::strong_count`) and count the references coming from other
//!    nodes of the subgraph.
//! 2. A node whose refcount exceeds its internal references is referenced
//!    from outside (a variable, the VM stack, ...): it and everything it
//!    reaches are alive.
//! 3. Everything else is garbage. Garbage objects whose destructor has not
//!    run get it run first (through the engine's callback); since a
//!    destructor may resurrect objects, the scan is then repeated.
//! 4. Garbage is freed by clearing the contents of its reference cells and
//!    objects, which breaks every cycle (an array cannot contain itself
//!    without a reference or an object in between); reference counting
//!    then frees the rest.
//!
//! Root recording costs one flag check on drop paths that leave a shared
//! container alive; arrays are only recorded once they have held an array,
//! object or reference (scalar-only arrays cannot be part of a cycle).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::array::{ARRAY_GC_BUFFERED, ArrayData};
use crate::object::{F_DTOR_CALLED, F_GC_BUFFERED, F_GC_CLEARED, F_HAS_DTOR, ObjRef, Object};
use crate::reference::{PhpRef, RefInner};
use crate::value::Value;

/// PHP's default root-buffer threshold (PHP manual, Garbage Collection:
/// "once the root buffer is full ... 10,000 possible roots").
pub const DEFAULT_THRESHOLD: usize = 10_000;

const R_GC_BUFFERED: u8 = 1;

enum Root {
    Arr(Weak<ArrayData>),
    Ref(Weak<RefInner>),
    Obj(Weak<Object>),
}

thread_local! {
    static ROOTS: RefCell<Vec<Root>> = const { RefCell::new(Vec::new()) };
    static STATS: Cell<GcStats> = const { Cell::new(GcStats { runs: 0, collected: 0 }) };
}

/// Collector statistics (`gc_status()`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcStats {
    pub runs: usize,
    pub collected: usize,
}

#[inline]
fn push_root(r: Root) {
    let _ = ROOTS.try_with(|roots| {
        if let Ok(mut roots) = roots.try_borrow_mut() {
            roots.push(r);
        }
    });
}

#[inline]
pub(crate) fn possible_root_array(rc: &Rc<ArrayData>) {
    let f = rc.flags.get();
    if f & ARRAY_GC_BUFFERED == 0 {
        rc.flags.set(f | ARRAY_GC_BUFFERED);
        push_root(Root::Arr(Rc::downgrade(rc)));
    }
}

#[inline]
pub(crate) fn possible_root_ref(r: &PhpRef) {
    let f = r.0.flags.get();
    if f & R_GC_BUFFERED == 0 {
        r.0.flags.set(f | R_GC_BUFFERED);
        push_root(Root::Ref(r.downgrade()));
    }
}

#[inline]
pub(crate) fn possible_root_obj(o: &ObjRef) {
    let f = o.0.flags.get();
    if f & F_GC_BUFFERED == 0 {
        o.0.flags.set(f | F_GC_BUFFERED);
        push_root(Root::Obj(Rc::downgrade(&o.0)));
    }
}

/// Number of buffered possible roots.
pub fn root_count() -> usize {
    ROOTS.with(|r| r.borrow().len())
}

/// Whether the root buffer reached `threshold` (the engine then runs
/// [`collect_cycles`] if GC is enabled).
pub fn should_collect(threshold: usize) -> bool {
    root_count() >= threshold
}

/// Statistics since the thread started.
pub fn stats() -> GcStats {
    STATS.with(|s| s.get())
}

#[derive(Clone)]
enum Node {
    Arr(Rc<ArrayData>),
    Ref(Rc<RefInner>),
    Obj(Rc<Object>),
}

impl Node {
    fn addr(&self) -> usize {
        match self {
            Node::Arr(r) => Rc::as_ptr(r) as *const u8 as usize,
            Node::Ref(r) => Rc::as_ptr(r) as *const u8 as usize,
            Node::Obj(r) => Rc::as_ptr(r) as *const u8 as usize,
        }
    }
    fn strong(&self) -> usize {
        match self {
            Node::Arr(r) => Rc::strong_count(r),
            Node::Ref(r) => Rc::strong_count(r),
            Node::Obj(r) => Rc::strong_count(r),
        }
    }
}

fn node_of(v: &Value) -> Option<Node> {
    match v {
        Value::Array(a) => a
            .rc()
            .filter(|rc| !rc.is_literal())
            .map(|rc| Node::Arr(rc.clone())),
        Value::Ref(r) => Some(Node::Ref(r.0.clone())),
        Value::Object(o) => Some(Node::Obj(o.0.clone())),
        _ => None,
    }
}

struct Info {
    node: Node,
    internal: usize,
    /// Contents couldn't be inspected (borrowed): treat as alive.
    opaque: bool,
    /// An object's property table (not counted separately).
    props_table: bool,
    alive: bool,
    edges: Vec<u32>,
}

struct Graph {
    infos: Vec<Info>,
    map: HashMap<usize, u32>,
}

impl Graph {
    fn add(&mut self, n: Node, props_table: bool) -> (u32, bool) {
        let a = n.addr();
        if let Some(&i) = self.map.get(&a) {
            return (i, false);
        }
        let i = self.infos.len() as u32;
        self.infos.push(Info {
            node: n,
            internal: 0,
            opaque: false,
            props_table,
            alive: false,
            edges: Vec::new(),
        });
        self.map.insert(a, i);
        (i, true)
    }

    fn build(roots: Vec<Node>) -> Graph {
        let mut g = Graph {
            infos: Vec::new(),
            map: HashMap::new(),
        };
        let mut stack = Vec::new();
        for r in roots {
            let (i, new) = g.add(r, false);
            if new {
                stack.push(i);
            }
        }
        while let Some(i) = stack.pop() {
            let mut children: Vec<(Node, bool)> = Vec::new();
            let mut opaque = false;
            match &g.infos[i as usize].node {
                Node::Arr(a) => {
                    a.for_each_value(|v| children.extend(node_of(v).map(|n| (n, false))))
                }
                Node::Ref(r) => match r.val.try_borrow() {
                    Ok(v) => children.extend(node_of(&v).map(|n| (n, false))),
                    Err(_) => opaque = true,
                },
                Node::Obj(o) => {
                    match o.slots.try_borrow() {
                        Ok(s) => children.extend(s.iter().filter_map(node_of).map(|n| (n, false))),
                        Err(_) => opaque = true,
                    }
                    match o.props.try_borrow() {
                        Ok(p) => {
                            if let Some(rc) = p.rc() {
                                children.push((Node::Arr(rc.clone()), true));
                            }
                        }
                        Err(_) => opaque = true,
                    }
                    match o.native.try_borrow() {
                        Ok(n) => {
                            if let Some(n) = &*n {
                                n.trace(&mut |v| children.extend(node_of(v).map(|n| (n, false))));
                            }
                        }
                        Err(_) => opaque = true,
                    }
                }
            }
            g.infos[i as usize].opaque = opaque;
            for (c, props) in children {
                let (j, new) = g.add(c, props);
                g.infos[j as usize].internal += 1;
                g.infos[i as usize].edges.push(j);
                if new {
                    stack.push(j);
                }
            }
        }
        g
    }

    /// Marks nodes reachable from outside the subgraph.
    fn mark_alive(&mut self) {
        let mut stack: Vec<u32> = Vec::new();
        for (i, info) in self.infos.iter_mut().enumerate() {
            // One strong count is our own clone in `node`.
            let external = info.node.strong() - 1 > info.internal;
            if external || info.opaque {
                info.alive = true;
                stack.push(i as u32);
            }
        }
        while let Some(i) = stack.pop() {
            let edges = std::mem::take(&mut self.infos[i as usize].edges);
            for &j in &edges {
                let inf = &mut self.infos[j as usize];
                if !inf.alive {
                    inf.alive = true;
                    stack.push(j);
                }
            }
            self.infos[i as usize].edges = edges;
        }
    }
}

fn take_roots() -> Vec<Node> {
    let roots = ROOTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let mut out = Vec::with_capacity(roots.len());
    for r in roots {
        match r {
            Root::Arr(w) => {
                if let Some(rc) = w.upgrade() {
                    rc.flags.set(rc.flags.get() & !ARRAY_GC_BUFFERED);
                    out.push(Node::Arr(rc));
                }
            }
            Root::Ref(w) => {
                if let Some(rc) = w.upgrade() {
                    rc.flags.set(rc.flags.get() & !R_GC_BUFFERED);
                    out.push(Node::Ref(rc));
                }
            }
            Root::Obj(w) => {
                if let Some(rc) = w.upgrade() {
                    rc.flags.set(rc.flags.get() & !F_GC_BUFFERED);
                    out.push(Node::Obj(rc));
                }
            }
        }
    }
    out
}

/// Collects garbage cycles reachable from the buffered roots
/// (`gc_collect_cycles()`), returning the number of freed arrays and
/// objects.
///
/// `run_destructor` is called for each garbage object whose destructor has
/// not run yet (the destructor is marked as called first). After running
/// destructors the scan is repeated, so objects they resurrect survive.
pub fn collect_cycles(run_destructor: &mut dyn FnMut(ObjRef)) -> usize {
    let mut extra_roots: Vec<Node> = Vec::new();
    let mut total = 0;
    for _round in 0..8 {
        let mut roots = take_roots();
        roots.append(&mut extra_roots);
        if roots.is_empty() {
            break;
        }
        let mut g = Graph::build(roots);
        g.mark_alive();
        let garbage: Vec<usize> = (0..g.infos.len()).filter(|&i| !g.infos[i].alive).collect();
        if garbage.is_empty() {
            break;
        }
        // Destructors first.
        let mut dtors = Vec::new();
        for &i in &garbage {
            if let Node::Obj(o) = &g.infos[i].node {
                let f = o.flags.get();
                if f & (F_HAS_DTOR | F_DTOR_CALLED) == F_HAS_DTOR {
                    o.flags.set(f | F_DTOR_CALLED);
                    dtors.push(ObjRef(o.clone()));
                }
            }
        }
        if !dtors.is_empty() {
            extra_roots = garbage.iter().map(|&i| g.infos[i].node.clone()).collect();
            drop(g);
            for o in dtors {
                run_destructor(o);
            }
            continue;
        }
        // Free: clear references and objects, which breaks every cycle.
        let mut trash: Vec<Value> = Vec::new();
        let mut count = 0;
        for &i in &garbage {
            let info = &g.infos[i];
            match &info.node {
                Node::Ref(r) => {
                    if let Ok(mut v) = r.val.try_borrow_mut() {
                        trash.push(std::mem::take(&mut *v));
                    }
                }
                Node::Obj(o) => {
                    count += 1;
                    o.flags.set(o.flags.get() | F_GC_CLEARED);
                    if let Ok(mut s) = o.slots.try_borrow_mut() {
                        trash.extend(std::mem::take(&mut *s));
                    }
                    if let Ok(mut p) = o.props.try_borrow_mut() {
                        trash.push(Value::Array(std::mem::take(&mut *p)));
                    }
                    if let Ok(mut n) = o.native.try_borrow_mut()
                        && let Some(n) = n.as_mut()
                    {
                        n.clear();
                    }
                }
                Node::Arr(_) => {
                    if !info.props_table {
                        count += 1;
                    }
                }
            }
        }
        drop(trash);
        drop(g);
        total += count;
        break;
    }
    STATS.with(|s| {
        let mut st = s.get();
        st.runs += 1;
        st.collected += total;
        s.set(st);
    });
    total
}
