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
mod hrh;
mod scan;

use std::collections::HashMap;
use std::rc::Rc;

use goro_runtime::{PhpStr, Value};
use goro_syntax::ast::*;

use crate::bytecode::*;
use crate::class::ClassDecl;

pub use decl::parse_native_sig;

/// Whether a variable name is a superglobal.
/// A string literal's bytes in the internal encoding (zend.multibyte with
/// a non-UTF-8 internal_encoding; test ext/mbstring/tests/zend_multibyte-02).
pub(crate) fn lit(s: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match LITERAL_ENCODING.with(|c| c.get()) {
        Some(e) if !s.is_ascii() => std::borrow::Cow::Owned(
            goro_charset::convert(s, e, goro_charset::Encoding::Utf8, &Default::default()).bytes,
        ),
        _ => std::borrow::Cow::Borrowed(s),
    }
}

/// A name class_alias() refuses (a reserved type name).
pub fn is_reserved_class_alias(l: &[u8]) -> bool {
    decl::is_reserved_type_name_pub(l)
}

pub fn is_superglobal_name(n: &[u8]) -> bool {
    scan::is_superglobal(n)
}

/// A fatal compile-time error ("Fatal error: ... on line N").
#[derive(Clone, Debug)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
    /// Diagnostics raised by the compilation before the error (shown
    /// first).
    pub notices: Vec<CompileNotice>,
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
}

pub(crate) struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    is_switch: bool,
    /// Iterator (foreach) or subject (switch) slot to free when leaving.
    free: Option<(Reg, bool)>,
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
    fastcalls: Vec<usize>,
    /// The try range this finally belongs to.
    range: u32,
}

/// State of the function being compiled.
pub(crate) struct FState<'a> {
    /// The declared return type accepts null.
    pub ret_allows_null: bool,
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
    /// Names of the `static` variables declared so far.
    pub static_names: Vec<Vec<u8>>,
    pub const_attrs: Vec<(PhpStr, u32, Rc<[AttrInfo]>)>,
    pub discarded_calls: Vec<u32>,
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
            ret_allows_null: false,
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
            static_names: Vec::new(),
            const_attrs: Vec::new(),
            discarded_calls: Vec::new(),
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
    /// Classes declared at the top of this file (lower-case full names).
    pub(crate) declared_classes: std::collections::HashSet<Vec<u8>>,
    pub(crate) use_func: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) use_const: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) class: Option<ClassCtx>,
    pub(crate) strict: bool,
    pub(crate) f: FState<'a>,
    pub(crate) notices: Vec<CompileNotice>,
    pub(crate) anon_count: u32,
    pub(crate) src: &'a [u8],
    /// Constants known at compile time (internal constants), for folding.
    pub(crate) known_const: Option<&'a dyn Fn(&[u8]) -> Option<Value>>,
    /// `declare(ticks=N)` in effect (0: none).
    pub(crate) ticks: u32,
    /// A statement other than declare() was compiled at the top level.
    pub(crate) past_first_stmt: bool,
    /// The zend.multibyte setting.
    pub zend_multibyte: bool,
    /// Inside a const expression (initializer).
    pub(crate) in_const_expr: bool,
    /// Compiling the base of a member access chain.
    pub(crate) in_chain: bool,
    /// Compiling the value of a `const` statement.
    pub(crate) in_const_stmt: bool,
    /// Pending `?->` short-circuit jumps of the current chain.
    pub(crate) nullsafe_jumps: Vec<(usize, Reg)>,
    /// Source ranges left uncompiled (the right operand of `false && x`):
    /// parser notices in them are not reported.
    pub(crate) dead_spans: Vec<(u32, u32)>,
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
        declared_classes: Default::default(),
        use_func: HashMap::new(),
        use_const: HashMap::new(),
        class: None,
        strict: false,
        f: FState::new(sc.vars),
        notices: Vec::new(),
        anon_count: 0,
        src,
        known_const,
        ticks: 0,
        past_first_stmt: false,
        zend_multibyte: ZEND_MULTIBYTE.with(|c| c.get()),
        in_const_expr: false,
        in_chain: false,
        in_const_stmt: false,
        nullsafe_jumps: Vec::new(),
        dead_spans: Vec::new(),
    };
    if let Some((cls, parent)) = eval_class {
        c.class = Some(ClassCtx {
            name: cls,
            parent,
            is_trait: false,
        });
    }
    if let Some(e) = prog.compile_errors.first() {
        return Err(CompileError {
            message: e.message.clone(),
            line: e.line,
            notices: Vec::new(),
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
    if hrh::mentioned(src) {
        for l in hrh::lines_stmts(prog.stmts, false) {
            c.notice(goro_runtime::Level::Deprecated, hrh::MESSAGE, l);
        }
    }
    c.f.flags |= fflags::PSEUDO_MAIN;
    c.f.line = 1;
    let mut hoisted = Vec::new();
    let mut classes = Vec::new();
    let r = (|| -> CResult<()> {
        c.check_strict_types(prog.stmts)?;
        c.check_namespaces(prog.stmts)?;
        c.top_stmts(prog.stmts, &mut hoisted, &mut classes)
    })();
    // Code PHP does not compile reports no deprecations (observed on PHP
    // 8.5: `assert(0 && ...)` in Zend/tests/assert/expect_015.phpt).
    if !c.dead_spans.is_empty() {
        let dead = std::mem::take(&mut c.dead_spans);
        let mut i = 0;
        let parser = prog.notices.len();
        c.notices.retain(|_| {
            let keep = i >= parser || {
                let o = prog.notices[i].offset;
                !dead.iter().any(|&(s, e)| o >= s && o < e)
            };
            i += 1;
            keep
        });
    }
    hrh::order(&mut c.notices);
    if let Err(mut e) = r {
        e.notices = std::mem::take(&mut c.notices);
        return Err(e);
    }
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
            notices: Vec::new(),
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
    pub(crate) fn resolve_class(&self, n: &Name<'_>) -> Vec<u8> {
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
    pub(crate) fn resolve_func(&self, n: &Name<'_>) -> (Vec<u8>, Option<Vec<u8>>) {
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

    pub(crate) fn resolve_const_name(&self, n: &Name<'_>) -> (Vec<u8>, Option<Vec<u8>>) {
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
        if params.iter().any(|p| p.ty.is_some()) {
            flags |= fflags::TYPED_PARAMS;
        }
        let byref_mask = crate::bytecode::byref_mask(&params);
        let direct_params = params.iter().filter(|p| !p.variadic).count() as u32;
        Ok(FuncCode {
            in_namespace: !self.ns.is_empty(),
            byref_mask,
            direct_params,
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
            const_attrs: std::mem::take(&mut f.const_attrs),
            discarded_calls: std::mem::take(&mut f.discarded_calls),
            caches,
            cv_map: std::cell::RefCell::new(None),
            statics: std::cell::RefCell::new(vec![None; f.num_statics as usize]),
        })
    }

    // ------------------------------------------------------------ statements

    /// Namespace placement rules (messages observed on PHP 8.5).
    fn check_namespaces(&mut self, stmts: &'a [Stmt<'a>]) -> CResult<()> {
        let mut style: Option<bool> = None;
        let mut code_before = false;
        let mut first = true;
        for s in stmts {
            match &s.kind {
                StmtKind::Namespace { body, .. } => {
                    let bracketed = body.is_some();
                    if let Some(b) = style
                        && b != bracketed
                    {
                        return self.err(
                            "Cannot mix bracketed namespace declarations with unbracketed namespace declarations",
                            s.span.line,
                        );
                    }
                    if code_before && style.is_none() {
                        return self.err(
                            "Namespace declaration statement has to be the very first statement or after any declare call in the script",
                            s.span.line,
                        );
                    }
                    style = Some(bracketed);
                }
                StmtKind::Declare(_) | StmtKind::Nop | StmtKind::HaltCompiler { .. } => {}
                StmtKind::InlineHtml(t) if first && is_shebang_or_empty(t) => {}
                _ => {
                    if style == Some(true) {
                        return self.err("No code may exist outside of namespace {}", s.span.line);
                    }
                    code_before = true;
                }
            }
            first = false;
        }
        if style == Some(true) && code_before {
            // Code before the first bracketed namespace.
            for s in stmts {
                match &s.kind {
                    StmtKind::Namespace { .. } => break,
                    StmtKind::Declare(_) | StmtKind::Nop | StmtKind::HaltCompiler { .. } => {}
                    StmtKind::InlineHtml(t) if is_shebang_or_empty(t) => {}
                    _ => {
                        return self.err(
                            "Namespace declaration statement has to be the very first statement or after any declare call in the script",
                            s.span.line,
                        );
                    }
                }
            }
        }
        Ok(())
    }

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

    /// A function can't take the name of a `use function` import (test
    /// Zend/tests/name_collision/name_collision_08).
    fn func_import_check(&mut self, fd: &'a FunctionDecl<'a>, line: u32) -> CResult<()> {
        let full = self.qualify(fd.name.name);
        if let Some(imp) = self.use_func.get(&fd.name.name.to_ascii_lowercase())
            && !imp.eq_ignore_ascii_case(&full)
        {
            return self.err(
                format!(
                    "Cannot redeclare function {}() (previously declared as local import)",
                    String::from_utf8_lossy(&full)
                ),
                line,
            );
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
                    self.func_import_check(fd, s.span.line)?;
                    let idx = self.func_decl(fd)?;
                    hoisted.push(idx);
                }
                StmtKind::Class(cd) if cd.name.is_some() => {
                    self.f.line = s.span.line;
                    if !self.file.is_empty() && needs_rtd_key(cd) {
                        next_rtd_key();
                    }
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
                    let t = self.declare_directives(d, s.span.line)?;
                    match d.body {
                        DeclareBody::None => {
                            if let Some(t) = t {
                                self.ticks = t;
                            }
                        }
                        DeclareBody::Block(b) | DeclareBody::Alt(b) => {
                            let saved = self.ticks;
                            if let Some(t) = t {
                                self.ticks = t;
                            }
                            // Statements of a declare block do not count as
                            // statements before a later encoding declare
                            // (test ext/mbstring/tests/zend_multibyte-15).
                            let first = self.past_first_stmt;
                            let r = self.top_stmts(b, hoisted, classes);
                            self.past_first_stmt = first;
                            self.ticks = saved;
                            r?
                        }
                    }
                }
                StmtKind::HaltCompiler { .. } => break,
                _ => {
                    self.past_first_stmt = true;
                    self.stmt(s)?
                }
            }
        }
        Ok(())
    }

    /// Applies declare() directives; returns the `ticks` value they set.
    fn declare_directives(&mut self, d: &'a DeclareStmt<'a>, line: u32) -> CResult<Option<u32>> {
        let mut ticks = None;
        for (k, v) in d.directives {
            if k.name.eq_ignore_ascii_case(b"ticks") {
                // Messages observed on PHP 8.5.
                match v.kind {
                    ExprKind::Int(n) => ticks = Some(n.max(0) as u32),
                    _ => return self.err("declare(ticks) value must be a literal", line),
                }
            } else if k.name.eq_ignore_ascii_case(b"encoding") {
                match &v.kind {
                    ExprKind::String(..) => {
                        if !self.zend_multibyte {
                            self.notice(
                                goro_runtime::Level::Warning,
                                "declare(encoding=...) ignored because Zend multibyte feature is turned off by settings".to_string(),
                                line,
                            );
                        }
                        if self.past_first_stmt || self.f.flags & fflags::PSEUDO_MAIN == 0 {
                            return self.err(
                                "Encoding declaration pragma must be the very first statement in the script",
                                line,
                            );
                        }
                    }
                    ExprKind::Int(n) => self.notice(
                        goro_runtime::Level::Warning,
                        format!("Unsupported encoding [{}]", n),
                        line,
                    ),
                    ExprKind::Float(f) => self.notice(
                        goro_runtime::Level::Warning,
                        format!(
                            "Unsupported encoding [{}]",
                            goro_runtime::float_fmt::to_php_string(*f, 14)
                        ),
                        line,
                    ),
                    _ => return self.err("Encoding must be a literal", line),
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
        Ok(ticks)
    }

    pub(crate) fn block(&mut self, b: Block<'a>) -> CResult<()> {
        for s in b {
            self.stmt(s)?;
        }
        Ok(())
    }

    pub(crate) fn stmt(&mut self, s: &'a Stmt<'a>) -> CResult<()> {
        self.stmt_inner(s)?;
        if self.ticks > 0
            && !matches!(
                s.kind,
                StmtKind::Function(_)
                    | StmtKind::Class(_)
                    | StmtKind::Nop
                    | StmtKind::Declare(_)
                    | StmtKind::Block(_)
                    | StmtKind::Label(_)
            )
        {
            let every = self.ticks;
            self.emit(Op::Tick { every });
        }
        Ok(())
    }

    fn stmt_inner(&mut self, s: &'a Stmt<'a>) -> CResult<()> {
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
                    // Test tests/lang/static_basic_002.
                    if self.f.static_names.iter().any(|n| n == v.name.name) {
                        return self.err(
                            format!(
                                "Duplicate declaration of static variable ${}",
                                String::from_utf8_lossy(v.name.name)
                            ),
                            v.span.line,
                        );
                    }
                    self.f.static_names.push(v.name.name.to_vec());
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
                let t = self.declare_directives(d, s.span.line)?;
                match d.body {
                    DeclareBody::None => {
                        if let Some(t) = t {
                            self.ticks = t;
                        }
                    }
                    DeclareBody::Block(b) | DeclareBody::Alt(b) => {
                        let saved = self.ticks;
                        if let Some(t) = t {
                            self.ticks = t;
                        }
                        let r = self.block(b);
                        self.ticks = saved;
                        r?
                    }
                }
                return Ok(());
            }
            StmtKind::Function(fd) => {
                self.func_import_check(fd, s.span.line)?;
                let idx = self.func_decl(fd)?;
                self.emit(Op::DeclareFunction { func: idx });
            }
            StmtKind::Class(cd) => {
                if cd.name.is_some() && !self.file.is_empty() {
                    next_rtd_key();
                }
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
            StmtKind::Const { items, attrs } => {
                if !attrs.is_empty() {
                    // PHP 8.5 (RFC: Attributes on Constants).
                    if items.len() > 1 {
                        return self.err(
                            "Cannot apply attributes to multiple constants at once",
                            s.span.line,
                        );
                    }
                    let a = self.attributes(attrs)?;
                    self.check_internal_attrs(&a, 64, s.span.line)?;
                    let full = self.qualify(items[0].name.name);
                    self.f
                        .const_attrs
                        .push((PhpStr::from(full), s.span.line, a));
                }
                for c in *items {
                    // A constant expression (PHP manual: Constants; tests
                    // Zend/tests/first_class_callable/constexpr/error_*).
                    self.check_const_expr(&c.value)?;
                    let full = self.qualify(c.name.name);
                    // Test Zend/tests/name_collision/name_collision_09.
                    if let Some(imp) = self.use_const.get(c.name.name)
                        && *imp != full
                    {
                        return self.err(
                            format!(
                                "Cannot declare const {} because the name is already in use",
                                String::from_utf8_lossy(&full)
                            ),
                            s.span.line,
                        );
                    }
                    let name = self.str_const(&full);
                    self.in_const_stmt = true;
                    let v = self.expr(&c.value);
                    self.in_const_stmt = false;
                    let v = v?;
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
        // `use Foo;` in the global namespace (test Zend/tests/bug69388).
        if kind == UseKind::Normal
            && self.ns.is_empty()
            && prefix.is_none()
            && u.alias.is_none()
            && !full.contains(&b'\\')
        {
            self.notice(
                goro_runtime::Level::Warning,
                format!(
                    "The use statement with non-compound name '{}' has no effect",
                    String::from_utf8_lossy(&full)
                ),
                line,
            );
        }
        match kind {
            UseKind::Normal => {
                let la = alias.to_ascii_lowercase();
                // tests Zend/tests/type_declarations/scalar_reserved*_use
                if matches!(&la[..], b"self" | b"parent" | b"static")
                    || decl::is_reserved_type_name_pub(&la)
                {
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
                let mut local = self.ns.clone();
                if !local.is_empty() {
                    local.push(b'\\');
                }
                local.extend_from_slice(&alias);
                if self.declared_classes.contains(&local.to_ascii_lowercase())
                    && !local.eq_ignore_ascii_case(&full)
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
        // Test tests/lang/foreachLoop.006 (PHP's grammar accepts the form,
        // the compiler rejects it).
        if f.key_by_ref
            && let Some(k) = &f.key
        {
            return self.err("Key element cannot be a reference", k.span.line);
        }
        let iter = self.hidden_pair();
        // `as [&$x, $y]` iterates by reference (test Zend/tests/list/
        // list_reference_004).
        fn list_has_ref(e: &Expr<'_>) -> bool {
            match &e.kind {
                ExprKind::List { items, .. } | ExprKind::Array { items, .. } => items
                    .iter()
                    .flatten()
                    .any(|i| i.by_ref || list_has_ref(&i.value)),
                _ => false,
            }
        }
        let by_ref = f.by_ref || list_has_ref(&f.value);
        // Tests Zend/tests/restrict_globals/invalid_foreach*.
        if matches!(f.value.kind, ExprKind::Variable(b"GLOBALS")) {
            return self.err(
                "$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax",
                line,
            );
        }
        // A `?->` chain is iterated as a value (test Zend/tests/
        // nullsafe_operator/023).
        let src = if by_ref
            && !expr::contains_nullsafe(&f.expr)
            && !matches!(f.expr.kind, ExprKind::Variable(b"this"))
        {
            self.ref_of(&f.expr)?
        } else {
            self.expr(&f.expr)?
        };
        self.free(src);
        self.f.line = line;
        let reset = self.emit(Op::FeReset {
            iter,
            src,
            by_ref,
            t: 0,
        });
        let top = self.here();
        // Value target.
        let (val_dst, direct_val) = match &f.value.kind {
            ExprKind::Variable(n) if !by_ref && *n != b"this" => (self.cv(n), true),
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
            if by_ref {
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
        let region_len = self.f.regions.len();
        self.f.region_ids += 1;
        self.f.regions.push(Region::Loop(self.f.region_ids, free));
        self.f.loops.push(LoopCtx {
            breaks: Vec::new(),
            continues: Vec::new(),
            is_switch,
            free,
            region_len,
            start: self.f.ops.len() as u32,
        });
    }

    /// Patches a goto: directly, or through a stub that frees the loop
    /// temporaries and runs the finally blocks of the blocks it leaves.
    fn resolve_goto(
        &mut self,
        at: usize,
        line: u32,
        target: u32,
        g: &[Region],
        l: &[Region],
    ) -> CResult<()> {
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
        if self.f.regions[rl..]
            .iter()
            .any(|r| matches!(r, Region::Finally(_)))
        {
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
        // `return;` with a return type (tests Zend/tests/type_declarations/
        // typed_return_*without_value).
        if e.is_none()
            && self.f.flags & fflags::HAS_RETURN_TYPE != 0
            && self.f.flags & fflags::RETURNS_VOID == 0
            && !self.f.is_generator
        {
            return self.err(
                if self.f.ret_allows_null {
                    "A function with return type must return a value (did you mean \"return null;\" instead of \"return;\"?)"
                } else {
                    "A function with return type must return a value"
                },
                line,
            );
        }
        if let Some(e) = e
            && by_ref
            && !self.f.is_generator
        {
            self.nullsafe_ref_check(e)?;
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
            if self.f.flags & fflags::BY_REF != 0 {
                // A by-reference function returning no variable: "Only
                // variable references should be returned by reference"
                // (test Zend/tests/return_ref_none.phpt).
                self.emit(Op::ReturnRef { src: val });
            } else {
                self.emit(Op::Return { src: val });
            }
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
                    if n.kind == NameKind::Unqualified && n.name.eq_ignore_ascii_case(b"static") {
                        return self.err("Bad class name in the catch statement", c.span.line);
                    }
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

thread_local! {
    /// Runtime declaration keys handed out so far in this request (across
    /// files and eval): anonymous classes are numbered with them.
    static RTD_KEYS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The next runtime declaration key. As observed on PHP 8.5, anonymous
/// classes take one, and so do class declarations that are not bound at
/// compile time (conditional ones, and ones with interfaces, traits or
/// enums); `class@anonymous` names end in `$key`.
pub(crate) fn next_rtd_key() -> u32 {
    RTD_KEYS.with(|k| {
        let v = k.get();
        k.set(v + 1);
        v
    })
}

/// Whether a top-level class declaration is bound at run time.
fn needs_rtd_key(cd: &goro_syntax::ast::ClassDecl) -> bool {
    cd.kind == goro_syntax::ast::ClassKind::Enum
        || !cd.implements.is_empty()
        || cd
            .members
            .iter()
            .any(|m| matches!(m, goro_syntax::ast::ClassMember::TraitUse(_)))
}

thread_local! {
    /// The zend.multibyte setting for the next compilation.
    pub static ZEND_MULTIBYTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// zend.multibyte: the encoding string literals are converted to from
    /// UTF-8 (the internal encoding).
    pub static LITERAL_ENCODING: std::cell::Cell<Option<goro_charset::Encoding>> =
        const { std::cell::Cell::new(None) };
}
