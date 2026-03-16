use crate::value::Value;

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
    /// Send argument: op1 = value, op2 = arg position
    SendVal,
    /// Execute the function call, result = return value
    DoFCall,
    /// Return op1 from current function
    Return,
    /// Declare a user function: op1 = name (const), op2 = function OpArray index (const)
    DeclareFunction,

    // ---- Variables ----
    /// Load a constant value into a temporary
    LoadConst,
    /// Runtime constant lookup: op1 = name (const), result = value
    ConstLookup,
    /// Include/require file: op1 = path, result = return value
    IncludeFile,
    /// Initialize a static variable: op1 = CV, op2 = default value (const), result = static key name (const)
    StaticVarInit,
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

    // ---- Foreach ----
    /// Initialize foreach: op1 = array, result = iterator state (tmp)
    ForeachInit,
    /// Fetch next value: op1 = iterator (tmp), result = value (tmp), op2 = jump target if done
    ForeachNext,
    /// Fetch key for current iteration: op1 = iterator (tmp), result = key (tmp)
    ForeachKey,

    // ---- OOP ----
    /// Create a new object: op1 = class name (const), result = object (tmp)
    NewObject,
    /// Get property: op1 = object, op2 = property name (const), result = value (tmp)
    PropertyGet,
    /// Set property: op1 = object, op2 = value, result = property name (const)
    PropertySet,
    /// Call method: op1 = object, op2 = method name (const)
    InitMethodCall,
    /// Declare a class: op1 = class name (const), op2 = class def index (const)
    DeclareClass,
    /// Get static property: op1 = class name (const), op2 = prop name (const), result = value
    StaticPropGet,
    /// Set static property: op1 = class name (const), op2 = value, result = prop name (const)
    StaticPropSet,

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

    // ---- Print ----
    /// print expr (returns 1)
    Print,

    // ---- Generator ----
    /// Yield a value: op1 = value (or Unused), op2 = key (or Unused), result = where to store sent value
    Yield,
    /// Generator return (like Return but for generators)
    GeneratorReturn,
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
    /// If set, the last param is variadic (...$args) at this CV index
    pub variadic_param: Option<u32>,
    /// Nested function OpArrays (for DeclareFunction)
    pub child_functions: Vec<OpArray>,
    /// Whether this function is a generator (contains yield)
    pub is_generator: bool,
}

impl OpArray {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            literals: Vec::new(),
            cv_names: Vec::new(),
            temp_count: 0,
            name: Vec::new(),
            param_count: 0,
            variadic_param: None,
            child_functions: Vec::new(),
            is_generator: false,
        }
    }

    /// Add a literal value to the constant pool, returning its index
    pub fn add_literal(&mut self, value: Value) -> u32 {
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
            OpCode::Jmp => op.op1 = OperandType::JmpTarget(target),
            OpCode::JmpZ | OpCode::JmpNz | OpCode::ForeachNext => {
                op.op2 = OperandType::JmpTarget(target);
            }
            _ => panic!("cannot patch non-jump instruction: {:?}", op.opcode),
        }
    }
}

impl Default for OpArray {
    fn default() -> Self {
        Self::new()
    }
}
