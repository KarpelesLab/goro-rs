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
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::{Rc, Weak};

use crate::array::PhpArray;
use crate::generator::PhpGenerator;
use crate::object::PhpObject;
use crate::value::Value;

/// Hasher for allocation addresses (Fibonacci hashing; SipHash is needlessly
/// slow for the collector's pointer-keyed tables).
#[derive(Default)]
pub struct PtrHasher(u64);
impl Hasher for PtrHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (n as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .rotate_right(29);
    }
    fn write_u64(&mut self, n: u64) {
        self.write_usize(n as usize);
    }
}
type PtrBuild = BuildHasherDefault<PtrHasher>;
/// Map keyed by allocation address.
pub type PtrMap<V> = HashMap<usize, V, PtrBuild>;
/// Set of allocation addresses.
pub type PtrSet = std::collections::HashSet<usize, PtrBuild>;

/// A buffered possible root. Like Zend, only arrays and objects are roots;
/// a reference losing a holder buffers its referent instead.
enum WeakNode {
    Obj(Weak<RefCell<PhpObject>>),
    Arr(Weak<RefCell<PhpArray>>),
}

impl WeakNode {
    /// Take the entry out of the buffer: clear the value's buffered slot.
    /// Returns the live node, if any.
    fn unbuffer(self) -> Option<Node> {
        let n = match self {
            WeakNode::Obj(w) => {
                let o = w.upgrade()?;
                if let Ok(b) = o.try_borrow() {
                    if b.gc_buffered.get() {
                        b.gc_buffered.set_slot(0);
                        live_dec();
                    }
                }
                Node::Obj(o)
            }
            WeakNode::Arr(w) => {
                let a = w.upgrade()?;
                if let Ok(b) = a.try_borrow() {
                    if b.gc_buffered.get() {
                        b.gc_buffered.set_slot(0);
                        live_dec();
                    }
                }
                Node::Arr(a)
            }
        };
        Some(n)
    }
    /// The value moved to slot `slot` (it is still buffered).
    fn move_to(&self, slot: u32) -> bool {
        match self {
            WeakNode::Obj(w) => w.upgrade().is_some_and(|o| {
                o.try_borrow().is_ok_and(|b| {
                    b.gc_buffered.set_slot(slot);
                    true
                })
            }),
            WeakNode::Arr(w) => w.upgrade().is_some_and(|a| {
                a.try_borrow().is_ok_and(|b| {
                    b.gc_buffered.set_slot(slot);
                    true
                })
            }),
        }
    }
}

/// Zend's GC_THRESHOLD_DEFAULT (10000 roots + the reserved first slot).
const THRESHOLD_DEFAULT: usize = 10001;
const THRESHOLD_STEP: usize = 10000;
const THRESHOLD_MAX: usize = 1_000_000_000;
const THRESHOLD_TRIGGER: i64 = 100;
const BUF_DEFAULT: usize = 16 * 1024;
const BUF_GROW_STEP: usize = 128 * 1024;
const BUF_MAX: usize = 0x4000_0000;

/// Root buffer, laid out like Zend's: a value is buffered at most once, in
/// a slot it records (its `gc_buffered` slot, Zend's GC_INFO address); a
/// buffered value that is freed gives its slot back from its Drop
/// (GC_REMOVE_FROM_BUFFER), and new roots take the most recently freed
/// slot first (GC_FETCH_UNUSED), else the next one. The collector visits
/// the roots in slot order, which decides the order destructors run in.
struct RootBuf {
    /// Slot n is `slots[n - 1]` (Zend's slot 0 is reserved).
    slots: Vec<Option<WeakNode>>,
    /// Free slots below `slots.len()`, most recently freed last.
    unused: Vec<u32>,
}

impl RootBuf {
    /// Apply the slot releases that could not be done at once.
    fn flush_freed(&mut self) {
        let freed = FREED_SLOTS.with(|f| std::mem::take(&mut *f.borrow_mut()));
        for s in freed {
            self.release(s);
        }
    }
    fn release(&mut self, slot: u32) {
        if let Some(e) = self.slots.get_mut(slot as usize - 1) {
            if e.take().is_some() {
                self.unused.push(slot);
            }
        }
    }
}

thread_local! {
    /// Generators created by the VM (candidates like objects).
    static TRACKED_GENERATORS: RefCell<Vec<Weak<RefCell<PhpGenerator>>>> = const { RefCell::new(Vec::new()) };
    /// Objects seen by the VM (each registered once).
    static TRACKED_OBJECTS: RefCell<Vec<Weak<RefCell<PhpObject>>>> = const { RefCell::new(Vec::new()) };
    static PRUNE_AT: Cell<usize> = const { Cell::new(4096) };
    /// Possible roots (arrays / objects that lost a holder).
    static POSSIBLE_ROOTS: RefCell<RootBuf> = const { RefCell::new(RootBuf {
        slots: Vec::new(),
        unused: Vec::new(),
    }) };
    /// Slots freed while the buffer was in use (see RootBuf::flush_freed).
    static FREED_SLOTS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    /// Number of buffered values still alive (Zend's num_roots).
    static LIVE_ROOTS: Cell<usize> = const { Cell::new(0) };
    /// Objects whose last reference went away while the collector was
    /// running their destructor. Zend leaves such an object in the root
    /// buffer with refcount 0 (GC_DELREF after dtor_obj) and the next run
    /// frees it as garbage, counting it; we hold the last strong ref here
    /// (the object is also buffered, so it counts as a root).
    static ZOMBIES: RefCell<Vec<Rc<RefCell<PhpObject>>>> = const { RefCell::new(Vec::new()) };
    static GC_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static GC_RUNS: Cell<i64> = const { Cell::new(0) };
    static GC_COLLECTED: Cell<i64> = const { Cell::new(0) };
    static GC_THRESHOLD: Cell<usize> = const { Cell::new(THRESHOLD_DEFAULT) };
    static GC_BUF_SIZE: Cell<usize> = const { Cell::new(BUF_DEFAULT) };
    static GC_FULL: Cell<bool> = const { Cell::new(false) };
    /// Set when a root was buffered while the buffer was at the threshold;
    /// the VM runs the collection at its next safe point.
    static GC_PENDING: Cell<bool> = const { Cell::new(false) };
    /// Objects with a pending destructor that just lost a holder, in release
    /// order (see `note_dtor_candidate`).
    static DTOR_CANDIDATES: RefCell<std::collections::VecDeque<Weak<RefCell<PhpObject>>>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
    /// The buffer is full but collecting is disabled (gc_disable()): stop
    /// raising GC_PENDING until something changes.
    static GC_SUPPRESSED: Cell<bool> = const { Cell::new(false) };
    /// Number of slots in use when the threshold was hit (those hold the
    /// roots Zend's synchronous collection would have processed).
    static GC_PENDING_ROOTS: Cell<usize> = const { Cell::new(0) };
    /// Accumulated times in nanoseconds: collector, destructors, freeing.
    static GC_TIMES: Cell<[u64; 3]> = const { Cell::new([0; 3]) };
    static GC_EPOCH: std::time::Instant = std::time::Instant::now();
}

/// get_gc of a native object: report each value its native data holds a
/// strong reference to, exactly once (a value reported but not owned would
/// make live data look like garbage; report nothing when unsure).
pub type NativeGcFn = fn(&Rc<dyn std::any::Any>, &mut dyn FnMut(&Value));

thread_local! {
    static NATIVE_GC: RefCell<Vec<(usize, NativeGcFn)>> = const { RefCell::new(Vec::new()) };
}

/// Register the get_gc handler of the objects using `handlers`. Garbage
/// objects with one have their native data released (free_obj).
pub fn register_native_gc(handlers: &'static crate::vm::NativeObjectHandlers, f: NativeGcFn) {
    let key = handlers as *const _ as usize;
    NATIVE_GC.with(|g| {
        let mut g = g.borrow_mut();
        if !g.iter().any(|(k, _)| *k == key) {
            g.push((key, f));
        }
    });
}

fn native_gc_of(n: &crate::object::NativeData) -> Option<NativeGcFn> {
    let key = n.handlers as *const _ as usize;
    NATIVE_GC.with(|g| g.borrow().iter().find(|(k, _)| *k == key).map(|(_, f)| *f))
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

/// Register a generator as a cycle-collection candidate.
pub fn track_generator(rc: &Rc<RefCell<PhpGenerator>>) {
    match rc.try_borrow() {
        Ok(b) if !b.gc_tracked.get() => b.gc_tracked.set(true),
        _ => return,
    }
    let _ = TRACKED_GENERATORS.try_with(|t| {
        let mut t = t.borrow_mut();
        if t.len() >= 4096 && t.len().is_power_of_two() {
            t.retain(|w| w.strong_count() > 0);
        }
        t.push(Rc::downgrade(rc));
    });
}

/// Record `v` as a possible cycle root: it is about to lose one holder while
/// others remain (Zend's gc_check_possible_root). Cheap no-op for scalars.
#[inline]
pub fn possible_root(v: &Value) {
    match v {
        Value::Array(a) => {
            if Rc::strong_count(a) > 1 {
                buffer_array(a);
            }
        }
        Value::Reference(r) => {
            // Whether the reference survives or dies, its referent is what
            // Zend buffers (references are never roots themselves); when the
            // reference dies the referent loses a holder.
            let shared = Rc::strong_count(r) > 1;
            if let Ok(inner) = r.try_borrow() {
                match &*inner {
                    Value::Array(a) if shared || Rc::strong_count(a) > 1 => buffer_array(a),
                    Value::Object(o) if shared || Rc::strong_count(o) > 1 => {
                        if !shared {
                            note_dtor_candidate(o);
                        }
                        buffer_object(o)
                    }
                    _ => {}
                }
            }
        }
        Value::Object(o) => {
            if Rc::strong_count(o) > 1 {
                note_dtor_candidate(o);
                buffer_object(o);
            }
        }
        _ => {}
    }
}

/// A lazy object's initializer closure released when the object is
/// initialized or realized: Zend holds it twice (the zval and the fcc), so
/// dropping the zval roots it before the fcc frees it.
pub fn root_released_initializer(v: &Value) {
    if let Value::Object(o) = v {
        buffer_object(o);
    }
}

/// An object a call holds as its $this, released when the call returns
/// (rooted when the call is set up): a possible root unless it already is
/// one, or nothing else holds it.
#[inline]
pub fn possible_root_call_this(o: &Rc<RefCell<PhpObject>>) {
    if Rc::strong_count(o) > 1 && o.try_borrow().is_ok_and(|b| !b.gc_buffered.get()) {
        buffer_object(o);
    }
}

/// possible_root() of an object.
#[inline]
pub fn possible_root_object(o: &Rc<RefCell<PhpObject>>) {
    if Rc::strong_count(o) > 1 {
        note_dtor_candidate(o);
        buffer_object(o);
    }
}

/// `o` is losing one holder: when the only other one is the VM's list of
/// objects awaiting destruction, queue it so the next release point runs its
/// destructor (regardless of where it sits in that list).
#[inline]
pub fn note_dtor_candidate(o: &Rc<RefCell<PhpObject>>) {
    if Rc::strong_count(o) == 2 && o.try_borrow().map(|b| b.dtor_listed.get()).unwrap_or(false) {
        DTOR_CANDIDATES.with(|c| c.borrow_mut().push_back(Rc::downgrade(o)));
    }
}

/// Watch a value that is about to be overwritten or removed: returns a
/// weak handle on it when it is an object awaiting destruction.
#[inline]
pub fn dtor_watch(v: &Value) -> Option<Weak<RefCell<PhpObject>>> {
    match v {
        Value::Object(o) => {
            if o.try_borrow().map(|b| b.dtor_listed.get()).unwrap_or(false) {
                Some(Rc::downgrade(o))
            } else {
                None
            }
        }
        Value::Reference(r) => match r.try_borrow() {
            Ok(inner) => match &*inner {
                Value::Object(_) => dtor_watch(&inner),
                _ => None,
            },
            Err(_) => None,
        },
        _ => None,
    }
}

/// After the watched value was released: queue it as a destructor candidate
/// when the destructible list is now its only holder.
#[inline]
pub fn dtor_watch_end(w: Option<Weak<RefCell<PhpObject>>>) {
    if let Some(rc) = w.and_then(|w| w.upgrade()) {
        note_dtor_candidate(&rc);
    }
}

/// Next queued destructor candidate (oldest release first).
pub fn pop_dtor_candidate() -> Option<Weak<RefCell<PhpObject>>> {
    DTOR_CANDIDATES.with(|c| c.borrow_mut().pop_front())
}

/// A container slot's value `old` was overwritten with `new`. Turning a
/// value into a reference to itself (ZEND_MAKE_REF) moves it rather than
/// releasing it, so that is not a possible root.
#[inline]
pub fn on_value_replaced(old: &Value, new: &Value) {
    if !matches!(
        old,
        Value::Array(_) | Value::Reference(_) | Value::Object(_)
    ) {
        return;
    }
    if let Value::Reference(r) = new {
        if let Ok(inner) = r.try_borrow() {
            let same = match (old, &*inner) {
                (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
                (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
                _ => false,
            };
            if same {
                return;
            }
        }
    }
    possible_root(old);
}

/// A call frame is being torn down: its locals lose a holder
/// (i_free_compiled_variables).
#[inline]
pub fn release_frame(cvs: &[Value]) {
    for v in cvs {
        on_child_released(v);
    }
}

/// Called from container drops (PhpArray / PhpObject) for each child value.
#[inline]
pub fn on_child_released(v: &Value) {
    match v {
        Value::Array(_) | Value::Reference(_) | Value::Object(_) => possible_root(v),
        _ => {}
    }
}

#[inline]
fn live_dec() {
    let _ = LIVE_ROOTS.try_with(|c| c.set(c.get().saturating_sub(1)));
}

/// A buffered value (in `slot`) was freed: its slot is free again.
#[inline]
pub(crate) fn root_freed(slot: u32) {
    live_dec();
    let _ = POSSIBLE_ROOTS.try_with(|r| match r.try_borrow_mut() {
        Ok(mut r) => r.release(slot),
        Err(_) => FREED_SLOTS.with(|f| f.borrow_mut().push(slot)),
    });
}

#[inline]
fn buffer_object(o: &Rc<RefCell<PhpObject>>) {
    let Ok(b) = o.try_borrow() else {
        return;
    };
    if b.gc_buffered.get() {
        return;
    }
    if !b.gc_tracked.get() {
        drop(b);
        track_object(o);
        let Ok(b) = o.try_borrow() else {
            return;
        };
        if let Some(s) = add_root(WeakNode::Obj(Rc::downgrade(o))) {
            b.gc_buffered.set_slot(s);
        }
        return;
    }
    if let Some(s) = add_root(WeakNode::Obj(Rc::downgrade(o))) {
        b.gc_buffered.set_slot(s);
    }
}

#[inline]
fn buffer_array(a: &Rc<RefCell<PhpArray>>) {
    let Ok(b) = a.try_borrow() else {
        return;
    };
    if b.gc_buffered.get() {
        return;
    }
    if let Some(s) = add_root(WeakNode::Arr(Rc::downgrade(a))) {
        b.gc_buffered.set_slot(s);
    }
}

/// Buffer a new root (gc_possible_root). Returns its slot, or None if the
/// buffer is unavailable (re-entrant use or thread teardown).
fn add_root(n: WeakNode) -> Option<u32> {
    POSSIBLE_ROOTS
        .try_with(|r| {
            let Ok(mut r) = r.try_borrow_mut() else {
                return None;
            };
            r.flush_freed();
            LIVE_ROOTS.with(|c| c.set(c.get() + 1));
            if let Some(s) = r.unused.pop() {
                r.slots[s as usize - 1] = Some(n);
                return Some(s);
            }
            // (slot 0 is reserved: the next slot is len + 1)
            if r.slots.len() + 1 >= GC_THRESHOLD.with(|t| t.get())
                && !GC_PENDING.with(|p| p.get())
                && !GC_SUPPRESSED.with(|p| p.get())
            {
                // gc_possible_root_when_full: Zend collects right here, over
                // the roots buffered so far, then adds this one.
                GC_PENDING.with(|p| p.set(true));
                GC_PENDING_ROOTS.with(|p| p.set(r.slots.len()));
            }
            r.slots.push(Some(n));
            Some(r.slots.len() as u32)
        })
        .ok()
        .flatten()
}

/// Nothing a release point would do is waiting in the collector's state:
/// no destructor candidate, dead WeakMap entry, deferred object id or due
/// collection (see Vm::run_pending_destructors).
#[inline]
pub fn release_quiet() -> bool {
    !GC_PENDING.with(|p| p.get())
        && !WEAK_DIRTY.with(|d| d.get())
        && DTOR_CANDIDATES.with(|c| c.borrow().is_empty())
        && DEFERRED_IDS.with(|m| m.borrow().is_empty())
}

/// True when the possible-root buffer reached the collection threshold.
#[inline]
pub fn gc_pending() -> bool {
    GC_PENDING.with(|p| p.get())
}

/// The zend.enable_gc setting (an INI boolean; on when unset).
pub fn ini_enabled(v: Option<&Value>) -> bool {
    match v {
        None => true,
        Some(Value::String(s)) => {
            let l = s.to_string_lossy().trim().to_ascii_lowercase();
            matches!(l.as_str(), "on" | "yes" | "true")
                || l.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
        Some(v) => v.is_truthy(),
    }
}

/// gc_enable(): collecting may resume.
pub fn gc_enabled_changed() {
    GC_SUPPRESSED.with(|p| p.set(false));
}

/// Zend's gc_grow_root_buffer: grow the (virtual) root buffer.
fn grow_buffer() {
    let cur = GC_BUF_SIZE.with(|b| b.get());
    if cur >= BUF_MAX {
        GC_FULL.with(|f| f.set(true));
        return;
    }
    let new = if cur < BUF_GROW_STEP {
        cur * 2
    } else {
        cur + BUF_GROW_STEP
    };
    GC_BUF_SIZE.with(|b| b.set(new.min(BUF_MAX)));
}

/// Zend's gc_adjust_threshold after an automatic collection.
fn adjust_threshold(count: i64) {
    let cur = GC_THRESHOLD.with(|t| t.get());
    let roots = live_root_count();
    if count < THRESHOLD_TRIGGER || roots >= cur {
        if cur < THRESHOLD_MAX {
            let new = (cur + THRESHOLD_STEP).min(THRESHOLD_MAX);
            if new > GC_BUF_SIZE.with(|b| b.get()) {
                grow_buffer();
            }
            if new <= GC_BUF_SIZE.with(|b| b.get()) {
                GC_THRESHOLD.with(|t| t.set(new));
            }
        }
    } else if cur > THRESHOLD_DEFAULT {
        GC_THRESHOLD.with(|t| t.set((cur - THRESHOLD_STEP).max(THRESHOLD_DEFAULT)));
    }
}

/// Automatic collection when the root buffer is full (zend_gc's
/// gc_possible_root_when_full): collect the roots buffered at the time the
/// threshold was hit, then adapt the threshold like gc_adjust_threshold.
pub fn auto_collect<H: GcHost>(host: &mut H, enabled: bool) {
    GC_PENDING.with(|p| p.set(false));
    let n = GC_PENDING_ROOTS.with(|p| p.replace(0));
    if !enabled || gc_active() {
        // No collection: the buffer just keeps growing.
        if !enabled {
            GC_SUPPRESSED.with(|p| p.set(true));
        }
        return;
    }
    // Roots buffered after the threshold was hit would only have been added
    // after Zend's synchronous collection: keep them out of this run.
    let overflow: Vec<WeakNode> = POSSIBLE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        r.flush_freed();
        let cut = n.min(r.slots.len());
        let over = r.slots.split_off(cut);
        r.unused.retain(|&s| (s as usize) <= cut);
        over.into_iter().flatten().collect()
    });
    let count = collect_cycles(host, Some(usize::MAX));
    // (added after the collection, in the slots that follow its own roots)
    POSSIBLE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        for e in overflow {
            let slot = match r.unused.pop() {
                Some(s) => s,
                None => r.slots.len() as u32 + 1,
            };
            if e.move_to(slot) {
                if (slot as usize) > r.slots.len() {
                    r.slots.push(Some(e));
                } else {
                    r.slots[slot as usize - 1] = Some(e);
                }
            } else if (slot as usize) <= r.slots.len() {
                r.unused.push(slot);
            }
        }
    });
    adjust_threshold(count);
}

pub fn gc_runs() -> i64 {
    GC_RUNS.with(|c| c.get())
}
pub fn gc_collected() -> i64 {
    GC_COLLECTED.with(|c| c.get())
}
fn live_root_count() -> usize {
    LIVE_ROOTS.with(|c| c.get())
}
/// Number of buffered possible roots (gc_status()['roots']).
pub fn gc_root_count() -> i64 {
    live_root_count() as i64
}
pub fn gc_active() -> bool {
    GC_ACTIVE.with(|c| c.get())
}

thread_local! {
    /// A WeakMap key or a WeakMap died: its entries must be released.
    static WEAK_DIRTY: Cell<bool> = const { Cell::new(false) };
    /// Object ids of WeakMaps that died.
    static DEAD_WEAKMAPS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// Handles of dead weak targets, recycled once their entries are gone,
    /// with the destructor depth they died at.
    static DEFERRED_IDS: RefCell<Vec<(u64, u32)>> = const { RefCell::new(Vec::new()) };
    /// Number of destructors running (see flush_weak_ids).
    static DTOR_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// A destructor starts running (its own release points must not recycle
/// the handle of the weak target whose release called it).
pub fn dtor_enter() {
    DTOR_DEPTH.with(|d| d.set(d.get() + 1));
}

pub fn dtor_leave() {
    DTOR_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

/// Called from PhpObject's Drop for a WeakMap key (`is_map` false) or a
/// WeakMap (`is_map` true).
pub(crate) fn weak_target_freed(id: u64, is_map: bool) {
    let _ = WEAK_DIRTY.try_with(|d| d.set(true));
    if id != 0 {
        let depth = DTOR_DEPTH.try_with(|d| d.get()).unwrap_or(0);
        let _ = DEFERRED_IDS.try_with(|m| m.borrow_mut().push((id, depth)));
    }
    if is_map {
        let _ = DEAD_WEAKMAPS.try_with(|m| m.borrow_mut().push(id));
    }
}

#[inline]
pub fn weak_dirty() -> bool {
    WEAK_DIRTY.with(|d| d.get())
}

/// Release the WeakMap entries whose key or map died (zend_weakrefs_notify
/// / zend_weakmap_free_obj). Releasing values may free more keys or maps.
pub fn purge_weakmaps(vm: &mut crate::vm::Vm) {
    while WEAK_DIRTY.with(|d| d.replace(false)) {
        let mut dropped: Vec<Value> = Vec::new();
        for id in DEAD_WEAKMAPS.with(|m| std::mem::take(&mut *m.borrow_mut())) {
            if let Some(entries) = vm.weak_map_entries.remove(&id) {
                dropped.extend(entries.into_iter().map(|(_, _, v)| v));
            }
        }
        vm.weak_map_entries.retain(|_, entries| {
            entries.retain_mut(|(_, w, v)| {
                if !w.is_alive() {
                    dropped.push(std::mem::replace(v, Value::Null));
                    false
                } else {
                    true
                }
            });
            !entries.is_empty()
        });
        // Targets whose last reference was an entry released here are freed
        // inside the release of the dead key or map, so their handles go
        // back to the free list before its own (zend_objects_store_del).
        let parents = DEFERRED_IDS.with(|m| m.borrow().len());
        drop(dropped);
        DEFERRED_IDS.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() > parents {
                let children: Vec<(u64, u32)> = m.drain(parents..).collect();
                m.splice(0..0, children);
            }
        });
    }
}

/// Recycle the handles of dead weak targets whose entries were released
/// (after the destructors this triggered ran, as in Zend where those run
/// nested inside the key's own release).
#[inline]
pub fn flush_weak_ids() {
    if DEFERRED_IDS.with(|m| m.borrow().is_empty()) || weak_dirty() {
        return;
    }
    flush_weak_ids_slow();
}

#[inline(never)]
fn flush_weak_ids_slow() {
    // (not from inside the destructors a target's release called)
    let depth = DTOR_DEPTH.with(|d| d.get());
    let ids: Vec<u64> = DEFERRED_IDS.with(|m| {
        let mut m = m.borrow_mut();
        let mut out = Vec::new();
        m.retain(|&(id, d)| {
            if d >= depth {
                out.push(id);
                false
            } else {
                true
            }
        });
        out
    });
    crate::object::FREED_OBJECT_IDS.with(|f| f.borrow_mut().extend(ids));
}

/// Snapshot for gc_status().
pub struct GcStatus {
    pub running: bool,
    pub protected: bool,
    pub full: bool,
    pub runs: i64,
    pub collected: i64,
    pub threshold: i64,
    pub buffer_size: i64,
    pub roots: i64,
    pub application_time: f64,
    pub collector_time: f64,
    pub destructor_time: f64,
    pub free_time: f64,
}

pub fn gc_status() -> GcStatus {
    let roots = live_root_count();
    // The buffer holds roots in slots 1..: grow it like Zend did on the way.
    while roots + 1 > GC_BUF_SIZE.with(|b| b.get()) && GC_BUF_SIZE.with(|b| b.get()) < BUF_MAX {
        grow_buffer();
    }
    let [c, d, f] = GC_TIMES.with(|t| t.get());
    let full = GC_FULL.with(|f| f.get());
    GcStatus {
        running: gc_active(),
        protected: full,
        full,
        runs: gc_runs(),
        collected: gc_collected(),
        threshold: GC_THRESHOLD.with(|t| t.get()) as i64,
        buffer_size: GC_BUF_SIZE.with(|b| b.get()) as i64,
        roots: roots as i64,
        application_time: GC_EPOCH.with(|e| e.elapsed().as_secs_f64()),
        collector_time: c as f64 / 1e9,
        destructor_time: d as f64 / 1e9,
        free_time: f as f64 / 1e9,
    }
}

/// Start the application clock (gc_status()['application_time']).
pub fn gc_request_startup() {
    GC_EPOCH.with(|_| {});
}

fn add_time(slot: usize, since: std::time::Instant) {
    let ns = since.elapsed().as_nanos() as u64;
    GC_TIMES.with(|t| {
        let mut v = t.get();
        v[slot] = v[slot].saturating_add(ns);
        t.set(v);
    });
}

#[derive(Clone)]
enum Node {
    Obj(Rc<RefCell<PhpObject>>),
    Arr(Rc<RefCell<PhpArray>>),
    Ref(Rc<RefCell<Value>>),
    Gen(Rc<RefCell<PhpGenerator>>),
    /// The storage arrays share copy-on-write (see array::Store): held by
    /// each of them, it holds their values. Not a PHP value of its own.
    Store(Rc<crate::array::Store>),
}

impl Node {
    fn ptr(&self) -> usize {
        match self {
            Node::Obj(o) => Rc::as_ptr(o) as *const u8 as usize,
            Node::Arr(a) => Rc::as_ptr(a) as *const u8 as usize,
            Node::Ref(r) => Rc::as_ptr(r) as *const u8 as usize,
            Node::Gen(g) => Rc::as_ptr(g) as *const u8 as usize,
            Node::Store(s) => Rc::as_ptr(s) as *const u8 as usize,
        }
    }
    fn strong(&self) -> usize {
        match self {
            Node::Obj(o) => Rc::strong_count(o),
            Node::Arr(a) => Rc::strong_count(a),
            Node::Ref(r) => Rc::strong_count(r),
            Node::Gen(g) => Rc::strong_count(g),
            Node::Store(s) => Rc::strong_count(s),
        }
    }
    fn of(v: &Value) -> Option<Node> {
        match v {
            Value::Object(o) => Some(Node::Obj(o.clone())),
            Value::Array(a) => Some(Node::Arr(a.clone())),
            Value::Reference(r) => Some(Node::Ref(r.clone())),
            Value::Generator(g) => Some(Node::Gen(g.clone())),
            _ => None,
        }
    }
    fn ptr_of(v: &Value) -> Option<usize> {
        match v {
            Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
            Value::Array(a) => Some(Rc::as_ptr(a) as *const u8 as usize),
            Value::Reference(r) => Some(Rc::as_ptr(r) as *const u8 as usize),
            Value::Generator(g) => Some(Rc::as_ptr(g) as *const u8 as usize),
            _ => None,
        }
    }
}

/// How a garbage array counts in gc_collect_cycles()'s result: like Zend
/// (an empty one is usually the immutable zend_empty_array), never (VM
/// bookkeeping), or always (a refcounted array even when empty).
const COUNT_NORMAL: u8 = 0;
const COUNT_NEVER: u8 = 1;
const COUNT_ALWAYS: u8 = 2;

struct Graph<'a> {
    /// Does this class keep a real PHP array in `__spl_array`
    /// (ArrayObject / ArrayIterator)? Other hidden storage arrays of native
    /// classes stand for C-level storage, which Zend does not count.
    view: &'a dyn GcView,
    array_storage_cache: HashMap<Vec<u8>, bool>,
    nodes: Vec<Node>,
    index: PtrMap<usize>,
    children: Vec<Vec<usize>>,
    internal: Vec<usize>,
    opaque: Vec<bool>,
    /// Arrays that are a Closure's internal capture storage (not PHP values,
    /// so not counted as collected).
    uncounted: Vec<u8>,
    /// WeakMap entries (ephemerons). Like Zend, an entry's value reference
    /// is accounted for once the map or the key is part of the graph, and
    /// the value is reachable through the entry only while both are live.
    eph: Vec<Eph>,
    eph_by_key: PtrMap<Vec<usize>>,
    eph_by_map: HashMap<u64, Vec<usize>>,
    /// Node index -> entries it is the map or key of.
    node_eph: PtrMap<Vec<usize>>,
}

struct Eph {
    value: Value,
    cancelled: bool,
    value_node: Option<usize>,
    map_node: Option<usize>,
    key_node: Option<usize>,
}

impl<'a> Graph<'a> {
    fn new(view: &'a dyn GcView) -> Self {
        Graph {
            view,
            array_storage_cache: HashMap::new(),
            nodes: Vec::new(),
            index: PtrMap::default(),
            children: Vec::new(),
            internal: Vec::new(),
            opaque: Vec::new(),
            uncounted: Vec::new(),
            eph: Vec::new(),
            eph_by_key: PtrMap::default(),
            eph_by_map: HashMap::new(),
            node_eph: PtrMap::default(),
        }
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
        self.uncounted.push(COUNT_NORMAL);
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
        let mut fiber_opaque = false;
        let ok = match &node {
            Node::Obj(o) => match o.try_borrow() {
                Ok(b) => {
                    for (k, v) in b.properties.iter() {
                        if let Some(c) = self.add_value(v) {
                            if matches!(v, Value::Array(_)) && crate::spl_array::is_hidden_prop(k) {
                                self.uncounted[c] = if k == b"__spl_array"
                                    && self.class_has_array_storage(&b.class_name)
                                {
                                    // (ArrayObject's own array: allocated even
                                    // when empty)
                                    COUNT_ALWAYS
                                } else {
                                    COUNT_NEVER
                                };
                            }
                            kids.push(c);
                        }
                    }
                    if let Some(cv) = &b.closure {
                        if let Some(c) = self.add_value(cv) {
                            if matches!(**cv, Value::Array(_)) {
                                self.uncounted[c] = COUNT_NEVER;
                            }
                            kids.push(c);
                        }
                    }
                    // What native data holds (the get_gc handler).
                    if let Some(n) = &b.native {
                        if let Some(f) = native_gc_of(n) {
                            let mut vals: Vec<Value> = Vec::new();
                            f(&n.data, &mut |v| {
                                if Node::ptr_of(v).is_some() {
                                    vals.push(v.clone());
                                }
                            });
                            for v in &vals {
                                if let Some(c) = self.add_value(v) {
                                    kids.push(c);
                                }
                            }
                        }
                    }
                    // A suspended fiber owns the locals of its frames
                    // (zend_fiber_object_gc).
                    if b.class_name.eq_ignore_ascii_case(b"Fiber") {
                        let mut vals: Vec<Value> = Vec::new();
                        match self.view.fiber_scan(b.object_id, &mut |v| {
                            if Node::ptr_of(v).is_some() {
                                vals.push(v.clone());
                            }
                        }) {
                            Some(true) => {
                                for v in &vals {
                                    if let Some(c) = self.add_value(v) {
                                        kids.push(c);
                                    }
                                }
                            }
                            Some(false) => fiber_opaque = true,
                            None => {}
                        }
                    }
                    // WeakMap entries this object is the key or the map of.
                    let mut ents: Vec<usize> = Vec::new();
                    if !self.eph.is_empty() {
                        let p = Rc::as_ptr(o) as *const u8 as usize;
                        if let Some(es) = self.eph_by_key.get(&p) {
                            ents.extend(es.iter().copied());
                            for &e in es {
                                self.eph[e].key_node = Some(i);
                            }
                        }
                        if b.class_name.eq_ignore_ascii_case(b"WeakMap") {
                            if let Some(es) = self.eph_by_map.get(&b.object_id) {
                                ents.extend(es.iter().copied());
                                for &e in es {
                                    self.eph[e].map_node = Some(i);
                                }
                            }
                        }
                    }
                    for &e in &ents {
                        if !self.eph[e].cancelled {
                            self.eph[e].cancelled = true;
                            let v = self.eph[e].value.clone();
                            if let Some(c) = self.add_value(&v) {
                                self.internal[c] += 1;
                                self.eph[e].value_node = Some(c);
                            }
                        }
                    }
                    if !ents.is_empty() {
                        self.node_eph.entry(i).or_default().extend(ents);
                    }
                    true
                }
                Err(_) => false,
            },
            Node::Arr(a) => match a.try_borrow() {
                // (storage shared with copies of the array: its node holds
                // the values)
                Ok(b) => match b.shared_store() {
                    Some(st) => {
                        kids.push(self.add(Node::Store(st.clone())));
                        true
                    }
                    None => {
                        for v in b.values() {
                            if let Some(c) = self.add_value(v) {
                                kids.push(c);
                            }
                        }
                        true
                    }
                },
                Err(_) => false,
            },
            Node::Store(st) => {
                for v in st.values() {
                    if let Some(c) = self.add_value(v) {
                        kids.push(c);
                    }
                }
                true
            }
            Node::Ref(r) => match r.try_borrow() {
                Ok(b) => {
                    if let Some(c) = self.add_value(&b) {
                        kids.push(c);
                    }
                    true
                }
                Err(_) => false,
            },
            Node::Gen(g) => match g.try_borrow() {
                Ok(b) if !b.is_running => {
                    b.for_each_gc_child(&mut |v| {
                        if let Some(c) = self.add_value(v) {
                            kids.push(c);
                        }
                    });
                    // WeakMap entries this generator is the key of.
                    if !self.eph.is_empty() {
                        self.key_ephemerons(i, Rc::as_ptr(g) as *const u8 as usize);
                    }
                    true
                }
                // Running inside a suspended fiber: the fiber holds its
                // values (see GcFrame::for_generator).
                _ => self.view.suspended_generator(g.as_ptr() as usize),
            },
        };
        drop(node);
        if !ok || fiber_opaque {
            self.opaque[i] = true;
            return;
        }
        for &c in &kids {
            self.internal[c] += 1;
        }
        self.children[i] = kids;
    }

    /// Node `i` (at address `p`, not an object node) is the key of WeakMap
    /// entries: their values are reached through it.
    #[inline(never)]
    fn key_ephemerons(&mut self, i: usize, p: usize) {
        let Some(es) = self.eph_by_key.get(&p).cloned() else {
            return;
        };
        for &e in &es {
            self.eph[e].key_node = Some(i);
            if !self.eph[e].cancelled {
                self.eph[e].cancelled = true;
                let v = self.eph[e].value.clone();
                if let Some(c) = self.add_value(&v) {
                    self.internal[c] += 1;
                    self.eph[e].value_node = Some(c);
                }
            }
        }
        self.node_eph.entry(i).or_default().extend(es);
    }

    fn class_has_array_storage(&mut self, class: &[u8]) -> bool {
        if let Some(&r) = self.array_storage_cache.get(class) {
            return r;
        }
        let r = self.view.class_has_array_storage(class);
        self.array_storage_cache.insert(class.to_vec(), r);
        r
    }

    /// Is entry `e`'s value reachable through it? (Both its map and its key
    /// are live; one outside the graph counts as live.)
    fn eph_live(&self, e: usize, live: &[bool]) -> bool {
        let en = &self.eph[e];
        en.map_node.is_none_or(|m| live[m]) && en.key_node.is_none_or(|k| live[k])
    }

    /// Values held through the WeakMap entries node `i` is the map or key of.
    fn eph_values(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.node_eph
            .get(&i)
            .into_iter()
            .flatten()
            .filter_map(|&e| self.eph[e].value_node)
    }
}

/// Result of one analysis pass.
struct Analysis<'a> {
    graph: Graph<'a>,
    garbage: Vec<bool>,
    /// The node of each root buffer slot (None: a free slot).
    root_slots: Vec<Option<usize>>,
}

/// gc_compact(): move the entries at the end of the buffer into the free
/// slots before them (`pos[i]` is slot i + 1).
fn compact_slots(pos: &mut [Option<usize>]) {
    let num = pos.iter().filter(|p| p.is_some()).count();
    if num == pos.len() || num == 0 {
        return;
    }
    // (Zend's `end` is slot num_roots)
    let end = num - 1;
    let mut free = 0usize;
    let mut scan = pos.len() - 1;
    while free < scan {
        while free < pos.len() && pos[free].is_some() {
            free += 1;
        }
        while scan > 0 && pos[scan].is_none() {
            scan -= 1;
        }
        if scan > free {
            pos[free] = pos[scan].take();
            free += 1;
            scan -= 1;
            if scan <= end {
                break;
            }
        } else {
            break;
        }
    }
}

/// The order the garbage of a run ends up in, in Zend's root buffer (the
/// order destructors are called in): the garbage roots in slot order, once
/// the live roots left their slots and gc_compact() moved the last entries
/// into the free slots; then the other garbage, as gc_collect_white() adds
/// it walking from each root; then (goro only) garbage found from no root.
#[allow(clippy::needless_range_loop)] // several per-node tables
fn buffer_order(an: &Analysis) -> Vec<usize> {
    let g = &an.graph;
    let len = g.nodes.len();
    let mut pos: Vec<Option<usize>> = an
        .root_slots
        .iter()
        .map(|o| o.filter(|&i| an.garbage[i]))
        .collect();
    compact_slots(&mut pos);
    let roots: Vec<usize> = pos.into_iter().flatten().collect();
    let mut in_buf = vec![false; len];
    let mut black = vec![false; len];
    let mut out: Vec<usize> = Vec::with_capacity(len);
    for &r in &roots {
        if !in_buf[r] {
            in_buf[r] = true;
            out.push(r);
        }
    }
    let counted = |i: usize| !matches!(g.nodes[i], Node::Ref(_) | Node::Store(_));
    let mut stack: Vec<usize> = Vec::new();
    for &r in &roots {
        if black[r] {
            continue;
        }
        black[r] = true;
        let mut cur = r;
        loop {
            if !in_buf[cur] && counted(cur) {
                in_buf[cur] = true;
                out.push(cur);
            }
            // The first white child is visited next, the others pushed.
            let mut first = None;
            for c in g.children[cur].iter().copied().chain(g.eph_values(cur)) {
                if an.garbage[c] && !black[c] {
                    black[c] = true;
                    if first.is_none() {
                        first = Some(c);
                    } else {
                        stack.push(c);
                    }
                }
            }
            match first.or_else(|| stack.pop()) {
                Some(n) => cur = n,
                None => break,
            }
        }
    }
    for i in 0..len {
        if an.garbage[i] && !in_buf[i] && counted(i) {
            out.push(i);
        }
    }
    if std::env::var_os("GORO_GC_DEBUG").is_some() {
        eprintln!("gc root slots {:?} -> order {:?}", an.root_slots, out);
    }
    out
}

#[allow(clippy::needless_range_loop)] // index loop mirrors the reference algorithm
fn analyze<'a>(
    extra_refs: &PtrMap<usize>,
    ephemerons: Vec<(u64, usize, Value)>,
    limit: Option<usize>,
    view: &'a dyn GcView,
) -> Analysis<'a> {
    let mut g = Graph::new(view);
    for (map_oid, key_ptr, value) in ephemerons {
        let e = g.eph.len();
        g.eph.push(Eph {
            value,
            cancelled: false,
            value_node: None,
            map_node: None,
            key_node: None,
        });
        g.eph_by_key.entry(key_ptr).or_default().push(e);
        g.eph_by_map.entry(map_oid).or_default().push(e);
    }
    // Seed with candidates.
    let objs: Vec<Rc<RefCell<PhpObject>>> = TRACKED_OBJECTS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|w| w.strong_count() > 0);
        PRUNE_AT.with(|p| p.set((t.len() * 2).max(4096)));
        if limit.is_some() {
            return Vec::new();
        }
        t.iter().filter_map(|w| w.upgrade()).collect()
    });
    // Automatic runs only look at the root buffer (like Zend); explicit
    // gc_collect_cycles() also scans every tracked object.
    for o in objs {
        g.add(Node::Obj(o));
    }
    if limit.is_none() {
        let gens: Vec<Rc<RefCell<PhpGenerator>>> = TRACKED_GENERATORS.with(|t| {
            let mut t = t.borrow_mut();
            t.retain(|w| w.strong_count() > 0);
            t.iter().filter_map(|w| w.upgrade()).collect()
        });
        for gr in gens {
            g.add(Node::Gen(gr));
        }
    }
    // Take every root out of the buffer, in slot order (holes kept: they
    // matter to gc_compact()).
    let taken: Vec<Option<WeakNode>> = POSSIBLE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        r.flush_freed();
        r.unused.clear();
        std::mem::take(&mut r.slots)
    });
    let mut root_slots: Vec<Option<usize>> = Vec::with_capacity(taken.len());
    for e in taken {
        root_slots.push(e.and_then(|e| e.unbuffer()).map(|n| g.add(n)));
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
        if let Some(es) = g.node_eph.get(&idx) {
            for &e in es {
                if let Some(v) = g.eph[e].value_node {
                    if !live[v] && g.eph_live(e, &live) {
                        live[v] = true;
                        stack.push(v);
                    }
                }
            }
        }
    }
    if std::env::var_os("GORO_GC_DEBUG").is_some() {
        for idx in 0..len {
            let n = &g.nodes[idx];
            let kind = match n {
                Node::Obj(o) => format!("obj#{}", o.try_borrow().map(|b| b.object_id).unwrap_or(0)),
                Node::Arr(_) => "arr".into(),
                Node::Ref(_) => "ref".into(),
                Node::Gen(_) => "gen".into(),
                Node::Store(_) => "store".into(),
            };
            eprintln!(
                "gc node {} {} ptr={:#x} strong={} internal={} extra={} opaque={} live={} kids={:?}",
                idx,
                kind,
                n.ptr(),
                n.strong(),
                g.internal[idx],
                extra_refs.get(&n.ptr()).copied().unwrap_or(0),
                g.opaque[idx],
                live[idx],
                g.children[idx]
            );
        }
    }
    let garbage = live.iter().map(|l| !l).collect();
    Analysis {
        graph: g,
        garbage,
        root_slots,
    }
}

/// The locals (CVs and temporaries) of a running user function, registered
/// for the duration of the call. While its fiber is suspended they belong to
/// the Fiber object's graph, like Zend's zend_fiber_object_gc scanning the
/// suspended execute_data chain.
pub(crate) enum GcFrame {
    /// A user function's frame.
    Frame {
        cvs: *const Vec<Value>,
        tmps: *const Vec<Value>,
        /// The frame's symbol table (its dynamic variables), once it has one.
        sym: *const Option<crate::symtab::FrameSymtab>,
        /// Iterator state of the frame's running foreach loops.
        fe: *const crate::vm::ForeachState,
        /// Exceptions pending in the frame's running finally blocks.
        fin: *const Vec<(usize, Option<Value>)>,
        /// The argument list its caller keeps while the call runs (see
        /// Vm::gc_call_args), or null; its low bit tags a call whose result
        /// is discarded (Vm::call_args_tagged).
        args: *const Vec<Value>,
        /// CV 0 is `$this`
        has_this: bool,
    },
    /// The frame of a running generator (a fiber suspended inside it owns
    /// what the generator holds: zend_generator_frame_gc()).
    Gen(*const PhpGenerator, bool),
    /// Values native code holds on its own stack while the call it makes
    /// runs, which Zend keeps in the call's frame (the closure of a fiber,
    /// the Generator an internal method runs on).
    Pin(*const dyn GcPin),
}

impl GcFrame {
    pub(crate) fn new(
        cvs: &Vec<Value>,
        tmps: &Vec<Value>,
        sym: &Option<crate::symtab::FrameSymtab>,
        fe: &crate::vm::ForeachState,
        fin: &Vec<(usize, Option<Value>)>,
        args: *const Vec<Value>,
        has_this: bool,
    ) -> Self {
        GcFrame::Frame {
            cvs,
            tmps,
            sym,
            fe,
            fin,
            args,
            has_this,
        }
    }
    /// The frame of running generator `g`.
    pub(crate) fn for_generator(g: &PhpGenerator) -> Self {
        let has_this = g.op_array.cv_names.first().map(|n| n.as_slice()) == Some(b"this");
        GcFrame::Gen(g, has_this)
    }
    /// Values a native caller holds while its call runs (see the type).
    pub(crate) fn pin<T: GcPin + 'static>(vals: &T) -> Self {
        GcFrame::Pin(vals as &dyn GcPin as *const dyn GcPin)
    }
    /// A pin rather than a function's frame.
    #[inline]
    pub(crate) fn is_pin(&self) -> bool {
        matches!(self, GcFrame::Pin(_))
    }
    /// The running generator of this frame (as `RefCell::as_ptr`), or 0.
    pub(crate) fn generator_ptr(&self) -> usize {
        match self {
            GcFrame::Gen(g, _) => *g as usize,
            _ => 0,
        }
    }
    /// `$this` of the frame (zend_get_this_object), if it has one.
    ///
    /// # Safety
    /// The frame must still be running (it is unregistered before its
    /// function returns), so its CV vector is alive.
    pub(crate) unsafe fn this_object(&self) -> Option<Value> {
        // SAFETY: see above.
        let cvs = match self {
            GcFrame::Frame {
                cvs,
                has_this: true,
                ..
            } => unsafe { &**cvs },
            GcFrame::Gen(g, true) => unsafe { &(**g).cvs },
            _ => return None,
        };
        match cvs.first().map(|v| v.deref()) {
            Some(v @ Value::Object(_)) => Some(v),
            _ => None,
        }
    }
    /// Visit the frame's values.
    ///
    /// # Safety
    /// Only for the frames of a suspended fiber: the vectors live on the
    /// fiber's stack, which is neither running nor freed until the fiber is
    /// resumed or destroyed (the frame is unregistered before it returns).
    pub(crate) unsafe fn for_each(&self, f: &mut dyn FnMut(&Value)) {
        match self {
            // SAFETY: see above (the pinned values outlive the pin).
            GcFrame::Pin(p) => unsafe { (**p).gc_visit(f) },
            // SAFETY: the generator is running in the suspended fiber: its
            // state is idle until the fiber resumes.
            GcFrame::Gen(g, _) => unsafe { (**g).for_each_gc_child(f) },
            GcFrame::Frame {
                cvs,
                tmps,
                sym,
                fe,
                fin,
                args,
                ..
            } => {
                // SAFETY: see above.
                let (cvs, tmps, sym, fe, fin) = unsafe { (&**cvs, &**tmps, &**sym, &**fe, &**fin) };
                for v in cvs.iter().chain(tmps.iter()) {
                    f(v);
                }
                // (the low bit tags a discarded result, see
                // Vm::call_args_tagged)
                let args = (*args as usize & !1) as *const Vec<Value>;
                if !args.is_null() {
                    // SAFETY: the caller keeps the list until the call returns.
                    for v in unsafe { &*args } {
                        f(v);
                    }
                }
                fe.for_each_value(f);
                for (_, e) in fin {
                    if let Some(e) = e {
                        f(e);
                    }
                }
                // A local symbol table holds the dynamic variables (the
                // global one is not the frame's).
                if let Some(fs) = sym {
                    if fs.owner && !fs.tab.global {
                        f(&Value::Array(fs.tab.arr.clone()));
                    }
                    // (the table cells its CVs are bound to)
                    for c in fs.bind.cells.iter().flatten() {
                        f(&Value::Reference(c.clone()));
                    }
                }
            }
        }
    }
}

/// Something native code holds that a suspended fiber owns (see
/// GcFrame::pin).
pub(crate) trait GcPin {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value));
}

impl GcPin for Value {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value)) {
        f(self)
    }
}

impl GcPin for Rc<RefCell<PhpGenerator>> {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value)) {
        f(&Value::Generator(self.clone()))
    }
}

impl GcPin for Rc<RefCell<PhpObject>> {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value)) {
        f(&Value::Object(self.clone()))
    }
}

impl<T: GcPin> GcPin for Vec<T> {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value)) {
        for v in self {
            v.gc_visit(f);
        }
    }
}

impl<T: GcPin> GcPin for Option<T> {
    fn gc_visit(&self, f: &mut dyn FnMut(&Value)) {
        if let Some(v) = self {
            v.gc_visit(f);
        }
    }
}

/// Callback of the collector's destructor fibers. Like php's
/// gc_destructor_fiber it is not a user-visible function.
pub const DESTRUCTOR_FIBER_FN: &[u8] = b"gc_destructor_fiber";

/// State of the destructor phase of a collection (shared with the
/// destructor fiber, which runs the destructors when the collection happens
/// inside a fiber).
struct DtorCtx {
    queue: std::collections::VecDeque<Weak<RefCell<PhpObject>>>,
    /// The VM's own strong refs per object (destructible list).
    base_extra: PtrMap<usize>,
    /// The largest of `base_extra` (an object holding more is no orphan).
    max_extra: usize,
    called: PtrSet,
    zombies: Vec<Rc<RefCell<PhpObject>>>,
    /// Which destructor phase this is.
    phase: u64,
}

thread_local! {
    static DTOR_CTX: RefCell<Option<DtorCtx>> = const { RefCell::new(None) };
    static DTOR_GEN: Cell<u64> = const { Cell::new(0) };
    /// Bumped when a destructor suspends the destructor fiber running it:
    /// that fiber is no longer GC_G(dtor_fiber), and stops calling
    /// destructors once that one returns.
    static DTOR_FIBER_GEN: Cell<u64> = const { Cell::new(0) };
}

/// Destructor-phase work is pending.
pub fn dtor_queue_pending() -> bool {
    DTOR_CTX.with(|c| c.borrow().as_ref().is_some_and(|c| !c.queue.is_empty()))
}

/// Call the next pending garbage destructor (gc_call_destructors). Returns
/// false when there is nothing left, or when the destructor phase it
/// belonged to is over (a destructor fiber resumed after the collection).
pub fn dtor_step<H: GcHost + ?Sized>(host: &mut H) -> bool {
    let next = DTOR_CTX.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        let w = c.queue.pop_front()?;
        Some((w, c.phase))
    });
    let Some((w, phase)) = next else {
        return false;
    };
    let Some(o) = w.upgrade() else {
        return true; // freed (and destructed) by refcounting meanwhile
    };
    if !host.gc_needs_destructor(&o) {
        return true;
    }
    let p = Rc::as_ptr(&o) as *const u8 as usize;
    // Only our upgrade and the VM's bookkeeping hold it: Zend would
    // already have freed it (destructor included) when its last
    // reference went away during an earlier destructor.
    let held = DTOR_CTX.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|c| c.base_extra.get(&p).copied())
            .unwrap_or(0)
    });
    let orphan = Rc::strong_count(&o) <= 1 + held;
    let fiber_gen = DTOR_FIBER_GEN.with(|g| g.get());
    host.gc_call_destructor(&o);
    let current = DTOR_CTX.with(|c| c.borrow().as_ref().is_some_and(|c| c.phase == phase))
        && DTOR_FIBER_GEN.with(|g| g.get()) == fiber_gen;
    if !current {
        // Resumed after its collection finished (the destructor had
        // suspended its fiber): just a possible root now.
        buffer_object(&o);
        return false;
    }
    if !orphan {
        dtor_drain_orphans(host);
    }
    let zombie = !orphan && Rc::strong_count(&o) <= 1 + held;
    if !orphan && !zombie {
        // Still alive: a possible root of the rerun.
        buffer_object(&o);
    }
    DTOR_CTX.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            c.called.insert(p);
            if zombie {
                // The destructor dropped the last reference: Zend's
                // GC_DELREF leaves it at refcount 0 in the buffer.
                c.zombies.push(o);
            }
        }
    });
    true
}

/// The destructor candidates the last destructor dropped the last
/// reference to: Zend frees them (zend_objects_store_del: destructor, then
/// free) while that destructor runs, before the destructors queued ahead
/// of them. Their bookkeeping goes too, so what they hold is released.
#[inline(never)]
fn dtor_drain_orphans<H: GcHost + ?Sized>(host: &mut H) {
    loop {
        let found = DTOR_CTX.with(|c| {
            let c = c.borrow();
            let c = c.as_ref()?;
            c.queue.iter().enumerate().find_map(|(i, w)| {
                // (most are held by more than the VM's lists can: no
                // upgrade or lookup for them)
                let n = w.strong_count();
                if n == 0 || n > c.max_extra {
                    return None;
                }
                let o = w.upgrade()?;
                let p = Rc::as_ptr(&o) as *const u8 as usize;
                let held = c.base_extra.get(&p).copied().unwrap_or(0);
                // (`o` here is our own upgrade)
                (Rc::strong_count(&o) <= 1 + held).then_some((i, o, p))
            })
        });
        let Some((i, o, p)) = found else {
            return;
        };
        DTOR_CTX.with(|c| {
            if let Some(c) = c.borrow_mut().as_mut() {
                c.queue.remove(i);
            }
        });
        if host.gc_needs_destructor(&o) {
            host.gc_call_destructor(&o);
        }
        let mut one = PtrSet::default();
        one.insert(p);
        DTOR_CTX.with(|c| {
            if let Some(c) = c.borrow_mut().as_mut() {
                c.called.insert(p);
                c.base_extra.remove(&p);
            }
        });
        host.gc_forget_destructed(&one);
        drop(o);
    }
}

/// Every generator has a destructor (zend_generator_dtor_storage, which
/// force-closes a suspended one so its `finally` blocks run), called once.
fn generator_needs_close(g: &Rc<RefCell<PhpGenerator>>) -> bool {
    g.try_borrow()
        .is_ok_and(|b| !b.is_running && !b.gc_dtor_called.get())
}

/// Read-only queries the graph builder makes.
pub trait GcView {
    fn class_has_array_storage(&self, class: &[u8]) -> bool;
    /// Values held by the suspended fiber of Fiber object `oid`: `Some(true)`
    /// when enumerated, `Some(false)` if they cannot be (opaque), `None` if
    /// it has no suspended stack.
    fn fiber_scan(&self, oid: u64, f: &mut dyn FnMut(&Value)) -> Option<bool>;
    /// Is the running generator `ptr` (RefCell::as_ptr) running inside a
    /// suspended fiber (which then owns its values)?
    fn suspended_generator(&self, _ptr: usize) -> bool {
        false
    }
}

impl<H: GcHost> GcView for H {
    fn class_has_array_storage(&self, class: &[u8]) -> bool {
        self.gc_class_has_array_storage(class)
    }
    fn fiber_scan(&self, oid: u64, f: &mut dyn FnMut(&Value)) -> Option<bool> {
        self.gc_fiber_scan(oid, f)
    }
    fn suspended_generator(&self, ptr: usize) -> bool {
        self.gc_suspended_generator(ptr)
    }
}

/// Hooks the collector needs from the VM.
pub trait GcHost {
    /// Strong references held by the VM's own bookkeeping (e.g. the list of
    /// destructible objects), keyed by Rc pointer.
    fn gc_extra_refs(&self) -> PtrMap<usize>;
    /// Does this object still need its destructor called?
    fn gc_needs_destructor(&self, obj: &Rc<RefCell<PhpObject>>) -> bool;
    /// Call the destructor of a garbage object.
    fn gc_call_destructor(&mut self, obj: &Rc<RefCell<PhpObject>>);
    /// Is PHP code running inside a fiber (EG(active_fiber))?
    fn gc_in_fiber(&self) -> bool {
        false
    }
    /// Run the pending destructors (dtor_step) inside destructor fibers.
    fn gc_run_destructor_fibers(&mut self) {
        while dtor_step(self) {}
    }
    /// Force-close a garbage generator (runs its finally blocks).
    fn gc_close_generator(&mut self, _g: &Rc<RefCell<PhpGenerator>>) {}
    /// Drop the VM's "needs a destructor" bookkeeping for these objects.
    fn gc_forget_destructed(&mut self, _ptrs: &PtrSet) {}
    /// Forget VM bookkeeping for objects about to be freed (`weakmaps`:
    /// object ids of the WeakMaps among them).
    fn gc_forget_objects(&mut self, ptrs: &PtrSet, weakmaps: &[u64]);
    /// Is `class` (as stored on the object) an ArrayObject / ArrayIterator,
    /// whose `__spl_array` storage is a PHP array?
    fn gc_class_has_array_storage(&self, _class: &[u8]) -> bool {
        false
    }
    /// See GcView::fiber_scan.
    fn gc_fiber_scan(&self, _oid: u64, _f: &mut dyn FnMut(&Value)) -> Option<bool> {
        None
    }
    /// See GcView::suspended_generator.
    fn gc_suspended_generator(&self, _ptr: usize) -> bool {
        false
    }
    /// WeakMap entries as (map object id, key object ptr, value clone).
    fn gc_ephemerons(&self) -> Vec<(u64, usize, Value)> {
        Vec::new()
    }
}

/// Run a collection. Returns the number of freed arrays/objects, counted
/// like zend_gc_collect_cycles: references are not counted, and objects
/// whose destructor runs are counted by the run that finally frees them.
///
/// `limit`: `None` for gc_collect_cycles() (also scans every tracked object
/// and generator), `Some(_)` for an automatic run over the buffered roots.
#[allow(clippy::needless_range_loop)] // index loop mirrors the reference algorithm
pub fn collect_cycles<H: GcHost>(host: &mut H, limit: Option<usize>) -> i64 {
    if GC_ACTIVE.with(|c| c.get()) {
        return 0;
    }
    let start = std::time::Instant::now();
    GC_ACTIVE.with(|c| c.set(true));
    let mut total = 0i64;
    let mut did_rerun = false;
    loop {
        let had_roots = live_root_count() > 0;
        let mut base_extra = host.gc_extra_refs();
        let mut extra = base_extra.clone();
        // Zombies (see ZOMBIES): our strong ref is bookkeeping, not a PHP
        // reference, and they are roots of this run.
        let zombies: Vec<Rc<RefCell<PhpObject>>> =
            ZOMBIES.with(|z| std::mem::take(&mut *z.borrow_mut()));
        for z in &zombies {
            *extra
                .entry(Rc::as_ptr(z) as *const u8 as usize)
                .or_insert(0) += 1;
            buffer_object(z);
        }
        // WeakMap values: our clone of each is accounted as an extra ref.
        let eph = host.gc_ephemerons();
        for (_, _, v) in &eph {
            if let Some(p) = Node::ptr_of(v) {
                *extra.entry(p).or_insert(0) += 1;
            }
        }
        let an = analyze(&extra, eph, limit, &*host);
        let len = an.graph.nodes.len();
        if had_roots || an.garbage.iter().any(|g| *g) {
            GC_RUNS.with(|c| c.set(c.get() + 1));
        }
        // Garbage objects that still need their destructor called.
        // (A suspended generator's dtor_obj, zend_generator_dtor_storage,
        // runs its pending finally blocks: it is one too.)
        let mut dtor_idx: Vec<usize> = Vec::new();
        for idx in buffer_order(&an) {
            match &an.graph.nodes[idx] {
                Node::Obj(o) if host.gc_needs_destructor(o) => dtor_idx.push(idx),
                Node::Gen(g) if generator_needs_close(g) => dtor_idx.push(idx),
                _ => {}
            }
        }
        let mut free = an.garbage.clone();
        if !dtor_idx.is_empty() {
            // Everything reachable from an object with a pending destructor
            // stays alive for now: the destructor may resurrect it
            // (gc_remove_nested_data_from_buffer).
            let mut stack = dtor_idx.clone();
            for &d in &dtor_idx {
                free[d] = false;
            }
            while let Some(idx) = stack.pop() {
                let g = &an.graph;
                for c in g.children[idx].iter().copied().chain(g.eph_values(idx)) {
                    if free[c] {
                        free[c] = false;
                        stack.push(c);
                    }
                }
            }
        }
        // Destructor candidates are held weakly: one freed by refcounting
        // while another destructor runs gets its destructor from the VM
        // right there, like Zend's nested zend_objects_store_del.
        let dtor_objs: Vec<Weak<RefCell<PhpObject>>> = dtor_idx
            .iter()
            .filter_map(|&i| match &an.graph.nodes[i] {
                Node::Obj(o) => Some(Rc::downgrade(o)),
                _ => None,
            })
            .collect();
        let dtor_gens: Vec<Weak<RefCell<PhpGenerator>>> = dtor_idx
            .iter()
            .filter_map(|&i| match &an.graph.nodes[i] {
                Node::Gen(g) => Some(Rc::downgrade(g)),
                _ => None,
            })
            .collect();
        // Keep the plain garbage (our clones) for the free phase; release
        // every other clone so destructors see true refcounts.
        let free_nodes: Vec<(Node, u8)> = (0..len)
            .filter(|&i| free[i])
            .map(|i| (an.graph.nodes[i].clone(), an.graph.uncounted[i]))
            .collect();
        drop(an);
        drop(zombies);
        // Zend calls the destructors first, then destroys the garbage.
        let mut new_zombies: Vec<Rc<RefCell<PhpObject>>> = Vec::new();
        if !dtor_objs.is_empty() {
            let dtor_start = std::time::Instant::now();
            let phase = DTOR_GEN.with(|g| {
                g.set(g.get() + 1);
                g.get()
            });
            DTOR_CTX.with(|c| {
                *c.borrow_mut() = Some(DtorCtx {
                    queue: dtor_objs.iter().cloned().collect(),
                    max_extra: base_extra.values().copied().max().unwrap_or(0),
                    base_extra: std::mem::take(&mut base_extra),
                    called: PtrSet::default(),
                    zombies: Vec::new(),
                    phase,
                })
            });
            if host.gc_in_fiber() {
                // gc_call_destructors_in_fiber: a destructor may suspend.
                host.gc_run_destructor_fibers();
            } else {
                while dtor_step(host) {}
            }
            let ctx = DTOR_CTX.with(|c| c.borrow_mut().take());
            if let Some(ctx) = ctx {
                host.gc_forget_destructed(&ctx.called);
                new_zombies = ctx.zombies;
            }
            add_time(1, dtor_start);
        }
        if !dtor_gens.is_empty() {
            let dtor_start = std::time::Instant::now();
            for w in &dtor_gens {
                if let Some(g) = w.upgrade() {
                    if generator_needs_close(&g) {
                        host.gc_close_generator(&g);
                    }
                }
            }
            add_time(1, dtor_start);
        }
        let free_start = std::time::Instant::now();
        // A garbage generator its close freed (the close released the
        // variables of its frame that made the cycle) was collected.
        let mut count = dtor_gens.iter().filter(|w| w.strong_count() == 0).count() as i64;
        let mut dead_props: Vec<Vec<(Vec<u8>, Value)>> = Vec::new();
        let mut dead_arrays: Vec<PhpArray> = Vec::new();
        let mut dead_vals: Vec<Value> = Vec::new();
        let mut dead_gens: Vec<Vec<Value>> = Vec::new();
        let mut dead_natives: Vec<crate::object::NativeData> = Vec::new();
        if !free_nodes.is_empty() {
            let ptrs: PtrSet = free_nodes.iter().map(|(n, _)| n.ptr()).collect();
            let weakmaps: Vec<u64> = free_nodes
                .iter()
                .filter_map(|(n, _)| match n {
                    Node::Obj(o) => o
                        .try_borrow()
                        .ok()
                        .filter(|b| b.class_name.eq_ignore_ascii_case(b"WeakMap"))
                        .map(|b| b.object_id),
                    _ => None,
                })
                .collect();
            host.gc_forget_objects(&ptrs, &weakmaps);
            for (node, uncounted) in &free_nodes {
                match node {
                    Node::Obj(o) => {
                        if let Ok(mut b) = o.try_borrow_mut() {
                            dead_props.push(b.properties.take_vec());
                            if let Some(c) = b.closure.take() {
                                dead_vals.push(*c);
                            }
                            if b.native.as_ref().and_then(native_gc_of).is_some() {
                                dead_natives.extend(b.native.take());
                            }
                            count += 1;
                        }
                    }
                    Node::Arr(a) => {
                        if let Ok(mut b) = a.try_borrow_mut() {
                            // An empty array is almost always Zend's
                            // immutable zend_empty_array, which is not
                            // refcounted and so never counted.
                            if *uncounted == COUNT_ALWAYS
                                || (*uncounted == COUNT_NORMAL && !b.is_empty())
                            {
                                count += 1;
                            }
                            dead_arrays.push(std::mem::take(&mut *b));
                        }
                    }
                    Node::Ref(r) => {
                        if let Ok(mut b) = r.try_borrow_mut() {
                            dead_vals.push(std::mem::replace(&mut *b, Value::Null));
                        }
                    }
                    Node::Gen(gr) => {
                        if let Ok(mut b) = gr.try_borrow_mut() {
                            dead_gens.push(b.take_gc_children());
                            count += 1;
                        }
                    }
                    // (released with the last of the garbage arrays holding it)
                    Node::Store(_) => {}
                }
            }
        }
        // Release our clones first, then the detached contents: this is
        // where the cycles actually get freed.
        drop(free_nodes);
        drop(dead_natives);
        drop(dead_vals);
        drop(dead_gens);
        drop(dead_arrays);
        drop(dead_props);
        add_time(2, free_start);
        total += count;
        for z in &new_zombies {
            buffer_object(z);
        }
        ZOMBIES.with(|z| z.borrow_mut().extend(new_zombies));
        if (dtor_objs.is_empty() && dtor_gens.is_empty()) || did_rerun {
            break;
        }
        did_rerun = true;
    }
    GC_PENDING.with(|p| p.set(false));
    GC_SUPPRESSED.with(|p| p.set(false));
    GC_COLLECTED.with(|c| c.set(c.get() + total));
    GC_ACTIVE.with(|c| c.set(false));
    add_time(0, start);
    total
}

impl GcHost for crate::vm::Vm {
    fn gc_extra_refs(&self) -> PtrMap<usize> {
        let mut m = PtrMap::default();
        for v in self.destructible_objects.iter() {
            if let Value::Object(o) = v {
                *m.entry(Rc::as_ptr(o) as *const u8 as usize).or_insert(0) += 1;
            }
        }
        for g in &self.finally_generators {
            *m.entry(Rc::as_ptr(g) as *const u8 as usize).or_insert(0) += 1;
        }
        m
    }
    fn gc_needs_destructor(&self, obj: &Rc<RefCell<PhpObject>>) -> bool {
        let Ok(b) = obj.try_borrow() else {
            return false;
        };
        // A suspended fiber is destroyed (unwound) like a destructor call.
        if b.class_name.eq_ignore_ascii_case(b"Fiber") {
            return self
                .fiber_coroutines
                .get(&b.object_id)
                .is_some_and(|f| f.started() && !f.done());
        }
        if matches!(b.get_property(b"__destructed"), Value::True)
            || matches!(b.get_property(b"__ctor_pending"), Value::True)
        {
            // Destructed already, or `new` was interrupted before its
            // constructor ran (no destructor then).
            return false;
        }
        let lower: Vec<u8> = b
            .class_name
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        self.classes
            .get(&lower)
            .and_then(|c| c.get_method(b"__destruct"))
            .is_some()
    }
    fn gc_call_destructor(&mut self, obj: &Rc<RefCell<PhpObject>>) {
        if obj.borrow().class_name.eq_ignore_ascii_case(b"Fiber") {
            self.fiber_destroy(obj);
            return;
        }
        // zend_objects_destroy_object(): lazy objects are not destructed.
        if matches!(obj.borrow().get_property(b"__lazy_state"), Value::String(_)) {
            obj.borrow_mut()
                .set_property(b"__destructed".to_vec(), Value::True);
            return;
        }
        // A pending exception is set aside while the destructor runs and
        // becomes the previous of whatever it throws.
        let prev = self.current_exception.take();
        let cn = obj.borrow().class_name.clone();
        let name = self.dtor_frame_name(&cn);
        self.call_stack
            .push((name, crate::rcstr::RcStr::new(), 0, Vec::new(), true));
        self.gc_run_destructor(obj);
        self.call_stack.pop();
        if let Some(prev) = prev {
            match self.current_exception.clone() {
                Some(exc) => crate::vm::Vm::exception_set_previous(&exc, prev),
                None => self.current_exception = Some(prev),
            }
        }
    }
    fn gc_in_fiber(&self) -> bool {
        self.current_fiber.is_some()
    }
    fn gc_run_destructor_fibers(&mut self) {
        // gc_call_destructors_in_fiber(): the destructors run in a fiber of
        // their own, so one that suspends only suspends that fiber; the rest
        // continue in a new one. Exceptions from all of them are chained.
        let mut exception = self.current_exception.take();
        // The fiber's frames follow gc_collect_cycles() in its backtraces.
        self.sync_builtin_frames();
        while dtor_queue_pending() {
            let id = self.alloc_object_id();
            let mut f = PhpObject::new(b"Fiber".to_vec(), id);
            f.set_property(
                b"__fiber_callback".to_vec(),
                Value::String(crate::string::PhpString::from_bytes(DESTRUCTOR_FIBER_FN)),
            );
            f.set_property(
                b"__fiber_state".to_vec(),
                Value::Long(crate::fiber::FiberState::Created as i64),
            );
            let fiber = Rc::new(RefCell::new(f));
            // Destroyed (unwound) when released while suspended.
            self.destructible_objects.push(Value::Object(fiber.clone()));
            let line = self.current_line;
            let _ = self.fiber_start(fiber.clone(), &[], line);
            // A destructor suspended it. If nothing else holds it, it is
            // released (and so destroyed) right away.
            let suspended = fiber.borrow().get_property(b"__fiber_state").to_long()
                == crate::fiber::FiberState::Suspended as i64;
            if suspended {
                // (the remaining destructors run in a new fiber)
                DTOR_FIBER_GEN.with(|g| g.set(g.get() + 1));
            }
            // (the VM's lists of destructible objects do not own it)
            let listed = self
                .destructible_objects
                .iter()
                .filter(|v| matches!(v, Value::Object(o) if Rc::ptr_eq(o, &fiber)))
                .count();
            if suspended && Rc::strong_count(&fiber) <= 1 + listed {
                if let Some(exc) = self.current_exception.take() {
                    if let Some(prev) = exception.take() {
                        crate::vm::Vm::exception_set_previous(&exc, prev);
                    }
                    exception = Some(exc);
                }
                self.destructible_objects
                    .retain(|v| !matches!(v, Value::Object(o) if Rc::ptr_eq(o, &fiber)));
                self.fiber_destroy(&fiber);
            }
            drop(fiber);
            if let Some(exc) = self.current_exception.take() {
                if let Some(prev) = exception.take() {
                    crate::vm::Vm::exception_set_previous(&exc, prev);
                }
                exception = Some(exc);
            }
        }
        self.current_exception = exception;
    }
    fn gc_close_generator(&mut self, g: &Rc<RefCell<PhpGenerator>>) {
        let prev = self.current_exception.take();
        self.finally_generators.retain(|x| !Rc::ptr_eq(x, g));
        if let Ok(mut b) = g.try_borrow_mut() {
            b.gc_dtor_called.set(true);
            b.close(self);
        }
        if let Some(prev) = prev {
            match self.current_exception.clone() {
                Some(exc) => crate::vm::Vm::exception_set_previous(&exc, prev),
                None => self.current_exception = Some(prev),
            }
        }
    }
    fn gc_forget_destructed(&mut self, ptrs: &PtrSet) {
        if ptrs.is_empty() {
            return;
        }
        self.destructible_objects.retain(|v| match v {
            Value::Object(o) => {
                let keep = !ptrs.contains(&(Rc::as_ptr(o) as *const u8 as usize));
                if !keep {
                    if let Ok(b) = o.try_borrow() {
                        b.dtor_listed.set(false);
                    }
                }
                keep
            }
            _ => true,
        });
    }
    fn gc_forget_objects(&mut self, ptrs: &PtrSet, weakmaps: &[u64]) {
        self.destructible_objects.retain(|v| match v {
            Value::Object(o) => {
                let keep = !ptrs.contains(&(Rc::as_ptr(o) as *const u8 as usize));
                if !keep {
                    if let Ok(b) = o.try_borrow() {
                        b.dtor_listed.set(false);
                    }
                }
                keep
            }
            _ => true,
        });
        self.finally_generators
            .retain(|g| !ptrs.contains(&(Rc::as_ptr(g) as *const u8 as usize)));
        // Entries whose key dies are dropped from their WeakMap.
        let mut dropped: Vec<Value> = Vec::new();
        // A dying WeakMap releases all its entries.
        for id in weakmaps {
            if let Some(entries) = self.weak_map_entries.remove(id) {
                dropped.extend(entries.into_iter().map(|(_, _, v)| v));
            }
        }
        for entries in self.weak_map_entries.values_mut() {
            entries.retain_mut(|(_, w, v)| {
                if ptrs.contains(&w.as_ptr()) {
                    dropped.push(std::mem::replace(v, Value::Null));
                    false
                } else {
                    true
                }
            });
        }
        drop(dropped);
    }
    fn gc_suspended_generator(&self, ptr: usize) -> bool {
        self.fiber_coroutines.values().any(|fiber| {
            fiber.started()
                && !fiber.done()
                && fiber
                    .saved_gc_frames
                    .iter()
                    .any(|fr| fr.generator_ptr() == ptr)
        })
    }
    fn gc_fiber_scan(&self, oid: u64, f: &mut dyn FnMut(&Value)) -> Option<bool> {
        let fiber = self.fiber_coroutines.get(&oid)?;
        if !fiber.started() || fiber.done() {
            return None;
        }
        for fr in &fiber.saved_gc_frames {
            // SAFETY: the fiber is suspended (it is in fiber_coroutines, not
            // being driven), so its frames are intact and idle.
            unsafe { fr.for_each(f) };
        }
        for call in &fiber.saved_pending_calls {
            for v in call
                .args
                .iter()
                .chain(call.named_args.iter().map(|(_, v)| v))
            {
                f(v);
            }
        }
        for (_, _, _, args, _) in &fiber.saved_call_stack {
            for v in args {
                f(v);
            }
        }
        for v in &fiber.start_args {
            f(v);
        }
        Some(true)
    }
    fn gc_class_has_array_storage(&self, class: &[u8]) -> bool {
        let lower = class.to_ascii_lowercase();
        [b"arrayobject".as_slice(), b"arrayiterator"]
            .iter()
            .any(|t| lower == *t || self.class_extends(&lower, t))
    }
    fn gc_ephemerons(&self) -> Vec<(u64, usize, Value)> {
        let mut out = Vec::new();
        for (map_id, entries) in &self.weak_map_entries {
            for (_, w, v) in entries {
                if w.is_alive()
                    && matches!(
                        v,
                        Value::Object(_)
                            | Value::Array(_)
                            | Value::Reference(_)
                            | Value::Generator(_)
                    )
                {
                    out.push((*map_id, w.as_ptr(), v.clone()));
                }
            }
        }
        out
    }
}
