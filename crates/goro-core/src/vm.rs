use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::object::{ClassEntry, PhpObject};
use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// Built-in function signature
pub type BuiltinFn = fn(&mut Vm, &[Value]) -> Result<Value, VmError>;

/// VM runtime error
#[derive(Debug, Clone)]
pub struct VmError {
    pub message: String,
    pub line: u32,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fatal error on line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for VmError {}

/// Pending function call being assembled
struct PendingCall {
    name: PhpString,
    args: Vec<Value>,
}

/// The virtual machine / executor
pub struct Vm {
    /// Output buffer
    output: Vec<u8>,
    /// Registered built-in functions
    functions: HashMap<Vec<u8>, BuiltinFn>,
    /// User-defined functions (compiled op arrays)
    user_functions: HashMap<Vec<u8>, OpArray>,
    /// Stack of pending function calls (supports nested calls)
    pending_calls: Vec<PendingCall>,
    /// Static variable storage (keyed by "funcname::varname")
    static_vars: HashMap<Vec<u8>, Value>,
    /// Global variables
    globals: HashMap<Vec<u8>, Value>,
    /// Class table
    classes: HashMap<Vec<u8>, ClassEntry>,
    /// Next object ID
    next_object_id: u64,
    /// Pending class definitions (from compiler, indexed by position)
    pending_classes: Vec<ClassEntry>,
    /// Whether we're executing the top-level script (vs a function)
    is_global_scope: bool,
    /// User-defined constants (from define())
    pub constants: HashMap<Vec<u8>, Value>,
    /// Current exception being thrown (used during try/catch)
    current_exception: Option<Value>,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            functions: HashMap::new(),
            user_functions: HashMap::new(),
            pending_calls: Vec::new(),
            static_vars: HashMap::new(),
            globals: HashMap::new(),
            classes: HashMap::new(),
            next_object_id: 1,
            pending_classes: Vec::new(),
            is_global_scope: true,
            current_exception: None,
            constants: HashMap::new(),
        }
    }

    /// Register a class (from the compiler's compiled_classes list)
    pub fn register_class(&mut self, class: ClassEntry) {
        self.pending_classes.push(class);
    }

    /// Register a user-defined function
    pub fn register_user_function(&mut self, name: &[u8], op_array: OpArray) {
        self.user_functions
            .insert(name.to_ascii_lowercase(), op_array);
    }

    /// Register a built-in function
    pub fn register_function(&mut self, name: &[u8], func: BuiltinFn) {
        self.functions.insert(name.to_ascii_lowercase(), func);
    }

    /// Get the output buffer contents
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Take the output buffer
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Write to the output buffer
    pub fn write_output(&mut self, data: &[u8]) {
        self.output.extend_from_slice(data);
    }

    /// Execute an op_array (main entry point)
    pub fn execute(&mut self, op_array: &OpArray) -> Result<Value, VmError> {
        self.is_global_scope = true;
        let cvs = vec![Value::Undef; op_array.cv_names.len()];
        let result = self.execute_op_array(op_array, cvs)?;
        Ok(result)
    }

    /// Execute an op_array with pre-initialized CVs
    fn execute_op_array(&mut self, op_array: &OpArray, mut cvs: Vec<Value>) -> Result<Value, VmError> {
        let mut ip: usize = 0;
        let temp_count = op_array.temp_count as usize;
        let mut tmps: Vec<Value> = vec![Value::Undef; temp_count];
        let mut foreach_positions: HashMap<u32, usize> = HashMap::new();
        // Maps CV index -> static var key (for saving back on write)
        let mut static_cv_keys: HashMap<u32, Vec<u8>> = HashMap::new();
        // Exception handler stack: (catch_target, finally_target, exception_tmp_idx)
        let mut exception_handlers: Vec<(u32, u32, u32)> = Vec::new();
        // Maps CV index -> global var name (for saving back on write)
        let mut global_cv_keys: HashMap<u32, Vec<u8>> = HashMap::new();

        loop {
            if ip >= op_array.ops.len() {
                return Ok(Value::Null);
            }

            let op = &op_array.ops[ip];
            ip += 1;

            match op.opcode {
                OpCode::Nop => {}

                OpCode::Echo => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let s = val.to_php_string();
                    self.output.extend_from_slice(s.as_bytes());
                }

                OpCode::Print => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let s = val.to_php_string();
                    self.output.extend_from_slice(s.as_bytes());
                    self.write_operand(&op.result, Value::Long(1), &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::Assign => {
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::Add => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.add(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Sub => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.sub(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Mul => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.mul(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Div => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    match a.div(&b) {
                        Ok(result) => self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Mod => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    match a.modulo(&b) {
                        Ok(result) => self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Pow => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.pow(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Concat => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.concat(&b), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::Negate => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, a.negate(), &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::BitwiseAnd => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() & b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseOr => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() | b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseXor => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long() ^ b.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::BitwiseNot => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(!a.to_long()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::ShiftLeft => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shl(b.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::ShiftRight => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shr(b.to_long() as u32)),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                OpCode::BooleanNot => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.is_truthy() { Value::False } else { Value::True },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }

                // Comparisons
                OpCode::Equal => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::NotEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) { Value::False } else { Value::True },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Identical => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::NotIdentical => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) { Value::False } else { Value::True },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Less => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) < 0 { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::LessEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) <= 0 { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Greater => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) > 0 { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::GreaterEqual => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) >= 0 { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::Spaceship => {
                    let a = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let b = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.compare(&b)), &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Compound assignments
                OpCode::AssignAdd => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.add(&rhs), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignSub => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.sub(&rhs), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignMul => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.mul(&rhs), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignDiv => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    match cv_val.div(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result, &mut cvs, &mut tmps, &static_cv_keys),
                        Err(msg) => return Err(VmError { message: msg.to_string(), line: op.line }),
                    }
                }
                OpCode::AssignMod => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    match cv_val.modulo(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result, &mut cvs, &mut tmps, &static_cv_keys),
                        Err(msg) => return Err(VmError { message: msg.to_string(), line: op.line }),
                    }
                }
                OpCode::AssignPow => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.pow(&rhs), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignConcat => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.concat(&rhs), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignBitwiseAnd => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() & rhs.to_long()), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignBitwiseOr => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() | rhs.to_long()), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignBitwiseXor => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() ^ rhs.to_long()), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignShiftLeft => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long().wrapping_shl(rhs.to_long() as u32)), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::AssignShiftRight => {
                    let cv_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long().wrapping_shr(rhs.to_long() as u32)), &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Increment / Decrement
                OpCode::PreIncrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.result, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PreDecrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.result, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PostIncrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.op1, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::PostDecrement => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                    self.write_operand(&op.op1, new_val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Control flow
                OpCode::Jmp => {
                    if let OperandType::JmpTarget(target) = op.op1 {
                        ip = target as usize;
                    }
                }
                OpCode::JmpZ => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if !val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }
                OpCode::JmpNz => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                // Function calls
                OpCode::InitFCall => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string();
                    self.pending_calls.push(PendingCall {
                        name,
                        args: Vec::new(),
                    });
                }
                OpCode::SendVal => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    if let Some(call) = self.pending_calls.last_mut() {
                        call.args.push(val);
                    }
                }
                OpCode::DoFCall => {
                    let call = self.pending_calls.pop().ok_or_else(|| VmError {
                        message: "no pending function call".into(),
                        line: op.line,
                    })?;

                    let func_name_lower: Vec<u8> = call.name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                    // Handle built-in return (for getter methods on objects)
                    if func_name_lower == b"__builtin_return" {
                        let result = call.args.first().cloned().unwrap_or(Value::Null);
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if let Some(func) = self.functions.get(&func_name_lower).copied() {
                        // Built-in function
                        let result = func(self, &call.args).map_err(|e| VmError {
                            message: e.message,
                            line: op.line,
                        })?;
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if let Some(user_fn) = self.user_functions.get(&func_name_lower).cloned() {
                        // User-defined function - execute its op_array
                        let was_global = self.is_global_scope;
                        self.is_global_scope = false;

                        // Save caller's globals before the call
                        if was_global {
                            for (i, cv) in cvs.iter().enumerate() {
                                if !matches!(cv, Value::Undef) {
                                    if let Some(name) = op_array.cv_names.get(i) {
                                        self.globals.insert(name.clone(), cv.clone());
                                    }
                                }
                            }
                        }

                        // Set up parameters as CVs
                        let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                        for (i, arg) in call.args.iter().enumerate() {
                            if i < func_cvs.len() {
                                func_cvs[i] = arg.clone();
                            }
                        }

                        // Execute the function's op_array
                        let call_result = self.execute_op_array(&user_fn, func_cvs);

                        self.is_global_scope = was_global;

                        let result = match call_result {
                            Ok(v) => v,
                            Err(e) => {
                                // Check if we have an exception handler for uncaught exceptions
                                if let Some(exc) = self.current_exception.take() {
                                    if let Some((catch_target, _, _)) = exception_handlers.pop() {
                                        self.current_exception = Some(exc);
                                        ip = catch_target as usize;
                                        // Reload globals
                                        if was_global {
                                            for (i, name) in op_array.cv_names.iter().enumerate() {
                                                if let Some(val) = self.globals.get(name) {
                                                    if i < cvs.len() { cvs[i] = val.clone(); }
                                                }
                                            }
                                        }
                                        continue;
                                    } else {
                                        self.current_exception = Some(exc);
                                        return Err(e);
                                    }
                                }
                                // Check if there's a stored exception from the called function
                                if let Some(exc) = self.current_exception.take() {
                                    if !exception_handlers.is_empty() {
                                        self.current_exception = Some(exc);
                                        let (catch_target, _, _) = exception_handlers.pop().unwrap();
                                        ip = catch_target as usize;
                                        continue;
                                    } else {
                                        self.current_exception = Some(exc);
                                        return Err(e);
                                    }
                                }
                                return Err(e);
                            }
                        };

                        // Reload globals into caller's CVs after the function returns
                        if was_global {
                            for (i, name) in op_array.cv_names.iter().enumerate() {
                                if let Some(val) = self.globals.get(name) {
                                    if i < cvs.len() {
                                        cvs[i] = val.clone();
                                    }
                                }
                            }
                        } else {
                            // In a non-global calling scope, reload any global-bound CVs
                            for (cv_idx, name) in &global_cv_keys {
                                if let Some(val) = self.globals.get(name) {
                                    if (*cv_idx as usize) < cvs.len() {
                                        cvs[*cv_idx as usize] = val.clone();
                                    }
                                }
                            }
                        }

                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else {
                        // If it's a constructor call and the class has no __construct, silently succeed
                        let name_bytes = call.name.as_bytes();
                        if name_bytes.ends_with(b"::__construct") || name_bytes == b"__construct" {
                            // For Exception-like classes, set message/code from args
                            if !call.args.is_empty() {
                                // First arg (after $this) is message, second is code
                                // args[0] = $this (for method calls)
                                let this_idx = if call.args.len() > 1 { 0 } else { usize::MAX };
                                if this_idx == 0 {
                                    if let Value::Object(obj) = &call.args[0] {
                                        let mut obj_mut = obj.borrow_mut();
                                        if call.args.len() > 1 {
                                            obj_mut.set_property(b"message".to_vec(), call.args[1].clone());
                                        }
                                        if call.args.len() > 2 {
                                            obj_mut.set_property(b"code".to_vec(), call.args[2].clone());
                                        }
                                    }
                                }
                            }
                            self.write_operand(&op.result, Value::Null, &mut cvs, &mut tmps, &static_cv_keys);
                        } else {
                            return Err(VmError {
                                message: format!(
                                    "Call to undefined function {}()",
                                    call.name.to_string_lossy()
                                ),
                                line: op.line,
                            });
                        }
                    }
                }

                OpCode::Return => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    // Save global-bound CVs back to globals
                    for (cv_idx, name) in &global_cv_keys {
                        if let Some(cv_val) = cvs.get(*cv_idx as usize) {
                            self.globals.insert(name.clone(), cv_val.clone());
                        }
                    }
                    // In global scope, save all CVs as globals
                    if self.is_global_scope {
                        for (i, cv) in cvs.iter().enumerate() {
                            if !matches!(cv, Value::Undef) {
                                if let Some(name) = op_array.cv_names.get(i) {
                                    self.globals.insert(name.clone(), cv.clone());
                                }
                            }
                        }
                    }
                    return Ok(val);
                }

                // Casts
                OpCode::CastInt => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(val.to_long()), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::CastFloat => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Double(val.to_double()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastString => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::String(val.to_php_string()),
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastBool => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if val.is_truthy() { Value::True } else { Value::False },
                        &mut cvs,
                        &mut tmps,
                        &static_cv_keys,
                    );
                }
                OpCode::CastArray => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let arr = match val {
                        Value::Array(a) => a,
                        other => {
                            let mut arr = PhpArray::new();
                            arr.push(other);
                            Rc::new(RefCell::new(arr))
                        }
                    };
                    self.write_operand(&op.result, Value::Array(arr), &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::CastObject => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let obj = match val {
                        Value::Object(o) => Value::Object(o),
                        Value::Array(arr) => {
                            let arr_borrow = arr.borrow();
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
                            for (key, value) in arr_borrow.iter() {
                                let prop_name = match key {
                                    ArrayKey::String(s) => s.as_bytes().to_vec(),
                                    ArrayKey::Int(n) => n.to_string().into_bytes(),
                                };
                                obj.set_property(prop_name, value.clone());
                            }
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                        Value::Null | Value::Undef => {
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            Value::Object(Rc::new(RefCell::new(PhpObject::new(b"stdClass".to_vec(), obj_id))))
                        }
                        other => {
                            let obj_id = self.next_object_id;
                            self.next_object_id += 1;
                            let mut obj = PhpObject::new(b"stdClass".to_vec(), obj_id);
                            obj.set_property(b"scalar".to_vec(), other);
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                    };
                    self.write_operand(&op.result, obj, &mut cvs, &mut tmps, &static_cv_keys);
                }

                // Arrays
                OpCode::ArrayNew => {
                    let arr = Rc::new(RefCell::new(PhpArray::new()));
                    self.write_operand(&op.result, Value::Array(arr), &mut cvs, &mut tmps, &static_cv_keys);
                }
                OpCode::ArrayAppend => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        arr.borrow_mut().push(val);
                    }
                }
                OpCode::ArraySet => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        let key = match key_val {
                            Value::Long(n) => ArrayKey::Int(n),
                            Value::String(s) => ArrayKey::String(s),
                            other => ArrayKey::Int(other.to_long()),
                        };
                        arr.borrow_mut().set(key, val);
                    }
                }
                OpCode::ArrayGet => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = match &key_val {
                            Value::Long(n) => ArrayKey::Int(*n),
                            Value::String(s) => ArrayKey::String(s.clone()),
                            other => ArrayKey::Int(other.to_long()),
                        };
                        arr.borrow().get(&key).cloned().unwrap_or(Value::Null)
                    } else if let Value::String(s) = &arr_val {
                        // String offset access
                        let idx = key_val.to_long();
                        let bytes = s.as_bytes();
                        if idx >= 0 && (idx as usize) < bytes.len() {
                            Value::String(PhpString::from_bytes(&[bytes[idx as usize]]))
                        } else {
                            Value::String(PhpString::empty())
                        }
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::ForeachInit => {
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    // Store array in the iterator tmp slot
                    self.write_operand(&op.result, arr_val, &mut cvs, &mut tmps, &static_cv_keys);
                    // Reset iteration position
                    let iter_idx = match op.result { OperandType::Tmp(idx) => idx, _ => 0 };
                    foreach_positions.insert(iter_idx, 0usize);
                }

                OpCode::ForeachNext => {
                    let iter_idx = match op.op1 { OperandType::Tmp(idx) => idx, _ => 0 };
                    let pos = foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        if pos >= entries.len() {
                            // Done - jump to end
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            let (_, value) = entries[pos];
                            self.write_operand(&op.result, value.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                            foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else {
                        // Not an array - jump to end
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::ForeachKey => {
                    let iter_idx = match op.op1 { OperandType::Tmp(idx) => idx, _ => 0 };
                    let pos = foreach_positions.get(&iter_idx).copied().unwrap_or(1);
                    let arr_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        // pos was already incremented by ForeachNext, so use pos - 1
                        let actual_pos = pos.saturating_sub(1);
                        if actual_pos < entries.len() {
                            let (key, _) = entries[actual_pos];
                            let key_val = match key {
                                ArrayKey::Int(n) => Value::Long(*n),
                                ArrayKey::String(s) => Value::String(s.clone()),
                            };
                            self.write_operand(&op.result, key_val, &mut cvs, &mut tmps, &static_cv_keys);
                        }
                    }
                }

                OpCode::BindGlobal => {
                    let name_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    // Load the current global value into the CV
                    if let Some(val) = self.globals.get(&name) {
                        self.write_operand(&op.op1, val.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                    }
                    // Register this CV as global so writes are synced
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        global_cv_keys.insert(cv_idx, name);
                    }
                }

                OpCode::StaticVarInit => {
                    let key_val = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals);
                    let key = key_val.to_php_string().as_bytes().to_vec();

                    if let Some(existing) = self.static_vars.get(&key) {
                        self.write_operand(&op.op1, existing.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                    } else {
                        let default = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                        self.write_operand(&op.op1, default.clone(), &mut cvs, &mut tmps, &static_cv_keys);
                        self.static_vars.insert(key.clone(), default);
                    }

                    // Register this CV as static so writes are persisted
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        static_cv_keys.insert(cv_idx, key);
                    }
                }

                OpCode::DeclareFunction => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let func_idx_val = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let func_idx = func_idx_val.to_long() as usize;

                    if let Some(func_op_array) = op_array.child_functions.get(func_idx) {
                        let name = name_val.to_php_string();
                        self.register_user_function(name.as_bytes(), func_op_array.clone());
                    }
                }

                OpCode::TypeCheck => {
                    // instanceof check: op1 = value, op2 = class name
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let class_name = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_php_string();
                    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                    let result = if let Value::Object(obj) = &val {
                        let obj_borrow = obj.borrow();
                        let obj_class_lower: Vec<u8> = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();

                        if obj_class_lower == class_lower {
                            Value::True
                        } else {
                            // Walk the class hierarchy
                            let mut current = obj_class_lower.clone();
                            let mut found = false;
                            loop {
                                if let Some(class_def) = self.classes.get(&current) {
                                    if let Some(ref parent) = class_def.parent {
                                        let parent_lower: Vec<u8> = parent.iter().map(|b| b.to_ascii_lowercase()).collect();
                                        if parent_lower == class_lower {
                                            found = true;
                                            break;
                                        }
                                        current = parent_lower;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            if found { Value::True } else { Value::False }
                        }
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::TryBegin => {
                    let catch_target = match op.op1 { OperandType::JmpTarget(t) => t, _ => 0 };
                    let finally_target = match op.op2 { OperandType::JmpTarget(t) => t, _ => 0 };
                    // Allocate a tmp to hold the caught exception
                    let exc_tmp = if temp_count > 0 { (temp_count - 1) as u32 } else { 0 };
                    exception_handlers.push((catch_target, finally_target, exc_tmp));
                }

                OpCode::TryEnd => {
                    exception_handlers.pop();
                }

                OpCode::CatchException => {
                    // Store current exception into the CV
                    if let Some(exc) = self.current_exception.take() {
                        self.write_operand(&op.op1, exc, &mut cvs, &mut tmps, &static_cv_keys);
                    }
                }

                OpCode::Throw => {
                    let exc_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);

                    if let Some((catch_target, _finally_target, _exc_tmp)) = exception_handlers.pop() {
                        // Store exception for the catch block to access
                        self.current_exception = Some(exc_val);
                        // Jump to catch handler
                        ip = catch_target as usize;
                    } else {
                        // No handler - store exception and return error
                        let msg = if let Value::Object(obj) = &exc_val {
                            let obj = obj.borrow();
                            let class = String::from_utf8_lossy(&obj.class_name).to_string();
                            let message = obj.get_property(b"message");
                            format!("Uncaught {}: {}", class, message.to_php_string().to_string_lossy())
                        } else {
                            format!("Uncaught exception: {}", exc_val.to_php_string().to_string_lossy())
                        };
                        self.current_exception = Some(exc_val);
                        return Err(VmError {
                            message: msg,
                            line: op.line,
                        });
                    }
                }

                OpCode::StaticPropGet => {
                    let class_name = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals).to_php_string();
                    let prop_name = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_php_string();
                    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                    let val = if let Some(class) = self.classes.get(&class_lower) {
                        // Check static properties first, then constants
                        class.static_properties.get(prop_name.as_bytes()).cloned()
                            .or_else(|| class.constants.get(prop_name.as_bytes()).cloned())
                            .unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::StaticPropSet => {
                    let class_name = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals).to_php_string();
                    let value = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let prop_name = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals).to_php_string();
                    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                    if let Some(class) = self.classes.get_mut(&class_lower) {
                        class.static_properties.insert(prop_name.as_bytes().to_vec(), value);
                    }
                }

                OpCode::ConstLookup => {
                    let name = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals).to_php_string();
                    let name_bytes = name.as_bytes();
                    // Look up in constants table
                    let val = self.constants.get(name_bytes).cloned()
                        .unwrap_or_else(|| {
                            // If not found, return the name as a string (PHP warning: undefined constant)
                            Value::String(name.clone())
                        });
                    self.write_operand(&op.result, val, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::LoadConst | OpCode::FastConcat => {
                    // TODO: implement
                }

                OpCode::DeclareClass => {
                    let name_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let class_idx = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_long() as usize;
                    if let Some(mut class) = self.pending_classes.get(class_idx).cloned() {
                        // Resolve inheritance: copy parent methods/properties
                        if let Some(parent_name) = &class.parent.clone() {
                            let parent_lower: Vec<u8> = parent_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            if let Some(parent) = self.classes.get(&parent_lower).cloned() {
                                // Inherit methods (child overrides take precedence)
                                for (method_name, method) in &parent.methods {
                                    if !class.methods.contains_key(method_name) {
                                        class.methods.insert(method_name.clone(), method.clone());
                                    }
                                }
                                // Inherit properties (child overrides take precedence)
                                let child_prop_names: Vec<Vec<u8>> = class.properties.iter().map(|p| p.name.clone()).collect();
                                for prop in &parent.properties {
                                    if !child_prop_names.contains(&prop.name) {
                                        class.properties.push(prop.clone());
                                    }
                                }
                                // Inherit constants
                                for (const_name, const_val) in &parent.constants {
                                    if !class.constants.contains_key(const_name) {
                                        class.constants.insert(const_name.clone(), const_val.clone());
                                    }
                                }
                                // Inherit static properties
                                for (prop_name, prop_val) in &parent.static_properties {
                                    if !class.static_properties.contains_key(prop_name) {
                                        class.static_properties.insert(prop_name.clone(), prop_val.clone());
                                    }
                                }
                            }
                        }
                        let name_lower: Vec<u8> = name_val.to_php_string().as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        let class_name_orig = name_val.to_php_string().as_bytes().to_vec();

                        // Register all methods as callable functions: ClassName::methodName
                        for (method_name, method) in &class.methods {
                            let mut func_name = class_name_orig.clone();
                            func_name.extend_from_slice(b"::");
                            func_name.extend_from_slice(&method.name);
                            self.user_functions.insert(func_name.to_ascii_lowercase(), method.op_array.clone());
                        }

                        self.classes.insert(name_lower, class);
                    }
                }

                OpCode::NewObject => {
                    let class_name = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals).to_php_string();
                    let name_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();

                    let obj_id = self.next_object_id;
                    self.next_object_id += 1;

                    let mut obj = PhpObject::new(class_name.as_bytes().to_vec(), obj_id);

                    // Built-in Exception/Error classes get default properties
                    if name_lower == b"exception" || name_lower == b"error"
                        || name_lower == b"runtimeexception" || name_lower == b"logicexception"
                        || name_lower == b"invalidargumentexception" || name_lower == b"typeerror"
                        || name_lower == b"valueerror" || name_lower == b"overflowexception"
                        || name_lower == b"underflowexception" || name_lower == b"rangeerror"
                        || name_lower == b"badmethodcallexception" || name_lower == b"badfunctioncallexception"
                        || name_lower == b"lengthexception" || name_lower == b"outofrangeexception"
                        || name_lower == b"unexpectedvalueexception" || name_lower == b"domainexception"
                        || name_lower == b"arithmeticerror" || name_lower == b"divisionbyzeroerror"
                    {
                        obj.set_property(b"message".to_vec(), Value::String(PhpString::empty()));
                        obj.set_property(b"code".to_vec(), Value::Long(0));
                        obj.set_property(b"file".to_vec(), Value::String(PhpString::from_bytes(b"")));
                        obj.set_property(b"line".to_vec(), Value::Long(0));
                    }

                    // Initialize properties from class definition
                    if let Some(class) = self.classes.get(&name_lower) {
                        for prop in &class.properties {
                            if !prop.is_static {
                                obj.set_property(prop.name.clone(), prop.default.clone());
                            }
                        }
                    }

                    self.write_operand(
                        &op.result,
                        Value::Object(Rc::new(RefCell::new(obj))),
                        &mut cvs, &mut tmps, &static_cv_keys,
                    );
                }

                OpCode::PropertyGet => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let prop_name = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_php_string();

                    let result = if let Value::Object(obj) = &obj_val {
                        obj.borrow().get_property(prop_name.as_bytes())
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                }

                OpCode::PropertySet => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let value = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals);
                    let prop_name = self.read_operand(&op.result, &cvs, &tmps, &op_array.literals).to_php_string();

                    if let Value::Object(obj) = &obj_val {
                        obj.borrow_mut().set_property(prop_name.as_bytes().to_vec(), value);
                    }
                }

                OpCode::InitMethodCall => {
                    let obj_val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
                    let method_name = self.read_operand(&op.op2, &cvs, &tmps, &op_array.literals).to_php_string();

                    if let Value::Object(obj) = &obj_val {
                        let class_name_orig;
                        let class_name_lower: Vec<u8>;
                        let method_name_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        let builtin_result;
                        {
                            let obj_borrow = obj.borrow();
                            class_name_orig = obj_borrow.class_name.clone();
                            class_name_lower = obj_borrow.class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                            builtin_result = match method_name_lower.as_slice() {
                                b"getmessage" => Some(obj_borrow.get_property(b"message")),
                                b"getcode" => Some(obj_borrow.get_property(b"code")),
                                b"getfile" => Some(obj_borrow.get_property(b"file")),
                                b"getline" => Some(obj_borrow.get_property(b"line")),
                                b"gettrace" | b"gettracestring" | b"gettracerasstring" | b"getprevious" => Some(Value::Null),
                                b"__tostring" => Some(obj_borrow.get_property(b"message")),
                                _ => None,
                            };
                        } // obj_borrow dropped here

                        if let Some(result) = builtin_result {
                            self.pending_calls.push(PendingCall {
                                name: PhpString::from_bytes(b"__builtin_return"),
                                args: vec![result],
                            });
                        } else
                        // Find the method in the class
                        if let Some(class) = self.classes.get(&class_name_lower) {
                            if let Some(method) = class.get_method(&method_name_lower) {
                                // Create a synthetic function name for the pending call
                                let mut func_name = class_name_orig.clone();
                                func_name.extend_from_slice(b"::");
                                func_name.extend_from_slice(&method.name);

                                // Register the method as a temporary user function
                                let call_name = PhpString::from_vec(func_name.clone());
                                self.user_functions.insert(func_name.to_ascii_lowercase(), method.op_array.clone());

                                // Push the pending call with $this as the first implicit arg
                                self.pending_calls.push(PendingCall {
                                    name: call_name,
                                    args: vec![obj_val.clone()], // $this is first arg, mapped to CV 0
                                });
                            } else {
                                // Method not found - push call with $this
                                self.pending_calls.push(PendingCall {
                                    name: method_name,
                                    args: vec![obj_val.clone()],
                                });
                            }
                        } else {
                            // Class not found in class table - push call with $this
                            self.pending_calls.push(PendingCall {
                                name: method_name,
                                args: vec![obj_val.clone()],
                            });
                        }
                    } else {
                        // Not an object - push call without $this
                        self.pending_calls.push(PendingCall {
                            name: method_name,
                            args: vec![],
                        });
                    }
                }
            }
        }
    }

    fn read_operand(
        &self,
        operand: &OperandType,
        cvs: &[Value],
        tmps: &[Value],
        literals: &[Value],
    ) -> Value {
        match operand {
            OperandType::Cv(idx) => cvs.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Const(idx) => literals.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Tmp(idx) => tmps.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Unused => Value::Null,
            OperandType::JmpTarget(_) => Value::Null,
        }
    }

    fn write_operand(
        &mut self,
        operand: &OperandType,
        value: Value,
        cvs: &mut [Value],
        tmps: &mut [Value],
        static_cv_keys: &HashMap<u32, Vec<u8>>,
    ) {
        match operand {
            OperandType::Cv(idx) => {
                if let Some(slot) = cvs.get_mut(*idx as usize) {
                    *slot = value.clone();
                }
                // If this CV is a static variable, persist the value
                if let Some(key) = static_cv_keys.get(idx) {
                    self.static_vars.insert(key.clone(), value);
                }
            }
            OperandType::Tmp(idx) => {
                if let Some(slot) = tmps.get_mut(*idx as usize) {
                    *slot = value;
                }
            }
            _ => {}
        }
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}
