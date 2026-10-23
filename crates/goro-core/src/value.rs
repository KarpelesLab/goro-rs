use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::array::PhpArray;
use crate::generator::PhpGenerator;
use crate::object::PhpObject;
use crate::string::PhpString;

/// Convert an internal class name to a display name.
/// For anonymous classes (which contain a NUL byte separator): returns the part before the NUL.
/// For regular classes: returns the name as-is.
pub fn display_class_name(name: &[u8]) -> String {
    // Anonymous class names are formatted as "{Base}@anonymous\0{file}:{line}${counter}"
    // Display only the part before the NUL byte.
    if let Some(nul_pos) = name.iter().position(|&b| b == b'\x00') {
        String::from_utf8_lossy(&name[..nul_pos]).into_owned()
    } else {
        // Also handle legacy __anonymous_class_N names (for backward compatibility)
        let s = String::from_utf8_lossy(name);
        if s.contains("__anonymous_class_") {
            if let Some(pos) = s.find("__anonymous_class_") {
                let prefix = &s[..pos];
                format!("{}class@anonymous", prefix)
            } else {
                s.into_owned()
            }
        } else {
            s.into_owned()
        }
    }
}

thread_local! {
    /// Nesting depth of Value::compare / Value::equals.
    static COMPARE_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// PHP `precision` ini setting. Default 14. -1 means shortest roundtrip.
    static PHP_PRECISION: Cell<i32> = const { Cell::new(14) };
    /// PHP `serialize_precision` ini setting. Default -1 (shortest roundtrip).
    static PHP_SERIALIZE_PRECISION: Cell<i32> = const { Cell::new(-1) };
    /// Current memory usage in bytes (approximate tracking)
    static MEMORY_USAGE: Cell<usize> = const { Cell::new(0) };
    /// Memory limit in bytes (default 128MB, 0 = unlimited)
    static MEMORY_LIMIT: Cell<usize> = const { Cell::new(128 * 1024 * 1024) };
    /// Size of the allocation that hit the memory limit (0 = none pending);
    /// the VM turns it into the "Allowed memory size ... exhausted" fatal.
    static MEMORY_EXHAUSTED: Cell<usize> = const { Cell::new(0) };
    /// heap->overflow: the memory-limit fatal is being reported; the limit
    /// is not enforced meanwhile.
    static MEMORY_OVERFLOW: Cell<bool> = const { Cell::new(false) };
    /// heap->peak: the highest MEMORY_USAGE since start / the last
    /// memory_reset_peak_usage().
    static MEMORY_PEAK: Cell<usize> = const { Cell::new(0) };
    /// Custom heap handlers (zend_mm_set_custom_handlers()) observing the
    /// allocations the accounting sees.
    static MM_OBSERVER: Cell<Option<fn(MmEvent)>> = const { Cell::new(None) };
}

/// What custom heap handlers observe (see set_mm_observer()).
#[derive(Clone, Copy, Debug)]
pub enum MmEvent {
    /// A block of this many bytes is allocated.
    Alloc(usize),
    /// A block of this many bytes is freed.
    Free(usize),
    /// zend_mm_gc() (gc_mem_caches()) freed this many bytes.
    Gc(usize),
}

/// Install (or remove) custom heap handlers observing allocations.
pub fn set_mm_observer(f: Option<fn(MmEvent)>) {
    MM_OBSERVER.with(|o| o.set(f));
}

#[inline]
fn mm_observe(ev: MmEvent) {
    if let Some(f) = MM_OBSERVER.with(|o| o.get()) {
        f(ev);
    }
}

/// zend_mm_gc(): the allocator frees its cached memory (gc_mem_caches());
/// nothing is cached here. Returns the bytes freed.
pub fn memory_gc() -> usize {
    mm_observe(MmEvent::Gc(0));
    0
}

#[inline]
fn note_peak(total: usize) {
    MEMORY_PEAK.with(|p| {
        if total > p.get() {
            p.set(total);
        }
    });
}

/// Set when a comparison ran into a container already being compared
/// (zend_hash_compare / zend_std_compare_objects recursion protection); the
/// VM raises "Nesting level too deep - recursive dependency?".
/// (per thread: goro-httpd runs requests on several threads)
pub fn compare_recursion_set() {
    COMPARE_RECURSION.with(|c| c.set(true));
}

/// Take the COMPARE_RECURSION flag.
#[inline]
pub fn compare_recursion_take() -> bool {
    COMPARE_RECURSION.with(|c| c.replace(false))
}

thread_local! {
    static COMPARE_RECURSION: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    /// Arrays/objects (left operands) currently being compared.
    static CMP_ACTIVE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// GC_PROTECT_RECURSION on the left operand of a container comparison.
struct CmpGuard(usize);

impl CmpGuard {
    fn enter(p: usize) -> Option<CmpGuard> {
        let recursive = CMP_ACTIVE.with(|a| {
            let mut a = a.borrow_mut();
            if a.contains(&p) {
                true
            } else {
                a.push(p);
                false
            }
        });
        if recursive {
            compare_recursion_set();
            None
        } else {
            Some(CmpGuard(p))
        }
    }
}

impl Drop for CmpGuard {
    fn drop(&mut self) {
        CMP_ACTIVE.with(|a| {
            let mut a = a.borrow_mut();
            if let Some(i) = a.iter().rposition(|&x| x == self.0) {
                a.remove(i);
            }
        });
    }
}

/// EG(timed_out): set from the SIGPROF handler when max_execution_time
/// expires; the VM turns it into the "Maximum execution time" fatal.
pub static TIMED_OUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// EG(vm_interrupt): set from signal handlers (after TIMED_OUT for a
/// timeout); the VM checks it between instructions and then runs
/// `Vm::interrupt_hook` (zend_interrupt_function).
pub static VM_INTERRUPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// A signal that must not end a sleep (php-fpm's slowlog request, which
/// php-fpm makes with ptrace()): sleep() runs `Vm::interrupt_hook` and
/// sleeps on.
pub static TRANSPARENT_INTERRUPT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// EG(timeout_seconds): the limit the pending timer was armed with.
pub static TIMEOUT_SECONDS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// EG(hard_timeout): grace period after a timeout before the process dies.
pub static HARD_TIMEOUT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(2);

/// Pending memory-limit violation: the size of the failed allocation.
#[inline]
pub fn memory_exhausted() -> usize {
    MEMORY_EXHAUSTED.with(|c| c.get())
}

/// Set/clear heap->overflow (see MEMORY_OVERFLOW).
pub fn set_memory_overflow(on: bool) {
    MEMORY_OVERFLOW.with(|o| o.set(on));
}

/// Take the pending memory-limit violation (0 = none).
pub fn take_memory_exhausted() -> usize {
    MEMORY_EXHAUSTED.with(|c| c.replace(0))
}

/// The block size Zend's allocator hands out for an `n`-byte request: a
/// small-bin size up to 3072 bytes, whole 4 KiB pages above.
#[inline]
pub fn zend_mm_size(n: usize) -> usize {
    const BINS: [usize; 22] = [
        80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024, 1280, 1536,
        1792, 2048, 2560, 3072,
    ];
    if n <= 64 {
        n.max(1).next_multiple_of(8)
    } else if n <= 3072 {
        BINS[BINS.partition_point(|&b| b < n)]
    } else {
        n.next_multiple_of(4096)
    }
}

/// Track a memory allocation. Returns false if over limit.
pub fn memory_alloc(bytes: usize) -> bool {
    memory_alloc_reporting(bytes, bytes)
}

/// Like memory_alloc(), but a failure reports `reported` bytes as the size
/// that could not be allocated (growing a block in place reports the size
/// of the whole new block).
pub fn memory_alloc_reporting(bytes: usize, reported: usize) -> bool {
    MEMORY_USAGE.with(|m| {
        let current = m.get();
        let limit = MEMORY_LIMIT.with(|l| l.get());
        let new_total = current.saturating_add(bytes);
        // heap->overflow: while the memory-limit error is pending or
        // being reported, allocations go through (the error message, its
        // backtrace, ini lookups...).
        if limit > 0 && new_total > limit && raise_memory_exhausted(reported) {
            return false;
        }
        m.set(new_total);
        note_peak(new_total);
        mm_observe(MmEvent::Alloc(bytes));
        true
    })
}

/// Account an allocation that cannot fail (objects): it is always added;
/// going over the limit raises the pending memory-limit error.
pub fn memory_charge(bytes: usize) {
    memory_charge_reporting(bytes, bytes)
}

/// memory_charge() reporting `reported` bytes as the failed allocation.
pub fn memory_charge_reporting(bytes: usize, reported: usize) {
    MEMORY_USAGE.with(|m| {
        let new_total = m.get().saturating_add(bytes);
        let limit = MEMORY_LIMIT.with(|l| l.get());
        if limit > 0 && new_total > limit {
            raise_memory_exhausted(reported);
        }
        m.set(new_total);
        note_peak(new_total);
        mm_observe(MmEvent::Alloc(bytes));
    })
}

/// zend_mm_safe_error(): record a memory-limit violation unless one is
/// already pending or being reported (heap->overflow). Returns true when
/// it was recorded, i.e. the allocation must fail.
#[cold]
pub fn raise_memory_exhausted(reported: usize) -> bool {
    if MEMORY_OVERFLOW.with(|o| o.get()) {
        return false;
    }
    MEMORY_EXHAUSTED.with(|c| {
        if c.get() == 0 {
            c.set(reported.max(1));
            true
        } else {
            false
        }
    })
}

/// Track a memory deallocation.
pub fn memory_free(bytes: usize) {
    MEMORY_USAGE.with(|m| {
        let current = m.get();
        m.set(current.saturating_sub(bytes));
    });
    mm_observe(MmEvent::Free(bytes));
}

/// Get current memory usage.
pub fn memory_get_usage() -> usize {
    MEMORY_USAGE.with(|m| m.get())
}

/// heap->peak (memory_get_peak_usage()).
pub fn memory_get_peak() -> usize {
    MEMORY_PEAK.with(|p| p.get()).max(memory_get_usage())
}

/// memory_reset_peak_usage(): the peak restarts from the current usage.
pub fn memory_reset_peak() {
    let cur = memory_get_usage();
    MEMORY_PEAK.with(|p| p.set(cur));
}

/// heap->real_size / real_peak (memory_get_usage(true)): the memory
/// reserved from the system, in 2 MiB chunks.
pub fn memory_real_size(bytes: usize) -> usize {
    const CHUNK: usize = 2 * 1024 * 1024;
    bytes.div_ceil(CHUNK).max(1) * CHUNK
}

/// Get memory limit.
pub fn memory_get_limit() -> usize {
    MEMORY_LIMIT.with(|l| l.get())
}

/// Set memory limit. 0 = unlimited, -1 = unlimited.
pub fn set_memory_limit(bytes: i64) {
    let limit = if bytes <= 0 { 0 } else { bytes as usize };
    MEMORY_LIMIT.with(|l| l.set(limit));
    // The binary caps its address space (RLIMIT_AS, 2 GiB) as a safety net
    // for memory the accounting does not see; a larger memory_limit lifts
    // that cap so the script reaches memory_limit first.
    #[cfg(unix)]
    if limit > 1 << 30 {
        let want = (limit as u64).saturating_add(1 << 30);
        unsafe {
            let mut cur = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::getrlimit(libc::RLIMIT_AS, &mut cur) == 0
                && cur.rlim_cur != libc::RLIM_INFINITY
                && cur.rlim_cur < want
            {
                cur.rlim_cur = want.min(cur.rlim_max);
                libc::setrlimit(libc::RLIMIT_AS, &cur);
            }
        }
    }
}

/// Reset memory tracking (for test isolation).
pub fn memory_reset() {
    // The request's first VM stack page, the objects store and the
    // compiler's arena (init_compiler(): CG(arena)) are allocated at
    // startup.
    let vm_stack = crate::stack::vm_stack_reset() + crate::vm::OBJECT_STORE_INITIAL + 64 * 1024;
    MEMORY_USAGE.with(|m| m.set(vm_stack));
    MEMORY_PEAK.with(|p| p.set(vm_stack));
    MEMORY_OVERFLOW.with(|o| o.set(false));
    MEMORY_EXHAUSTED.with(|c| c.set(0));
}

/// Set the PHP `precision` ini value (used by float-to-string conversion).
pub fn set_php_precision(p: i32) {
    PHP_PRECISION.with(|c| c.set(p));
}

/// Get the current PHP `precision` ini value.
pub fn get_php_precision() -> i32 {
    PHP_PRECISION.with(|c| c.get())
}

/// Set the PHP `serialize_precision` ini value (used by var_dump, json_encode, etc.).
pub fn set_php_serialize_precision(p: i32) {
    PHP_SERIALIZE_PRECISION.with(|c| c.set(p));
}

/// Get the current PHP `serialize_precision` ini value.
pub fn get_php_serialize_precision() -> i32 {
    PHP_SERIALIZE_PRECISION.with(|c| c.get())
}

/// The core value type (equivalent to zval in PHP)
#[derive(Clone, Default)]
pub enum Value {
    Undef,
    #[default]
    Null,
    False,
    True,
    Long(i64),
    Double(f64),
    String(PhpString),
    Array(Rc<RefCell<PhpArray>>),
    Object(Rc<RefCell<PhpObject>>),
    /// A PHP reference (&$var) - both variables share the same Rc<RefCell<Value>>
    Reference(Rc<RefCell<Value>>),
    /// A PHP Generator object
    Generator(Rc<RefCell<PhpGenerator>>),
    /// A PHP resource (e.g. file handle, dir handle): a counted reference
    /// to the resource id. The resource_type/closed-state is looked up from
    /// RESOURCE_REGISTRY; when the last reference goes away, a resource with
    /// a registered destructor is queued for freeing (zend_list_free()).
    Resource(ResourceRef),
}

/// A counted reference to a resource id (the zend_resource refcount). All
/// live references to an id share one allocation (looked up through
/// `LIVE_RESOURCES`), so the count is right however the value was built.
#[derive(Clone)]
pub struct ResourceRef(Rc<ResourceCell>);

struct ResourceCell {
    id: i64,
}

thread_local! {
    /// id -> the shared cell of its live references.
    static LIVE_RESOURCES: RefCell<std::collections::HashMap<i64, std::rc::Weak<ResourceCell>>> =
        RefCell::new(std::collections::HashMap::new());
    /// Resources that are freed when their last reference goes away, with
    /// their destructor (the list entry's rsrc_dtor).
    static RESOURCE_DTORS: RefCell<std::collections::HashMap<i64, ResourceDtor>> =
        RefCell::new(std::collections::HashMap::new());
    /// Released resources whose destructor is still to run.
    static FREED_RESOURCES: RefCell<Vec<(i64, ResourceDtor)>> = const { RefCell::new(Vec::new()) };
    static FREED_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl ResourceRef {
    /// A reference to resource `id`.
    pub fn new(id: i64) -> Self {
        LIVE_RESOURCES
            .try_with(|m| {
                let mut m = m.borrow_mut();
                if let Some(rc) = m.get(&id).and_then(|w| w.upgrade()) {
                    return ResourceRef(rc);
                }
                let rc = Rc::new(ResourceCell { id });
                m.insert(id, Rc::downgrade(&rc));
                ResourceRef(rc)
            })
            .unwrap_or_else(|_| ResourceRef(Rc::new(ResourceCell { id })))
    }

    #[inline]
    pub fn id(&self) -> i64 {
        self.0.id
    }

    /// Number of live references to this resource.
    pub fn refcount(&self) -> usize {
        Rc::strong_count(&self.0)
    }
}

impl Drop for ResourceCell {
    fn drop(&mut self) {
        let id = self.id;
        let _ = LIVE_RESOURCES.try_with(|m| {
            if let Ok(mut m) = m.try_borrow_mut() {
                if m.get(&id).is_some_and(|w| w.strong_count() == 0) {
                    m.remove(&id);
                }
            }
        });
        let dtor = RESOURCE_DTORS
            .try_with(|d| d.try_borrow_mut().ok().and_then(|mut d| d.remove(&id)))
            .ok()
            .flatten();
        if let Some(dtor) = dtor {
            let _ = FREED_RESOURCES.try_with(|f| f.borrow_mut().push((id, dtor)));
            let _ = FREED_PENDING.try_with(|p| p.set(true));
        }
    }
}

/// What `Value::resource()` accepts: a resource id or a reference to one.
pub trait IntoResourceRef {
    fn into_resource_ref(self) -> ResourceRef;
}
impl IntoResourceRef for i64 {
    fn into_resource_ref(self) -> ResourceRef {
        ResourceRef::new(self)
    }
}
impl IntoResourceRef for &i64 {
    fn into_resource_ref(self) -> ResourceRef {
        ResourceRef::new(*self)
    }
}
impl IntoResourceRef for ResourceRef {
    fn into_resource_ref(self) -> ResourceRef {
        self
    }
}
impl IntoResourceRef for &ResourceRef {
    fn into_resource_ref(self) -> ResourceRef {
        self.clone()
    }
}

impl std::fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

impl std::fmt::Debug for ResourceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

impl PartialEq for ResourceRef {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl Eq for ResourceRef {}

impl std::hash::Hash for ResourceRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state)
    }
}

/// A resource destructor (zend_rsrc_list_dtors_entry.list_dtor_ex), run by
/// the VM at the release point after the last reference went away.
pub type ResourceDtor = fn(&mut crate::vm::Vm, i64);

/// The resource `id` is freed by `dtor` when its last reference goes away
/// (zend_list_free()), unless it was closed before.
pub fn set_resource_dtor(id: i64, dtor: ResourceDtor) {
    RESOURCE_DTORS.with(|d| d.borrow_mut().insert(id, dtor));
}

/// The resource `id` is no longer freed by its references (it was closed,
/// zend_list_close()); returns the destructor it had.
pub fn clear_resource_dtor(id: i64) -> Option<ResourceDtor> {
    RESOURCE_DTORS
        .try_with(|d| d.borrow_mut().remove(&id))
        .ok()
        .flatten()
}

/// The resources whose destructor has not run yet (zend_close_rsrc_list()
/// at shutdown), most recent last.
pub fn take_resource_dtors() -> Vec<(i64, ResourceDtor)> {
    let mut v: Vec<(i64, ResourceDtor)> = RESOURCE_DTORS.with(|d| d.borrow_mut().drain().collect());
    v.sort_by_key(|(id, _)| *id);
    v
}

thread_local! {
    /// Destructors by resource type, for the resources handed to PHP code
    /// (see `adopt_new_resource`).
    static TYPE_DTORS: RefCell<Vec<(&'static [u8], ResourceDtor)>> = const { RefCell::new(Vec::new()) };
}

/// Resources of type `ty` returned to PHP code by a builtin are freed by
/// `dtor` when their last reference goes away.
pub fn register_resource_type_dtor(ty: &'static [u8], dtor: ResourceDtor) {
    TYPE_DTORS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|(k, _)| *k != ty);
        t.push((ty, dtor));
    });
}

/// The id the next resource will get.
#[inline]
pub fn next_resource_id() -> i64 {
    NEXT_RESOURCE_ID.with(|n| n.get())
}

/// A builtin called from PHP code created resource `id` and returned it:
/// from now on its references own it (its type's destructor runs when the
/// last one goes away). Resources the engine keeps by id internally are
/// never handed over this way.
#[cold]
pub fn adopt_new_resource(id: i64) {
    let ty = RESOURCE_REGISTRY.with(|r| {
        r.borrow()
            .get(&id)
            .filter(|e| !e.closed)
            .map(|e| e.resource_type.clone())
    });
    let Some(ty) = ty else { return };
    let Some(dtor) = TYPE_DTORS.with(|t| {
        t.borrow()
            .iter()
            .find(|(k, _)| *k == ty.as_slice())
            .map(|(_, d)| *d)
    }) else {
        return;
    };
    RESOURCE_DTORS.with(|d| {
        d.borrow_mut().entry(id).or_insert(dtor);
    });
}

/// A builtin (function or method) started when the next resource id was
/// `first` and returned `r`: the resources it created and returned, as the
/// value or as an element of a returned array (stream_socket_pair()), are
/// owned by the zvals holding them from now on, as in C.
#[inline]
pub fn adopt_returned_resources(first: i64, r: &Value) {
    if next_resource_id() == first {
        return;
    }
    adopt_returned_resources_slow(first, r);
}

#[cold]
#[inline(never)]
fn adopt_returned_resources_slow(first: i64, r: &Value) {
    match r {
        Value::Resource(res) if res.id() >= first => adopt_new_resource(res.id()),
        Value::Array(a) => {
            for (_, v) in a.borrow().iter() {
                if let Value::Resource(res) = v.deref() {
                    if res.id() >= first {
                        adopt_new_resource(res.id());
                    }
                }
            }
        }
        _ => {}
    }
}

/// Call builtin `f` from Rust code the way PHP code calls it: a resource
/// it creates and returns (a stream, a stream context, a socket) is freed
/// with the last zval holding it, not only at shutdown.
pub fn call_builtin_adopting(
    vm: &mut crate::vm::Vm,
    f: crate::vm::BuiltinFn,
    args: &[Value],
) -> Result<Value, crate::vm::VmError> {
    let first = next_resource_id();
    let r = f(vm, args);
    if let Ok(v) = &r {
        adopt_returned_resources(first, v);
    }
    r
}

/// Whether released resources wait for their destructor.
#[inline]
pub fn has_freed_resources() -> bool {
    FREED_PENDING.with(|p| p.get())
}

/// Take the released resources whose destructor is to run, in release order.
pub fn take_freed_resources() -> Vec<(i64, ResourceDtor)> {
    FREED_PENDING.with(|p| p.set(false));
    FREED_RESOURCES.with(|f| std::mem::take(&mut *f.borrow_mut()))
}

thread_local! {
    /// Registry mapping resource id -> type name. When fopen/curl_init/etc.
    /// allocate a handle id, they register it here so `is_resource`,
    /// `get_resource_type`, and `gettype` report the value as a resource.
    /// Removing an entry (on fclose etc.) marks the resource as closed.
    static RESOURCE_REGISTRY: RefCell<std::collections::HashMap<i64, ResourceRegistryEntry>> =
        RefCell::new(std::collections::HashMap::new());

    /// Registry of close callbacks for resources. When fclose() (or similar)
    /// is invoked, any registered callback is invoked exactly once and removed.
    /// This lets extensions (e.g. zlib's gzopen handle) cleanly flush data on
    /// fclose without requiring fclose to know about every extension's handle
    /// types.
    static RESOURCE_CLOSE_HOOKS: RefCell<std::collections::HashMap<i64, Box<dyn FnOnce()>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Register a closure to run when the resource id is closed (via close_resource).
/// The closure will be called exactly once. Any previously registered hook is replaced.
pub fn register_close_hook(id: i64, hook: Box<dyn FnOnce()>) {
    RESOURCE_CLOSE_HOOKS.with(|r| {
        r.borrow_mut().insert(id, hook);
    });
}

/// Invoke (and remove) the close hook for a resource id, if registered.
pub fn run_close_hook(id: i64) {
    let hook = RESOURCE_CLOSE_HOOKS.with(|r| r.borrow_mut().remove(&id));
    if let Some(h) = hook {
        h();
    }
}

#[derive(Clone, Debug)]
struct ResourceRegistryEntry {
    resource_type: Vec<u8>,
    closed: bool,
}

thread_local! {
    /// EG(regular_list) next free index: every resource kind shares it, ids
    /// are never reused. 1-3 are the CLI's STDIN/STDOUT/STDERR.
    static NEXT_RESOURCE_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(4) };
}

/// Allocate the next resource id (zend_list_insert()).
pub fn alloc_resource_id() -> i64 {
    NEXT_RESOURCE_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

thread_local! {
    static DEFAULT_CONTEXT_HOOK: std::cell::Cell<Option<fn() -> i64>> =
        const { std::cell::Cell::new(None) };
}

/// Installed by the streams extension: allocates FG(default_context).
pub fn set_default_context_hook(f: fn() -> i64) {
    DEFAULT_CONTEXT_HOOK.with(|h| h.set(Some(f)));
}

/// php_stream_context_from_zval(NULL, 0) from another extension: make sure
/// the default stream context (and its resource id) exists.
pub fn touch_default_stream_context() {
    if let Some(f) = DEFAULT_CONTEXT_HOOK.with(|h| h.get()) {
        f();
    }
}

/// Mark an integer id as representing a resource of the given type.
pub fn register_resource(id: i64, resource_type: &[u8]) {
    RESOURCE_REGISTRY.with(|r| {
        r.borrow_mut().insert(
            id,
            ResourceRegistryEntry {
                resource_type: resource_type.to_vec(),
                closed: false,
            },
        );
    });
}

/// Mark a resource as closed (for fclose/curl_close semantics). Also invokes
/// any registered close hook.
pub fn close_resource(id: i64) {
    clear_resource_dtor(id);
    run_close_hook(id);
    RESOURCE_REGISTRY.with(|r| {
        if let Some(entry) = r.borrow_mut().get_mut(&id) {
            entry.closed = true;
        }
    });
}

/// Completely remove a resource from the registry.
pub fn remove_resource(id: i64) {
    RESOURCE_REGISTRY.with(|r| {
        r.borrow_mut().remove(&id);
    });
}

/// Clear all registered resources. Intended for resetting state between
/// independent test invocations so literal integers from one test are not
/// mis-identified as resources leaked from a previous test.
pub fn reset_resources() {
    RESOURCE_REGISTRY.with(|r| {
        r.borrow_mut().clear();
    });
}

/// Return the registered resource type name for this id, or None if not registered.
pub fn resource_type_of(id: i64) -> Option<Vec<u8>> {
    RESOURCE_REGISTRY.with(|r| r.borrow().get(&id).map(|e| e.resource_type.clone()))
}

/// True iff id is registered as a resource (even if closed).
pub fn is_registered_resource(id: i64) -> bool {
    RESOURCE_REGISTRY.with(|r| r.borrow().contains_key(&id))
}

/// EG(regular_list) for get_resources(): the registered resources still
/// referenced from PHP values, plus the open ones the engine keeps by id;
/// as (id, type name or None when closed), by id.
pub fn live_resources() -> Vec<(i64, Option<Vec<u8>>)> {
    let referenced: std::collections::HashSet<i64> = LIVE_RESOURCES
        .try_with(|m| {
            m.borrow()
                .iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(id, _)| *id)
                .collect()
        })
        .unwrap_or_default();
    let mut v: Vec<(i64, Option<Vec<u8>>)> = RESOURCE_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .filter(|(id, e)| !e.closed || referenced.contains(id))
            .map(|(id, e)| (*id, (!e.closed).then(|| e.resource_type.clone())))
            .collect()
    });
    v.sort_by_key(|(id, _)| *id);
    v
}

thread_local! {
    /// Streams PHP code may not fclose() (PHP_STREAM_FLAG_NO_FCLOSE): the
    /// ones an object owns (SplFileObject, XMLWriter, ...).
    static NO_FCLOSE: RefCell<std::collections::HashSet<i64>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Mark (or unmark) stream `id` as owned by an object: fclose() refuses it.
pub fn set_resource_no_fclose(id: i64, on: bool) {
    let _ = NO_FCLOSE.try_with(|s| {
        let mut s = s.borrow_mut();
        if on {
            s.insert(id);
        } else {
            s.remove(&id);
        }
    });
}

/// Whether fclose() must refuse stream `id` (see set_resource_no_fclose).
pub fn is_resource_no_fclose(id: i64) -> bool {
    NO_FCLOSE
        .try_with(|s| {
            let s = s.borrow();
            !s.is_empty() && s.contains(&id)
        })
        .unwrap_or(false)
}

/// True iff id is registered as a resource AND has not been closed.
pub fn is_open_resource(id: i64) -> bool {
    RESOURCE_REGISTRY.with(|r| r.borrow().get(&id).map(|e| !e.closed).unwrap_or(false))
}

/// ZEND_THREEWAY_COMPARE for doubles: an unordered pair (NAN) compares as 1.
fn float_threeway(a: f64, b: f64) -> i64 {
    if a == b {
        0
    } else if a < b {
        -1
    } else {
        1
    }
}

impl Value {
    /// Dereference a value: if it's a Reference, return the inner value; otherwise return self.
    /// This is used by read_operand so existing code transparently reads through references.
    pub fn deref(&self) -> Value {
        match self {
            Value::Reference(r) => r.borrow().clone(),
            other => other.clone(),
        }
    }

    /// The dereferenced value for a container to keep (an argument stored
    /// by value: array_push(), SplDoublyLinkedList::push()...). Arrays are
    /// copied on assignment rather than separated on write, so an array
    /// still shared with the caller's variable is copied: a later write
    /// to the variable must not show through the stored element.
    pub fn deref_for_storage(&self) -> Value {
        match self.deref() {
            Value::Array(a) if Rc::strong_count(&a) > 1 => {
                let copy = a.borrow().clone();
                Value::Array(Rc::new(RefCell::new(copy)))
            }
            v => v,
        }
    }

    /// If this is a `Closure` object, return its underlying callable
    /// (the legacy closure representation used for dispatch).
    pub fn closure_inner(&self) -> Option<Value> {
        match self {
            Value::Object(o) => o.borrow().closure.as_ref().map(|c| (**c).clone()),
            Value::Reference(r) => r.borrow().closure_inner(),
            _ => None,
        }
    }

    /// Whether this value is a `Closure` object.
    pub fn is_closure_obj(&self) -> bool {
        match self {
            Value::Object(o) => o.borrow().closure.is_some(),
            Value::Reference(r) => r.borrow().is_closure_obj(),
            _ => false,
        }
    }

    /// Replace a `Closure` object by its underlying callable; any other value
    /// is returned unchanged (cloned).
    pub fn unwrap_closure(&self) -> Value {
        self.closure_inner().unwrap_or_else(|| self.clone())
    }

    /// Like `unwrap_closure`, but a bound-method closure (`["Class::m", $obj]`)
    /// is returned in the standard `[$obj, "m"]` array-callable form, which
    /// generic callback dispatchers understand.
    pub fn closure_callable(&self) -> Value {
        let inner = match self.closure_inner() {
            Some(i) => i,
            None => {
                // `[$closure, '__invoke']` is the closure itself.
                if let Value::Array(arr) = self {
                    let a = arr.borrow();
                    if a.len() == 2 {
                        if let (Some(c), Some(Value::String(m))) = (a.get_int(0), a.get_int(1)) {
                            if c.is_closure_obj() && m.as_bytes().eq_ignore_ascii_case(b"__invoke")
                            {
                                let c = c.deref();
                                drop(a);
                                return c.closure_callable();
                            }
                        }
                    }
                }
                return self.clone();
            }
        };
        if let Value::Array(arr) = &inner {
            let a = arr.borrow();
            if a.len() == 2 {
                if let (Some(Value::String(n)), Some(Value::Object(o))) =
                    (a.get_int(0), a.get_int(1))
                {
                    let b = n.as_bytes();
                    if !b.starts_with(b"__") {
                        if let Some(p) = b.windows(2).position(|w| w == b"::") {
                            let mut out = crate::array::PhpArray::new();
                            out.push(Value::Object(o.clone()));
                            out.push(Value::String(PhpString::from_vec(b[p + 2..].to_vec())));
                            return Value::Array(Rc::new(RefCell::new(out)));
                        }
                    }
                }
            }
        }
        inner
    }

    /// Returns true if this value is a Reference
    pub fn is_reference(&self) -> bool {
        matches!(self, Value::Reference(_))
    }

    // ---- Truthiness ----

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undef | Value::Null | Value::False => false,
            Value::True => true,
            Value::Long(n) => *n != 0,
            Value::Double(f) => *f != 0.0,
            Value::String(s) => {
                let data = s.as_bytes();
                !data.is_empty() && data != b"0"
            }
            Value::Array(arr) => !arr.borrow().is_empty(),
            Value::Object(o) => match native_cast(o, 0) {
                Some(v) => v.is_truthy(),
                None => true,
            },
            Value::Generator(_) => true,
            Value::Reference(r) => r.borrow().is_truthy(),
            Value::Resource(_) => true,
        }
    }

    // ---- Type name ----

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undef => "NULL",
            Value::Null => "NULL",
            Value::False | Value::True => "bool",
            Value::Long(_) => "int",
            Value::Double(_) => "float",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) | Value::Generator(_) => "object",
            Value::Reference(r) => r.borrow().type_name(),
            Value::Resource(id) => {
                if is_open_resource(id.id()) {
                    "resource"
                } else {
                    "resource (closed)"
                }
            }
        }
    }

    /// A (counted) reference to the resource `id`.
    #[inline]
    pub fn resource(id: impl IntoResourceRef) -> Value {
        Value::Resource(id.into_resource_ref())
    }

    /// Returns true if this value is a resource (open or closed).
    pub fn is_resource(&self) -> bool {
        match self {
            Value::Resource(_) => true,
            Value::Reference(r) => r.borrow().is_resource(),
            _ => false,
        }
    }

    /// Returns the resource id if this is a resource, or -1 otherwise.
    pub fn resource_id(&self) -> i64 {
        match self {
            Value::Resource(n) => n.id(),
            Value::Reference(r) => r.borrow().resource_id(),
            _ => -1,
        }
    }

    /// Returns the resource type name (e.g. "stream") if this is a resource.
    pub fn resource_type_name(&self) -> Option<Vec<u8>> {
        match self {
            Value::Resource(n) => resource_type_of(n.id()),
            Value::Reference(r) => r.borrow().resource_type_name(),
            _ => None,
        }
    }

    // ---- Conversion helpers ----

    /// Convert to long using PHP's zval_get_long() semantics for typed parameters.
    /// For strings, this first parses as float (handling scientific notation like "2.1e1")
    /// then truncates to int. This differs from to_long() which does (int) cast semantics.
    pub fn to_long_coerced(&self) -> i64 {
        match self {
            Value::String(s) => {
                let bytes = s.as_bytes();
                if let Some(f) = parse_numeric_string(bytes) {
                    f as i64
                } else {
                    self.to_long()
                }
            }
            Value::Reference(r) => r.borrow().to_long_coerced(),
            _ => self.to_long(),
        }
    }

    pub fn to_long(&self) -> i64 {
        match self {
            Value::Undef | Value::Null | Value::False => 0,
            Value::True => 1,
            Value::Long(n) => *n,
            Value::Resource(n) => n.id(),
            Value::Double(f) => php_dval_to_lval(*f),
            Value::String(s) => php_str_to_lval(s.as_bytes()),
            Value::Array(arr) => {
                if arr.borrow().is_empty() {
                    0
                } else {
                    1
                }
            }
            Value::Object(o) => match native_cast(o, 1) {
                Some(v) => v.to_long(),
                None => 1,
            },
            Value::Generator(_) => 1,
            Value::Reference(r) => r.borrow().to_long(),
        }
    }

    pub fn to_double(&self) -> f64 {
        match self {
            Value::Undef | Value::Null | Value::False => 0.0,
            Value::True => 1.0,
            Value::Long(n) => *n as f64,
            Value::Resource(n) => n.id() as f64,
            Value::Double(f) => *f,
            Value::String(s) => {
                let str = s.to_string_lossy();
                let str = str.trim();
                // PHP parses leading numeric portion
                parse_leading_float(str)
            }
            Value::Array(arr) => {
                if arr.borrow().is_empty() {
                    0.0
                } else {
                    1.0
                }
            }
            Value::Object(o) => match native_cast(o, 2) {
                Some(v) => v.to_double(),
                None => 1.0,
            },
            Value::Generator(_) => 1.0,
            Value::Reference(r) => r.borrow().to_double(),
        }
    }

    pub fn to_php_string(&self) -> PhpString {
        match self {
            Value::Undef | Value::Null => PhpString::empty(),
            Value::False => PhpString::empty(),
            Value::True => PhpString::from_bytes(b"1"),
            Value::Long(n) => PhpString::from_string(n.to_string()),
            Value::Resource(n) => PhpString::from_string(format!("Resource id #{}", n)),
            Value::Double(f) => PhpString::from_string(format_php_float(*f)),
            Value::String(s) => s.clone(),
            Value::Array(_) => PhpString::from_bytes(b"Array"),
            Value::Object(obj) => {
                if let Some(v) = native_cast(obj, 3) {
                    return v.to_php_string();
                }
                let obj = obj.borrow();
                // SimpleXMLElement: __toString returns text content
                if obj.class_name.eq_ignore_ascii_case(b"SimpleXMLElement") {
                    let text = obj.get_property(b"0");
                    if matches!(text, Value::Null | Value::Undef) {
                        return PhpString::empty();
                    }
                    return text.to_php_string();
                }
                // PHP tries __toString magic method; for now return class name
                PhpString::from_vec(obj.class_name.clone())
            }
            Value::Generator(_) => PhpString::from_bytes(b"Generator"),
            Value::Reference(r) => r.borrow().to_php_string(),
        }
    }

    pub fn to_bool(&self) -> bool {
        self.is_truthy()
    }

    // ---- Arithmetic helpers ----

    pub fn add(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().add(other);
        }
        if let Value::Reference(r) = other {
            return self.add(&r.borrow());
        }
        match (self, other) {
            // Array + Array = array union
            (Value::Array(a), Value::Array(b)) => {
                let a = a.borrow();
                let b = b.borrow();
                let mut result = crate::array::PhpArray::new();
                // Copy all entries from a
                for (key, val) in a.iter() {
                    result.set(key.clone(), val.clone());
                }
                // Add entries from b that don't exist in a
                for (key, val) in b.iter() {
                    if !result.contains_key(key) {
                        result.set(key.clone(), val.clone());
                    }
                }
                Value::Array(std::rc::Rc::new(std::cell::RefCell::new(result)))
            }
            (Value::Long(a), Value::Long(b)) => match a.checked_add(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 + *b as f64),
            },
            (Value::Double(a), Value::Double(b)) => Value::Double(a + b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 + b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a + *b as f64),
            _ => {
                // Type juggling: convert to numeric
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.add(&b)
            }
        }
    }

    pub fn sub(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().sub(other);
        }
        if let Value::Reference(r) = other {
            return self.sub(&r.borrow());
        }
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => match a.checked_sub(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 - *b as f64),
            },
            (Value::Double(a), Value::Double(b)) => Value::Double(a - b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 - b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a - *b as f64),
            _ => {
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.sub(&b)
            }
        }
    }

    pub fn mul(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().mul(other);
        }
        if let Value::Reference(r) = other {
            return self.mul(&r.borrow());
        }
        match (self, other) {
            (Value::Long(a), Value::Long(b)) => match a.checked_mul(*b) {
                Some(result) => Value::Long(result),
                None => Value::Double(*a as f64 * *b as f64),
            },
            (Value::Double(a), Value::Double(b)) => Value::Double(a * b),
            (Value::Long(a), Value::Double(b)) => Value::Double(*a as f64 * b),
            (Value::Double(a), Value::Long(b)) => Value::Double(a * *b as f64),
            _ => {
                let a = self.to_numeric();
                let b = other.to_numeric();
                a.mul(&b)
            }
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value, &'static str> {
        if let Value::Reference(r) = self {
            return r.borrow().div(other);
        }
        if let Value::Reference(r) = other {
            return self.div(&r.borrow());
        }
        // div_function: operands are converted to numbers first; long/long
        // stays a long when the division is exact.
        let an = self.to_numeric();
        let bn = other.to_numeric();
        if let (Value::Long(x), Value::Long(y)) = (&an, &bn) {
            if *y == 0 {
                return Err("Division by zero");
            }
            if *y == -1 && *x == i64::MIN {
                return Ok(Value::Double(*x as f64 / -1.0));
            }
            if x % y == 0 {
                return Ok(Value::Long(x / y));
            }
            return Ok(Value::Double(*x as f64 / *y as f64));
        }
        let a = an.to_double();
        let b = bn.to_double();
        if b == 0.0 {
            return Err("Division by zero");
        }
        Ok(Value::Double(a / b))
    }

    pub fn modulo(&self, other: &Value) -> Result<Value, &'static str> {
        if let Value::Reference(r) = self {
            return r.borrow().modulo(other);
        }
        if let Value::Reference(r) = other {
            return self.modulo(&r.borrow());
        }
        let a = self.to_long();
        let b = other.to_long();
        if b == 0 {
            return Err("Modulo by zero");
        }
        // Handle overflow case: i64::MIN % -1 panics in Rust but PHP returns 0
        if a == i64::MIN && b == -1 {
            Ok(Value::Long(0))
        } else {
            Ok(Value::Long(a % b))
        }
    }

    pub fn pow(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().pow(other);
        }
        if let Value::Reference(r) = other {
            return self.pow(&r.borrow());
        }
        // zendi_try_convert_scalar_to_number(): null/bool/numeric strings
        // take part as int or float.
        let num = |v: &Value| match v {
            Value::Long(_) | Value::Double(_) => None,
            Value::Null | Value::Undef | Value::True | Value::False | Value::String(_) => {
                Some(v.to_numeric())
            }
            _ => None,
        };
        let (a_conv, b_conv) = (num(self), num(other));
        if a_conv.is_some() || b_conv.is_some() {
            let a = a_conv.as_ref().unwrap_or(self);
            let b = b_conv.as_ref().unwrap_or(other);
            return a.pow(b);
        }
        match (self, other) {
            (Value::Long(base), Value::Long(exp)) if *exp >= 0 => pow_long(*base, *exp),
            _ => Value::Double(self.to_double().powf(other.to_double())),
        }
    }

    pub fn concat(&self, other: &Value) -> Value {
        if let Value::Reference(r) = self {
            return r.borrow().concat(other);
        }
        if let Value::Reference(r) = other {
            return self.concat(&r.borrow());
        }
        let a = self.to_php_string();
        let b = other.to_php_string();
        let mut result = a.as_bytes().to_vec();
        result.extend_from_slice(b.as_bytes());
        Value::String(PhpString::from_vec(result))
    }

    pub fn negate(&self) -> Value {
        match self {
            Value::Reference(r) => r.borrow().negate(),
            Value::Long(n) => match n.checked_neg() {
                Some(neg) => Value::Long(neg),
                None => Value::Double(-(*n as f64)),
            },
            Value::Double(f) => Value::Double(-f),
            _ => {
                let n = self.to_numeric();
                n.negate()
            }
        }
    }

    /// Convert to numeric type (int or float), PHP-style
    fn to_numeric(&self) -> Value {
        match self {
            Value::Long(_) | Value::Double(_) => self.clone(),
            Value::Resource(n) => Value::Long(n.id()),
            Value::True => Value::Long(1),
            Value::False | Value::Null | Value::Undef => Value::Long(0),
            Value::String(s) => match parse_numeric_str(s.as_bytes(), true) {
                Some((NumStr::Long(n), _)) => Value::Long(n),
                Some((NumStr::Double(f), _)) => Value::Double(f),
                None => Value::Long(0),
            },
            Value::Object(o) if native_cast(o, 3).is_some() => {
                native_cast_number(o).unwrap_or(Value::Long(0))
            }
            Value::Array(_) | Value::Object(_) | Value::Generator(_) => {
                Value::Long(if self.is_truthy() { 1 } else { 0 })
            }
            Value::Reference(r) => r.borrow().to_numeric(),
        }
    }

    // ---- Comparison ----

    /// PHP == comparison
    pub fn equals(&self, other: &Value) -> bool {
        let depth = COMPARE_DEPTH.with(|d| {
            let n = d.get();
            d.set(n + 1);
            n
        });
        let r = if depth > 1000 {
            compare_recursion_set();
            false
        } else {
            self.equals_inner(other)
        };
        COMPARE_DEPTH.with(|d| d.set(depth));
        r
    }

    fn equals_inner(&self, other: &Value) -> bool {
        if let Value::Reference(r) = self {
            return r.borrow().equals(other);
        }
        if let Value::Reference(r) = other {
            return self.equals(&r.borrow());
        }
        if let Some(c) = enum_compare(self, other) {
            return c == 0;
        }
        if let Some(c) = native_compare(self, other) {
            return c == 0;
        }
        match (self, other) {
            (Value::Null | Value::Undef, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::False)
            | (Value::False, Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::String(s))
            | (Value::String(s), Value::Null | Value::Undef) => s.as_bytes().is_empty(),
            (Value::Null | Value::Undef, Value::Long(0))
            | (Value::Long(0), Value::Null | Value::Undef) => true,
            (Value::Null | Value::Undef, Value::Double(f))
            | (Value::Double(f), Value::Null | Value::Undef) => *f == 0.0,
            (Value::Null | Value::Undef, Value::Array(a))
            | (Value::Array(a), Value::Null | Value::Undef) => a.borrow().is_empty(),
            (Value::Null | Value::Undef, _) | (_, Value::Null | Value::Undef) => false,
            (Value::True, other) | (other, Value::True) => other.is_truthy(),
            (Value::False, other) | (other, Value::False) => !other.is_truthy(),
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::Long(a), Value::Double(b)) | (Value::Double(b), Value::Long(a)) => {
                *a as f64 == *b
            }
            // Generator objects have no properties: any two compare equal
            // (zend_std_compare_objects()).
            (Value::Generator(_), Value::Generator(_)) => true,
            (Value::String(a), Value::String(b)) => smart_streq(a.as_bytes(), b.as_bytes()),
            (Value::String(s), Value::Long(n)) | (Value::Long(n), Value::String(s)) => {
                // PHP 8: if string is numeric, compare numerically; otherwise false
                let s_bytes = s.as_bytes();
                if let Some(num) = parse_numeric_string(s_bytes) {
                    num == *n as f64
                } else {
                    false
                }
            }
            (Value::String(s), Value::Double(f)) | (Value::Double(f), Value::String(s)) => {
                let s_bytes = s.as_bytes();
                if let Some(num) = parse_numeric_string(s_bytes) {
                    num == *f
                } else {
                    // compare_double_to_string(): a non-numeric string is
                    // compared with the float's string form ("INF" == INF).
                    !f.is_nan() && format_php_float(*f).as_bytes() == s_bytes
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                // Same hash table: equal (zend_hash_compare's ht1 == ht2 shortcut).
                if std::rc::Rc::ptr_eq(a, b) {
                    return true;
                }
                let Some(_guard) = CmpGuard::enter(Rc::as_ptr(a) as *const () as usize) else {
                    return false;
                };
                let a = a.borrow();
                let b = b.borrow();
                if a.len() != b.len() {
                    return false;
                }
                for (key, a_val) in a.iter() {
                    match b.get(key) {
                        Some(b_val) => {
                            if !a_val.equals(b_val) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (Value::Object(a), Value::Object(b)) => {
                // Same object reference
                if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
                    return true;
                }
                if a.borrow().closure.is_some() || b.borrow().closure.is_some() {
                    return crate::closure::closures_equal(&a.borrow(), &b.borrow());
                }
                // Z_PROTECT_RECURSION on the left object.
                let Some(_guard) = CmpGuard::enter(a.as_ptr() as *const () as usize) else {
                    return false;
                };
                {
                    let a_b = a.borrow();
                    let b_b = b.borrow();
                    // Built-in DateTime hierarchy: compare by timestamp.
                    // Use presence of internal __timestamp property as a marker.
                    let a_has_ts =
                        !matches!(a_b.get_property(b"__timestamp"), Value::Null | Value::Undef);
                    let b_has_ts =
                        !matches!(b_b.get_property(b"__timestamp"), Value::Null | Value::Undef);
                    let a_has_tzt = !matches!(
                        a_b.get_property(b"timezone_type"),
                        Value::Null | Value::Undef
                    );
                    let b_has_tzt = !matches!(
                        b_b.get_property(b"timezone_type"),
                        Value::Null | Value::Undef
                    );
                    if a_has_ts && b_has_ts && a_has_tzt && b_has_tzt {
                        let ta = a_b.get_property(b"__timestamp").to_long();
                        let tb = b_b.get_property(b"__timestamp").to_long();
                        ta == tb
                            && a_b.get_property(b"__timestamp_us").to_long()
                                == b_b.get_property(b"__timestamp_us").to_long()
                    } else if a_b.has_property(b"__timestamp_tzobj")
                        && b_b.has_property(b"__timestamp_tzobj")
                    {
                        // ext/date DateTimeZone: same kind and same zone
                        a_b.get_property(b"timezone_type").to_long()
                            == b_b.get_property(b"timezone_type").to_long()
                            && a_b.get_property(b"timezone").to_php_string().as_bytes()
                                == b_b.get_property(b"timezone").to_php_string().as_bytes()
                    } else if a_b.has_property(b"__timestamp_ival")
                        && b_b.has_property(b"__timestamp_ival")
                    {
                        // ext/date DateInterval objects are not comparable
                        false
                    } else if a_b.has_property(b"__timestamp_period")
                        && b_b.has_property(b"__timestamp_period")
                        && a_b.class_name.eq_ignore_ascii_case(&b_b.class_name)
                    {
                        // ext/date DatePeriod: zend_std_compare_objects() of
                        // the property slots, which its fields are not (they
                        // are shown by its get_properties handler): only the
                        // dynamic properties count.
                        let dynamic = |o: &PhpObject| -> Vec<(Vec<u8>, Value)> {
                            o.properties
                                .iter()
                                .filter(|(k, _)| {
                                    !k.starts_with(b"__timestamp")
                                        && !matches!(
                                            k.as_slice(),
                                            b"start"
                                                | b"current"
                                                | b"end"
                                                | b"interval"
                                                | b"recurrences"
                                                | b"include_start_date"
                                                | b"include_end_date"
                                        )
                                })
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        };
                        let (av, bv) = (dynamic(&a_b), dynamic(&b_b));
                        av.len() == bv.len()
                            && av.iter().all(|(k, v)| v.equals(&b_b.get_property(k)))
                    } else if a_b.class_name.eq_ignore_ascii_case(&b_b.class_name)
                        && (crate::spl_dll::is_spl_array_object(&a_b)
                            || crate::spl_dll::is_spl_array_object(&b_b))
                    {
                        // spl_array_compare_objects(): the storages, then the
                        // (non-internal) properties.
                        let visible = |o: &PhpObject| -> Vec<(Vec<u8>, Value)> {
                            o.properties
                                .iter()
                                .filter(|(k, _)| !k.starts_with(b"__spl_"))
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        };
                        let (av, bv) = (visible(&a_b), visible(&b_b));
                        crate::spl_dll::spl_storage_view(&a_b)
                            .equals(&crate::spl_dll::spl_storage_view(&b_b))
                            && av.len() == bv.len()
                            && av.iter().all(|(k, v)| v.equals(&b_b.get_property(k)))
                    } else if a_b.class_name.eq_ignore_ascii_case(&b_b.class_name)
                        && (has_internal_slots(&a_b) || has_internal_slots(&b_b))
                    {
                        // (engine bookkeeping slots are not properties)
                        let (av, bv) = (real_props(&a_b), real_props(&b_b));
                        av.len() == bv.len()
                            && av.iter().all(|(k, v)| v.equals(&b_b.get_property(k)))
                    } else if a_b.class_name.eq_ignore_ascii_case(&b_b.class_name)
                        && a_b.properties.len() == b_b.properties.len()
                    {
                        let mut eq = true;
                        for (name, a_val) in &a_b.properties {
                            let b_val = b_b.get_property(name);
                            if !a_val.equals(&b_val) {
                                eq = false;
                                break;
                            }
                        }
                        eq
                    } else {
                        false
                    }
                }
            }
            // Object vs scalar: convert object to int(1) for comparison
            (Value::Object(_), Value::Long(n)) | (Value::Long(n), Value::Object(_)) => *n == 1,
            (Value::Object(_), Value::Double(f)) | (Value::Double(f), Value::Object(_)) => {
                *f == 1.0
            }
            (Value::Object(_), Value::String(_s)) | (Value::String(_s), Value::Object(_)) => {
                // Object compared to string: not equal in PHP 8
                false
            }
            (Value::Resource(a), Value::Resource(b)) => a == b,
            (Value::Resource(a), Value::Long(b)) | (Value::Long(b), Value::Resource(a)) => {
                a.id() == *b
            }
            (Value::Resource(a), Value::Double(f)) | (Value::Double(f), Value::Resource(a)) => {
                a.id() as f64 == *f
            }
            (Value::Resource(a), Value::String(s)) | (Value::String(s), Value::Resource(a)) => {
                if let Some(num) = parse_numeric_string(s.as_bytes()) {
                    num == a.id() as f64
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// PHP == comparison with object-to-scalar casting
    /// This is the same as equals() but handles the object conversion case
    /// (used by VM which also emits the Notice)
    pub fn equals_with_object_cast(&self, other: &Value) -> bool {
        self.equals(other)
    }

    /// PHP === comparison
    pub fn identical(&self, other: &Value) -> bool {
        if let Value::Reference(r) = self {
            return r.borrow().identical(other);
        }
        if let Value::Reference(r) = other {
            return self.identical(&r.borrow());
        }
        match (self, other) {
            (Value::Undef, Value::Undef) => true,
            (Value::Null, Value::Null) => true,
            (Value::True, Value::True) | (Value::False, Value::False) => true,
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
            (Value::Array(a), Value::Array(b)) => {
                // Same hash table: equal (zend_hash_compare's ht1 == ht2 shortcut).
                if std::rc::Rc::ptr_eq(a, b) {
                    return true;
                }
                let Some(_guard) = CmpGuard::enter(Rc::as_ptr(a) as *const () as usize) else {
                    return false;
                };
                let a = a.borrow();
                let b = b.borrow();
                if a.len() != b.len() {
                    return false;
                }
                // zend_hash_compare(ordered=1): same keys in the same order.
                for ((ka, a_val), (kb, b_val)) in a.iter().zip(b.iter()) {
                    if ka != kb || !a_val.identical(b_val) {
                        return false;
                    }
                }
                true
            }
            (Value::Object(a), Value::Object(b)) => {
                // === for objects means same instance
                std::ptr::eq(a.as_ptr(), b.as_ptr())
            }
            (Value::Generator(a), Value::Generator(b)) => Rc::ptr_eq(a, b),
            (Value::Resource(a), Value::Resource(b)) => a == b,
            _ => false,
        }
    }

    /// zend_compare(): like compare(), with an uncomparable pair reported
    /// as 1 (ZEND_UNCOMPARABLE) the way `<=>`, sort() and max() see it.
    pub fn compare_zend(&self, other: &Value) -> i64 {
        match self.compare(other) {
            i64::MIN => 1,
            c => c,
        }
    }

    /// PHP <=> comparison, returns -1, 0, or 1
    pub fn compare(&self, other: &Value) -> i64 {
        // Arrays nested into themselves through references: stop instead of
        // overflowing the stack.
        let depth = COMPARE_DEPTH.with(|d| {
            let n = d.get();
            d.set(n + 1);
            n
        });
        let r = if depth > 1000 {
            compare_recursion_set();
            0
        } else {
            self.compare_inner(other)
        };
        COMPARE_DEPTH.with(|d| d.set(depth));
        r
    }

    fn compare_inner(&self, other: &Value) -> i64 {
        if let Value::Reference(r) = self {
            return r.borrow().compare(other);
        }
        if let Value::Reference(r) = other {
            return self.compare(&r.borrow());
        }
        if let Some(c) = native_compare(self, other) {
            return if c > 1 { 1 } else { c };
        }
        if let Some(c) = enum_compare(self, other) {
            return c;
        }
        match (self, other) {
            (Value::Resource(a), Value::Resource(b)) => {
                let (a, b) = (a.id(), b.id());
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Resource(a), Value::Long(b)) => {
                let a = &a.id();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Long(a), Value::Resource(b)) => {
                let b = &b.id();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Resource(a), Value::Double(b)) => float_threeway(a.id() as f64, *b),
            (Value::Double(a), Value::Resource(b)) => float_threeway(*a, b.id() as f64),
            (Value::Long(a), Value::Long(b)) => {
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            (Value::Double(a), Value::Double(b)) => float_threeway(*a, *b),
            (Value::Long(a), Value::Double(b)) => float_threeway(*a as f64, *b),
            (Value::Double(a), Value::Long(b)) => float_threeway(*a, *b as f64),
            (Value::Array(a), Value::Array(b)) => {
                // Same hash table: equal (zend_hash_compare's ht1 == ht2 shortcut).
                if std::rc::Rc::ptr_eq(a, b) {
                    return 0;
                }
                let Some(_guard) = CmpGuard::enter(Rc::as_ptr(a) as *const () as usize) else {
                    return 0;
                };
                let a = a.borrow();
                let b = b.borrow();
                // Compare by size first
                if a.len() != b.len() {
                    return if a.len() < b.len() { -1 } else { 1 };
                }
                // Compare element by element
                for (key, a_val) in a.iter() {
                    if let Some(b_val) = b.get(key) {
                        let cmp = a_val.compare(b_val);
                        if cmp != 0 {
                            return cmp;
                        }
                    } else {
                        return 1; // key exists in a but not b
                    }
                }
                0
            }
            (Value::Null | Value::Undef, Value::Null | Value::Undef) => 0,
            (Value::Null | Value::Undef, Value::False) => 0,
            (Value::False, Value::Null | Value::Undef) => 0,
            (Value::Null | Value::Undef, Value::True) => -1,
            (Value::True, Value::Null | Value::Undef) => 1,
            // PHP 8: NULL is <= any int/float for ordering
            // NULL == 0 for equality AND ordering (compare returns 0 for NULL vs 0)
            // NULL < positive_int, NULL < negative_int (NULL is always <= any nonzero)
            (Value::Null | Value::Undef, Value::Long(n)) => {
                if *n == 0 {
                    0
                } else {
                    -1
                }
            }
            (Value::Long(n), Value::Null | Value::Undef) => {
                if *n == 0 {
                    0
                } else {
                    1
                }
            }
            (Value::Null | Value::Undef, Value::Double(f)) => {
                if *f == 0.0 {
                    0
                } else {
                    -1
                }
            }
            (Value::Double(f), Value::Null | Value::Undef) => {
                if *f == 0.0 {
                    0
                } else {
                    1
                }
            }
            (Value::Null | Value::Undef, Value::String(s)) => {
                if s.is_empty() {
                    0
                } else {
                    -1
                }
            }
            (Value::String(s), Value::Null | Value::Undef) => {
                if s.is_empty() {
                    0
                } else {
                    1
                }
            }
            (Value::Null | Value::Undef, Value::Array(a)) => {
                if a.borrow().is_empty() {
                    0
                } else {
                    -1
                }
            }
            (Value::Array(a), Value::Null | Value::Undef) => {
                if a.borrow().is_empty() {
                    0
                } else {
                    1
                }
            }
            (Value::Null | Value::Undef, _) => -1,
            (_, Value::Null | Value::Undef) => 1,
            // Bool comparisons: convert other side to bool
            (Value::True, Value::True) | (Value::False, Value::False) => 0,
            (Value::True, Value::False) => 1,
            (Value::False, Value::True) => -1,
            (Value::True, other) => {
                if other.is_truthy() {
                    0
                } else {
                    1
                }
            }
            (Value::False, other) => {
                if other.is_truthy() {
                    -1
                } else {
                    0
                }
            }
            (other, Value::True) => {
                if other.is_truthy() {
                    0
                } else {
                    -1
                }
            }
            (other, Value::False) => {
                if other.is_truthy() {
                    1
                } else {
                    0
                }
            }
            (Value::String(a), Value::String(b)) => smart_strcmp(a.as_bytes(), b.as_bytes()),
            // PHP 8: int vs string - compare numerically if string is numeric, else string > int
            (Value::Long(n), Value::String(s)) | (Value::String(s), Value::Long(n)) => {
                let is_left_long = matches!(self, Value::Long(_));
                if let Some(num) = parse_numeric_string(s.as_bytes()) {
                    let n_f = *n as f64;
                    if is_left_long {
                        if n_f < num {
                            -1
                        } else if n_f > num {
                            1
                        } else {
                            0
                        }
                    } else {
                        if num < n_f {
                            -1
                        } else if num > n_f {
                            1
                        } else {
                            0
                        }
                    }
                } else {
                    // PHP 8: non-numeric string always > int in comparison
                    // Actually: cast both to their natural comparison type
                    // In PHP 8, comparing int with non-numeric string:
                    // the int is cast to string and then compared as strings
                    let n_str = format!("{}", n);
                    let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                    if is_left_long {
                        match n_str.as_bytes().cmp(s_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    } else {
                        match s_str.as_bytes().cmp(n_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    }
                }
            }
            // PHP 8: float vs string - compare numerically if string is numeric, else cast float to string
            (Value::Double(f), Value::String(s)) | (Value::String(s), Value::Double(f)) => {
                if f.is_nan() {
                    // NAN is unordered with any string, either side.
                    return 1;
                }
                let is_left_double = matches!(self, Value::Double(_));
                if let Some(num) = parse_numeric_string(s.as_bytes()) {
                    if is_left_double {
                        float_threeway(*f, num)
                    } else {
                        float_threeway(num, *f)
                    }
                } else {
                    // PHP 8: non-numeric string - cast float to string and compare as strings
                    let f_str = format_php_float(*f);
                    let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                    if is_left_double {
                        match f_str.as_bytes().cmp(s_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    } else {
                        match s_str.as_bytes().cmp(f_str.as_bytes()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        }
                    }
                }
            }
            // Object vs Object comparison
            (Value::Object(a_obj), Value::Object(b_obj)) => {
                if Rc::ptr_eq(a_obj, b_obj) {
                    return 0;
                }
                let Some(_guard) = CmpGuard::enter(a_obj.as_ptr() as *const () as usize) else {
                    return 0;
                };
                let a_borrow = a_obj.borrow();
                let b_borrow = b_obj.borrow();
                // Enum cases: only equal if same class and same case name
                let a_is_enum = a_borrow.has_property(b"__enum_case");
                let b_is_enum = b_borrow.has_property(b"__enum_case");
                if a_is_enum || b_is_enum {
                    // Enums are not orderable - return incomparable (i64::MIN)
                    // They can only be compared for equality via == / ===
                    if a_is_enum && b_is_enum {
                        let a_class = &a_borrow.class_name;
                        let b_class = &b_borrow.class_name;
                        let a_case = a_borrow.get_property(b"name");
                        let b_case = b_borrow.get_property(b"name");
                        if a_class.eq_ignore_ascii_case(b_class) {
                            if a_case.compare(&b_case) == 0 {
                                return 0; // Same enum case
                            }
                        }
                    }
                    return i64::MIN; // Not comparable
                }
                // ext/date DateTimeInterface objects compare by (timestamp, microseconds).
                if a_borrow.has_property(b"__timestamp_us")
                    && b_borrow.has_property(b"__timestamp_us")
                {
                    let a_key = (
                        a_borrow.get_property(b"__timestamp").to_long(),
                        a_borrow.get_property(b"__timestamp_us").to_long(),
                    );
                    let b_key = (
                        b_borrow.get_property(b"__timestamp").to_long(),
                        b_borrow.get_property(b"__timestamp_us").to_long(),
                    );
                    return match a_key.cmp(&b_key) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
                }
                // ext/date DateTimeZone: equal or uncomparable; DateInterval: uncomparable.
                if a_borrow.has_property(b"__timestamp_tzobj")
                    && b_borrow.has_property(b"__timestamp_tzobj")
                {
                    let same = a_borrow.get_property(b"timezone_type").to_long()
                        == b_borrow.get_property(b"timezone_type").to_long()
                        && a_borrow
                            .get_property(b"timezone")
                            .to_php_string()
                            .as_bytes()
                            == b_borrow
                                .get_property(b"timezone")
                                .to_php_string()
                                .as_bytes();
                    return if same { 0 } else { i64::MIN };
                }
                if a_borrow.has_property(b"__timestamp_ival")
                    && b_borrow.has_property(b"__timestamp_ival")
                {
                    return i64::MIN;
                }
                // Regular objects: compare by class name and properties
                if !a_borrow
                    .class_name
                    .eq_ignore_ascii_case(&b_borrow.class_name)
                {
                    // Different classes - not comparable in PHP 8
                    // <, >, <=, >= all return false; == returns false
                    // <=> returns 1 but </>/>=/<=  use i64::MIN to indicate incomparable
                    return i64::MIN;
                }
                // spl_array_compare_objects(): the storages, then the
                // (non-internal) properties.
                if crate::spl_dll::is_spl_array_object(&a_borrow)
                    || crate::spl_dll::is_spl_array_object(&b_borrow)
                {
                    let cmp = crate::spl_dll::spl_storage_view(&a_borrow)
                        .compare(&crate::spl_dll::spl_storage_view(&b_borrow));
                    if cmp != 0 {
                        return cmp;
                    }
                    let visible = |o: &PhpObject| -> Vec<(Vec<u8>, Value)> {
                        o.properties
                            .iter()
                            .filter(|(k, _)| !k.starts_with(b"__spl_"))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    };
                    let (av, bv) = (visible(&a_borrow), visible(&b_borrow));
                    if av.len() != bv.len() {
                        return if av.len() < bv.len() { -1 } else { 1 };
                    }
                    for (key, a_val) in &av {
                        let b_val = b_borrow.get_property(key);
                        let cmp = a_val.compare(&b_val);
                        if cmp != 0 {
                            return cmp;
                        }
                    }
                    return 0;
                }
                // (engine bookkeeping slots are not properties)
                if has_internal_slots(&a_borrow) || has_internal_slots(&b_borrow) {
                    let (av, bv) = (real_props(&a_borrow), real_props(&b_borrow));
                    if av.len() != bv.len() {
                        return if av.len() < bv.len() { -1 } else { 1 };
                    }
                    for (key, a_val) in &av {
                        let cmp = a_val.compare(&b_borrow.get_property(key));
                        if cmp != 0 {
                            return cmp;
                        }
                    }
                    return 0;
                }
                // Same class: compare properties
                if a_borrow.properties.len() != b_borrow.properties.len() {
                    return if a_borrow.properties.len() < b_borrow.properties.len() {
                        -1
                    } else {
                        1
                    };
                }
                // Compare each property value
                for (key, a_val) in &a_borrow.properties {
                    let b_val = b_borrow.get_property(key);
                    let cmp = a_val.compare(&b_val);
                    if cmp != 0 {
                        return cmp;
                    }
                }
                0
            }
            // Array vs Object: object is always greater than array in PHP 8
            (Value::Array(_), Value::Object(_)) => -1,
            (Value::Object(_), Value::Array(_)) => 1,
            // Array vs non-array (non-null, non-object): arrays are always greater than scalars in PHP
            (Value::Array(_), _) => 1,
            (_, Value::Array(_)) => -1,
            // Object vs String: object is always greater than string in PHP 8
            (Value::Object(_), Value::String(_)) => 1,
            (Value::String(_), Value::Object(_)) => -1,
            // Object vs int/float: convert object to int/float, compare numerically
            (Value::Object(_), _) | (_, Value::Object(_)) => {
                // PHP 8 converts object to int (1) for numeric comparisons
                let a = self.to_double();
                let b = other.to_double();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            _ => {
                let a = self.to_double();
                let b = other.to_double();
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// Format a float the way PHP does (respecting `precision` ini setting).
/// With precision=14 (default), uses 14 significant digits.
/// With precision=-1, uses shortest roundtrip representation.
pub fn format_php_float(f: f64) -> String {
    let precision = get_php_precision();
    if precision < 0 {
        return format_php_float_shortest(f);
    }
    format_php_float_with_precision(f, precision as usize)
}

/// Shortest round-trip representation (`%.*H` with precision -1).
pub fn format_php_float_repr(f: f64) -> String {
    format_php_float_shortest(f)
}

/// Public wrapper for format_php_float_shortest (zend_gcvt mode 0).
pub fn format_php_float_shortest_pub(f: f64) -> String {
    format_php_float_shortest(f)
}

/// Format a float using shortest-roundtrip representation (like precision=-1).
fn format_php_float_shortest(f: f64) -> String {
    zend_gcvt_string(f, -1, b'.', b'E')
}

/// Public wrapper for format_php_float_with_precision
pub fn format_php_float_with_precision_pub(f: f64, sig_digits: usize) -> String {
    format_php_float_with_precision(f, sig_digits)
}

/// Format a float with a specific number of significant digits.
fn format_php_float_with_precision(f: f64, sig_digits: usize) -> String {
    zend_gcvt_string(f, sig_digits.clamp(1, 800) as i32, b'.', b'E')
}

fn zend_gcvt_string(value: f64, ndigit: i32, dec_point: u8, exp_char: u8) -> String {
    String::from_utf8(zend_gcvt(value, ndigit, dec_point, exp_char))
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// zend_dtoa() mode 0 rounds an exact tie between two shortest
/// round-tripping digit strings to the even last digit; Rust's shortest
/// formatting rounds it up. `digits` (exponent `exp`) is Rust's choice.
pub fn shortest_tie_to_even(v: f64, digits: &mut [u8], exp: i32) {
    let n = digits.len();
    let last = digits[n - 1];
    if (last - b'0').is_multiple_of(2) {
        return;
    }
    // The candidate one unit lower in the last digit must round-trip too...
    let mut alt = digits.to_vec();
    alt[n - 1] = last - 1;
    let text = |d: &[u8]| {
        let mut t = String::with_capacity(d.len() + 8);
        t.push(d[0] as char);
        t.push('.');
        for &c in &d[1..] {
            t.push(c as char);
        }
        t.push('e');
        t.push_str(&exp.to_string());
        t
    };
    if text(&alt).parse::<f64>().ok() != Some(v) {
        return;
    }
    // ...and the exact value lie halfway: its expansion is alt followed by
    // 5 and zeros (a double has at most 767 significant digits).
    let exact = format!("{:.800e}", v);
    let (mant, _) = exact.split_once('e').unwrap_or((&exact, ""));
    let ed: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
    if ed.len() > n && ed[..n] == alt[..] && ed[n] == b'5' && ed[n + 1..].iter().all(|&c| c == b'0')
    {
        digits[n - 1] = last - 1;
    }
}

/// zend_gcvt(): `ndigit` significant digits (mode 2), or the shortest
/// round-trip digits when `ndigit` is negative (mode 0, 17 max).
pub fn zend_gcvt(value: f64, ndigit: i32, dec_point: u8, exp_char: u8) -> Vec<u8> {
    if !value.is_finite() {
        let s: &[u8] = if value.is_nan() {
            b"NAN"
        } else if value < 0.0 {
            b"-INF"
        } else {
            b"INF"
        };
        return s.to_vec();
    }
    let (ndigit, digits, decpt) = {
        let neg_zero_free = value.abs();
        let s = if ndigit < 0 {
            format!("{:e}", neg_zero_free)
        } else {
            format!("{:.*e}", (ndigit.max(1) - 1) as usize, neg_zero_free)
        };
        let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        let mut digits: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
        while digits.len() > 1 && digits.last() == Some(&b'0') {
            digits.pop();
        }
        if ndigit < 0 {
            shortest_tie_to_even(neg_zero_free, &mut digits, exp);
        }
        let decpt = if value == 0.0 { 1 } else { exp + 1 };
        (if ndigit < 0 { 17 } else { ndigit }, digits, decpt)
    };
    let mut dst = Vec::with_capacity(digits.len() + 8);
    if value.is_sign_negative() {
        dst.push(b'-');
    }
    if (decpt >= 0 && decpt > ndigit) || decpt < -3 {
        let e = decpt - 1;
        dst.push(digits[0]);
        dst.push(dec_point);
        if digits.len() == 1 {
            dst.push(b'0');
        } else {
            dst.extend_from_slice(&digits[1..]);
        }
        dst.push(exp_char);
        dst.push(if e < 0 { b'-' } else { b'+' });
        dst.extend_from_slice(e.unsigned_abs().to_string().as_bytes());
    } else if decpt < 0 {
        dst.push(b'0');
        dst.push(dec_point);
        dst.extend(std::iter::repeat_n(b'0', decpt.unsigned_abs() as usize));
        dst.extend_from_slice(&digits);
    } else {
        let decpt = decpt as usize;
        for i in 0..decpt {
            dst.push(*digits.get(i).unwrap_or(&b'0'));
        }
        if digits.len() > decpt {
            if decpt == 0 {
                dst.push(b'0');
            }
            dst.push(dec_point);
            dst.extend_from_slice(&digits[decpt..]);
        }
    }
    dst
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef => write!(f, "UNDEF"),
            Value::Null => write!(f, "NULL"),
            Value::False => write!(f, "false"),
            Value::True => write!(f, "true"),
            Value::Long(n) => write!(f, "int({})", n),
            Value::Double(n) => write!(f, "float({})", n),
            Value::String(s) => write!(f, "string({:?})", s.to_string_lossy()),
            Value::Array(arr) => write!(f, "array({})", arr.borrow().len()),
            Value::Object(obj) => {
                let obj = obj.borrow();
                write!(f, "object({})", String::from_utf8_lossy(&obj.class_name))
            }
            Value::Generator(_) => write!(f, "object(Generator)"),
            Value::Resource(n) => write!(f, "resource({})", n),
            Value::Reference(r) => {
                write!(f, "&{:?}", *r.borrow())
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef | Value::Null | Value::False => Ok(()),
            Value::True => write!(f, "1"),
            Value::Long(n) => write!(f, "{}", n),
            Value::Resource(n) => write!(f, "Resource id #{}", n),
            Value::Double(d) => {
                write!(f, "{}", format_php_float(*d))
            }
            Value::String(s) => f.write_str(&s.to_string_lossy()),
            Value::Array(_) => write!(f, "Array"),
            Value::Object(_) | Value::Generator(_) => write!(f, "Object"),
            Value::Reference(r) => {
                write!(f, "{}", *r.borrow())
            }
        }
    }
}

/// Parse a PHP numeric string, returning its value as f64 if it's numeric.
/// Handles integers, floats, and scientific notation with optional leading whitespace and sign.
/// Returns None for non-numeric strings.
/// Parse the leading numeric portion of a string as a float.
/// If the string doesn't start with a number, returns 0.0.
fn parse_leading_float(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    // zend_strtod() does not accept "inf"/"nan" spellings.
    // Find the longest valid prefix that's a number
    let bytes = s.as_bytes();
    let mut i = 0;
    // Optional sign
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let start = i;
    // Digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    // Optional decimal
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start {
        return 0.0;
    }
    // Optional exponent
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let save = i;
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            i = save; // Not a valid exponent, backtrack
        }
    }
    s[..i].parse::<f64>().unwrap_or(0.0)
}

/// True if `d` fits in a PHP int (ZEND_DOUBLE_FITS_LONG).
pub fn php_double_fits_long(d: f64) -> bool {
    !(d >= 9.223_372_036_854_776e18 || d < -9.223_372_036_854_776e18)
}

/// zend_dval_to_lval: non-finite -> 0, out of range -> modular arithmetic.
pub fn php_dval_to_lval(d: f64) -> i64 {
    if !d.is_finite() {
        0
    } else if !php_double_fits_long(d) {
        let two_pow_64 = 18446744073709551616.0f64;
        let mut dmod = d % two_pow_64;
        if dmod < 0.0 {
            dmod += two_pow_64;
        }
        (dmod as u64) as i64
    } else {
        d as i64
    }
}

/// zend_dval_to_lval_cap: non-finite -> 0, out of range -> saturate.
pub fn php_dval_to_lval_cap(d: f64) -> i64 {
    if !d.is_finite() {
        0
    } else if !php_double_fits_long(d) {
        if d > 0.0 { i64::MAX } else { i64::MIN }
    } else {
        d as i64
    }
}

/// String to int conversion as done by (int) casts / zval_get_long:
/// leading numeric prefix, integer overflow and float strings saturate.
pub fn php_str_to_lval(b: &[u8]) -> i64 {
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let start = i;
    let mut j = i;
    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
        j += 1;
    }
    let digits_start = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    let int_digits = j - digits_start;
    let mut is_float = false;
    if j < b.len()
        && b[j] == b'.'
        && (int_digits > 0 || (j + 1 < b.len() && b[j + 1].is_ascii_digit()))
    {
        is_float = true;
    } else if int_digits > 0 && j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let mut k = j + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        if k < b.len() && b[k].is_ascii_digit() {
            is_float = true;
        }
    }
    if int_digits == 0 && !is_float {
        return 0;
    }
    if !is_float {
        let neg = b[start] == b'-';
        let mut acc: i64 = 0;
        let mut overflow = false;
        for &c in &b[digits_start..j] {
            let d = (c - b'0') as i64;
            match acc.checked_mul(10).and_then(|v| {
                if neg {
                    v.checked_sub(d)
                } else {
                    v.checked_add(d)
                }
            }) {
                Some(v) => acc = v,
                None => {
                    overflow = true;
                    break;
                }
            }
        }
        if !overflow {
            return acc;
        }
    }
    let text = String::from_utf8_lossy(&b[start..]);
    php_dval_to_lval_cap(parse_leading_float(&text))
}

/// Parse a leading-numeric PHP string, returning (value, is_fully_numeric).
/// For "123abc", returns Some((123.0, false)).
/// For "123", returns Some((123.0, true)).
/// For "abc", returns None.
pub fn parse_leading_numeric_string(s: &[u8]) -> Option<(f64, bool)> {
    let s_str = std::str::from_utf8(s).ok()?;
    let trimmed = s_str.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Try parsing the leading part as a number
    let mut end = 0;
    let mut chars = trimmed.chars().peekable();
    // Optional sign
    if matches!(chars.peek(), Some('+') | Some('-')) {
        chars.next();
        end += 1;
    }
    let mut has_digits = false;
    let mut has_dot = false;
    // Integer/decimal part
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            has_digits = true;
            chars.next();
            end += 1;
        } else if c == '.' && !has_dot {
            has_dot = true;
            chars.next();
            end += 1;
        } else {
            break;
        }
    }
    if !has_digits {
        return None;
    }
    // Optional exponent
    if matches!(chars.peek(), Some('e') | Some('E')) {
        let saved_end = end;
        chars.next();
        end += 1;
        if matches!(chars.peek(), Some('+') | Some('-')) {
            chars.next();
            end += 1;
        }
        if !matches!(chars.peek(), Some('0'..='9')) {
            // Invalid exponent - revert
            end = saved_end;
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    chars.next();
                    end += 1;
                } else {
                    break;
                }
            }
        }
    }
    let numeric_part = &trimmed[..end];
    let is_fully_numeric = end == trimmed.len();
    numeric_part
        .parse::<f64>()
        .ok()
        .map(|v| (v, is_fully_numeric))
}

/// Parse a PHP numeric string, returning its value as f64 if it's numeric
/// (`is_numeric_string()`: no trailing garbage; "INF"/"NAN" are not numeric).
pub fn parse_numeric_string(s: &[u8]) -> Option<f64> {
    match parse_numeric_str(s, false)? {
        (NumStr::Long(n), _) => Some(n as f64),
        (NumStr::Double(d), _) => Some(d),
    }
}

/// Enum operands (zend_compare() with zend_objects_not_comparable): a case
/// equals only itself, is greater than null and compares with true/false
/// as `true`; any other
/// pairing is uncomparable (i64::MIN).
fn enum_compare(a: &Value, b: &Value) -> Option<i64> {
    let is_enum = |v: &Value| match v {
        Value::Object(o) => o
            .try_borrow()
            .map(|o| o.has_property(b"__enum_case"))
            .unwrap_or(false),
        _ => false,
    };
    let (ae, be) = (is_enum(a), is_enum(b));
    if !ae && !be {
        return None;
    }
    if let (Value::Object(x), Value::Object(y)) = (a, b) {
        if Rc::ptr_eq(x, y) {
            return Some(0);
        }
    }
    Some(match (a, b) {
        (_, Value::Null | Value::Undef) => 1,
        (Value::Null | Value::Undef, _) => -1,
        (_, Value::True) | (Value::True, _) => 0,
        (_, Value::False) => 1,
        (Value::False, _) => -1,
        _ => i64::MIN,
    })
}

fn has_internal_slots(o: &crate::object::PhpObject) -> bool {
    o.properties
        .iter()
        .any(|(k, _)| crate::foreach_obj::is_internal_slot(k))
}

/// The properties of an object without the engine's bookkeeping slots.
fn real_props(o: &crate::object::PhpObject) -> Vec<(Vec<u8>, Value)> {
    o.properties
        .iter()
        .filter(|(k, _)| !crate::foreach_obj::is_internal_slot(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The `oflow` of `_is_numeric_string_ex()` for a numeric string: -1 / 1
/// when an integer literal overflowed into a double (20+ integer digits, or
/// 19 at or beyond 2**63), 0 otherwise.
fn numeric_str_oflow(bytes: &[u8]) -> i64 {
    let is_ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let mut p = 0;
    while p < bytes.len() && is_ws(bytes[p]) {
        p += 1;
    }
    let neg = bytes.get(p) == Some(&b'-');
    if matches!(bytes.get(p), Some(b'-' | b'+')) {
        p += 1;
    }
    while bytes.get(p) == Some(&b'0') {
        p += 1;
    }
    let start = p;
    while p < bytes.len() && bytes[p].is_ascii_digit() {
        p += 1;
    }
    let digits = p - start;
    let sign = if neg { -1 } else { 1 };
    if digits >= 20 {
        return sign;
    }
    if digits == 19 {
        // (a fraction or exponent makes it a double literal: no overflow)
        let next = bytes.get(p).copied().unwrap_or(0);
        if next == b'.' {
            return 0;
        }
        if next == b'e' || next == b'E' {
            let mut e = p + 1;
            if matches!(bytes.get(e), Some(b'-' | b'+')) {
                e += 1;
            }
            if bytes.get(e).is_some_and(|c| c.is_ascii_digit()) {
                return 0;
            }
        }
        let cmp = bytes[start..p].cmp(b"9223372036854775808".as_slice());
        if !(cmp == std::cmp::Ordering::Less || (cmp == std::cmp::Ordering::Equal && neg)) {
            return sign;
        }
    }
    0
}

/// zend_binary_strcmp(), normalized.
fn binary_strcmp(a: &[u8], b: &[u8]) -> i64 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// zendi_smart_streq(): `==` of two strings.
pub fn smart_streq(a: &[u8], b: &[u8]) -> bool {
    let (Some((n1, _)), Some((n2, _))) = (parse_numeric_str(a, false), parse_numeric_str(b, false))
    else {
        return a == b;
    };
    let (o1, o2) = (numeric_str_oflow(a), numeric_str_oflow(b));
    let d = |n: NumStr| match n {
        NumStr::Long(l) => l as f64,
        NumStr::Double(d) => d,
    };
    if o1 != 0 && o1 == o2 && d(n1) - d(n2) == 0.0 {
        // both integers overflowed to the same side: compare the strings
        return a == b;
    }
    match (n1, n2) {
        (NumStr::Long(l1), NumStr::Long(l2)) => l1 == l2,
        (NumStr::Long(l1), NumStr::Double(d2)) => o2 == 0 && l1 as f64 == d2,
        (NumStr::Double(d1), NumStr::Long(l2)) => o1 == 0 && d1 == l2 as f64,
        (NumStr::Double(d1), NumStr::Double(d2)) => {
            if d1 == d2 && !d1.is_finite() {
                a == b
            } else {
                d1 == d2
            }
        }
    }
}

/// zendi_smart_strcmp(): `<=>` of two strings.
pub fn smart_strcmp(a: &[u8], b: &[u8]) -> i64 {
    let (Some((n1, _)), Some((n2, _))) = (parse_numeric_str(a, false), parse_numeric_str(b, false))
    else {
        return binary_strcmp(a, b);
    };
    let (o1, o2) = (numeric_str_oflow(a), numeric_str_oflow(b));
    let d = |n: NumStr| match n {
        NumStr::Long(l) => l as f64,
        NumStr::Double(d) => d,
    };
    let norm = |x: f64| -> i64 { (x > 0.0) as i64 - (x < 0.0) as i64 };
    if o1 != 0 && o1 == o2 && d(n1) - d(n2) == 0.0 {
        return binary_strcmp(a, b);
    }
    match (n1, n2) {
        (NumStr::Long(l1), NumStr::Long(l2)) => (l1 > l2) as i64 - (l1 < l2) as i64,
        (NumStr::Long(l1), NumStr::Double(d2)) => {
            if o2 != 0 {
                -o2
            } else {
                norm(l1 as f64 - d2)
            }
        }
        (NumStr::Double(d1), NumStr::Long(l2)) => {
            if o1 != 0 {
                o1
            } else {
                norm(d1 - l2 as f64)
            }
        }
        (NumStr::Double(d1), NumStr::Double(d2)) => {
            if d1 == d2 && !d1.is_finite() {
                binary_strcmp(a, b)
            } else {
                norm(d1 - d2)
            }
        }
    }
}

/// Result of `_is_numeric_string_ex`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumStr {
    Long(i64),
    Double(f64),
}

/// Port of zend_operators.c `_is_numeric_string_ex`: leading whitespace, an
/// optional sign, then an integer or float literal; trailing whitespace is
/// allowed. With `allow_errors`, trailing garbage is accepted and reported in
/// the returned flag. Integers that overflow become doubles.
pub fn parse_numeric_str(bytes: &[u8], allow_errors: bool) -> Option<(NumStr, bool)> {
    const MAX_LENGTH_OF_LONG: usize = 20;
    let is_ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let at = |i: usize| -> u8 { bytes.get(i).copied().unwrap_or(0) };
    let mut start = 0;
    while start < bytes.len() && is_ws(bytes[start]) {
        start += 1;
    }
    let mut ptr = start;
    let mut neg = false;
    if at(ptr) == b'-' {
        neg = true;
        ptr += 1;
    } else if at(ptr) == b'+' {
        ptr += 1;
    }
    let mut is_double = false;
    let mut digits = 0usize;
    let mut tmp: u64 = 0;
    let end;
    if at(ptr).is_ascii_digit() {
        while at(ptr) == b'0' {
            ptr += 1;
        }
        loop {
            if digits >= MAX_LENGTH_OF_LONG {
                is_double = true;
                break;
            }
            let c = at(ptr);
            if c.is_ascii_digit() {
                tmp = tmp.wrapping_mul(10).wrapping_add((c - b'0') as u64);
                digits += 1;
                ptr += 1;
                continue;
            } else if c == b'.' {
                is_double = true;
            } else if c == b'e' || c == b'E' {
                let mut e = ptr + 1;
                if at(e) == b'-' || at(e) == b'+' {
                    e += 1;
                }
                if at(e).is_ascii_digit() {
                    is_double = true;
                }
            }
            break;
        }
    } else if at(ptr) == b'.' && at(ptr + 1).is_ascii_digit() {
        is_double = true;
    } else {
        return None;
    }
    let mut dval = 0.0;
    if is_double {
        // zend_strtod on the longest float prefix
        let mut q = start;
        if at(q) == b'-' || at(q) == b'+' {
            q += 1;
        }
        while at(q).is_ascii_digit() {
            q += 1;
        }
        if at(q) == b'.' {
            q += 1;
            while at(q).is_ascii_digit() {
                q += 1;
            }
        }
        if at(q) == b'e' || at(q) == b'E' {
            let mut e = q + 1;
            if at(e) == b'-' || at(e) == b'+' {
                e += 1;
            }
            if at(e).is_ascii_digit() {
                while at(e).is_ascii_digit() {
                    e += 1;
                }
                q = e;
            }
        }
        let txt = std::str::from_utf8(&bytes[start..q]).unwrap_or("0");
        dval = txt.parse::<f64>().unwrap_or(0.0);
        end = q;
    } else {
        end = ptr;
    }
    let mut trailing = false;
    if end != bytes.len() {
        let mut e = end;
        while e < bytes.len() && is_ws(bytes[e]) {
            e += 1;
        }
        if e != bytes.len() {
            if !allow_errors {
                return None;
            }
            trailing = true;
        }
    }
    if is_double {
        return Some((NumStr::Double(dval), trailing));
    }
    if digits == MAX_LENGTH_OF_LONG - 1 {
        // Compare against "9223372036854775808"
        let digit_str = &bytes[end - digits..end];
        let cmp = digit_str.cmp(b"9223372036854775808".as_slice());
        if !(cmp == std::cmp::Ordering::Less || (cmp == std::cmp::Ordering::Equal && neg)) {
            let txt = std::str::from_utf8(&bytes[start..end]).unwrap_or("0");
            return Some((NumStr::Double(txt.parse::<f64>().unwrap_or(0.0)), trailing));
        }
    }
    let v = if neg {
        (tmp as i64).wrapping_neg()
    } else {
        tmp as i64
    };
    Some((NumStr::Long(v), trailing))
}

/// Run the native cast handler of an extension object (see
/// `NativeObjectHandlers::cast`).
#[inline]
pub fn native_cast(o: &Rc<RefCell<crate::object::PhpObject>>, kind: u8) -> Option<Value> {
    let f = {
        let b = o.borrow();
        b.native.as_ref().and_then(|n| n.handlers.cast)
    }?;
    f(o, kind)
}

/// cast_object(_IS_NUMBER) of an extension object: its string cast read as
/// a number (None when it has no cast handler).
pub fn native_cast_number(o: &Rc<RefCell<crate::object::PhpObject>>) -> Option<Value> {
    let s = native_cast(o, 3)?.to_php_string();
    Some(match parse_numeric_str(s.as_bytes(), true) {
        Some((NumStr::Long(n), _)) => Value::Long(n),
        Some((NumStr::Double(f), _)) => Value::Double(f),
        None => Value::Long(0),
    })
}

/// Comparison involving an extension object (zend compare handler plus the
/// cast-based fallback for object vs scalar). None = default behaviour.
pub fn native_compare(a: &Value, b: &Value) -> Option<i64> {
    let ha = if let Value::Object(o) = a {
        o.borrow().native.as_ref().map(|n| n.handlers)
    } else {
        None
    };
    let hb = if let Value::Object(o) = b {
        o.borrow().native.as_ref().map(|n| n.handlers)
    } else {
        None
    };
    if ha.is_none() && hb.is_none() {
        return None;
    }
    if let (Value::Object(_), Value::Object(_)) = (a, b) {
        let f = ha
            .and_then(|h| h.compare)
            .or_else(|| hb.and_then(|h| h.compare))?;
        return f(a, b);
    }
    // object vs scalar: the compare handler (zend_compare() calls it
    // whenever an operand is an object), else the cast to the scalar's type
    if let Some(f) = ha
        .and_then(|h| h.compare)
        .or_else(|| hb.and_then(|h| h.compare))
    {
        if let Some(r) = f(a, b) {
            return Some(r);
        }
    }
    let (obj, other, swapped) = match (a, b) {
        (Value::Object(o), x) => (o, x, false),
        (x, Value::Object(o)) => (o, x, true),
        _ => return None,
    };
    let h = if swapped { hb } else { ha }?;
    let cast = h.cast?;
    let kind = match other {
        Value::True | Value::False => 0,
        Value::Long(_) => 1,
        Value::Double(_) => 2,
        Value::String(_) => 3,
        Value::Null | Value::Undef => return Some(if swapped { -1 } else { 1 }),
        _ => return None,
    };
    let cv = cast(obj, kind)?;
    let r = if swapped {
        other.compare(&cv)
    } else {
        cv.compare(other)
    };
    Some(r)
}

/// pow_function_base() for long ** long (exp >= 0): square-and-multiply,
/// falling back to doubles at the first overflow exactly like php-src.
fn pow_long(base: i64, exp: i64) -> Value {
    if exp == 0 {
        return Value::Long(1);
    }
    if base == 0 {
        return Value::Long(0);
    }
    let (mut l1, mut l2, mut i) = (1i64, base, exp);
    while i >= 1 {
        if i % 2 != 0 {
            i -= 1;
            match l1.checked_mul(l2) {
                Some(v) => l1 = v,
                None => {
                    let dval = l1 as f64 * l2 as f64;
                    return Value::Double(dval * (l2 as f64).powf(i as f64));
                }
            }
        } else {
            i /= 2;
            match l2.checked_mul(l2) {
                Some(v) => l2 = v,
                None => {
                    let dval = l2 as f64 * l2 as f64;
                    return Value::Double(l1 as f64 * dval.powf(i as f64));
                }
            }
        }
    }
    Value::Long(l1)
}
