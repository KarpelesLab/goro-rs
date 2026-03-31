use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::opcode::{OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};

/// The state of a PHP generator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratorState {
    /// Generator has been created but not yet started (before first next()/rewind())
    Created,
    /// Generator is suspended at a yield point
    Suspended,
    /// Generator has completed (returned or no more yields)
    Completed,
}

/// A PHP Generator object
///
/// Generators are functions that use `yield` to produce values lazily.
/// When called, a generator function returns a Generator object that
/// implements the Iterator interface.
#[derive(Debug, Clone)]
pub struct PhpGenerator {
    /// The compiled function body
    pub op_array: OpArray,
    /// Current instruction pointer (where to resume)
    pub ip: usize,
    /// Compiled variables (function locals)
    pub cvs: Vec<Value>,
    /// Temporary variables
    pub tmps: Vec<Value>,
    /// Foreach iteration positions
    pub foreach_positions: HashMap<u32, usize>,
    /// Static variable keys
    pub static_cv_keys: HashMap<u32, Vec<u8>>,
    /// Global variable keys
    pub global_cv_keys: HashMap<u32, Vec<u8>>,
    /// Exception handlers
    pub exception_handlers: Vec<(u32, u32, u32)>,
    /// Current state
    pub state: GeneratorState,
    /// The current yielded value
    pub current_value: Value,
    /// The current yield key (auto-incrementing integer by default)
    pub current_key: Value,
    /// Auto-increment key counter
    pub key_counter: i64,
    /// Value sent to the generator via send()
    pub send_value: Value,
    /// The return value of the generator (set when it returns)
    pub return_value: Value,
    /// Yield from: inner iterable being delegated to
    pub yield_from_source: Option<Value>,
    /// Yield from: position within array iteration
    pub yield_from_pos: usize,
    /// When fiber suspension happens during DoFCall, this stores the result operand
    /// that should receive the resume value when the fiber resumes
    pub fiber_suspend_result: Option<crate::opcode::OperandType>,
    /// Whether this generator is currently running (executing its body).
    /// Used to detect and prevent recursive resumption.
    pub is_running: bool,
}

impl PhpGenerator {
    /// Create a new generator from a compiled op_array and initial CVs
    pub fn new(op_array: OpArray, cvs: Vec<Value>) -> Self {
        let temp_count = op_array.temp_count as usize;
        Self {
            op_array,
            ip: 0,
            cvs,
            tmps: vec![Value::Undef; temp_count],
            foreach_positions: HashMap::new(),
            static_cv_keys: HashMap::new(),
            global_cv_keys: HashMap::new(),
            exception_handlers: Vec::new(),
            state: GeneratorState::Created,
            current_value: Value::Null,
            current_key: Value::Null,
            key_counter: 0,
            send_value: Value::Null,
            return_value: Value::Null,
            yield_from_source: None,
            yield_from_pos: 0,
            fiber_suspend_result: None,
            is_running: false,
        }
    }

    /// Resume execution of the generator until the next yield or return.
    /// Returns Ok(true) if a value was yielded, Ok(false) if the generator completed.
    pub fn resume(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }
        if self.is_running {
            let err_msg = "Cannot resume an already running generator";
            let exc = vm.create_exception(b"Error", err_msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: format!("Uncaught Error: {}", err_msg), line: 0 });
        }

        // Track call depth to prevent stack overflow from recursive yield-from
        vm.enter_generator_resume(0)?;

        self.is_running = true;
        let result = self.resume_inner(vm);
        self.is_running = false;

        vm.leave_generator_resume();

        result
    }

    /// Inner resume implementation (depth check is done by the caller)
    fn resume_inner(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        let op_array = self.op_array.clone();
        let mut ip = self.ip;

        loop {
            if ip >= op_array.ops.len() {
                self.state = GeneratorState::Completed;
                self.ip = ip;
                return Ok(false);
            }

            let op = &op_array.ops[ip];
            ip += 1;

            match op.opcode {
                OpCode::Yield => {
                    // op1 = value to yield (or Unused for bare yield)
                    // op2 = key to yield (or Unused for auto-key)
                    // result = where to store the sent value (or Unused)
                    let yielded_value = if matches!(op.op1, OperandType::Unused) {
                        Value::Null
                    } else {
                        self.read_operand(&op.op1, &op_array.literals)
                    };

                    let yielded_key = if matches!(op.op2, OperandType::Unused) {
                        let key = Value::Long(self.key_counter);
                        self.key_counter += 1;
                        key
                    } else {
                        let key = self.read_operand(&op.op2, &op_array.literals);
                        // Update key_counter to be after the explicit key
                        if let Value::Long(n) = &key {
                            if *n >= self.key_counter {
                                self.key_counter = *n + 1;
                            }
                        }
                        key
                    };

                    self.current_value = yielded_value;
                    self.current_key = yielded_key;
                    self.state = GeneratorState::Suspended;
                    self.ip = ip;

                    // The send value will be written to the result operand when we resume
                    return Ok(true);
                }

                OpCode::YieldFrom => {
                    // Check if we're resuming a yield-from iteration
                    if let Some(ref source) = self.yield_from_source.clone() {
                        match source {
                            Value::Array(arr) => {
                                let arr_borrow = arr.borrow();
                                let entries: Vec<_> = arr_borrow.iter().collect();
                                if self.yield_from_pos < entries.len() {
                                    let (key, val) = entries[self.yield_from_pos];
                                    let k = match key {
                                        crate::array::ArrayKey::Int(n) => Value::Long(*n),
                                        crate::array::ArrayKey::String(s) => {
                                            Value::String(s.clone())
                                        }
                                    };
                                    self.current_value = val.clone();
                                    self.current_key = k;
                                    self.yield_from_pos += 1;
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip - 1; // Re-execute YieldFrom on next resume
                                    return Ok(true);
                                }
                                // Array exhausted
                                self.yield_from_source = None;
                                self.yield_from_pos = 0;
                                self.write_operand(&op.result, Value::Null);
                            }
                            Value::Generator(inner_gen) => {
                                // Read inner state without holding borrow
                                let (is_suspended, val, key, ret) = {
                                    let inner = inner_gen.borrow();
                                    (
                                        inner.state == GeneratorState::Suspended,
                                        inner.current_value.clone(),
                                        inner.current_key.clone(),
                                        inner.return_value.clone(),
                                    )
                                };
                                if is_suspended {
                                    self.current_value = val;
                                    self.current_key = key;
                                    self.state = GeneratorState::Suspended;
                                    self.ip = ip - 1; // Re-execute YieldFrom on resume
                                    // Advance inner generator for next resume
                                    let mut inner = inner_gen.borrow_mut();
                                    inner.write_send_value();
                                    let _ = inner.resume(vm);
                                    return Ok(true);
                                }
                                // Inner completed
                                self.yield_from_source = None;
                                self.write_operand(&op.result, ret);
                            }
                            _ => {
                                self.yield_from_source = None;
                                self.write_operand(&op.result, Value::Null);
                            }
                        }
                    } else {
                        // First time: set up the yield-from source
                        let iterable = self.read_operand(&op.op1, &op_array.literals);
                        match &iterable {
                            Value::Array(_) => {
                                self.yield_from_source = Some(iterable);
                                self.yield_from_pos = 0;
                                // Re-execute to start yielding
                                ip -= 1; // Will re-execute this opcode
                            }
                            Value::Generator(inner_gen) => {
                                // Initialize inner generator if needed
                                let mut inner = inner_gen.borrow_mut();
                                if inner.state == GeneratorState::Created {
                                    let _ = inner.resume(vm);
                                }
                                drop(inner);
                                self.yield_from_source = Some(iterable);
                                ip -= 1; // Re-execute to start yielding
                            }
                            _ => {
                                // Non-iterable - throw error
                                return Err(crate::vm::VmError {
                                    message:
                                        "Can use \"yield from\" only with arrays and Traversables"
                                            .to_string(),
                                    line: op.line,
                                });
                            }
                        }
                    }
                }

                OpCode::GeneratorReturn => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.return_value = val;
                    self.state = GeneratorState::Completed;
                    self.current_value = Value::Null;
                    self.ip = ip;
                    return Ok(false);
                }

                OpCode::Return => {
                    // In a generator, return acts as GeneratorReturn
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.return_value = val;
                    self.state = GeneratorState::Completed;
                    self.current_value = Value::Null;
                    self.ip = ip;
                    return Ok(false);
                }

                // Re-implement all the VM opcodes that a generator might use
                OpCode::Nop => {}

                OpCode::Echo => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let s = vm.value_to_string(&val);
                    vm.write_output(s.as_bytes());
                }

                OpCode::Print => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let s = val.to_php_string();
                    vm.write_output(s.as_bytes());
                    self.write_operand(&op.result, Value::Long(1));
                }

                OpCode::Assign => {
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, val);
                }

                OpCode::AssignRef => {
                    if let (OperandType::Cv(target_idx), OperandType::Cv(value_idx)) =
                        (op.op1, op.op2)
                    {
                        let ti = target_idx as usize;
                        let vi = value_idx as usize;
                        let ref_cell = if let Value::Reference(r) = &self.cvs[vi] {
                            r.clone()
                        } else {
                            let r = Rc::new(RefCell::new(self.cvs[vi].clone()));
                            self.cvs[vi] = Value::Reference(r.clone());
                            r
                        };
                        self.cvs[ti] = Value::Reference(ref_cell);
                    }
                }

                OpCode::Add => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.add(&b));
                }
                OpCode::Sub => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.sub(&b));
                }
                OpCode::Mul => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.mul(&b));
                }
                OpCode::Div => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    match a.div(&b) {
                        Ok(result) => self.write_operand(&op.result, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Mod => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    match a.modulo(&b) {
                        Ok(result) => self.write_operand(&op.result, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::Pow => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, a.pow(&b));
                }
                OpCode::Concat => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let a_str = vm.value_to_string(&a);
                    let b_str = vm.value_to_string(&b);
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(&op.result, Value::String(PhpString::from_vec(result)));
                }
                OpCode::Negate => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, a.negate());
                }

                OpCode::BitwiseAnd => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() & b.to_long()));
                }
                OpCode::BitwiseOr => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() | b.to_long()));
                }
                OpCode::BitwiseXor => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.to_long() ^ b.to_long()));
                }
                OpCode::BoolXor => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let result = if a.is_truthy() ^ b.is_truthy() { Value::True } else { Value::False };
                    self.write_operand(&op.result, result);
                }
                OpCode::BitwiseNot => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(!a.to_long()));
                }
                OpCode::ShiftLeft => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shl(b.to_long() as u32)),
                    );
                }
                OpCode::ShiftRight => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        Value::Long(a.to_long().wrapping_shr(b.to_long() as u32)),
                    );
                }

                OpCode::BooleanNot => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.is_truthy() {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }

                // Comparisons
                OpCode::Equal => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::NotEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.equals(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }
                OpCode::Identical => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::NotIdentical => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.identical(&b) {
                            Value::False
                        } else {
                            Value::True
                        },
                    );
                }
                OpCode::Less => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) < 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::LessEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) <= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::Greater => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) > 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::GreaterEqual => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if a.compare(&b) >= 0 {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::Spaceship => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(a.compare(&b)));
                }

                // Compound assignments
                OpCode::AssignAdd => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.add(&rhs));
                }
                OpCode::AssignSub => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.sub(&rhs));
                }
                OpCode::AssignMul => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.mul(&rhs));
                }
                OpCode::AssignDiv => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    match cv_val.div(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignMod => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    match cv_val.modulo(&rhs) {
                        Ok(result) => self.write_operand(&op.op1, result),
                        Err(msg) => {
                            return Err(VmError {
                                message: msg.to_string(),
                                line: op.line,
                            });
                        }
                    }
                }
                OpCode::AssignPow => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.pow(&rhs));
                }
                OpCode::AssignConcat => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, cv_val.concat(&rhs));
                }
                OpCode::AssignBitwiseAnd => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() & rhs.to_long()));
                }
                OpCode::AssignBitwiseOr => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() | rhs.to_long()));
                }
                OpCode::AssignBitwiseXor => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(&op.op1, Value::Long(cv_val.to_long() ^ rhs.to_long()));
                }
                OpCode::AssignShiftLeft => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shl(rhs.to_long() as u32)),
                    );
                }
                OpCode::AssignShiftRight => {
                    let cv_val = self.read_operand(&op.op1, &op_array.literals);
                    let rhs = self.read_operand(&op.op2, &op_array.literals);
                    self.write_operand(
                        &op.op1,
                        Value::Long(cv_val.to_long().wrapping_shr(rhs.to_long() as u32)),
                    );
                }

                // Increment / Decrement
                OpCode::PreIncrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone());
                    self.write_operand(&op.result, new_val);
                }
                OpCode::PreDecrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.op1, new_val.clone());
                    self.write_operand(&op.result, new_val);
                }
                OpCode::PostIncrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.add(&Value::Long(1));
                    self.write_operand(&op.result, val);
                    self.write_operand(&op.op1, new_val);
                }
                OpCode::PostDecrement => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let new_val = val.sub(&Value::Long(1));
                    self.write_operand(&op.result, val);
                    self.write_operand(&op.op1, new_val);
                }

                // Control flow
                OpCode::Jmp => {
                    if let OperandType::JmpTarget(target) = op.op1 {
                        ip = target as usize;
                    }
                }
                OpCode::JmpZ => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    if !val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }
                OpCode::JmpNz => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    if val.is_truthy() {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                // Function calls - delegate to VM
                OpCode::InitFCall => {
                    let name_val = self.read_operand(&op.op1, &op_array.literals);
                    vm.generator_init_fcall(name_val);
                }
                OpCode::InitDynamicStaticCall => {
                    let class_val = self.read_operand(&op.op1, &op_array.literals);
                    let method_val = self.read_operand(&op.op2, &op_array.literals);
                    let method_name = method_val.to_php_string();
                    let class_name = match &class_val {
                        Value::Object(obj) => obj.borrow().class_name.clone(),
                        Value::String(s) => s.as_bytes().to_vec(),
                        _ => class_val.to_php_string().as_bytes().to_vec(),
                    };
                    let mut func_name = class_name;
                    func_name.extend_from_slice(b"::");
                    func_name.extend_from_slice(method_name.as_bytes());
                    vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                }
                OpCode::SendVal => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    vm.generator_send_val(val);
                }
                OpCode::SendNamedVal => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let name_val = self.read_operand(&op.op2, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    vm.generator_send_named_val(name, val);
                }
                OpCode::DoFCall => {
                    match vm.generator_do_fcall(op.line) {
                        Ok(result) => {
                            self.write_operand(&op.result, result);
                        }
                        Err(e) => {
                            if vm.fiber_suspended {
                                // Fiber::suspend() was called from within a nested function.
                                // Save the generator state so we can resume later.
                                // ip currently points past the DoFCall; we save it there
                                // because on resume, the send_value will be written to
                                // the result operand as the return value of Fiber::suspend().
                                self.ip = ip;
                                // Store which operand should receive the resume value
                                self.fiber_suspend_result = Some(op.result);
                                return Err(e);
                            }
                            // Check for exception handling in the generator
                            if vm.current_exception.is_some() {
                                if let Some((catch_target, _, _)) = self.exception_handlers.last() {
                                    let ct = *catch_target as usize;
                                    self.exception_handlers.pop();
                                    ip = ct;
                                    continue;
                                }
                            }
                            // Unhandled exception from function call - generator is done
                            self.state = GeneratorState::Completed;
                            self.current_value = Value::Null;
                            self.ip = ip;
                            return Err(e);
                        }
                    }
                }

                // Casts
                OpCode::CastInt => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Long(val.to_long()));
                }
                OpCode::CastFloat => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::Double(val.to_double()));
                }
                OpCode::CastString => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, Value::String(val.to_php_string()));
                }
                OpCode::CastBool => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(
                        &op.result,
                        if val.is_truthy() {
                            Value::True
                        } else {
                            Value::False
                        },
                    );
                }
                OpCode::CastArray => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let arr = match val {
                        Value::Array(a) => a,
                        other => {
                            let mut arr = crate::array::PhpArray::new();
                            arr.push(other);
                            Rc::new(RefCell::new(arr))
                        }
                    };
                    self.write_operand(&op.result, Value::Array(arr));
                }
                OpCode::CastObject => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let obj = match val {
                        Value::Object(o) => Value::Object(o),
                        _ => {
                            // Simplified - just create an empty stdClass for now
                            let obj = crate::object::PhpObject::new(b"stdClass".to_vec(), 0);
                            Value::Object(Rc::new(RefCell::new(obj)))
                        }
                    };
                    self.write_operand(&op.result, obj);
                }

                // Arrays
                OpCode::ArrayNew => {
                    let arr = Rc::new(RefCell::new(crate::array::PhpArray::new()));
                    self.write_operand(&op.result, Value::Array(arr));
                }
                OpCode::ArrayAppend => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        arr.borrow_mut().push(val);
                    }
                }
                OpCode::ArraySet => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let val = self.read_operand(&op.op2, &op_array.literals);
                    let key_val = self.read_operand(&op.result, &op_array.literals);
                    if let Value::Array(arr) = arr_val {
                        let key = Vm::value_to_array_key(key_val);
                        arr.borrow_mut().set(key, val);
                    }
                }
                OpCode::ArrayGet | OpCode::ListGet => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Vm::value_to_array_key(key_val.clone());
                        arr.borrow().get(&key).cloned().unwrap_or(Value::Null)
                    } else if matches!(op.opcode, OpCode::ArrayGet) {
                        if let Value::String(s) = &arr_val {
                            let idx = key_val.to_long();
                            let bytes = s.as_bytes();
                            if idx >= 0 && (idx as usize) < bytes.len() {
                                Value::String(PhpString::from_bytes(&[bytes[idx as usize]]))
                            } else {
                                Value::String(PhpString::empty())
                            }
                        } else {
                            Value::Null
                        }
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result);
                }

                // Foreach within generators
                OpCode::ForeachInit => {
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, arr_val);
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    self.foreach_positions.insert(iter_idx, 0usize);
                }
                OpCode::ForeachNext => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        if pos >= entries.len() {
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            let (_, value) = entries[pos];
                            self.write_operand(&op.result, value.clone());
                            self.foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }
                OpCode::ForeachKey => {
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(1);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        let actual_pos = pos.saturating_sub(1);
                        if actual_pos < entries.len() {
                            let (key, _) = entries[actual_pos];
                            let key_val = match key {
                                crate::array::ArrayKey::Int(n) => Value::Long(*n),
                                crate::array::ArrayKey::String(s) => Value::String(s.clone()),
                            };
                            self.write_operand(&op.result, key_val);
                        }
                    }
                }

                OpCode::ForeachInitRef => {
                    // In generators, treat by-ref foreach like by-value
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    self.write_operand(&op.result, arr_val);
                    let iter_idx = match op.result {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    self.foreach_positions.insert(iter_idx, 0usize);
                }
                OpCode::ForeachNextRef => {
                    // In generators, treat by-ref foreach like by-value
                    let iter_idx = match op.op1 {
                        OperandType::Tmp(idx) => idx,
                        _ => 0,
                    };
                    let pos = self.foreach_positions.get(&iter_idx).copied().unwrap_or(0);
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    if let Value::Array(arr) = &arr_val {
                        let arr_borrow = arr.borrow();
                        let entries: Vec<_> = arr_borrow.iter().collect();
                        if pos >= entries.len() {
                            if let OperandType::JmpTarget(target) = op.op2 {
                                ip = target as usize;
                            }
                        } else {
                            let (_, value) = entries[pos];
                            self.write_operand(&op.result, value.clone());
                            self.foreach_positions.insert(iter_idx, pos + 1);
                        }
                    } else {
                        if let OperandType::JmpTarget(target) = op.op2 {
                            ip = target as usize;
                        }
                    }
                }

                OpCode::ConstLookup => {
                    let name = self
                        .read_operand(&op.op1, &op_array.literals)
                        .to_php_string();
                    let val = vm.lookup_constant(name.as_bytes());
                    self.write_operand(&op.result, val);
                }

                OpCode::DeclareFunction => {
                    let name_val = self.read_operand(&op.op1, &op_array.literals);
                    let func_idx_val = self.read_operand(&op.op2, &op_array.literals);
                    let func_idx = func_idx_val.to_long() as usize;
                    if let Some(func_op_array) = op_array.child_functions.get(func_idx) {
                        let name = name_val.to_php_string();
                        vm.register_user_function(name.as_bytes(), func_op_array.clone());
                    }
                }

                OpCode::StaticVarInit => {
                    let key_val = self.read_operand(&op.result, &op_array.literals);
                    let key = key_val.to_php_string().as_bytes().to_vec();
                    if let Some(existing) = vm.get_static_var(&key) {
                        self.write_operand(&op.op1, existing);
                    } else {
                        let default = self.read_operand(&op.op2, &op_array.literals);
                        self.write_operand(&op.op1, default.clone());
                        vm.set_static_var(key.clone(), default);
                    }
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        self.static_cv_keys.insert(cv_idx, key);
                    }
                }

                OpCode::BindGlobal => {
                    let name_val = self.read_operand(&op.op2, &op_array.literals);
                    let name = name_val.to_php_string().as_bytes().to_vec();
                    if let Some(val) = vm.get_global(&name) {
                        self.write_operand(&op.op1, val);
                    }
                    if let OperandType::Cv(cv_idx) = op.op1 {
                        self.global_cv_keys.insert(cv_idx, name);
                    }
                }

                OpCode::Throw => {
                    let exc_val = self.read_operand(&op.op1, &op_array.literals);
                    vm.current_exception = Some(exc_val);
                    if let Some((catch_target, _, _)) = self.exception_handlers.pop() {
                        ip = catch_target as usize;
                    } else {
                        self.state = GeneratorState::Completed;
                        self.current_value = Value::Null;
                        self.ip = ip;
                        return Err(VmError {
                            message: "Uncaught exception in generator".to_string(),
                            line: op.line,
                        });
                    }
                }

                OpCode::TryBegin => {
                    if let (OperandType::JmpTarget(catch), OperandType::JmpTarget(_finally)) =
                        (op.op1, op.op2)
                    {
                        self.exception_handlers.push((catch, 0, 0));
                    }
                }

                OpCode::TryEnd => {
                    self.exception_handlers.pop();
                }

                OpCode::CatchException => {
                    if let Some(exc) = vm.current_exception.take() {
                        self.write_operand(&op.op1, exc);
                    }
                }

                OpCode::TypeCheck => {
                    let exc_val = self.read_operand(&op.op1, &op_array.literals);
                    let type_name = self.read_operand(&op.op2, &op_array.literals);
                    let type_str = type_name.to_php_string();
                    let type_lower: Vec<u8> = type_str.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let matches = if let Value::Object(obj) = &exc_val {
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        class_lower == type_lower
                            || crate::vm::is_builtin_subclass(&class_lower, &type_lower)
                            || vm.class_extends(&class_lower, &type_lower)
                            || type_lower == b"throwable"
                    } else {
                        false
                    };
                    self.write_operand(&op.result, if matches { Value::True } else { Value::False });
                }

                OpCode::PropertyGet => {
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let prop_name = self.read_operand(&op.op2, &op_array.literals).to_php_string();
                    let result = if let Value::Object(obj) = &obj_val {
                        obj.borrow().get_property(prop_name.as_bytes())
                    } else {
                        Value::Null
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::PropertySet => {
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let prop_name = self.read_operand(&op.op2, &op_array.literals).to_php_string();
                    let val = self.read_operand(&op.result, &op_array.literals);
                    if let Value::Object(obj) = &obj_val {
                        obj.borrow_mut().set_property(prop_name.as_bytes().to_vec(), val);
                    } else if let Value::Generator(_) = &obj_val {
                        let prop_str = prop_name.to_string_lossy();
                        let msg = format!("Cannot create dynamic property Generator::${}", prop_str);
                        let exc = vm.create_exception(b"Error", &msg, op.line);
                        vm.current_exception = Some(exc);
                        return Err(VmError { message: format!("Uncaught Error: {}", msg), line: op.line });
                    }
                }

                OpCode::IssetCheck => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let result = match val {
                        Value::Null | Value::Undef => Value::False,
                        _ => Value::True,
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::ArrayIsset => {
                    // isset($arr[$key]) in generator context
                    let arr_val = self.read_operand(&op.op1, &op_array.literals);
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let result = if let Value::Array(arr) = &arr_val {
                        let key = Vm::value_to_array_key(key_val.clone());
                        match arr.borrow().get(&key) {
                            Some(v) if !matches!(v, Value::Null | Value::Undef) => Value::True,
                            _ => Value::False,
                        }
                    } else if let Value::Object(obj) = &arr_val {
                        let class_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let is_spl_array = matches!(class_lower.as_slice(),
                            b"arrayobject" | b"arrayiterator" | b"recursivearrayiterator" |
                            b"splfixedarray" | b"splobjectstorage");
                        let has_user_offset = vm.classes.get(&class_lower)
                            .map(|c| c.get_method(b"offsetexists").is_some())
                            .unwrap_or(false);
                        if is_spl_array || has_user_offset {
                            let args = vec![arr_val.clone(), key_val.clone()];
                            let exists_result = vm.handle_spl_docall(&class_lower, b"offsetexists", &args)
                                .unwrap_or_else(|| {
                                    vm.call_object_method(&arr_val, b"offsetexists", &[key_val])
                                        .unwrap_or(Value::False)
                                });
                            if exists_result.is_truthy() { Value::True } else { Value::False }
                        } else {
                            Value::False
                        }
                    } else if let Value::String(s) = &arr_val {
                        let idx = key_val.to_long();
                        let len = s.as_bytes().len() as i64;
                        if (idx >= 0 && idx < len) || (idx < 0 && (-idx) <= len) {
                            Value::True
                        } else {
                            Value::False
                        }
                    } else {
                        Value::False
                    };
                    self.write_operand(&op.result, result);
                }

                OpCode::ErrorSuppress => {
                    vm.error_reporting_stack.push(vm.error_reporting);
                    vm.error_reporting = 0;
                }

                OpCode::ErrorRestore => {
                    if let Some(saved) = vm.error_reporting_stack.pop() {
                        vm.error_reporting = saved;
                    }
                }

                OpCode::UnsetCv => {
                    if let OperandType::Cv(idx) = op.op1 {
                        if let Some(slot) = self.cvs.get_mut(idx as usize) {
                            *slot = Value::Undef;
                        }
                    }
                }

                OpCode::ArrayUnset => {
                    let key_val = self.read_operand(&op.op2, &op_array.literals);
                    let arr_val = if let OperandType::Cv(idx) = &op.op1 {
                        self.cvs.get(*idx as usize).cloned()
                    } else {
                        None
                    };
                    if let Some(Value::Array(arr)) = arr_val {
                        let key = Vm::value_to_array_key(key_val);
                        arr.borrow_mut().remove(&key);
                    }
                }

                OpCode::CloneObj => {
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    let cloned = match &val {
                        Value::Object(obj) => {
                            let obj_borrow = obj.borrow();
                            let clone_id = vm.next_object_id();
                            let mut new_obj = crate::object::PhpObject::new(obj_borrow.class_name.clone(), clone_id);
                            for (name, value) in &obj_borrow.properties {
                                new_obj.set_property(name.clone(), value.clone());
                            }
                            Value::Object(Rc::new(RefCell::new(new_obj)))
                        }
                        other => other.clone(),
                    };
                    self.write_operand(&op.result, cloned);
                }

                OpCode::ArraySpread => {
                    let target_val = if let OperandType::Tmp(idx) = &op.op1 {
                        self.tmps.get(*idx as usize).cloned()
                    } else if let OperandType::Cv(idx) = &op.op1 {
                        self.cvs.get(*idx as usize).cloned()
                    } else {
                        None
                    };
                    let source = self.read_operand(&op.op2, &op_array.literals);
                    if let (Some(Value::Array(target)), Value::Array(source_arr)) = (target_val, source) {
                        let source_borrow = source_arr.borrow();
                        let mut target_borrow = target.borrow_mut();
                        for (key, val) in source_borrow.iter() {
                            match key {
                                crate::array::ArrayKey::Int(_) => {
                                    target_borrow.push(val.clone());
                                }
                                crate::array::ArrayKey::String(s) => {
                                    target_borrow.set(crate::array::ArrayKey::String(s.clone()), val.clone());
                                }
                            }
                        }
                    }
                }

                OpCode::InitMethodCall => {
                    // Set up method call from generator context
                    let obj_val = self.read_operand(&op.op1, &op_array.literals);
                    let method_name = self.read_operand(&op.op2, &op_array.literals).to_php_string();
                    if let Value::Object(obj) = &obj_val {
                        let class_name_lower: Vec<u8> = obj.borrow().class_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                        let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        // Check if method exists in user classes
                        let has_method = vm.classes.get(&class_name_lower)
                            .map(|c| c.get_method(&method_lower).is_some())
                            .unwrap_or(false);
                        // Handle built-in Throwable methods in generator context
                        let is_throwable = class_name_lower == b"exception"
                            || class_name_lower == b"error"
                            || crate::vm::is_builtin_subclass(&class_name_lower, b"exception")
                            || crate::vm::is_builtin_subclass(&class_name_lower, b"error")
                            || vm.class_extends(&class_name_lower, b"exception")
                            || vm.class_extends(&class_name_lower, b"error");
                        if is_throwable && !has_method {
                            let obj_borrow = obj.borrow();
                            let builtin_result = match method_lower.as_slice() {
                                b"getmessage" => Some(obj_borrow.get_property(b"message")),
                                b"getcode" => Some(obj_borrow.get_property(b"code")),
                                b"getfile" => Some(obj_borrow.get_property(b"file")),
                                b"getline" => Some(obj_borrow.get_property(b"line")),
                                b"gettrace" => Some(obj_borrow.get_property(b"trace")),
                                b"getprevious" => Some(obj_borrow.get_property(b"previous")),
                                _ => None,
                            };
                            drop(obj_borrow);
                            if let Some(result) = builtin_result {
                                vm.generator_init_fcall(Value::String(PhpString::from_bytes(b"__builtin_return")));
                                vm.generator_send_val(result);
                            } else {
                                // Fall through to general handling
                                let mut func_name = class_name_lower.clone();
                                func_name.extend_from_slice(b"::");
                                func_name.extend_from_slice(&method_lower);
                                vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                                vm.generator_send_val(obj_val.clone());
                            }
                        } else
                        // Handle Fiber methods in generator context
                        if class_name_lower == b"fiber" {
                            match method_lower.as_slice() {
                                b"__construct" => {
                                    // Fiber constructor - set up as fiber::__construct
                                    vm.generator_init_fcall(Value::String(PhpString::from_bytes(b"fiber::__construct")));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                b"isstarted" | b"isrunning" | b"issuspended" | b"isterminated" | b"getreturn" => {
                                    let mut fiber_name = b"__fiber::".to_vec();
                                    fiber_name.extend_from_slice(&method_lower);
                                    vm.generator_init_fcall(Value::String(PhpString::from_vec(fiber_name)));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                b"start" | b"resume" | b"throw" => {
                                    let mut fiber_name = b"__fiber::".to_vec();
                                    fiber_name.extend_from_slice(&method_lower);
                                    vm.generator_init_fcall(Value::String(PhpString::from_vec(fiber_name)));
                                    vm.generator_send_val(obj_val.clone());
                                }
                                _ => {
                                    self.state = GeneratorState::Completed;
                                    self.ip = ip;
                                    return Err(VmError {
                                        message: format!("Call to undefined method Fiber::{}()", method_name.to_string_lossy()),
                                        line: op.line,
                                    });
                                }
                            }
                        } else if has_method {
                            // Set up as a method call: name = "class::method", first arg = $this
                            let mut func_name = class_name_lower.clone();
                            func_name.extend_from_slice(b"::");
                            func_name.extend_from_slice(&method_lower);
                            vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                            vm.generator_send_val(obj_val.clone());
                        } else {
                            // Check for __call magic method
                            let has_call = vm.classes.get(&class_name_lower)
                                .map(|c| c.get_method(b"__call").is_some())
                                .unwrap_or(false);
                            if has_call {
                                let mut func_name = class_name_lower.clone();
                                func_name.extend_from_slice(b"::__call");
                                vm.generator_init_fcall(Value::String(PhpString::from_vec(func_name)));
                                vm.generator_send_val(obj_val.clone());
                                // __call receives (method_name, args_array) - will need special handling
                            } else {
                                let class_name_vec = obj.borrow().class_name.clone();
                                let class_name_str = String::from_utf8_lossy(&class_name_vec);
                                let method_str = method_name.to_string_lossy();
                                self.state = GeneratorState::Completed;
                                self.ip = ip;
                                return Err(VmError {
                                    message: format!("Call to undefined method {}::{}()", class_name_str, method_str),
                                    line: op.line,
                                });
                            }
                        }
                    } else if let Value::Generator(inner_gen_rc) = &obj_val {
                        // Calling a method on another generator from within this generator
                        let method_lower: Vec<u8> = method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                        // Check if the inner generator is already running (to prevent
                        // "Cannot resume an already running generator" crash/panic).
                        // If try_borrow() fails, the RefCell is mutably borrowed = generator is running.
                        // If it succeeds, check the is_running flag for nested resume detection.
                        let already_running = inner_gen_rc.try_borrow().map(|b| b.is_running).unwrap_or(true);
                        if already_running && (method_lower == b"next" || method_lower == b"send" || method_lower == b"rewind") {
                            // Set up a pending call that will throw the error in DoFCall
                            let err_msg = b"Cannot resume an already running generator";
                            vm.generator_init_fcall(Value::String(crate::string::PhpString::from_bytes(b"__throw_error")));
                            vm.generator_send_val(Value::String(crate::string::PhpString::from_bytes(err_msg)));
                            vm.generator_send_val(Value::Long(op.line as i64));
                            continue; // let DoFCall handle the throw
                        }
                        let result = match method_lower.as_slice() {
                            b"valid" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    if inner.state == GeneratorState::Completed { Value::False } else { Value::True }
                                } else { Value::False }
                            }
                            b"current" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.current_value.clone()
                                } else { Value::Null }
                            }
                            b"key" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.current_key.clone()
                                } else { Value::Null }
                            }
                            b"next" => {
                                if let Ok(mut inner) = inner_gen_rc.try_borrow_mut() {
                                    if inner.state == GeneratorState::Created {
                                        let _ = inner.resume(vm);
                                    }
                                    inner.write_send_value();
                                    let _ = inner.resume(vm);
                                }
                                Value::Null
                            }
                            b"rewind" => Value::Null, // no-op for started generators
                            b"send" => {
                                // send() will be handled via __generator_send in DoFCall
                                // Set up a pending call with the generator as first arg
                                vm.generator_init_fcall(Value::String(crate::string::PhpString::from_bytes(b"__generator_send")));
                                vm.generator_send_val(obj_val.clone());
                                // The actual argument(s) will be added by subsequent SendVal opcodes
                                // Don't push __builtin_return here; let DoFCall handle it
                                continue; // skip the push_pending below
                            }
                            _ => Value::Null,
                        };
                        vm.generator_init_fcall(Value::String(crate::string::PhpString::from_bytes(b"__builtin_return")));
                        vm.generator_send_val(result);
                    } else {
                        self.state = GeneratorState::Completed;
                        self.ip = ip;
                        return Err(VmError {
                            message: format!("Call to a member function {}() on {}", method_name.to_string_lossy(), Vm::value_type_name(&obj_val)),
                            line: op.line,
                        });
                    }
                }

                OpCode::NewObject => {
                    // Create new object from generator context
                    let class_name = self.read_operand(&op.op1, &op_array.literals).to_php_string();
                    let class_lower: Vec<u8> = class_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                    let canonical = vm.builtin_canonical_name(&class_lower);
                    let mut obj = crate::object::PhpObject::new(
                        if let Some(cls) = vm.classes.get(&class_lower) {
                            cls.name.clone()
                        } else {
                            canonical.as_bytes().to_vec()
                        },
                        vm.next_object_id(),
                    );
                    // Initialize exception/error properties
                    let is_throwable = class_lower == b"exception"
                        || class_lower == b"error"
                        || crate::vm::is_builtin_subclass(&class_lower, b"exception")
                        || crate::vm::is_builtin_subclass(&class_lower, b"error")
                        || vm.class_extends(&class_lower, b"exception")
                        || vm.class_extends(&class_lower, b"error");
                    if is_throwable {
                        obj.set_property(b"message".to_vec(), Value::String(PhpString::empty()));
                        obj.set_property(b"code".to_vec(), Value::Long(0));
                        obj.set_property(b"file".to_vec(), Value::String(PhpString::from_string(vm.current_file.clone())));
                        obj.set_property(b"line".to_vec(), Value::Long(op.line as i64));
                        obj.set_property(b"trace".to_vec(), Value::Array(Rc::new(RefCell::new(crate::array::PhpArray::new()))));
                        obj.set_property(b"previous".to_vec(), Value::Null);
                    }
                    // Initialize properties from class definition
                    if let Some(class) = vm.classes.get(&class_lower) {
                        let props: Vec<_> = class.properties.iter()
                            .filter(|p| !p.is_static)
                            .map(|p| (p.name.clone(), p.default.clone()))
                            .collect();
                        for (name, default) in props {
                            obj.set_property(name, default);
                        }
                    }
                    let obj_val = Value::Object(Rc::new(RefCell::new(obj)));
                    self.write_operand(&op.result, obj_val);
                    // Note: constructor call will be set up by the subsequent InitMethodCall opcode
                }

                OpCode::SendRef | OpCode::MakeRef | OpCode::SendUnpack => {
                    // Handle SendRef like SendVal in generator context
                    let val = self.read_operand(&op.op1, &op_array.literals);
                    vm.generator_send_val(val);
                }

                OpCode::FastConcat => {
                    let a = self.read_operand(&op.op1, &op_array.literals);
                    let b = self.read_operand(&op.op2, &op_array.literals);
                    let a_str = a.to_php_string();
                    let b_str = b.to_php_string();
                    let mut result = a_str.as_bytes().to_vec();
                    result.extend_from_slice(b_str.as_bytes());
                    self.write_operand(&op.result, Value::String(PhpString::from_vec(result)));
                }

                OpCode::GetClassName => {
                    let obj = self.read_operand(&op.op1, &op_array.literals);
                    let name = match &obj {
                        Value::Object(o) => {
                            Value::String(PhpString::from_vec(o.borrow().class_name.clone()))
                        }
                        _ => Value::String(PhpString::empty()),
                    };
                    self.write_operand(&op.result, name);
                }

                // For any unhandled opcode, skip it silently
                _ => {}
            }
        }
    }

    /// Write the send_value to the result operand of the last Yield instruction
    /// or to the fiber_suspend_result operand if resuming from a fiber suspension
    pub fn write_send_value(&mut self) {
        // Check if we're resuming from a fiber suspension (Fiber::suspend() in nested call)
        if let Some(result_op) = self.fiber_suspend_result.take() {
            self.write_operand(&result_op, self.send_value.clone());
            return;
        }
        // After resuming, if the previous Yield had a result operand, write the send value there
        if self.ip > 0 {
            let prev_ip = self.ip - 1;
            if prev_ip < self.op_array.ops.len() {
                let prev_op = self.op_array.ops[prev_ip].clone();
                if prev_op.opcode == OpCode::Yield && !matches!(prev_op.result, OperandType::Unused)
                {
                    self.write_operand(&prev_op.result, self.send_value.clone());
                }
            }
        }
    }

    /// Resume the generator with an exception (for Fiber::throw)
    /// The exception should already be set in vm.current_exception
    pub fn resume_with_exception(&mut self, vm: &mut Vm) -> Result<bool, VmError> {
        if self.state == GeneratorState::Completed {
            return Ok(false);
        }

        vm.enter_generator_resume(0)?;

        // If we have a fiber_suspend_result, we're resuming from a nested call
        // Clear it since we're throwing instead
        self.fiber_suspend_result = None;

        // Try to find an exception handler in the generator
        if let Some((catch_target, _, _)) = self.exception_handlers.last() {
            let ct = *catch_target as usize;
            self.exception_handlers.pop();
            self.ip = ct;
        }
        // If no handler, the exception will propagate out

        let result = self.resume_inner(vm);

        vm.leave_generator_resume();

        result
    }

    fn read_operand(&self, operand: &OperandType, literals: &[Value]) -> Value {
        match operand {
            OperandType::Cv(idx) => {
                let val = self.cvs.get(*idx as usize).cloned().unwrap_or(Value::Null);
                val.deref()
            }
            OperandType::Const(idx) => literals.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Tmp(idx) => self.tmps.get(*idx as usize).cloned().unwrap_or(Value::Null),
            OperandType::Unused => Value::Null,
            OperandType::JmpTarget(_) => Value::Null,
        }
    }

    fn write_operand(&mut self, operand: &OperandType, value: Value) {
        match operand {
            OperandType::Cv(idx) => {
                if let Some(slot) = self.cvs.get_mut(*idx as usize) {
                    if let Value::Reference(r) = slot {
                        *r.borrow_mut() = value.clone();
                    } else {
                        *slot = value.clone();
                    }
                }
                // Persist static variables
                // Note: we can't access vm.static_vars here directly, but
                // the static_cv_keys are tracked for when we have VM access
            }
            OperandType::Tmp(idx) => {
                if let Some(slot) = self.tmps.get_mut(*idx as usize) {
                    *slot = value;
                }
            }
            _ => {}
        }
    }
}
