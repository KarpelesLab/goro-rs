use crate::value::Value;

/// Represents a parameter type constraint parsed from a type hint
#[derive(Debug, Clone)]
pub enum ParamType {
    /// A simple type: int, float, string, bool, array, object, callable, mixed, null, void, iterable, self, parent, static, false, true, never
    Simple(Vec<u8>),
    /// ?Type — allows null in addition to the inner type
    Nullable(Box<ParamType>),
    /// Type1|Type2 — union type
    Union(Vec<ParamType>),
    /// Type1&Type2 — intersection type
    Intersection(Vec<ParamType>),
}

/// Information about a parameter's declared type
#[derive(Debug, Clone)]
pub struct ParamTypeInfo {
    /// The declared type constraint
    pub param_type: ParamType,
    /// The parameter name (for error messages)
    pub param_name: Vec<u8>,
    /// Whether this parameter was implicitly made nullable (Type $x = null)
    pub implicitly_nullable: bool,
}

/// Operand type (how to interpret the operand index)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandType {
    /// Not used
    Unused,
    /// Compiled variable (named $var, persists for function scope)
    Cv(u32),
    /// Constant value from the literal pool
    Const(u32),
    /// Temporary (short-lived, no references)
    Tmp(u32),
    /// Jump target (instruction index)
    JmpTarget(u32),
}

/// A single bytecode instruction
#[derive(Debug, Clone)]
pub struct Op {
    pub opcode: OpCode,
    pub op1: OperandType,
    pub op2: OperandType,
    pub result: OperandType,
    pub line: u32,
}

/// Opcode types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    /// No operation
    Nop,
    /// Release temporaries [op1 Tmp, op2 Tmp) at the end of an expression
    /// statement (PHP frees TMP_VARs right after their last use).
    FreeTmps,

    // ---- Arithmetic ----
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Negate,

    // ---- Bitwise ----
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseNot,
    ShiftLeft,
    ShiftRight,

    // ---- Comparison ----
    Equal,
    NotEqual,
    Identical,
    NotIdentical,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Spaceship,

    // ---- Logical ----
    BooleanNot,
    BoolXor,

    // ---- Assignment ----
    /// Assign op2 to CV(op1), result = value
    Assign,
    /// Assign by reference: both CV(op1) and CV(op2) share the same Rc<RefCell<Value>>
    AssignRef,
    /// Compound assign: CV(op1) op= op2
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignMod,
    AssignPow,
    AssignConcat,
    AssignBitwiseAnd,
    AssignBitwiseOr,
    AssignBitwiseXor,
    AssignShiftLeft,
    AssignShiftRight,

    // ---- Unary numeric ----
    /// Unary plus: coerce to number, preserving negative zero
    UnaryPlus,

    // ---- Increment/Decrement ----
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,

    // ---- Control flow ----
    /// Unconditional jump to op1
    Jmp,
    /// Jump to op2 if op1 is false
    JmpZ,
    /// Jump to op2 if op1 is true
    JmpNz,

    // ---- Output ----
    /// Echo op1
    Echo,

    // ---- Function calls ----
    /// Initialize a function call: op1 = function name (const)
    InitFCall,
    /// Initialize a dynamic static call: op1 = class expr, op2 = method name (const), result = arg count (const)
    InitDynamicStaticCall,
    /// Send argument: op1 = value, op2 = arg position
    SendVal,
    /// Send argument preserving Reference wrapper: op1 = value, op2 = arg position
    /// Used for var_dump/print_r so they can display & prefix for references
    SendValRaw,
    /// Send named argument: op1 = value, op2 = argument name (const)
    SendNamedVal,
    /// Unpack array argument: op1 = array value (each element becomes a separate arg)
    SendUnpack,
    /// Execute the function call, result = return value
    DoFCall,
    /// Return op1 from current function
    Return,
    /// Declare a user function: op1 = name (const), op2 = function OpArray index (const)
    DeclareFunction,
    /// Wrap the internal callable in op1 into a new `Closure` object: result = object
    MakeClosure,
    /// First-class callable `f(...)`: op1 = fallback wrapper closure (internal
    /// form), op2 = target descriptor (const string), result = Closure object
    MakeFcc,
    /// Register attributes for a top-level constant. op1 = index into the
    /// current OpArray's `const_attribute_table` (const literal of type Long).
    RegisterConstAttrs,

    // ---- Variables ----
    /// Load a constant value into a temporary
    LoadConst,
    /// Runtime constant lookup: op1 = name (const), result = value
    ConstLookup,
    /// Include/require file: op1 = path, result = return value
    IncludeFile,
    /// Initialize a static variable: op1 = CV, op2 = default value (const), result = static key name (const)
    StaticVarInit,
    /// `static $x = <non-constant expr>;` guard (ZEND_BIND_INIT_STATIC_OR_JMP):
    /// jump to op2 (the matching StaticVarInit, which binds) when the static
    /// named by result (const key) is already initialized, skipping the
    /// initializer. op1 = CV.
    StaticVarInitOrJmp,
    /// Bind a global variable: op1 = CV, op2 = variable name (const)
    BindGlobal,

    // ---- Cast ----
    CastInt,
    CastFloat,
    CastString,
    CastBool,
    CastArray,
    CastObject,

    // ---- Array ----
    /// Create a new empty array, result = new array
    ArrayNew,
    /// Append value to array: op1 = array (CV), op2 = value
    ArrayAppend,
    /// Set array element: op1 = array (CV), op2 = value, extended_value = key const index
    ArraySet,
    /// Read array element: result = op1[op2]
    ArrayGet,
    /// Read array element for read-write context (compound assignment):
    /// like ArrayGet but throws PHP errors instead of warnings when the
    /// container or key is invalid for writing (matches FETCH_DIM_RW).
    /// - array key of type array/object -> TypeError
    /// - scalar container (true/int/float) -> Error "Cannot use a scalar value as an array"
    /// - undefined keys are not warned (the write side will create them)
    ArrayGetRW,
    /// List destructure array element: result = op1[op2] (emits "Cannot use X as array" instead of "Trying to access array offset")
    ListGet,

    // ---- Foreach ----
    /// Initialize foreach: op1 = array, result = iterator state (tmp)
    ForeachInit,
    /// Fetch next value: op1 = iterator (tmp), result = value (tmp), op2 = jump target if done
    ForeachNext,
    /// Fetch key for current iteration: op1 = iterator (tmp), result = key (tmp)
    ForeachKey,
    /// Initialize foreach by-ref: op1 = array, result = iterator state (tmp)
    ForeachInitRef,
    /// Fetch next value by-ref: op1 = iterator (tmp), result = reference (tmp), op2 = jump target if done
    ForeachNextRef,

    // ---- OOP ----
    /// Create a new object: op1 = class name (const), result = object (tmp)
    NewObject,
    /// Get property: op1 = object, op2 = property name (const), result = value (tmp)
    PropertyGet,
    /// Get property for read-write context (compound assignment): like
    /// PropertyGet but throws PHP Error instead of warning when the LHS
    /// is a scalar (matches FETCH_OBJ_RW).
    PropertyGetRW,
    /// Set property: op1 = object, op2 = value, result = property name (const)
    PropertySet,
    /// Call method: op1 = object, op2 = method name (const)
    InitMethodCall,
    /// Declare a class: op1 = class name (const), op2 = class def index (const)
    DeclareClass,
    /// Get static property OR class constant: op1 = class name (const),
    /// op2 = prop/const name (const), result = value. Used for `C::FOO`.
    StaticPropGet,
    /// Fetch static property only (no constant fallback): op1 = class name,
    /// op2 = prop name (const), result = value. Used for `C::$p`. Errors
    /// with "Access to undeclared static property" if not declared.
    StaticPropFetch,
    /// Set static property: op1 = class name (const), op2 = value, result = prop name (const)
    StaticPropSet,
    /// Isset check for static property: op1 = class name, op2 = prop name,
    /// result = bool (Value::True/Value::False). Never throws.
    StaticPropIsset,
    /// StaticPropFetch for a read-modify-write (compound assignment, ++/--).
    StaticPropFetchRW,
    /// Fetch a static property as the container of a dimension write
    /// (null / unset is promoted to an array): op1 = class, op2 = name.
    StaticPropFetchW,
    /// Fetch a reference to a static property slot: op1 = class,
    /// op2 = name, result = tmp holding the Reference.
    StaticPropGetRef,
    /// Bind a static property to a reference: op1 = class, op2 = value
    /// (a Reference binds, anything else assigns), result = name.
    StaticPropSetRef,
    /// unset(C::$p): op1 = class, op2 = name. Always throws.
    StaticPropUnset,
    /// Throw "Using $this when not in object context" unless the `$this`
    /// CV (op1) holds an object.
    CheckThis,
    /// In a constant expression, a property fetch on an object (op1) that
    /// is not an enum throws; with op2 set, a dimension fetch on any object.
    ConstExprPropCheck,

    // ---- Exceptions ----
    /// Throw: op1 = exception value. Sets VM exception state.
    Throw,
    /// Begin try block: op1 = jump target for catch, op2 = jump target for finally
    TryBegin,
    /// End try/catch, clear exception handler
    TryEnd,
    /// Catch: store the current exception into CV. op1 = CV target
    CatchException,

    // ---- Type checking ----
    TypeCheck,

    // ---- String interpolation ----
    /// Concatenate multiple values: fast concat of op1 and op2
    FastConcat,

    // ---- Clone ----
    /// Clone an object: op1 = source, result = cloned copy
    CloneObj,
    /// Get class name from object: op1 = object, result = class name string
    GetClassName,
    /// Check if object property is set (calls __isset if available)
    PropertyIsset,
    /// The container fetch of a nested isset (`isset($o->p[k])`,
    /// `isset($o->p->q)`): like PropertyIsset, gating the PropertyGet that
    /// follows it, with FETCH_OBJ_IS semantics: __get() is reached when
    /// __isset() is absent or already running for the property.
    PropertyIssetFetch,

    // ---- Print ----
    /// print expr (returns 1)
    Print,

    // ---- Generator ----
    /// Yield a value: op1 = value (or Unused), op2 = key (or Unused), result = where to store sent value
    Yield,
    /// Generator return (like Return but for generators)
    GeneratorReturn,
    /// Yield from: op1 = iterable, result = return value of inner generator
    YieldFrom,

    // ---- Unset ----
    /// Unset a CV variable (direct replace, breaks reference links): op1 = CV
    UnsetCv,
    /// Remove array element: op1 = array, op2 = key
    ArrayUnset,
    /// Remove object property: op1 = object, op2 = property name
    PropertyUnset,
    /// Throw UnhandledMatchError with the unmatched value: op1 = subject value
    MatchError,
    /// Spread array into another array: op1 = target array, op2 = source array
    ArraySpread,
    /// isset() check: result = True if op1 is not Null and not Undef
    IssetCheck,
    /// isset() on array dimension: op1 = array/object, op2 = key, result = True/False
    /// For objects implementing ArrayAccess, calls offsetExists() instead of offsetGet()
    ArrayIsset,
    /// ArrayIsset as the probe of a quiet dimension fetch (FETCH_DIM_IS:
    /// `??` and the outer dimensions of isset()/empty()): an illegal
    /// offset reports "Cannot access offset of type X on array".
    ArrayIssetFetch,
    /// Make a CV into a reference (wrap in Rc<RefCell>): op1 = CV to make ref
    MakeRef,
    /// Record the exception-handler stack depth at a loop entry: result = tmp.
    SaveHandlerDepth,
    /// Release a loop temporary when return/break leaves the loop
    /// (FE_FREE with ZEND_FREE_ON_RETURN): op1 = tmp, op2 = tmp holding the
    /// loop's SaveHandlerDepth. An exception thrown by a destructor run here
    /// is dispatched from the loop's level.
    FreeLoopVar,
    /// Write-fetch of a CV passed to a builtin's `array &$array` parameter
    /// (goro passes those by shared handle): an undefined CV becomes null
    /// silently, as ZEND_SEND_REF would. op1 = CV.
    InitCvNull,
    /// Verify a parameter CV after its non-constant default value was
    /// assigned (RECV_INIT type check): op1 = the parameter's CV.
    RecvDefaultCheck,
    /// Send a value as a reference (for by-ref function args like preg_match &$matches)
    SendRef,
    /// Append to array without deref (preserves Reference wrapper): op1 = array, op2 = CV
    ArrayAppendRef,
    /// Set array element as reference: op1 = array CV, op2 = value CV, result = key
    ArraySetRef,
    /// Get or create a reference for an array element: op1 = array CV, op2 = key, result = Reference tmp
    ArrayGetRef,
    /// Emit a Notice: op1 = message string literal
    EmitNotice,
    /// Throw an Error: op1 = message string literal
    ThrowErrorMsg,
    /// Dynamic variable read: op1 = name expr, result = value
    VarVarGet,
    /// Dynamic variable write: op1 = name expr, op2 = value
    VarVarSet,
    /// Dynamic variable fetch for writing (FETCH_W / FETCH_RW / FETCH_UNSET):
    /// op1 = name expr, op2 = Const mode (0 = W, 1 = RW, 2 = UNSET),
    /// result = CV or tmp receiving the variable's reference cell.
    VarVarFetch,
    /// unset($$name): op1 = name expr
    VarVarUnset,
    /// $$name = &source: op1 = name expr, op2 = source (CV, or tmp holding a
    /// reference), result = value
    VarVarAssignRef,
    /// Save return value for deferred return (finally blocks): op1 = value
    SaveReturn,
    /// Save jump target for deferred break/continue (finally blocks): op1 = JmpTarget
    SaveJump,
    /// Check and execute deferred return or jump after finally block
    /// (ZEND_FAST_RET): op1 = finally start, op2 = the finally's slot tmp
    ReturnDeferred,
    /// Start of a finally block: moves what led here (the pending exception,
    /// deferred return value or jump) into the slot tmp op1 (the fast_call
    /// variable of ZEND_FAST_CALL / ZEND_HANDLE_EXCEPTION).
    FinallyEnter,
    /// Save and suppress error reporting (for @ operator)
    ErrorSuppress,
    /// Restore error reporting (for @ operator)
    ErrorRestore,
    /// Eval: op1 = code string, result = return value
    Eval,
    /// Extract: op1 = array value, op2 = flags (const), result = count of extracted vars
    /// Iterates over array keys and sets corresponding CVs in the current scope
    Extract,
    /// compact() set: op1 = array (tmp), op2 = CV, result = key (const)
    /// Like ArraySet but skips if CV is Undef and emits a warning
    CompactSet,
    /// compact(...): op1 = tmp array of the call's arguments, result = tmp array
    /// of the named variables of the current scope (php_compact_var semantics).
    CompactDyn,
    /// get_defined_vars(): result = array of the current scope's variables
    GetDefinedVars,
    /// ZEND_TICKS: op1 = declare(ticks) value (const)
    Tick,
    /// exit/die: op1 = value. If int, set exit code; if string, echo the string.
    ExitOp,
    /// Get (creating) a reference to an object property: op1 = object,
    /// op2 = property name, result = tmp holding the Reference
    PropertyGetRef,
    /// Bind an object property to a reference: op1 = object, op2 = value
    /// (CV made a reference, or a tmp Reference), result = property name
    PropertySetRef,
    /// Fetch a property for a dimension write (`$obj->p[...] = v`): like
    /// PropertyGet, but a missing/null property is initialised to an array,
    /// a shared array is separated, and readonly properties are rejected.
    PropertyGetW,
    /// Fetch the object of a nested property write (`$a->b->c = v`,
    /// FETCH_OBJ_W): behaves like PropertyGet for ordinary objects, while
    /// extension objects (SimpleXML) get a write fetch that may create
    /// the property.
    PropertyGetObjW,
    /// Fetch `$a[k]` as the object of a property write (FETCH_DIM_W before
    /// ASSIGN_OBJ): like ArrayGet for arrays; extension objects get a write
    /// fetch (SimpleXML creates the element).
    ArrayGetObjW,
    /// Jump to op1 (JmpTarget) if the pending call's next positional
    /// parameter is by-reference (runtime FUNC_ARG dispatch).
    JmpIfArgByRef,
    /// Fetch an array element for a nested write (`$a[k][...] = v`):
    /// op1 = container (CV or tmp array), op2 = key (Unused = append),
    /// result = tmp holding the (vivified, separated) element array.
    ArrayGetW,
    /// Copy op1's value (dereferenced, no undefined-variable warning) into
    /// the result tmp: snapshots an operand that a magic method could change
    /// between two uses (`$o->$name ?? x` calls __isset then __get).
    QmAssign,
    /// Like ArrayGetW for the containers of a compound assignment /
    /// increment (`$a[k][j] op= v`, FETCH_DIM_RW): undefined variables and
    /// keys are warned about before being created.
    ArrayGetWRW,
    /// Like ArrayGetW for the containers of `unset($a[k][j])`
    /// (FETCH_DIM_UNSET): nothing is created; a missing element yields null.
    ArrayGetWUnset,
    /// SendVal of a non-variable argument (op1, position op2) whose callee
    /// may take it by reference: result = Const(0) for a call result
    /// (SEND_VAR_NO_REF_EX: "Only variables should be passed by reference"
    /// notice), Const(1) for another expression (SEND_VAL_EX: Error).
    SendValChecked,
    /// Read side of `$a[k]++` (FETCH_DIM_RW for an increment): like
    /// ArrayGetRW, but on ArrayAccess objects the by-value offsetGet() result
    /// is reported as an indirect modification.
    ArrayGetIncDec,
    /// Write-back of `$a[k]++`: like ArraySet, but a no-op on objects.
    ArraySetIncDec,
    /// FETCH_DIM_W for the container of an object write (`$a[k]->p = v`):
    /// a missing element is created as null (no array vivification).
    ArrayGetWObj,
    /// FETCH_DIM_RW for the container of an object read-modify-write
    /// (`$a[k]->p += v`): a missing element is warned about, then created
    /// as null.
    ArrayGetWObjRW,
    /// FETCH_OBJ_W for the container of an object write (`$o->a->b = v`):
    /// a missing property is created as null; non-objects throw "Attempt
    /// to modify property".
    PropertyGetWObj,
    /// FETCH_OBJ_RW for the container of a compound object write
    /// (`$o->a->b op= v`): like PropertyGetWObj, but an undefined property
    /// warns and an uninitialized typed one throws.
    PropertyGetWObjRW,
    /// PropertyGet for empty() / isset() (FETCH_OBJ_IS): an overloaded
    /// property is checked with __isset() before __get() is called.
    PropertyGetIs,
    /// The read of `$obj->p++` / `--$obj->p` (PRE/POST_INC_OBJ): like
    /// PropertyGet, but a non-object throws "Attempt to increment/decrement
    /// property" first.
    PropertyGetIncDec,
    /// Like PropertyGetW for the container of a compound assignment
    /// (`$o->p[k] op= v`, FETCH_OBJ_RW): an undefined property is warned about.
    PropertyGetWRW,
    /// Like PropertyGetW for `unset($o->p[k])` (FETCH_OBJ_UNSET): a missing
    /// property is created as null instead of an array.
    PropertyGetWUnset,
}

/// A compiled function / script
#[derive(Debug, Clone)]
pub struct OpArray {
    /// The bytecodes
    pub ops: Vec<Op>,
    /// Constant pool (literal values)
    pub literals: Vec<Value>,
    /// Compiled variable names (for named variables like $x, $y)
    pub cv_names: Vec<Vec<u8>>,
    /// Number of temporary slots needed
    pub temp_count: u32,
    /// Function name (empty for top-level script)
    pub name: Vec<u8>,
    /// Number of required/declared parameters (for variadic handling)
    pub param_count: u32,
    /// Number of required parameters (params without defaults)
    pub required_param_count: u32,
    /// If set, the last param is variadic (...$args) at this CV index
    pub variadic_param: Option<u32>,
    /// Nested function OpArrays (for DeclareFunction)
    /// (shared: declaring one registers it without a copy, and a cached
    /// script's functions are those of every request)
    pub child_functions: Vec<std::rc::Rc<OpArray>>,
    /// Whether this function is a generator (contains yield)
    pub is_generator: bool,
    /// Parameter type information (indexed by CV index, None if no type hint)
    pub param_types: Vec<Option<ParamTypeInfo>>,
    /// Return type constraint (None if no return type declared)
    pub return_type: Option<ParamType>,
    /// Class scope for this function (lowercase). Set for methods and closures defined within a class.
    /// Used to determine visibility access rights at runtime.
    pub scope_class: Option<Vec<u8>>,
    /// Whether this is a static closure (declared with `static function() {}`)
    pub is_static_closure: bool,
    /// Whether this function declared `function &name()` (returns/yields by reference).
    /// For generator functions, this controls whether `foreach ($gen as &$v)` is allowed.
    pub returns_by_ref: bool,
    /// Line number of the function/method declaration (for error reporting)
    pub decl_line: u32,
    /// Line number of the closing brace of the function/method.
    pub end_line: u32,
    /// Doc comment attached to the function/method declaration (if any).
    pub doc_comment: Option<Vec<u8>>,
    /// Literal indices that contain __CLASS__ magic constant values (for trait patching)
    pub class_const_literals: Vec<u32>,
    /// Whether this file/function was declared with declare(strict_types=1)
    pub strict_types: bool,
    /// Which parameters are passed by reference (indexed by parameter position, true = by-ref)
    pub by_ref_params: Vec<bool>,
    /// Which parameters have default values (indexed by parameter position)
    pub param_has_default: Vec<bool>,
    pub attributes: Vec<crate::object::RuntimeAttribute>,
    pub param_attributes: Vec<Vec<crate::object::RuntimeAttribute>>,
    /// Source filename (for closures, error reporting)
    pub filename: Vec<u8>,
    /// `filename` as the shared string the executing file and backtrace
    /// frames hold (see [`OpArray::file_name`]), made on first use.
    pub filename_rc: std::cell::OnceCell<crate::rcstr::RcStr>,
    /// For FCC closures: the original function/method name (e.g. "strrev", "Foo::bar")
    pub fcc_target_name: Option<Vec<u8>>,
    /// Display name shown to users (e.g. `{closure:/path/to/file.php:5}` or
    /// `{closure:Class::method():5}`). Empty for non-closure functions; in that
    /// case the regular `name` field is the display name.
    pub display_name: Vec<u8>,
    /// Attribute payloads for top-level constants, keyed by FQN. Indexed via
    /// `OpCode::RegisterConstAttrs` instructions emitted by the compiler.
    pub const_attribute_table: Vec<(Vec<u8>, Vec<crate::object::RuntimeAttribute>)>,
    /// Classes declared inside this function body (DeclareClass with a negative index).
    pub local_classes: Vec<crate::object::ClassEntry>,
    /// Set on signature-only OpArrays synthesized for internal functions and
    /// methods from php-src stubs (see `crate::stubs`).
    pub internal_sig: Option<&'static crate::stubs::StubFunc>,
    /// Reflection rendering of each parameter's default value (by CV index),
    /// as PHP prints it (evaluated literal or constant-expression AST dump).
    pub param_default_repr: Vec<Option<String>>,
    /// Constant initial values of `static` variables (by the literal index
    /// of their key), shown before the declaration first runs.
    pub static_defaults: Option<Box<Vec<(u32, Value)>>>,
    /// For closures: CV names bound to the captured values of the closure's
    /// capture array, in order (`this` first when captured, then `use` vars).
    pub closure_capture_names: Vec<Vec<u8>>,
    /// For closures: declared parameters as (name, by_ref).
    pub closure_params: Vec<(Vec<u8>, bool)>,
    /// For first-class callables in constant expressions: the target
    /// descriptor resolved when the constant is evaluated (see Vm::make_fcc).
    pub fcc_desc: Option<Vec<u8>>,
    /// For closures without a bound $this: the called class (static::)
    /// captured when the closure was created.
    pub closure_called_class: Option<Vec<u8>>,
    /// What this code's call sites and property accesses found (allocated
    /// on first use: an OpArray stays small, the compiler keeps some in
    /// its recursive frames).
    /// (of the request that made them: see OpArray::sites())
    sites: std::cell::RefCell<Option<Box<SiteCaches>>>,
    /// See OpArray::cv_name_info().
    cv_info: std::cell::OnceCell<CvNameInfo>,
    /// See OpArray::free_mask().
    free_mask: std::cell::OnceCell<FreeInfo>,
    /// See OpArray::lean_kind().
    lean_kind: FreshOnClone<u8>,
}

/// A value an OpArray computes from itself on first use, which a copy of
/// it computes again (a copy is made to be changed: Rc::make_mut()).
#[derive(Debug, Default)]
pub(crate) struct FreshOnClone<T>(std::cell::OnceCell<T>);

impl<T> Clone for FreshOnClone<T> {
    fn clone(&self) -> Self {
        FreshOnClone(std::cell::OnceCell::new())
    }
}

/// The variables a frame binds as it starts: `$GLOBALS` and the auto
/// globals (`$_SERVER`, ...), by CV index.
#[derive(Debug, Clone, Default)]
pub(crate) struct CvNameInfo {
    pub(crate) globals: Option<u32>,
    pub(crate) auto_globals: Box<[u32]>,
    /// No variable is `$GLOBALS` or an auto-global, nor `$this` but the
    /// first (a frame needs none of them set up: see vm::leancall).
    pub(crate) plain_cvs: bool,
    /// How often lately a call of the function ran to its end without a
    /// frame of its own (see vm::leancall): below 0, calls stop trying.
    pub(crate) lean_score: std::cell::Cell<i16>,
}

thread_local! {
    /// The request whose call sites the SiteCaches are for: code kept by
    /// the script cache serves later requests, which start from nothing.
    static SITE_EPOCH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A new request starts (see SITE_EPOCH).
pub fn new_site_epoch() {
    SITE_EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
}

/// See OpArray::sites; entries are by opcode index.
#[derive(Debug, Clone)]
pub(crate) struct SiteCaches {
    /// The SITE_EPOCH they were made in.
    epoch: u32,
    /// What the plain function calls of the INIT_FCALLs resolved to (see
    /// Vm::plain_call_target).
    pub(crate) call_targets: crate::fxhash::FxHashMap<u32, crate::vm::CachedCallTarget>,
    /// The plain property accesses' last class and scope, and whether the
    /// property is a plain one of it there (see Vm::fast_prop_op).
    pub(crate) prop_sites: crate::fxhash::FxHashMap<u32, (Vec<u8>, Option<Vec<u8>>, u8)>,
    /// What the INIT_METHOD_CALLs found (see Vm::init_method_call_fast).
    pub(crate) method_sites: crate::fxhash::FxHashMap<u32, crate::vm::MethodSite>,
    /// What the forwarding calls (self::m(), ...) last resolved to (see
    /// Vm::push_forward_call).
    pub(crate) forward_sites: crate::fxhash::FxHashMap<u32, crate::vm::ForwardSite>,
    /// The INIT_METHOD_CALLs that set up a deferred SPL call: the object's
    /// class name, the pending call's name and whether the method takes
    /// no argument by reference (see Vm::init_spl_method_fast).
    pub(crate) spl_sites: crate::fxhash::FxHashMap<u32, (Vec<u8>, crate::string::PhpString, bool)>,
    /// The template the NEWs of a user class last used (see
    /// Vm::new_object_fast).
    pub(crate) new_sites: crate::fxhash::FxHashMap<u32, std::rc::Rc<crate::object::NewTemplate>>,
    /// The unqualified namespaced function calls / `f(...)` callables that
    /// fell back to the global function: the namespaced (lowercase) name.
    /// Like the runtime cache of INIT_NS_FCALL_BY_NAME, a function of that
    /// name declared later is not seen by the site.
    pub(crate) ns_fallbacks: crate::fxhash::FxHashMap<u32, Vec<u8>>,
    /// What a class constant fetch (`A::C`, `self::C`, `A::class`) found,
    /// for the class scope it ran in (see Vm::fast_class_const).
    pub(crate) const_sites: crate::fxhash::FxHashMap<u32, (Option<Vec<u8>>, Value)>,
}

impl Default for SiteCaches {
    fn default() -> Self {
        SiteCaches {
            epoch: SITE_EPOCH.with(|e| e.get()),
            call_targets: Default::default(),
            prop_sites: Default::default(),
            method_sites: Default::default(),
            forward_sites: Default::default(),
            spl_sites: Default::default(),
            new_sites: Default::default(),
            ns_fallbacks: Default::default(),
            const_sites: Default::default(),
        }
    }
}

impl OpArray {
    /// Whether calls of this code may run without a frame of their own (see
    /// vm::leancall): 1 for a function, 2 for an instance method (its first
    /// variable is `$this`), 0 when its frame has to check or set up
    /// something (typed parameters, a return type, a by-reference return, a
    /// generator, variadics, attributes, static variables, `$GLOBALS` or
    /// auto-globals, internal code, a closure).
    #[inline]
    pub(crate) fn lean_kind(&self) -> u8 {
        match self.lean_kind.0.get() {
            Some(k) => *k,
            None => self.lean_kind_init(),
        }
    }

    #[cold]
    #[inline(never)]
    fn lean_kind_init(&self) -> u8 {
        *self.lean_kind.0.get_or_init(|| {
            let plain = self.return_type.is_none()
                && !self.returns_by_ref
                && !self.is_generator
                && self.variadic_param.is_none()
                && self.attributes.is_empty()
                && self.internal_sig.is_none()
                && self.static_defaults.is_none()
                && !self.name.is_empty()
                && self.param_types.iter().all(|t| t.is_none())
                && !crate::closure::is_closure_fn_name(&self.name)
                && self.cv_name_info().plain_cvs;
            match (plain, self.cv_names.first().map(|n| n.as_slice())) {
                (false, _) => 0,
                (true, Some(b"this")) => 2,
                (true, _) => 1,
            }
        })
    }

    /// Which of the CVs are `$GLOBALS` and the auto globals (`cv_names`
    /// is complete once the code runs).
    #[inline]
    /// Per opcode, the temporaries it is the last user of (bit 0: op1, bit
    /// 1: op2, bit 2: result): Zend frees a TMP operand as the op consuming
    /// it runs, so the VM releases an object or array such an op leaves in
    /// its temporary right after it (instead of at the statement's end).
    /// Empty when no opcode releases one, or `ops` changed since it was
    /// computed.
    pub(crate) fn free_mask(&self) -> &[u8] {
        let m = &self.free_mask.get_or_init(|| compute_free_mask(self)).mask;
        if m.len() == self.ops.len() { m } else { &[] }
    }

    /// The temporary holding the callable (a Closure) of the call DoFCall
    /// `p` completes, released as the call ends (free_mask() bit 4).
    pub(crate) fn call_end_free(&self, p: usize) -> Option<u32> {
        let calls = &self.free_mask.get()?.call_frees;
        let i = calls.binary_search_by_key(&(p as u32), |c| c.0).ok()?;
        Some(calls[i].1)
    }

    pub(crate) fn cv_name_info(&self) -> &CvNameInfo {
        self.cv_info.get_or_init(|| CvNameInfo {
            globals: self
                .cv_names
                .iter()
                .position(|n| n == b"GLOBALS")
                .map(|i| i as u32),
            auto_globals: (0..self.cv_names.len() as u32)
                .filter(|&i| crate::vm::is_auto_global_name(&self.cv_names[i as usize]))
                .collect(),
            plain_cvs: !self.cv_names.iter().enumerate().any(|(i, n)| {
                (i > 0 && n.as_slice() == b"this")
                    || n.as_slice() == b"GLOBALS"
                    || crate::vm::is_auto_global_name(n)
            }),
            lean_score: std::cell::Cell::new(0),
        })
    }

    /// A listing of the opcodes (and those of the nested functions), for
    /// debugging (`GORO_DUMP_OPS=1` prints the main script's).
    pub fn dump(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let name = String::from_utf8_lossy(self.effective_name());
        let _ = writeln!(
            out,
            "== {} ({} cvs, {} tmps)",
            if name.is_empty() {
                "(main)".into()
            } else {
                name
            },
            self.cv_names.len(),
            self.temp_count
        );
        let operand = |o: &OperandType| match o {
            OperandType::Unused => "-".to_string(),
            OperandType::Cv(i) => format!(
                "${}",
                self.cv_names
                    .get(*i as usize)
                    .map(|n| String::from_utf8_lossy(n).into_owned())
                    .unwrap_or_else(|| format!("cv{}", i))
            ),
            OperandType::Tmp(i) => format!("T{}", i),
            OperandType::Const(i) => match self.literals.get(*i as usize) {
                Some(Value::String(s)) => format!("{:?}", s.to_string_lossy()),
                Some(v) => format!("{:?}", v),
                None => format!("c{}", i),
            },
            OperandType::JmpTarget(t) => format!("->{}", t),
        };
        let free_mask = compute_free_mask(self).mask;
        for (i, op) in self.ops.iter().enumerate() {
            let _ = write!(
                out,
                "{:5} L{:<5} {:?} {} {} {}",
                i,
                op.line,
                op.opcode,
                operand(&op.op1),
                operand(&op.op2),
                operand(&op.result)
            );
            // (the temporaries it releases: see free_mask())
            match free_mask.get(i) {
                Some(&m) if m != 0 => {
                    let _ = writeln!(out, "  [free {:#x}]", m);
                }
                _ => out.push('\n'),
            }
        }
        for f in &self.child_functions {
            out.push_str(&f.dump());
        }
        out
    }

    /// Drop the spare capacity of the vectors compiling filled (the code of
    /// a function stays in memory as long as the function is declared).
    pub fn shrink_to_fit(&mut self) {
        self.ops.shrink_to_fit();
        self.literals.shrink_to_fit();
        self.cv_names.shrink_to_fit();
        self.child_functions.shrink_to_fit();
        self.param_types.shrink_to_fit();
        self.class_const_literals.shrink_to_fit();
        self.by_ref_params.shrink_to_fit();
        self.param_has_default.shrink_to_fit();
        self.param_default_repr.shrink_to_fit();
    }

    /// shrink_to_fit(), as a value.
    pub fn shrunk(mut self) -> OpArray {
        self.shrink_to_fit();
        self
    }

    /// `cv_names` was edited (see cv_name_info()).
    pub(crate) fn cv_names_changed(&mut self) {
        self.cv_info.take();
        self.lean_kind = FreshOnClone::default();
    }

    /// What this code's call sites found in the current request.
    #[inline]
    pub(crate) fn sites(&self) -> &std::cell::RefCell<Option<Box<SiteCaches>>> {
        let stale = match self.sites.try_borrow() {
            Ok(s) => s
                .as_ref()
                .is_some_and(|c| c.epoch != SITE_EPOCH.with(|e| e.get())),
            Err(_) => false,
        };
        if stale {
            *self.sites.borrow_mut() = None;
        }
        &self.sites
    }

    /// `filename` (lossily decoded) as a shared string: entering the
    /// function and its backtrace frames share one copy of it.
    ///
    /// (`filename` is set as the function is compiled, before this is
    /// first called: a name changed later would not be seen.)
    #[inline]
    pub fn file_name(&self) -> crate::rcstr::RcStr {
        let f = self
            .filename_rc
            .get_or_init(|| crate::rcstr::RcStr::from(String::from_utf8_lossy(&self.filename)));
        debug_assert!(f.as_bytes() == self.filename.as_slice() || !f.is_ascii());
        f.clone()
    }

    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            literals: Vec::new(),
            cv_names: Vec::new(),
            temp_count: 0,
            name: Vec::new(),
            param_count: 0,
            required_param_count: 0,
            variadic_param: None,
            child_functions: Vec::new(),
            is_generator: false,
            param_types: Vec::new(),
            return_type: None,
            scope_class: None,
            is_static_closure: false,
            returns_by_ref: false,
            decl_line: 0,
            end_line: 0,
            doc_comment: None,
            class_const_literals: Vec::new(),
            strict_types: false,
            by_ref_params: Vec::new(),
            param_has_default: Vec::new(),
            attributes: Vec::new(),
            fcc_target_name: None,
            param_attributes: Vec::new(),
            filename: Vec::new(),
            filename_rc: std::cell::OnceCell::new(),
            display_name: Vec::new(),
            const_attribute_table: Vec::new(),
            local_classes: Vec::new(),
            internal_sig: None,
            param_default_repr: Vec::new(),
            static_defaults: None,
            closure_capture_names: Vec::new(),
            closure_params: Vec::new(),
            fcc_desc: None,
            closure_called_class: None,
            sites: Default::default(),
            cv_info: Default::default(),
            free_mask: Default::default(),
            lean_kind: Default::default(),
        }
    }

    /// Get the user-visible name. For closures (display_name set), returns the
    /// canonical PHP form `{closure:%s:%d}`. Otherwise returns `name`.
    pub fn effective_name(&self) -> &[u8] {
        if !self.display_name.is_empty() {
            &self.display_name
        } else {
            &self.name
        }
    }

    /// Record the reflection rendering of a parameter's default value.
    pub fn set_param_default_repr(&mut self, cv: u32, repr: Option<String>) {
        let i = cv as usize;
        if self.param_default_repr.len() <= i {
            self.param_default_repr.resize(i + 1, None);
        }
        self.param_default_repr[i] = repr;
    }

    /// Add a literal value to the constant pool, returning its index
    pub fn add_literal(&mut self, value: Value) -> u32 {
        // Compiled string literals are interned strings.
        if let Value::String(s) = &value {
            s.mark_interned();
        }
        let idx = self.literals.len() as u32;
        self.literals.push(value);
        idx
    }

    /// Get or create a CV slot for a variable name
    pub fn get_or_create_cv(&mut self, name: &[u8]) -> u32 {
        if let Some(idx) = self.cv_names.iter().position(|n| n == name) {
            return idx as u32;
        }
        let idx = self.cv_names.len() as u32;
        self.cv_names.push(name.to_vec());
        self.cv_names_changed();
        idx
    }

    /// Allocate a new temporary slot
    pub fn alloc_temp(&mut self) -> u32 {
        let idx = self.temp_count;
        self.temp_count += 1;
        idx
    }

    /// Emit an instruction, returning its index
    pub fn emit(&mut self, op: Op) -> u32 {
        let idx = self.ops.len() as u32;
        self.ops.push(op);
        idx
    }

    /// Get the current instruction index (for jump patching)
    pub fn current_offset(&self) -> u32 {
        self.ops.len() as u32
    }

    /// Patch a jump target
    pub fn patch_jump(&mut self, op_index: u32, target: u32) {
        let op = &mut self.ops[op_index as usize];
        match op.opcode {
            OpCode::Jmp | OpCode::SaveJump | OpCode::JmpIfArgByRef => {
                op.op1 = OperandType::JmpTarget(target)
            }
            OpCode::JmpZ | OpCode::JmpNz | OpCode::ForeachNext | OpCode::ForeachNextRef => {
                op.op2 = OperandType::JmpTarget(target);
            }
            _ => panic!("cannot patch non-jump instruction: {:?}", op.opcode),
        }
    }
}

/// See OpArray::free_mask(). A temporary is released after the last opcode
/// naming it (FreeTmps ranges aside), unless a loop may come back to a read
/// of it: a jump from that opcode or later to a point after its first use
/// and up to its last one (the target of a deferred finally jump counts as
/// jumped to from the end). An opcode that only peeks at its operand (the
/// container of a list() assignment, the callable of a dynamic call) does
/// not release it.
#[derive(Debug, Clone, Default)]
pub(crate) struct FreeInfo {
    mask: Box<[u8]>,
    /// (DoFCall index, callable temporary), by index.
    call_frees: Box<[(u32, u32)]>,
}

#[inline(never)]
fn compute_free_mask(oa: &OpArray) -> FreeInfo {
    let n = oa.ops.len();
    let tc = oa.temp_count as usize;
    if tc == 0 || n == 0 {
        return FreeInfo {
            mask: Box::new([]),
            call_frees: Box::new([]),
        };
    }
    const NONE: u32 = u32::MAX;
    let mut first = vec![NONE; tc];
    let mut last = vec![NONE; tc];
    let mut keep = vec![false; tc];
    // The temporaries holding a compound assignment's read of its target.
    let mut rw_read = vec![false; tc];
    // Latest source of a jump to each opcode.
    let mut jump_src = vec![NONE; n + 1];
    let mut any_jump = false;
    // The calls being set up (the callable temporary of an InitFCall), and
    // the DoFCalls ending one with a temporary callable.
    let mut open_calls: Vec<u32> = Vec::new();
    let mut calls_balanced = true;
    let mut call_ends: Vec<(u32, u32)> = Vec::new();
    for (i, op) in oa.ops.iter().enumerate() {
        match op.opcode {
            OpCode::InitFCall => open_calls.push(match op.op1 {
                OperandType::Tmp(t) => t,
                _ => NONE,
            }),
            OpCode::InitMethodCall | OpCode::InitDynamicStaticCall => open_calls.push(NONE),
            OpCode::DoFCall => match open_calls.pop() {
                Some(NONE) => {}
                Some(t) => call_ends.push((i as u32, t)),
                None => calls_balanced = false,
            },
            _ => {}
        }
        for o in [op.op1, op.op2, op.result] {
            if let OperandType::JmpTarget(k) = o {
                let k = (k as usize).min(n);
                let src = if op.opcode == OpCode::SaveJump {
                    n as u32
                } else {
                    i as u32
                };
                if jump_src[k] == NONE || jump_src[k] < src {
                    jump_src[k] = src;
                }
                any_jump = true;
            }
        }
        if op.opcode == OpCode::FreeTmps {
            continue;
        }
        // (a dynamic call's callable, a Closure, is the call frame's until
        // the call ends)
        let peek = matches!(op.opcode, OpCode::ListGet | OpCode::InitFCall);
        if let OperandType::Tmp(r) = op.result {
            if matches!(
                op.opcode,
                OpCode::ArrayGetRW
                    | OpCode::PropertyGetRW
                    | OpCode::StaticPropFetchRW
                    | OpCode::ArrayGetWObjRW
                    | OpCode::PropertyGetWObjRW
            ) && (r as usize) < tc
            {
                rw_read[r as usize] = true;
            }
        }
        for (slot, o) in [op.op1, op.op2, op.result].into_iter().enumerate() {
            if let OperandType::Tmp(t) = o {
                let t = t as usize;
                if t >= tc {
                    continue;
                }
                if first[t] == NONE {
                    first[t] = i as u32;
                }
                last[t] = i as u32;
                if peek && slot == 0 {
                    keep[t] = true;
                }
            }
        }
    }
    let mut mask = vec![0u8; n];
    for t in 0..tc {
        let (f, l) = (first[t], last[t]);
        if l == NONE || keep[t] {
            continue;
        }
        if any_jump && f < l {
            // A loop back into the live range.
            if l - f > 4096 {
                continue;
            }
            if ((f as usize + 1)..=(l as usize)).any(|k| jump_src[k] != NONE && jump_src[k] >= l) {
                continue;
            }
        }
        let op = &oa.ops[l as usize];
        // (a returned value leaves with the frame)
        if matches!(op.opcode, OpCode::Return) {
            continue;
        }
        // ASSIGN_DIM_OP / ASSIGN_OBJ_OP / ASSIGN_STATIC_PROP_OP free their
        // operand after the write: the operand of `target_read op value`
        // followed by the write of its result goes with the write (bit 3:
        // the previous opcode's op2).
        if let (OperandType::Tmp(r), OperandType::Tmp(v), OperandType::Tmp(res)) =
            (op.op1, op.op2, op.result)
        {
            if v as usize == t && (r as usize) < tc && rw_read[r as usize] {
                if let Some(next) = oa.ops.get(l as usize + 1) {
                    if next.op1 == OperandType::Tmp(res) || next.op2 == OperandType::Tmp(res) {
                        mask[l as usize + 1] |= 8;
                        continue;
                    }
                }
            }
        }
        let mut bits = 0u8;
        for (b, o) in [op.op1, op.op2, op.result].into_iter().enumerate() {
            if o == OperandType::Tmp(t as u32) {
                bits |= 1 << b;
            }
        }
        mask[l as usize] |= bits;
    }
    // A Closure called from a temporary goes as the call ends (when nothing
    // after the call setup reads it again).
    let mut call_frees = Vec::new();
    if calls_balanced && open_calls.is_empty() {
        for (d, t) in call_ends {
            let tu = t as usize;
            if tu < tc
                && keep[tu]
                && oa
                    .ops
                    .get(last[tu] as usize)
                    .is_some_and(|o| o.opcode == OpCode::InitFCall)
                && last[tu] < d
            {
                mask[d as usize] |= 16;
                call_frees.push((d, t));
            }
        }
    }
    // (none: no mask, whose lookups the dispatch loop then skips at once)
    if mask.iter().all(|&m| m == 0) {
        mask = Vec::new();
    }
    FreeInfo {
        mask: mask.into_boxed_slice(),
        call_frees: call_frees.into_boxed_slice(),
    }
}

impl Default for OpArray {
    fn default() -> Self {
        Self::new()
    }
}
