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

    // ---- Cast ----
    CastInt,
    CastFloat,
    CastString,
    CastBool,
    CastArray,

    // ---- Array ----
    /// Create a new empty array, result = new array
    ArrayNew,
    /// Append value to array: op1 = array (CV), op2 = value
    ArrayAppend,
    /// Set array element: op1 = array (CV), op2 = value, extended_value = key const index
    ArraySet,
    /// Read array element: result = op1[op2]
    ArrayGet,

    // ---- Type checking ----
    TypeCheck,

    // ---- String interpolation ----
    /// Concatenate multiple values: fast concat of op1 and op2
    FastConcat,

    // ---- Print ----
    /// print expr (returns 1)
    Print,
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
    /// Nested function OpArrays (for DeclareFunction)
    pub child_functions: Vec<OpArray>,
}

impl OpArray {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            literals: Vec::new(),
            cv_names: Vec::new(),
            temp_count: 0,
            name: Vec::new(),
            child_functions: Vec::new(),
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
            OpCode::JmpZ | OpCode::JmpNz => op.op2 = OperandType::JmpTarget(target),
            _ => panic!("cannot patch non-jump instruction"),
        }
    }
}

impl Default for OpArray {
    fn default() -> Self {
        Self::new()
    }
}
