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

impl Vm {
    // ------------------------------------------------------------ compiling

    /// Parses and compiles source code.
    pub fn compile_source(
        &mut self,
        src: &[u8],
        file: &[u8],
        eval: bool,
    ) -> Result<Unit, crate::compiler::CompileError> {
        let arena = goro_syntax::Arena::new();
        // The AST borrows the source; keep a copy alive for the call.
        let opts = goro_syntax::ParseOptions {
            short_open_tag: self.ini_bool(b"short_open_tag", true),
            in_scripting: eval,
            stack_limit: PARSER_STACK,
        };
        let prog = goro_syntax::parse_with(&arena, src, opts).map_err(|e| {
            crate::compiler::CompileError {
                message: format!(
                    "{}{}",
                    if e.kind == goro_syntax::ErrorKind::Parse {
                        "\u{1}"
                    } else {
                        ""
                    },
                    e.message
                ),
                line: e.line,
            }
        })?;
        let consts = &self.constants;
        let kc = |n: &[u8]| -> Option<Value> {
            // Only engine constants that can't be redefined.
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
        let unit = crate::compiler::compile_program(&prog, src, file, kc_opt, None)?;
        if let Some(off) = prog.halt_compiler_offset {
            self.halt_offsets.insert(PhpStr::new(file), off as i64);
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
                self.declare_class(&decl)?;
            }
        }
        Ok(())
    }

    /// Reports a compile error: parse errors as `ParseError` exceptions in
    /// eval/include, fatal errors otherwise.
    pub(crate) fn compile_error(
        &mut self,
        e: crate::compiler::CompileError,
        file: &[u8],
        throw: bool,
    ) -> Unwind {
        let (is_parse, msg) = match e.message.strip_prefix('\u{1}') {
            Some(m) => (true, m.to_string()),
            None => (false, e.message),
        };
        if is_parse && throw {
            match self.create_exception("ParseError", &msg, 0) {
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
            if let KeyRef::Str(name) = k {
                if let Some(i) = main.cv_slot(name) {
                    self.globals.slots[i as usize] = Some(v.clone());
                    continue;
                }
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
        self.run().map(|_| ())
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
        let resolved = self.resolve_include(&path);
        let Some(resolved) = resolved else {
            if path.is_empty() {
                self.raise(super::errors::E_DEPRECATED & 0, "")?;
                let is_req = matches!(kind, IncludeKind::Require | IncludeKind::RequireOnce);
                if is_req {
                    return self.value_error(&format!("Path cannot be empty"));
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
            Ok(s) => s,
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

    /// Compiles and runs code in the current scope (include, eval).
    fn run_attached(&mut self, src: &[u8], file: &[u8], eval: bool, what: &str) -> R<Value> {
        let unit = match self.compile_source(src, file, eval) {
            Ok(u) => u,
            Err(e) => return Err(self.compile_error(e, file, true)),
        };
        let main = unit.main.clone();
        self.units.push(main.clone());
        self.load_unit(&unit)?;
        let kind_hint = match what {
            "include" => 1,
            "include_once" => 2,
            "require" => 3,
            "require_once" => 4,
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

    /// `$GLOBALS` as an array (a read-only copy since PHP 8.1).
    pub fn globals_array(&mut self) -> PhpArray {
        let mut a = PhpArray::new();
        if let Some(code) = self.globals.code.clone() {
            for (i, n) in code.cv_names.iter().enumerate() {
                if let Some(v) = &self.globals.slots[i] {
                    a.insert(Key::from_php_str(n), v.deref());
                }
            }
        }
        for (k, v) in self.globals.dyn_vars.iter() {
            a.insert(k.to_owned(), v.deref());
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
        if f.flags & frame_flags::GLOBAL != 0 {
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
            Some(v) => Some(v),
            None if fallback => {
                let g = self.code_ref().consts[name as usize + 2].clone();
                let Value::Str(g) = g else { unreachable!() };
                self.lookup_constant(&g)?
            }
            None => None,
        };
        match found {
            Some(v) => {
                let idx = self.const_cache.len();
                self.const_cache.push(v.clone());
                self.code_ref().caches[cache as usize].set(Cache {
                    key: 1,
                    a: idx,
                    b: 0,
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
                self.throw("Error", &format!("Undefined constant \"{}\"", shown))
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

    pub(crate) fn declare_constant(&mut self, n: Value, v: Value) -> R<()> {
        let name = self.to_php_str(&n)?;
        let key = normalize_const_name(&name);
        if self.constants.contains_key(&key) {
            self.warn(&format!(
                "Constant {} already defined",
                name.to_string_lossy()
            ))?;
            return Ok(());
        }
        self.constants.insert(key.clone(), v);
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
