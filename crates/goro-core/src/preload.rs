//! opcache.preload (ZendAccelerator.c's accel_preload()): a script run
//! before the request whose functions and classes stay declared for it.
//!
//! The preload script runs in the request's VM; afterwards the state of
//! that "preload request" is shut down (globals, constants, handlers,
//! included files, static variables and static properties) while the
//! functions and classes it declared are kept, as php-src keeps them in
//! shared memory. Classes of files compiled with opcache_compile_file()
//! (and anonymous classes never instantiated) are linked by
//! preload_link(): repeatedly, those whose parent, interfaces and traits
//! are available are declared; the others are reported as "Can't preload
//! unlinked class". Including a preloaded file later runs its code without
//! its declarations (preload_remove_declares()).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::object::ClassEntry;
use crate::opcode::{Op, OpArray, OpCode, OperandType};
use crate::value::Value;
use crate::vm::Vm;

/// A class compiled during preloading but not declared by execution.
struct Unlinked {
    class: ClassEntry,
    anon: bool,
    file: String,
}

#[derive(Default)]
struct State {
    /// The files compiled during preloading, in order (preload_scripts).
    scripts: Vec<String>,
    unlinked: Vec<Unlinked>,
    /// The preloaded functions and classes, by file (lowercase names).
    file_functions: HashMap<String, HashSet<Vec<u8>>>,
    file_classes: HashMap<String, HashSet<Vec<u8>>>,
    /// preload_statistics: function and class names, in declaration order.
    functions: Vec<Vec<u8>>,
    classes: Vec<Vec<u8>>,
    /// The static properties of the compiled classes, before declaration
    /// (default_static_members_table), by lowercase name.
    statics: HashMap<Vec<u8>, indexmap::IndexMap<Vec<u8>, Value>>,
    done: bool,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// ZCG(preloading) / ZEND_COMPILE_PRELOAD: the preload script is running.
pub fn active() -> bool {
    ACTIVE.with(|a| a.get())
}

/// Whether preloading took place (ZCSG(preload_script)).
pub fn done() -> bool {
    STATE.with(|s| s.borrow().done)
}

/// preload_statistics of opcache_get_status(): (functions, classes,
/// scripts).
pub fn statistics() -> (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<String>) {
    STATE.with(|s| {
        let s = s.borrow();
        (s.functions.clone(), s.classes.clone(), s.scripts.clone())
    })
}

fn note_script(path: &str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.scripts.iter().any(|p| p == path) {
            s.scripts.push(path.to_string());
        }
    });
}

fn is_anon(class: &ClassEntry) -> bool {
    class.name.windows(10).any(|w| w == b"@anonymous")
}

/// The anonymous classes declared in the bodies of `op` (and of the
/// functions and methods nested in it).
fn collect_anon(op: &OpArray, out: &mut Vec<ClassEntry>) {
    for c in &op.local_classes {
        if is_anon(c) {
            out.push(c.clone());
        }
        collect_class_anon(c, out);
    }
    for f in &op.child_functions {
        collect_anon(f, out);
    }
}

fn collect_class_anon(c: &ClassEntry, out: &mut Vec<ClassEntry>) {
    for m in c.methods.values() {
        collect_anon(&m.op_array, out);
    }
}

/// Remembers the static property defaults of the classes of a file.
fn note_statics(op: &OpArray, classes: &[ClassEntry]) {
    fn walk(op: &OpArray, out: &mut Vec<(Vec<u8>, indexmap::IndexMap<Vec<u8>, Value>)>) {
        for c in &op.local_classes {
            class(c, out);
        }
        for f in &op.child_functions {
            walk(f, out);
        }
    }
    fn class(c: &ClassEntry, out: &mut Vec<(Vec<u8>, indexmap::IndexMap<Vec<u8>, Value>)>) {
        out.push((c.name.to_ascii_lowercase(), c.static_properties.clone()));
        for m in c.methods.values() {
            walk(&m.op_array, out);
        }
    }
    let mut out = Vec::new();
    for c in classes {
        class(c, &mut out);
    }
    walk(op, &mut out);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for (k, v) in out {
            s.statics.entry(k).or_insert(v);
        }
    });
}

fn queue(path: &str, classes: Vec<ClassEntry>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for class in classes {
            let anon = is_anon(&class);
            s.unlinked.push(Unlinked {
                class,
                anon,
                file: path.to_string(),
            });
        }
    });
}

/// preload_compile_file(): a file compiled (to be executed) while
/// preloading. Its anonymous classes are linked at the end unless they got
/// declared.
pub(crate) fn note_compiled(path: &str, op_array: &OpArray, classes: &[ClassEntry]) {
    note_script(path);
    note_statics(op_array, classes);
    let mut anon = Vec::new();
    for c in classes {
        if is_anon(c) {
            anon.push(c.clone());
        }
        collect_class_anon(c, &mut anon);
    }
    collect_anon(op_array, &mut anon);
    queue(path, anon);
}

/// The start lines of the top-level (ZEND_ACC_TOP_LEVEL) class
/// declarations of a file.
fn top_level_class_lines(stmts: &[goro_parser::ast::Statement], out: &mut HashSet<u32>) {
    use goro_parser::ast::StmtKind;
    for s in stmts {
        match &s.kind {
            StmtKind::ClassDecl { .. } => {
                out.insert(s.span.line);
            }
            StmtKind::NamespaceDecl {
                body: Some(body), ..
            } => top_level_class_lines(body, out),
            _ => {}
        }
    }
}

impl Vm {
    /// opcache_compile_file() while preloading: the file's top-level
    /// functions are declared (they are bound at compile time) and its
    /// top-level and anonymous classes are left for preload_link().
    pub(crate) fn preload_compiled_only(
        &mut self,
        path: &str,
        program: &goro_parser::ast::Program,
        op_array: &OpArray,
        classes: &[ClassEntry],
    ) -> Result<(), crate::vm::VmError> {
        note_script(path);
        note_statics(op_array, classes);
        for op in &op_array.ops {
            if op.opcode != OpCode::DeclareFunction {
                continue;
            }
            let OperandType::Const(m) = op.result else {
                continue;
            };
            if op_array.literals.get(m as usize).map(|v| v.to_long()) != Some(64) {
                continue;
            }
            let name = match &op.op1 {
                OperandType::Const(c) => op_array.literals[*c as usize].to_php_string(),
                _ => continue,
            };
            let idx = match &op.op2 {
                OperandType::Const(c) => op_array.literals[*c as usize].to_long() as usize,
                _ => continue,
            };
            let Some(f) = op_array.child_functions.get(idx) else {
                continue;
            };
            if let Some(msg) = self.function_redeclaration_error(name.as_bytes()) {
                let saved = std::mem::replace(&mut self.current_file, path.into());
                let e = self.emit_fatal_typed_at(crate::errdisplay::E_COMPILE_ERROR, &msg, op.line);
                self.current_file = saved;
                return Err(e);
            }
            self.register_user_function(name.as_bytes(), f.clone());
        }
        let mut lines = HashSet::new();
        top_level_class_lines(&program.statements, &mut lines);
        let mut queued = Vec::new();
        for c in classes {
            if is_anon(c) || lines.contains(&c.start_line) {
                queued.push(c.clone());
            }
            collect_class_anon(c, &mut queued);
        }
        collect_anon(op_array, &mut queued);
        queue(path, queued);
        Ok(())
    }

    fn preload_class_known(&self, name: &[u8]) -> bool {
        let lower = name.to_ascii_lowercase();
        self.lookup_class(&lower).is_some()
    }

    /// preload_resolve_deps()
    fn preload_resolve_deps(&self, ce: &ClassEntry) -> Result<(), (&'static str, Vec<u8>)> {
        if let Some(p) = &ce.parent {
            if !self.preload_class_known(p) {
                return Err(("Unknown parent ", p.clone()));
            }
        }
        for i in &ce.interfaces {
            if !self.preload_class_known(i) {
                return Err(("Unknown interface ", i.clone()));
            }
        }
        for t in &ce.traits {
            if !self.preload_class_known(t) {
                return Err(("Unknown trait ", t.clone()));
            }
        }
        Ok(())
    }

    /// Declares a class as DECLARE_CLASS would; on a fatal error the
    /// output it produced is discarded and the error message returned.
    fn preload_declare_class(&mut self, u: &Unlinked) -> Result<(), (String, u32)> {
        let saved_file =
            std::mem::replace(&mut self.current_file, crate::rcstr::RcStr::from(&u.file));
        let out_len = self.active_output_buffer().len();
        let saved_fatal = self.fatal_emitted;
        let saved_recorded = self.recorded_errors.replace(Vec::new());
        let r = self.declare_class_entry(u.class.clone(), &u.file);
        let recorded = std::mem::replace(&mut self.recorded_errors, saved_recorded);
        let result = match r {
            Ok(_) if self.current_exception.is_none() => {
                // Inheritance successful: the warnings it raised are shown.
                for (ty, msg, line) in recorded.unwrap_or_default() {
                    self.raise_error(ty, &msg, line);
                }
                Ok(())
            }
            r => {
                let (msg, line) = match (&r, self.current_exception.take()) {
                    (_, Some(Value::Object(o))) => {
                        let ob = o.borrow();
                        (
                            ob.get_property(b"message")
                                .to_php_string()
                                .to_string_lossy(),
                            ob.get_property(b"line").to_long() as u32,
                        )
                    }
                    (Err(e), _) => (e.message.clone(), e.line),
                    _ => (String::new(), u.class.start_line),
                };
                let buf = self.active_output_buffer();
                if buf.len() > out_len {
                    buf.truncate(out_len);
                }
                self.fatal_emitted = saved_fatal;
                let line = if line == 0 { u.class.start_line } else { line };
                Err((msg, line))
            }
        };
        self.current_file = saved_file;
        result
    }

    /// preload_link()
    fn preload_link(&mut self) {
        let unlinked: Vec<Unlinked> = STATE.with(|s| std::mem::take(&mut s.borrow_mut().unlinked));
        let mut linked = vec![false; unlinked.len()];
        let mut errors: HashMap<usize, (String, u32)> = HashMap::new();
        loop {
            let mut changed = false;
            for (i, u) in unlinked.iter().enumerate() {
                if linked[i] {
                    continue;
                }
                if self.preload_class_known(&u.class.name) {
                    if u.anon {
                        // Instantiated while preloading.
                        linked[i] = true;
                    }
                    continue;
                }
                if self.preload_resolve_deps(&u.class).is_err() {
                    continue;
                }
                match self.preload_declare_class(u) {
                    Ok(()) => {
                        self.anon_pending.remove(&u.class.name.to_ascii_lowercase());
                        linked[i] = true;
                        errors.remove(&i);
                        changed = true;
                    }
                    Err(e) => {
                        errors.insert(i, e);
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // Warn for classes that could not be linked.
        for (i, u) in unlinked.iter().enumerate() {
            if linked[i] {
                continue;
            }
            let name = crate::value::display_class_name(&u.class.name);
            let saved_file =
                std::mem::replace(&mut self.current_file, crate::rcstr::RcStr::from(&u.file));
            if !u.anon && self.preload_class_known(&u.class.name) {
                self.raise_error(
                    crate::errdisplay::E_WARNING,
                    &format!("Can't preload already declared class {}", name),
                    u.class.start_line,
                );
            } else if let Err((kind, dep)) = self.preload_resolve_deps(&u.class) {
                self.raise_error(
                    crate::errdisplay::E_WARNING,
                    &format!(
                        "Can't preload unlinked class {}: {}{}",
                        name,
                        kind,
                        crate::string::msg_str(&dep)
                    ),
                    u.class.start_line,
                );
            } else if let Some((msg, line)) = errors.get(&i) {
                self.raise_error(
                    crate::errdisplay::E_WARNING,
                    &format!("Can't preload unlinked class {}: {}", name, msg),
                    *line,
                );
            }
            self.current_file = saved_file;
        }
    }

    /// accel_preload(): runs the preload script and keeps what it
    /// declared. Returns false when preloading failed (the request must
    /// not run).
    #[inline(never)]
    pub fn preload_run(&mut self, path: &str) -> bool {
        let fns_before: HashSet<Vec<u8>> = self.user_functions.keys().cloned().collect();
        let classes_before: HashSet<Vec<u8>> = self.classes.keys().cloned().collect();
        let constants_before = self.constants.clone();
        let saved_file = std::mem::replace(&mut self.current_file, path.into());
        let saved_included = (
            self.included_files.clone(),
            self.included_files_list.clone(),
        );
        self.included_files.clear();
        self.included_files_list.clear();
        ACTIVE.with(|a| a.set(true));

        let ok = self.preload_execute(path);

        // Cleanup executor: shutdown functions, then the request state.
        if ok {
            loop {
                let fns = std::mem::take(&mut self.shutdown_functions);
                if fns.is_empty() || self.exit_requested {
                    break;
                }
                for (callback, args) in fns {
                    let _ = self.call_callback(&callback, &args);
                    if self.current_exception.is_some() {
                        break;
                    }
                }
            }
        }
        self.shutdown_functions.clear();
        self.error_handler = None;
        self.error_handler_stack.clear();
        self.exception_handler = None;
        self.exception_handler_stack.clear();
        self.autoload_functions.clear();
        *self.globals.borrow_mut() = crate::array::PhpArray::new();
        self.bump_globals_gen();
        self.global_ref_names.clear();
        self.static_vars.clear();
        self.static_gen = crate::vm::fresh_static_gen();
        self.enum_case_cache.clear();
        self.constants = constants_before;
        self.included_files = saved_included.0;
        self.included_files_list = saved_included.1;
        self.current_exception = None;

        if ok {
            self.preload_link();
        }
        ACTIVE.with(|a| a.set(false));
        self.current_file = saved_file;
        if !ok {
            return false;
        }

        // The preloaded functions and classes, by file.
        let mut functions = Vec::new();
        let mut file_functions: HashMap<String, HashSet<Vec<u8>>> = HashMap::new();
        for (k, f) in self.user_functions.iter() {
            if !fns_before.contains(k) {
                functions.push(f.name.clone());
                let file = String::from_utf8_lossy(&f.filename).into_owned();
                file_functions.entry(file).or_default().insert(k.clone());
            }
        }
        let mut classes = Vec::new();
        let mut file_classes: HashMap<String, HashSet<Vec<u8>>> = HashMap::new();
        let mut reset: Vec<Vec<u8>> = Vec::new();
        for (k, c) in self.classes.iter() {
            if !classes_before.contains(k) {
                classes.push(if c.name.eq_ignore_ascii_case(k) {
                    c.name.clone()
                } else {
                    k.clone()
                });
                if let Some(f) = &c.filename {
                    file_classes.entry(f.clone()).or_default().insert(k.clone());
                }
                reset.push(k.clone());
            }
        }
        // Static properties start from their defaults in the request.
        let statics = STATE.with(|s| std::mem::take(&mut s.borrow_mut().statics));
        for k in reset {
            let Some(c) = self.classes.get_mut(&k) else {
                continue;
            };
            let traits: Vec<Vec<u8>> = c.traits.iter().map(|t| t.to_ascii_lowercase()).collect();
            for (name, v) in c.static_properties.iter_mut() {
                let default = statics.get(&k).and_then(|m| m.get(name)).or_else(|| {
                    traits
                        .iter()
                        .find_map(|t| statics.get(t.strip_prefix(b"\\").unwrap_or(t))?.get(name))
                });
                if let Some(d) = default {
                    *v = d.clone();
                }
            }
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.functions = functions;
            s.classes = classes;
            s.file_functions = file_functions;
            s.file_classes = file_classes;
            s.done = true;
        });
        true
    }

    /// Compiles and executes the preload script as the main script of the
    /// preload request.
    fn preload_execute(&mut self, path: &str) -> bool {
        let source = match std::fs::read(path) {
            Ok(s) => s,
            Err(e) => {
                let s = e.to_string();
                let reason = match s.find(" (os error") {
                    Some(i) => s[..i].to_string(),
                    None => s,
                };
                // php_stream_open_for_zend_ex() failing for the main script
                self.raise_error(
                    crate::errdisplay::E_WARNING,
                    &format!("require({}): Failed to open stream: {}", path, reason),
                    0,
                );
                return false;
            }
        };
        let script = goro_parser::multibyte::lex_script(&source, true);
        let mut parser = goro_parser::Parser::new(script.tokens).with_source(&script.source);
        let parsed = parser.parse();
        let lex_diags = goro_parser::order_lexer_diagnostics(
            &goro_parser::take_lexer_deprecations(),
            &goro_parser::take_lexer_warnings(),
        );
        let program = match parsed {
            Ok(p) => p,
            Err(e) => {
                let msg = if crate::vm::is_php_parse_message(&e.message) {
                    e.message.clone()
                } else {
                    "syntax error".to_string()
                };
                self.display_error("Parse error", &msg, path, e.span.line);
                return false;
            }
        };
        let mut compiler = crate::compiler::Compiler::new();
        compiler.source_file = path.as_bytes().to_vec();
        let (op_array, classes, warnings) = match compiler.compile(&program) {
            Ok(r) => r,
            Err(e) => {
                for (kind, msg, wline) in crate::compiler::take_pending_compile_warnings() {
                    self.display_error(kind, &msg, path, wline);
                }
                self.display_error("Fatal error", &e.message, path, e.line);
                return false;
            }
        };
        self.replay_cached_diags(&crate::script_cache::merged_compile_diags(
            &lex_diags, &warnings,
        ));
        note_compiled(path, &op_array, &classes);
        let saved_pending = std::mem::replace(&mut self.pending_classes, std::rc::Rc::new(classes));
        let r = self.execute_chunk(&op_array);
        self.pending_classes = saved_pending;
        match r {
            Ok(_) if self.current_exception.is_none() && !self.fatal_emitted => true,
            Ok(_) => {
                let e = crate::vm::VmError {
                    message: String::new(),
                    line: 0,
                };
                self.report_uncaught(&e);
                false
            }
            Err(e) => {
                if self.current_exception.is_some() || !self.fatal_emitted {
                    self.report_uncaught(&e);
                }
                false
            }
        }
    }

    /// preload_remove_declares(): including a preloaded file runs its code
    /// without the declarations preloading already made.
    pub(crate) fn preload_strip_declares(&self, path: &str, op_array: &mut OpArray) {
        let (fns, classes) = STATE.with(|s| {
            let s = s.borrow();
            (
                s.file_functions.get(path).cloned(),
                s.file_classes.get(path).cloned(),
            )
        });
        if fns.is_none() && classes.is_none() {
            return;
        }
        let fns = fns.unwrap_or_default();
        let classes = classes.unwrap_or_default();
        for i in 0..op_array.ops.len() {
            let op = &op_array.ops[i];
            let set = match op.opcode {
                OpCode::DeclareFunction if matches!(op.result, OperandType::Const(_)) => &fns,
                OpCode::DeclareClass => &classes,
                _ => continue,
            };
            let OperandType::Const(c) = op.op1 else {
                continue;
            };
            let name = op_array.literals[c as usize].to_php_string();
            let lower = name.as_bytes().to_ascii_lowercase();
            let lower = lower.strip_prefix(b"\\").unwrap_or(&lower).to_vec();
            if set.contains(&lower) {
                op_array.ops[i] = Op {
                    opcode: OpCode::Nop,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: op_array.ops[i].line,
                };
            }
        }
    }
}
