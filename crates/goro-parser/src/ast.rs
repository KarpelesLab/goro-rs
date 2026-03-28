use crate::token::Span;

/// Top-level AST: a PHP file is a sequence of statements
#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
}

/// Statement nodes
#[derive(Debug, Clone)]
pub struct Statement {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    /// Raw HTML output (outside <?php tags)
    InlineHtml(Vec<u8>),

    /// Expression statement (expression followed by ;)
    Expression(Expr),

    /// echo expr1, expr2, ...;
    Echo(Vec<Expr>),

    /// return expr?;
    Return(Option<Expr>),

    /// $var = expr (also compound assignments)
    /// Represented as Expression(Assign(...)) but kept for clarity

    /// if (cond) { ... } elseif ... else { ... }
    If {
        condition: Expr,
        body: Vec<Statement>,
        elseif_clauses: Vec<(Expr, Vec<Statement>)>,
        else_body: Option<Vec<Statement>>,
    },

    /// while (cond) { ... }
    While {
        condition: Expr,
        body: Vec<Statement>,
    },

    /// do { ... } while (cond);
    DoWhile {
        body: Vec<Statement>,
        condition: Expr,
    },

    /// for (init; cond; update) { ... }
    For {
        init: Vec<Expr>,
        condition: Vec<Expr>,
        update: Vec<Expr>,
        body: Vec<Statement>,
    },

    /// foreach ($array as $key => $value) { ... }
    Foreach {
        expr: Expr,
        key: Option<Expr>,
        value: Expr,
        by_ref: bool,
        body: Vec<Statement>,
    },

    /// switch (expr) { case ...: ... }
    Switch { expr: Expr, cases: Vec<SwitchCase> },

    /// break N;
    Break(Option<Expr>),

    /// continue N;
    Continue(Option<Expr>),

    /// function name(params): returntype { body }
    FunctionDecl {
        name: Vec<u8>,
        params: Vec<Param>,
        return_type: Option<TypeHint>,
        body: Vec<Statement>,
        is_static: bool,
    },

    /// class Name extends Parent implements Iface { ... }
    ClassDecl {
        name: Vec<u8>,
        modifiers: ClassModifiers,
        extends: Option<Vec<u8>>,
        implements: Vec<Vec<u8>>,
        body: Vec<ClassMember>,
        /// For enums: the backing type (e.g. b"string", b"int"), None for unit enums
        enum_backing_type: Option<Vec<u8>>,
    },

    /// try { ... } catch (E $e) { ... } finally { ... }
    TryCatch {
        try_body: Vec<Statement>,
        catches: Vec<CatchClause>,
        finally_body: Option<Vec<Statement>>,
    },

    /// throw expr;
    Throw(Expr),

    /// global $var1, $var2;
    Global(Vec<Vec<u8>>),

    /// static $var = expr;
    StaticVar(Vec<(Vec<u8>, Option<Expr>)>),

    /// unset($var, ...);
    Unset(Vec<Expr>),

    /// declare(strict_types=1) { ... }
    Declare {
        directives: Vec<(Vec<u8>, Expr)>,
        body: Option<Vec<Statement>>,
    },

    /// namespace Name\Space;
    NamespaceDecl {
        name: Option<Vec<Vec<u8>>>,
        body: Option<Vec<Statement>>,
    },

    /// use Name\Space\{Class1, Class2};
    UseDecl(Vec<UseItem>),

    /// label:
    Label(Vec<u8>),

    /// goto label;
    Goto(Vec<u8>),

    /// Empty statement (lone ;)
    Nop,
}

/// Expression nodes
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    // Literals
    Int(i64),
    Float(f64),
    String(Vec<u8>),
    InterpolatedString(Vec<StringPart>),
    True,
    False,
    Null,
    Array(Vec<ArrayElement>),

    // Variables
    Variable(Vec<u8>),          // $foo
    DynamicVariable(Box<Expr>), // $$foo

    // Operations
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

    // Member access
    PropertyAccess {
        object: Box<Expr>,
        property: Box<Expr>,
        nullsafe: bool,
    },
    StaticPropertyAccess {
        class: Box<Expr>,
        property: Vec<u8>,
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
    },
    /// Dynamic static method call: Foo::$method()
    DynamicStaticMethodCall {
        class: Box<Expr>,
        method: Box<Expr>,
        args: Vec<Argument>,
    },
    ArrayAccess {
        array: Box<Expr>,
        index: Option<Box<Expr>>,
    },

    // Function calls
    FunctionCall {
        name: Box<Expr>,
        args: Vec<Argument>,
    },

    // Ternary
    Ternary {
        condition: Box<Expr>,
        if_true: Option<Box<Expr>>, // None for short ternary ($a ?: $b)
        if_false: Box<Expr>,
    },

    // Null coalescing
    NullCoalesce {
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // Match expression
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    // Closures
    Closure {
        is_static: bool,
        params: Vec<Param>,
        use_vars: Vec<ClosureUse>,
        return_type: Option<TypeHint>,
        body: Vec<Statement>,
    },
    ArrowFunction {
        is_static: bool,
        params: Vec<Param>,
        return_type: Option<TypeHint>,
        body: Box<Expr>,
    },

    // Object creation
    New {
        class: Box<Expr>,
        args: Vec<Argument>,
    },

    // Instanceof
    Instanceof {
        expr: Box<Expr>,
        class: Box<Expr>,
    },

    // Cast
    Cast(CastType, Box<Expr>),

    // Special
    Print(Box<Expr>),
    Exit(Option<Box<Expr>>),
    Empty(Box<Expr>),
    Isset(Vec<Expr>),
    Eval(Box<Expr>),
    Include {
        kind: IncludeKind,
        path: Box<Expr>,
    },
    /// yield [key =>] [value]
    /// First field: value (None for bare yield)
    /// Second field: key (Some for yield $key => $value)
    Yield(Option<Box<Expr>>, Option<Box<Expr>>),
    YieldFrom(Box<Expr>),

    // Clone
    Clone(Box<Expr>),

    // Spread in function args / array
    Spread(Box<Expr>),

    // Pipe operator (PHP 8.5): expr |> callable
    Pipe {
        value: Box<Expr>,
        callable: Box<Expr>,
    },

    // Name resolution
    ConstantAccess(Vec<Vec<u8>>), // FOO, Namespace\FOO
    ClassConstAccess {
        class: Box<Expr>,
        constant: Vec<u8>,
    },
    /// Dynamic class constant fetch: Foo::{$expr}
    DynamicClassConstAccess {
        class: Box<Expr>,
        constant: Box<Expr>,
    },

    // Throw as expression (PHP 8.0+)
    ThrowExpr(Box<Expr>),

    // Error suppression
    Suppress(Box<Expr>),

    // Pre-resolved identifier (used internally during compilation)
    Identifier(Vec<u8>),

    // First-class callable syntax: strlen(...), $obj->method(...), Foo::method(...)
    FirstClassCallable(CallableTarget),
}

/// The target of a first-class callable expression
#[derive(Debug, Clone)]
pub enum CallableTarget {
    /// Function call: strlen(...)
    Function(Box<Expr>),
    /// Method call: $obj->method(...)
    Method {
        object: Box<Expr>,
        method: Box<Expr>,
        nullsafe: bool,
    },
    /// Static method call: Foo::method(...)
    StaticMethod {
        class: Box<Expr>,
        method: Vec<u8>,
    },
}

// Supporting types

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
    LogicalAnd, // 'and' keyword
    LogicalOr,  // 'or' keyword
    LogicalXor, // 'xor' keyword
    Equal,
    Identical,
    NotEqual,
    NotIdentical,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Spaceship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,        // -
    Plus,          // + (unary)
    BitwiseNot,    // ~
    BooleanNot,    // !
    PreIncrement,  // ++$x
    PreDecrement,  // --$x
    PostIncrement, // $x++
    PostDecrement, // $x--
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
    pub name: Option<Vec<u8>>, // Named argument
    pub value: Expr,
    pub unpack: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Vec<u8>,
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    pub visibility: Option<Visibility>,
    pub readonly: bool,
}

#[derive(Debug, Clone)]
pub enum TypeHint {
    Simple(Vec<u8>),             // int, string, ClassName
    Nullable(Box<TypeHint>),     // ?Type
    Union(Vec<TypeHint>),        // Type1|Type2
    Intersection(Vec<TypeHint>), // Type1&Type2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: Option<Expr>, // None = default
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub conditions: Option<Vec<Expr>>, // None = default
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: Vec<Vec<Vec<u8>>>, // Qualified class names
    pub variable: Option<Vec<u8>>,
    pub body: Vec<Statement>,
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
        type_hint: Option<TypeHint>,
        default: Option<Expr>,
        visibility: Visibility,
        is_static: bool,
        is_readonly: bool,
        /// Property get hook body (PHP 8.4)
        get_hook: Option<Vec<Statement>>,
        /// Property set hook: (parameter_name, body) (PHP 8.4)
        set_hook: Option<(Vec<u8>, Vec<Statement>)>,
    },
    Method {
        name: Vec<u8>,
        params: Vec<Param>,
        return_type: Option<TypeHint>,
        body: Option<Vec<Statement>>, // None for abstract
        visibility: Visibility,
        is_static: bool,
        is_abstract: bool,
        is_final: bool,
        line: u32,
    },
    ClassConstant {
        name: Vec<u8>,
        value: Expr,
        visibility: Visibility,
    },
    TraitUse {
        traits: Vec<Vec<u8>>,
        adaptations: Vec<TraitAdaptation>,
    },
    EnumCase {
        name: Vec<u8>,
        value: Option<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum TraitAdaptation {
    Alias {
        trait_name: Option<Vec<u8>>,
        method: Vec<u8>,
        new_name: Option<Vec<u8>>,
        new_visibility: Option<Visibility>,
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
