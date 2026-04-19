use crate::token::Span;
#[derive(Debug, Clone)]
pub struct Attribute { pub name: Vec<Vec<u8>>, pub args: Vec<Argument> }
#[derive(Debug, Clone)]
pub struct Program { pub statements: Vec<Statement> }
#[derive(Debug, Clone)]
pub struct Statement { pub kind: StmtKind, pub span: Span }
#[derive(Debug, Clone)]
pub enum StmtKind {
    InlineHtml(Vec<u8>), Expression(Expr), Echo(Vec<Expr>), Return(Option<Expr>),
    If { condition: Expr, body: Vec<Statement>, elseif_clauses: Vec<(Expr, Vec<Statement>)>, else_body: Option<Vec<Statement>> },
    While { condition: Expr, body: Vec<Statement> }, DoWhile { body: Vec<Statement>, condition: Expr },
    For { init: Vec<Expr>, condition: Vec<Expr>, update: Vec<Expr>, body: Vec<Statement> },
    Foreach { expr: Expr, key: Option<Expr>, value: Expr, by_ref: bool, body: Vec<Statement> },
    Switch { expr: Expr, cases: Vec<SwitchCase> }, Break(Option<Expr>), Continue(Option<Expr>),
    FunctionDecl { name: Vec<u8>, params: Vec<Param>, return_type: Option<TypeHint>, body: Vec<Statement>, is_static: bool, attributes: Vec<Attribute> },
    ClassDecl { name: Vec<u8>, modifiers: ClassModifiers, extends: Option<Vec<u8>>, implements: Vec<Vec<u8>>, body: Vec<ClassMember>, enum_backing_type: Option<Vec<u8>>, attributes: Vec<Attribute> },
    TryCatch { try_body: Vec<Statement>, catches: Vec<CatchClause>, finally_body: Option<Vec<Statement>> },
    Throw(Expr), Global(Vec<Vec<u8>>), StaticVar(Vec<(Vec<u8>, Option<Expr>)>), Unset(Vec<Expr>),
    Declare { directives: Vec<(Vec<u8>, Expr)>, body: Option<Vec<Statement>> },
    NamespaceDecl { name: Option<Vec<Vec<u8>>>, body: Option<Vec<Statement>> },
    UseDecl(Vec<UseItem>), Label(Vec<u8>), Goto(Vec<u8>), Nop,
}
#[derive(Debug, Clone)]
pub struct Expr { pub kind: ExprKind, pub span: Span }
#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64), Float(f64), String(Vec<u8>), InterpolatedString(Vec<StringPart>), True, False, Null, Array(Vec<ArrayElement>),
    Variable(Vec<u8>), DynamicVariable(Box<Expr>),
    BinaryOp { op: BinaryOp, left: Box<Expr>, right: Box<Expr> }, UnaryOp { op: UnaryOp, operand: Box<Expr>, prefix: bool },
    Assign { target: Box<Expr>, value: Box<Expr> }, CompoundAssign { op: BinaryOp, target: Box<Expr>, value: Box<Expr> }, AssignRef { target: Box<Expr>, value: Box<Expr> },
    PropertyAccess { object: Box<Expr>, property: Box<Expr>, nullsafe: bool }, StaticPropertyAccess { class: Box<Expr>, property: Vec<u8> }, DynamicStaticPropertyAccess { class: Box<Expr>, property: Box<Expr> },
    MethodCall { object: Box<Expr>, method: Box<Expr>, args: Vec<Argument>, nullsafe: bool }, StaticMethodCall { class: Box<Expr>, method: Vec<u8>, args: Vec<Argument> },
    DynamicStaticMethodCall { class: Box<Expr>, method: Box<Expr>, args: Vec<Argument> }, ArrayAccess { array: Box<Expr>, index: Option<Box<Expr>> },
    FunctionCall { name: Box<Expr>, args: Vec<Argument> }, Ternary { condition: Box<Expr>, if_true: Option<Box<Expr>>, if_false: Box<Expr> },
    NullCoalesce { left: Box<Expr>, right: Box<Expr> }, Match { subject: Box<Expr>, arms: Vec<MatchArm> },
    Closure { is_static: bool, params: Vec<Param>, use_vars: Vec<ClosureUse>, return_type: Option<TypeHint>, body: Vec<Statement>, attributes: Vec<Attribute> },
    ArrowFunction { is_static: bool, params: Vec<Param>, return_type: Option<TypeHint>, body: Box<Expr>, attributes: Vec<Attribute> },
    New { class: Box<Expr>, args: Vec<Argument> }, Instanceof { expr: Box<Expr>, class: Box<Expr> },
    Cast(CastType, Box<Expr>), Print(Box<Expr>), Exit(Option<Box<Expr>>), Empty(Box<Expr>), Isset(Vec<Expr>), Eval(Box<Expr>),
    Include { kind: IncludeKind, path: Box<Expr> }, Yield(Option<Box<Expr>>, Option<Box<Expr>>), YieldFrom(Box<Expr>),
    Clone(Box<Expr>), CloneWith { object: Box<Expr>, with_args: Vec<(Vec<u8>, Expr)> }, Spread(Box<Expr>), Pipe { value: Box<Expr>, callable: Box<Expr> },
    ConstantAccess(Vec<Vec<u8>>), ClassConstAccess { class: Box<Expr>, constant: Vec<u8> }, DynamicClassConstAccess { class: Box<Expr>, constant: Box<Expr> },
    ThrowExpr(Box<Expr>), Suppress(Box<Expr>), Identifier(Vec<u8>), FirstClassCallable(CallableTarget),
}
#[derive(Debug, Clone)]
pub enum CallableTarget { Function(Box<Expr>), Method { object: Box<Expr>, method: Box<Expr>, nullsafe: bool }, StaticMethod { class: Box<Expr>, method: Vec<u8> } }
#[derive(Debug, Clone)]
pub enum StringPart { Literal(Vec<u8>), Expr(Expr) }
#[derive(Debug, Clone)]
pub struct ArrayElement { pub key: Option<Expr>, pub value: Expr, pub unpack: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp { Add, Sub, Mul, Div, Mod, Pow, Concat, BitwiseAnd, BitwiseOr, BitwiseXor, ShiftLeft, ShiftRight, BooleanAnd, BooleanOr, LogicalAnd, LogicalOr, LogicalXor, Equal, Identical, NotEqual, NotIdentical, Less, Greater, LessEqual, GreaterEqual, Spaceship }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp { Negate, Plus, BitwiseNot, BooleanNot, PreIncrement, PreDecrement, PostIncrement, PostDecrement }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastType { Int, Float, String, Bool, Array, Object, Unset, Void }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind { Include, IncludeOnce, Require, RequireOnce }
#[derive(Debug, Clone)]
pub struct Argument { pub name: Option<Vec<u8>>, pub value: Expr, pub unpack: bool }
#[derive(Debug, Clone)]
pub struct Param { pub name: Vec<u8>, pub type_hint: Option<TypeHint>, pub default: Option<Expr>, pub by_ref: bool, pub variadic: bool, pub visibility: Option<Visibility>, pub set_visibility: Option<Visibility>, pub readonly: bool, pub is_final: bool, pub attributes: Vec<Attribute>, pub get_hook: Option<Vec<Statement>>, pub set_hook: Option<(Vec<u8>, Vec<Statement>)>, pub get_hook_final: bool, pub set_hook_final: bool, pub get_hook_abstract: bool, pub set_hook_abstract: bool }
#[derive(Debug, Clone)]
pub enum TypeHint { Simple(Vec<u8>), Nullable(Box<TypeHint>), Union(Vec<TypeHint>), Intersection(Vec<TypeHint>) }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility { Public, Protected, Private }
#[derive(Debug, Clone)]
pub struct SwitchCase { pub value: Option<Expr>, pub body: Vec<Statement> }
#[derive(Debug, Clone)]
pub struct MatchArm { pub conditions: Option<Vec<Expr>>, pub body: Expr }
#[derive(Debug, Clone)]
pub struct CatchClause { pub types: Vec<Vec<Vec<u8>>>, pub variable: Option<Vec<u8>>, pub body: Vec<Statement> }
#[derive(Debug, Clone)]
pub struct ClosureUse { pub variable: Vec<u8>, pub by_ref: bool }
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassModifiers { pub is_abstract: bool, pub is_final: bool, pub is_readonly: bool, pub is_interface: bool, pub is_trait: bool, pub is_enum: bool }
#[derive(Debug, Clone)]
pub enum ClassMember {
    Property { name: Vec<u8>, type_hint: Option<TypeHint>, default: Option<Expr>, visibility: Visibility, set_visibility: Option<Visibility>, is_static: bool, is_readonly: bool, is_abstract: bool, is_final: bool, get_hook: Option<Vec<Statement>>, set_hook: Option<(Vec<u8>, Vec<Statement>)>, get_hook_final: bool, set_hook_final: bool, get_hook_abstract: bool, set_hook_abstract: bool, attributes: Vec<Attribute> },
    Method { name: Vec<u8>, params: Vec<Param>, return_type: Option<TypeHint>, body: Option<Vec<Statement>>, visibility: Visibility, is_static: bool, is_abstract: bool, is_final: bool, line: u32, attributes: Vec<Attribute> },
    ClassConstant { name: Vec<u8>, value: Expr, visibility: Visibility, is_final: bool, attributes: Vec<Attribute> },
    TraitUse { traits: Vec<Vec<u8>>, adaptations: Vec<TraitAdaptation> },
    EnumCase { name: Vec<u8>, value: Option<Expr>, attributes: Vec<Attribute> },
}
#[derive(Debug, Clone)]
pub enum TraitAdaptation { Alias { trait_name: Option<Vec<u8>>, method: Vec<u8>, new_name: Option<Vec<u8>>, new_visibility: Option<Visibility> }, Precedence { trait_name: Vec<u8>, method: Vec<u8>, instead_of: Vec<Vec<u8>> } }
#[derive(Debug, Clone)]
pub struct UseItem { pub name: Vec<Vec<u8>>, pub alias: Option<Vec<u8>>, pub kind: UseKind }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind { Normal, Function, Constant }
