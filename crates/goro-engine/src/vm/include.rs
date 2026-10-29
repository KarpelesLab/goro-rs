//! Compiling and running files and `eval()` code; global and
//! variable-variable access; scopes shared by included code; constants.
//!
//! PHP manual: include, require, include_once, eval, Variable scope,
//! Variable variables, Constants.

use std::rc::Rc;

use goro_runtime::{Key, KeyRef, PhpArray, PhpRef, PhpStr, Value};

use super::{Frame, R, Slot, StackMark, Unwind, Vm, frame_flags};
use crate::bytecode::*;
use crate::compiler::Unit;
use crate::func::Func;

/// Stack the parser may use (it runs on the engine's large thread stack).
pub const PARSER_STACK: usize = 512 << 20;

/// The __halt_compiler() data offset of the original source, from the one
/// of its converted form (the keyword is ASCII in both).
fn halt_offset_in(orig: &[u8], conv: &[u8], off: usize) -> usize {
    let kw = b"__halt_compiler";
    let find_all = |s: &[u8]| -> Vec<usize> {
        (0..s.len().saturating_sub(kw.len() - 1))
            .filter(|&i| s[i..i + kw.len()].eq_ignore_ascii_case(kw))
            .collect()
    };
    let c = find_all(conv);
    let o = find_all(orig);
    match c.iter().rposition(|&p| p < off) {
        Some(n) if n < o.len() => o[n] + (off - c[n]),
        _ => off,
    }
}

impl Vm {
    // ------------------------------------------------------------ compiling

    /// Parses and compiles source code.
    pub fn compile_source(
        &mut self,
        src: &[u8],
        file: &[u8],
        eval: bool,
    ) -> Result<Unit, crate::compiler::CompileError> {
        // zend.multibyte: the script is read in zend.script_encoding and
        // its literals turned into internal_encoding (PHP manual: Runtime
        // Configuration, zend.multibyte; tests ext/mbstring/tests/
        // zend_multibyte-*).
        let mb = self.ini_bool(b"zend.multibyte", false);
        let mut converted: Option<Vec<u8>> = None;
        let mut lit_enc = None;
        if mb {
            let first = |v: Option<PhpStr>| -> Option<goro_charset::Encoding> {
                let v = v?;
                let n = v.split(|&c| c == b',').next()?.trim_ascii().to_vec();
                goro_charset::Encoding::from_name(&n)
            };
            let se = first(self.get_ini(b"zend.script_encoding"));
            let ie = first(self.get_ini(b"internal_encoding"))
                .or_else(|| first(self.get_ini(b"mbstring.internal_encoding")))
                .unwrap_or(goro_charset::Encoding::Utf8);
            if let Some(se) = se {
                if se != goro_charset::Encoding::Utf8 {
                    converted = Some(
                        goro_charset::convert(
                            src,
                            goro_charset::Encoding::Utf8,
                            se,
                            &Default::default(),
                        )
                        .bytes,
                    );
                }
                if ie != goro_charset::Encoding::Utf8 {
                    lit_enc = Some(ie);
                }
            }
        }
        let orig_src = src;
        let src: &[u8] = converted.as_deref().unwrap_or(src);
        let arena = goro_syntax::Arena::new();
        // The AST borrows the source; keep a copy alive for the call.
        let opts = goro_syntax::ParseOptions {
            short_open_tag: self.ini_bool(b"short_open_tag", true),
            in_scripting: eval,
            stack_limit: PARSER_STACK,
            encoding_checks: false,
        };
        let prog = goro_syntax::parse_with(&arena, src, opts).map_err(|e| {
            crate::compiler::CompileError {
                message: format!(
                    "{}{}",
                    if e.kind == goro_syntax::ErrorKind::Parse {
                        "\u{1}"
                    } else {
                        "\u{2}"
                    },
                    e.message
                ),
                line: e.line,
                notices: Vec::new(),
            }
        })?;
        let consts = &self.constants;
        let attrs = &self.const_attrs;
        let kc = |n: &[u8]| -> Option<Value> {
            // Only engine constants that can't be redefined; deprecated
            // ones (also those with attributes) warn at run time.
            if crate::builtins::misc::const_deprecation(n).is_some()
                || attrs.contains_key(&PhpStr::new(n))
            {
                return None;
            }
            let v = consts.get(&PhpStr::new(n))?;
            match v {
                Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Null | Value::Str(_) => {
                    Some(v.clone())
                }
                _ => None,
            }
        };
        let kc_ref: &dyn Fn(&[u8]) -> Option<Value> = &kc;
        let kc_opt = if self.fold_constants {
            Some(kc_ref)
        } else {
            None
        };
        crate::compiler::ZEND_MULTIBYTE.with(|c| c.set(mb));
        let was = crate::compiler::LITERAL_ENCODING.with(|c| c.replace(lit_enc));
        let unit = crate::compiler::compile_program(&prog, src, file, kc_opt, None);
        crate::compiler::LITERAL_ENCODING.with(|c| c.set(was));
        let unit = unit?;
        // Development aid: GORO_DUMP_OPS=1 prints the bytecode of user code.
        if !file.is_empty() && std::env::var_os("GORO_DUMP_OPS").is_some() {
            fn dump(c: &crate::bytecode::FuncCode) {
                eprintln!("== {} ({} slots)", c.name.to_string_lossy(), c.num_slots);
                for (i, op) in c.ops.iter().enumerate() {
                    eprintln!("{:4} {:?}", i, op);
                }
                for ch in &c.children {
                    dump(ch);
                }
                for cl in &c.classes {
                    for m in &cl.methods {
                        dump(&m.code);
                    }
                }
            }
            dump(&unit.main);
        }
        if let Some(off) = prog.halt_compiler_offset {
            let off = if converted.is_some() {
                halt_offset_in(orig_src, src, off as usize) as u32
            } else {
                off
            };
            let mut off = off as i64;
            if let Some((f, d)) = &self.halt_shift
                && f.as_bytes() == file
            {
                off += d;
            }
            self.halt_offsets.insert(PhpStr::new(file), off);
        }
        Ok(unit)
    }

    pub(crate) fn ini_bool(&self, k: &[u8], default: bool) -> bool {
        match self.ini.values.get(&PhpStr::new(k)) {
            Some(v) => {
                let l = v.to_ascii_lowercase();
                matches!(&l[..], b"1" | b"on" | b"yes" | b"true")
            }
            None => default,
        }
    }

    /// Declares the hoisted functions and early-bindable classes of a unit.
    pub(crate) fn load_unit(&mut self, unit: &Unit) -> R<()> {
        for n in &unit.notices {
            let file = unit.main.file.clone();
            self.raise_at(n.level.errno(), &n.message, file, n.line)?;
        }
        for &i in &unit.hoisted_funcs {
            let code = unit.main.children[i as usize].clone();
            self.declare_function(code)?;
        }
        for &i in &unit.toplevel_classes {
            let decl = unit.main.classes[i as usize].clone();
            if self.can_early_bind(&decl) {
                self.early_bind_class(&decl)?;
            }
        }
        Ok(())
    }

    /// Reports a compile error: parse errors as `ParseError` exceptions in
    /// eval/include, fatal errors otherwise.
    pub(crate) fn compile_error(
        &mut self,
        mut e: crate::compiler::CompileError,
        file: &[u8],
        throw: bool,
    ) -> Unwind {
        for n in std::mem::take(&mut e.notices) {
            if let Err(u) = self.raise_at(n.level.errno(), &n.message, PhpStr::new(file), n.line) {
                return u;
            }
        }
        let (is_parse, msg) = match e.message.strip_prefix('\u{1}') {
            Some(m) => (true, m.to_string()),
            None => (false, e.message),
        };
        // eval() throws the parser's other errors as CompileError (test
        // Zend/tests/bug75218); the compiler's are fatal there too
        // (observed on PHP 8.5: "Key element cannot be a reference").
        // include throws only parse errors.
        let (from_parser, msg) = match msg.strip_prefix('\u{2}') {
            Some(m) => (true, m.to_string()),
            None => (false, msg),
        };
        let throw_compile = throw && self.compiling_eval && from_parser;
        if (is_parse && throw) || throw_compile {
            let class = if is_parse {
                "ParseError"
            } else {
                "CompileError"
            };
            match self.create_exception(class, &msg, 0) {
                Ok(o) => {
                    self.set_prop_raw(&o, b"file", Value::Str(PhpStr::new(file)));
                    self.set_prop_raw(&o, b"line", Value::Int(e.line as i64));
                    return Unwind::Throw(o);
                }
                Err(u) => return u,
            }
        }
        let label_level = if is_parse {
            super::errors::E_PARSE
        } else {
            super::errors::E_COMPILE_ERROR
        };
        self.err.last = Some((
            label_level,
            PhpStr::from(msg.clone()),
            PhpStr::new(file),
            e.line as i64,
        ));
        if self.ini.error_reporting & label_level != 0 {
            self.display_error(label_level, &msg, file, e.line);
        }
        self.exit_status = 255;
        Unwind::Fatal
    }

    // ------------------------------------------------------------ main script

    /// Runs the main script (global scope).
    pub fn run_main_code(&mut self, src: &[u8], file: &[u8]) -> R<()> {
        self.run_main_code_value(src, file).map(|_| ())
    }

    /// Runs `src` as the main script; returns its `return` value (the web
    /// server's router script: PHP manual, Built-in web server).
    pub fn run_main_code_value(&mut self, src: &[u8], file: &[u8]) -> R<Value> {
        let unit = match self.compile_source(src, file, false) {
            Ok(u) => u,
            Err(e) => return Err(self.compile_error(e, file, false)),
        };
        let main = unit.main.clone();
        self.units.push(main.clone());
        // Global variables live in the main code's slots.
        let n = main.num_slots as usize;
        let mut slots: Box<[Slot]> = (0..n).map(|_| None).collect();
        let base = slots.as_mut_ptr();
        // Superglobals and argv/argc set before the script runs.
        let pre = std::mem::take(&mut self.globals.dyn_vars);
        self.globals = super::Globals {
            code: Some(main.clone()),
            slots,
            dyn_vars: PhpArray::new(),
        };
        for (k, v) in pre.iter() {
            if let KeyRef::Str(name) = k
                && let Some(i) = main.cv_slot(name)
            {
                self.globals.slots[i as usize] = Some(v.clone());
                continue;
            }
            self.globals.dyn_vars.insert(k.to_owned(), v.clone());
        }
        self.strict_main = main.flags & fflags::STRICT != 0;
        let func = Rc::new(Func::user(main.clone(), 0));
        let frame = Frame {
            code: Rc::as_ptr(&main),
            func,
            ip: 0,
            base,
            this: None,
            static_class: std::ptr::null(),
            scope: std::ptr::null(),
            ret: std::ptr::null_mut(),
            argc: 0,
            extra_args: Vec::new(),
            dyn_vars: None,
            closure: None,
            generator: None,
            flags: frame_flags::ENTRY | frame_flags::GLOBAL | frame_flags::OWN_SLOTS,
            mark: StackMark::default(),
            statics: &main.statics,
            owner: usize::MAX,
            pending_base: 0,
            native_base: 0,
            line_hint: 0,
        };
        self.push_frame(frame);
        if let Err(e) = self.load_unit(&unit) {
            self.unwind_frames(0);
            return Err(e);
        }
        self.run()
    }

    // ------------------------------------------------------------ include

    /// Resolves an include path (PHP manual: include: absolute and relative
    /// paths, include_path, the calling script's directory).
    pub fn resolve_include(&self, path: &[u8]) -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
        if path.is_empty() {
            return None;
        }
        if p.is_absolute() {
            return p.is_file().then(|| p.to_path_buf());
        }
        let rel = path.starts_with(b"./") || path.starts_with(b"../");
        if !rel {
            for dir in self.ini.include_path.split(|&c| c == b':') {
                if dir.is_empty() {
                    continue;
                }
                let d = std::path::Path::new(std::ffi::OsStr::from_bytes(dir));
                let c = if d.is_absolute() {
                    d.join(p)
                } else {
                    self.cwd.join(d).join(p)
                };
                if c.is_file() {
                    return Some(c);
                }
            }
            // The directory of the calling script.
            let f = self.cur_file();
            let fp = std::path::Path::new(std::ffi::OsStr::from_bytes(&f));
            if let Some(dir) = fp.parent() {
                let c = dir.join(p);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
        let c = self.cwd.join(p);
        c.is_file().then_some(c)
    }

    pub(crate) fn op_include(&mut self, dst: Reg, v: Value, kind: IncludeKind) -> R<()> {
        let path = self.to_php_str(&v)?;
        let fname = match kind {
            IncludeKind::Include => "include",
            IncludeKind::IncludeOnce => "include_once",
            IncludeKind::Require => "require",
            IncludeKind::RequireOnce => "require_once",
        };
        if let Some(cls) = crate::builtins::userstream::wrapper_for(self, &path) {
            let Some(src) = crate::builtins::userstream::read_all(self, fname, &cls, &path)? else {
                self.warn(&format!(
                    "{}(): Failed opening '{}' for inclusion (include_path='{}')",
                    fname,
                    path.to_string_lossy(),
                    self.ini.include_path.to_string_lossy()
                ))?;
                self.put(dst, Value::Bool(false));
                return Ok(());
            };
            let v = self.run_attached(&src, &path, false, fname)?;
            self.put(dst, v);
            return Ok(());
        }
        if let Some(op) = crate::builtins::streams::wrapper_for(self, &path).and_then(|w| w.include)
        {
            if self.include_native(dst, op, &path, kind, fname, false)? {
                return Ok(());
            }
            return self.include_failed(dst, &path, kind, fname);
        }
        let resolved = self.resolve_include(&path);
        if resolved.is_none()
            && !path.is_empty()
            && !path.starts_with(b"/")
            && !path.windows(3).any(|w| w == b"://")
        {
            // A relative path from a file inside a wrapper with an include
            // hook (phar://): include_path entries of that wrapper, then
            // the directory of the calling script (PHP manual: Phar, using
            // Phar archives).
            // A path starting with ./ or ../ is relative to the working
            // directory only (PHP manual: include), which a front controller
            // sets inside its archive (test ext/phar/tests/frontcontroller34).
            let dotted = path.starts_with(b"./") || path.starts_with(b"../");
            let virtual_cwd = if dotted {
                crate::builtins::streams::virtual_cwd(self)
            } else {
                None
            };
            let dirs: Vec<Vec<u8>> = match virtual_cwd {
                Some(d) => vec![d],
                None => {
                    let mut dirs: Vec<Vec<u8>> =
                        crate::builtins::streams::split_include_path(&self.ini.include_path)
                            .into_iter()
                            .filter(|d| d.windows(3).any(|w| w == b"://"))
                            .collect();
                    let cur = self.cur_file();
                    if let Some(p) = cur.iter().rposition(|&c| c == b'/')
                        && cur.windows(3).any(|w| w == b"://")
                    {
                        dirs.push(cur[..p].to_vec());
                    }
                    dirs
                }
            };
            for mut d in dirs {
                let Some(op) =
                    crate::builtins::streams::wrapper_for(self, &d).and_then(|w| w.include)
                else {
                    continue;
                };
                let rel = path.strip_prefix(b"./").unwrap_or(&path);
                d.push(b'/');
                d.extend_from_slice(rel);
                if self.include_native(dst, op, &d, kind, fname, true)? {
                    return Ok(());
                }
            }
        }
        let Some(resolved) = resolved else {
            if path.is_empty() {
                self.raise(0, "")?;
                let is_req = matches!(kind, IncludeKind::Require | IncludeKind::RequireOnce);
                if is_req {
                    return self.value_error("Path cannot be empty");
                }
                return self.value_error("Path cannot be empty");
            }
            self.warn(&format!(
                "{}({}): Failed to open stream: No such file or directory",
                fname,
                path.to_string_lossy()
            ))?;
            let ip = self.ini.include_path.to_string_lossy();
            if matches!(kind, IncludeKind::Require | IncludeKind::RequireOnce) {
                let msg = format!(
                    "Failed opening required '{}' (include_path='{}')",
                    path.to_string_lossy(),
                    ip
                );
                return Err(self.fatal(&msg));
            }
            self.warn(&format!(
                "{}(): Failed opening '{}' for inclusion (include_path='{}')",
                fname,
                path.to_string_lossy(),
                ip
            ))?;
            self.put(dst, Value::Bool(false));
            return Ok(());
        };
        let canon = std::fs::canonicalize(&resolved).unwrap_or(resolved.clone());
        use std::os::unix::ffi::OsStrExt;
        let key = PhpStr::new(canon.as_os_str().as_bytes());
        if matches!(kind, IncludeKind::IncludeOnce | IncludeKind::RequireOnce)
            && self.included_set.contains(&key)
        {
            self.put(dst, Value::Bool(true));
            return Ok(());
        }
        let src = match std::fs::read(&resolved) {
            // an archive a wrapper runs another way (a tar- or zip-based
            // phar runs its stub; PHP manual: Phar)
            Ok(s) => match crate::builtins::streams::include_filter(
                self,
                canon.as_os_str().as_bytes(),
                &s,
            ) {
                Some(f) => f,
                None => s,
            },
            Err(_) => {
                self.warn(&format!(
                    "{}({}): Failed to open stream: Permission denied",
                    fname,
                    path.to_string_lossy()
                ))?;
                self.put(dst, Value::Bool(false));
                return Ok(());
            }
        };
        if !self.included_set.contains(&key) {
            self.included_set.insert(key.clone());
            self.included.push(key.clone());
        }
        // The file name as PHP reports it: the resolved path.
        let shown = if std::path::Path::new(std::ffi::OsStr::from_bytes(&path)).is_absolute() {
            path.to_vec()
        } else {
            canon.as_os_str().as_bytes().to_vec()
        };
        let v = self.run_attached(&src, &shown, false, fname)?;
        self.put(dst, v);
        Ok(())
    }

    /// include of a URL through a native wrapper's include hook (which
    /// warns when it can't open the file, unless `quiet`): false when the
    /// file can't be read.
    fn include_native(
        &mut self,
        dst: Reg,
        op: crate::builtins::streams::IncludeHook,
        url: &[u8],
        kind: IncludeKind,
        fname: &str,
        quiet: bool,
    ) -> R<bool> {
        let Some((src, name)) = op(self, fname, url, quiet)? else {
            return Ok(false);
        };
        let key = PhpStr::new(&name);
        if self.included_set.contains(&key) {
            if matches!(kind, IncludeKind::IncludeOnce | IncludeKind::RequireOnce) {
                self.put(dst, Value::Bool(true));
                return Ok(true);
            }
        } else {
            self.included_set.insert(key.clone());
            self.included.push(key);
        }
        let v = self.run_attached(&src, &name, false, fname)?;
        self.put(dst, v);
        Ok(true)
    }

    /// The end of an include whose file can't be opened.
    fn include_failed(&mut self, dst: Reg, path: &[u8], kind: IncludeKind, fname: &str) -> R<()> {
        let ip = self.ini.include_path.to_string_lossy();
        if matches!(kind, IncludeKind::Require | IncludeKind::RequireOnce) {
            let msg = format!(
                "Failed opening required '{}' (include_path='{}')",
                String::from_utf8_lossy(path),
                ip
            );
            return Err(self.fatal(&msg));
        }
        self.warn(&format!(
            "{}(): Failed opening '{}' for inclusion (include_path='{}')",
            fname,
            String::from_utf8_lossy(path),
            ip
        ))?;
        self.put(dst, Value::Bool(false));
        Ok(())
    }

    /// Runs a file once, found through include_path, without diagnostics
    /// when it is missing (PHP manual: spl_autoload()). Whether the file
    /// exists.
    pub fn include_once_quiet(&mut self, path: &[u8]) -> R<bool> {
        use std::os::unix::ffi::OsStrExt;
        let Some(resolved) = self.resolve_include(path) else {
            return Ok(false);
        };
        let canon = std::fs::canonicalize(&resolved).unwrap_or(resolved.clone());
        let key = PhpStr::new(canon.as_os_str().as_bytes());
        if self.included_set.contains(&key) {
            return Ok(true);
        }
        let Ok(src) = std::fs::read(&resolved) else {
            return Ok(false);
        };
        self.included_set.insert(key.clone());
        self.included.push(key.clone());
        self.run_attached(&src, &key, false, "include_once")?;
        Ok(true)
    }

    /// Runs a file in the current scope as a front controller does
    /// (Phar::webPhar): like an include, but without a frame of its own in
    /// stack traces (test ext/phar/tests/frontcontroller29.phpt: the trace
    /// goes from the file's code straight to Phar::webPhar()).
    pub fn run_front_file(&mut self, src: &[u8], file: &[u8]) -> R<Value> {
        let key = PhpStr::new(file);
        if !self.included_set.contains(&key) {
            self.included_set.insert(key.clone());
            self.included.push(key);
        }
        self.run_attached(src, file, false, "front controller")
    }

    /// Compiles and runs code in the current scope (include, eval).
    fn run_attached(&mut self, src: &[u8], file: &[u8], eval: bool, what: &str) -> R<Value> {
        let unit = match self.compile_source(src, file, eval) {
            Ok(u) => u,
            Err(e) => {
                self.compiling_eval = eval;
                let u = self.compile_error(e, file, true);
                self.compiling_eval = false;
                return Err(u);
            }
        };
        let main = unit.main.clone();
        self.units.push(main.clone());
        self.load_unit(&unit)?;
        let kind_hint = match what {
            "include" => 1,
            "include_once" => 2,
            "require" => 3,
            "require_once" => 4,
            "front controller" => 6,
            _ => 5,
        };
        // The owner of the variables.
        let cur = self.frame();
        let owner = if cur.flags & frame_flags::GLOBAL != 0 {
            usize::MAX
        } else if cur.flags & frame_flags::ATTACHED != 0 {
            cur.owner
        } else {
            self.ex.frames.len() - 1
        };
        let this = cur.this.clone();
        let static_class = cur.static_class;
        let scope = cur.scope;
        let n = main.num_slots as usize;
        let (base, mark) = self.ex.stack.alloc(n);
        let func = Rc::new(Func::user(main.clone(), 0));
        if !scope.is_null() {
            func.scope.set(scope);
        }
        let mut result: Slot = None;
        let frame = Frame {
            code: Rc::as_ptr(&main),
            func,
            ip: 0,
            base,
            this,
            static_class,
            scope,
            ret: &mut result as *mut Slot,
            argc: 0,
            extra_args: Vec::new(),
            dyn_vars: None,
            closure: None,
            generator: None,
            flags: frame_flags::ENTRY | frame_flags::ATTACHED,
            mark,
            statics: &main.statics,
            owner,
            pending_base: self.ex.pending.len() as u32,
            native_base: self.ex.native_calls.len() as u32,
            line_hint: kind_hint,
        };
        self.push_frame(frame);
        // Bind variables the owner already has.
        for i in 0..main.num_cvs {
            let name = main.cv_names[i as usize].clone();
            if let Some(p) = self.owner_var_ptr(owner, &name, false) {
                let r = unsafe { (*p).make_ref() };
                unsafe { *base.add(i as usize) = Some(Value::Ref(r)) };
            }
        }
        let r = self.run();
        let _ = what;
        r
    }

    pub(crate) fn op_eval(&mut self, dst: Reg, v: Value) -> R<()> {
        let code = self.to_php_str(&v)?;
        let file = format!(
            "{}({}) : eval()'d code",
            self.cur_file().to_string_lossy(),
            self.cur_line()
        );
        let r = self.run_attached(&code, file.as_bytes(), true, "eval")?;
        self.put(dst, r);
        Ok(())
    }

    // ------------------------------------------------------------ attached scopes

    /// Pointer to a variable of an owner scope (the global scope when
    /// `owner == usize::MAX`), creating it as null with `create`.
    pub(crate) fn owner_var_ptr(
        &mut self,
        owner: usize,
        name: &PhpStr,
        create: bool,
    ) -> Option<*mut Value> {
        if owner == usize::MAX {
            return self.global_var_ptr(name, create);
        }
        let f = &mut self.ex.frames[owner];
        let code = f.code();
        if let Some(i) = code.cv_slot(name) {
            let p = unsafe { &mut *f.base.add(i as usize) };
            if p.is_none() {
                if !create {
                    return None;
                }
                *p = Some(Value::Null);
            }
            return p.as_mut().map(|v| v as *mut Value);
        }
        let dv = f.dyn_vars.get_or_insert_with(|| Box::new(PhpArray::new()));
        let key = Key::from_php_str(name);
        if dv.get(key.as_ref()).is_none() && !create {
            return None;
        }
        Some(dv.entry(key) as *mut Value)
    }

    pub(crate) fn global_var_ptr(&mut self, name: &PhpStr, create: bool) -> Option<*mut Value> {
        if let Some(code) = &self.globals.code
            && let Some(i) = code.cv_slot(name)
        {
            let p = &mut self.globals.slots[i as usize];
            if p.is_none() {
                if !create {
                    return None;
                }
                *p = Some(Value::Null);
            }
            return p.as_mut().map(|v| v as *mut Value);
        }
        let key = Key::from_php_str(name);
        if self.globals.dyn_vars.get(key.as_ref()).is_none() && !create {
            return None;
        }
        Some(self.globals.dyn_vars.entry(key) as *mut Value)
    }

    fn frame_owner(&self) -> usize {
        self.frame().owner
    }

    /// An undefined variable of an attached frame: bind it if the owner has
    /// it now.
    pub(crate) fn attached_bind(&mut self, r: Reg) -> bool {
        let name = self.var_name(r);
        let owner = self.frame_owner();
        if let Some(p) = self.owner_var_ptr(owner, &name, false) {
            let rf = unsafe { (*p).make_ref() };
            let sp = self.slot_ptr(r);
            unsafe { *sp = Some(Value::Ref(rf)) };
            return true;
        }
        false
    }

    pub(crate) fn attached_lookup(&mut self, r: Reg) -> Option<Value> {
        if self.frame().flags & frame_flags::ATTACHED == 0
            || slot_of(r) >= self.code_ref().num_cvs as usize
        {
            return None;
        }
        let name = self.var_name(r);
        let owner = self.frame_owner();
        let p = self.owner_var_ptr(owner, &name, false)?;
        Some(unsafe { (*p).deref() })
    }

    pub(crate) fn attached_bind_or_create(&mut self, r: Reg) {
        if slot_of(r) >= self.code_ref().num_cvs as usize {
            return;
        }
        let name = self.var_name(r);
        let owner = self.frame_owner();
        if let Some(p) = self.owner_var_ptr(owner, &name, true) {
            let rf = unsafe { (*p).make_ref() };
            let sp = self.slot_ptr(r);
            unsafe { *sp = Some(Value::Ref(rf)) };
        }
    }

    /// Writes a variable of an attached frame (creating it in the owner).
    pub(crate) fn put_attached(&mut self, dst: Reg, v: Value) {
        if slot_of(dst) >= self.code_ref().num_cvs as usize {
            let p = self.slot_ptr(dst);
            unsafe { *p = Some(v) };
            return;
        }
        self.attached_bind_or_create(dst);
        let p = self.slot_ptr(dst);
        unsafe {
            match &mut *p {
                Some(Value::Ref(r)) => {
                    let old = r.set(v.into_deref());
                    drop(old);
                }
                slot => *slot = Some(v),
            }
        }
    }

    pub(crate) fn attached_bind_ref(&mut self, dst: Reg, r: &PhpRef) {
        if slot_of(dst) >= self.code_ref().num_cvs as usize {
            return;
        }
        let name = self.var_name(dst);
        let owner = self.frame_owner();
        if let Some(p) = self.owner_var_ptr(owner, &name, true) {
            let old = unsafe { std::mem::replace(&mut *p, Value::Ref(r.clone())) };
            drop(old);
        }
    }

    pub(crate) fn attached_unset(&mut self, var: Reg) {
        if slot_of(var) >= self.code_ref().num_cvs as usize {
            return;
        }
        let name = self.var_name(var);
        let owner = self.frame_owner();
        self.unset_owner_var(owner, &name);
    }

    fn unset_owner_var(&mut self, owner: usize, name: &PhpStr) {
        if owner == usize::MAX {
            if let Some(code) = &self.globals.code
                && let Some(i) = code.cv_slot(name)
            {
                let old = self.globals.slots[i as usize].take();
                drop(old);
                return;
            }
            let old = self
                .globals
                .dyn_vars
                .remove(Key::from_php_str(name).as_ref());
            drop(old);
            return;
        }
        let f = &mut self.ex.frames[owner];
        if let Some(i) = f.code().cv_slot(name) {
            let old = unsafe { (*f.base.add(i as usize)).take() };
            drop(old);
            return;
        }
        if let Some(dv) = &mut f.dyn_vars {
            let old = dv.remove(Key::from_php_str(name).as_ref());
            drop(old);
        }
    }

    /// After an attached frame ends: references only the owner holds become
    /// plain values again.
    pub(crate) fn detach_frame(&mut self, f: &Frame) {
        let n = f.code().num_slots as usize;
        for i in 0..n {
            let old = unsafe { (*f.base.add(i)).take() };
            drop(old);
        }
        let unwrap = |s: &mut Slot| {
            if let Some(Value::Ref(r)) = s
                && r.refcount() == 1
            {
                let v = r.get();
                *s = Some(v);
            }
        };
        if f.owner == usize::MAX {
            for s in self.globals.slots.iter_mut() {
                unwrap(s);
            }
        } else if let Some(of) = self.ex.frames.get(f.owner) {
            let n = of.code().num_slots as usize;
            for i in 0..n {
                unwrap(unsafe { &mut *of.base.add(i) });
            }
        }
    }

    // ------------------------------------------------------------ globals

    pub(crate) fn fetch_global(&mut self, n: &Value, quiet: bool) -> R<Value> {
        let name = self.to_php_str(n)?;
        match self.global_var_ptr(&name, false) {
            Some(p) => Ok(unsafe { (*p).deref() }),
            None => {
                if !quiet {
                    self.warn(&format!(
                        "Undefined global variable ${}",
                        name.to_string_lossy()
                    ))?;
                }
                Ok(Value::Null)
            }
        }
    }

    pub(crate) fn global_place(&mut self, name: &PhpStr) -> *mut Value {
        self.global_var_ptr(name, true).unwrap()
    }

    pub(crate) fn global_action(
        &mut self,
        name: &PhpStr,
        action: PathAction,
        value: Option<Value>,
        res: Reg,
    ) -> R<()> {
        if let PathAction::Unset = action {
            self.unset_owner_var(usize::MAX, name);
            return Ok(());
        }
        // `$GLOBALS['x'] op= v` on a missing global warns first (test
        // Zend/tests/undef_index_to_exception).
        if matches!(action, PathAction::AssignOp(..) | PathAction::IncDec(_))
            && self.global_var_ptr(name, false).is_none()
        {
            self.warn(&format!(
                "Undefined global variable ${}",
                name.to_string_lossy()
            ))?;
        }
        let p = self.global_place(name);
        let r = unsafe { (*p).make_ref() };
        self.cell_action(&r, action, value, res)
    }

    /// `global $x`.
    pub(crate) fn bind_global(&mut self, var: Reg, n: Value) -> R<()> {
        let name = self.to_php_str(&n)?;
        let p = self.global_place(&name);
        let r = unsafe { (*p).make_ref() };
        if var == NONE {
            // global $$name
            let lp = self.var_var_place(&name)?;
            let old = unsafe { std::mem::replace(&mut *lp, Value::Ref(r)) };
            drop(old);
            return Ok(());
        }
        self.bind_ref_var(var, Value::Ref(r))
    }

    /// `$GLOBALS` as an array (a read-only copy since PHP 8.1). Like any
    /// array copy it keeps references that something else shares
    /// (`$a[0] =& $a` serializes as a reference to the global).
    pub fn globals_array(&mut self) -> PhpArray {
        fn keep(v: &Value) -> Value {
            match v {
                Value::Ref(r) if r.refcount() > 1 => v.clone(),
                _ => v.deref(),
            }
        }
        // Order as PHP's symbol table has it: the request's variables
        // ($_ENV and $_REQUEST only once filled: they are created on
        // first use), the script's variables, then dynamic ones.
        let is_auto = |k: &goro_runtime::KeyRef| {
            matches!(k, goro_runtime::KeyRef::Str(s) if matches!(s.as_bytes(),
                b"_GET" | b"_POST" | b"_COOKIE" | b"_FILES" | b"argv" | b"argc"
                | b"_SERVER" | b"_ENV" | b"_REQUEST"))
        };
        let mut a = PhpArray::new();
        for (k, v) in self.globals.dyn_vars.iter() {
            if is_auto(&k) {
                if matches!(&k, goro_runtime::KeyRef::Str(s) if matches!(s.as_bytes(), b"_ENV" | b"_REQUEST"))
                    && matches!(v.deref(), Value::Array(x) if x.is_empty())
                {
                    continue;
                }
                a.insert(k.to_owned(), keep(v));
            }
        }
        if let Some(code) = self.globals.code.clone() {
            for (i, n) in code.cv_names.iter().enumerate() {
                if let Some(v) = &self.globals.slots[i] {
                    a.insert(Key::from_php_str(n), keep(v));
                }
            }
        }
        for (k, v) in self.globals.dyn_vars.iter() {
            if !is_auto(&k) {
                a.insert(k.to_owned(), keep(v));
            }
        }
        a
    }

    // ------------------------------------------------------------ variable variables

    /// Pointer to the variable named `name` in the current scope (created).
    pub(crate) fn var_var_place(&mut self, name: &PhpStr) -> R<*mut Value> {
        if name.as_bytes() == b"this" {
            return self.throw("Error", "Cannot re-assign $this");
        }
        let f = self.frame();
        if f.flags & frame_flags::GLOBAL != 0 {
            return Ok(self.global_place(name));
        }
        if let Some(i) = f.code().cv_slot(name) {
            let r = i;
            let p = self.slot_ptr(r);
            unsafe {
                if (*p).is_none() {
                    if f.flags & frame_flags::ATTACHED != 0 {
                        self.attached_bind_or_create(r);
                    }
                    if (*p).is_none() {
                        *p = Some(Value::Null);
                    }
                }
                return Ok((*p).as_mut().unwrap() as *mut Value);
            }
        }
        if f.flags & frame_flags::ATTACHED != 0 {
            let owner = f.owner;
            return Ok(self.owner_var_ptr(owner, name, true).unwrap());
        }
        let f = self.frame_mut();
        let dv = f.dyn_vars.get_or_insert_with(|| Box::new(PhpArray::new()));
        Ok(dv.entry(Key::from_php_str(name)) as *mut Value)
    }

    fn var_var_lookup(&mut self, name: &PhpStr) -> Option<Value> {
        let f = self.frame();
        if f.flags & frame_flags::GLOBAL != 0 {
            return self
                .global_var_ptr(name, false)
                .map(|p| unsafe { (*p).clone() });
        }
        if let Some(i) = f.code().cv_slot(name) {
            let p = self.slot_ptr(i);
            if let Some(v) = unsafe { (*p).clone() } {
                return Some(v);
            }
            if f.flags & frame_flags::ATTACHED != 0 {
                let owner = f.owner;
                return self
                    .owner_var_ptr(owner, name, false)
                    .map(|p| unsafe { (*p).clone() });
            }
            return None;
        }
        if f.flags & frame_flags::ATTACHED != 0 {
            let owner = f.owner;
            return self
                .owner_var_ptr(owner, name, false)
                .map(|p| unsafe { (*p).clone() });
        }
        f.dyn_vars
            .as_ref()
            .and_then(|d| d.get(Key::from_php_str(name).as_ref()).cloned())
    }

    pub(crate) fn fetch_var_var(&mut self, n: &Value, quiet: bool) -> R<Value> {
        let name = self.to_php_str(n)?;
        if name.as_bytes() == b"this" {
            return match self.this_obj() {
                Some(o) => Ok(Value::Object(o)),
                None => {
                    if !quiet {
                        self.warn("Undefined variable $this")?;
                    }
                    Ok(Value::Null)
                }
            };
        }
        if crate::compiler::is_superglobal_name(&name) {
            return self.fetch_global(&Value::Str(name), quiet);
        }
        match self.var_var_lookup(&name) {
            Some(v) => Ok(v.deref()),
            None => {
                if !quiet {
                    self.warn(&format!("Undefined variable ${}", name.to_string_lossy()))?;
                }
                Ok(Value::Null)
            }
        }
    }

    pub(crate) fn assign_var_var(&mut self, n: &Value, v: Value) -> R<()> {
        let name = self.to_php_str(n)?;
        let p = self.var_var_place(&name)?;
        let old = unsafe { (*p).assign(v) };
        drop(old);
        Ok(())
    }

    pub(crate) fn var_var_action(
        &mut self,
        name: &PhpStr,
        action: PathAction,
        value: Option<Value>,
        res: Reg,
    ) -> R<()> {
        if let PathAction::Unset = action {
            return self.unset_var_var(&Value::Str(name.clone()));
        }
        if crate::compiler::is_superglobal_name(name) {
            return self.global_action(name, action, value, res);
        }
        let p = self.var_var_place(name)?;
        // A slot that is a variable: work through a reference to it.
        if let PathAction::BindRef(_) = action {
            let r = match value.clone().unwrap() {
                Value::Ref(r) => r,
                other => PhpRef::new(other),
            };
            let old = unsafe { std::mem::replace(&mut *p, Value::Ref(r.clone())) };
            drop(old);
            if res != NONE {
                self.put(res, Value::Ref(r));
            }
            return Ok(());
        }
        let r = unsafe { (*p).make_ref() };
        self.cell_action(&r, action, value, res)
    }

    pub(crate) fn unset_var_var(&mut self, n: &Value) -> R<()> {
        let name = self.to_php_str(n)?;
        let f = self.frame();
        if f.flags & frame_flags::GLOBAL != 0
            || (crate::compiler::is_superglobal_name(&name) && name.as_bytes() != b"GLOBALS")
        {
            self.unset_owner_var(usize::MAX, &name);
            return Ok(());
        }
        if let Some(i) = f.code().cv_slot(&name) {
            self.unset_var(i);
            return Ok(());
        }
        if f.flags & frame_flags::ATTACHED != 0 {
            let owner = f.owner;
            self.unset_owner_var(owner, &name);
            return Ok(());
        }
        let f = self.frame_mut();
        if let Some(dv) = &mut f.dyn_vars {
            let old = dv.remove(Key::from_php_str(&name).as_ref());
            drop(old);
        }
        Ok(())
    }

    /// The variables of the current scope (get_defined_vars).
    pub fn defined_vars(&mut self) -> PhpArray {
        let f = self.frame();
        if f.flags & frame_flags::GLOBAL != 0 {
            return self.globals_array();
        }
        let owner = if f.flags & frame_flags::ATTACHED != 0 {
            Some(f.owner)
        } else {
            None
        };
        if let Some(o) = owner {
            if o == usize::MAX {
                return self.globals_array();
            }
            let of = &self.ex.frames[o];
            let mut a = PhpArray::new();
            collect_frame_vars(of, &mut a);
            return a;
        }
        let mut a = PhpArray::new();
        collect_frame_vars(f, &mut a);
        a
    }

    // ------------------------------------------------------------ constants

    pub(crate) fn op_fetch_const(
        &mut self,
        dst: Reg,
        name: u32,
        cache: u32,
        fallback: bool,
    ) -> R<()> {
        let c = self.code_ref().caches[cache as usize].get();
        if c.key == 1 {
            let v = self.const_cache[c.a].clone();
            self.put(dst, v);
            return Ok(());
        }
        let n = self.code_ref().consts[name as usize].clone();
        let Value::Str(n) = n else { unreachable!() };
        let found = match self.lookup_constant(&n)? {
            Some(v) => Some((v, n.clone())),
            None if fallback => {
                let g = self.code_ref().consts[name as usize + 2].clone();
                let Value::Str(g) = g else { unreachable!() };
                self.lookup_constant(&g)?.map(|v| (v, g))
            }
            None => None,
        };
        match found {
            // Deprecated internal constants warn on every fetch, so they
            // are not cached.
            Some((v, n)) if crate::builtins::misc::const_deprecation(&n).is_some() => {
                self.const_deprecated(&n)?;
                self.put(dst, v);
                Ok(())
            }
            Some((v, n)) if !self.const_attrs.is_empty() && self.user_const_deprecated(&n)? => {
                self.put(dst, v);
                Ok(())
            }
            Some((v, _)) => {
                let idx = self.const_cache.len();
                self.const_cache.push(v.clone());
                self.code_ref().caches[cache as usize].set(Cache {
                    key: 1,
                    a: idx,
                    b: 0,
                    ..Cache::default()
                });
                self.put(dst, v);
                Ok(())
            }
            None => {
                let shown = if fallback {
                    let g = self.code_ref().consts[name as usize].clone();
                    g.as_str().map(|s| s.to_string_lossy()).unwrap_or_default()
                } else {
                    n.to_string_lossy()
                };
                // In a constant expression the error shows its frame (test
                // Zend/tests/bug47572).
                let fl = self.code_ref().flags;
                let ce = fl & crate::bytecode::fflags::CONST_EXPR != 0
                    && fl & crate::bytecode::fflags::BARE_CONST == 0;
                let on = ce || self.trace_const_expr;
                let saved = std::mem::replace(&mut self.trace_const_expr, on);
                let r = self.throw("Error", &format!("Undefined constant \"{}\"", shown));
                self.trace_const_expr = saved;
                r
            }
        }
    }

    /// A constant's value by name (case-sensitive, with namespace part
    /// case-insensitive).
    pub fn lookup_constant(&mut self, name: &PhpStr) -> R<Option<Value>> {
        let name = PhpStr::new(name.strip_prefix(b"\\").unwrap_or(name));
        if let Some(v) = self.constants.get(&name) {
            return Ok(Some(v.clone()));
        }
        if name.as_bytes() == b"__COMPILER_HALT_OFFSET__" {
            let f = self.cur_file();
            if let Some(v) = self.halt_offsets.get(&f) {
                return Ok(Some(Value::Int(*v)));
            }
        }
        // Namespace parts are case-insensitive.
        if let Some(pos) = name.iter().rposition(|&c| c == b'\\') {
            let mut k = name[..pos].to_ascii_lowercase();
            k.extend_from_slice(&name[pos..]);
            if let Some(v) = self.constants.get(&PhpStr::from(k)) {
                return Ok(Some(v.clone()));
            }
        }
        // true/false/null in any case.
        let l = name.to_ascii_lowercase();
        match &l[..] {
            b"true" => return Ok(Some(Value::Bool(true))),
            b"false" => return Ok(Some(Value::Bool(false))),
            b"null" => return Ok(Some(Value::Null)),
            _ => {}
        }
        Ok(None)
    }

    /// Raises the deprecation of a `#[\\Deprecated]` user constant; whether
    /// it has one (such constants are not cached).
    pub(crate) fn user_const_deprecated(&mut self, name: &[u8]) -> R<bool> {
        let key = normalize_const_name(name);
        let Some(attrs) = self.const_attrs.get(&key).cloned() else {
            return Ok(false);
        };
        if self.deprecating.contains(&(attrs.as_ptr() as usize)) {
            // Its own message being evaluated: once is enough.
            return Ok(attrs.iter().any(|a| {
                a.name
                    .strip_prefix(b"\\")
                    .unwrap_or(&a.name)
                    .eq_ignore_ascii_case(b"Deprecated")
            }));
        }
        let Some((since, msg)) = self.deprecation_detail(&attrs, std::ptr::null(), false)? else {
            return Ok(false);
        };
        let text = deprecation_text(
            &format!(
                "Constant {}",
                String::from_utf8_lossy(name.strip_prefix(b"\\").unwrap_or(name))
            ),
            since,
            msg,
        );
        self.raise(super::errors::E_USER_DEPRECATED, &text)?;
        Ok(true)
    }

    /// Raises the deprecation of an internal constant, if it has one.
    pub(crate) fn const_deprecated(&mut self, name: &[u8]) -> R<()> {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if let Some(why) = crate::builtins::misc::const_deprecation(name) {
            self.deprecated(&format!(
                "Constant {} is deprecated since {}",
                String::from_utf8_lossy(name),
                why
            ))?;
        }
        Ok(())
    }

    /// A `const` statement: the constant, with its attributes when it is
    /// new (a redefinition keeps the first one's).
    pub(crate) fn declare_const_stmt(&mut self, n: Value, v: Value) -> R<()> {
        let name = self.to_php_str(&n)?;
        let key = normalize_const_name(&name);
        let fresh = !self.constants.contains_key(&key);
        self.declare_constant(n, v)?;
        if fresh && !self.code_ref().const_attrs.is_empty() {
            // The statement's own attributes: by name and line, since a
            // redefinition with other attributes keeps the first's (test
            // Zend/tests/attributes/constants/constant_redefined_addition).
            let line = self.cur_line();
            let attrs = self
                .code_ref()
                .const_attrs
                .iter()
                .find(|(k, l, _)| *l == line && normalize_const_name(k) == key)
                .map(|(_, _, a)| a.clone());
            if let Some(a) = attrs {
                self.const_attrs.insert(key, a);
            }
        }
        Ok(())
    }

    pub(crate) fn declare_constant(&mut self, n: Value, v: Value) -> R<()> {
        let name = self.to_php_str(&n)?;
        let key = normalize_const_name(&name);
        if self.constants.contains_key(&key) {
            self.warn(&format!(
                "Constant {} already defined, this will be an error in PHP 9",
                name.to_string_lossy()
            ))?;
            return Ok(());
        }
        self.constants.insert(key.clone(), v);
        crate::builtins::reflection::note_constant(self, &key);
        self.user_constants.push(key);
        Ok(())
    }
}

/// Namespace part lower-cased (`NS\Foo` and `ns\Foo` are the same
/// constant).
pub fn normalize_const_name(name: &[u8]) -> PhpStr {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    match name.iter().rposition(|&c| c == b'\\') {
        Some(pos) => {
            let mut k = name[..pos].to_ascii_lowercase();
            k.extend_from_slice(&name[pos..]);
            PhpStr::from(k)
        }
        None => PhpStr::new(name),
    }
}

fn collect_frame_vars(f: &Frame, a: &mut PhpArray) {
    let code = f.code();
    for (i, n) in code.cv_names.iter().enumerate() {
        if let Some(v) = unsafe { (*f.base.add(i)).as_ref() } {
            a.insert(Key::from_php_str(n), v.deref());
        }
    }
    if let Some(dv) = &f.dyn_vars {
        for (k, v) in dv.iter() {
            a.insert(k.to_owned(), v.deref());
        }
    }
}

/// "What is deprecated[ since X][, message]".
pub(crate) fn deprecation_text(what: &str, since: Option<String>, msg: Option<String>) -> String {
    let mut s = format!("{} is deprecated", what);
    if let Some(v) = since {
        s.push_str(" since ");
        s.push_str(&v);
    }
    if let Some(m) = msg {
        s.push_str(", ");
        s.push_str(&m);
    }
    s
}
