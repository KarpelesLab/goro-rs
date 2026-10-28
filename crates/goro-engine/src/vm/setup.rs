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
            b"memory_limit" => self.ini.memory_limit = parse_size(v),
            b"implicit_flush" => self.out.implicit_flush = ini_truthy(v),
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

/// `128M` and friends.
pub fn parse_size(v: &[u8]) -> i64 {
    let v = v.trim_ascii();
    if v.is_empty() {
        return 0;
    }
    let (num, mul) = match v.last().unwrap().to_ascii_lowercase() {
        b'k' => (&v[..v.len() - 1], 1024),
        b'm' => (&v[..v.len() - 1], 1024 * 1024),
        b'g' => (&v[..v.len() - 1], 1024 * 1024 * 1024),
        _ => (v, 1),
    };
    goro_runtime::convert::str_to_int(num).saturating_mul(mul)
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
