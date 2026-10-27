use crate::token::Span;
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: Vec<Vec<u8>>,
    pub args: Vec<Argument>,
    /// Line of the attribute name.
    pub line: u32,
    /// Which `#[...]` group of the list the attribute is in (from 1).
    pub group: u32,
    /// Whether the attribute has an argument list (`#[A()]` vs `#[A]`).
    pub has_args: bool,
}
#[derive(Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
    /// Whether a `yield` was parsed: when false no function of the program
    /// is a generator.
    pub has_yield: bool,
    /// The runtime definition key slots the file's class declarations
    /// take (see `StmtKind::ClassDecl::rtd_seq`).
    pub rtd_slots: u32,
}

// (as derived, without has_yield / rtd_slots: what the statements tell)
impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program")
            .field("statements", &self.statements)
            .finish()
    }
}
#[derive(Debug, Clone)]
pub struct Statement {
    pub kind: StmtKind,
    pub span: Span,
}
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // AST node; boxing variants would churn every consumer
pub enum StmtKind {
    InlineHtml(Vec<u8>),
    Expression(Expr),
    Echo(Vec<Expr>),
    Return(Option<Expr>),
    If {
        condition: Expr,
        body: Vec<Statement>,
        elseif_clauses: Vec<(Expr, Vec<Statement>)>,
        else_body: Option<Vec<Statement>>,
    },
    While {
        condition: Expr,
        body: Vec<Statement>,
    },
    DoWhile {
        body: Vec<Statement>,
        condition: Expr,
    },
    For {
        init: Vec<Expr>,
        condition: Vec<Expr>,
        update: Vec<Expr>,
        body: Vec<Statement>,
    },
    Foreach {
        expr: Expr,
        key: Option<Box<Expr>>,
        value: Box<Expr>,
        by_ref: bool,
        body: Vec<Statement>,
    },
    Switch {
        expr: Expr,
        cases: Vec<SwitchCase>,
    },
    Break(Option<Expr>),
    Continue(Option<Expr>),
    FunctionDecl {
        name: Vec<u8>,
        params: Vec<Param>,
        return_type: Option<TypeHint>,
        body: Vec<Statement>,
        is_static: bool,
        attributes: Vec<Attribute>,
        end_line: u32,
        doc_comment: Option<Vec<u8>>,
        returns_by_ref: bool,
    },
    ClassDecl {
        name: Vec<u8>,
        modifiers: ClassModifiers,
        extends: Option<Vec<u8>>,
        implements: Vec<Vec<u8>>,
        body: Vec<ClassMember>,
        enum_backing_type: Option<Vec<u8>>,
        attributes: Vec<Attribute>,
        end_line: u32,
        doc_comment: Option<Vec<u8>>,
        /// Position among the file's class declarations in the order
        /// zend_compile_class_decl() builds their runtime definition keys
        /// (CG(rtd_key_counter)): an anonymous class's when it is met, a
        /// named class's after its body. (Anonymous classes are named
        /// __anonymous_class_<rtd_seq>.)
        rtd_seq: u32,
    },
    TryCatch {
        try_body: Vec<Statement>,
        catches: Vec<CatchClause>,
        finally_body: Option<Vec<Statement>>,
    },
    Throw(Expr),
    Global(Vec<Vec<u8>>),
    GlobalDynamic(Vec<Expr>),
    StaticVar(Vec<(Vec<u8>, Option<Expr>)>),
    Unset(Vec<Expr>),
    Declare {
        directives: Vec<(Vec<u8>, Expr)>,
        body: Option<Vec<Statement>>,
        attributes: Vec<Attribute>,
        /// `const A = 1, B = 2;` (rather than a `declare()` statement).
        is_const: bool,
    },
    NamespaceDecl {
        name: Option<Vec<Vec<u8>>>,
        body: Option<Vec<Statement>>,
    },
    UseDecl(Vec<UseItem>),
    Label(Vec<u8>),
    Goto(Vec<u8>),
    Nop,
}
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    String(Vec<u8>),
    InterpolatedString(Vec<StringPart>),
    True,
    False,
    Null,
    Array(Vec<ArrayElement>),
    Variable(Vec<u8>),
    DynamicVariable(Box<Expr>),
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        prefix: bool,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    CompoundAssign {
        op: BinaryOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    AssignRef {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    PropertyAccess {
        object: Box<Expr>,
        property: Box<Expr>,
        nullsafe: bool,
    },
    StaticPropertyAccess {
        class: Box<Expr>,
        property: Vec<u8>,
    },
    DynamicStaticPropertyAccess {
        class: Box<Expr>,
        property: Box<Expr>,
    },
    MethodCall {
        object: Box<Expr>,
        method: Box<Expr>,
        args: Vec<Argument>,
        nullsafe: bool,
    },
    StaticMethodCall {
        class: Box<Expr>,
        method: Vec<u8>,
        args: Vec<Argument>,
        /// Line of the method name (the call's line, GH-8810); 0 = unknown.
        method_line: u32,
    },
    DynamicStaticMethodCall {
        class: Box<Expr>,
        method: Box<Expr>,
        args: Vec<Argument>,
    },
    ArrayAccess {
        array: Box<Expr>,
        index: Option<Box<Expr>>,
    },
    FunctionCall {
        name: Box<Expr>,
        args: Vec<Argument>,
    },
    Ternary {
        condition: Box<Expr>,
        if_true: Option<Box<Expr>>,
        if_false: Box<Expr>,
    },
    NullCoalesce {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// (boxed: the largest expressions, kept out of every Expr)
    Closure(Box<ClosureExpr>),
    ArrowFunction(Box<ArrowFnExpr>),
    New {
        class: Box<Expr>,
        args: Vec<Argument>,
    },
    Instanceof {
        expr: Box<Expr>,
        class: Box<Expr>,
    },
    Cast(CastType, Box<Expr>),
    Print(Box<Expr>),
    Exit(Option<Box<Expr>>),
    Empty(Box<Expr>),
    Isset(Vec<Expr>),
    Eval(Box<Expr>),
    Include {
        kind: IncludeKind,
        path: Box<Expr>,
    },
    Yield(Option<Box<Expr>>, Option<Box<Expr>>),
    YieldFrom(Box<Expr>),
    Clone(Box<Expr>),
    Spread(Box<Expr>),
    Pipe {
        value: Box<Expr>,
        callable: Box<Expr>,
    },
    ConstantAccess(Vec<Vec<u8>>),
    ClassConstAccess {
        class: Box<Expr>,
        constant: Vec<u8>,
    },
    DynamicClassConstAccess {
        class: Box<Expr>,
        constant: Box<Expr>,
    },
    ThrowExpr(Box<Expr>),
    Suppress(Box<Expr>),
    /// `cmd`: the `shell_exec("cmd")` call it compiles to.
    ShellExec(Box<Expr>),
    Identifier(Vec<u8>),
    FirstClassCallable(CallableTarget),
}
#[derive(Debug, Clone)]
pub enum CallableTarget {
    Function(Box<Expr>),
    Method {
        object: Box<Expr>,
        method: Box<Expr>,
        nullsafe: bool,
    },
    StaticMethod {
        class: Box<Expr>,
        method: Vec<u8>,
    },
}
#[derive(Debug, Clone)]
pub enum StringPart {
    Literal(Vec<u8>),
    Expr(Expr),
}
#[derive(Debug, Clone)]
pub struct ArrayElement {
    pub key: Option<Expr>,
    pub value: Expr,
    pub unpack: bool,
    pub by_ref: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    ShiftLeft,
    ShiftRight,
    BooleanAnd,
    BooleanOr,
    LogicalAnd,
    LogicalOr,
    LogicalXor,
    Equal,
    Identical,
    NotEqual,
    NotIdentical,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Spaceship,
    /// Only as `CompoundAssign` op: `$x ??= v` (ZEND_ASSIGN_COALESCE).
    Coalesce,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Plus,
    BitwiseNot,
    BooleanNot,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastType {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Unset,
    Void,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
}
#[derive(Debug, Clone)]
pub struct Argument {
    pub name: Option<Vec<u8>>,
    pub value: Expr,
    pub unpack: bool,
}
#[derive(Debug, Clone)]
pub struct Param {
    /// Line of the parameter's first token after its attributes.
    pub line: u32,
    pub name: Vec<u8>,
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    pub visibility: Option<Visibility>,
    pub set_visibility: Option<Visibility>,
    pub readonly: bool,
    pub is_final: bool,
    pub attributes: Vec<Attribute>,
    pub get_hook: Option<Vec<Statement>>,
    pub set_hook: Option<(Vec<u8>, Vec<Statement>)>,
    pub get_hook_final: bool,
    pub set_hook_final: bool,
    pub get_hook_abstract: bool,
    pub set_hook_abstract: bool,
    pub doc_comment: Option<Vec<u8>>,
    pub set_hook_type: Option<TypeHint>,
    /// `&get { ... }`: the get hook returns by reference.
    pub get_hook_by_ref: bool,
    pub hook_attrs: HookAttrs,
}
/// Attributes of a property's hooks and of the set hook's parameter.
#[derive(Debug, Clone, Default)]
pub struct HookAttrs {
    pub get: Vec<Attribute>,
    pub set: Vec<Attribute>,
    pub set_param: Vec<Attribute>,
    /// `get => expr;` / `set => expr;` (short bodies, expanded by the parser).
    pub get_short: bool,
    pub set_short: bool,
    /// The hooks' doc comments (get, set), boxed: rarely present.
    pub docs: Option<Box<[Option<Vec<u8>>; 2]>>,
    /// The hooks' (start, end) lines (get, set): from the hook's name to the
    /// end of its body (0: unknown).
    pub lines: [(u32, u32); 2],
}
impl HookAttrs {
    /// The doc comment of the get (`set` false) or set hook.
    pub fn doc(&self, set: bool) -> Option<&Vec<u8>> {
        self.docs.as_ref().and_then(|d| d[set as usize].as_ref())
    }
    pub fn set_doc(&mut self, set: bool, doc: Option<Vec<u8>>) {
        if doc.is_some() {
            self.docs.get_or_insert_with(Default::default)[set as usize] = doc;
        }
    }
}
#[derive(Debug, Clone)]
pub enum TypeHint {
    Simple(Vec<u8>),
    Nullable(Box<TypeHint>),
    Union(Vec<TypeHint>),
    Intersection(Vec<TypeHint>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: Option<Expr>,
    pub body: Vec<Statement>,
}
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub conditions: Option<Vec<Expr>>,
    pub body: Expr,
}
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: Vec<Vec<Vec<u8>>>,
    pub variable: Option<Vec<u8>>,
    pub body: Vec<Statement>,
    /// Line of the `catch` keyword (the ZEND_CATCH opline).
    pub line: u32,
}
#[derive(Debug, Clone)]
pub struct ClosureUse {
    pub variable: Vec<u8>,
    pub by_ref: bool,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassModifiers {
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub is_interface: bool,
    pub is_trait: bool,
    pub is_enum: bool,
}
#[derive(Debug, Clone)]
pub enum ClassMember {
    Property {
        name: Vec<u8>,
        /// Line of the property's `$name`.
        line: u32,
        type_hint: Option<TypeHint>,
        default: Option<Expr>,
        visibility: Visibility,
        set_visibility: Option<Visibility>,
        is_static: bool,
        is_readonly: bool,
        is_abstract: bool,
        is_final: bool,
        get_hook: Option<Vec<Statement>>,
        set_hook: Option<(Vec<u8>, Vec<Statement>)>,
        get_hook_final: bool,
        set_hook_final: bool,
        get_hook_abstract: bool,
        set_hook_abstract: bool,
        attributes: Vec<Attribute>,
        doc_comment: Option<Vec<u8>>,
        set_hook_type: Option<TypeHint>,
        /// `&get { ... }`: the get hook returns by reference.
        get_hook_by_ref: bool,
        hook_attrs: Box<HookAttrs>,
        /// Declared after a comma in the previous member's declaration
        /// (`public $a, $b;`).
        group_continues: bool,
    },
    Method {
        name: Vec<u8>,
        params: Vec<Param>,
        return_type: Option<TypeHint>,
        body: Option<Vec<Statement>>,
        visibility: Visibility,
        is_static: bool,
        is_abstract: bool,
        is_final: bool,
        line: u32,
        end_line: u32,
        attributes: Vec<Attribute>,
        doc_comment: Option<Vec<u8>>,
        returns_by_ref: bool,
    },
    ClassConstant {
        name: Vec<u8>,
        value: Expr,
        visibility: Visibility,
        is_final: bool,
        attributes: Vec<Attribute>,
        type_hint: Option<TypeHint>,
        doc_comment: Option<Vec<u8>>,
        /// Declared after a comma in the previous constant's declaration.
        group_continues: bool,
    },
    TraitUse {
        traits: Vec<Vec<u8>>,
        adaptations: Vec<TraitAdaptation>,
        /// Line of the first trait name (the ZEND_AST_USE_TRAIT's).
        line: u32,
    },
    EnumCase {
        name: Vec<u8>,
        /// Line of the `case` keyword
        line: u32,
        value: Option<Expr>,
        attributes: Vec<Attribute>,
        doc_comment: Option<Vec<u8>>,
    },
}
#[derive(Debug, Clone)]
pub enum TraitAdaptation {
    Alias {
        trait_name: Option<Vec<u8>>,
        method: Vec<u8>,
        new_name: Option<Vec<u8>>,
        new_visibility: Option<Visibility>,
        /// `as final`
        new_final: bool,
    },
    Precedence {
        trait_name: Vec<u8>,
        method: Vec<u8>,
        instead_of: Vec<Vec<u8>>,
    },
}
#[derive(Debug, Clone)]
pub struct UseItem {
    pub name: Vec<Vec<u8>>,
    pub alias: Option<Vec<u8>>,
    pub kind: UseKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Normal,
    Function,
    Constant,
}

/// A closure expression's declaration (ExprKind::Closure).
#[derive(Debug, Clone)]
pub struct ClosureExpr {
    pub is_static: bool,
    pub params: Vec<Param>,
    pub use_vars: Vec<ClosureUse>,
    pub return_type: Option<TypeHint>,
    pub body: Vec<Statement>,
    pub attributes: Vec<Attribute>,
    pub end_line: u32,
    pub doc_comment: Option<Vec<u8>>,
    pub returns_by_ref: bool,
}

/// An arrow function's declaration (ExprKind::ArrowFunction).
#[derive(Debug, Clone)]
pub struct ArrowFnExpr {
    pub is_static: bool,
    pub params: Vec<Param>,
    pub return_type: Option<TypeHint>,
    pub body: Box<Expr>,
    pub attributes: Vec<Attribute>,
    pub end_line: u32,
    pub doc_comment: Option<Vec<u8>>,
    pub returns_by_ref: bool,
    /// Written in parentheses (ZEND_PARENTHESIZED_ARROW_FUNC).
    pub parenthesized: bool,
}
