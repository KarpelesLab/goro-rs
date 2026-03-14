use goro_parser::ast::*;

use crate::object::{ClassEntry, MethodDef, PropertyDef, Visibility as ObjVisibility};
use crate::opcode::{Op, OpArray, OpCode, OperandType};
use crate::string::PhpString;
use crate::value::Value;

/// Compilation error
#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compile error on line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for CompileError {}

pub type CompileResult<T> = Result<T, CompileError>;

/// Loop context for break/continue support
struct LoopContext {
    /// Jump targets to patch with the loop's end address (for break)
    break_jumps: Vec<u32>,
    /// Jump targets to patch with the loop's continue address
    continue_jumps: Vec<u32>,
    /// The offset to jump to for continue (set when known)
    continue_target: Option<u32>,
}

/// Compiles an AST into bytecode
pub struct Compiler {
    op_array: OpArray,
    /// Stack of loop contexts for break/continue
    loop_stack: Vec<LoopContext>,
    /// Compiled class entries (stored in the compiler, passed to VM)
    pub compiled_classes: Vec<ClassEntry>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            op_array: OpArray::new(),
            loop_stack: Vec::new(),
            compiled_classes: Vec::new(),
        }
    }

    /// Extract the numeric level from a break/continue expression.
    /// `break` and `break 1` both return 1 (innermost loop).
    /// `break 2` returns 2 (two levels out), etc.
    fn extract_break_continue_level(level_expr: &Option<Expr>) -> usize {
        match level_expr {
            Some(expr) => match &expr.kind {
                ExprKind::Int(n) if *n >= 1 => *n as usize,
                _ => 1,
            },
            None => 1,
        }
    }

    /// Compile a complete program
    /// Compile a program, returning the op_array and compiled classes
    pub fn compile(mut self, program: &Program) -> CompileResult<(OpArray, Vec<ClassEntry>)> {
        for stmt in &program.statements {
            self.compile_stmt(stmt)?;
        }
        // Emit implicit return null at end of script
        let null_idx = self.op_array.add_literal(Value::Null);
        self.op_array.emit(Op {
            opcode: OpCode::Return,
            op1: OperandType::Const(null_idx),
            op2: OperandType::Unused,
            result: OperandType::Unused,
            line: 0,
        });
        Ok((self.op_array, self.compiled_classes))
    }

    fn compile_stmt(&mut self, stmt: &Statement) -> CompileResult<()> {
        match &stmt.kind {
            StmtKind::Nop => Ok(()),

            StmtKind::InlineHtml(html) => {
                let idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(html.clone())));
                self.op_array.emit(Op {
                    opcode: OpCode::Echo,
                    op1: OperandType::Const(idx),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                Ok(())
            }

            StmtKind::Echo(exprs) => {
                for expr in exprs {
                    let operand = self.compile_expr(expr)?;
                    self.op_array.emit(Op {
                        opcode: OpCode::Echo,
                        op1: operand,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                }
                Ok(())
            }

            StmtKind::Expression(expr) => {
                self.compile_expr(expr)?;
                Ok(())
            }

            StmtKind::Return(value) => {
                let operand = if let Some(expr) = value {
                    self.compile_expr(expr)?
                } else {
                    let idx = self.op_array.add_literal(Value::Null);
                    OperandType::Const(idx)
                };
                self.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: operand,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                Ok(())
            }

            StmtKind::If {
                condition,
                body,
                elseif_clauses,
                else_body,
            } => {
                let cond = self.compile_expr(condition)?;

                // Jump past body if condition is false
                let jmp_false = self.op_array.emit(Op {
                    opcode: OpCode::JmpZ,
                    op1: cond,
                    op2: OperandType::JmpTarget(0), // patched later
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // After body: jump past else/elseif
                let mut end_jumps = Vec::new();
                if !elseif_clauses.is_empty() || else_body.is_some() {
                    let jmp_end = self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(0),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    end_jumps.push(jmp_end);
                }

                // Patch false jump to here
                let after_body = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_false, after_body);

                // Elseif clauses
                for (elseif_cond, elseif_body) in elseif_clauses {
                    let cond = self.compile_expr(elseif_cond)?;
                    let jmp_false = self.op_array.emit(Op {
                        opcode: OpCode::JmpZ,
                        op1: cond,
                        op2: OperandType::JmpTarget(0),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    for s in elseif_body {
                        self.compile_stmt(s)?;
                    }
                    let jmp_end = self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(0),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    end_jumps.push(jmp_end);
                    let after_elseif = self.op_array.current_offset();
                    self.op_array.patch_jump(jmp_false, after_elseif);
                }

                // Else body
                if let Some(else_stmts) = else_body {
                    for s in else_stmts {
                        self.compile_stmt(s)?;
                    }
                }

                // Patch all end jumps to here
                let end = self.op_array.current_offset();
                for jmp in end_jumps {
                    self.op_array.patch_jump(jmp, end);
                }

                Ok(())
            }

            StmtKind::While { condition, body } => {
                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                });

                let loop_start = self.op_array.current_offset();
                let cond = self.compile_expr(condition)?;
                let jmp_false = self.op_array.emit(Op {
                    opcode: OpCode::JmpZ,
                    op1: cond,
                    op2: OperandType::JmpTarget(0),
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // Set continue target to loop_start (re-evaluate condition)
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continue_target = Some(loop_start);
                }

                for s in body {
                    self.compile_stmt(s)?;
                }
                self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(loop_start),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let after_loop = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_false, after_loop);

                // Patch break/continue jumps
                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_loop);
                }
                for jmp in ctx.continue_jumps {
                    self.op_array.patch_jump(jmp, loop_start);
                }
                Ok(())
            }

            StmtKind::DoWhile { body, condition } => {
                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                });

                let loop_start = self.op_array.current_offset();
                for s in body {
                    self.compile_stmt(s)?;
                }
                let continue_target = self.op_array.current_offset();
                let cond = self.compile_expr(condition)?;
                self.op_array.emit(Op {
                    opcode: OpCode::JmpNz,
                    op1: cond,
                    op2: OperandType::JmpTarget(loop_start),
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let after_loop = self.op_array.current_offset();

                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_loop);
                }
                for jmp in ctx.continue_jumps {
                    self.op_array.patch_jump(jmp, continue_target);
                }
                Ok(())
            }

            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                // Compile init expressions
                for expr in init {
                    self.compile_expr(expr)?;
                }

                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                });

                let loop_start = self.op_array.current_offset();

                // Condition
                let jmp_false = if !condition.is_empty() {
                    let cond = self.compile_expr(&condition[0])?;
                    Some(self.op_array.emit(Op {
                        opcode: OpCode::JmpZ,
                        op1: cond,
                        op2: OperandType::JmpTarget(0),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    }))
                } else {
                    None
                };

                // Body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // Continue target is right before the update expressions
                let continue_target = self.op_array.current_offset();

                // Update
                for expr in update {
                    self.compile_expr(expr)?;
                }

                // Jump back to condition check
                self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(loop_start),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                let after_loop = self.op_array.current_offset();
                if let Some(jmp) = jmp_false {
                    self.op_array.patch_jump(jmp, after_loop);
                }

                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_loop);
                }
                for jmp in ctx.continue_jumps {
                    self.op_array.patch_jump(jmp, continue_target);
                }

                Ok(())
            }

            StmtKind::Foreach {
                expr,
                key,
                value,
                body,
                ..
            } => {
                let arr = self.compile_expr(expr)?;

                // Create iterator temp
                let iter_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::ForeachInit,
                    op1: arr,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(iter_tmp),
                    line: stmt.span.line,
                });

                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                });

                let loop_start = self.op_array.current_offset();

                // Fetch next value (or jump to end if done)
                let val_tmp = self.op_array.alloc_temp();
                let jmp_done = self.op_array.emit(Op {
                    opcode: OpCode::ForeachNext,
                    op1: OperandType::Tmp(iter_tmp),
                    op2: OperandType::JmpTarget(0), // patched later
                    result: OperandType::Tmp(val_tmp),
                    line: stmt.span.line,
                });

                // Assign value to the value variable
                match &value.kind {
                    ExprKind::Variable(name) => {
                        let cv = self.op_array.get_or_create_cv(name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Tmp(val_tmp),
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
                    }
                    _ => {}
                }

                // Assign key if present
                if let Some(key_expr) = key {
                    let key_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::ForeachKey,
                        op1: OperandType::Tmp(iter_tmp),
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(key_tmp),
                        line: stmt.span.line,
                    });
                    match &key_expr.kind {
                        ExprKind::Variable(name) => {
                            let cv = self.op_array.get_or_create_cv(name);
                            self.op_array.emit(Op {
                                opcode: OpCode::Assign,
                                op1: OperandType::Cv(cv),
                                op2: OperandType::Tmp(key_tmp),
                                result: OperandType::Unused,
                                line: stmt.span.line,
                            });
                        }
                        _ => {}
                    }
                }

                // Compile body
                for s in body {
                    self.compile_stmt(s)?;
                }

                // Jump back to next iteration
                self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(loop_start),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                let after_loop = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_done, after_loop);

                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_loop);
                }
                for jmp in ctx.continue_jumps {
                    self.op_array.patch_jump(jmp, loop_start);
                }

                Ok(())
            }

            StmtKind::Switch { expr, cases } => {
                let subject = self.compile_expr(expr)?;
                let subject_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(subject_tmp),
                    op2: subject,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                self.loop_stack.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    continue_target: None,
                });

                // Switch compilation strategy:
                // 1. Emit all comparisons first, jumping to the matching body
                // 2. Then emit all bodies in order (supporting fall-through)

                let mut case_body_jumps = Vec::new(); // (jmp_to_body_idx, case_index)
                let mut default_index: Option<usize> = None;

                // Phase 1: emit comparisons
                for (i, case) in cases.iter().enumerate() {
                    if let Some(case_val) = &case.value {
                        let case_op = self.compile_expr(case_val)?;
                        let cmp_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::Equal,
                            op1: OperandType::Tmp(subject_tmp),
                            op2: case_op,
                            result: OperandType::Tmp(cmp_tmp),
                            line: stmt.span.line,
                        });
                        // If match, jump to this case's body
                        let jmp = self.op_array.emit(Op {
                            opcode: OpCode::JmpNz,
                            op1: OperandType::Tmp(cmp_tmp),
                            op2: OperandType::JmpTarget(0), // patched later
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
                        case_body_jumps.push((jmp, i));
                    } else {
                        default_index = Some(i);
                    }
                }

                // If no case matched and there's a default, jump to default body
                // Otherwise jump past the switch
                let jmp_to_default_or_end = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0), // patched later
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // Phase 2: emit bodies
                let mut body_offsets = Vec::new();
                for case in cases {
                    let offset = self.op_array.current_offset();
                    body_offsets.push(offset);
                    for s in &case.body {
                        self.compile_stmt(s)?;
                    }
                    // Fall through to next case's body (no implicit break)
                }

                let after_switch = self.op_array.current_offset();

                // Patch comparison jumps to their corresponding body offsets
                for (jmp, case_idx) in case_body_jumps {
                    self.op_array.patch_jump(jmp, body_offsets[case_idx]);
                }

                // Patch default/end jump
                if let Some(def_idx) = default_index {
                    self.op_array.patch_jump(jmp_to_default_or_end, body_offsets[def_idx]);
                } else {
                    self.op_array.patch_jump(jmp_to_default_or_end, after_switch);
                }

                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_switch);
                }

                Ok(())
            }

            StmtKind::Break(level_expr) => {
                let level = Self::extract_break_continue_level(level_expr);
                let jmp = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let stack_len = self.loop_stack.len();
                if level <= stack_len {
                    let target_index = stack_len - level;
                    self.loop_stack[target_index].break_jumps.push(jmp);
                }
                Ok(())
            }

            StmtKind::Continue(level_expr) => {
                let level = Self::extract_break_continue_level(level_expr);
                let jmp = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let stack_len = self.loop_stack.len();
                if level <= stack_len {
                    let target_index = stack_len - level;
                    self.loop_stack[target_index].continue_jumps.push(jmp);
                }
                Ok(())
            }

            StmtKind::FunctionDecl {
                name,
                params,
                body,
                ..
            } => {
                // Compile the function body into a sub-OpArray
                let mut func_compiler = Compiler::new();
                func_compiler.op_array.name = name.clone();

                // Set up parameter CVs
                for param in params {
                    func_compiler.op_array.get_or_create_cv(&param.name);
                }

                for s in body {
                    func_compiler.compile_stmt(s)?;
                }

                // Implicit return null
                let null_idx = func_compiler.op_array.add_literal(Value::Null);
                func_compiler.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Const(null_idx),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: 0,
                });

                // Store the compiled function and emit a DeclareFunction opcode
                let func_idx = self.op_array.child_functions.len() as u32;
                self.op_array.child_functions.push(func_compiler.op_array);

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(name.clone())));
                let idx_literal = self.op_array.add_literal(Value::Long(func_idx as i64));

                self.op_array.emit(Op {
                    opcode: OpCode::DeclareFunction,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(idx_literal),
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                Ok(())
            }

            StmtKind::Declare { .. } => {
                // declare(strict_types=1) - just skip for now
                Ok(())
            }

            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    let cv = self.op_array.get_or_create_cv(name);
                    let default_val = if let Some(expr) = default {
                        self.compile_expr(expr)?
                    } else {
                        let idx = self.op_array.add_literal(Value::Null);
                        OperandType::Const(idx)
                    };
                    // Create a key for the static variable: "funcname::varname"
                    let mut key = self.op_array.name.clone();
                    key.extend_from_slice(b"::");
                    key.extend_from_slice(name);
                    let key_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(key)));
                    self.op_array.emit(Op {
                        opcode: OpCode::StaticVarInit,
                        op1: OperandType::Cv(cv),
                        op2: default_val,
                        result: OperandType::Const(key_idx),
                        line: stmt.span.line,
                    });
                }
                Ok(())
            }

            StmtKind::Global(vars) => {
                for name in vars {
                    let cv = self.op_array.get_or_create_cv(name);
                    let name_idx = self.op_array.add_literal(
                        Value::String(PhpString::from_vec(name.clone()))
                    );
                    self.op_array.emit(Op {
                        opcode: OpCode::BindGlobal,
                        op1: OperandType::Cv(cv),
                        op2: OperandType::Const(name_idx),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                }
                Ok(())
            }

            StmtKind::Unset(exprs) => {
                for expr in exprs {
                    let operand = self.compile_expr(expr)?;
                    // Set variable to null
                    let null_idx = self.op_array.add_literal(Value::Null);
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: operand,
                        op2: OperandType::Const(null_idx),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                }
                Ok(())
            }

            StmtKind::Throw(expr) => {
                // For now, treat throw as a fatal error - output the message
                let val = self.compile_expr(expr)?;
                self.op_array.emit(Op {
                    opcode: OpCode::Echo,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let null_idx = self.op_array.add_literal(Value::Null);
                self.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Const(null_idx),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                Ok(())
            }

            StmtKind::TryCatch { try_body, catches, finally_body } => {
                // Simplified try/catch: just execute the try body
                // TODO: proper exception handling
                for s in try_body {
                    self.compile_stmt(s)?;
                }
                // Skip catch bodies for now (jump over them)
                let jmp_end = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                // Compile catch bodies (unreachable for now, but avoids compile errors)
                for catch in catches {
                    for s in &catch.body {
                        self.compile_stmt(s)?;
                    }
                }
                if let Some(finally_stmts) = finally_body {
                    let finally_start = self.op_array.current_offset();
                    self.op_array.patch_jump(jmp_end, finally_start);
                    for s in finally_stmts {
                        self.compile_stmt(s)?;
                    }
                } else {
                    let end = self.op_array.current_offset();
                    self.op_array.patch_jump(jmp_end, end);
                }
                Ok(())
            }

            StmtKind::ClassDecl { name, modifiers, extends, implements, body } => {
                let mut class = ClassEntry::new(name.clone());
                class.parent = extends.clone();
                class.interfaces = implements.clone();
                class.is_abstract = modifiers.is_abstract;
                class.is_final = modifiers.is_final;

                for member in body {
                    match member {
                        ClassMember::Property { name: prop_name, default, visibility, is_static, .. } => {
                            let default_val = if let Some(expr) = default {
                                // Compile the default value expression (constants only)
                                match &expr.kind {
                                    ExprKind::Int(n) => Value::Long(*n),
                                    ExprKind::Float(f) => Value::Double(*f),
                                    ExprKind::String(s) => Value::String(PhpString::from_vec(s.clone())),
                                    ExprKind::True => Value::True,
                                    ExprKind::False => Value::False,
                                    ExprKind::Null => Value::Null,
                                    ExprKind::Array(elements) if elements.is_empty() => {
                                        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(crate::array::PhpArray::new())))
                                    }
                                    _ => Value::Null,
                                }
                            } else {
                                Value::Null
                            };
                            let vis = match visibility {
                                Visibility::Public => ObjVisibility::Public,
                                Visibility::Protected => ObjVisibility::Protected,
                                Visibility::Private => ObjVisibility::Private,
                            };
                            class.properties.push(PropertyDef {
                                name: prop_name.clone(),
                                default: default_val,
                                is_static: *is_static,
                                visibility: vis,
                            });
                        }
                        ClassMember::Method { name: method_name, params, body: method_body, visibility, is_static, is_abstract, .. } => {
                            if let Some(body_stmts) = method_body {
                                let mut method_compiler = Compiler::new();
                                method_compiler.op_array.name = method_name.clone();

                                // First CV is always $this (for non-static methods)
                                if !is_static {
                                    method_compiler.op_array.get_or_create_cv(b"this");
                                }

                                // Set up parameter CVs
                                for param in params {
                                    method_compiler.op_array.get_or_create_cv(&param.name);
                                }

                                for s in body_stmts {
                                    method_compiler.compile_stmt(s)?;
                                }

                                // Implicit return null
                                let null_idx = method_compiler.op_array.add_literal(Value::Null);
                                method_compiler.op_array.emit(Op {
                                    opcode: OpCode::Return,
                                    op1: OperandType::Const(null_idx),
                                    op2: OperandType::Unused,
                                    result: OperandType::Unused,
                                    line: 0,
                                });

                                let vis = match visibility {
                                    Visibility::Public => ObjVisibility::Public,
                                    Visibility::Protected => ObjVisibility::Protected,
                                    Visibility::Private => ObjVisibility::Private,
                                };

                                let param_count = params.len();
                                let lower_name: Vec<u8> = method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                class.methods.insert(lower_name, MethodDef {
                                    name: method_name.clone(),
                                    op_array: method_compiler.op_array,
                                    param_count,
                                    is_static: *is_static,
                                    is_abstract: *is_abstract,
                                    visibility: vis,
                                });
                            }
                        }
                        ClassMember::ClassConstant { name: const_name, value: const_expr, .. } => {
                            let val = match &const_expr.kind {
                                ExprKind::Int(n) => Value::Long(*n),
                                ExprKind::Float(f) => Value::Double(*f),
                                ExprKind::String(s) => Value::String(PhpString::from_vec(s.clone())),
                                ExprKind::True => Value::True,
                                ExprKind::False => Value::False,
                                ExprKind::Null => Value::Null,
                                _ => Value::Null,
                            };
                            class.constants.insert(const_name.clone(), val);
                        }
                        ClassMember::TraitUse { .. } => {
                            // TODO: trait support
                        }
                    }
                }

                // Store the class and emit a DeclareClass opcode
                let class_idx = self.compiled_classes.len();
                self.compiled_classes.push(class);

                let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(name.clone())));
                let idx_literal = self.op_array.add_literal(Value::Long(class_idx as i64));
                self.op_array.emit(Op {
                    opcode: OpCode::DeclareClass,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(idx_literal),
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                Ok(())
            }

            StmtKind::NamespaceDecl { .. } | StmtKind::UseDecl(_) => {
                // Skip namespace/use declarations for now
                Ok(())
            }

            StmtKind::Label(_) | StmtKind::Goto(_) => {
                // Skip goto/labels for now
                Ok(())
            }

            _ => {
                // Unimplemented statement types
                Err(CompileError {
                    message: format!("unimplemented statement: {:?}", std::mem::discriminant(&stmt.kind)),
                    line: stmt.span.line,
                })
            }
        }
    }

    /// Compile an expression, returning the operand that holds the result
    fn compile_expr(&mut self, expr: &Expr) -> CompileResult<OperandType> {
        match &expr.kind {
            ExprKind::Int(n) => {
                let idx = self.op_array.add_literal(Value::Long(*n));
                Ok(OperandType::Const(idx))
            }
            ExprKind::Float(f) => {
                let idx = self.op_array.add_literal(Value::Double(*f));
                Ok(OperandType::Const(idx))
            }
            ExprKind::String(s) => {
                let idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(s.clone())));
                Ok(OperandType::Const(idx))
            }
            ExprKind::True => {
                let idx = self.op_array.add_literal(Value::True);
                Ok(OperandType::Const(idx))
            }
            ExprKind::False => {
                let idx = self.op_array.add_literal(Value::False);
                Ok(OperandType::Const(idx))
            }
            ExprKind::Null => {
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Variable(name) => {
                let cv = self.op_array.get_or_create_cv(name);
                Ok(OperandType::Cv(cv))
            }

            ExprKind::Assign { target, value } => {
                let val = self.compile_expr(value)?;
                match &target.kind {
                    ExprKind::Variable(name) => {
                        let cv = self.op_array.get_or_create_cv(name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Cv(cv))
                    }
                    ExprKind::ArrayAccess { array, index } => {
                        // $arr[$key] = $val  →  ArraySet
                        let arr_op = self.compile_expr(array)?;
                        if let Some(idx_expr) = index {
                            let idx_op = self.compile_expr(idx_expr)?;
                            self.op_array.emit(Op {
                                opcode: OpCode::ArraySet,
                                op1: arr_op,
                                op2: val,
                                result: idx_op,
                                line: expr.span.line,
                            });
                        } else {
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayAppend,
                                op1: arr_op,
                                op2: val,
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                        }
                        Ok(val)
                    }
                    ExprKind::PropertyAccess { object, property, .. } => {
                        let obj = self.compile_expr(object)?;
                        let prop_name = match &property.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => return Ok(val),
                        };
                        let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(prop_name)));
                        self.op_array.emit(Op {
                            opcode: OpCode::PropertySet,
                            op1: obj,
                            op2: val,
                            result: OperandType::Const(name_idx),
                            line: expr.span.line,
                        });
                        Ok(val)
                    }
                    _ => Err(CompileError {
                        message: "invalid assignment target".into(),
                        line: expr.span.line,
                    }),
                }
            }

            ExprKind::CompoundAssign { op, target, value } => {
                let val = self.compile_expr(value)?;
                match &target.kind {
                    ExprKind::Variable(name) => {
                        let cv = self.op_array.get_or_create_cv(name);
                        let opcode = match op {
                            BinaryOp::Add => OpCode::AssignAdd,
                            BinaryOp::Sub => OpCode::AssignSub,
                            BinaryOp::Mul => OpCode::AssignMul,
                            BinaryOp::Div => OpCode::AssignDiv,
                            BinaryOp::Mod => OpCode::AssignMod,
                            BinaryOp::Pow => OpCode::AssignPow,
                            BinaryOp::Concat => OpCode::AssignConcat,
                            BinaryOp::BitwiseAnd => OpCode::AssignBitwiseAnd,
                            BinaryOp::BitwiseOr => OpCode::AssignBitwiseOr,
                            BinaryOp::BitwiseXor => OpCode::AssignBitwiseXor,
                            BinaryOp::ShiftLeft => OpCode::AssignShiftLeft,
                            BinaryOp::ShiftRight => OpCode::AssignShiftRight,
                            _ => {
                                return Err(CompileError {
                                    message: format!("unsupported compound assignment: {:?}", op),
                                    line: expr.span.line,
                                });
                            }
                        };
                        self.op_array.emit(Op {
                            opcode,
                            op1: OperandType::Cv(cv),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Cv(cv))
                    }
                    ExprKind::ArrayAccess { array, index } => {
                        // $arr[$key] op= $val
                        // Read current: tmp = $arr[$key]
                        // Compute: tmp2 = tmp op $val
                        // Write back: $arr[$key] = tmp2
                        let arr = self.compile_expr(array)?;
                        let idx = if let Some(idx_expr) = index {
                            self.compile_expr(idx_expr)?
                        } else {
                            return Err(CompileError {
                                message: "cannot use [] for compound assignment".into(),
                                line: expr.span.line,
                            });
                        };

                        let read_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayGet,
                            op1: arr,
                            op2: idx,
                            result: OperandType::Tmp(read_tmp),
                            line: expr.span.line,
                        });

                        let result_tmp = self.op_array.alloc_temp();
                        let bin_opcode = match op {
                            BinaryOp::Add => OpCode::Add,
                            BinaryOp::Sub => OpCode::Sub,
                            BinaryOp::Mul => OpCode::Mul,
                            BinaryOp::Div => OpCode::Div,
                            BinaryOp::Mod => OpCode::Mod,
                            BinaryOp::Concat => OpCode::Concat,
                            _ => OpCode::Add,
                        };
                        self.op_array.emit(Op {
                            opcode: bin_opcode,
                            op1: OperandType::Tmp(read_tmp),
                            op2: val,
                            result: OperandType::Tmp(result_tmp),
                            line: expr.span.line,
                        });

                        self.op_array.emit(Op {
                            opcode: OpCode::ArraySet,
                            op1: arr,
                            op2: OperandType::Tmp(result_tmp),
                            result: idx,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Tmp(result_tmp))
                    }
                    ExprKind::PropertyAccess { object, property, .. } => {
                        // $obj->prop op= $val
                        let obj = self.compile_expr(object)?;
                        let prop_name = match &property.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => return Ok(val),
                        };
                        let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(prop_name)));

                        let read_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::PropertyGet,
                            op1: obj,
                            op2: OperandType::Const(name_idx),
                            result: OperandType::Tmp(read_tmp),
                            line: expr.span.line,
                        });

                        let result_tmp = self.op_array.alloc_temp();
                        let bin_opcode = match op {
                            BinaryOp::Add => OpCode::Add,
                            BinaryOp::Sub => OpCode::Sub,
                            BinaryOp::Mul => OpCode::Mul,
                            BinaryOp::Div => OpCode::Div,
                            BinaryOp::Mod => OpCode::Mod,
                            BinaryOp::Concat => OpCode::Concat,
                            _ => OpCode::Add,
                        };
                        self.op_array.emit(Op {
                            opcode: bin_opcode,
                            op1: OperandType::Tmp(read_tmp),
                            op2: val,
                            result: OperandType::Tmp(result_tmp),
                            line: expr.span.line,
                        });

                        self.op_array.emit(Op {
                            opcode: OpCode::PropertySet,
                            op1: obj,
                            op2: OperandType::Tmp(result_tmp),
                            result: OperandType::Const(name_idx),
                            line: expr.span.line,
                        });
                        Ok(OperandType::Tmp(result_tmp))
                    }
                    _ => {
                        // For any other target, try to compile it as read-modify-write
                        let target_val = self.compile_expr(target)?;
                        let result_tmp = self.op_array.alloc_temp();
                        let bin_opcode = match op {
                            BinaryOp::Add => OpCode::Add,
                            BinaryOp::Sub => OpCode::Sub,
                            BinaryOp::Mul => OpCode::Mul,
                            BinaryOp::Div => OpCode::Div,
                            BinaryOp::Mod => OpCode::Mod,
                            BinaryOp::Concat => OpCode::Concat,
                            _ => OpCode::Add,
                        };
                        self.op_array.emit(Op {
                            opcode: bin_opcode,
                            op1: target_val,
                            op2: val,
                            result: OperandType::Tmp(result_tmp),
                            line: expr.span.line,
                        });
                        Ok(OperandType::Tmp(result_tmp))
                    }
                }
            }

            ExprKind::BinaryOp { op, left, right } => {
                // Short-circuit boolean operators
                match op {
                    BinaryOp::BooleanAnd | BinaryOp::LogicalAnd => {
                        let result_tmp = self.op_array.alloc_temp();
                        let l = self.compile_expr(left)?;
                        // If left is false, short-circuit: result = false
                        let jmp_false = self.op_array.emit(Op {
                            opcode: OpCode::JmpZ,
                            op1: l,
                            op2: OperandType::JmpTarget(0),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        // Left was truthy, evaluate right
                        let r = self.compile_expr(right)?;
                        // Result is truthiness of right
                        self.op_array.emit(Op {
                            opcode: OpCode::CastBool,
                            op1: r,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(result_tmp),
                            line: expr.span.line,
                        });
                        let jmp_end = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        // Short-circuit: result = false
                        let false_target = self.op_array.current_offset();
                        self.op_array.patch_jump(jmp_false, false_target);
                        let false_idx = self.op_array.add_literal(Value::False);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Tmp(result_tmp),
                            op2: OperandType::Const(false_idx),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        let end = self.op_array.current_offset();
                        self.op_array.patch_jump(jmp_end, end);
                        return Ok(OperandType::Tmp(result_tmp));
                    }
                    BinaryOp::BooleanOr | BinaryOp::LogicalOr => {
                        let result_tmp = self.op_array.alloc_temp();
                        let l = self.compile_expr(left)?;
                        // If left is true, short-circuit: result = true
                        let jmp_true = self.op_array.emit(Op {
                            opcode: OpCode::JmpNz,
                            op1: l,
                            op2: OperandType::JmpTarget(0),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        // Left was falsy, evaluate right
                        let r = self.compile_expr(right)?;
                        self.op_array.emit(Op {
                            opcode: OpCode::CastBool,
                            op1: r,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(result_tmp),
                            line: expr.span.line,
                        });
                        let jmp_end = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        // Short-circuit: result = true
                        let true_target = self.op_array.current_offset();
                        self.op_array.patch_jump(jmp_true, true_target);
                        let true_idx = self.op_array.add_literal(Value::True);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Tmp(result_tmp),
                            op2: OperandType::Const(true_idx),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        let end = self.op_array.current_offset();
                        self.op_array.patch_jump(jmp_end, end);
                        return Ok(OperandType::Tmp(result_tmp));
                    }
                    _ => {}
                }

                let l = self.compile_expr(left)?;
                let r = self.compile_expr(right)?;
                let tmp = self.op_array.alloc_temp();
                let opcode = match op {
                    BinaryOp::Add => OpCode::Add,
                    BinaryOp::Sub => OpCode::Sub,
                    BinaryOp::Mul => OpCode::Mul,
                    BinaryOp::Div => OpCode::Div,
                    BinaryOp::Mod => OpCode::Mod,
                    BinaryOp::Pow => OpCode::Pow,
                    BinaryOp::Concat => OpCode::Concat,
                    BinaryOp::BitwiseAnd => OpCode::BitwiseAnd,
                    BinaryOp::BitwiseOr => OpCode::BitwiseOr,
                    BinaryOp::BitwiseXor => OpCode::BitwiseXor,
                    BinaryOp::ShiftLeft => OpCode::ShiftLeft,
                    BinaryOp::ShiftRight => OpCode::ShiftRight,
                    BinaryOp::Equal => OpCode::Equal,
                    BinaryOp::Identical => OpCode::Identical,
                    BinaryOp::NotEqual => OpCode::NotEqual,
                    BinaryOp::NotIdentical => OpCode::NotIdentical,
                    BinaryOp::Less => OpCode::Less,
                    BinaryOp::Greater => OpCode::Greater,
                    BinaryOp::LessEqual => OpCode::LessEqual,
                    BinaryOp::GreaterEqual => OpCode::GreaterEqual,
                    BinaryOp::Spaceship => OpCode::Spaceship,
                    BinaryOp::LogicalXor => OpCode::BitwiseXor,
                    BinaryOp::BooleanAnd | BinaryOp::BooleanOr
                    | BinaryOp::LogicalAnd | BinaryOp::LogicalOr => unreachable!(),
                };
                self.op_array.emit(Op {
                    opcode,
                    op1: l,
                    op2: r,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::UnaryOp { op, operand, prefix } => {
                let val = self.compile_expr(operand)?;
                let tmp = self.op_array.alloc_temp();
                let opcode = match (op, prefix) {
                    (UnaryOp::Negate, true) => OpCode::Negate,
                    (UnaryOp::BooleanNot, true) => OpCode::BooleanNot,
                    (UnaryOp::BitwiseNot, true) => OpCode::BitwiseNot,
                    (UnaryOp::PreIncrement, true) => OpCode::PreIncrement,
                    (UnaryOp::PreDecrement, true) => OpCode::PreDecrement,
                    (UnaryOp::PostIncrement, false) => OpCode::PostIncrement,
                    (UnaryOp::PostDecrement, false) => OpCode::PostDecrement,
                    _ => OpCode::Nop,
                };
                self.op_array.emit(Op {
                    opcode,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::FunctionCall { name, args } => {
                // Compile the function name
                let name_op = match &name.kind {
                    ExprKind::Identifier(name) => {
                        let idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(name.clone())));
                        OperandType::Const(idx)
                    }
                    _ => self.compile_expr(name)?,
                };

                // Init function call
                let arg_count_idx = self.op_array.add_literal(Value::Long(args.len() as i64));
                self.op_array.emit(Op {
                    opcode: OpCode::InitFCall,
                    op1: name_op,
                    op2: OperandType::Const(arg_count_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // Send arguments
                for (i, arg) in args.iter().enumerate() {
                    let val = self.compile_expr(&arg.value)?;
                    let pos_idx = self.op_array.add_literal(Value::Long(i as i64));
                    self.op_array.emit(Op {
                        opcode: OpCode::SendVal,
                        op1: val,
                        op2: OperandType::Const(pos_idx),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                }

                // Do the call
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });

                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Print(inner) => {
                let val = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Print,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Exit(value) => {
                if let Some(val_expr) = value {
                    let val = self.compile_expr(val_expr)?;
                    self.op_array.emit(Op {
                        opcode: OpCode::Echo,
                        op1: val,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                }
                let zero = self.op_array.add_literal(Value::Long(0));
                self.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Const(zero),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Cast(cast_type, inner) => {
                let val = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                let opcode = match cast_type {
                    CastType::Int => OpCode::CastInt,
                    CastType::Float => OpCode::CastFloat,
                    CastType::String => OpCode::CastString,
                    CastType::Bool => OpCode::CastBool,
                    CastType::Array => OpCode::CastArray,
                    CastType::Object => OpCode::Nop, // TODO: object cast
                    CastType::Unset => OpCode::Nop,  // (unset) is deprecated
                };
                self.op_array.emit(Op {
                    opcode,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                let cond = self.compile_expr(condition)?;
                let result_tmp = self.op_array.alloc_temp();

                let jmp_false = self.op_array.emit(Op {
                    opcode: OpCode::JmpZ,
                    op1: cond,
                    op2: OperandType::JmpTarget(0),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let true_val = if let Some(true_expr) = if_true {
                    self.compile_expr(true_expr)?
                } else {
                    cond // short ternary: $a ?: $b
                };
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(result_tmp),
                    op2: true_val,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let jmp_end = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let false_start = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_false, false_start);

                let false_val = self.compile_expr(if_false)?;
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(result_tmp),
                    op2: false_val,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let end = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_end, end);

                Ok(OperandType::Tmp(result_tmp))
            }

            ExprKind::NullCoalesce { left, right } => {
                // $a ?? $b: if $a is not null, use $a, else use $b
                let left_val = self.compile_expr(left)?;
                let result_tmp = self.op_array.alloc_temp();

                // Check if left is null
                let null_idx = self.op_array.add_literal(Value::Null);
                let check_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Identical,
                    op1: left_val,
                    op2: OperandType::Const(null_idx),
                    result: OperandType::Tmp(check_tmp),
                    line: expr.span.line,
                });
                let jmp_null = self.op_array.emit(Op {
                    opcode: OpCode::JmpNz,
                    op1: OperandType::Tmp(check_tmp),
                    op2: OperandType::JmpTarget(0),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // Not null: use left
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(result_tmp),
                    op2: left_val,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let jmp_end = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // Null: use right
                let null_target = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_null, null_target);
                let right_val = self.compile_expr(right)?;
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(result_tmp),
                    op2: right_val,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let end = self.op_array.current_offset();
                self.op_array.patch_jump(jmp_end, end);

                Ok(OperandType::Tmp(result_tmp))
            }

            ExprKind::Array(elements) => {
                let arr_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::ArrayNew,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(arr_tmp),
                    line: expr.span.line,
                });

                for elem in elements {
                    let val = self.compile_expr(&elem.value)?;
                    if let Some(key_expr) = &elem.key {
                        let key = self.compile_expr(key_expr)?;
                        self.op_array.emit(Op {
                            opcode: OpCode::ArraySet,
                            op1: OperandType::Tmp(arr_tmp),
                            op2: val,
                            result: key,
                            line: expr.span.line,
                        });
                    } else {
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayAppend,
                            op1: OperandType::Tmp(arr_tmp),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                    }
                }

                Ok(OperandType::Tmp(arr_tmp))
            }

            ExprKind::ArrayAccess { array, index } => {
                let arr = self.compile_expr(array)?;
                if let Some(idx_expr) = index {
                    let idx = self.compile_expr(idx_expr)?;
                    let tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::ArrayGet,
                        op1: arr,
                        op2: idx,
                        result: OperandType::Tmp(tmp),
                        line: expr.span.line,
                    });
                    Ok(OperandType::Tmp(tmp))
                } else {
                    Err(CompileError {
                        message: "cannot read from $arr[] without index".into(),
                        line: expr.span.line,
                    })
                }
            }

            ExprKind::InterpolatedString(parts) => {
                // Compile each part and concatenate
                let mut result: Option<OperandType> = None;
                for part in parts {
                    let part_op = match part {
                        StringPart::Literal(s) => {
                            let idx = self
                                .op_array
                                .add_literal(Value::String(PhpString::from_vec(s.clone())));
                            OperandType::Const(idx)
                        }
                        StringPart::Expr(e) => self.compile_expr(e)?,
                    };
                    result = Some(if let Some(prev) = result {
                        let tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::Concat,
                            op1: prev,
                            op2: part_op,
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });
                        OperandType::Tmp(tmp)
                    } else {
                        part_op
                    });
                }
                Ok(result.unwrap_or_else(|| {
                    let idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::empty()));
                    OperandType::Const(idx)
                }))
            }

            ExprKind::Suppress(inner) => {
                // @ operator: for now, just evaluate the inner expression
                self.compile_expr(inner)
            }

            ExprKind::Match { subject, arms } => {
                let subj = self.compile_expr(subject)?;
                let subj_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(subj_tmp),
                    op2: subj,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                let result_tmp = self.op_array.alloc_temp();
                let mut end_jumps = Vec::new();

                for arm in arms {
                    if let Some(conditions) = &arm.conditions {
                        // Non-default arm: check each condition with ===
                        let mut arm_match_jumps = Vec::new();
                        for cond in conditions {
                            let cond_val = self.compile_expr(cond)?;
                            let cmp_tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::Identical,
                                op1: OperandType::Tmp(subj_tmp),
                                op2: cond_val,
                                result: OperandType::Tmp(cmp_tmp),
                                line: expr.span.line,
                            });
                            let jmp = self.op_array.emit(Op {
                                opcode: OpCode::JmpNz,
                                op1: OperandType::Tmp(cmp_tmp),
                                op2: OperandType::JmpTarget(0),
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                            arm_match_jumps.push(jmp);
                        }

                        // If none matched, jump to next arm
                        let jmp_next = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });

                        // Patch match jumps to here (body start)
                        let body_start = self.op_array.current_offset();
                        for jmp in arm_match_jumps {
                            self.op_array.patch_jump(jmp, body_start);
                        }

                        // Compile body
                        let body_val = self.compile_expr(&arm.body)?;
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Tmp(result_tmp),
                            op2: body_val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        let jmp_end = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        end_jumps.push(jmp_end);

                        // Patch "next arm" jump
                        let next_arm = self.op_array.current_offset();
                        self.op_array.patch_jump(jmp_next, next_arm);
                    } else {
                        // Default arm
                        let body_val = self.compile_expr(&arm.body)?;
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Tmp(result_tmp),
                            op2: body_val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        let jmp_end = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        end_jumps.push(jmp_end);
                    }
                }

                let end = self.op_array.current_offset();
                for jmp in end_jumps {
                    self.op_array.patch_jump(jmp, end);
                }

                Ok(OperandType::Tmp(result_tmp))
            }

            ExprKind::Identifier(name) => {
                // A bare identifier used as an expression could be a constant
                // Check for well-known constants
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                let val = match lower.as_slice() {
                    b"php_eol" => Value::String(PhpString::from_bytes(b"\n")),
                    b"php_int_max" => Value::Long(i64::MAX),
                    b"php_int_min" => Value::Long(i64::MIN),
                    b"php_int_size" => Value::Long(8),
                    b"php_float_max" => Value::Double(f64::MAX),
                    b"php_float_min" => Value::Double(f64::MIN_POSITIVE),
                    b"php_float_epsilon" => Value::Double(f64::EPSILON),
                    b"php_maxpathlen" => Value::Long(4096),
                    b"php_os" => Value::String(PhpString::from_bytes(b"Linux")),
                    b"php_os_family" => Value::String(PhpString::from_bytes(b"Linux")),
                    b"php_sapi" => Value::String(PhpString::from_bytes(b"cli")),
                    b"php_version" => Value::String(PhpString::from_bytes(b"8.5.4")),
                    b"php_major_version" => Value::Long(8),
                    b"php_minor_version" => Value::Long(5),
                    b"php_release_version" => Value::Long(4),
                    b"true" => Value::True,
                    b"false" => Value::False,
                    b"null" => Value::Null,
                    b"stdin" | b"stdout" | b"stderr" => Value::Null, // TODO: streams
                    b"e_all" => Value::Long(32767),
                    b"e_error" => Value::Long(1),
                    b"e_warning" => Value::Long(2),
                    b"e_notice" => Value::Long(8),
                    b"e_strict" => Value::Long(2048),
                    b"e_deprecated" => Value::Long(8192),
                    b"php_prefix_separator" | b"directory_separator" | b"path_separator" => {
                        Value::String(PhpString::from_bytes(if cfg!(windows) { b"\\" } else { b"/" }))
                    }
                    b"str_pad_right" => Value::Long(1),
                    b"str_pad_left" => Value::Long(0),
                    b"str_pad_both" => Value::Long(2),
                    b"sort_regular" => Value::Long(0),
                    b"sort_numeric" => Value::Long(1),
                    b"sort_string" => Value::Long(2),
                    b"sort_flag_case" => Value::Long(8),
                    b"array_filter_use_both" => Value::Long(1),
                    b"array_filter_use_key" => Value::Long(2),
                    _ => Value::String(PhpString::from_vec(name.clone())),
                };
                let idx = self.op_array.add_literal(val);
                Ok(OperandType::Const(idx))
            }

            ExprKind::New { class, args } => {
                // Get class name
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::Null);
                        return Ok(OperandType::Const(idx));
                    }
                };

                let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(class_name)));
                let tmp = self.op_array.alloc_temp();

                // Create the object
                self.op_array.emit(Op {
                    opcode: OpCode::NewObject,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });

                // Always call constructor (it may have default params or side effects)
                {
                    let constructor_name = self.op_array.add_literal(
                        Value::String(PhpString::from_bytes(b"__construct"))
                    );
                    self.op_array.emit(Op {
                        opcode: OpCode::InitMethodCall,
                        op1: OperandType::Tmp(tmp),
                        op2: OperandType::Const(constructor_name),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    for (i, arg) in args.iter().enumerate() {
                        let val = self.compile_expr(&arg.value)?;
                        let pos_idx = self.op_array.add_literal(Value::Long(i as i64));
                        self.op_array.emit(Op {
                            opcode: OpCode::SendVal,
                            op1: val,
                            op2: OperandType::Const(pos_idx),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                    }
                    let discard_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::DoFCall,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(discard_tmp),
                        line: expr.span.line,
                    });
                }

                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Instanceof { expr, class } => {
                // Stub: return false for now
                let _ = self.compile_expr(expr)?;
                let idx = self.op_array.add_literal(Value::False);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Include { kind, path } => {
                // TODO: implement proper include/require
                let _ = self.compile_expr(path)?;
                let idx = self.op_array.add_literal(Value::True);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Eval(inner) => {
                // TODO: implement eval
                let _ = self.compile_expr(inner)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Isset(exprs) => {
                // Check if all variables are set (not null/undef)
                if exprs.len() == 1 {
                    let val = self.compile_expr(&exprs[0])?;
                    let tmp = self.op_array.alloc_temp();
                    // Check if != null
                    let null_idx = self.op_array.add_literal(Value::Null);
                    self.op_array.emit(Op {
                        opcode: OpCode::NotIdentical,
                        op1: val,
                        op2: OperandType::Const(null_idx),
                        result: OperandType::Tmp(tmp),
                        line: expr.span.line,
                    });
                    Ok(OperandType::Tmp(tmp))
                } else {
                    let idx = self.op_array.add_literal(Value::True);
                    Ok(OperandType::Const(idx))
                }
            }

            ExprKind::Empty(inner) => {
                let val = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::BooleanNot,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Closure { params, body, .. } => {
                // TODO: proper closure support
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::ArrowFunction { .. } => {
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Yield(_) | ExprKind::YieldFrom(_) => {
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Clone(inner) => {
                // For now, just return the value (no proper clone semantics)
                self.compile_expr(inner)
            }

            ExprKind::Spread(inner) => {
                self.compile_expr(inner)
            }

            ExprKind::ClassConstAccess { class, constant } => {
                // Stub: return null
                let _ = self.compile_expr(class)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::StaticMethodCall { class, method, args } => {
                let _ = self.compile_expr(class)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::StaticPropertyAccess { class, property } => {
                let _ = self.compile_expr(class)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::PropertyAccess { object, property, .. } => {
                let obj = self.compile_expr(object)?;
                let prop_name = match &property.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(property)?;
                        return Ok(obj); // fallback
                    }
                };
                let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(prop_name)));
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::PropertyGet,
                    op1: obj,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::MethodCall { object, method, args, .. } => {
                let obj = self.compile_expr(object)?;
                let method_name = match &method.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => b"__invoke".to_vec(),
                };
                let name_idx = self.op_array.add_literal(Value::String(PhpString::from_vec(method_name)));

                self.op_array.emit(Op {
                    opcode: OpCode::InitMethodCall,
                    op1: obj,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                for (i, arg) in args.iter().enumerate() {
                    let val = self.compile_expr(&arg.value)?;
                    let pos_idx = self.op_array.add_literal(Value::Long(i as i64));
                    self.op_array.emit(Op {
                        opcode: OpCode::SendVal,
                        op1: val,
                        op2: OperandType::Const(pos_idx),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                }

                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::DynamicVariable(inner) => {
                // $$var - not supported yet
                let _ = self.compile_expr(inner)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::AssignRef { target, value } => {
                // For now, treat assign-ref as regular assign
                let val = self.compile_expr(value)?;
                match &target.kind {
                    ExprKind::Variable(name) => {
                        let cv = self.op_array.get_or_create_cv(name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Cv(cv))
                    }
                    _ => {
                        let idx = self.op_array.add_literal(Value::Null);
                        Ok(OperandType::Const(idx))
                    }
                }
            }

            ExprKind::Pipe { value, callable } => {
                // pipe operator: $value |> $callable  ==>  $callable($value)
                let val = self.compile_expr(value)?;
                let func = self.compile_expr(callable)?;
                let arg_count = self.op_array.add_literal(Value::Long(1));
                self.op_array.emit(Op {
                    opcode: OpCode::InitFCall,
                    op1: func,
                    op2: OperandType::Const(arg_count),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let pos_idx = self.op_array.add_literal(Value::Long(0));
                self.op_array.emit(Op {
                    opcode: OpCode::SendVal,
                    op1: val,
                    op2: OperandType::Const(pos_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::ConstantAccess(parts) => {
                // Qualified constant - just return the last part as a string
                let empty = vec![];
                let name = parts.last().unwrap_or(&empty);
                let idx = self.op_array.add_literal(Value::String(PhpString::from_vec(name.clone())));
                Ok(OperandType::Const(idx))
            }

            _ => Err(CompileError {
                message: format!(
                    "unimplemented expression: {:?}",
                    std::mem::discriminant(&expr.kind)
                ),
                line: expr.span.line,
            }),
        }
    }
}
