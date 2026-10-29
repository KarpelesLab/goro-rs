//! Engine startup (modules, ini, superglobals) and request shutdown.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::*;
use crate::registry::{Module, sig_name};

impl Vm {
    /// A new engine with the given modules.
    pub fn new(modules: &[&'static Module]) -> Vm {
        // Resource ids as PHP's CLI numbers them: STDIN, STDOUT, STDERR,
        // then one internal (here the uninitialized-property marker).
        let stdio = crate::builtins::file::stdio_resources();
        let uninit = goro_runtime::ResRef::new(PhpStr::intern(b"uninitialized"), Box::new(()));
        let unset_mark = goro_runtime::PhpRef::new(Value::Null);
        let mut vm = Vm {
            ex: ExecState::default(),
            arg_pool: Vec::new(),
            functions: HashMap::new(),
            disabled_functions: HashMap::new(),
            trace_const_expr: false,
            deprecating: Vec::new(),
            early_binding: false,
            gen_delegators: Vec::new(),
            enums_checked: Default::default(),
            linking_early: false,
            link_deferred: false,
            obligations: Vec::new(),
            delayed_obligations: Vec::new(),
            magic_call_name: None,
            tick_funcs: Vec::new(),
            tick_count: 0,
            in_tick: false,
            tick_running: None,
            const_attrs: HashMap::new(),
            user_functions: Vec::new(),
            classes: HashMap::new(),
            class_order: Vec::new(),
            next_class_id: 1,
            constants: HashMap::new(),
            user_constants: Vec::new(),
            included: Vec::new(),
            included_set: HashSet::new(),
            globals: Globals {
                code: None,
                slots: Box::new([]),
                dyn_vars: PhpArray::new(),
            },
            ini: Ini {
                values: HashMap::new(),
                error_reporting: errors::E_ALL,
                display_errors: 1,
                log_errors: false,
                html_errors: false,
                precision: 14,
                serialize_precision: -1,
                assertions: 1,
                memory_limit: 128 << 20,
                fiber_stack_size: None,
                include_path: PhpStr::from(".:/usr/share/php"),
                max_nesting: 0,
                originals: HashMap::new(),
            },
            out: output::Output::default(),
            err: ErrorState::default(),
            core: CoreClasses::default(),
            autoloaders: Vec::new(),
            defined_in_errors: Vec::new(),
            typed_refs: HashMap::new(),
            user_wrappers: std::collections::HashMap::new(),
            disabled_wrappers: Vec::new(),
            autoloading: Vec::new(),
            shutdown_functions: Vec::new(),
            rmw_key_warned: false,
            header_callback: None,
            exit_status: 0,
            script_path: PhpStr::empty(),
            argv: Vec::new(),
            cwd: std::env::current_dir().unwrap_or_default(),
            lazy_classes: HashMap::new(),
            modules: Vec::new(),
            native_methods: HashMap::new(),
            native_hooks: HashMap::new(),
            ext_state: HashMap::new(),
            units: Vec::new(),
            shutting_down: false,
            gc_enabled: true,
            start_time: std::time::Instant::now(),
            dtor_depth: 0,
            compile_notices_pending: Vec::new(),
            run_depth: 0,
            resources: resources::Resources {
                stdin: Some(stdio.0),
                stdout: Some(stdio.1),
                stderr: Some(stdio.2),
            },
            keep_alive: Vec::new(),
            fiber: None,
            strict_main: false,
            stdout_res: None,
            uncaught_reported: false,
            uninit,
            unset_mark,
            prop_guards: Vec::new(),
            hook_guards: Vec::new(),
            cloning: Vec::new(),
            noop: None,
            compare_depth: 0,
            closure_funcs: HashMap::new(),
            cur_exception: None,
            loc_override: None,
            fold_constants: true,
            const_cache: Vec::new(),
            halt_offsets: HashMap::new(),
            const_eval_names: Vec::new(),
            const_fetch_spelling: None,
            halt_shift: None,
            dump_stack: Vec::new(),
            weakrefs: HashMap::new(),
            last_dir: None,
            compiling_eval: false,
            ex_parents: Vec::new(),
            fiber_stack: Vec::new(),
        };
        for m in modules {
            vm.register_module(m);
        }
        // Core classes the engine uses directly.
        let core = |vm: &mut Vm, n: &str| vm.find_class(n.as_bytes(), false).ok().flatten();
        vm.core.traversable = core(&mut vm, "Traversable");
        vm.core.iterator = core(&mut vm, "Iterator");
        vm.core.iterator_aggregate = core(&mut vm, "IteratorAggregate");
        vm.core.array_access = core(&mut vm, "ArrayAccess");
        vm.core.countable = core(&mut vm, "Countable");
        vm.core.stringable = core(&mut vm, "Stringable");
        vm.core.throwable = core(&mut vm, "Throwable");
        vm.core.std_class = core(&mut vm, "stdClass");
        vm.core.closure = core(&mut vm, "Closure");
        vm.core.generator = core(&mut vm, "Generator");
        vm.core.unit_enum = core(&mut vm, "UnitEnum");
        vm.core.backed_enum = core(&mut vm, "BackedEnum");
        for m in modules {
            if let Some(f) = m.init {
                f(&mut vm);
            }
        }
        vm
    }

    fn register_module(&mut self, m: &'static Module) {
        let mi = self.modules.len();
        self.modules.push(m);
        for &(sig, h) in m.functions {
            let name = sig_name(sig);
            let f = Rc::new(crate::func::Func::native(name, sig, h, 0));
            self.functions.insert(f.lname.clone(), f);
        }
        for (ci, &(lname, _)) in m.classes.iter().enumerate() {
            self.lazy_classes
                .insert(PhpStr::intern(lname.as_bytes()), (mi, ci));
        }
        for &(k, h) in m.methods {
            self.native_methods.insert(PhpStr::intern(k.as_bytes()), h);
        }
        for &(k, h) in m.hooks {
            self.native_hooks.insert(PhpStr::intern(k.as_bytes()), h);
        }
        for &(k, v) in m.constants {
            self.constants
                .insert(PhpStr::intern(k.as_bytes()), v.to_value());
        }
        for &(k, v) in m.ini {
            self.ini
                .values
                .insert(PhpStr::intern(k.as_bytes()), PhpStr::intern(v.as_bytes()));
        }
    }

    /// Sets an ini value (startup `-d` or `ini_set`), updating the typed
    /// settings the engine reads directly.
    pub fn set_ini(&mut self, k: &[u8], v: &[u8]) {
        let key = PhpStr::new(k);
        self.ini.values.insert(key, PhpStr::new(v));
        match k {
            b"error_reporting" => self.ini.error_reporting = parse_error_level(v, self),
            b"display_errors" => {
                let l = v.to_ascii_lowercase();
                self.ini.display_errors = match &l[..] {
                    b"stderr" => 2,
                    b"stdout" | b"1" | b"on" | b"yes" | b"true" => 1,
                    b"0" | b"off" | b"no" | b"false" | b"" => 0,
                    _ => {
                        if goro_runtime::convert::str_to_int(&l) != 0 {
                            1
                        } else {
                            0
                        }
                    }
                };
            }
            b"log_errors" => self.ini.log_errors = ini_truthy(v),
            b"html_errors" => self.ini.html_errors = ini_truthy(v),
            b"precision" => self.ini.precision = goro_runtime::convert::str_to_int(v) as i32,
            b"serialize_precision" => {
                self.ini.serialize_precision = goro_runtime::convert::str_to_int(v) as i32
            }
            b"zend.assertions" => self.ini.assertions = goro_runtime::convert::str_to_int(v),
            b"include_path" => self.ini.include_path = PhpStr::new(v),
            b"memory_limit" => {
                self.ini.memory_limit = parse_size(v);
                self.apply_memory_limit();
            }
            b"implicit_flush" => self.out.implicit_flush = ini_truthy(v),
            b"fiber.stack_size" => self.ini.fiber_stack_size = Some(parse_size(v)),
            b"zend.enable_gc" => self.gc_enabled = ini_truthy(v),
            _ => {}
        }
    }

    pub fn get_ini(&self, k: &[u8]) -> Option<PhpStr> {
        self.ini.values.get(&PhpStr::new(k)).cloned()
    }

    /// Sets `$argv`, `$argc` and `$_SERVER` for the script.
    pub fn set_script(&mut self, path: &[u8], argv: &[Vec<u8>]) {
        self.script_path = PhpStr::new(path);
        let args = PhpArray::from_values(argv.iter().map(|a| Value::Str(PhpStr::new(a))));
        let argc = argv.len() as i64;
        let mut server = PhpArray::new();
        for (k, v) in std::env::vars_os() {
            use std::os::unix::ffi::OsStrExt;
            server.insert(
                Key::from_bytes(k.as_bytes()),
                Value::Str(PhpStr::new(v.as_bytes())),
            );
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        server.insert(Key::from("PHP_SELF"), Value::Str(PhpStr::new(path)));
        server.insert(Key::from("SCRIPT_NAME"), Value::Str(PhpStr::new(path)));
        server.insert(Key::from("SCRIPT_FILENAME"), Value::Str(PhpStr::new(path)));
        server.insert(Key::from("PATH_TRANSLATED"), Value::Str(PhpStr::new(path)));
        server.insert(Key::from("DOCUMENT_ROOT"), Value::Str(PhpStr::empty()));
        server.insert(
            Key::from("REQUEST_TIME_FLOAT"),
            Value::Float(now.as_secs_f64()),
        );
        server.insert(Key::from("REQUEST_TIME"), Value::Int(now.as_secs() as i64));
        server.insert(Key::from("argv"), Value::Array(args.clone()));
        server.insert(Key::from("argc"), Value::Int(argc));
        let g = &mut self.globals.dyn_vars;
        g.insert(Key::from("_GET"), Value::Array(PhpArray::new()));
        g.insert(Key::from("_POST"), Value::Array(PhpArray::new()));
        g.insert(Key::from("_COOKIE"), Value::Array(PhpArray::new()));
        g.insert(Key::from("_FILES"), Value::Array(PhpArray::new()));
        g.insert(Key::from("argv"), Value::Array(args));
        g.insert(Key::from("argc"), Value::Int(argc));
        g.insert(Key::from("_SERVER"), Value::Array(server));
        g.insert(Key::from("_REQUEST"), Value::Array(PhpArray::new()));
        // variables_order "E" fills $_ENV (PHP manual: variables_order;
        // the default is "EGPCS").
        let order = self
            .ini
            .values
            .get(&PhpStr::new(b"variables_order"))
            .map(|v| v.to_vec())
            .unwrap_or_else(|| b"EGPCS".to_vec());
        let mut env = PhpArray::new();
        if order.iter().any(|c| c.eq_ignore_ascii_case(&b'E')) {
            use std::os::unix::ffi::OsStrExt;
            for (k, v) in std::env::vars_os() {
                env.insert(
                    Key::from_bytes(k.as_bytes()),
                    Value::Str(PhpStr::new(v.as_bytes())),
                );
            }
        }
        let g = &mut self.globals.dyn_vars;
        g.insert(Key::from("_ENV"), Value::Array(env));
        self.argv = argv.iter().map(|a| PhpStr::new(a)).collect();
    }

    /// Runs a script with PHP's end-of-request sequence; returns the exit
    /// status.
    pub fn run_script(&mut self, src: &[u8], path: &[u8]) -> i32 {
        self.arm_memory_limit();
        // a script a wrapper runs another way (a tar/zip phar: its stub)
        let filtered = crate::builtins::streams::include_filter(self, path, src);
        let src = filtered.as_deref().unwrap_or(src);
        // PHP manual (php.ini directives): auto_prepend_file and
        // auto_append_file are parsed before/after the main file, as with
        // require.
        let side = |vm: &Vm, k: &[u8]| -> Option<(Vec<u8>, Vec<u8>)> {
            let f = vm.get_ini(k)?;
            if f.is_empty() {
                return None;
            }
            let p = vm.resolve_include(&f)?;
            use std::os::unix::ffi::OsStrExt;
            let src = std::fs::read(&p).ok()?;
            Some((src, p.as_os_str().as_bytes().to_vec()))
        };
        if let Some((s, p)) = side(self, b"auto_prepend_file") {
            let key = PhpStr::new(&p);
            if self.included_set.insert(key.clone()) {
                self.included.push(key);
            }
            let r = self.run_main_code(&s, &p);
            let stop = r.is_err();
            self.handle_unwind(r);
            if stop {
                self.shutdown();
                return self.exit_status;
            }
        }
        let r = self.run_main_code(src, path);
        let ok = r.is_ok();
        self.handle_unwind(r);
        if ok && let Some((s, p)) = side(self, b"auto_append_file") {
            let r = self.run_main_code(&s, &p);
            self.handle_unwind(r);
        }
        self.shutdown();
        self.exit_status
    }

    pub fn handle_unwind(&mut self, r: R<()>) {
        match r {
            Ok(()) => {}
            Err(Unwind::Throw(e)) => {
                self.ex.frames.clear();
                self.ex.pending.clear();
                self.ex.native_calls.clear();
                // The main code's temporaries (a foreach's generator) go
                // as the exception leaves it, before it is reported
                // (observed on PHP 8.5; test Zend/tests/generators/bug69740).
                let ncv = self.globals.code.as_ref().map_or(0, |c| c.num_cvs as usize);
                let temps: Vec<Slot> = self
                    .globals
                    .slots
                    .iter_mut()
                    .skip(ncv)
                    .map(|s| s.take())
                    .collect();
                drop(temps);
                let r = self.run_pending_destructors();
                if let Err(Unwind::Throw(e2)) = r {
                    self.chain_previous(&e2, e.clone());
                    self.ex.frames.clear();
                    self.ex.pending.clear();
                    self.ex.native_calls.clear();
                    self.report_uncaught(e2);
                    return;
                }
                self.report_uncaught(e);
            }
            Err(Unwind::Exit) | Err(Unwind::Fatal) => {
                self.ex.frames.clear();
                self.ex.pending.clear();
                self.ex.native_calls.clear();
            }
        }
    }

    /// End of request: shutdown functions, destructors, output.
    pub fn shutdown(&mut self) {
        self.shutting_down = true;
        // register_shutdown_function callbacks (they may register more).
        let mut i = 0;
        while i < self.shutdown_functions.len() {
            let (f, args) = self.shutdown_functions[i].clone();
            i += 1;
            let r = self.call_value(&f, args).map(|_| ());
            if matches!(r, Err(Unwind::Exit)) {
                break;
            }
            self.handle_unwind(r);
        }
        // Destructors: objects held by global variables first (in order),
        // then everything else still alive (creation order).
        // An exception escaping a destructor at shutdown ends the calls of
        // the remaining destructors (observed on PHP 8.5).
        let r = self.destroy_globals();
        let mut stop = matches!(r, Err(Unwind::Throw(_)));
        self.handle_unwind(r);
        let r = self.run_pending_destructors();
        stop |= matches!(r, Err(Unwind::Throw(_)));
        self.handle_unwind(r);
        if stop {
            for o in goro_runtime::object::live_objects() {
                o.mark_destructor_called();
            }
        }
        if self.exit_status != 255 || true {
            for o in goro_runtime::object::live_objects() {
                if o.needs_destructor() {
                    o.mark_destructor_called();
                    let r = self.call_destructor(o);
                    if matches!(r, Err(Unwind::Exit)) {
                        break;
                    }
                    let thrown = matches!(r, Err(Unwind::Throw(_)));
                    self.handle_unwind(r);
                    if thrown {
                        for o in goro_runtime::object::live_objects() {
                            o.mark_destructor_called();
                        }
                        break;
                    }
                }
            }
        }
        self.ob_end_all();
        self.send_headers();
        crate::builtins::file::close_open_streams(self);
        for m in self.modules.clone() {
            if let Some(f) = m.shutdown {
                f(self);
            }
        }
        self.out.flush();
    }

    fn destroy_globals(&mut self) -> R<()> {
        // Global variables holding the only handle to an object are
        // released last-declared first, repeatedly while that frees more
        // (observed on PHP 8.5: `$a, $b, $c` objects destruct as c, b, a);
        // the remaining objects then destruct in creation order.
        fn sole_object(v: &Value) -> bool {
            match v {
                Value::Object(o) => o.refcount() == 1,
                Value::Ref(r) if r.refcount() == 1 => {
                    matches!(r.get(), Value::Object(o) if o.refcount() == 2)
                }
                _ => false,
            }
        }
        loop {
            let mut changed = false;
            let keys: Vec<goro_runtime::Key> = self
                .globals
                .dyn_vars
                .iter()
                .filter(|(_, v)| sole_object(v))
                .map(|(k, _)| k.to_owned())
                .collect();
            for k in keys.into_iter().rev() {
                let v = self.globals.dyn_vars.remove(k.as_ref());
                drop(v);
                changed = true;
                self.run_pending_destructors()?;
            }
            for i in (0..self.globals.slots.len()).rev() {
                if self.globals.slots[i].as_ref().is_some_and(sole_object) {
                    let v = self.globals.slots[i].take();
                    drop(v);
                    changed = true;
                    self.run_pending_destructors()?;
                }
            }
            if !changed {
                break;
            }
        }
        self.run_pending_destructors()?;
        Ok(())
    }

    /// `gc_collect_cycles()`.
    pub fn collect_cycles(&mut self) -> R<usize> {
        let mut dtors = Vec::new();
        let n = goro_runtime::gc::collect_cycles(&mut |o| dtors.push(o));
        let mut total = n;
        if !dtors.is_empty() {
            for o in dtors {
                self.call_destructor(o)?;
            }
            total += goro_runtime::gc::collect_cycles(&mut |o| {
                // Destructors already called once.
                let _ = o;
            });
        }
        Ok(total)
    }
}

fn ini_truthy(v: &[u8]) -> bool {
    let l = v.to_ascii_lowercase();
    match &l[..] {
        b"1" | b"on" | b"yes" | b"true" => true,
        b"" | b"0" | b"off" | b"no" | b"false" | b"none" => false,
        _ => goro_runtime::convert::str_to_int(&l) != 0,
    }
}

// ------------------------------------------------------------ memory_limit

static MEM_VM: std::sync::atomic::AtomicPtr<Vm> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
/// Heap use when the script started: the limit counts from there.
static MEM_BASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// The allocation that crossed the limit.
static MEM_TRIED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// `--unlimited-mem`: no backstop with memory_limit=-1.
static MEM_UNLIMITED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The cap applied when memory_limit is -1 (unlimited): a backstop
/// against runaway scripts, lifted by `--unlimited-mem`.
pub const MEM_BACKSTOP: i64 = 4 << 30;

/// `--unlimited-mem`: memory_limit=-1 really means unlimited.
pub fn set_unlimited_memory(on: bool) {
    MEM_UNLIMITED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The allocator hook: remembers the request; when the headroom is gone
/// too, reports the fatal error and ends the process at once.
fn memory_hook(size: usize, hard: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    MEM_TRIED.store(size, Relaxed);
    if !hard {
        return;
    }
    let p = MEM_VM.load(Relaxed);
    if !p.is_null() {
        // SAFETY: the VM outlives the script run that armed the hook; the
        // process ends here, so nothing observes the state afterwards.
        let vm = unsafe { &mut *p };
        let msg = vm.memory_exhausted_msg(size);
        let file = vm.cur_file();
        let line = vm.cur_line();
        vm.display_error(super::errors::E_ERROR, &msg, &file, line);
        let pending: Vec<u8> = vm
            .out
            .buffers
            .iter()
            .flat_map(|b| b.data.iter().copied())
            .collect();
        vm.out.write_stdout(&pending);
        vm.out.flush();
    } else {
        eprintln!("Fatal error: Allowed memory size exhausted (tried to allocate {size} bytes)");
    }
    std::process::exit(255);
}

impl Vm {
    /// Starts enforcing memory_limit for this script (PHP manual:
    /// memory_limit).
    pub fn arm_memory_limit(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        MEM_VM.store(self as *mut Vm, Relaxed);
        MEM_BASE.store(goro_runtime::mem::used(), Relaxed);
        goro_runtime::mem::reset_peak();
        goro_runtime::mem::set_hook(memory_hook);
        self.apply_memory_limit();
    }

    /// The effective limit in bytes (the backstop for -1, `None` when
    /// really unlimited).
    fn effective_memory_limit(&self) -> Option<i64> {
        let l = self.ini.memory_limit;
        if l >= 0 {
            Some(l)
        } else if MEM_UNLIMITED.load(std::sync::atomic::Ordering::Relaxed) {
            None
        } else {
            Some(MEM_BACKSTOP)
        }
    }

    /// Re-arms the allocator limit after a memory_limit change.
    pub(crate) fn apply_memory_limit(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if MEM_VM.load(Relaxed).is_null() {
            return;
        }
        let base = MEM_BASE.load(Relaxed);
        let lim = self
            .effective_memory_limit()
            .map(|l| base.saturating_add(l as usize));
        goro_runtime::mem::set_limit(lim);
    }

    /// The highest heap use of the script (counted from its start).
    pub fn memory_peak(&self) -> usize {
        goro_runtime::mem::peak()
            .saturating_sub(MEM_BASE.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Heap bytes the script uses (counted from its start).
    pub fn memory_in_use(&self) -> usize {
        goro_runtime::mem::used()
            .saturating_sub(MEM_BASE.load(std::sync::atomic::Ordering::Relaxed))
    }

    fn memory_exhausted_msg(&self, size: usize) -> String {
        format!(
            "Allowed memory size of {} bytes exhausted (tried to allocate {} bytes)",
            self.effective_memory_limit().unwrap_or(MEM_BACKSTOP),
            size
        )
    }

    /// The fatal error for a crossed memory_limit, at a safe point.
    #[cold]
    #[inline(never)]
    pub(crate) fn memory_exhausted(&mut self) -> super::Unwind {
        goro_runtime::mem::take_pending();
        let size = MEM_TRIED.load(std::sync::atomic::Ordering::Relaxed);
        let msg = self.memory_exhausted_msg(size);
        self.fatal(&msg)
    }
}

/// `128M` and friends.
pub fn parse_size(v: &[u8]) -> i64 {
    parse_quantity(v).0
}

/// Shows a quantity in a warning: control and non-ASCII bytes as `\xNN`.
fn quantity_shown(v: &[u8]) -> String {
    let mut s = String::new();
    for &b in v {
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02X}"));
        }
    }
    s
}

/// Parses an ini quantity (PHP manual: ini_parse_quantity(); rules
/// observed on PHP 8.5 and in tests Zend/tests/zend_ini/*): surrounding
/// whitespace, an optional sign, a `0x`/`0o`/`0b` prefix or a leading `0`
/// (octal), digits, optional whitespace and a `k`/`m`/`g` multiplier
/// (powers of 1024). Malformed input still gives a value, with the warning
/// to emit.
pub fn parse_quantity(v: &[u8]) -> (i64, Option<String>) {
    let is_ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c');
    let shown = || quantity_shown(v);
    let mut start = 0;
    while start < v.len() && is_ws(v[start]) {
        start += 1;
    }
    let mut end = v.len();
    while end > start && is_ws(v[end - 1]) {
        end -= 1;
    }
    if start == end {
        return (0, None);
    }
    let last = v[end - 1];
    let shift = match last.to_ascii_lowercase() {
        b'k' => 10,
        b'm' => 20,
        b'g' => 30,
        _ => 0,
    };
    // The digits end before the multiplier.
    let dend = if shift > 0 { end - 1 } else { end };
    let mut p = start;
    let neg = match v[p] {
        b'-' => {
            p += 1;
            true
        }
        b'+' => {
            p += 1;
            false
        }
        _ => false,
    };
    let no_digits = || {
        Some(format!(
            "Invalid quantity \"{}\": no valid leading digits, interpreting as \"0\" for backwards compatibility",
            shown()
        ))
    };
    let mut base = 10u32;
    if p < end && v[p] == b'0' && p + 1 < end {
        let c = v[p + 1];
        let prefix = match c.to_ascii_lowercase() {
            b'x' => Some(16),
            b'o' => Some(8),
            b'b' => Some(2),
            _ => None,
        };
        if let Some(b) = prefix {
            base = b;
            p += 2;
            // strtoull() would take a sign, whitespace or a second prefix
            // here (test Zend/tests/zend_ini/gh11876).
            let eaten = p == end
                || is_ws(v[p])
                || v[p] == b'+'
                || v[p] == b'-'
                || (b != 8 && v[p] == b'0' && p + 1 < end && v[p + 1].eq_ignore_ascii_case(&c));
            if eaten {
                return (
                    0,
                    Some(format!(
                        "Invalid quantity \"{}\": no digits after base prefix, interpreting as \"0\" for backwards compatibility",
                        shown()
                    )),
                );
            }
        } else if c.is_ascii_digit() {
            base = 8;
        } else if !matches!(c.to_ascii_lowercase(), b'k' | b'm' | b'g') {
            return (
                0,
                Some(format!(
                    "Invalid prefix \"{}\", interpreting as \"0\" for backwards compatibility",
                    quantity_shown(&v[p..p + 2])
                )),
            );
        }
    }
    let dstart = p;
    let mut u: u64 = 0;
    let mut over = false;
    while p < dend {
        let d = match (v[p] as char).to_digit(base) {
            Some(d) => d as u64,
            None => break,
        };
        match u.checked_mul(base as u64).and_then(|x| x.checked_add(d)) {
            Some(x) => u = x,
            None => {
                over = true;
                u = u64::MAX;
            }
        }
        p += 1;
    }
    if p == dstart {
        return (0, no_digits());
    }
    let digits_end = p;
    let mut q = p;
    while q < dend && is_ws(v[q]) {
        q += 1;
    }
    let mut warning = None;
    let mut shift = shift;
    if shift > 0 {
        if q < dend {
            let mut interp = quantity_shown(&v[..q]);
            interp.push(last as char);
            warning = Some(format!(
                "Invalid quantity \"{}\", interpreting as \"{}\" for backwards compatibility",
                shown(),
                interp
            ));
        }
    } else if q < end {
        shift = 0;
        warning = Some(format!(
            "Invalid quantity \"{}\": unknown multiplier \"{}\", interpreting as \"{}\" for backwards compatibility",
            shown(),
            quantity_shown(&[last]),
            quantity_shown(&v[..q])
        ));
    }
    let _ = digits_end;
    let shifted = u.wrapping_shl(shift);
    if shift > 0 && (shifted >> shift) != u {
        over = true;
    }
    let lim = if neg { 1u64 << 63 } else { i64::MAX as u64 };
    if over || shifted > lim {
        return (
            shifted as i64,
            Some(format!(
                "Invalid quantity \"{}\": value is out of range, using overflow result for backwards compatibility",
                shown()
            )),
        );
    }
    let val = if neg {
        (shifted as i64).wrapping_neg()
    } else {
        shifted as i64
    };
    (val, warning)
}

/// error_reporting values may be expressions of E_* constants
/// (`E_ALL & ~E_NOTICE`).
fn parse_error_level(v: &[u8], vm: &Vm) -> i64 {
    let s = String::from_utf8_lossy(v);
    let s = s.trim();
    if let Ok(n) = s.parse::<i64>() {
        return n;
    }
    // Tiny expression evaluator: | & ~ ^ and names.
    let toks: Vec<String> = {
        let mut t = Vec::new();
        let mut cur = String::new();
        for c in s.chars() {
            if c.is_alphanumeric() || c == '_' {
                cur.push(c);
            } else {
                if !cur.is_empty() {
                    t.push(std::mem::take(&mut cur));
                }
                if !c.is_whitespace() {
                    t.push(c.to_string());
                }
            }
        }
        if !cur.is_empty() {
            t.push(cur);
        }
        t
    };
    let val = |name: &str| -> i64 {
        if let Ok(n) = name.parse::<i64>() {
            return n;
        }
        match vm.constants.get(&PhpStr::from(name)) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        }
    };
    // Left-to-right with ~ prefix.
    let mut result: i64 = 0;
    let mut op = '|';
    let mut i = 0;
    while i < toks.len() {
        let mut neg = false;
        while i < toks.len() && toks[i] == "~" {
            neg = !neg;
            i += 1;
        }
        if i >= toks.len() {
            break;
        }
        let mut x = val(&toks[i]);
        if neg {
            x = !x;
        }
        result = match op {
            '&' => result & x,
            '^' => result ^ x,
            _ => result | x,
        };
        i += 1;
        if i < toks.len() {
            op = toks[i].chars().next().unwrap_or('|');
            i += 1;
        }
    }
    result
}
