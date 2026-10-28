//! AST → bytecode.
//!
//! One [`Compiler`] compiles a file (or `eval()` string). Functions,
//! closures, methods and constant expressions are compiled into their own
//! [`FuncCode`]; the file's main code is one too. Names are resolved against
//! the namespace and `use` imports here (PHP manual: Namespaces, "Name
//! resolution rules"); unqualified function and constant names inside a
//! namespace keep a global fallback resolved at run time.

mod decl;
mod expr;
mod scan;

use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{PhpStr, Value};
use goro_syntax::ast::*;

use crate::bytecode::*;
use crate::class::ClassDecl;

pub use decl::parse_native_sig;

/// Whether a variable name is a superglobal.
pub fn is_superglobal_name(n: &[u8]) -> bool {
    scan::is_superglobal(n)
}

/// A fatal compile-time error ("Fatal error: ... on line N").
#[derive(Clone, Debug)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
}

pub type CResult<T> = Result<T, CompileError>;

/// A deprecation or warning raised while compiling.
#[derive(Clone, Debug)]
pub struct CompileNotice {
    pub level: goro_runtime::Level,
    pub message: String,
    pub line: u32,
}

/// The result of compiling a file.
pub struct Unit {
    pub main: Rc<FuncCode>,
    /// Indices into `main.children` of functions declared unconditionally at
    /// top level (declared before the file runs).
    pub hoisted_funcs: Vec<u32>,
    /// Indices into `main.classes` of top-level classes (declared early
    /// when their dependencies are known).
    pub toplevel_classes: Vec<u32>,
    pub notices: Vec<CompileNotice>,
}

#[derive(Clone)]
pub(crate) struct ClassCtx {
    pub name: PhpStr,
    pub parent: Option<PhpStr>,
    pub is_trait: bool,
    pub is_interface: bool,
    pub is_enum: bool,
}

pub(crate) struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    is_switch: bool,
    /// Iterator (foreach) or subject (switch) slot to free when leaving.
    free: Option<(Reg, bool)>,
    finally_depth: usize,
    /// `regions.len()` when the loop was entered.
    region_len: usize,
    /// First instruction of the loop (where its temporary is live).
    start: u32,
}

/// A block a `goto` may leave or enter: loops (with the temporary freed
/// when leaving), and the parts of a try statement (by try range index).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Region {
    Loop(u32, Option<(Reg, bool)>),
    Try(u32),
    Catch(u32),
    Finally(u32),
}

pub(crate) struct FinCtx {
    ret: Reg,
    fastcalls: Vec<usize>,
    /// The try range this finally belongs to.
    range: u32,
}

/// State of the function being compiled.
pub(crate) struct FState<'a> {
    pub ops: Vec<Op>,
    pub lines: Vec<u32>,
    pub consts: Vec<Value>,
    const_ints: HashMap<i64, u32>,
    const_strs: HashMap<Vec<u8>, u32>,
    pub cvs: Vec<&'a [u8]>,
    pub num_cvs: u32,
    next_slot: u32,
    free_tmps: Vec<u32>,
    max_slot: u32,
    pub paths: Vec<WritePath>,
    pub lists: Vec<Vec<Reg>>,
    pub tables: Vec<SwitchTable>,
    pub children: Vec<Rc<FuncCode>>,
    pub classes: Vec<Rc<ClassDecl>>,
    pub try_ranges: Vec<TryRange>,
    pub try_holes: Vec<(u32, u32, u32)>,
    pub live_ranges: Vec<(u32, u32, Reg, bool)>,
    pub type_hints: Vec<TypeHint>,
    pub num_caches: u32,
    loops: Vec<LoopCtx>,
    finallys: Vec<FinCtx>,
    pub call_depth: u32,
    labels: HashMap<&'a [u8], (u32, Vec<Region>)>,
    gotos: Vec<(&'a [u8], usize, u32, Vec<Region>)>,
    regions: Vec<Region>,
    region_ids: u32,
    pub flags: u32,
    pub num_statics: u32,
    pub line: u32,
    /// `__FUNCTION__`
    pub func_name: PhpStr,
    /// `__METHOD__`
    pub method_name: PhpStr,
    /// `__PROPERTY__` inside a hook.
    pub prop_name: Option<PhpStr>,
    pub is_generator: bool,
    pub in_class_method: bool,
    pub is_static_method: bool,
    pub uses: Vec<(u32, u32, bool)>,
    pub loop_free_stack: Vec<Reg>,
    /// Variables the pre-pass didn't see (hidden slots).
    pub extra_cvs: Vec<(&'a [u8], Reg)>,
}

impl<'a> FState<'a> {
    pub fn new(cvs: Vec<&'a [u8]>) -> FState<'a> {
        let n = cvs.len() as u32;
        FState {
            ops: Vec::new(),
            lines: Vec::new(),
            consts: Vec::new(),
            const_ints: HashMap::new(),
            const_strs: HashMap::new(),
            cvs,
            num_cvs: n,
            next_slot: n,
            free_tmps: Vec::new(),
            max_slot: n,
            paths: Vec::new(),
            lists: Vec::new(),
            tables: Vec::new(),
            children: Vec::new(),
            classes: Vec::new(),
            try_ranges: Vec::new(),
            try_holes: Vec::new(),
            live_ranges: Vec::new(),
            type_hints: Vec::new(),
            num_caches: 0,
            loops: Vec::new(),
            finallys: Vec::new(),
            call_depth: 0,
            labels: HashMap::new(),
            gotos: Vec::new(),
            regions: Vec::new(),
            region_ids: 0,
            flags: 0,
            num_statics: 0,
            line: 1,
            func_name: PhpStr::empty(),
            method_name: PhpStr::empty(),
            prop_name: None,
            is_generator: false,
            in_class_method: false,
            is_static_method: false,
            uses: Vec::new(),
            loop_free_stack: Vec::new(),
            extra_cvs: Vec::new(),
        }
    }
}

pub struct Compiler<'a> {
    pub(crate) file: PhpStr,
    pub(crate) ns: Vec<u8>,
    pub(crate) use_class: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) use_func: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) use_const: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) class: Option<ClassCtx>,
    pub(crate) strict: bool,
    pub(crate) f: FState<'a>,
    pub(crate) notices: Vec<CompileNotice>,
    pub(crate) anon_count: u32,
    pub(crate) closure_count: u32,
    pub(crate) src: &'a [u8],
    /// Constants known at compile time (internal constants), for folding.
    pub(crate) known_const: Option<&'a dyn Fn(&[u8]) -> Option<Value>>,
    /// Inside a const expression (initializer).
    pub(crate) in_const_expr: bool,
    /// Compiling the base of a member access chain.
    pub(crate) in_chain: bool,
    /// Pending `?->` short-circuit jumps of the current chain.
    pub(crate) nullsafe_jumps: Vec<(usize, Reg)>,
}

/// Compiles a parsed file. `eval_scope` is true for `eval()` code.
pub fn compile_program<'a>(
    prog: &'a Program<'a>,
    src: &'a [u8],
    file: &[u8],
    known_const: Option<&'a dyn Fn(&[u8]) -> Option<Value>>,
    eval_class: Option<(PhpStr, Option<PhpStr>)>,
) -> CResult<Unit> {
    let mut sc = scan::Scan::default();
    sc.stmts(prog.stmts);
    let mut c = Compiler {
        file: PhpStr::new(file),
        ns: Vec::new(),
        use_class: HashMap::new(),
        use_func: HashMap::new(),
        use_const: HashMap::new(),
        class: None,
        strict: false,
        f: FState::new(sc.vars),
        notices: Vec::new(),
        anon_count: 0,
        closure_count: 0,
        src,
        known_const,
        in_const_expr: false,
        in_chain: false,
        nullsafe_jumps: Vec::new(),
    };
    if let Some((cls, parent)) = eval_class {
        c.class = Some(ClassCtx {
            name: cls,
            parent,
            is_trait: false,
            is_interface: false,
            is_enum: false,
        });
    }
    for e in &prog.compile_errors {
        return Err(CompileError {
            message: e.message.clone(),
            line: e.line,
        });
    }
    for n in &prog.notices {
        c.notices.push(CompileNotice {
            level: match n.kind {
                NoticeKind::Deprecated => goro_runtime::Level::Deprecated,
                NoticeKind::Warning => goro_runtime::Level::Warning,
            },
            message: n.message.clone(),
            line: n.line,
        });
    }
    c.f.flags |= fflags::PSEUDO_MAIN;
    c.f.line = 1;
    let mut hoisted = Vec::new();
    let mut classes = Vec::new();
    c.check_strict_types(prog.stmts)?;
    c.top_stmts(prog.stmts, &mut hoisted, &mut classes)?;
    // Implicit `return 1` of a file (include's result).
    let one = c.int_const(1);
    c.emit(Op::Return { src: one });
    let halt = prog.halt_compiler_offset;
    let mut main = c.finish_func(PhpStr::empty(), Vec::new(), 0, None, 1, 0)?;
    if let Some(off) = halt {
        // __COMPILER_HALT_OFFSET__ is registered by the engine.
        main.consts.push(Value::Int(off as i64));
    }
    Ok(Unit {
        main: Rc::new(main),
        hoisted_funcs: hoisted,
        toplevel_classes: classes,
        notices: c.notices,
    })
}

impl<'a> Compiler<'a> {
    pub(crate) fn err<T>(&self, msg: impl Into<String>, line: u32) -> CResult<T> {
        Err(CompileError {
            message: msg.into(),
            line,
        })
    }

    pub(crate) fn notice(&mut self, level: goro_runtime::Level, msg: impl Into<String>, line: u32) {
        self.notices.push(CompileNotice {
            level,
            message: msg.into(),
            line,
        });
    }

    // ------------------------------------------------------------ emitting

    #[inline]
    pub(crate) fn emit(&mut self, op: Op) -> usize {
        self.f.ops.push(op);
        self.f.lines.push(self.f.line);
        self.f.ops.len() - 1
    }

    pub(crate) fn here(&self) -> u32 {
        self.f.ops.len() as u32
    }

    /// Sets the jump target of the instruction at `at`.
    pub(crate) fn patch(&mut self, at: usize, target: u32) {
        match &mut self.f.ops[at] {
            Op::Jmp { t }
            | Op::JmpZ { t, .. }
            | Op::JmpNZ { t, .. }
            | Op::JmpLt { t, .. }
            | Op::JmpZEx { t, .. }
            | Op::JmpNZEx { t, .. }
            | Op::JmpSet { t, .. }
            | Op::JmpNull { t, .. }
            | Op::RecvDefault { t, .. }
            | Op::FeReset { t, .. }
            | Op::FeFetch { t, .. }
            | Op::JmpArgByRef { t, .. }
            | Op::JmpNamedArgByRef { t, .. }
            | Op::FastCall { t, .. }
            | Op::BindStatic { t, .. } => *t = target,
            Op::Catch { next, .. } => *next = target,
            Op::New { nargs, .. } | Op::NewAnon { nargs, .. } => {
                // The skip target of `new` is stored in the high bits.
                *nargs = (*nargs & 0xFFFF) | (target << 16);
            }
            _ => panic!("patch of a non-jump instruction"),
        }
    }

    pub(crate) fn patch_here(&mut self, at: usize) {
        let h = self.here();
        self.patch(at, h);
    }

    pub(crate) fn cache(&mut self) -> u32 {
        self.f.num_caches += 1;
        self.f.num_caches - 1
    }

    // ------------------------------------------------------------ constants

    pub(crate) fn add_const(&mut self, v: Value) -> Reg {
        match &v {
            Value::Int(i) => {
                if let Some(&k) = self.f.const_ints.get(i) {
                    return CONST | k;
                }
                let k = self.f.consts.len() as u32;
                self.f.const_ints.insert(*i, k);
                self.f.consts.push(v);
                CONST | k
            }
            Value::Str(s) => {
                if let Some(&k) = self.f.const_strs.get(s.as_bytes()) {
                    return CONST | k;
                }
                let k = self.f.consts.len() as u32;
                self.f.const_strs.insert(s.as_bytes().to_vec(), k);
                self.f.consts.push(v);
                CONST | k
            }
            _ => {
                let k = self.f.consts.len() as u32;
                self.f.consts.push(v);
                CONST | k
            }
        }
    }

    pub(crate) fn int_const(&mut self, i: i64) -> Reg {
        self.add_const(Value::Int(i))
    }

    pub(crate) fn str_const(&mut self, s: &[u8]) -> Reg {
        self.add_const(Value::Str(PhpStr::intern(s)))
    }

    pub(crate) fn null_const(&mut self) -> Reg {
        self.add_const(Value::Null)
    }

    /// Adds `name` and its lower-case form at consecutive constant indices,
    /// returning the first index (not an operand).
    pub(crate) fn name_pair(&mut self, name: &[u8]) -> u32 {
        let k = self.f.consts.len() as u32;
        self.f.consts.push(Value::Str(PhpStr::intern(name)));
        self.f
            .consts
            .push(Value::Str(crate::class::lower_intern(name)));
        k
    }

    // ------------------------------------------------------------ slots

    /// A temporary (consumed by its reader).
    pub(crate) fn tmp(&mut self) -> Reg {
        let s = match self.f.free_tmps.pop() {
            Some(s) => s,
            None => {
                let s = self.f.next_slot;
                self.f.next_slot += 1;
                self.f.max_slot = self.f.max_slot.max(self.f.next_slot);
                s
            }
        };
        TMP | s
    }

    /// A hidden variable (read by copy, freed explicitly).
    pub(crate) fn hidden(&mut self) -> Reg {
        let s = self.tmp();
        s & SLOT_MASK
    }

    /// Two consecutive hidden slots (foreach iterators).
    pub(crate) fn hidden_pair(&mut self) -> Reg {
        let s = self.f.next_slot;
        self.f.next_slot += 2;
        self.f.max_slot = self.f.max_slot.max(self.f.next_slot);
        s
    }

    /// Releases a temporary or hidden slot for reuse.
    pub(crate) fn free(&mut self, r: Reg) {
        if r == NONE || r == THIS || is_const(r) {
            return;
        }
        let s = r & SLOT_MASK;
        if s >= self.f.num_cvs && !self.f.free_tmps.contains(&s) {
            self.f.free_tmps.push(s);
        }
    }

    pub(crate) fn free_pair(&mut self, r: Reg) {
        let _ = r;
    }

    pub(crate) fn cv(&mut self, name: &'a [u8]) -> Reg {
        if let Some(i) = self.f.cvs.iter().position(|n| *n == name) {
            return i as u32;
        }
        // Not found by the pre-pass (superglobals reached through a path
        // that has no special form): a hidden slot.
        if let Some(&(n, s)) = self.f.extra_cvs.iter().find(|(n, _)| *n == name) {
            let _ = n;
            return s;
        }
        let s = self.hidden();
        self.f.extra_cvs.push((name, s));
        s
    }

    pub(crate) fn has_cv(&self, name: &[u8]) -> Option<Reg> {
        self.f.cvs.iter().position(|n| *n == name).map(|i| i as u32)
    }

    // ------------------------------------------------------------ names

    fn qualify(&self, name: &[u8]) -> Vec<u8> {
        if self.ns.is_empty() {
            name.to_vec()
        } else {
            let mut v = self.ns.clone();
            v.push(b'\\');
            v.extend_from_slice(name);
            v
        }
    }

    /// Resolves a class name (PHP manual: Name resolution rules).
    pub(crate) fn resolve_class(&self, n: &Name<'a>) -> Vec<u8> {
        match n.kind {
            NameKind::FullyQualified => n.name.to_vec(),
            NameKind::Relative => self.qualify(n.name),
            NameKind::Unqualified | NameKind::Qualified => {
                let (first, rest) = match n.name.iter().position(|&c| c == b'\\') {
                    Some(i) => (&n.name[..i], Some(&n.name[i..])),
                    None => (n.name, None),
                };
                if let Some(full) = self.use_class.get(&first.to_ascii_lowercase()) {
                    let mut v = full.clone();
                    if let Some(r) = rest {
                        v.extend_from_slice(r);
                    }
                    return v;
                }
                self.qualify(n.name)
            }
        }
    }

    /// Resolves a function name: the qualified name, and the global fallback
    /// for unqualified names inside a namespace.
    pub(crate) fn resolve_func(&self, n: &Name<'a>) -> (Vec<u8>, Option<Vec<u8>>) {
        match n.kind {
            NameKind::FullyQualified => (n.name.to_vec(), None),
            NameKind::Relative => (self.qualify(n.name), None),
            NameKind::Qualified => (self.resolve_class(n), None),
            NameKind::Unqualified => {
                if let Some(full) = self.use_func.get(&n.name.to_ascii_lowercase()) {
                    return (full.clone(), None);
                }
                if self.ns.is_empty() {
                    (n.name.to_vec(), None)
                } else {
                    (self.qualify(n.name), Some(n.name.to_vec()))
                }
            }
        }
    }

    pub(crate) fn resolve_const_name(&self, n: &Name<'a>) -> (Vec<u8>, Option<Vec<u8>>) {
        match n.kind {
            NameKind::FullyQualified => (n.name.to_vec(), None),
            NameKind::Relative => (self.qualify(n.name), None),
            NameKind::Qualified => (self.resolve_class(n), None),
            NameKind::Unqualified => {
                if let Some(full) = self.use_const.get(n.name) {
                    return (full.clone(), None);
                }
                if self.ns.is_empty() {
                    (n.name.to_vec(), None)
                } else {
                    (self.qualify(n.name), Some(n.name.to_vec()))
                }
            }
        }
    }

    // ------------------------------------------------------------ functions

    /// Finishes the current function state into a [`FuncCode`].
    pub(crate) fn finish_func(
        &mut self,
        name: PhpStr,
        params: Vec<ParamInfo>,
        num_required: u32,
        ret_type: Option<TypeHint>,
        line_start: u32,
        line_end: u32,
    ) -> CResult<FuncCode> {
        // Resolve gotos.
        let gotos = std::mem::take(&mut self.f.gotos);
        for (label, at, line, gpath) in gotos {
            match self.f.labels.get(label).cloned() {
                Some((target, lpath)) => self.resolve_goto(at, line, target, &gpath, &lpath)?,
                None => {
                    return self.err(
                        format!(
                            "'goto' to undefined label '{}'",
                            String::from_utf8_lossy(label)
                        ),
                        line,
                    );
                }
            }
        }
        let f = &mut self.f;
        let cv_names: Vec<PhpStr> = f.cvs.iter().map(|n| PhpStr::intern(n)).collect();
        let caches = (0..f.num_caches)
            .map(|_| std::cell::Cell::new(Cache::default()))
            .collect();
        let mut flags = f.flags;
        if f.is_generator {
            flags |= fflags::GENERATOR;
        }
        if self.strict {
            flags |= fflags::STRICT;
        }
        Ok(FuncCode {
            name,
            class_name: self.class.as_ref().map(|c| c.name.clone()),
            file: self.file.clone(),
            ops: std::mem::take(&mut f.ops),
            lines: std::mem::take(&mut f.lines),
            consts: std::mem::take(&mut f.consts),
            num_cvs: f.num_cvs,
            num_slots: f.max_slot,
            cv_names,
            params,
            num_required,
            ret_type,
            flags,
            try_ranges: std::mem::take(&mut f.try_ranges),
            try_holes: std::mem::take(&mut f.try_holes),
            live_ranges: std::mem::take(&mut f.live_ranges),
            paths: std::mem::take(&mut f.paths),
            lists: std::mem::take(&mut f.lists),
            tables: std::mem::take(&mut f.tables),
            children: std::mem::take(&mut f.children),
            classes: std::mem::take(&mut f.classes),
            uses: std::mem::take(&mut f.uses),
            num_statics: f.num_statics,
            type_hints: std::mem::take(&mut f.type_hints),
            line_start,
            line_end,
            doc_comment: None,
            attrs: Rc::from(Vec::new()),
            caches,
            cv_map: std::cell::RefCell::new(None),
            statics: std::cell::RefCell::new(vec![None; f.num_statics as usize]),
        })
    }

    // ------------------------------------------------------------ statements

    fn check_strict_types(&mut self, stmts: &'a [Stmt<'a>]) -> CResult<()> {
        let mut first = true;
        for s in stmts {
            if let StmtKind::Declare(d) = &s.kind {
                for (k, v) in d.directives {
                    if k.name.eq_ignore_ascii_case(b"strict_types") {
                        if !first {
                            return self.err(
                                "strict_types declaration must be the very first statement in the script",
                                s.span.line,
                            );
                        }
                        if !matches!(d.body, DeclareBody::None) {
                            return self.err(
                                "strict_types declaration must not use block mode",
                                s.span.line,
                            );
                        }
                        match v.kind {
                            ExprKind::Int(1) => self.strict = true,
                            ExprKind::Int(0) => self.strict = false,
                            _ => {
                                return self.err(
                                    "declare(strict_types) value must be a literal",
                                    s.span.line,
                                );
                            }
                        }
                    }
                }
            }
            match &s.kind {
                StmtKind::InlineHtml(t) if first && is_shebang_or_empty(t) => {}
                StmtKind::Declare(_) => {}
                _ => first = false,
            }
        }
        Ok(())
    }

    fn top_stmts(
        &mut self,
        stmts: &'a [Stmt<'a>],
        hoisted: &mut Vec<u32>,
        classes: &mut Vec<u32>,
    ) -> CResult<()> {
        for s in stmts {
            match &s.kind {
                StmtKind::Function(fd) => {
                    self.f.line = s.span.line;
                    let idx = self.func_decl(fd)?;
                    hoisted.push(idx);
                }
                StmtKind::Class(cd) if cd.name.is_some() => {
                    self.f.line = s.span.line;
                    let idx = self.class_decl(cd)?;
                    classes.push(idx);
                    self.emit(Op::DeclareClass { class: idx });
                }
                StmtKind::Namespace { name, body } => {
                    self.ns = name.map(|n| n.name.to_vec()).unwrap_or_default();
                    self.use_class.clear();
                    self.use_func.clear();
                    self.use_const.clear();
                    if let Some(b) = body {
                        self.top_stmts(b, hoisted, classes)?;
                        self.ns.clear();
                        self.use_class.clear();
                        self.use_func.clear();
                        self.use_const.clear();
                    }
                }
                StmtKind::Declare(d) => {
                    self.declare_directives(d, s.span.line)?;
                    match d.body {
                        DeclareBody::None => {}
                        DeclareBody::Block(b) | DeclareBody::Alt(b) => {
                            self.top_stmts(b, hoisted, classes)?
                        }
                    }
                }
                StmtKind::HaltCompiler { .. } => break,
                _ => self.stmt(s)?,
            }
        }
        Ok(())
    }

    fn declare_directives(&mut self, d: &'a DeclareStmt<'a>, line: u32) -> CResult<()> {
        for (k, v) in d.directives {
            if k.name.eq_ignore_ascii_case(b"ticks") {
                if !matches!(v.kind, ExprKind::Int(_)) {
                    // PHP requires a literal.
                }
            } else if k.name.eq_ignore_ascii_case(b"encoding") {
                if !matches!(v.kind, ExprKind::String(..)) {
                    return self.err("Encoding must be a literal", line);
                }
            } else if k.name.eq_ignore_ascii_case(b"strict_types") {
            } else {
                self.notice(
                    goro_runtime::Level::Warning,
                    format!("Unsupported declare '{}'", String::from_utf8_lossy(k.name)),
                    line,
                );
            }
        }
        Ok(())
    }

    pub(crate) fn block(&mut self, b: Block<'a>) -> CResult<()> {
        for s in b {
            self.stmt(s)?;
        }
        Ok(())
    }

    pub(crate) fn stmt(&mut self, s: &'a Stmt<'a>) -> CResult<()> {
        self.f.line = s.span.line;
        match &s.kind {
            StmtKind::Expr(e) => self.expr_stmt(e)?,
            StmtKind::Echo(es) => {
                for e in *es {
                    self.f.line = e.span.line;
                    let r = self.expr(e)?;
                    self.free(r);
                    self.emit(Op::Echo { src: r });
                }
            }
            StmtKind::InlineHtml(t) => {
                if !t.is_empty() {
                    let c = self.str_const(t);
                    self.emit(Op::Echo { src: c });
                }
            }
            StmtKind::Block(b) => self.block(b)?,
            StmtKind::If(i) => self.if_stmt(i)?,
            StmtKind::While { cond, body, .. } => {
                // Layout: jmp cond; body; cond: jnz body
                let j = self.emit(Op::Jmp { t: 0 });
                let top = self.here();
                self.push_loop(false, None);
                self.block(body)?;
                let cont = self.here();
                self.patch_here(j);
                self.f.line = cond.span.line;
                self.cond_jump(cond, true, top)?;
                self.pop_loop(cont);
            }
            StmtKind::DoWhile { body, cond } => {
                let top = self.here();
                self.push_loop(false, None);
                self.block(body)?;
                let cont = self.here();
                self.f.line = cond.span.line;
                self.cond_jump(cond, true, top)?;
                self.pop_loop(cont);
            }
            StmtKind::For(f) => self.for_stmt(f)?,
            StmtKind::Foreach(f) => self.foreach_stmt(f, s.span.line)?,
            StmtKind::Switch(sw) => self.switch_stmt(sw)?,
            StmtKind::Break(e) => self.break_continue(e, true, s.span.line)?,
            StmtKind::Continue(e) => self.break_continue(e, false, s.span.line)?,
            StmtKind::Return(e) => self.return_stmt(e.as_ref(), s.span.line)?,
            StmtKind::Global(vars) => {
                for v in *vars {
                    match &v.kind {
                        ExprKind::Variable(n) => {
                            if *n == b"this" {
                                return self
                                    .err("Cannot use $this as global variable", v.span.line);
                            }
                            let cv = self.cv(n);
                            let name = self.str_const(n);
                            self.emit(Op::BindGlobal { var: cv, name });
                        }
                        ExprKind::VariableVariable(x) => {
                            // global $$x: bind the variable named by x.
                            let name = self.expr(x)?;
                            self.free(name);
                            self.emit(Op::BindGlobal { var: NONE, name });
                        }
                        _ => {}
                    }
                }
            }
            StmtKind::Static(vars) => {
                for v in *vars {
                    if v.name.name == b"this" {
                        return self.err("Cannot use $this as static variable", v.span.line);
                    }
                    let cv = self.cv(v.name.name);
                    let index = self.f.num_statics;
                    self.f.num_statics += 1;
                    let at = self.emit(Op::BindStatic {
                        var: cv,
                        index,
                        t: 0,
                    });
                    let src = match &v.default {
                        Some(d) => self.expr(d)?,
                        None => self.null_const(),
                    };
                    self.free(src);
                    self.emit(Op::StaticInit { index, src });
                    self.patch_here(at);
                    let _ = cv;
                }
            }
            StmtKind::Unset(es) => {
                for e in *es {
                    self.unset(e)?;
                }
            }
            StmtKind::Try(t) => self.try_stmt(t)?,
            StmtKind::Goto(l) => {
                let at = self.emit(Op::Jmp { t: 0 });
                let path = self.f.regions.clone();
                self.f.gotos.push((l.name, at, s.span.line, path));
            }
            StmtKind::Label(l) => {
                if self.f.labels.contains_key(l.name) {
                    return self.err(
                        format!(
                            "Label '{}' already defined",
                            String::from_utf8_lossy(l.name)
                        ),
                        s.span.line,
                    );
                }
                let h = self.here();
                let path = self.f.regions.clone();
                self.f.labels.insert(l.name, (h, path));
            }
            StmtKind::Declare(d) => {
                self.declare_directives(d, s.span.line)?;
                match d.body {
                    DeclareBody::None => {}
                    DeclareBody::Block(b) | DeclareBody::Alt(b) => self.block(b)?,
                }
            }
            StmtKind::Function(fd) => {
                let idx = self.func_decl(fd)?;
                self.emit(Op::DeclareFunction { func: idx });
            }
            StmtKind::Class(cd) => {
                let idx = self.class_decl(cd)?;
                self.emit(Op::DeclareClass { class: idx });
            }
            StmtKind::Namespace { .. } => {
                return self.err(
                    "Namespace declaration statement has to be the very first statement or after any declare call in the script",
                    s.span.line,
                );
            }
            StmtKind::Use { kind, uses } => {
                for u in *uses {
                    self.add_use(*kind, u, None, s.span.line)?;
                }
            }
            StmtKind::GroupUse { kind, prefix, uses } => {
                for u in *uses {
                    let k = if *kind == UseKind::Normal {
                        u.kind
                    } else {
                        *kind
                    };
                    self.add_use(k, u, Some(prefix), s.span.line)?;
                }
            }
            StmtKind::Const { items, .. } => {
                for c in *items {
                    let full = self.qualify(c.name.name);
                    let name = self.str_const(&full);
                    let v = self.expr(&c.value)?;
                    self.free(v);
                    self.emit(Op::DeclareConst { name, val: v });
                }
            }
            StmtKind::HaltCompiler { .. } => {}
            StmtKind::Nop => {}
        }
        Ok(())
    }

    fn add_use(
        &mut self,
        kind: UseKind,
        u: &UseItem<'a>,
        prefix: Option<&Name<'a>>,
        line: u32,
    ) -> CResult<()> {
        let mut full = Vec::new();
        if let Some(p) = prefix {
            full.extend_from_slice(p.name);
            full.push(b'\\');
        }
        full.extend_from_slice(u.name.name);
        let alias: Vec<u8> = match &u.alias {
            Some(a) => a.name.to_vec(),
            None => full
                .rsplit(|&c| c == b'\\')
                .next()
                .unwrap_or(&full)
                .to_vec(),
        };
        match kind {
            UseKind::Normal => {
                let la = alias.to_ascii_lowercase();
                if matches!(&la[..], b"self" | b"parent" | b"static") {
                    return self.err(
                        format!(
                            "Cannot use {} as {} because '{}' is a special class name",
                            String::from_utf8_lossy(&full),
                            String::from_utf8_lossy(&alias),
                            String::from_utf8_lossy(&alias)
                        ),
                        line,
                    );
                }
                if let Some(prev) = self.use_class.get(&la)
                    && !prev.eq_ignore_ascii_case(&full)
                {
                    return self.err(
                        format!(
                            "Cannot use {} as {} because the name is already in use",
                            String::from_utf8_lossy(&full),
                            String::from_utf8_lossy(&alias)
                        ),
                        line,
                    );
                }
                self.use_class.insert(la, full);
            }
            UseKind::Function => {
                self.use_func.insert(alias.to_ascii_lowercase(), full);
            }
            UseKind::Const => {
                self.use_const.insert(alias, full);
            }
        }
        Ok(())
    }

    fn if_stmt(&mut self, i: &'a IfStmt<'a>) -> CResult<()> {
        let mut ends = Vec::new();
        let j = self.cond_jump_fwd(&i.cond, false)?;
        self.block(i.then)?;
        let mut pending = j;
        for ei in i.elseifs {
            ends.push(self.emit(Op::Jmp { t: 0 }));
            self.patch_here(pending);
            self.f.line = ei.cond.span.line;
            pending = self.cond_jump_fwd(&ei.cond, false)?;
            self.block(ei.body)?;
        }
        if let Some(e) = i.else_ {
            ends.push(self.emit(Op::Jmp { t: 0 }));
            self.patch_here(pending);
            self.block(e)?;
        } else {
            self.patch_here(pending);
        }
        for e in ends {
            self.patch_here(e);
        }
        Ok(())
    }

    fn for_stmt(&mut self, f: &'a ForStmt<'a>) -> CResult<()> {
        for e in f.init {
            self.expr_stmt(e)?;
        }
        let j = self.emit(Op::Jmp { t: 0 });
        let top = self.here();
        self.push_loop(false, None);
        self.block(f.body)?;
        let cont = self.here();
        for e in f.step {
            self.f.line = e.span.line;
            self.expr_stmt(e)?;
        }
        self.patch_here(j);
        if f.cond.is_empty() {
            self.emit(Op::Jmp { t: top });
        } else {
            let n = f.cond.len();
            for (k, c) in f.cond.iter().enumerate() {
                self.f.line = c.span.line;
                if k + 1 < n {
                    self.expr_stmt(c)?;
                } else {
                    self.cond_jump(c, true, top)?;
                }
            }
        }
        self.pop_loop(cont);
        Ok(())
    }

    fn foreach_stmt(&mut self, f: &'a ForeachStmt<'a>, line: u32) -> CResult<()> {
        let iter = self.hidden_pair();
        let src = if f.by_ref {
            self.ref_of(&f.expr)?
        } else {
            self.expr(&f.expr)?
        };
        self.free(src);
        self.f.line = line;
        let reset = self.emit(Op::FeReset {
            iter,
            src,
            by_ref: f.by_ref,
            t: 0,
        });
        let top = self.here();
        // Value target.
        let (val_dst, direct_val) = match &f.value.kind {
            ExprKind::Variable(n) if !f.by_ref && *n != b"this" => (self.cv(n), true),
            _ => (self.tmp(), false),
        };
        let (key_dst, direct_key) = match &f.key {
            None => (NONE, true),
            Some(k) => match &k.kind {
                ExprKind::Variable(n) if *n != b"this" => (self.cv(n), true),
                _ => (self.tmp(), false),
            },
        };
        self.f.line = line;
        let fetch = self.emit(Op::FeFetch {
            iter,
            val: val_dst,
            key: key_dst,
            t: 0,
        });
        if !direct_val {
            if f.by_ref {
                self.bind_ref_to(&f.value, val_dst)?;
            } else if let ExprKind::List { items, .. } | ExprKind::Array { items, .. } =
                &f.value.kind
            {
                let h = self.hidden();
                self.emit(Op::Copy {
                    dst: h,
                    src: val_dst,
                });
                self.free(val_dst);
                self.destructure(items, h, line)?;
                self.emit(Op::Free { slot: h });
                self.free(h);
            } else {
                self.assign_to(&f.value, val_dst, NONE)?;
            }
        }
        if !direct_key {
            self.assign_to(f.key.as_ref().unwrap(), key_dst, NONE)?;
        }
        self.push_loop(false, Some((iter, true)));
        self.block(f.body)?;
        self.emit(Op::Jmp { t: top });
        let end = self.here();
        self.patch(reset, end);
        self.patch(fetch, end);
        self.pop_loop(top);
        self.emit(Op::FeFree { iter });
        Ok(())
    }

    fn switch_stmt(&mut self, sw: &'a SwitchStmt<'a>) -> CResult<()> {
        let subj = self.expr(&sw.subject)?;
        let s = self.hidden();
        self.free(subj);
        self.emit(Op::Copy { dst: s, src: subj });
        let mut case_jumps = Vec::new();
        let mut default_idx = None;
        for (i, c) in sw.cases.iter().enumerate() {
            match &c.cond {
                Some(e) => {
                    self.f.line = c.span.line;
                    let v = self.expr(e)?;
                    self.free(v);
                    let t = self.tmp();
                    self.emit(Op::Eq { dst: t, a: s, b: v });
                    self.free(t);
                    case_jumps.push((i, self.emit(Op::JmpNZ { c: t, t: 0 })));
                }
                None => {
                    if default_idx.is_some() {
                        return self.err(
                            "Switch statements may only contain one default clause",
                            c.span.line,
                        );
                    }
                    default_idx = Some(i);
                }
            }
        }
        let to_default = self.emit(Op::Jmp { t: 0 });
        self.push_loop(true, Some((s, false)));
        let mut starts = Vec::new();
        for c in sw.cases {
            starts.push(self.here());
            self.block(c.body)?;
        }
        let end = self.here();
        for (i, at) in case_jumps {
            self.patch(at, starts[i]);
        }
        match default_idx {
            Some(d) => self.patch(to_default, starts[d]),
            None => self.patch(to_default, end),
        }
        self.pop_loop(end);
        self.emit(Op::Free { slot: s });
        self.free(s);
        Ok(())
    }

    fn push_loop(&mut self, is_switch: bool, free: Option<(Reg, bool)>) {
        let fd = self.f.finallys.len();
        let region_len = self.f.regions.len();
        self.f.region_ids += 1;
        self.f.regions.push(Region::Loop(self.f.region_ids, free));
        self.f.loops.push(LoopCtx {
            breaks: Vec::new(),
            continues: Vec::new(),
            is_switch,
            free,
            finally_depth: fd,
            region_len,
            start: self.f.ops.len() as u32,
        });
    }

    /// Patches a goto: directly, or through a stub that frees the loop
    /// temporaries and runs the finally blocks of the blocks it leaves.
    fn resolve_goto(&mut self, at: usize, line: u32, target: u32, g: &[Region], l: &[Region]) -> CResult<()> {
        let k = g.iter().zip(l.iter()).take_while(|(a, b)| a == b).count();
        if l[k..].iter().any(|r| matches!(r, Region::Finally(_))) {
            return self.err("jump into a finally block is disallowed", line);
        }
        if l[k..].iter().any(|r| matches!(r, Region::Loop(..))) {
            return self.err("'goto' into loop or switch statement is disallowed", line);
        }
        if g[k..].iter().any(|r| matches!(r, Region::Finally(_))) {
            return self.err("jump out of a finally block is disallowed", line);
        }
        let needs_stub = g[k..].iter().any(|r| match r {
            Region::Loop(_, f) => f.is_some(),
            Region::Try(i) | Region::Catch(i) => self.f.try_ranges[*i as usize].finally != 0,
            Region::Finally(_) => false,
        });
        if !needs_stub {
            self.patch(at, target);
            return Ok(());
        }
        let stub = self.here();
        self.patch(at, stub);
        self.f.line = line;
        self.leave_regions(&g[k..]);
        self.emit(Op::Jmp { t: target });
        Ok(())
    }

    /// Emits the code leaving the blocks of `path` (innermost last): loop
    /// temporaries are freed and finally blocks called. Each instruction is
    /// outside the try statements already left (an exception thrown by a
    /// destructor there is not caught by them). Returns those try ranges.
    fn leave_regions(&mut self, path: &[Region]) -> Vec<u32> {
        let mut exited: Vec<u32> = Vec::new();
        for r in path.iter().rev() {
            match *r {
                Region::Loop(_, Some((slot, is_iter))) => {
                    let at = self.here();
                    if is_iter {
                        self.emit(Op::FeFree { iter: slot });
                    } else {
                        self.emit(Op::Free { slot });
                    }
                    self.add_holes(at, self.here(), &exited);
                }
                Region::Loop(_, None) => {}
                Region::Try(i) | Region::Catch(i) => {
                    let tr = self.f.try_ranges[i as usize];
                    if tr.finally_ret != NONE {
                        let at = self.emit(Op::FastCall {
                            t: tr.finally,
                            ret: tr.finally_ret,
                        });
                        if tr.finally == 0
                            && let Some(fc) = self.f.finallys.iter_mut().find(|f| f.range == i)
                        {
                            fc.fastcalls.push(at);
                        }
                        self.add_holes(at as u32, at as u32 + 1, &exited);
                    }
                    exited.push(i);
                }
                Region::Finally(i) => exited.push(i),
            }
        }
        exited
    }

    fn add_holes(&mut self, start: u32, end: u32, tries: &[u32]) {
        if start < end {
            for &t in tries {
                self.f.try_holes.push((start, end, t));
            }
        }
    }

    /// Ends a loop: breaks jump here, continues to `cont`.
    fn pop_loop(&mut self, cont: u32) {
        let l = self.f.loops.pop().unwrap();
        self.f.regions.truncate(l.region_len);
        let end = self.here();
        if let Some((slot, is_iter)) = l.free {
            self.f.live_ranges.push((l.start, end, slot, is_iter));
        }
        for b in l.breaks {
            self.patch(b, end);
        }
        for c in l.continues {
            self.patch(c, cont);
        }
    }

    fn break_continue(&mut self, e: &Option<Expr<'a>>, is_break: bool, line: u32) -> CResult<()> {
        let what = if is_break { "break" } else { "continue" };
        let n = match e {
            None => 1,
            Some(Expr {
                kind: ExprKind::Int(n),
                ..
            }) => {
                if *n < 1 {
                    return self.err(
                        format!("'{what}' operator accepts only positive integers"),
                        line,
                    );
                }
                *n as usize
            }
            Some(_) => {
                return self.err(
                    format!("'{what}' operator with non-integer operand is no longer supported"),
                    line,
                );
            }
        };
        if self.f.loops.is_empty() {
            return self.err(
                format!("'{what}' not in the 'loop' or 'switch' context"),
                line,
            );
        }
        if n > self.f.loops.len() {
            return self.err(
                format!(
                    "Cannot '{}' {} level{}",
                    what,
                    n,
                    if n == 1 { "" } else { "s" }
                ),
                line,
            );
        }
        let target = self.f.loops.len() - n;
        let rl = self.f.loops[target].region_len;
        if self.f.regions[rl..].iter().any(|r| matches!(r, Region::Finally(_))) {
            return self.err("jump out of a finally block is disallowed", line);
        }
        if !is_break && self.f.loops[target].is_switch {
            // continue targeting switch acts as break (compile warning
            // emitted by PHP for `continue` without levels).
            let msg = if n == 1 {
                "\"continue\" targeting switch is equivalent to \"break\"".to_string()
            } else {
                format!(
                    "\"continue {}\" targeting switch is equivalent to \"break {}\"",
                    n, n
                )
            };
            self.notice(goro_runtime::Level::Warning, msg, line);
        }
        // Leave the blocks nested in the target loop: their loop
        // temporaries are freed and finally blocks run, innermost first
        // (the target's own iterator is freed at its end label on break,
        // and kept on continue).
        let path = self.f.regions[rl + 1..].to_vec();
        self.leave_regions(&path);
        let at = self.emit(Op::Jmp { t: 0 });
        if is_break || self.f.loops[target].is_switch {
            self.f.loops[target].breaks.push(at);
        } else {
            self.f.loops[target].continues.push(at);
        }
        Ok(())
    }

    fn return_stmt(&mut self, e: Option<&'a Expr<'a>>, line: u32) -> CResult<()> {
        let by_ref = self.f.flags & fflags::BY_REF != 0;
        if self.f.flags & fflags::RETURNS_VOID != 0 && e.is_some() && !self.f.is_generator {
            if matches!(e.map(|x| &x.kind), Some(ExprKind::Const(n)) if n.name.eq_ignore_ascii_case(b"null"))
            {
                return self.err(
                    "A void function must not return a value (did you mean \"return;\" instead of \"return null;\"?)",
                    line,
                );
            }
            return self.err("A void function must not return a value", line);
        }
        if self.f.flags & fflags::RETURNS_NEVER != 0 && !self.f.is_generator {
            return self.err("A never-returning function must not return", line);
        }
        let val = match e {
            None => self.null_const(),
            Some(e) if by_ref && !self.f.is_generator && is_ref_capable(e) => {
                let r = self.ref_of(e)?;
                if self.f.flags & fflags::HAS_RETURN_TYPE != 0 {
                    self.emit(Op::VerifyReturn {
                        src: r & !TMP,
                        implicit: false,
                    });
                }
                if !self.f.finallys.is_empty()
                    || !self.f.loop_free_stack.is_empty()
                    || self.has_loop_frees()
                {
                    let h = self.hidden();
                    self.emit(Op::Copy { dst: h, src: r });
                    self.free(r);
                    self.finish_return(h, true, line)?;
                    return Ok(());
                }
                self.free(r);
                self.f.line = line;
                self.emit(Op::ReturnRef { src: r });
                return Ok(());
            }
            Some(e) if by_ref && !self.f.is_generator => {
                // Not a variable: returned by reference with a notice.
                let mut v = self.expr(e)?;
                if self.f.flags & fflags::HAS_RETURN_TYPE != 0 {
                    if is_const(v) {
                        let t = self.tmp();
                        self.emit(Op::Copy { dst: t, src: v });
                        v = t;
                    }
                    self.emit(Op::VerifyReturn {
                        src: v & !TMP,
                        implicit: false,
                    });
                }
                if !self.f.finallys.is_empty() || self.has_loop_frees() {
                    let h = self.hidden();
                    self.emit(Op::Copy { dst: h, src: v });
                    self.free(v);
                    self.finish_return(h, true, line)?;
                    return Ok(());
                }
                self.free(v);
                self.f.line = line;
                self.emit(Op::ReturnRef { src: v });
                return Ok(());
            }
            Some(e) => self.expr(e)?,
        };
        if !self.f.finallys.is_empty() || self.has_loop_frees() {
            let h = self.hidden();
            self.emit(Op::Copy { dst: h, src: val });
            self.free(val);
            self.finish_return(h, false, line)?;
            return Ok(());
        }
        self.free(val);
        self.f.line = line;
        self.emit_return(val, e.is_none());
        Ok(())
    }

    /// Frees the loop temporaries of loops `from..to`, innermost first.
    fn free_loops(&mut self, from: usize, to: usize) {
        if from >= to {
            return;
        }
        let frees: Vec<(Reg, bool)> = self.f.loops[from..to].iter().rev().filter_map(|l| l.free).collect();
        for (slot, is_iter) in frees {
            if is_iter {
                self.emit(Op::FeFree { iter: slot });
            } else {
                self.emit(Op::Free { slot });
            }
        }
    }

    fn has_loop_frees(&self) -> bool {
        self.f.loops.iter().any(|l| l.free.is_some())
    }

    fn finish_return(&mut self, h: Reg, is_ref: bool, line: u32) -> CResult<()> {
        let path = self.f.regions.clone();
        let exited = self.leave_regions(&path);
        self.f.line = line;
        let at = self.here();
        if is_ref {
            self.emit(Op::ReturnRef { src: h });
        } else {
            self.emit_return(h, false);
        }
        self.add_holes(at, self.here(), &exited);
        Ok(())
    }

    pub(crate) fn emit_return(&mut self, val: Reg, implicit: bool) {
        if self.f.is_generator {
            self.emit(Op::GeneratorReturn { src: val });
        } else {
            let mut val = val;
            if self.f.flags & fflags::HAS_RETURN_TYPE != 0
                && self.f.flags & fflags::RETURNS_VOID == 0
            {
                if is_const(val) {
                    let t = self.tmp();
                    self.emit(Op::Copy { dst: t, src: val });
                    self.free(t);
                    val = t;
                }
                self.emit(Op::VerifyReturn {
                    src: val & !TMP,
                    implicit,
                });
            }
            self.emit(Op::Return { src: val });
        }
    }

    fn try_stmt(&mut self, t: &'a TryStmt<'a>) -> CResult<()> {
        let has_finally = t.finally.is_some();
        let ret = if has_finally { self.hidden() } else { NONE };
        let exc = if has_finally { self.hidden() } else { NONE };
        let start = self.here();
        let range_idx = self.f.try_ranges.len();
        self.f.try_ranges.push(TryRange {
            start,
            end: 0,
            catch: 0,
            finally: 0,
            finally_end: 0,
            finally_ret: ret,
            exc_slot: exc,
            call_depth: self.f.call_depth,
        });
        if has_finally {
            self.f.finallys.push(FinCtx {
                ret,
                fastcalls: Vec::new(),
                range: range_idx as u32,
            });
        }
        self.f.regions.push(Region::Try(range_idx as u32));
        self.block(t.body)?;
        self.f.regions.pop();
        let mut to_end = Vec::new();
        if has_finally {
            let at = self.emit(Op::FastCall { t: 0, ret });
            self.f.finallys.last_mut().unwrap().fastcalls.push(at);
        }
        to_end.push(self.emit(Op::Jmp { t: 0 }));
        let body_end = self.here();
        self.f.try_ranges[range_idx].end = body_end;
        if !t.catches.is_empty() {
            self.f.try_ranges[range_idx].catch = body_end;
            let mut pending_next: Option<usize> = None;
            for c in t.catches {
                if let Some(p) = pending_next {
                    self.patch_here(p);
                }
                self.f.line = c.span.line;
                let mut names = Vec::new();
                for n in c.types {
                    let full = self.resolve_class(n);
                    let lf = full.to_ascii_lowercase();
                    if lf == b"self" || lf == b"static" || lf == b"parent" {
                        // resolved at run time via class context
                    }
                    let k = self.str_const(&full);
                    names.push(k);
                }
                let list = self.f.lists.len() as u32;
                self.f.lists.push(names);
                let var = match &c.var {
                    Some(v) => {
                        if v.name == b"this" {
                            return self.err("Cannot re-assign $this", c.span.line);
                        }
                        self.cv(v.name)
                    }
                    None => NONE,
                };
                let at = self.emit(Op::Catch { list, var, next: 0 });
                pending_next = Some(at);
                self.f.regions.push(Region::Catch(range_idx as u32));
                self.block(c.body)?;
                self.f.regions.pop();
                if has_finally {
                    let at = self.emit(Op::FastCall { t: 0, ret });
                    self.f.finallys.last_mut().unwrap().fastcalls.push(at);
                }
                to_end.push(self.emit(Op::Jmp { t: 0 }));
            }
            if let Some(p) = pending_next {
                self.patch_here(p);
            }
            self.emit(Op::Rethrow);
        }
        if let Some(fin) = t.finally {
            let fc = self.f.finallys.pop().unwrap();
            let fstart = self.here();
            self.f.try_ranges[range_idx].finally = fstart;
            for at in fc.fastcalls {
                self.patch(at, fstart);
            }
            self.f.regions.push(Region::Finally(range_idx as u32));
            self.block(fin)?;
            self.f.regions.pop();
            self.emit(Op::FastRet {
                ret,
                try_index: range_idx as u32,
            });
            self.f.try_ranges[range_idx].finally_end = self.here();
        }
        for j in to_end {
            self.patch_here(j);
        }
        Ok(())
    }

    // ------------------------------------------------------------ conditions

    /// Emits a jump to `target` when `cond` is `when`.
    fn cond_jump(&mut self, cond: &'a Expr<'a>, when: bool, target: u32) -> CResult<()> {
        // Fused compare-and-jump for `$i < $n` loops.
        if when
            && let ExprKind::Binary {
                op: BinaryOp::Less,
                left,
                right,
            } = &cond.kind
            && is_simple_operand(left)
            && is_simple_operand(right)
        {
            let a = self.expr(left)?;
            let b = self.expr(right)?;
            self.emit(Op::JmpLt { a, b, t: target });
            return Ok(());
        }
        let c = self.expr(cond)?;
        self.free(c);
        if when {
            self.emit(Op::JmpNZ { c, t: target });
        } else {
            self.emit(Op::JmpZ { c, t: target });
        }
        Ok(())
    }

    /// Emits a forward jump taken when `cond` is `when`; returns it for
    /// patching.
    fn cond_jump_fwd(&mut self, cond: &'a Expr<'a>, when: bool) -> CResult<usize> {
        let c = self.expr(cond)?;
        self.free(c);
        Ok(if when {
            self.emit(Op::JmpNZ { c, t: 0 })
        } else {
            self.emit(Op::JmpZ { c, t: 0 })
        })
    }
}

fn is_shebang_or_empty(t: &[u8]) -> bool {
    t.starts_with(b"#!") || t.iter().all(|c| c.is_ascii_whitespace())
}

/// Operands that compile to a variable or constant (no instruction).
fn is_simple_operand(e: &Expr<'_>) -> bool {
    match &e.kind {
        ExprKind::Variable(n) => *n != b"this",
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(..) => true,
        _ => false,
    }
}

/// Expressions that can be returned/passed by reference.
pub(crate) fn is_ref_capable(e: &Expr<'_>) -> bool {
    match &e.kind {
        ExprKind::Variable(n) => *n != b"this",
        ExprKind::VariableVariable(_) | ExprKind::StaticProp(_) => true,
        ExprKind::Index { base, .. } => {
            is_ref_capable(base)
                || matches!(
                    base.kind,
                    ExprKind::Prop { .. }
                        | ExprKind::Call { .. }
                        | ExprKind::MethodCall { .. }
                        | ExprKind::StaticCall(_)
                )
        }
        ExprKind::Prop { nullsafe, .. } => !nullsafe,
        _ => false,
    }
}
