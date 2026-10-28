//! The bytecode: a register machine over per-frame slots.
//!
//! Each compiled function ([`FuncCode`]) owns its instructions, a constant
//! pool and a slot layout: the named variables (compiled variables, "CVs")
//! come first, in order of first appearance, then hidden variables and
//! temporaries. Variable names are resolved to slot indices at compile
//! time, so running code never hashes a variable name.
//!
//! An operand ([`Reg`]) is a 32-bit word:
//! - `CONST | i`: constant `i` of the pool,
//! - `TMP | s`: slot `s`, a temporary that is *consumed* by its single
//!   reader (moved out, so values are released as soon as PHP releases
//!   them, which keeps destructor timing exact),
//! - `s`: slot `s`, a variable (read by copy).
//!
//! Lines of every instruction are kept in a parallel vector, read only for
//! diagnostics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_runtime::{PhpStr, Value};

pub type Reg = u32;
pub const CONST: u32 = 1 << 31;
pub const TMP: u32 = 1 << 30;
pub const SLOT_MASK: u32 = TMP - 1;
/// No operand (an unused result, a missing key...).
pub const NONE: Reg = u32::MAX;
/// The object operand of property and method instructions meaning `$this`.
pub const THIS: Reg = u32::MAX - 1;

#[inline(always)]
pub fn is_const(r: Reg) -> bool {
    r & CONST != 0
}
#[inline(always)]
pub fn is_tmp(r: Reg) -> bool {
    r & (CONST | TMP) == TMP
}
#[inline(always)]
pub fn slot_of(r: Reg) -> usize {
    (r & SLOT_MASK) as usize
}

/// Binary operators (also used by compound assignment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Identical,
    NotIdentical,
    Lt,
    Le,
    Spaceship,
    /// `??=` (only in compound assignment)
    Coalesce,
}

impl Bin {
    pub fn symbol(self) -> &'static str {
        match self {
            Bin::Add => "+",
            Bin::Sub => "-",
            Bin::Mul => "*",
            Bin::Div => "/",
            Bin::Mod => "%",
            Bin::Pow => "**",
            Bin::Concat => ".",
            Bin::BitAnd => "&",
            Bin::BitOr => "|",
            Bin::BitXor => "^",
            Bin::Shl => "<<",
            Bin::Shr => ">>",
            Bin::Eq => "==",
            Bin::Ne => "!=",
            Bin::Identical => "===",
            Bin::NotIdentical => "!==",
            Bin::Lt => "<",
            Bin::Le => "<=",
            Bin::Spaceship => "<=>",
            Bin::Coalesce => "??",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Un {
    Not,
    BitNot,
    Neg,
    Plus,
    Bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CastTo {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IncDec {
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

impl IncDec {
    pub fn is_inc(self) -> bool {
        matches!(self, IncDec::PreInc | IncDec::PostInc)
    }
    pub fn is_post(self) -> bool {
        matches!(self, IncDec::PostInc | IncDec::PostDec)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
}

/// What a magic constant evaluated at run time refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Special {
    /// `static::class`
    StaticClass,
    /// `self::class` in a trait or closure (resolved with the scope).
    SelfClass,
    ParentClass,
    /// `__CLASS__` inside a trait.
    TraitClass,
    /// `func_get_args()`
    FuncGetArgs,
    FuncNumArgs,
    /// `get_called_class()`
    CalledClass,
    /// `$this` as a value.
    This,
    /// `$this` or null (isset($this)).
    ThisQuiet,
    /// `$GLOBALS` as a value (read-only copy).
    Globals,
}

/// A class named by an instruction: a constant name, `self`, `parent`,
/// `static`, or a run-time value (`$obj::X`, `new $cls`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassSpec {
    /// Constant index of the fully qualified name (and of its lower case
    /// form at index + 1).
    Named(u32),
    SelfRef,
    Parent,
    Static,
    /// A value: class name string or object.
    Dynamic(Reg),
}

/// One step of a write path (`$a[k]`, `$a[]`, `$a->p`).
#[derive(Clone, Copy, Debug)]
pub enum PathElem {
    Dim(Reg),
    Append,
    /// Property: name operand and an inline cache slot.
    Prop(Reg, u32),
}

/// The root of a write path.
#[derive(Clone, Copy, Debug)]
pub enum PathBase {
    /// A variable slot (CV or hidden).
    Var(Reg),
    /// `$this`
    This,
    /// A temporary holding a value (`f()->p = 1`, `(clone $x)->p = 1`).
    Tmp(Reg),
    /// `$$name`: variable looked up by name.
    VarVar(Reg),
    /// `A::$p`
    StaticProp(ClassSpec, Reg),
    /// A global variable by name (superglobals, `$GLOBALS['x']`).
    Global(Reg),
}

/// What a write path does at its end.
#[derive(Clone, Copy, Debug)]
pub enum PathAction {
    Assign(Reg),
    AssignOp(Bin, Reg),
    IncDec(IncDec),
    Unset,
    /// Make the target a reference and put the reference in the result.
    MakeRef,
    /// Bind the target to the reference held by the operand (`=&`).
    BindRef(Reg),
    /// Fetch the target for writing as a container value (used by
    /// `foreach` by reference over `$a[k]` and nested list()).
    FetchW,
}

#[derive(Clone, Debug)]
pub struct WritePath {
    pub base: PathBase,
    pub elems: Box<[PathElem]>,
    pub action: PathAction,
}

/// Instruction.
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Nop,
    // ----- moves
    /// `$cv = src` (through references; drops the old value).
    Assign {
        dst: Reg,
        src: Reg,
    },
    /// `res = ($cv = src)`
    AssignRes {
        dst: Reg,
        src: Reg,
        res: Reg,
    },
    /// `tmp = src` (copy, dereferenced).
    Copy {
        dst: Reg,
        src: Reg,
    },
    /// `$dst = &$src` between variables.
    AssignRef {
        dst: Reg,
        src: Reg,
    },
    /// Binds variable `dst` to the reference in operand `src` (which holds a
    /// `Value::Ref`).
    BindRef {
        dst: Reg,
        src: Reg,
    },
    /// `unset($cv)`
    UnsetVar {
        var: Reg,
    },
    /// Clears a hidden slot (end of a foreach, match...).
    Free {
        slot: Reg,
    },
    // ----- operators
    Add {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Sub {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Mul {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Concat {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Lt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Le {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Eq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Identical {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// Other binary operators.
    BinOp {
        op: Bin,
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Unary {
        op: Un,
        dst: Reg,
        a: Reg,
    },
    Cast {
        to: CastTo,
        dst: Reg,
        a: Reg,
    },
    /// Concatenation of `n` operands listed in `lists[list]`.
    Rope {
        dst: Reg,
        list: u32,
    },
    /// `$cv op= src`
    AssignOpVar {
        op: Bin,
        var: Reg,
        src: Reg,
        res: Reg,
    },
    /// `++$cv` and friends
    IncDecVar {
        op: IncDec,
        var: Reg,
        res: Reg,
    },
    // ----- control flow
    Jmp {
        t: u32,
    },
    JmpZ {
        c: Reg,
        t: u32,
    },
    JmpNZ {
        c: Reg,
        t: u32,
    },
    /// Jumps if `a < b` / `a <= b` (loops), operands not consumed.
    JmpLt {
        a: Reg,
        b: Reg,
        t: u32,
    },
    /// `res = (bool)c`, then jump if it is false / true (`&&`, `||`).
    JmpZEx {
        c: Reg,
        res: Reg,
        t: u32,
    },
    JmpNZEx {
        c: Reg,
        res: Reg,
        t: u32,
    },
    /// Jumps if the operand is set and not null (`??`): copies it to `res`.
    JmpSet {
        c: Reg,
        res: Reg,
        t: u32,
    },
    /// Jumps if the operand is null (nullsafe `?->`), setting `res` to null.
    JmpNull {
        c: Reg,
        res: Reg,
        t: u32,
    },
    /// `switch`/`match` by jump table: `tables[table]` maps keys to
    /// targets; `default` otherwise. `strict` for match.
    Switch {
        c: Reg,
        table: u32,
    },
    Echo {
        src: Reg,
    },
    Return {
        src: Reg,
    },
    /// Return from a by-reference function: `src` holds a reference.
    ReturnRef {
        src: Reg,
    },
    /// Evaluate the default of parameter `param` (jumps to `t` if the
    /// argument was passed).
    RecvDefault {
        param: u32,
        t: u32,
    },
    /// Check / coerce parameter `param` after its default was set.
    RecvCheck {
        param: u32,
    },
    /// Collect variadic arguments into parameter slot.
    RecvVariadic {
        param: u32,
    },
    /// Check the return value against the declared type.
    VerifyReturn {
        src: Reg,
        implicit: bool,
    },
    // ----- arrays
    NewArray {
        dst: Reg,
        cap: u32,
    },
    /// Append (key NONE) or insert into the temporary array `arr`.
    AddElem {
        arr: Reg,
        key: Reg,
        val: Reg,
    },
    /// Add an element by reference: `val` holds a reference.
    AddElemRef {
        arr: Reg,
        key: Reg,
        val: Reg,
    },
    /// `...$x` inside an array literal.
    AddSpread {
        arr: Reg,
        src: Reg,
    },
    /// `dst = base[key]` (read).
    FetchDim {
        dst: Reg,
        base: Reg,
        key: Reg,
    },
    /// `dst = base[key]` without warnings (isset chains, `??`).
    FetchDimIs {
        dst: Reg,
        base: Reg,
        key: Reg,
    },
    /// `dst = isset(base[key])` / `empty(...)` when `empty`.
    IssetDim {
        dst: Reg,
        base: Reg,
        key: Reg,
        empty: bool,
    },
    /// `list()` element: `dst = base[key]` with list semantics.
    FetchList {
        dst: Reg,
        base: Reg,
        key: Reg,
    },
    /// `$cv[key] = val` (key NONE: append).
    AssignDim {
        var: Reg,
        key: Reg,
        val: Reg,
        res: Reg,
    },
    /// Generic write path `paths[path]`, result in `res`.
    Write {
        path: u32,
        res: Reg,
    },
    // ----- objects and classes
    FetchProp {
        dst: Reg,
        obj: Reg,
        name: Reg,
        cache: u32,
    },
    FetchPropIs {
        dst: Reg,
        obj: Reg,
        name: Reg,
        cache: u32,
    },
    IssetProp {
        dst: Reg,
        obj: Reg,
        name: Reg,
        empty: bool,
    },
    AssignProp {
        obj: Reg,
        name: Reg,
        val: Reg,
        cache: u32,
    },
    FetchStaticProp {
        dst: Reg,
        class: ClassSpec,
        name: Reg,
    },
    IssetStaticProp {
        dst: Reg,
        class: ClassSpec,
        name: Reg,
        empty: bool,
    },
    ClassConst {
        dst: Reg,
        class: ClassSpec,
        name: Reg,
        cache: u32,
    },
    /// `X::class` on a value or special class.
    ClassName {
        dst: Reg,
        class: ClassSpec,
    },
    InstanceOf {
        dst: Reg,
        a: Reg,
        class: ClassSpec,
    },
    /// `new X`: creates the object and prepares the constructor call (a
    /// pending call; `DoCall` follows after the arguments).
    New {
        dst: Reg,
        class: ClassSpec,
        nargs: u32,
        /// Inline cache of the resolved class (named classes).
        cache: u32,
    },
    /// Declares anonymous class template `class` and instantiates it.
    NewAnon {
        dst: Reg,
        class: u32,
        nargs: u32,
    },
    Clone {
        dst: Reg,
        src: Reg,
    },
    /// `clone($x, $withArray)`
    CloneWith {
        dst: Reg,
        src: Reg,
        with: Reg,
    },
    // ----- calls
    /// Call of a function by name: constant `name` (FQ name, then lower
    /// case at name+1; for namespaced unqualified calls the global fallback
    /// is at name+2 and lower case at name+3 and `fallback` is true).
    InitFCall {
        name: u32,
        nargs: u32,
        cache: u32,
        fallback: bool,
    },
    /// Call of a callable value.
    InitDynCall {
        callee: Reg,
        nargs: u32,
    },
    InitMethod {
        obj: Reg,
        name: Reg,
        nargs: u32,
        cache: u32,
    },
    /// Nullsafe method call: jumps to `t` if the object is null.
    InitStaticCall {
        class: ClassSpec,
        name: Reg,
        nargs: u32,
        cache: u32,
    },
    Send {
        src: Reg,
        arg: u32,
    },
    /// Send a variable: by reference if the parameter is by-reference.
    SendVar {
        var: Reg,
        arg: u32,
    },
    /// Send a reference (the operand holds a `Value::Ref`).
    SendRef {
        src: Reg,
        arg: u32,
    },
    /// Send a value to a by-reference parameter (notice "could not be
    /// passed by reference" for function results).
    SendValRef {
        src: Reg,
        arg: u32,
    },
    SendUnpack {
        src: Reg,
    },
    SendNamed {
        src: Reg,
        name: Reg,
    },
    SendNamedVar {
        var: Reg,
        name: Reg,
    },
    /// Jumps to `t` if argument `arg` of the pending call is by reference.
    JmpArgByRef {
        arg: u32,
        t: u32,
    },
    /// Jumps to `t` if the named argument is by reference.
    JmpNamedArgByRef {
        name: Reg,
        t: u32,
    },
    DoCall {
        dst: Reg,
    },
    /// `f(...)`: turns the pending call into a Closure.
    CallableConvert {
        dst: Reg,
    },
    // ----- declarations
    DeclareFunction {
        func: u32,
    },
    DeclareClass {
        class: u32,
    },
    DeclareConst {
        name: Reg,
        val: Reg,
    },
    MakeClosure {
        dst: Reg,
        func: u32,
    },
    /// Bind a static variable: `static $x = init` (`init` NONE: null;
    /// otherwise the initialiser code is at the next instruction and runs
    /// once, ending with `StaticInit`).
    BindStatic {
        var: Reg,
        index: u32,
        t: u32,
    },
    StaticInit {
        index: u32,
        src: Reg,
    },
    BindGlobal {
        var: Reg,
        name: Reg,
    },
    // ----- variables by name
    /// A global variable by name (superglobals, `$GLOBALS[...]`).
    FetchGlobal {
        dst: Reg,
        name: Reg,
        quiet: bool,
    },
    FetchVarVar {
        dst: Reg,
        name: Reg,
        quiet: bool,
    },
    IssetVarVar {
        dst: Reg,
        name: Reg,
        empty: bool,
    },
    AssignVarVar {
        name: Reg,
        val: Reg,
        res: Reg,
    },
    UnsetVarVar {
        name: Reg,
    },
    /// Fetch a variable as a reference (by-ref arguments, `=&`).
    RefVar {
        dst: Reg,
        var: Reg,
    },
    IssetVar {
        dst: Reg,
        var: Reg,
        empty: bool,
    },
    // ----- constants
    FetchConst {
        dst: Reg,
        name: u32,
        cache: u32,
        fallback: bool,
    },
    Special {
        dst: Reg,
        what: Special,
    },
    // ----- exceptions
    Throw {
        src: Reg,
    },
    /// Exception handler entry: checks the current exception against
    /// `classes` (constant indices of names, list in `lists`), binds it to
    /// `var` (NONE: no variable), or jumps to `next`.
    Catch {
        list: u32,
        var: Reg,
        next: u32,
    },
    /// Re-throws the exception being handled (no catch matched).
    Rethrow,
    /// Enter a finally block: `ret` remembers where to continue.
    FastCall {
        t: u32,
        ret: Reg,
    },
    FastRet {
        ret: Reg,
        try_index: u32,
    },
    /// A jump out of a finally block discards the pending exception.
    DiscardException {
        ret: Reg,
    },
    // ----- iteration
    /// Prepare `foreach` over `src` into iterator slot `iter`; jumps to
    /// `t` when there is nothing to iterate.
    FeReset {
        iter: Reg,
        src: Reg,
        by_ref: bool,
        t: u32,
    },
    /// Next element: value to `val`, key to `key` (NONE: unused); jumps
    /// to `t` at the end.
    FeFetch {
        iter: Reg,
        val: Reg,
        key: Reg,
        t: u32,
    },
    FeFree {
        iter: Reg,
    },
    // ----- generators
    Yield {
        dst: Reg,
        val: Reg,
        key: Reg,
    },
    YieldFrom {
        dst: Reg,
        src: Reg,
    },
    GeneratorReturn {
        src: Reg,
    },
    // ----- misc
    Include {
        dst: Reg,
        src: Reg,
        kind: IncludeKind,
    },
    Eval {
        dst: Reg,
        src: Reg,
    },
    Exit {
        src: Reg,
    },
    BeginSilence {
        save: Reg,
    },
    EndSilence {
        save: Reg,
    },
    Isset {
        dst: Reg,
        src: Reg,
    },
    /// Raise a compile-time-detected run-time error.
    Error {
        msg: u32,
        kind: u8,
    },
    /// `match` without a matching arm.
    MatchError {
        src: Reg,
    },
    /// Checks `$x instanceof Countable`-like helpers for builtins compiled
    /// inline: `strlen`, `count`...
    Strlen {
        dst: Reg,
        src: Reg,
    },
    Count {
        dst: Reg,
        src: Reg,
    },
    /// Pipe `|>` with a callable value: `dst = callee(arg)`.
    TypeCheck {
        dst: Reg,
        src: Reg,
        ty: u32,
    },
    /// Line marker for ticks / statements (unused for now).
    Ticks,
}

/// Default value of a parameter.
#[derive(Clone, Debug)]
pub enum ParamDefault {
    None,
    /// A constant value.
    Const(Value),
    /// Evaluated by code in the function prologue.
    Code,
    /// Evaluated when needed (native functions, Reflection).
    Lazy(ConstExpr),
}

/// Compiled type declaration.
#[derive(Clone, Debug, Default)]
pub struct TypeHint {
    pub mask: u32,
    /// Class names (fully qualified; `self`/`parent` kept as names).
    /// An inner vector with several names is an intersection.
    pub classes: Vec<Vec<PhpStr>>,
}

pub mod tmask {
    pub const NULL: u32 = 1;
    pub const FALSE: u32 = 2;
    pub const TRUE: u32 = 4;
    pub const BOOL: u32 = FALSE | TRUE;
    pub const INT: u32 = 8;
    pub const FLOAT: u32 = 16;
    pub const STRING: u32 = 32;
    pub const ARRAY: u32 = 64;
    pub const OBJECT: u32 = 128;
    pub const CALLABLE: u32 = 256;
    pub const ITERABLE: u32 = 512;
    pub const MIXED: u32 = 1024;
    pub const VOID: u32 = 2048;
    pub const NEVER: u32 = 4096;
    pub const STATIC: u32 = 8192;
    pub const RESOURCE: u32 = 16384;
}

impl TypeHint {
    pub fn allows_null(&self) -> bool {
        self.mask & (tmask::NULL | tmask::MIXED) != 0
    }
    pub fn is_simple_class(&self) -> bool {
        self.mask & !tmask::NULL == 0 && self.classes.len() == 1 && self.classes[0].len() == 1
    }

    /// The type as PHP prints it in messages.
    pub fn display(&self) -> String {
        use tmask::*;
        let mut parts: Vec<String> = Vec::new();
        let nullable_short = self.mask & NULL != 0
            && self.mask & MIXED == 0
            && ((self.classes.len() == 1 && self.classes[0].len() == 1 && self.mask & !NULL == 0)
                || (self.classes.is_empty() && (self.mask & !NULL).count_ones() == 1)
                || (self.classes.is_empty() && self.mask & !NULL == BOOL));
        for c in &self.classes {
            if c.len() == 1 {
                parts.push(c[0].to_string_lossy());
            } else {
                let inner: Vec<String> = c.iter().map(|s| s.to_string_lossy()).collect();
                if self.classes.len() > 1 || self.mask != 0 {
                    parts.push(format!("({})", inner.join("&")));
                } else {
                    parts.push(inner.join("&"));
                }
            }
        }
        let m = self.mask;
        if m & MIXED != 0 {
            parts.push("mixed".into());
        }
        if m & STATIC != 0 {
            parts.push("static".into());
        }
        if m & CALLABLE != 0 {
            parts.push("callable".into());
        }
        if m & ITERABLE != 0 {
            // PHP 8.2 shows iterable as Traversable|array
            parts.push("Traversable".into());
            if m & ARRAY == 0 {
                parts.push("array".into());
            }
        }
        if m & OBJECT != 0 {
            parts.push("object".into());
        }
        if m & ARRAY != 0 {
            parts.push("array".into());
        }
        if m & STRING != 0 {
            parts.push("string".into());
        }
        if m & INT != 0 {
            parts.push("int".into());
        }
        if m & FLOAT != 0 {
            parts.push("float".into());
        }
        if m & BOOL == BOOL {
            parts.push("bool".into());
        } else if m & FALSE != 0 {
            parts.push("false".into());
        } else if m & TRUE != 0 {
            parts.push("true".into());
        }
        if m & VOID != 0 {
            parts.push("void".into());
        }
        if m & NEVER != 0 {
            parts.push("never".into());
        }
        if m & RESOURCE != 0 {
            parts.push("resource".into());
        }
        if nullable_short && parts.len() == 1 {
            return format!("?{}", parts[0]);
        }
        if m & NULL != 0 && m & MIXED == 0 {
            parts.push("null".into());
        }
        parts.join("|")
    }
}

#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub name: PhpStr,
    pub by_ref: bool,
    pub variadic: bool,
    pub ty: Option<TypeHint>,
    pub default: ParamDefault,
    /// Source text of the default value (Reflection, messages).
    pub default_text: Option<String>,
    /// Constructor promotion: property modifiers.
    pub promote: Option<u16>,
    pub attrs: Rc<[AttrInfo]>,
    /// The default as a constant expression when it is computed by the
    /// prologue (for Reflection).
    pub default_ce: Option<ConstExpr>,
}

/// A compiled attribute: name and argument evaluation code.
#[derive(Clone, Debug)]
pub struct AttrInfo {
    pub name: PhpStr,
    /// Arguments: (name or None, constant-expression code).
    pub args: Vec<(Option<PhpStr>, ConstExpr)>,
    pub line: u32,
}

/// A constant expression: a value known at compile time, or code that
/// computes it (run in the declaring scope on first use).
#[derive(Clone, Debug)]
pub enum ConstExpr {
    Value(Value),
    Code(Rc<FuncCode>),
}

/// A try/catch/finally region.
#[derive(Clone, Copy, Debug)]
pub struct TryRange {
    pub start: u32,
    pub end: u32,
    /// First catch instruction (0: none).
    pub catch: u32,
    /// Finally block start (0: none) and its return-address slot.
    pub finally: u32,
    pub finally_end: u32,
    pub finally_ret: Reg,
    /// The slot holding the exception pending across a finally.
    pub exc_slot: Reg,
    /// Number of pending calls open at the start of the try.
    pub call_depth: u32,
}

pub mod fflags {
    pub const BY_REF: u32 = 1;
    pub const GENERATOR: u32 = 2;
    pub const VARIADIC: u32 = 4;
    pub const STRICT: u32 = 8;
    pub const CLOSURE: u32 = 16;
    pub const STATIC_CLOSURE: u32 = 32;
    pub const ARROW: u32 = 64;
    pub const USES_THIS: u32 = 128;
    /// Main code of a file or eval: runs in the includer's scope.
    pub const PSEUDO_MAIN: u32 = 256;
    pub const HAS_RETURN_TYPE: u32 = 512;
    pub const ABSTRACT: u32 = 1024;
    /// Uses `$$`, `extract`, `compact`, `get_defined_vars`...
    pub const DYNAMIC_VARS: u32 = 2048;
    pub const RETURNS_NEVER: u32 = 4096;
    pub const RETURNS_VOID: u32 = 8192;
    /// A const-expression thunk.
    pub const CONST_EXPR: u32 = 16384;
    pub const HAS_DEFAULT_CODE: u32 = 32768;
    pub const DEPRECATED: u32 = 65536;
    pub const NODISCARD: u32 = 131072;
}

/// Jump table for `switch` / `match`.
#[derive(Clone, Debug)]
pub struct SwitchTable {
    pub ints: Vec<(i64, u32)>,
    pub strs: Vec<(PhpStr, u32)>,
    pub default: u32,
}

/// A compiled function, method, closure, file or const expression.
#[derive(Debug)]
pub struct FuncCode {
    /// Name as declared (`foo`, `bar` for methods, `{closure}`).
    pub name: PhpStr,
    /// Declaring class name for methods (for messages: `A::foo`).
    pub class_name: Option<PhpStr>,
    pub file: PhpStr,
    pub ops: Vec<Op>,
    pub lines: Vec<u32>,
    pub consts: Vec<Value>,
    /// Number of named variables (slots 0..num_cvs).
    pub num_cvs: u32,
    /// Total slots (variables, hidden variables, temporaries).
    pub num_slots: u32,
    pub cv_names: Vec<PhpStr>,
    pub params: Vec<ParamInfo>,
    pub num_required: u32,
    pub ret_type: Option<TypeHint>,
    pub flags: u32,
    pub try_ranges: Vec<TryRange>,
    /// (start, end, try index): instructions not covered by that try
    /// statement although inside its range (code leaving it on return,
    /// break or goto, after its finally ran).
    pub try_holes: Vec<(u32, u32, u32)>,
    /// (start, end, slot, is_iterator): loop temporaries (foreach
    /// iterators, switch subjects) live over [start, end), freed when an
    /// exception leaves that range.
    pub live_ranges: Vec<(u32, u32, Reg, bool)>,
    pub paths: Vec<WritePath>,
    pub lists: Vec<Vec<Reg>>,
    pub tables: Vec<SwitchTable>,
    /// Nested functions and closures (by index from instructions).
    pub children: Vec<Rc<FuncCode>>,
    /// Classes declared in this code (templates, by index).
    pub classes: Vec<Rc<crate::class::ClassDecl>>,
    /// Closure `use` variables: (slot in this function, slot in parent,
    /// by reference).
    pub uses: Vec<(u32, u32, bool)>,
    /// Number of static variables.
    pub num_statics: u32,
    pub type_hints: Vec<TypeHint>,
    pub line_start: u32,
    pub line_end: u32,
    pub doc_comment: Option<PhpStr>,
    pub attrs: Rc<[AttrInfo]>,
    /// Instructions (DoCall) whose result is discarded by an expression
    /// statement (not by a `(void)` cast), sorted.
    pub discarded_calls: Vec<u32>,
    /// Attributes of `const` statements in this code, by constant name.
    pub const_attrs: Vec<(PhpStr, Rc<[AttrInfo]>)>,
    /// Inline caches (instruction-local, by index).
    pub caches: Box<[Cell<Cache>]>,
    /// Lazily computed name → slot map for dynamic variable access.
    pub cv_map: RefCell<Option<std::collections::HashMap<PhpStr, u32>>>,
    /// Static variable storage for plain functions (methods and closures
    /// keep their own).
    pub statics: RefCell<Vec<Option<goro_runtime::PhpRef>>>,
}

/// An inline cache entry. `key` identifies what was seen (class or
/// function pointer), `a`/`b` what was found.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cache {
    pub key: usize,
    pub a: usize,
    pub b: u32,
}

impl FuncCode {
    pub fn line_of(&self, ip: usize) -> u32 {
        self.lines.get(ip).copied().unwrap_or(self.line_start)
    }

    pub fn cv_slot(&self, name: &[u8]) -> Option<u32> {
        if self.cv_names.len() < 8 {
            return self
                .cv_names
                .iter()
                .position(|n| n.as_bytes() == name)
                .map(|i| i as u32);
        }
        let mut m = self.cv_map.borrow_mut();
        let m = m.get_or_insert_with(|| {
            self.cv_names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), i as u32))
                .collect()
        });
        m.get(&PhpStr::new(name)).copied()
    }

    pub fn is_generator(&self) -> bool {
        self.flags & fflags::GENERATOR != 0
    }

    /// `A::foo` / `foo` / `{closure}` for messages.
    pub fn display_name(&self) -> String {
        match &self.class_name {
            Some(c) if self.flags & fflags::CLOSURE == 0 => {
                format!("{}::{}", c.to_string_lossy(), self.name.to_string_lossy())
            }
            _ => self.name.to_string_lossy(),
        }
    }
}
