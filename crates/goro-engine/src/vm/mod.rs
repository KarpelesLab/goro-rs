//! The virtual machine.
//!
//! # Frames and slots
//!
//! Each call of a user function gets a [`Frame`] (metadata) and a run of
//! slots on the slot stack (its variables and temporaries). The slot stack
//! is a list of chunks that never move, so frames address their slots with
//! a raw base pointer; generators keep their slots in their own heap block
//! and fibers have their own stacks. Calls between user functions don't
//! recurse on the Rust stack: the dispatch loop pushes and pops frames.
//! Native functions run on the Rust stack and re-enter the loop for
//! callbacks ([`Vm::call_value`]).
//!
//! # Calls
//!
//! A call is prepared by an `Init*` instruction, which resolves the callee
//! and reserves its slots (a [`Pending`] call), followed by `Send*`
//! instructions writing the arguments straight into the callee's parameter
//! slots, and `DoCall`, which checks the arguments and activates the frame.
//!
//! # Errors
//!
//! Operations return [`Reg`]: `Err(Unwind::Throw(obj))` is a PHP exception
//! propagating; `Exit` and `Fatal` unwind everything.

pub mod calls;
pub mod classes;
pub mod ctx;
pub mod dims;
pub mod errors;
pub mod exec;
pub mod include;
pub mod iter;
pub mod lazy;
pub mod objects;
pub mod output;
pub mod setup;
pub mod types;
pub mod variance;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use goro_runtime::{ObjRef, PhpArray, PhpRef, PhpStr, Value};

use crate::bytecode::FuncCode;
use crate::class::Class;
use crate::func::Func;

pub type Slot = Option<Value>;

/// Non-local exit from an operation.
pub enum Unwind {
    /// A PHP exception.
    Throw(ObjRef),
    /// `exit()`: the status is in [`Vm::exit_status`].
    Exit,
    /// A fatal error (already reported).
    Fatal,
}

impl std::fmt::Debug for Unwind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unwind::Throw(o) => write!(f, "Throw({:?})", o),
            Unwind::Exit => write!(f, "Exit"),
            Unwind::Fatal => write!(f, "Fatal"),
        }
    }
}

pub type R<T> = Result<T, Unwind>;

pub mod frame_flags {
    /// Returning from this frame returns from the Rust call that ran it.
    pub const ENTRY: u32 = 1;
    /// Variables are shared with an enclosing scope (include, eval).
    pub const ATTACHED: u32 = 2;
    /// The global scope (main script).
    pub const GLOBAL: u32 = 4;
    /// A generator body.
    pub const GENERATOR: u32 = 8;
    /// Constructor called by `new`: the result is the object.
    pub const CTOR: u32 = 16;
    /// Strict types for calls made from this frame.
    pub const STRICT: u32 = 32;
    /// Discard the return value.
    pub const DISCARD: u32 = 64;
    /// The frame's slots are owned elsewhere (generator, globals).
    pub const OWN_SLOTS: u32 = 128;
    /// A destructor call.
    pub const DTOR: u32 = 256;
    /// Called through a magic method trampoline (__call), hidden in traces.
    pub const MAGIC: u32 = 512;
    /// A property hook.
    pub const HOOK: u32 = 1024;
}

/// Where a frame's slots are on the slot stack (to free them on return).
#[derive(Clone, Copy, Debug, Default)]
pub struct StackMark {
    pub chunk: u32,
    pub top: u32,
}

/// An activation of a user function.
pub struct Frame {
    pub func: Rc<Func>,
    pub code: *const FuncCode,
    /// Saved instruction pointer (the running frame's is [`Vm::ip`]).
    pub ip: u32,
    pub base: *mut Slot,
    pub this: Option<ObjRef>,
    /// Late static binding class.
    pub static_class: *const Class,
    /// Class scope for visibility (the method's class, or a closure's
    /// bound scope).
    pub scope: *const Class,
    /// Where the caller wants the result (null: nowhere).
    pub ret: *mut Slot,
    pub argc: u32,
    /// Arguments beyond the declared parameters (`func_get_args`).
    pub extra_args: Vec<Value>,
    /// Variables not compiled to slots (`$$x`, `extract`...).
    pub dyn_vars: Option<Box<PhpArray>>,
    /// The closure object being run (keeps it alive).
    pub closure: Option<ObjRef>,
    /// The generator object (generator bodies).
    pub generator: Option<ObjRef>,
    pub flags: u32,
    pub mark: StackMark,
    /// Static variable storage.
    pub statics: *const RefCell<Vec<Option<PhpRef>>>,
    /// For attached frames: index of the frame whose variables these are
    /// (`usize::MAX` for the global scope).
    pub owner: usize,
    /// Pending-call depth when the frame started.
    pub pending_base: u32,
    /// Native-call depth when the frame started.
    pub native_base: u32,
    /// Silence (`@`) depth saved across the call.
    pub line_hint: u32,
}

impl Frame {
    #[inline]
    pub fn code(&self) -> &FuncCode {
        // SAFETY: the frame holds `func`, which owns the code.
        unsafe { &*self.code }
    }
}

/// A call being prepared (between `Init*` and `DoCall`).
pub struct Pending {
    pub func: Rc<Func>,
    /// User functions: the callee's slots.
    pub base: *mut Slot,
    pub mark: StackMark,
    /// Native functions: the arguments.
    pub args: Vec<Value>,
    pub argc: u32,
    pub extra: Vec<Value>,
    /// Named arguments that went to a variadic parameter.
    pub named_extra: Option<PhpArray>,
    pub this: Option<ObjRef>,
    pub static_class: *const Class,
    pub scope: *const Class,
    pub closure: Option<ObjRef>,
    pub flags: u32,
    /// `new`: the object being constructed.
    pub new_obj: Option<ObjRef>,
    /// `__call` / `__callStatic` trampolines: the called name.
    pub magic_name: Option<PhpStr>,
    /// Statics of the callee (closures have their own).
    pub statics: *const RefCell<Vec<Option<PhpRef>>>,
    /// Whether a named argument was sent (affects defaults filling).
    pub has_named: bool,
    /// Bit set of parameters passed (for named arguments), first 64.
    pub passed: u64,
}

pub mod pending_flags {
    pub const NEW: u32 = 1;
    /// Call made by `new` with no constructor: nothing to run.
    pub const NO_CTOR: u32 = 2;
    pub const MAGIC_CALL: u32 = 4;
    pub const MAGIC_STATIC: u32 = 8;
    /// Callable through a Closure created from a callable string/array.
    pub const VIA_CLOSURE: u32 = 16;
}

/// A native call in progress (for backtraces).
pub struct NativeCall {
    pub func: Rc<Func>,
    pub args: *const Vec<Value>,
    pub this: Option<ObjRef>,
    /// Frame depth at the call.
    pub depth: u32,
    pub line: u32,
}

const CHUNK: usize = 1 << 16;

/// The slot stack: chunks that never move.
pub struct SlotStack {
    chunks: Vec<Box<[Slot]>>,
    cur: usize,
    top: usize,
}

impl Default for SlotStack {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotStack {
    pub fn new() -> SlotStack {
        SlotStack {
            chunks: vec![new_chunk(CHUNK)],
            cur: 0,
            top: 0,
        }
    }

    /// Reserves `n` slots (all `None`).
    #[inline]
    pub fn alloc(&mut self, n: usize) -> (*mut Slot, StackMark) {
        let mark = StackMark {
            chunk: self.cur as u32,
            top: self.top as u32,
        };
        if self.top + n > self.chunks[self.cur].len() {
            self.cur += 1;
            if self.cur == self.chunks.len() {
                self.chunks.push(new_chunk(CHUNK.max(n)));
            } else if self.chunks[self.cur].len() < n {
                self.chunks[self.cur] = new_chunk(n);
            }
            self.top = 0;
        }
        let p = unsafe { self.chunks[self.cur].as_mut_ptr().add(self.top) };
        self.top += n;
        (p, mark)
    }

    /// Releases everything allocated since `mark`. The slots must already
    /// be cleared.
    #[inline]
    pub fn release(&mut self, mark: StackMark) {
        self.cur = mark.chunk as usize;
        self.top = mark.top as usize;
    }

    /// Bytes reserved (for memory accounting).
    pub fn used_slots(&self) -> usize {
        self.chunks[..self.cur]
            .iter()
            .map(|c| c.len())
            .sum::<usize>()
            + self.top
    }
}

fn new_chunk(n: usize) -> Box<[Slot]> {
    (0..n).map(|_| None).collect::<Vec<_>>().into_boxed_slice()
}

/// Execution state that belongs to one fiber (the main program is one).
#[derive(Default)]
pub struct ExecState {
    pub frames: Vec<Frame>,
    pub ip: usize,
    pub stack: SlotStack,
    pub pending: Vec<Pending>,
    pub native_calls: Vec<NativeCall>,
    /// The top frame's slots and code (kept in sync by [`ExecState::sync_top`]
    /// on every push/pop: the dispatch loop reads them on each operand).
    pub top: TopFrame,
}

/// A typed property a reference is bound to: the object (none for static
/// properties) and the property; it stops counting once the object is gone
/// or the property no longer holds the reference.
#[derive(Clone)]
pub struct TypedRefSource {
    pub obj: Option<goro_runtime::WeakObj>,
    pub prop: Rc<crate::class::PropInfo>,
}

#[derive(Clone, Copy)]
pub struct TopFrame {
    pub base: *mut Slot,
    pub code: *const FuncCode,
}

impl Default for TopFrame {
    fn default() -> TopFrame {
        TopFrame {
            base: std::ptr::null_mut(),
            code: std::ptr::null(),
        }
    }
}

impl ExecState {
    /// Refreshes the cached top-frame pointers.
    #[inline]
    pub fn sync_top(&mut self) {
        self.top = match self.frames.last() {
            Some(f) => TopFrame {
                base: f.base,
                code: f.code,
            },
            None => TopFrame::default(),
        };
    }

    /// Visits every value the state holds (cycle collection of fibers).
    pub fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for f in &self.frames {
            let n = f.code().num_slots as usize;
            for i in 0..n {
                if let Some(v) = unsafe { (*f.base.add(i)).as_ref() } {
                    visit(v);
                }
            }
            if let Some(t) = &f.this {
                visit(&Value::Object(t.clone()));
            }
        }
    }
}

/// The global scope's variables.
pub struct Globals {
    pub code: Option<Rc<FuncCode>>,
    pub slots: Box<[Slot]>,
    pub dyn_vars: PhpArray,
}

/// ini settings.
pub struct Ini {
    pub values: HashMap<PhpStr, PhpStr>,
    pub error_reporting: i64,
    pub display_errors: u8,
    pub log_errors: bool,
    pub html_errors: bool,
    pub precision: i32,
    pub serialize_precision: i32,
    pub assertions: i64,
    pub memory_limit: i64,
    pub include_path: PhpStr,
    pub max_nesting: usize,
    /// Settings changed at run time, with their original values
    /// (`ini_restore`).
    pub originals: HashMap<PhpStr, Option<PhpStr>>,
}

/// Error handling state.
#[derive(Default)]
pub struct ErrorState {
    /// `set_error_handler` stack: (handler, mask).
    pub handlers: Vec<(Value, i64)>,
    pub exception_handlers: Vec<Value>,
    pub last: Option<(i64, PhpStr, PhpStr, i64)>,
    /// `@` nesting: saved error_reporting values.
    pub silence: Vec<i64>,
    /// Inside the user error handler (no recursion).
    pub in_handler: bool,
    /// An exception thrown by an error handler during a runtime operation
    /// (reported through `Ctx::notice`, which can't fail).
    pub deferred: Option<Unwind>,
    /// Diagnostics wait in `queued` while this is non-zero.
    pub defer_depth: u32,
    pub queued: Vec<(i64, String, PhpStr, u32)>,
    /// The last diagnostic shown (ignore_repeated_errors).
    pub last_shown: Option<(String, Vec<u8>, u32)>,
}

/// Classes and functions the engine refers to directly.
#[derive(Default)]
pub struct CoreClasses {
    pub std_class: Option<Rc<Class>>,
    pub closure: Option<Rc<Class>>,
    pub generator: Option<Rc<Class>>,
    pub array_access: Option<Rc<Class>>,
    pub iterator: Option<Rc<Class>>,
    pub iterator_aggregate: Option<Rc<Class>>,
    pub traversable: Option<Rc<Class>>,
    pub countable: Option<Rc<Class>>,
    pub stringable: Option<Rc<Class>>,
    pub throwable: Option<Rc<Class>>,
    pub unit_enum: Option<Rc<Class>>,
    pub backed_enum: Option<Rc<Class>>,
}

pub struct Vm {
    pub ex: ExecState,
    pub arg_pool: Vec<Vec<Value>>,
    pub functions: HashMap<PhpStr, Rc<Func>>,
    /// Functions removed by disable_functions (internal classes written in
    /// PHP still reach them).
    pub disabled_functions: HashMap<PhpStr, Rc<Func>>,
    /// A class constant lookup of a constant expression runs: its errors'
    /// traces show the expression's frame.
    pub trace_const_expr: bool,
    /// Attribute lists (by address) whose #[\Deprecated] arguments are
    /// being evaluated: their item's own deprecation is not raised again
    /// (a constant whose message is itself).
    pub deprecating: Vec<usize>,
    /// Classes are being bound early (when a file is loaded): variance
    /// checks needing unknown classes defer the class to run time.
    pub early_binding: bool,
    /// The class being linked is bound early.
    pub(crate) linking_early: bool,
    /// Its binding waits for run time (a check needed an unknown class).
    pub(crate) link_deferred: bool,
    /// Variance checks of the class being linked.
    pub(crate) obligations: Vec<variance::Obligation>,
    /// Variance checks waiting for a class being autoloaded (lowercase
    /// name, check, declaring class).
    pub(crate) delayed_obligations:
        Vec<(PhpStr, variance::Obligation, Rc<crate::class::ClassDecl>)>,
    /// The method name a `__call`/`__callStatic` call is about to run for.
    pub magic_call_name: Option<PhpStr>,
    /// register_tick_function(): (callable, arguments).
    pub tick_funcs: Vec<(Value, Vec<Value>)>,
    /// Statements run under declare(ticks), and whether tick functions run.
    pub tick_count: u64,
    pub in_tick: bool,
    /// The tick function running.
    pub tick_running: Option<Value>,
    /// Attributes of user constants declared with `const` (by normalized
    /// name).
    pub const_attrs: HashMap<PhpStr, Rc<[crate::bytecode::AttrInfo]>>,
    /// User functions in declaration order (get_defined_functions).
    pub user_functions: Vec<PhpStr>,
    pub classes: HashMap<PhpStr, Rc<Class>>,
    /// Declared class names in order (get_declared_classes).
    pub class_order: Vec<PhpStr>,
    pub next_class_id: u32,
    pub constants: HashMap<PhpStr, Value>,
    /// Lower-case names of constants declared case-insensitively... and
    /// the declaration order of user constants.
    pub user_constants: Vec<PhpStr>,
    pub included: Vec<PhpStr>,
    pub included_set: HashSet<PhpStr>,
    pub globals: Globals,
    pub ini: Ini,
    pub out: output::Output,
    pub err: ErrorState,
    pub core: CoreClasses,
    /// Registered autoloaders, resolved when they were registered (SPL:
    /// spl_autoload_register(); the called scope is the registration's).
    pub autoloaders: Vec<calls::Callee>,
    /// Exceptions whose string form says "and defined in" (see
    /// `mark_defined_in`).
    pub defined_in_errors: Vec<goro_runtime::WeakObj>,
    /// Typed references: the properties each one is bound to (by
    /// reference address; valid while the reference's typed flag is set).
    pub typed_refs: HashMap<usize, Vec<TypedRefSource>>,
    /// User stream wrappers: lowercase protocol to class name.
    pub user_wrappers: std::collections::HashMap<Vec<u8>, PhpStr>,
    /// Built-in wrappers disabled by stream_wrapper_unregister().
    pub disabled_wrappers: Vec<Vec<u8>>,
    pub autoloading: Vec<PhpStr>,
    pub shutdown_functions: Vec<(Value, Vec<Value>)>,
    pub exit_status: i32,
    pub script_path: PhpStr,
    pub argv: Vec<PhpStr>,
    pub cwd: std::path::PathBuf,
    /// Internal classes not declared yet: lower name → (module index,
    /// class index).
    pub lazy_classes: HashMap<PhpStr, (usize, usize)>,
    pub modules: Vec<&'static crate::registry::Module>,
    pub native_methods: HashMap<PhpStr, crate::func::NativeFn>,
    pub native_hooks: HashMap<PhpStr, fn() -> crate::class::NativeHooks>,
    /// Per-extension state (keyed by type).
    pub ext_state: HashMap<std::any::TypeId, Box<dyn std::any::Any>>,
    /// Compiled units kept alive (their code is referenced by functions).
    pub units: Vec<Rc<FuncCode>>,
    pub shutting_down: bool,
    pub gc_enabled: bool,
    pub start_time: std::time::Instant,
    /// Destructor calls in progress (for nesting decisions).
    pub dtor_depth: u32,
    pub compile_notices_pending: Vec<crate::compiler::CompileNotice>,
    /// Depth of nested `run` loops (native → user re-entry).
    pub run_depth: u32,
    pub resources: resources::Resources,
    /// Objects created for enum cases etc. kept alive.
    pub keep_alive: Vec<Value>,
    pub fiber: Option<ObjRef>,
    pub strict_main: bool,
    pub stdout_res: Option<goro_runtime::ResRef>,
    pub uncaught_reported: bool,
    /// Markers of uninitialized / unset declared properties.
    pub uninit: goro_runtime::ResRef,
    pub unset_mark: PhpRef,
    /// Magic property method recursion guards: (object, name, kind).
    pub prop_guards: Vec<(u32, PhpStr, u8)>,
    /// Property hooks running: (object, property).
    pub hook_guards: Vec<(u32, PhpStr)>,
    /// Objects inside `__clone` (readonly reinitialization allowed).
    pub cloning: Vec<u32>,
    pub noop: Option<Rc<Func>>,
    pub compare_depth: u32,
    /// Shared function objects of closures, by code.
    pub closure_funcs: HashMap<usize, Rc<Func>>,
    /// The exception being matched by `catch`.
    pub cur_exception: Option<ObjRef>,
    /// Location used for diagnostics instead of the current line.
    /// While a call binds its arguments: the callee, whose declaration
    /// is where diagnostics of the binding are reported.
    pub loc_override: Option<*const crate::bytecode::FuncCode>,
    /// Fold internal constants at compile time.
    pub fold_constants: bool,
    /// Values cached by constant fetches.
    pub const_cache: Vec<Value>,
    /// `__COMPILER_HALT_OFFSET__` per file.
    pub halt_offsets: HashMap<PhpStr, i64>,
    /// Containers being dumped (recursion detection).
    pub dump_stack: Vec<usize>,
    /// The directory handle of the last opendir() (readdir() default).
    pub last_dir: Option<goro_runtime::ResRef>,
    /// WeakReference objects by target handle.
    pub weakrefs: HashMap<u32, (goro_runtime::WeakObj, goro_runtime::WeakObj)>,
    /// Compiling eval() code (compile errors are thrown).
    pub compiling_eval: bool,
    /// Execution states of the resumers of running fibers.
    pub ex_parents: Vec<ExecState>,
    /// Running fibers, innermost last.
    pub fiber_stack: Vec<ObjRef>,
}

pub mod resources {
    /// Registry of stream resources (STDIN/STDOUT/STDERR and files).
    #[derive(Default)]
    pub struct Resources {
        pub stdin: Option<goro_runtime::ResRef>,
        pub stdout: Option<goro_runtime::ResRef>,
        pub stderr: Option<goro_runtime::ResRef>,
    }
}

impl Vm {
    // ------------------------------------------------------------ frames

    #[inline]
    pub fn frame(&self) -> &Frame {
        self.ex.frames.last().expect("no frame")
    }

    #[inline]
    pub fn frame_mut(&mut self) -> &mut Frame {
        self.ex.frames.last_mut().expect("no frame")
    }

    pub fn has_frame(&self) -> bool {
        !self.ex.frames.is_empty()
    }

    /// The current line (of the running user code).
    pub fn cur_line(&self) -> u32 {
        if let Some(c) = self.loc_override {
            // SAFETY: set only while the callee's code is held by its frame.
            return unsafe { (*c).line_start };
        }
        match self.ex.frames.last() {
            Some(f) if !f.code().file.is_empty() => {
                let ip = self.ex.ip.saturating_sub(1);
                f.code().line_of(ip)
            }
            Some(_) => match self.user_frame() {
                Some(f) => f.code().line_of(f.ip.saturating_sub(1) as usize),
                None => 0,
            },
            None => 0,
        }
    }

    /// Inside internal (stub) code: the innermost frame running user code,
    /// whose current instruction is the call into the internal code.
    fn user_frame(&self) -> Option<&Frame> {
        self.ex
            .frames
            .iter()
            .rev()
            .skip(1)
            .find(|f| !f.code().file.is_empty())
    }

    pub fn cur_file(&self) -> PhpStr {
        if let Some(c) = self.loc_override {
            // SAFETY: as in cur_line.
            return unsafe { (*c).file.clone() };
        }
        match self.ex.frames.last() {
            Some(f) if !f.code().file.is_empty() => f.code().file.clone(),
            Some(_) => match self.user_frame() {
                Some(f) => f.code().file.clone(),
                None => self.script_path.clone(),
            },
            None if self.shutting_down => PhpStr::from("[no active file]"),
            None => self.script_path.clone(),
        }
    }

    /// Whether calls from the current code use strict types.
    pub fn caller_strict(&self) -> bool {
        match self.ex.frames.last() {
            Some(f) => f.code().flags & crate::bytecode::fflags::STRICT != 0,
            None => false,
        }
    }

    pub fn scope_class(&self) -> Option<&Class> {
        let p = self.ex.frames.last()?.scope;
        if p.is_null() {
            None
        } else {
            Some(unsafe { &*p })
        }
    }

    pub fn scope_ptr(&self) -> *const Class {
        match self.ex.frames.last() {
            Some(f) => f.scope,
            None => std::ptr::null(),
        }
    }

    pub fn static_class_ptr(&self) -> *const Class {
        match self.ex.frames.last() {
            Some(f) => f.static_class,
            None => std::ptr::null(),
        }
    }

    pub fn this_obj(&self) -> Option<ObjRef> {
        self.ex.frames.last()?.this.clone()
    }

    /// Takes a vector from the argument pool.
    #[inline]
    pub fn take_args_vec(&mut self) -> Vec<Value> {
        self.arg_pool.pop().unwrap_or_default()
    }

    #[inline]
    pub fn give_args_vec(&mut self, mut v: Vec<Value>) {
        if v.capacity() == 0 {
            return;
        }
        v.clear();
        if self.arg_pool.len() < 64 {
            self.arg_pool.push(v);
        }
    }

    // ------------------------------------------------------------ class lookup helpers

    pub fn class_by_ptr(&self, p: *const Class) -> Option<Rc<Class>> {
        if p.is_null() {
            return None;
        }
        let c = unsafe { &*p };
        self.classes
            .get(&c.lname)
            .cloned()
            .filter(|x| std::ptr::eq(&**x, p))
            .or_else(|| {
                // anonymous or shadowed: search
                None
            })
    }

    /// The Rc of a class known by pointer (classes are kept in `classes`
    /// or `anon` lists).
    #[inline]
    pub fn rc_class(&self, p: *const Class) -> Rc<Class> {
        // SAFETY: every class is owned by an Rc kept alive by the engine;
        // reconstruct a strong handle.
        unsafe {
            Rc::increment_strong_count(p);
            Rc::from_raw(p)
        }
    }

    pub fn ext<T: 'static + Default>(&mut self) -> &mut T {
        self.ext_state
            .entry(std::any::TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut::<T>()
            .unwrap()
    }
}
