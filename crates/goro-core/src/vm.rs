use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
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
}

impl Vm {
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            functions: HashMap::new(),
            user_functions: HashMap::new(),
            pending_calls: Vec::new(),
            static_vars: HashMap::new(),
        }
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
        let cvs = vec![Value::Undef; op_array.cv_names.len()];
        self.execute_op_array(op_array, cvs)
    }

    /// Execute an op_array with pre-initialized CVs
    fn execute_op_array(&mut self, op_array: &OpArray, mut cvs: Vec<Value>) -> Result<Value, VmError> {
        let mut ip: usize = 0;
        let temp_count = op_array.temp_count as usize;
        let mut tmps: Vec<Value> = vec![Value::Undef; temp_count];
        let mut foreach_positions: HashMap<u32, usize> = HashMap::new();
        // Maps CV index -> static var key (for saving back on write)
        let mut static_cv_keys: HashMap<u32, Vec<u8>> = HashMap::new();

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

                    if let Some(func) = self.functions.get(&func_name_lower).copied() {
                        // Built-in function
                        let result = func(self, &call.args).map_err(|e| VmError {
                            message: e.message,
                            line: op.line,
                        })?;
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
                    } else if let Some(user_fn) = self.user_functions.get(&func_name_lower).cloned() {
                        // User-defined function - execute its op_array
                        // Set up parameters as CVs
                        let mut func_cvs = vec![Value::Undef; user_fn.cv_names.len()];
                        for (i, arg) in call.args.iter().enumerate() {
                            if i < func_cvs.len() {
                                func_cvs[i] = arg.clone();
                            }
                        }

                        // Execute the function's op_array
                        let result = self.execute_op_array(&user_fn, func_cvs)?;
                        self.write_operand(&op.result, result, &mut cvs, &mut tmps, &static_cv_keys);
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

                OpCode::Return => {
                    let val = self.read_operand(&op.op1, &cvs, &tmps, &op_array.literals);
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

                OpCode::LoadConst | OpCode::FastConcat | OpCode::TypeCheck => {
                    // TODO: implement
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
