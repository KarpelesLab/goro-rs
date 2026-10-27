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

/// The result operand of the FREE_TMPS that ends a statement: no
/// temporary from its op1 on is live past it on any path (the compiler
/// reuses them for the next statements, see compute_free_mask).
pub const STMT_END: OperandType = OperandType::Const(u32::MAX);

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
/// (numbered in declaration order: see OpCode::from_u8)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
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
    /// ZEND_INIT_USER_CALL's check of `call_user_func($cb, ...)` /
    /// `call_user_func_array($cb, ...)`, right after $cb is sent (before
    /// the other arguments are evaluated): a callback that is not callable
    /// throws its TypeError. op1 = the function name literal.
    CufCheck,
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
    /// Like StaticPropFetchW for `unset(C::$p[k])` (FETCH_STATIC_PROP_UNSET):
    /// a null / uninitialized property is left alone.
    StaticPropFetchUnset,
    // (new opcodes go above: LAST_OPCODE names the last one)
}

/// The opcode numbered highest (the last declared).
const LAST_OPCODE: OpCode = OpCode::StaticPropFetchUnset;

impl OpCode {
    /// The opcode numbered `n` (`op as u8`), as the file cache stores it.
    pub fn from_u8(n: u8) -> Option<OpCode> {
        if n > LAST_OPCODE as u8 {
            return None;
        }
        // SAFETY: OpCode is a fieldless repr(u8) enum without explicit
        // discriminants, numbered 0..=LAST_OPCODE.
        Some(unsafe { std::mem::transmute::<u8, OpCode>(n) })
    }
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
    /// The next temporary alloc_temp() hands out (while compiling: the
    /// temporaries of a finished statement are reused, see STMT_END;
    /// temp_count is the most the code uses at once).
    pub temp_next: u32,
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
    /// Literal indices of the constant names of class constant fetches
    /// compiled in a constant expression (a parameter default): those go
    /// through zend_get_class_constant_ex(), which does not create every
    /// case of a backed enum (the FETCH_CLASS_CONSTANT opcode does).
    pub const_expr_const_names: Vec<u32>,
    /// Whether this file/function was declared with declare(strict_types=1)
    pub strict_types: bool,
    /// Which parameters are passed by reference (indexed by parameter position, true = by-ref)
    pub by_ref_params: Vec<bool>,
    /// Which parameters have default values (indexed by parameter position)
    pub param_has_default: Vec<bool>,
    /// The lines of the parameters' RECV opcodes (by parameter position),
    /// when one is not `decl_line` (empty otherwise): see `recv_line()`.
    pub param_lines: Vec<u32>,
    /// The CVs of the parameters a constructor promotes to properties.
    pub promoted_params: Vec<u32>,
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
    /// Identifies the compiled function across copies (a trait method
    /// reached through several traits is the same function: the opcodes
    /// zend_add_trait_method() compares).
    pub origin_id: u32,
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
    lean_kind: FreshOnClone<[u8; 2]>,
    /// See OpArray::prop_ic().
    prop_ics: FreshOnClone<PropIcs>,
    /// See OpArray::fused_sends() and OpArray::call_hint().
    fused_sends: FreshOnClone<Box<[std::cell::Cell<u8>]>>,
    /// See OpArray::static_sites().
    static_sites: FreshOnClone<std::cell::RefCell<Vec<StaticSite>>>,
    /// The name of the frame of a call of this function a builtin makes
    /// (see Vm::push_callback_display), once made.
    callback_display: FreshOnClone<Box<str>>,
    /// See OpArray::type_kind_masks().
    type_masks: FreshOnClone<Box<[u16]>>,
}

/// The cell of a static variable a declaration of it (StaticVarInit, at
/// `ip`) found, while static_vars keeps it: in the VM's `generation`
/// of static_vars (see Vm::static_gen), for the key literal `key`.
#[derive(Debug)]
pub(crate) struct StaticSite {
    pub(crate) ip: u32,
    pub(crate) generation: u64,
    pub(crate) key: crate::string::PhpString,
    pub(crate) cell: std::rc::Weak<std::cell::RefCell<Value>>,
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
    /// The `$http_response_header` variable (set by the http:// wrapper).
    pub(crate) hrh: Option<u32>,
    /// How often lately a call of the function ran to its end without a
    /// frame of its own (see vm::leancall): below 0, calls stop trying.
    pub(crate) lean_score: std::cell::Cell<i16>,
}

thread_local! {
    /// The request whose call sites the SiteCaches are for: code kept by
    /// the script cache serves later requests, which start from nothing.
    static SITE_EPOCH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The current SITE_EPOCH.
#[inline]
pub(crate) fn site_epoch() -> u32 {
    SITE_EPOCH.with(|e| e.get())
}

/// A new request starts (see SITE_EPOCH).
pub fn new_site_epoch() {
    SITE_EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
}

/// What a property access by a constant name found last (see
/// Vm::fast_prop_op): the object's class and the class scope it ran in,
/// whether the property is a plain one of that class there, and the slot
/// the object held it in (the declared properties of a class keep their
/// slots; a hint, checked against the slot's key).
#[derive(Debug, Clone, Default)]
pub(crate) struct PropSlot {
    pub(crate) class: Vec<u8>,
    pub(crate) scope: Option<Vec<u8>>,
    pub(crate) plain: u8,
    pub(crate) slot: u32,
    pub(crate) set: bool,
    /// Of a write to a typed property there: the kinds of values its type
    /// takes as they are (see Vm::typed_prop_mask; 0: not known yet, or
    /// not a type of plain kinds).
    pub(crate) typed: u16,
}

/// What the read, isset() or write of a variable's object property (or
/// the read of a temporary's) by a constant name at an opcode found last (see Vm::prop_ic_hit): the
/// class_tag() of the object's class (0: nothing), whether the property is
/// a plain one there (plain_public_prop()), in any class scope or only in
/// `scope` (exact bytes), and the slot the object held it in (a hint,
/// checked against the slot's key).
#[derive(Debug, Default)]
pub(crate) struct PropIc {
    pub(crate) tag: std::cell::Cell<u32>,
    pub(crate) slot: std::cell::Cell<u32>,
    pub(crate) plain: std::cell::Cell<u8>,
    pub(crate) any_scope: std::cell::Cell<bool>,
    /// The objects of the class have native data their properties have
    /// nothing to do with (see Vm::native_props_plain).
    pub(crate) native_ok: std::cell::Cell<bool>,
    pub(crate) scope: std::cell::RefCell<Option<Box<[u8]>>>,
}

/// The PropIcs of an op array's property accesses by a constant name:
/// the index of each opcode's (u16::MAX: none), made when the first one
/// runs, for the request of SITE_EPOCH `epoch`.
#[derive(Debug, Default)]
pub(crate) struct PropIcs {
    epoch: std::cell::Cell<u32>,
    ids: Box<[u16]>,
    ics: Box<[PropIc]>,
}

impl PropIcs {
    #[cold]
    #[inline(never)]
    fn new(ops: &[Op]) -> PropIcs {
        let mut n = 0usize;
        let ids = ops
            .iter()
            .map(|o| {
                // (a read of a temporary's object too)
                let (name, tmp_ok) = match o.opcode {
                    OpCode::PropertyGet
                    | OpCode::PropertyGetIs
                    | OpCode::PropertyIsset
                    | OpCode::PropertyIssetFetch => (&o.op2, true),
                    OpCode::PropertySet => (&o.result, false),
                    // (the fetches of `$cv->p++` and `$cv->p[k] = v`)
                    OpCode::PropertyGetIncDec
                    | OpCode::PropertyGetW
                    | OpCode::PropertyGetRW
                    | OpCode::PropertyGetWObj
                    | OpCode::PropertyGetWObjRW => (&o.op2, false),
                    _ => return u16::MAX,
                };
                if (matches!(o.op1, OperandType::Cv(_))
                    || (tmp_ok && matches!(o.op1, OperandType::Tmp(_))))
                    && matches!(name, OperandType::Const(_))
                    && n < u16::MAX as usize
                {
                    n += 1;
                    (n - 1) as u16
                } else {
                    u16::MAX
                }
            })
            .collect();
        PropIcs {
            epoch: std::cell::Cell::new(SITE_EPOCH.with(|e| e.get())),
            ids,
            ics: (0..n).map(|_| PropIc::default()).collect(),
        }
    }

    /// A new request: what the caches found is forgotten.
    #[cold]
    #[inline(never)]
    fn renew(&self, epoch: u32) {
        for ic in self.ics.iter() {
            ic.tag.set(0);
        }
        self.epoch.set(epoch);
    }
}

/// The opcodes fast_prop_op() handles.
#[inline]
pub(crate) fn is_fast_prop_op(o: OpCode) -> bool {
    matches!(
        o,
        OpCode::PropertyGet
            | OpCode::PropertySet
            | OpCode::PropertyGetW
            | OpCode::PropertyGetIncDec
            | OpCode::PropertyGetRW
            | OpCode::PropertyIsset
            | OpCode::PropertyIssetFetch
    )
}

impl SiteCaches {
    /// The PropSlot of the property access at `ip` of `ops` (made with
    /// the others of the code on first use).
    #[inline(always)]
    pub(crate) fn prop_slot(&mut self, ops: &[Op], ip: usize) -> Option<&mut PropSlot> {
        if self.prop_ids.is_empty() {
            self.init_prop_ids(ops);
        }
        let id = *self.prop_ids.get(ip)?;
        self.prop_slots.get_mut(id as usize)
    }

    /// A new request (SITE_EPOCH `epoch`): what the sites found is
    /// forgotten, but for the call targets, which are checked again as a
    /// call site meets them (see Vm::revalidate_call_target), and which
    /// opcodes are property accesses.
    #[cold]
    #[inline(never)]
    fn renew(&mut self, epoch: u32) {
        let mut call_targets = std::mem::take(&mut self.call_targets);
        for c in call_targets.0.iter_mut().flatten() {
            c.set_stale();
        }
        let prop_ids = std::mem::take(&mut self.prop_ids);
        *self = SiteCaches {
            epoch,
            call_targets,
            ..Default::default()
        };
        if !prop_ids.is_empty() {
            self.prop_ids = prop_ids;
            let n = self.prop_ids.iter().filter(|&&i| i != u16::MAX).count();
            self.prop_slots = vec![PropSlot::default(); n];
        }
    }

    #[cold]
    #[inline(never)]
    fn init_prop_ids(&mut self, ops: &[Op]) {
        let mut n = 0usize;
        self.prop_ids = ops
            .iter()
            .map(|o| {
                if (is_fast_prop_op(o.opcode) || o.opcode == OpCode::TypeCheck)
                    && n < u16::MAX as usize
                {
                    n += 1;
                    (n - 1) as u16
                } else {
                    u16::MAX
                }
            })
            .collect();
        if self.prop_ids.is_empty() {
            self.prop_ids.push(u16::MAX);
        }
        self.prop_slots = vec![PropSlot::default(); n];
    }
}

/// A map of an op array's sites by opcode index (the caches of
/// SiteCaches): a slot per opcode, up to the last one written (an
/// opcode's lookup is an index, not a hash).
#[derive(Debug, Clone)]
pub(crate) struct IpMap<T>(Vec<Option<Box<T>>>);

impl<T> Default for IpMap<T> {
    fn default() -> Self {
        IpMap(Vec::new())
    }
}

impl<T> IpMap<T> {
    #[inline]
    pub(crate) fn get(&self, ip: &u32) -> Option<&T> {
        self.0.get(*ip as usize)?.as_deref()
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, ip: &u32) -> Option<&mut T> {
        self.0.get_mut(*ip as usize)?.as_deref_mut()
    }

    pub(crate) fn insert(&mut self, ip: u32, v: T) -> Option<T> {
        let i = ip as usize;
        if self.0.len() <= i {
            self.0.resize_with(i + 1, || None);
        }
        self.0[i].replace(Box::new(v)).map(|b| *b)
    }
}

/// See OpArray::sites; entries are by opcode index.
#[derive(Debug, Clone)]
pub(crate) struct SiteCaches {
    /// The SITE_EPOCH they were made in.
    epoch: u32,
    /// What the plain function calls of the INIT_FCALLs resolved to (see
    /// Vm::plain_call_target).
    pub(crate) call_targets: IpMap<crate::vm::CachedCallTarget>,
    /// The property accesses by a constant name (see Vm::fast_prop_op): the
    /// index into `prop_slots` of each opcode's site (u16::MAX: none),
    /// made when the first one runs.
    pub(crate) prop_ids: Vec<u16>,
    pub(crate) prop_slots: Vec<PropSlot>,
    /// What the INIT_METHOD_CALLs found (see Vm::init_method_call_fast).
    pub(crate) method_sites: IpMap<crate::vm::MethodSite>,
    /// What the forwarding calls (self::m(), ...) last resolved to (see
    /// Vm::push_forward_call).
    pub(crate) forward_sites: IpMap<crate::vm::ForwardSite>,
    /// The INIT_METHOD_CALLs that set up a deferred SPL call: the object's
    /// class name, the pending call's name and whether the method takes
    /// no argument by reference (see Vm::init_spl_method_fast).
    pub(crate) spl_sites: IpMap<(Vec<u8>, crate::string::PhpString, bool)>,
    /// The INIT_METHOD_CALLs `__construct` on an object of an internal
    /// class: the object's class name, the pending call's name and whether
    /// the call releases its $this, and its CallTarget::internal_ctor (see
    /// Vm::init_internal_ctor_fast).
    pub(crate) ctor_sites: IpMap<(Vec<u8>, crate::string::PhpString, bool, u8)>,
    /// The template the NEWs of a user class last used (see
    /// Vm::new_object_fast).
    /// (and whether the constructor call after the NEW may be skipped:
    /// Vm::ctor_call_shape())
    pub(crate) new_sites: IpMap<(std::rc::Rc<crate::object::NewTemplate>, bool)>,
    /// What the NEWs of an internal class made (see Vm::new_internal_fast).
    pub(crate) internal_new_sites: IpMap<std::rc::Rc<crate::object::InternalNewTemplate>>,
    /// The unqualified namespaced function calls / `f(...)` callables that
    /// fell back to the global function: the namespaced (lowercase) name.
    /// Like the runtime cache of INIT_NS_FCALL_BY_NAME, a function of that
    /// name declared later is not seen by the site.
    pub(crate) ns_fallbacks: IpMap<Vec<u8>>,
    /// What a class constant fetch (`A::C`, `self::C`, `A::class`) found,
    /// for the class scope it ran in (see Vm::fast_class_const).
    /// (and, of `static::C`, the called class)
    pub(crate) const_sites: IpMap<(Option<Vec<u8>>, Option<Vec<u8>>, Value)>,
    /// Where the static property reads (`A::$p`) found the property: the
    /// class holding its slot and the slot's index in its
    /// static_properties (see Vm::fast_static_prop_fetch).
    pub(crate) sprop_sites: IpMap<(usize, usize, Option<Vec<u8>>)>,
    /// The value a global constant fetch (not deprecated) found: constants
    /// do not change once defined (see Vm::fast_const_lookup).
    pub(crate) gconst_sites: IpMap<Value>,
}

impl Default for SiteCaches {
    fn default() -> Self {
        SiteCaches {
            epoch: SITE_EPOCH.with(|e| e.get()),
            call_targets: Default::default(),
            prop_ids: Vec::new(),
            prop_slots: Vec::new(),
            method_sites: Default::default(),
            forward_sites: Default::default(),
            spl_sites: Default::default(),
            ctor_sites: Default::default(),
            new_sites: Default::default(),
            internal_new_sites: Default::default(),
            ns_fallbacks: Default::default(),
            const_sites: Default::default(),
            sprop_sites: Default::default(),
            gconst_sites: Default::default(),
        }
    }
}

thread_local! {
    /// The JIT auto-globals ($_ENV, $_REQUEST) compiled code referred to
    /// since the last take_auto_global_refs() (bit 0: _ENV, bit 1: _REQUEST).
    static AUTO_GLOBAL_REFS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// A variable named `name` is compiled (zend_is_auto_global() arms a JIT
/// auto-global the compiler meets).
#[inline]
fn note_auto_global_ref(name: &[u8]) {
    let bit = match name {
        b"_ENV" => 1,
        b"_REQUEST" => 2,
        _ => return,
    };
    AUTO_GLOBAL_REFS.with(|c| c.set(c.get() | bit));
}

/// The JIT auto-globals referred to by code compiled since the last call
/// (see note_auto_global_ref()).
pub fn take_auto_global_refs() -> u8 {
    AUTO_GLOBAL_REFS.with(|c| c.replace(0))
}

impl OpArray {
    /// The line of the RECV opcode of parameter `idx` (0-based, `$this`
    /// not counted; past the last one: the variadic's): where the errors
    /// about its argument are raised. 0 when unknown.
    #[inline(never)]
    pub fn recv_line(&self, idx: usize) -> u32 {
        match self.param_lines.last() {
            Some(&last) => self.param_lines.get(idx).copied().unwrap_or(last),
            None => self.decl_line,
        }
    }

    /// `recv_line()` of the parameter a "...: Argument #N ..." message is
    /// about (`decl_line` without one).
    #[inline(never)]
    pub fn recv_line_for_msg(&self, msg: &str) -> u32 {
        if self.param_lines.is_empty() {
            return self.decl_line;
        }
        let n = msg.find("Argument #").and_then(|p| {
            let digits: String = msg[p + 10..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse::<usize>().ok()
        });
        match n {
            Some(n) if n > 0 => self.recv_line(n - 1),
            _ => self.decl_line,
        }
    }

    /// The value kinds each parameter's declared type takes as they are
    /// (see recv_fast::kind_bit), then the return type's: 0 for none, or
    /// for a type naming a class or anything else (the full check then).
    #[inline]
    pub(crate) fn type_kind_masks(&self) -> &[u16] {
        self.type_masks
            .0
            .get_or_init(|| crate::recv_fast::type_kind_masks_of(self))
    }

    /// See `callback_display`.
    #[inline]
    pub(crate) fn callback_display(&self) -> &std::cell::OnceCell<Box<str>> {
        &self.callback_display.0
    }

    /// Whether calls of this code may run without a frame of their own (see
    /// vm::leancall): 1 for a function, 2 for an instance method (its first
    /// variable is `$this`), 0 when its frame has to check or set up
    /// something (a by-reference return, a generator, variadics,
    /// attributes, `$GLOBALS` or auto-globals, internal code, a closure); plus 4 when it has typed parameters, 8 a return
    /// type (checked per call, see vm::leancall), 16 a parameter by
    /// reference.
    #[inline]
    pub(crate) fn lean_kind(&self) -> u8 {
        match self.lean_kind.0.get() {
            Some(k) => k[0],
            None => self.lean_kind_init()[0],
        }
    }

    /// lean_kind() of a call made by a builtin (a callback, see
    /// vm::leancall): a closure's too (its caller binds its scope and
    /// `$this`, its captured values are among its arguments).
    #[inline]
    pub(crate) fn callback_lean_kind(&self) -> u8 {
        match self.lean_kind.0.get() {
            Some(k) => k[1],
            None => self.lean_kind_init()[1],
        }
    }

    #[cold]
    #[inline(never)]
    fn lean_kind_init(&self) -> [u8; 2] {
        *self.lean_kind.0.get_or_init(|| {
            // (typed parameters and a return type: the arguments and the
            // returned value are checked to have their types already, see
            // vm::leancall)
            let plain = !self.returns_by_ref
                && !self.is_generator
                // (a variadic parameter by value: the call packs its
                // arguments, see vm::leancall)
                && self
                    .variadic_param
                    .is_none_or(|v| !self.by_ref_params.get(v as usize).copied().unwrap_or(false))
                && self.attributes.is_empty()
                && self.internal_sig.is_none()
                && !self.name.is_empty()
                && self.cv_name_info().plain_cvs;
            let typed = 4 * u8::from(self.param_types.iter().any(|t| t.is_some()))
                + 8 * u8::from(self.return_type.is_some())
                + 16 * u8::from(self.by_ref_params.iter().any(|&b| b));
            let k = match (plain, self.cv_names.first().map(|n| n.as_slice())) {
                (false, _) => 0,
                (true, Some(b"this")) => 2 | typed,
                (true, _) => 1 | typed,
            };
            if crate::closure::is_closure_fn_name(&self.name) {
                [0, k]
            } else {
                [k, k]
            }
        })
    }

    /// The INIT_FCALL / INIT_METHOD_CALL of a call at `ip`: the number of
    /// plain sends (SEND_VAL, SEND_VAL_EX) right after it, at most 16, when
    /// the DO_FCALL of the call follows them (see Vm::lean_fused).
    #[inline]
    pub(crate) fn fused_sends(&self, ip: usize) -> Option<usize> {
        let m = match self.fused_sends.0.get() {
            Some(m) if m.len() == self.ops.len() => m,
            Some(_) => return fused_sends_at(&self.ops, ip),
            None => self.fused_sends_init(),
        };
        match m.get(ip).map(|c| c.get() & CALL_SENDS) {
            Some(n) if n != CALL_SENDS => Some(n as usize),
            _ => None,
        }
    }

    /// What the opcode at `ip` is as a call's INIT_* (CALL_SENDS: its
    /// fused_sends(), CALL_SENDS when none; CALL_PLAIN: an INIT_FCALL by a
    /// plain function name, see vm::plain_fcall_name; CALL_NOT_DIRECT: see
    /// no_direct_lean): computed with fused_sends(), once.
    #[inline]
    pub(crate) fn call_hint(&self, ip: usize) -> u8 {
        let m = match self.fused_sends.0.get() {
            Some(m) => m,
            None => self.fused_sends_init(),
        };
        m.get(ip).map_or(CALL_SENDS, |c| c.get())
    }

    /// The call at `ip` is not one Vm::lean_direct() makes (its function
    /// is a builtin...): it is not tried there again.
    #[cold]
    pub(crate) fn no_direct_lean(&self, ip: usize) {
        if let Some(c) = self.fused_sends.0.get().and_then(|m| m.get(ip)) {
            c.set(c.get() | CALL_NOT_DIRECT);
        }
    }

    /// The INIT_FCALL at `ip` resolved to a builtin of fastpath's
    /// SPECIAL_CALLS / PURE_CALLS: the next runs of it look for the
    /// builtin's cached target (see CALL_SPECIAL; a stale mark only costs
    /// that look).
    #[cold]
    #[inline(never)]
    pub(crate) fn mark_special_call(&self, ip: usize) {
        let m = match self.fused_sends.0.get() {
            Some(m) => m,
            None => self.fused_sends_init(),
        };
        if let Some(c) = m.get(ip) {
            c.set(c.get() | CALL_SPECIAL);
        }
    }

    #[cold]
    #[inline(never)]
    fn fused_sends_init(&self) -> &[std::cell::Cell<u8>] {
        self.fused_sends.0.get_or_init(|| {
            self.ops
                .iter()
                .enumerate()
                .map(|(ip, op)| {
                    // (only the INIT_* of calls are asked)
                    if !matches!(op.opcode, OpCode::InitFCall | OpCode::InitMethodCall) {
                        return std::cell::Cell::new(CALL_SENDS);
                    }
                    let n = fused_sends_at(&self.ops, ip).map_or(CALL_SENDS, |n| n as u8);
                    let plain = op.opcode == OpCode::InitFCall
                        && crate::vm::plain_fcall_name(op, &self.literals).is_some();
                    std::cell::Cell::new(n | if plain { CALL_PLAIN } else { 0 })
                })
                .collect()
        })
    }

    /// The cells the declarations of static variables found (see
    /// StaticSite; a copy of the code starts without them: its keys may
    /// differ).
    #[inline]
    pub(crate) fn static_sites(&self) -> &std::cell::RefCell<Vec<StaticSite>> {
        self.static_sites.0.get_or_init(Default::default)
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

    /// The PropIc of the property access at `ip` (see Vm::prop_ic_hit).
    #[inline(always)]
    pub(crate) fn prop_ic(&self, ip: usize) -> Option<&PropIc> {
        let t = match self.prop_ics.0.get() {
            Some(t) => t,
            None => self.prop_ics.0.get_or_init(|| PropIcs::new(&self.ops)),
        };
        let epoch = SITE_EPOCH.with(|e| e.get());
        if t.epoch.get() != epoch {
            t.renew(epoch);
        }
        let id = *t.ids.get(ip)?;
        t.ics.get(id as usize)
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
                    || n.as_slice() == b"http_response_header"
            }),
            hrh: self
                .cv_names
                .iter()
                .position(|n| n == b"http_response_header")
                .map(|i| i as u32),
            lean_score: std::cell::Cell::new(0),
        })
    }

    /// A listing of the opcodes (and those of the nested functions), for
    /// debugging (`GORO_DUMP_OPS=1` prints the main script's).
    pub fn dump(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let name = crate::string::lossy(self.effective_name());
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
                    .map(|n| crate::string::lossy(n).into_owned())
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
            match free_mask.get(i).map(|m| m & FREE_BITS) {
                Some(m) if m != 0 => {
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
        self.const_expr_const_names.shrink_to_fit();
        self.by_ref_params.shrink_to_fit();
        self.param_has_default.shrink_to_fit();
        self.param_lines.shrink_to_fit();
        self.promoted_params.shrink_to_fit();
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
    #[inline(always)]
    pub(crate) fn sites(&self) -> &std::cell::RefCell<Option<Box<SiteCaches>>> {
        let epoch = SITE_EPOCH.with(|e| e.get());
        let stale = match self.sites.try_borrow() {
            Ok(s) => s.as_ref().is_some_and(|c| c.epoch != epoch),
            Err(_) => false,
        };
        if stale {
            if let Some(c) = self.sites.borrow_mut().as_mut() {
                c.renew(epoch);
            }
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
            .get_or_init(|| crate::rcstr::RcStr::from(crate::string::lossy(&self.filename)));
        debug_assert!(f.as_bytes() == self.filename.as_slice() || !f.is_ascii());
        f.clone()
    }

    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            literals: Vec::new(),
            cv_names: Vec::new(),
            temp_count: 0,
            temp_next: 0,
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
            const_expr_const_names: Vec::new(),
            strict_types: false,
            by_ref_params: Vec::new(),
            param_has_default: Vec::new(),
            param_lines: Vec::new(),
            promoted_params: Vec::new(),
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
            origin_id: {
                static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            sites: Default::default(),
            cv_info: Default::default(),
            free_mask: Default::default(),
            lean_kind: Default::default(),
            prop_ics: Default::default(),
            fused_sends: Default::default(),
            static_sites: Default::default(),
            callback_display: Default::default(),
            type_masks: Default::default(),
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
        note_auto_global_ref(name);
        self.cv_names.push(name.to_vec());
        self.cv_names_changed();
        idx
    }

    /// Allocate a new temporary slot (the next free one: the compiler
    /// reuses the temporaries of a finished statement, see temp_next).
    pub fn alloc_temp(&mut self) -> u32 {
        let idx = self.temp_next;
        self.temp_next += 1;
        if self.temp_next > self.temp_count {
            self.temp_count = self.temp_next;
        }
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

/// OpArray::call_hint(): the number of fused sends (CALL_SENDS: none).
pub(crate) const CALL_SENDS: u8 = 0x1f;
/// OpArray::call_hint(): an INIT_FCALL by a plain function name.
pub(crate) const CALL_PLAIN: u8 = 0x20;
/// OpArray::call_hint(): see OpArray::no_direct_lean().
pub(crate) const CALL_NOT_DIRECT: u8 = 0x80;
/// OpArray::call_hint(): the INIT_FCALL resolved (when last set up) to a
/// builtin Vm::fast_special_call() may run directly (see
/// OpArray::mark_special_call).
pub(crate) const CALL_SPECIAL: u8 = 0x40;

/// OpArray::fused_sends() of the opcode at `ip` of `ops`.
fn fused_sends_at(ops: &[Op], ip: usize) -> Option<usize> {
    let rest = ops.get(ip + 1..)?;
    let n = rest
        .iter()
        .take(17)
        .position(|s| !matches!(s.opcode, OpCode::SendVal | OpCode::SendValChecked))?;
    (rest[n].opcode == OpCode::DoFCall).then_some(n)
}

/// The free mask of `oa` (see FreeInfo), by live range: a temporary's index
/// is used again once the FREE_TMPS ending a statement (STMT_END) released
/// it (the compiler reuses the temporaries of a finished statement), each
/// use of the index then being a range of its own.
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
    /// A live range of a temporary's index: its first and last uses, a
    /// use that keeps it for later (see `peek`), and whether it holds a
    /// compound assignment's read of its target.
    struct Range {
        t: u32,
        first: u32,
        last: u32,
        keep: bool,
        rw_read: bool,
    }
    let mut ranges: Vec<Range> = Vec::new();
    // The open range of each index.
    let mut open = vec![NONE; tc];
    // The range of the op1 temporary of each opcode.
    let mut op1_range = vec![NONE; n];
    // Latest source of a jump to each opcode.
    let mut jump_src = vec![NONE; n + 1];
    let mut any_jump = false;
    // The calls being set up (the callable temporary of an InitFCall), and
    // the DoFCalls ending one with a temporary callable (its range).
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
                Some(t) => {
                    if let Some(&r) = open.get(t as usize).filter(|&&r| r != NONE) {
                        call_ends.push((i as u32, r));
                    }
                }
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
            // (at the end of a statement, the range of each index it
            // releases ends: elsewhere, the temporaries may still be used
            // on another path, later in the code)
            if let (OperandType::Tmp(a), OperandType::Tmp(b), STMT_END) =
                (op.op1, op.op2, op.result)
            {
                let b = (b as usize).min(tc);
                for r in open.get_mut((a as usize).min(b)..b).into_iter().flatten() {
                    *r = NONE;
                }
            }
            continue;
        }
        // (a dynamic call's callable, a Closure, is the call frame's until
        // the call ends)
        let peek = matches!(op.opcode, OpCode::ListGet | OpCode::InitFCall);
        for (slot, o) in [op.op1, op.op2, op.result].into_iter().enumerate() {
            if let OperandType::Tmp(t) = o {
                let tu = t as usize;
                if tu >= tc {
                    continue;
                }
                let r = match open[tu] {
                    NONE => {
                        ranges.push(Range {
                            t,
                            first: i as u32,
                            last: i as u32,
                            keep: false,
                            rw_read: false,
                        });
                        open[tu] = (ranges.len() - 1) as u32;
                        open[tu]
                    }
                    r => r,
                };
                let range = &mut ranges[r as usize];
                range.last = i as u32;
                if peek && slot == 0 {
                    range.keep = true;
                }
                if slot == 0 {
                    op1_range[i] = r;
                }
            }
        }
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
                let rr = open[r as usize];
                if rr != NONE {
                    ranges[rr as usize].rw_read = true;
                }
            }
        }
    }
    let mut mask = vec![0u8; n];
    for range in &ranges {
        let (t, f, l) = (range.t, range.first, range.last);
        if range.keep {
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
        if let (OperandType::Tmp(_), OperandType::Tmp(v), OperandType::Tmp(res)) =
            (op.op1, op.op2, op.result)
        {
            let r1 = op1_range[l as usize];
            if v == t && r1 != NONE && ranges[r1 as usize].rw_read {
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
            if o == OperandType::Tmp(t) {
                bits |= 1 << b;
            }
        }
        mask[l as usize] |= bits;
    }
    // A Closure called from a temporary goes as the call ends (when nothing
    // after the call setup reads it again).
    let mut call_frees = Vec::new();
    if calls_balanced && open_calls.is_empty() {
        for (d, r) in call_ends {
            let range = &ranges[r as usize];
            if range.keep
                && oa
                    .ops
                    .get(range.last as usize)
                    .is_some_and(|o| o.opcode == OpCode::InitFCall)
                && range.last < d
            {
                mask[d as usize] |= 16;
                call_frees.push((d, range.t));
            }
        }
    }
    mark_plain_free_tmps(oa, &mut mask);
    mark_tested_free_tmps(oa, &mut mask);
    // (none: no mask, whose lookups the dispatch loop then skips at once)
    if mask.iter().all(|&m| m == 0) {
        mask = Vec::new();
    }
    FreeInfo {
        mask: mask.into_boxed_slice(),
        call_frees: call_frees.into_boxed_slice(),
    }
}

/// OpArray::free_mask() bit 5 (of a FREE_TMPS): whenever it runs, its
/// temporaries hold nothing to release, only Undef or plain values (null,
/// bools, numbers: see mark_plain_free_tmps). With no destructor due, it
/// has nothing to do then (run_fast_ops() steps over it). (The bits of the
/// temporaries an opcode releases are the lower ones, FREE_BITS.)
pub(crate) const FREE_PLAIN: u8 = 0x20;
/// OpArray::free_mask() bit 6 (of a JMPZ / JMPNZ): the next opcode is a
/// FREE_TMPS of a few temporaries (an `if` condition's: run_fast_ops() steps
/// over it when they hold plain values).
pub(crate) const FREE_TESTED_NEXT: u8 = 0x40;
/// Bit 7: the same of the opcode the JMPZ / JMPNZ jumps to.
pub(crate) const FREE_TESTED_TARGET: u8 = 0x80;
/// The free_mask() bits of the temporaries released after an opcode.
pub(crate) const FREE_BITS: u8 = 0x1f;

/// Whether the result an opcode writes in a temporary is a plain value
/// (null, a bool, an int or a float), when it writes one: a comparison, a
/// cast to a scalar, a test.
#[inline]
fn plain_result(opc: OpCode) -> bool {
    matches!(
        opc,
        OpCode::Identical
            | OpCode::NotIdentical
            | OpCode::Equal
            | OpCode::NotEqual
            | OpCode::Less
            | OpCode::LessEqual
            | OpCode::Greater
            | OpCode::GreaterEqual
            | OpCode::Spaceship
            | OpCode::BooleanNot
            | OpCode::BoolXor
            | OpCode::CastBool
            | OpCode::CastInt
            | OpCode::CastFloat
            | OpCode::TypeCheck
            | OpCode::IssetCheck
            | OpCode::ArrayIsset
            | OpCode::ArrayIssetFetch
            | OpCode::PropertyIsset
            | OpCode::PropertyIssetFetch
            | OpCode::StaticPropIsset
    )
}

/// Whether an opcode only reads its op1 / op2 temporaries (or moves the
/// value out of them): it never stores anything there.
#[inline]
fn reads_tmp_operands(opc: OpCode) -> bool {
    plain_result(opc)
        || matches!(
            opc,
            OpCode::JmpZ
                | OpCode::JmpNz
                | OpCode::QmAssign
                | OpCode::Echo
                | OpCode::Return
                | OpCode::SendVal
                | OpCode::SendValChecked
                | OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::Concat
                | OpCode::ArrayGet
        )
}

/// A literal that is a plain value (not Undef).
#[inline]
fn plain_literal(oa: &OpArray, o: OperandType) -> bool {
    matches!(o, OperandType::Const(c) if matches!(
        oa.literals.get(c as usize),
        Some(Value::Null | Value::False | Value::True | Value::Long(_) | Value::Double(_))
    ))
}

/// Sets FREE_PLAIN in `mask` (see compute_free_mask) on each FREE_TMPS of
/// `oa` whose temporaries can only hold Undef or plain values when it runs.
/// A forward data flow over the code: per temporary, whether it may hold
/// something else ("dirty"). The temporaries start Undef; an opcode's
/// result makes its temporary dirty unless the opcode only stores plain
/// values (plain_result(), or a plain literal copied); an op1 / op2
/// temporary of an opcode that may store into it gets dirty (all but those
/// of reads_tmp_operands()); a FREE_TMPS leaves its temporaries Undef, as
/// an ASSIGN to a variable leaves the temporary it moves the value of. The
/// target of a jump other than the plain jumps (a catch, a finally, a
/// deferred jump: the exception's temporary is written there) starts with
/// all of them dirty.
#[inline(never)]
fn mark_plain_free_tmps(oa: &OpArray, mask: &mut [u8]) {
    let n = oa.ops.len();
    let tc = oa.temp_count as usize;
    // (one bit per temporary: code with more of them is left alone)
    if n == 0 || tc == 0 || tc > 127 || mask.len() != n {
        return;
    }
    if !oa.ops.iter().any(|o| o.opcode == OpCode::FreeTmps) {
        return;
    }
    let bit = |t: u32| -> u128 { if (t as usize) < tc { 1 << t } else { 0 } };
    // (the temporaries a..b)
    let span = |a: u32, b: u32| -> u128 {
        let below = |t: u32| (1u128 << t.min(tc as u32)) - 1;
        below(b) & !below(a)
    };
    // The state before each opcode (0: not reached; SEEN set once it is).
    const SEEN: u128 = 1 << 127;
    let mut state = vec![0u128; n + 1];
    state[0] = SEEN;
    // (a pass over the code, again from the earliest opcode a jump back
    // changed the state of)
    let mut from = 0;
    for _ in 0..64 {
        let mut again = usize::MAX;
        for i in from..n {
            let before = state[i];
            if before == 0 {
                continue;
            }
            let op = &oa.ops[i];
            let mut cur = before;
            if op.opcode == OpCode::FreeTmps {
                if let (OperandType::Tmp(a), OperandType::Tmp(b)) = (op.op1, op.op2) {
                    cur &= !span(a, b);
                }
            } else {
                if !reads_tmp_operands(op.opcode) {
                    for o in [op.op1, op.op2] {
                        if let OperandType::Tmp(t) = o {
                            cur |= bit(t);
                        }
                    }
                }
                if let OperandType::Tmp(r) = op.result {
                    let plain = plain_result(op.opcode)
                        || (op.opcode == OpCode::QmAssign
                            && match op.op1 {
                                OperandType::Tmp(s) => before & bit(s) == 0,
                                o => plain_literal(oa, o),
                            });
                    if !plain {
                        cur |= bit(r);
                    }
                }
                if op.opcode == OpCode::Assign {
                    // (`Assign T, literal`: a condition's value, see `&&`)
                    if let OperandType::Tmp(t) = op.op1 {
                        if op.result == OperandType::Unused
                            && before & bit(t) == 0
                            && plain_literal(oa, op.op2)
                        {
                            cur &= !bit(t);
                        }
                    }
                    // (a temporary is moved into the variable, its slot
                    // left Undef)
                    if let (OperandType::Tmp(s), OperandType::Cv(_)) = (op.op2, op.op1) {
                        if op.result != OperandType::Tmp(s) {
                            cur &= !bit(s);
                        }
                    }
                }
            }
            let plain_jump = matches!(
                op.opcode,
                OpCode::Jmp
                    | OpCode::JmpZ
                    | OpCode::JmpNz
                    | OpCode::ForeachNext
                    | OpCode::ForeachNextRef
                    | OpCode::JmpIfArgByRef
                    | OpCode::StaticVarInitOrJmp
            );
            let mut flow = |k: usize, v: u128| {
                let k = k.min(n);
                let new = state[k] | v;
                if state[k] != new {
                    state[k] = new;
                    if k <= i {
                        again = again.min(k);
                    }
                }
            };
            for o in [op.op1, op.op2, op.result] {
                if let OperandType::JmpTarget(k) = o {
                    flow(k as usize, if plain_jump { cur } else { u128::MAX });
                }
            }
            if !matches!(op.opcode, OpCode::Jmp | OpCode::Return) {
                flow(i + 1, cur);
            }
        }
        if again == usize::MAX {
            for (i, op) in oa.ops.iter().enumerate() {
                let (OpCode::FreeTmps, OperandType::Tmp(a), OperandType::Tmp(b)) =
                    (op.opcode, op.op1, op.op2)
                else {
                    continue;
                };
                let s = state[i];
                if s != 0 && a <= b && b as usize <= tc && s & span(a, b) == 0 {
                    mask[i] |= FREE_PLAIN;
                }
            }
            return;
        }
        from = again;
    }
}

/// Sets FREE_TESTED_NEXT / FREE_TESTED_TARGET in `mask` (see
/// compute_free_mask) on the JMPZ / JMPNZ of `oa` followed by (jumping to)
/// a FREE_TMPS of a few temporaries.
#[inline(never)]
fn mark_tested_free_tmps(oa: &OpArray, mask: &mut [u8]) {
    if mask.len() != oa.ops.len() {
        return;
    }
    let small_free = |k: usize| match oa.ops.get(k) {
        Some(f) if f.opcode == OpCode::FreeTmps => matches!(
            (f.op1, f.op2),
            (OperandType::Tmp(a), OperandType::Tmp(b)) if a < b && b - a <= 8
        ),
        _ => false,
    };
    for (i, op) in oa.ops.iter().enumerate() {
        let (OpCode::JmpZ | OpCode::JmpNz, OperandType::JmpTarget(t)) = (op.opcode, op.op2) else {
            continue;
        };
        if small_free(i + 1) {
            mask[i] |= FREE_TESTED_NEXT;
        }
        if small_free(t as usize) {
            mask[i] |= FREE_TESTED_TARGET;
        }
    }
}

impl Default for OpArray {
    fn default() -> Self {
        Self::new()
    }
}
