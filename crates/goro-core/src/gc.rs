//! Cycle collector (gc_collect_cycles).
//!
//! Values are reference counted with `Rc`, so cycles (`$o->self = $o`,
//! `$a[] = &$a`) leak unless something breaks them. This module implements a
//! synchronous trial-deletion collector in the spirit of Zend's `zend_gc.c`:
//!
//! 1. Candidate roots: every object that has flowed through the VM (tracked
//!    once, weakly) plus arrays/references recorded as "possible roots" when a
//!    container holding them is dropped or a variable holding them is unset.
//! 2. The subgraph reachable from the candidates is built; for each node the
//!    number of strong references coming from inside the subgraph is counted.
//! 3. Nodes whose `Rc::strong_count` exceeds the internal count are externally
//!    referenced; they and everything reachable from them are live.
//! 4. The remaining nodes are garbage: only reachable from each other. Their
//!    contents are cleared, which breaks the cycles and lets `Rc` free them.
//!
//! Safety argument: an edge is only counted when we actually hold the `Value`
//! that owns the strong count, so internal counts are never over-estimated.
//! Anything we can't see into (generators, borrowed cells, side tables) only
//! makes nodes look externally referenced, i.e. keeps them alive.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::array::PhpArray;
use crate::object::PhpObject;
use crate::value::Value;

enum WeakNode {
    Obj(Weak<RefCell<PhpObject>>),
    Arr(Weak<RefCell<PhpArray>>),
    Ref(Weak<RefCell<Value>>),
}

impl WeakNode {
    fn alive(&self) -> bool {
        match self {
            WeakNode::Obj(w) => w.strong_count() > 0,
            WeakNode::Arr(w) => w.strong_count() > 0,
            WeakNode::Ref(w) => w.strong_count() > 0,
        }
    }
    fn ptr(&self) -> usize {
        match self {
            WeakNode::Obj(w) => w.as_ptr() as *const u8 as usize,
            WeakNode::Arr(w) => w.as_ptr() as *const u8 as usize,
            WeakNode::Ref(w) => w.as_ptr() as *const u8 as usize,
        }
    }
}

/// Root buffer. Entries are unique: a buffered Weak keeps the allocation
/// alive, so a pointer in `set` can't be reused by another node meanwhile.
#[derive(Default)]
struct RootBuf {
    list: Vec<WeakNode>,
    set: std::collections::HashSet<usize>,
}

thread_local! {
    /// Objects seen by the VM (each registered once).
    static TRACKED_OBJECTS: RefCell<Vec<Weak<RefCell<PhpObject>>>> = const { RefCell::new(Vec::new()) };
    static PRUNE_AT: Cell<usize> = const { Cell::new(4096) };
    /// Transient possible roots (arrays / references that lost a holder).
    static POSSIBLE_ROOTS: RefCell<RootBuf> = RefCell::new(RootBuf::default());
    static GC_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static GC_RUNS: Cell<i64> = const { Cell::new(0) };
    static GC_COLLECTED: Cell<i64> = const { Cell::new(0) };
    static GC_THRESHOLD: Cell<usize> = const { Cell::new(10000) };
    static GC_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Register an object as a permanent cycle-collection candidate.
#[inline]
pub fn track_object(rc: &Rc<RefCell<PhpObject>>) {
    if let Ok(b) = rc.try_borrow() {
        if b.gc_tracked.get() {
            return;
        }
        b.gc_tracked.set(true);
    } else {
        return;
    }
    let _ = TRACKED_OBJECTS.try_with(|t| {
        let mut t = t.borrow_mut();
        t.push(Rc::downgrade(rc));
        let limit = PRUNE_AT.with(|p| p.get());
        if t.len() >= limit {
            t.retain(|w| w.strong_count() > 0);
            PRUNE_AT.with(|p| p.set((t.len() * 2).max(4096)));
        }
    });
}

/// Record `v` as a possible cycle root: it is about to lose one holder while
/// others remain. Cheap no-op for scalars.
#[inline]
pub fn possible_root(v: &Value) {
    match v {
        Value::Array(a) => {
            if Rc::strong_count(a) > 1 {
                push_root(WeakNode::Arr(Rc::downgrade(a)));
            }
        }
        Value::Reference(r) => {
            if Rc::strong_count(r) > 1 {
                push_root(WeakNode::Ref(Rc::downgrade(r)));
            } else if let Ok(inner) = r.try_borrow() {
                // The reference itself dies: its referent loses a holder.
                if matches!(&*inner, Value::Array(_) | Value::Reference(_)) {
                    possible_root(&inner);
                }
            }
        }
        Value::Object(o) => {
            if Rc::strong_count(o) > 1 {
                track_object(o);
                push_root(WeakNode::Obj(Rc::downgrade(o)));
            }
        }
        _ => {}
    }
}

/// True when the possible-root buffer reached the collection threshold.
#[inline]
pub fn gc_pending() -> bool {
    GC_PENDING.with(|p| p.get())
}

fn prune_roots() {
    POSSIBLE_ROOTS.with(|r| {
        if let Ok(mut r) = r.try_borrow_mut() {
            let RootBuf { list, set } = &mut *r;
            list.retain(|n| {
                let keep = n.alive();
                if !keep {
                    set.remove(&n.ptr());
                }
                keep
            });
        }
    });
}

/// Automatic collection when the root buffer is full (zend_gc's
/// gc_possible_root_when_full). Only the first `threshold` roots are
/// processed, then the threshold adapts like GC_THRESHOLD_STEP does.
pub fn auto_collect<H: GcHost>(host: &mut H, enabled: bool) {
    GC_PENDING.with(|p| p.set(false));
    if !enabled || gc_active() {
        prune_roots();
        return;
    }
    let th = GC_THRESHOLD.with(|t| t.get());
    let count = collect_cycles(host, Some(th));
    let remaining = gc_root_count() as usize;
    GC_THRESHOLD.with(|t| {
        let cur = t.get();
        if count < 100 || remaining >= cur {
            t.set((cur + 10000).min(1_000_000_000));
        } else if cur > 10000 {
            t.set((cur - 10000).max(10000));
        }
    });
}

fn push_root(n: WeakNode) {
    let _ = POSSIBLE_ROOTS.try_with(|r| {
        if let Ok(mut r) = r.try_borrow_mut() {
            if !r.set.insert(n.ptr()) {
                return;
            }
            r.list.push(n);
            if r.list.len() >= GC_THRESHOLD.with(|t| t.get()) {
                GC_PENDING.with(|p| p.set(true));
            }
        }
    });
}

/// Called from container drops (PhpArray / PhpObject) for each child value.
#[inline]
pub fn on_child_released(v: &Value) {
    match v {
        Value::Array(_) | Value::Reference(_) | Value::Object(_) => possible_root(v),
        _ => {}
    }
}

pub fn gc_runs() -> i64 {
    GC_RUNS.with(|c| c.get())
}
pub fn gc_collected() -> i64 {
    GC_COLLECTED.with(|c| c.get())
}
pub fn gc_root_count() -> i64 {
    POSSIBLE_ROOTS.with(|r| r.borrow().list.len()) as i64
}
pub fn gc_active() -> bool {
    GC_ACTIVE.with(|c| c.get())
}

#[derive(Clone)]
enum Node {
    Obj(Rc<RefCell<PhpObject>>),
    Arr(Rc<RefCell<PhpArray>>),
    Ref(Rc<RefCell<Value>>),
}

impl Node {
    fn ptr(&self) -> usize {
        match self {
            Node::Obj(o) => Rc::as_ptr(o) as *const u8 as usize,
            Node::Arr(a) => Rc::as_ptr(a) as *const u8 as usize,
            Node::Ref(r) => Rc::as_ptr(r) as *const u8 as usize,
        }
    }
    fn strong(&self) -> usize {
        match self {
            Node::Obj(o) => Rc::strong_count(o),
            Node::Arr(a) => Rc::strong_count(a),
            Node::Ref(r) => Rc::strong_count(r),
        }
    }
    fn of(v: &Value) -> Option<Node> {
        match v {
            Value::Object(o) => Some(Node::Obj(o.clone())),
            Value::Array(a) => Some(Node::Arr(a.clone())),
            Value::Reference(r) => Some(Node::Ref(r.clone())),
            _ => None,
        }
    }
    fn ptr_of(v: &Value) -> Option<usize> {
        match v {
            Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
            Value::Array(a) => Some(Rc::as_ptr(a) as *const u8 as usize),
            Value::Reference(r) => Some(Rc::as_ptr(r) as *const u8 as usize),
            _ => None,
        }
    }
}

struct Graph {
    nodes: Vec<Node>,
    index: HashMap<usize, usize>,
    children: Vec<Vec<usize>>,
    internal: Vec<usize>,
    opaque: Vec<bool>,
    /// WeakMap entries: key object ptr -> values (ephemeron edges key -> value)
    ephemerons: HashMap<usize, Vec<Value>>,
}

impl Graph {
    fn new() -> Self {
        Graph { nodes: Vec::new(), index: HashMap::new(), children: Vec::new(), internal: Vec::new(), opaque: Vec::new(), ephemerons: HashMap::new() }
    }
    fn add(&mut self, n: Node) -> usize {
        let p = n.ptr();
        if let Some(&i) = self.index.get(&p) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(n);
        self.index.insert(p, i);
        self.children.push(Vec::new());
        self.internal.push(0);
        self.opaque.push(false);
        i
    }
    fn add_value(&mut self, v: &Value) -> Option<usize> {
        let p = Node::ptr_of(v)?;
        if let Some(&i) = self.index.get(&p) {
            return Some(i);
        }
        Node::of(v).map(|n| self.add(n))
    }
    /// Expand node `i`: enumerate the values it owns.
    fn expand(&mut self, i: usize) {
        let node = self.nodes[i].clone();
        let mut kids: Vec<usize> = Vec::new();
        let ok = match &node {
            Node::Obj(o) => match o.try_borrow() {
                Ok(b) => {
                    for (_, v) in b.properties.iter() {
                        if let Some(c) = self.add_value(v) {
                            kids.push(c);
                        }
                    }
                    // WeakMap values keyed by this object are reachable
                    // through it (and only through it).
                    if let Some(vals) = self.ephemerons.remove(&(Rc::as_ptr(o) as *const u8 as usize)) {
                        for v in &vals {
                            if let Some(c) = self.add_value(v) {
                                kids.push(c);
                            }
                        }
                        self.ephemerons.insert(Rc::as_ptr(o) as *const u8 as usize, vals);
                    }
                    true
                }
                Err(_) => false,
            },
            Node::Arr(a) => match a.try_borrow() {
                Ok(b) => {
                    for v in b.values() {
                        if let Some(c) = self.add_value(v) {
                            kids.push(c);
                        }
                    }
                    true
                }
                Err(_) => false,
            },
            Node::Ref(r) => match r.try_borrow() {
                Ok(b) => {
                    if let Some(c) = self.add_value(&b) {
                        kids.push(c);
                    }
                    true
                }
                Err(_) => false,
            },
        };
        drop(node);
        if !ok {
            self.opaque[i] = true;
            return;
        }
        for &c in &kids {
            self.internal[c] += 1;
        }
        self.children[i] = kids;
    }
}

/// Result of one analysis pass.
struct Analysis {
    graph: Graph,
    garbage: Vec<bool>,
}

fn analyze(extra_refs: &HashMap<usize, usize>, ephemerons: HashMap<usize, Vec<Value>>, limit: Option<usize>) -> Analysis {
    let mut g = Graph::new();
    g.ephemerons = ephemerons;
    // Seed with candidates.
    let objs: Vec<Rc<RefCell<PhpObject>>> = TRACKED_OBJECTS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|w| w.strong_count() > 0);
        PRUNE_AT.with(|p| p.set((t.len() * 2).max(4096)));
        if limit.is_some() { return Vec::new(); }
        t.iter().filter_map(|w| w.upgrade()).collect()
    });
    // Automatic runs only look at the root buffer (like Zend); explicit
    // gc_collect_cycles() also scans every tracked object.
    for o in objs {
        g.add(Node::Obj(o));
    }
    let roots: Vec<WeakNode> = POSSIBLE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        let taken: Vec<WeakNode> = match limit {
            Some(n) if n < r.list.len() => r.list.drain(..n).collect(),
            _ => std::mem::take(&mut r.list),
        };
        if r.list.is_empty() {
            r.set.clear();
        } else {
            for t in &taken {
                r.set.remove(&t.ptr());
            }
        }
        taken
    });
    for r in roots {
        let n = match r {
            WeakNode::Obj(w) => w.upgrade().map(Node::Obj),
            WeakNode::Arr(w) => w.upgrade().map(Node::Arr),
            WeakNode::Ref(w) => w.upgrade().map(Node::Ref),
        };
        if let Some(n) = n {
            g.add(n);
        }
    }
    let mut i = 0;
    while i < g.nodes.len() {
        g.expand(i);
        i += 1;
    }
    // Register objects discovered during traversal as permanent candidates.
    for n in &g.nodes {
        if let Node::Obj(o) = n {
            track_object(o);
        }
    }
    let len = g.nodes.len();
    let mut live = vec![false; len];
    let mut stack: Vec<usize> = Vec::new();
    for idx in 0..len {
        let n = &g.nodes[idx];
        // One strong count is our own clone in `g.nodes`.
        let accounted = 1 + g.internal[idx] + extra_refs.get(&n.ptr()).copied().unwrap_or(0);
        if g.opaque[idx] || n.strong() != accounted {
            live[idx] = true;
            stack.push(idx);
        }
    }
    while let Some(idx) = stack.pop() {
        for &c in &g.children[idx] {
            if !live[c] {
                live[c] = true;
                stack.push(c);
            }
        }
    }
    if std::env::var_os("GORO_GC_DEBUG").is_some() {
        for idx in 0..len {
            let n = &g.nodes[idx];
            let kind = match n { Node::Obj(o) => format!("obj#{}", o.try_borrow().map(|b| b.object_id).unwrap_or(0)), Node::Arr(_) => "arr".into(), Node::Ref(_) => "ref".into() };
            eprintln!("gc node {} {} strong={} internal={} extra={} opaque={} live={} kids={:?}", idx, kind, n.strong(), g.internal[idx], extra_refs.get(&n.ptr()).copied().unwrap_or(0), g.opaque[idx], live[idx], g.children[idx]);
        }
    }
    let garbage = live.iter().map(|l| !l).collect();
    Analysis { graph: g, garbage }
}

/// Hooks the collector needs from the VM.
pub trait GcHost {
    /// Strong references held by the VM's own bookkeeping (e.g. the list of
    /// destructible objects), keyed by Rc pointer.
    fn gc_extra_refs(&self) -> HashMap<usize, usize>;
    /// Does this object still need its destructor called?
    fn gc_needs_destructor(&self, obj: &Rc<RefCell<PhpObject>>) -> bool;
    /// Call the destructor of a garbage object.
    fn gc_call_destructor(&mut self, obj: &Rc<RefCell<PhpObject>>);
    /// Forget VM bookkeeping for objects about to be freed.
    fn gc_forget_objects(&mut self, ptrs: &std::collections::HashSet<usize>);
    /// WeakMap entries as (key object ptr, value clone).
    fn gc_ephemerons(&self) -> Vec<(usize, Value)> {
        Vec::new()
    }
}


/// Run a collection. Returns the number of freed arrays/objects.
pub fn collect_cycles<H: GcHost>(host: &mut H, limit: Option<usize>) -> i64 {
    if GC_ACTIVE.with(|c| c.get()) {
        return 0;
    }
    GC_ACTIVE.with(|c| c.set(true));
    let mut total = 0i64;
    let mut did_rerun = false;
    loop {
        let had_roots = gc_root_count() > 0;
        let mut extra = host.gc_extra_refs();
        // WeakMap values: each entry's strong ref is modelled as an edge from
        // its key; our clone of the value is accounted as an extra ref.
        let mut eph: HashMap<usize, Vec<Value>> = HashMap::new();
        for (kptr, v) in host.gc_ephemerons() {
            if let Some(p) = Node::ptr_of(&v) {
                *extra.entry(p).or_insert(0) += 1;
            }
            eph.entry(kptr).or_default().push(v);
        }
        let an = analyze(&extra, eph, limit);
        let len = an.graph.nodes.len();
        if had_roots || an.garbage.iter().any(|g| *g) {
            GC_RUNS.with(|c| c.set(c.get() + 1));
        }
        // Garbage objects that still need their destructor called.
        let mut dtor_idx: Vec<usize> = Vec::new();
        for idx in 0..len {
            if an.garbage[idx] {
                if let Node::Obj(o) = &an.graph.nodes[idx] {
                    if host.gc_needs_destructor(o) {
                        dtor_idx.push(idx);
                    }
                }
            }
        }
        let mut free = an.garbage.clone();
        if !dtor_idx.is_empty() {
            // Everything reachable from an object with a pending destructor
            // stays alive for now: the destructor may resurrect it.
            let mut stack = dtor_idx.clone();
            for &d in &dtor_idx {
                free[d] = false;
            }
            while let Some(idx) = stack.pop() {
                for &c in &an.graph.children[idx] {
                    if free[c] {
                        free[c] = false;
                        stack.push(c);
                    }
                }
            }
        }
        let dtor_objs: Vec<Rc<RefCell<PhpObject>>> = dtor_idx
            .iter()
            .filter_map(|&i| match &an.graph.nodes[i] {
                Node::Obj(o) => Some(o.clone()),
                _ => None,
            })
            .collect();
        let mut ptrs = std::collections::HashSet::new();
        for idx in 0..len {
            if free[idx] {
                ptrs.insert(an.graph.nodes[idx].ptr());
            }
        }
        let mut count = 0i64;
        let mut dead_props: Vec<Vec<(Vec<u8>, Value)>> = Vec::new();
        let mut dead_arrays: Vec<PhpArray> = Vec::new();
        let mut dead_vals: Vec<Value> = Vec::new();
        if !ptrs.is_empty() {
            host.gc_forget_objects(&ptrs);
            for idx in 0..len {
                if !free[idx] {
                    continue;
                }
                match &an.graph.nodes[idx] {
                    Node::Obj(o) => {
                        if let Ok(mut b) = o.try_borrow_mut() {
                            dead_props.push(std::mem::take(&mut b.properties));
                            count += 1;
                        }
                    }
                    Node::Arr(a) => {
                        if let Ok(mut b) = a.try_borrow_mut() {
                            dead_arrays.push(std::mem::replace(&mut *b, PhpArray::new()));
                            count += 1;
                        }
                    }
                    Node::Ref(r) => {
                        if let Ok(mut b) = r.try_borrow_mut() {
                            dead_vals.push(std::mem::replace(&mut *b, Value::Null));
                        }
                    }
                }
            }
        }
        // Release our clones first, then the detached contents: this is
        // where the cycles actually get freed.
        drop(an);
        drop(dead_vals);
        drop(dead_arrays);
        drop(dead_props);
        total += count;
        if dtor_objs.is_empty() {
            break;
        }
        for o in &dtor_objs {
            host.gc_call_destructor(o);
        }
        // Objects whose destructor ran stay possible roots for the rerun.
        for o in &dtor_objs {
            push_root(WeakNode::Obj(Rc::downgrade(o)));
        }
        drop(dtor_objs);
        if did_rerun {
            break;
        }
        did_rerun = true;
    }
    // Freeing pushes the dying children as roots; drop the dead entries.
    prune_roots();
    GC_COLLECTED.with(|c| c.set(c.get() + total));
    GC_ACTIVE.with(|c| c.set(false));
    total
}

impl GcHost for crate::vm::Vm {
    fn gc_extra_refs(&self) -> HashMap<usize, usize> {
        let mut m = HashMap::new();
        for v in &self.destructible_objects {
            if let Value::Object(o) = v {
                *m.entry(Rc::as_ptr(o) as *const u8 as usize).or_insert(0) += 1;
            }
        }
        m
    }
    fn gc_needs_destructor(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let Ok(b) = obj.try_borrow() else { return false };
        if matches!(b.get_property(b"__destructed"), Value::True) {
            return false;
        }
        let lower: Vec<u8> = b.class_name.iter().map(|c| c.to_ascii_lowercase()).collect();
        self.classes.get(&lower).and_then(|c| c.get_method(b"__destruct")).is_some()
    }
    fn gc_call_destructor(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        if self.current_exception.is_some() {
            return;
        }
        self.gc_run_destructor(obj);
    }
    fn gc_forget_objects(&mut self, ptrs: &std::collections::HashSet<usize>) {
        self.destructible_objects.retain(|v| match v {
            Value::Object(o) => !ptrs.contains(&(Rc::as_ptr(o) as *const u8 as usize)),
            _ => true,
        });
        // Entries whose key dies are dropped from their WeakMap.
        let mut dropped: Vec<Value> = Vec::new();
        for entries in self.weak_map_entries.values_mut() {
            entries.retain_mut(|(_, w, v)| {
                if ptrs.contains(&(w.as_ptr() as *const u8 as usize)) {
                    dropped.push(std::mem::replace(v, Value::Null));
                    false
                } else {
                    true
                }
            });
        }
        drop(dropped);
    }
    fn gc_ephemerons(&self) -> Vec<(usize, Value)> {
        let mut out = Vec::new();
        for entries in self.weak_map_entries.values() {
            for (_, w, v) in entries {
                if w.strong_count() > 0 && matches!(v, Value::Object(_) | Value::Array(_) | Value::Reference(_)) {
                    out.push((w.as_ptr() as *const u8 as usize, v.clone()));
                }
            }
        }
        out
    }
}
