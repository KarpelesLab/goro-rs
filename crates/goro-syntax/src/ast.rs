//! The abstract syntax tree.
//!
//! Nodes borrow identifiers and literal text from the source (`'a`). Every
//! node carries a [`Span`]: byte offsets and the first and last line.
//! Language constructs follow the PHP manual's Language Reference.

/// Source location of a node: byte range and lines (1-based).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    /// Line of the first token.
    pub line: u32,
    /// Line of the last token (the line it starts on).
    pub end_line: u32,
}

impl Span {
    pub fn new(start: u32, end: u32, line: u32, end_line: u32) -> Self {
        Span {
            start,
            end,
            line,
            end_line,
        }
    }

    /// The span covering `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
            line: self.line,
            end_line: other.end_line,
        }
    }
}

/// An identifier: function, class, constant, member or label name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident<'a> {
    pub name: &'a [u8],
    pub span: Span,
}

/// How a name is qualified (PHP manual: "Namespaces: Basics").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// `foo`
    Unqualified,
    /// `foo\bar`
    Qualified,
    /// `\foo\bar`
    FullyQualified,
    /// `namespace\foo`
    Relative,
}

/// A class, function or constant name as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name<'a> {
    pub kind: NameKind,
    /// The name without a leading `\` or `namespace\` prefix.
    pub name: &'a [u8],
    pub span: Span,
}

impl<'a> Name<'a> {
    /// Whether the (unqualified) name is `self`, `parent` or `static`
    /// (case-insensitive).
    pub fn is_special_class(&self) -> bool {
        self.kind == NameKind::Unqualified
            && (self.name.eq_ignore_ascii_case(b"self")
                || self.name.eq_ignore_ascii_case(b"parent")
                || self.name.eq_ignore_ascii_case(b"static"))
    }
}

/// A whole file or `eval()`'d string.
#[derive(Debug)]
pub struct Program<'a> {
    pub stmts: &'a [Stmt<'a>],
    /// Byte offset of the data after `__halt_compiler();`
    /// (`__COMPILER_HALT_OFFSET__`), if present at top level.
    pub halt_compiler_offset: Option<u32>,
    /// Compile-time errors the parser detected that PHP reports after a
    /// successful parse (Fatal error), in source order. The compiler merges
    /// them with its own diagnostics.
    pub compile_errors: Vec<CompileError>,
    /// Deprecations and warnings emitted while parsing (for example
    /// non-canonical casts), in source order.
    pub notices: Vec<Notice>,
    /// Byte offsets of keyword tokens the parser used as names (`X::class`,
    /// `$o->list()`, `function list()`, named arguments...). With
    /// `TOKEN_PARSE`, `token_get_all()` reports these as `T_STRING` (test
    /// ext/tokenizer/tests/token_get_all_TOKEN_PARSE_001.phpt).
    pub reserved_identifiers: Vec<u32>,
}

/// A compile-time error found while parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
    pub offset: u32,
}

/// The level of a parser notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Deprecated,
    Warning,
}

/// A deprecation or warning found while parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub message: String,
    pub line: u32,
    /// Byte offset of the token being parsed when it was found.
    pub offset: u32,
}

// ===== statements =====

#[derive(Debug, Clone, Copy)]
pub struct Stmt<'a> {
    pub kind: StmtKind<'a>,
    pub span: Span,
}

/// A block body: `{ ... }`, a single statement, or the alternative syntax
/// (`: ... endif;`).
pub type Block<'a> = &'a [Stmt<'a>];

#[derive(Debug, Clone, Copy)]
pub enum StmtKind<'a> {
    /// `expr;`
    Expr(Expr<'a>),
    /// `echo a, b;` and `<?= a ?>`.
    Echo(&'a [Expr<'a>]),
    /// Text outside PHP tags.
    InlineHtml(&'a [u8]),
    /// `{ ... }`
    Block(Block<'a>),
    If(&'a IfStmt<'a>),
    While {
        cond: Expr<'a>,
        body: Block<'a>,
        alt_syntax: bool,
    },
    DoWhile {
        body: Block<'a>,
        cond: Expr<'a>,
    },
    For(&'a ForStmt<'a>),
    Foreach(&'a ForeachStmt<'a>),
    Switch(&'a SwitchStmt<'a>),
    /// `break n;` (`None`: no level given).
    Break(Option<Expr<'a>>),
    Continue(Option<Expr<'a>>),
    Return(Option<Expr<'a>>),
    /// `global $a, $$b;`
    Global(&'a [Expr<'a>]),
    /// `static $a = 1, $b;`
    Static(&'a [StaticVar<'a>]),
    /// `unset($a, $b[1]);`
    Unset(&'a [Expr<'a>]),
    Try(&'a TryStmt<'a>),
    /// `goto label;`
    Goto(Ident<'a>),
    /// `label:`
    Label(Ident<'a>),
    Declare(&'a DeclareStmt<'a>),
    Function(&'a FunctionDecl<'a>),
    Class(&'a ClassDecl<'a>),
    /// `namespace Foo;` (body `None`) or `namespace Foo { ... }`.
    Namespace {
        name: Option<Name<'a>>,
        body: Option<Block<'a>>,
    },
    /// `use A\B as C, D;`, `use function f;`, `use const C;`
    Use {
        kind: UseKind,
        uses: &'a [UseItem<'a>],
    },
    /// `use A\{B, function c, const D};`
    GroupUse {
        kind: UseKind,
        prefix: Name<'a>,
        uses: &'a [UseItem<'a>],
    },
    /// `const A = 1, B = 2;` (top level / namespace).
    Const {
        attrs: &'a [AttributeGroup<'a>],
        items: &'a [ConstItem<'a>],
    },
    /// `__halt_compiler();`: the data starts at `offset`.
    HaltCompiler {
        offset: u32,
    },
    /// `;` alone.
    Nop,
}

#[derive(Debug, Clone, Copy)]
pub struct IfStmt<'a> {
    pub cond: Expr<'a>,
    pub then: Block<'a>,
    pub elseifs: &'a [ElseIf<'a>],
    pub else_: Option<Block<'a>>,
    /// `if (...): ... endif;`
    pub alt_syntax: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ElseIf<'a> {
    pub cond: Expr<'a>,
    pub body: Block<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct ForStmt<'a> {
    pub init: &'a [Expr<'a>],
    pub cond: &'a [Expr<'a>],
    pub step: &'a [Expr<'a>],
    pub body: Block<'a>,
    pub alt_syntax: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ForeachStmt<'a> {
    pub expr: Expr<'a>,
    pub key: Option<Expr<'a>>,
    /// The value target: a variable, `list()`/`[...]` destructuring, or
    /// any writable expression.
    pub value: Expr<'a>,
    /// `as &$v`
    pub by_ref: bool,
    pub body: Block<'a>,
    pub alt_syntax: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SwitchStmt<'a> {
    pub subject: Expr<'a>,
    pub cases: &'a [SwitchCase<'a>],
    pub alt_syntax: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SwitchCase<'a> {
    /// `None` for `default:`.
    pub cond: Option<Expr<'a>>,
    pub body: Block<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct StaticVar<'a> {
    pub name: Ident<'a>,
    pub default: Option<Expr<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct TryStmt<'a> {
    pub body: Block<'a>,
    pub catches: &'a [Catch<'a>],
    pub finally: Option<Block<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub struct Catch<'a> {
    pub types: &'a [Name<'a>],
    /// `catch (E)` without a variable (PHP 8.0).
    pub var: Option<Ident<'a>>,
    pub body: Block<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct DeclareStmt<'a> {
    pub directives: &'a [(Ident<'a>, Expr<'a>)],
    pub body: DeclareBody<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum DeclareBody<'a> {
    /// `declare(...);`
    None,
    /// `declare(...) stmt` or `declare(...) { ... }`
    Block(Block<'a>),
    /// `declare(...): ... enddeclare;`
    Alt(Block<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseKind {
    Normal,
    Function,
    Const,
}

#[derive(Debug, Clone, Copy)]
pub struct UseItem<'a> {
    pub name: Name<'a>,
    pub alias: Option<Ident<'a>>,
    /// In a group use, the kind given on the item.
    pub kind: UseKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct ConstItem<'a> {
    pub name: Ident<'a>,
    pub value: Expr<'a>,
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

// ===== declarations =====

/// Modifier flags for classes, members and promoted parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(pub u16);

impl Modifiers {
    pub const PUBLIC: u16 = 1;
    pub const PROTECTED: u16 = 2;
    pub const PRIVATE: u16 = 4;
    pub const STATIC: u16 = 8;
    pub const ABSTRACT: u16 = 16;
    pub const FINAL: u16 = 32;
    pub const READONLY: u16 = 64;
    pub const PUBLIC_SET: u16 = 128;
    pub const PROTECTED_SET: u16 = 256;
    pub const PRIVATE_SET: u16 = 512;
    /// `var` (a property declared with `var`).
    pub const VAR: u16 = 1024;
    pub const VISIBILITY: u16 = Self::PUBLIC | Self::PROTECTED | Self::PRIVATE;
    pub const SET_VISIBILITY: u16 = Self::PUBLIC_SET | Self::PROTECTED_SET | Self::PRIVATE_SET;

    #[inline]
    pub fn has(self, f: u16) -> bool {
        self.0 & f != 0
    }
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AttributeGroup<'a> {
    pub attrs: &'a [Attribute<'a>],
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct Attribute<'a> {
    pub name: Name<'a>,
    pub args: &'a [Arg<'a>],
    /// Whether an argument list was written (`#[A()]` vs `#[A]`).
    pub has_args: bool,
    pub span: Span,
}

/// A type declaration (PHP manual: "Type declarations").
#[derive(Debug, Clone, Copy)]
pub enum Type<'a> {
    /// A class or scalar type name (`int`, `Foo`, `self`, `mixed`...).
    Named(Name<'a>),
    /// `array`
    Array(Span),
    /// `callable`
    Callable(Span),
    /// `static`
    Static(Span),
    /// `?T`
    Nullable(&'a Type<'a>, Span),
    /// `A|B`
    Union(&'a [Type<'a>], Span),
    /// `A&B`
    Intersection(&'a [Type<'a>], Span),
}

impl Type<'_> {
    pub fn span(&self) -> Span {
        match self {
            Type::Named(n) => n.span,
            Type::Array(s) | Type::Callable(s) | Type::Static(s) => *s,
            Type::Nullable(_, s) | Type::Union(_, s) | Type::Intersection(_, s) => *s,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Param<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    /// Constructor promotion modifiers (visibility, `readonly`, set
    /// visibility, `final`).
    pub modifiers: Modifiers,
    pub ty: Option<Type<'a>>,
    pub by_ref: bool,
    pub variadic: bool,
    /// Name without `$`.
    pub name: Ident<'a>,
    pub default: Option<Expr<'a>>,
    /// Property hooks on a promoted parameter (PHP 8.4).
    pub hooks: &'a [PropertyHook<'a>],
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct FunctionDecl<'a> {
    pub name: Ident<'a>,
    pub attrs: &'a [AttributeGroup<'a>],
    pub by_ref: bool,
    pub params: &'a [Param<'a>],
    pub ret: Option<Type<'a>>,
    pub body: Block<'a>,
    pub doc_comment: Option<&'a [u8]>,
    /// Span of the whole declaration (start line to closing brace line).
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

#[derive(Debug, Clone, Copy)]
pub struct ClassDecl<'a> {
    pub kind: ClassKind,
    /// `None` for anonymous classes.
    pub name: Option<Ident<'a>>,
    pub attrs: &'a [AttributeGroup<'a>],
    /// `abstract`, `final`, `readonly`.
    pub modifiers: Modifiers,
    /// `extends` of a class.
    pub extends: Option<Name<'a>>,
    /// `implements` of a class or enum; `extends` of an interface.
    pub implements: &'a [Name<'a>],
    /// Backing type of an enum (`enum E: string`).
    pub enum_backing: Option<Type<'a>>,
    pub members: &'a [ClassMember<'a>],
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub enum ClassMember<'a> {
    Property(PropertyDecl<'a>),
    Const(ClassConstDecl<'a>),
    Method(MethodDecl<'a>),
    TraitUse(TraitUse<'a>),
    EnumCase(EnumCase<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct PropertyDecl<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub modifiers: Modifiers,
    pub ty: Option<Type<'a>>,
    pub props: &'a [PropertyItem<'a>],
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct PropertyItem<'a> {
    /// Name without `$`.
    pub name: Ident<'a>,
    pub default: Option<Expr<'a>>,
    /// Property hooks (PHP 8.4); empty when there is no hook list.
    pub hooks: &'a [PropertyHook<'a>],
    /// Whether a `{ ... }` hook list was written.
    pub has_hook_list: bool,
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct PropertyHook<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    /// `final` (and other modifiers the grammar accepts, which the
    /// compiler rejects).
    pub modifiers: Modifiers,
    pub by_ref: bool,
    /// `get` or `set` (any identifier is accepted by the grammar).
    pub name: Ident<'a>,
    /// `set(Type $v)`: explicit parameter list.
    pub params: Option<&'a [Param<'a>]>,
    pub body: HookBody<'a>,
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub enum HookBody<'a> {
    /// `get;`
    Abstract,
    /// `get => expr;`
    Expr(&'a Expr<'a>),
    /// `get { ... }`
    Block(Block<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct ClassConstDecl<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub modifiers: Modifiers,
    /// Typed class constant (PHP 8.3).
    pub ty: Option<Type<'a>>,
    pub items: &'a [ConstItem<'a>],
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct MethodDecl<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub modifiers: Modifiers,
    pub by_ref: bool,
    pub name: Ident<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<Type<'a>>,
    /// `None` for abstract / interface methods (`;` body).
    pub body: Option<Block<'a>>,
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct TraitUse<'a> {
    pub traits: &'a [Name<'a>],
    pub adaptations: &'a [TraitAdaptation<'a>],
    /// Whether a `{ ... }` block was written.
    pub has_block: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub enum TraitAdaptation<'a> {
    /// `A::foo insteadof B, C;`
    Precedence {
        trait_: Name<'a>,
        method: Ident<'a>,
        insteadof: &'a [Name<'a>],
        span: Span,
    },
    /// `[A::]foo as [visibility] [bar];`
    Alias {
        trait_: Option<Name<'a>>,
        method: Ident<'a>,
        modifiers: Modifiers,
        alias: Option<Ident<'a>>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct EnumCase<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub name: Ident<'a>,
    pub value: Option<Expr<'a>>,
    pub doc_comment: Option<&'a [u8]>,
    pub span: Span,
}

// ===== expressions =====

#[derive(Debug, Clone, Copy)]
pub struct Expr<'a> {
    pub kind: ExprKind<'a>,
    pub span: Span,
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
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
    ShiftLeft,
    ShiftRight,
    /// `&&` / `and`
    BoolAnd,
    /// `||` / `or`
    BoolOr,
    /// `xor`
    LogicalXor,
    Equal,
    NotEqual,
    Identical,
    NotIdentical,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Spaceship,
    /// `??`
    Coalesce,
    /// `|>` (PHP 8.5 pipe operator).
    Pipe,
}

/// Compound assignment operators (`+=` ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssignOp {
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
    ShiftLeft,
    ShiftRight,
    Coalesce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `!`
    Not,
    /// `~`
    BitNot,
    /// `-`
    Neg,
    /// `+`
    Plus,
    /// `@`
    Silence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IncDec {
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastKind {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    /// `(unset)`: removed in PHP 8.0, a compile error.
    Unset,
    /// `(void)` (PHP 8.5).
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MagicConst {
    Line,
    File,
    Dir,
    Class,
    Trait,
    Method,
    Function,
    Property,
    Namespace,
}

/// The class part of `A::b`, `new A`, `$x instanceof A`.
#[derive(Debug, Clone, Copy)]
pub enum ClassRef<'a> {
    /// A name (including `self`, `parent`).
    Name(Name<'a>),
    /// `static`
    Static(Span),
    /// Any expression (`$obj::X`, `(expr)::X`, `new $cls`).
    Expr(&'a Expr<'a>),
}

/// A member name after `->`, `?->` or `::`.
#[derive(Debug, Clone, Copy)]
pub enum MemberName<'a> {
    Ident(Ident<'a>),
    /// `->$name`, `->{expr}`, `::{expr}`, `::$name()`.
    Expr(&'a Expr<'a>),
}

/// A static property name: `A::$x` or `A::$$x` / `A::${expr}`.
#[derive(Debug, Clone, Copy)]
pub enum StaticPropName<'a> {
    Ident(Ident<'a>),
    Expr(&'a Expr<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct Arg<'a> {
    /// Named argument (PHP 8.0).
    pub name: Option<Ident<'a>>,
    pub value: Expr<'a>,
    /// `...$args`
    pub unpack: bool,
    pub span: Span,
}

/// Call arguments.
#[derive(Debug, Clone, Copy)]
pub enum Args<'a> {
    List(&'a [Arg<'a>]),
    /// `f(...)`: first-class callable syntax (PHP 8.1).
    FirstClassCallable,
}

impl<'a> Args<'a> {
    pub fn as_slice(&self) -> &[Arg<'a>] {
        match self {
            Args::List(v) => v,
            Args::FirstClassCallable => &[],
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArrayItem<'a> {
    pub key: Option<Expr<'a>>,
    pub value: Expr<'a>,
    pub by_ref: bool,
    /// `...$spread`
    pub unpack: bool,
    pub span: Span,
}

/// A part of an interpolated string.
#[derive(Debug, Clone, Copy)]
pub enum StrPart<'a> {
    /// Literal text, escapes decoded.
    Lit(&'a [u8]),
    Expr(Expr<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct ClosureUse<'a> {
    pub name: Ident<'a>,
    pub by_ref: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub struct Closure<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub is_static: bool,
    pub by_ref: bool,
    pub params: &'a [Param<'a>],
    pub uses: &'a [ClosureUse<'a>],
    pub ret: Option<Type<'a>>,
    pub body: Block<'a>,
    pub doc_comment: Option<&'a [u8]>,
}

#[derive(Debug, Clone, Copy)]
pub struct ArrowFn<'a> {
    pub attrs: &'a [AttributeGroup<'a>],
    pub is_static: bool,
    pub by_ref: bool,
    pub params: &'a [Param<'a>],
    pub ret: Option<Type<'a>>,
    pub body: &'a Expr<'a>,
    pub doc_comment: Option<&'a [u8]>,
}

#[derive(Debug, Clone, Copy)]
pub struct MatchArm<'a> {
    /// `None` for `default`.
    pub conds: Option<&'a [Expr<'a>]>,
    pub body: Expr<'a>,
    pub span: Span,
}

/// `A::$b` (payload of [`ExprKind::StaticProp`]; the `::` forms are boxed
/// to keep [`Expr`] small).
#[derive(Debug, Clone, Copy)]
pub struct StaticPropNode<'a> {
    pub class: ClassRef<'a>,
    pub name: StaticPropName<'a>,
}

/// `A::B` (payload of [`ExprKind::ClassConst`]).
#[derive(Debug, Clone, Copy)]
pub struct ClassConstNode<'a> {
    pub class: ClassRef<'a>,
    pub name: MemberName<'a>,
}

/// `A::m(...)` (payload of [`ExprKind::StaticCall`]).
#[derive(Debug, Clone, Copy)]
pub struct StaticCallNode<'a> {
    pub class: ClassRef<'a>,
    pub name: MemberName<'a>,
    pub args: Args<'a>,
}

/// What `new` instantiates.
#[derive(Debug, Clone, Copy)]
pub enum NewClass<'a> {
    Ref(ClassRef<'a>),
    Anonymous(&'a ClassDecl<'a>),
}

/// How a string literal was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrKind {
    SingleQuoted,
    DoubleQuoted,
    Heredoc,
    Nowdoc,
}

#[derive(Debug, Clone, Copy)]
pub enum ExprKind<'a> {
    // --- literals ---
    Int(i64),
    Float(f64),
    /// A string without interpolation; escapes and heredoc indentation
    /// already processed.
    String(&'a [u8], StrKind),
    /// `"a $b c"`, heredoc with interpolation.
    Interpolated(&'a [StrPart<'a>], StrKind),
    /// `` `cmd $x` ``
    ShellExec(&'a [StrPart<'a>]),
    /// `[1, 2]` / `array(1, 2)`.
    Array {
        items: &'a [Option<ArrayItem<'a>>],
        short: bool,
    },
    /// `list(...)` / `[...]` on the left of `=` or in `foreach`. Skipped
    /// elements are `None`.
    List {
        items: &'a [Option<ArrayItem<'a>>],
        short: bool,
    },
    MagicConst(MagicConst),

    // --- variables and access ---
    /// `$name` (name without `$`).
    Variable(&'a [u8]),
    /// `$$expr` / `${expr}`.
    VariableVariable(&'a Expr<'a>),
    /// `a[b]` / `a[]` (index `None`). `a{b}` is not PHP 8 syntax.
    Index {
        base: &'a Expr<'a>,
        index: Option<&'a Expr<'a>>,
    },
    /// `a->b`, `a?->b`.
    Prop {
        obj: &'a Expr<'a>,
        name: MemberName<'a>,
        nullsafe: bool,
    },
    /// `A::$b`
    StaticProp(&'a StaticPropNode<'a>),
    /// `FOO`, `\Foo\BAR`, `true`, `null`...
    Const(Name<'a>),
    /// `A::B`, `A::class`, `A::{expr}` (PHP 8.3 dynamic constant fetch).
    ClassConst(&'a ClassConstNode<'a>),

    // --- calls ---
    /// `f(...)` with a name.
    Call {
        name: Name<'a>,
        args: Args<'a>,
    },
    /// `$f(...)`, `(expr)(...)`.
    CallExpr {
        callee: &'a Expr<'a>,
        args: Args<'a>,
    },
    MethodCall {
        obj: &'a Expr<'a>,
        name: MemberName<'a>,
        args: Args<'a>,
        nullsafe: bool,
    },
    /// `A::m(...)`
    StaticCall(&'a StaticCallNode<'a>),
    New {
        class: NewClass<'a>,
        /// `None`: no parentheses (`new A`).
        args: Option<Args<'a>>,
    },

    // --- operators ---
    Assign {
        target: &'a Expr<'a>,
        value: &'a Expr<'a>,
    },
    /// `$a = &$b`
    AssignRef {
        target: &'a Expr<'a>,
        value: &'a Expr<'a>,
    },
    AssignOp {
        op: AssignOp,
        target: &'a Expr<'a>,
        value: &'a Expr<'a>,
    },
    Binary {
        op: BinaryOp,
        left: &'a Expr<'a>,
        right: &'a Expr<'a>,
    },
    Unary {
        op: UnaryOp,
        expr: &'a Expr<'a>,
    },
    IncDec {
        op: IncDec,
        expr: &'a Expr<'a>,
    },
    Cast {
        kind: CastKind,
        expr: &'a Expr<'a>,
    },
    /// `a ? b : c`, `a ?: c` (`then` is `None`).
    Ternary {
        cond: &'a Expr<'a>,
        then: Option<&'a Expr<'a>>,
        else_: &'a Expr<'a>,
    },
    Instanceof {
        expr: &'a Expr<'a>,
        class: ClassRef<'a>,
    },

    // --- language constructs ---
    Isset(&'a [Expr<'a>]),
    Empty(&'a Expr<'a>),
    Eval(&'a Expr<'a>),
    Include {
        kind: IncludeKind,
        expr: &'a Expr<'a>,
    },
    /// `exit`, `exit(...)`, `die(...)`: `None` without parentheses.
    Exit(Option<Args<'a>>),
    Print(&'a Expr<'a>),
    /// `clone $x` (`args` `None`), or the PHP 8.5 function form
    /// `clone($x, [...])` / `clone(...)`.
    Clone {
        expr: Option<&'a Expr<'a>>,
        args: Option<Args<'a>>,
    },
    Throw(&'a Expr<'a>),
    Yield {
        key: Option<&'a Expr<'a>>,
        value: Option<&'a Expr<'a>>,
    },
    YieldFrom(&'a Expr<'a>),
    Closure(&'a Closure<'a>),
    ArrowFn(&'a ArrowFn<'a>),
    Match {
        subject: &'a Expr<'a>,
        arms: &'a [MatchArm<'a>],
    },
}
