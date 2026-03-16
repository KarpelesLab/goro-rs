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
    /// Current class name (for __CLASS__, __METHOD__)
    current_class: Option<Vec<u8>>,
    /// Current parent class name (for parent::)
    current_parent_class: Option<Vec<u8>>,
    /// Stack of finally block targets (for deferred return)
    finally_targets: Vec<u32>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            op_array: OpArray::new(),
            loop_stack: Vec::new(),
            compiled_classes: Vec::new(),
            current_class: None,
            current_parent_class: None,
            finally_targets: Vec::new(),
        }
    }

    /// Compile and emit SendVal/SendUnpack opcodes for function call arguments.
    fn compile_send_args(&mut self, args: &[Argument], line: u32) -> CompileResult<()> {
        for (i, arg) in args.iter().enumerate() {
            let val = self.compile_expr(&arg.value)?;
            if arg.unpack {
                self.op_array.emit(Op {
                    opcode: OpCode::SendUnpack,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line,
                });
            } else {
                let pos_idx = self.op_array.add_literal(Value::Long(i as i64));
                self.op_array.emit(Op {
                    opcode: OpCode::SendVal,
                    op1: val,
                    op2: OperandType::Const(pos_idx),
                    result: OperandType::Unused,
                    line,
                });
            }
        }
        Ok(())
    }

    /// Extract the numeric level from a break/continue expression.
    /// `break` and `break 1` both return 1 (innermost loop).
    /// `break 2` returns 2 (two levels out), etc.
    fn extract_break_continue_level(level_expr: &Option<Expr>) -> usize {
        match level_expr {
            Some(expr) => match &expr.kind {
                ExprKind::Int(n) => *n as usize,
                _ => 0, // Non-integer operand, will trigger error
            },
            None => 1,
        }
    }

    /// Compile a complete program
    /// Compile a program, returning the op_array and compiled classes
    pub fn compile(mut self, program: &Program) -> CompileResult<(OpArray, Vec<ClassEntry>)> {
        // First pass: compile all top-level function and class declarations
        // This enables forward references (calling a function before it's declared)
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FunctionDecl { .. } | StmtKind::ClassDecl { .. } => {
                    self.compile_stmt(stmt)?;
                }
                _ => {}
            }
        }
        // Second pass: compile everything else
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FunctionDecl { .. } | StmtKind::ClassDecl { .. } => {
                    // Already compiled in first pass
                }
                _ => {
                    self.compile_stmt(stmt)?;
                }
            }
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
                if let Some(&finally_target) = self.finally_targets.last() {
                    // Inside try-with-finally: save return value and jump to finally
                    self.op_array.emit(Op {
                        opcode: OpCode::SaveReturn,
                        op1: operand,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(finally_target),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                } else {
                    self.op_array.emit(Op {
                        opcode: OpCode::Return,
                        op1: operand,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                }
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
                if let ExprKind::Variable(name) = &value.kind {
                    let cv = self.op_array.get_or_create_cv(name);
                    self.op_array.emit(Op {
                        opcode: OpCode::Assign,
                        op1: OperandType::Cv(cv),
                        op2: OperandType::Tmp(val_tmp),
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
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
                    if let ExprKind::Variable(name) = &key_expr.kind {
                        let cv = self.op_array.get_or_create_cv(name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Tmp(key_tmp),
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
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
                    self.op_array
                        .patch_jump(jmp_to_default_or_end, body_offsets[def_idx]);
                } else {
                    self.op_array
                        .patch_jump(jmp_to_default_or_end, after_switch);
                }

                let ctx = self.loop_stack.pop().unwrap();
                for jmp in ctx.break_jumps {
                    self.op_array.patch_jump(jmp, after_switch);
                }

                Ok(())
            }

            StmtKind::Break(level_expr) => {
                let level = Self::extract_break_continue_level(level_expr);
                if level == 0 {
                    return Err(CompileError {
                        message: "'break' operator accepts only positive integers".into(),
                        line: stmt.span.line,
                    });
                }
                if self.loop_stack.is_empty() {
                    return Err(CompileError {
                        message: "'break' not in the 'loop' or 'switch' context".into(),
                        line: stmt.span.line,
                    });
                }
                let stack_len = self.loop_stack.len();
                if level > stack_len {
                    return Err(CompileError {
                        message: format!("Cannot 'break' {} levels", level),
                        line: stmt.span.line,
                    });
                }
                let jmp = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let target_index = stack_len - level;
                self.loop_stack[target_index].break_jumps.push(jmp);
                Ok(())
            }

            StmtKind::Continue(level_expr) => {
                let level = Self::extract_break_continue_level(level_expr);
                if level == 0 {
                    return Err(CompileError {
                        message: "'continue' operator accepts only positive integers".into(),
                        line: stmt.span.line,
                    });
                }
                if self.loop_stack.is_empty() {
                    return Err(CompileError {
                        message: "'continue' not in the 'loop' or 'switch' context".into(),
                        line: stmt.span.line,
                    });
                }
                let stack_len = self.loop_stack.len();
                if level > stack_len {
                    return Err(CompileError {
                        message: format!("Cannot 'continue' {} levels", level),
                        line: stmt.span.line,
                    });
                }
                let jmp = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let target_index = stack_len - level;
                self.loop_stack[target_index].continue_jumps.push(jmp);
                Ok(())
            }

            StmtKind::FunctionDecl {
                name, params, body, ..
            } => {
                // Check if this function contains yield (making it a generator)
                let is_generator = stmts_contain_yield(body);

                // Compile the function body into a sub-OpArray
                let mut func_compiler = Compiler::new();
                func_compiler.op_array.name = name.clone();
                func_compiler.op_array.is_generator = is_generator;

                // Set up parameter CVs and default values
                func_compiler.op_array.param_count = params.len() as u32;
                for param in params {
                    let cv = func_compiler.op_array.get_or_create_cv(&param.name);
                    if param.variadic {
                        func_compiler.op_array.variadic_param = Some(cv);
                    }
                    if let Some(default_expr) = &param.default {
                        // Emit: if param is Undef, set to default
                        let default_val = func_compiler.compile_expr(default_expr)?;
                        // Check if CV is Undef (null check + type check)
                        let null_idx = func_compiler.op_array.add_literal(Value::Undef);
                        let check_tmp = func_compiler.op_array.alloc_temp();
                        func_compiler.op_array.emit(Op {
                            opcode: OpCode::Identical,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Const(null_idx),
                            result: OperandType::Tmp(check_tmp),
                            line: 0,
                        });
                        let jmp_skip = func_compiler.op_array.emit(Op {
                            opcode: OpCode::JmpZ,
                            op1: OperandType::Tmp(check_tmp),
                            op2: OperandType::JmpTarget(0),
                            result: OperandType::Unused,
                            line: 0,
                        });
                        func_compiler.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: default_val,
                            result: OperandType::Unused,
                            line: 0,
                        });
                        let after = func_compiler.op_array.current_offset();
                        func_compiler.op_array.patch_jump(jmp_skip, after);
                    }
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
                    let key_idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(key)));
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
                    let name_idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(name.clone())));
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
                    match &expr.kind {
                        ExprKind::ArrayAccess { array, index } => {
                            // unset($arr[$key]) - remove element from array
                            let arr_operand = self.compile_expr(array)?;
                            if let Some(idx_expr) = index {
                                let idx_operand = self.compile_expr(idx_expr)?;
                                self.op_array.emit(Op {
                                    opcode: OpCode::ArrayUnset,
                                    op1: arr_operand,
                                    op2: idx_operand,
                                    result: OperandType::Unused,
                                    line: stmt.span.line,
                                });
                            }
                        }
                        ExprKind::PropertyAccess {
                            object, property, ..
                        } => {
                            // unset($obj->prop) - remove property
                            let obj_operand = self.compile_expr(object)?;
                            let prop_operand = self.compile_expr(property)?;
                            self.op_array.emit(Op {
                                opcode: OpCode::PropertyUnset,
                                op1: obj_operand,
                                op2: prop_operand,
                                result: OperandType::Unused,
                                line: stmt.span.line,
                            });
                        }
                        _ => {
                            // unset($var) - set variable to Undef
                            let operand = self.compile_expr(expr)?;
                            let undef_idx = self.op_array.add_literal(Value::Undef);
                            self.op_array.emit(Op {
                                opcode: OpCode::Assign,
                                op1: operand,
                                op2: OperandType::Const(undef_idx),
                                result: OperandType::Unused,
                                line: stmt.span.line,
                            });
                        }
                    }
                }
                Ok(())
            }

            StmtKind::Throw(expr) => {
                let val = self.compile_expr(expr)?;
                self.op_array.emit(Op {
                    opcode: OpCode::Throw,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                Ok(())
            }

            StmtKind::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                // Emit TryBegin with jump target for catch handler
                let try_begin = self.op_array.emit(Op {
                    opcode: OpCode::TryBegin,
                    op1: OperandType::JmpTarget(0), // catch target (patched)
                    op2: OperandType::JmpTarget(0), // finally target (patched)
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // If there's a finally block, push a placeholder target
                // so return statements inside try know to defer
                let has_finally = finally_body.is_some();
                if has_finally {
                    self.finally_targets.push(0); // placeholder, patched below
                }

                // Compile try body
                for s in try_body {
                    self.compile_stmt(s)?;
                }

                // End of try: clear exception handler and jump to finally/end
                self.op_array.emit(Op {
                    opcode: OpCode::TryEnd,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });
                let jmp_after_try = self.op_array.emit(Op {
                    opcode: OpCode::Jmp,
                    op1: OperandType::JmpTarget(0), // patched to finally/end
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // Compile catch blocks
                let catch_start = self.op_array.current_offset();
                let mut end_of_catch_jumps = Vec::new();

                // Store exception in a temp for type checking
                let exc_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::CatchException,
                    op1: OperandType::Tmp(exc_tmp),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                for catch in catches.iter() {
                    // Type check: skip this catch if exception doesn't match
                    let mut next_catch_jumps = Vec::new();

                    if !catch.types.is_empty()
                        && !(catch.types.len() == 1
                            && catch.types[0].len() == 1
                            && catch.types[0][0].eq_ignore_ascii_case(b"Throwable"))
                    {
                        // Check if exception matches any of the catch types
                        let mut match_jumps = Vec::new();
                        for type_parts in &catch.types {
                            // Join qualified name parts
                            let type_name: Vec<u8> = if type_parts.len() == 1 {
                                type_parts[0].clone()
                            } else {
                                type_parts.join(&b'\\')
                            };
                            let type_idx = self
                                .op_array
                                .add_literal(Value::String(PhpString::from_vec(type_name)));
                            let check_tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::TypeCheck,
                                op1: OperandType::Tmp(exc_tmp),
                                op2: OperandType::Const(type_idx),
                                result: OperandType::Tmp(check_tmp),
                                line: stmt.span.line,
                            });
                            let match_jmp = self.op_array.emit(Op {
                                opcode: OpCode::JmpNz,
                                op1: OperandType::Tmp(check_tmp),
                                op2: OperandType::JmpTarget(0),
                                result: OperandType::Unused,
                                line: stmt.span.line,
                            });
                            match_jumps.push(match_jmp);
                        }

                        // None matched - jump to next catch
                        let skip_jmp = self.op_array.emit(Op {
                            opcode: OpCode::Jmp,
                            op1: OperandType::JmpTarget(0),
                            op2: OperandType::Unused,
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
                        next_catch_jumps.push(skip_jmp);

                        // Patch match jumps to here (catch body start)
                        let body_start = self.op_array.current_offset();
                        for jmp in match_jumps {
                            self.op_array.patch_jump(jmp, body_start);
                        }
                    }

                    // Assign exception to variable if specified
                    if let Some(var_name) = &catch.variable {
                        let cv = self.op_array.get_or_create_cv(var_name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(cv),
                            op2: OperandType::Tmp(exc_tmp),
                            result: OperandType::Unused,
                            line: stmt.span.line,
                        });
                    }

                    for s in &catch.body {
                        self.compile_stmt(s)?;
                    }

                    // Jump to finally/end after catch body
                    let jmp = self.op_array.emit(Op {
                        opcode: OpCode::Jmp,
                        op1: OperandType::JmpTarget(0),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });
                    end_of_catch_jumps.push(jmp);

                    // Patch next-catch jumps
                    let next_catch_start = self.op_array.current_offset();
                    for jmp in next_catch_jumps {
                        self.op_array.patch_jump(jmp, next_catch_start);
                    }
                }

                // If no catch matched, re-throw the exception
                self.op_array.emit(Op {
                    opcode: OpCode::Throw,
                    op1: OperandType::Tmp(exc_tmp),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: stmt.span.line,
                });

                // Patch TryBegin to point to catch start
                self.op_array.ops[try_begin as usize].op1 = OperandType::JmpTarget(catch_start);

                // Compile finally block
                let finally_or_end = if let Some(finally_stmts) = finally_body {
                    let finally_start = self.op_array.current_offset();

                    // Patch the finally target placeholder for return deferral
                    if let Some(target) = self.finally_targets.last_mut() {
                        *target = finally_start;
                    }
                    // Now go back and patch any SaveReturn+Jmp that used the placeholder
                    // Actually, we pushed 0 and return statements jumped to 0.
                    // We need to patch those jumps. Let's find them:
                    let ops_len = self.op_array.ops.len();
                    for i in (try_begin as usize)..ops_len {
                        if self.op_array.ops[i].opcode == OpCode::Jmp {
                            if let OperandType::JmpTarget(0) = self.op_array.ops[i].op1 {
                                // Check if preceded by SaveReturn
                                if i > 0 && self.op_array.ops[i - 1].opcode == OpCode::SaveReturn {
                                    self.op_array.ops[i].op1 =
                                        OperandType::JmpTarget(finally_start);
                                }
                            }
                        }
                    }

                    for s in finally_stmts {
                        self.compile_stmt(s)?;
                    }

                    // After finally, check for deferred return
                    self.op_array.emit(Op {
                        opcode: OpCode::ReturnDeferred,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: stmt.span.line,
                    });

                    // Pop the finally target
                    self.finally_targets.pop();

                    // Patch TryBegin's finally target
                    self.op_array.ops[try_begin as usize].op2 =
                        OperandType::JmpTarget(finally_start);
                    self.op_array.current_offset()
                } else {
                    let end = self.op_array.current_offset();
                    self.op_array.ops[try_begin as usize].op2 = OperandType::JmpTarget(end);
                    end
                };

                // Patch all jump-to-end targets
                self.op_array.patch_jump(jmp_after_try, finally_or_end);
                for jmp in end_of_catch_jumps {
                    self.op_array.patch_jump(jmp, finally_or_end);
                }
                Ok(())
            }

            StmtKind::ClassDecl {
                name,
                modifiers,
                extends,
                implements,
                body,
            } => {
                let mut class = ClassEntry::new(name.clone());
                class.parent = extends.clone();
                class.interfaces = implements.clone();
                class.is_abstract = modifiers.is_abstract;
                class.is_final = modifiers.is_final;
                class.is_interface = modifiers.is_interface;
                class.is_trait = modifiers.is_trait;

                for member in body {
                    match member {
                        ClassMember::Property {
                            name: prop_name,
                            default,
                            visibility,
                            is_static,
                            ..
                        } => {
                            let default_val = if let Some(expr) = default {
                                // Compile the default value expression (constants only)
                                match &expr.kind {
                                    ExprKind::Int(n) => Value::Long(*n),
                                    ExprKind::Float(f) => Value::Double(*f),
                                    ExprKind::String(s) => {
                                        Value::String(PhpString::from_vec(s.clone()))
                                    }
                                    ExprKind::True => Value::True,
                                    ExprKind::False => Value::False,
                                    ExprKind::Null => Value::Null,
                                    ExprKind::Array(elements) if elements.is_empty() => {
                                        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
                                            crate::array::PhpArray::new(),
                                        )))
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
                            if *is_static {
                                class
                                    .static_properties
                                    .insert(prop_name.clone(), default_val.clone());
                            }
                            class.properties.push(PropertyDef {
                                name: prop_name.clone(),
                                default: default_val,
                                is_static: *is_static,
                                visibility: vis,
                            });
                        }
                        ClassMember::Method {
                            name: method_name,
                            params,
                            body: method_body,
                            visibility,
                            is_static,
                            is_abstract,
                            ..
                        } => {
                            // Add promoted properties from constructor params
                            {
                                let mn_lower: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if mn_lower == b"__construct" {
                                    for param in params {
                                        if let Some(vis) = &param.visibility {
                                            let prop_vis = match vis {
                                                Visibility::Public => {
                                                    crate::object::Visibility::Public
                                                }
                                                Visibility::Protected => {
                                                    crate::object::Visibility::Protected
                                                }
                                                Visibility::Private => {
                                                    crate::object::Visibility::Private
                                                }
                                            };
                                            class.properties.push(PropertyDef {
                                                name: param.name.clone(),
                                                default: Value::Null,
                                                is_static: false,
                                                visibility: prop_vis,
                                            });
                                        }
                                    }
                                }
                            }

                            if let Some(body_stmts) = method_body {
                                // Check if method body contains yield
                                let method_is_generator = stmts_contain_yield(body_stmts);

                                let mut method_compiler = Compiler::new();
                                method_compiler.op_array.name = method_name.clone();
                                method_compiler.op_array.is_generator = method_is_generator;
                                method_compiler.current_class = Some(name.clone());
                                method_compiler.current_parent_class = extends.clone();

                                // First CV is always $this (for non-static methods)
                                if !is_static {
                                    method_compiler.op_array.get_or_create_cv(b"this");
                                }

                                // Set up parameter CVs with default values
                                for param in params {
                                    let cv = method_compiler.op_array.get_or_create_cv(&param.name);
                                    if let Some(default_expr) = &param.default {
                                        let default_val =
                                            method_compiler.compile_expr(default_expr)?;
                                        let undef_idx =
                                            method_compiler.op_array.add_literal(Value::Undef);
                                        let check_tmp = method_compiler.op_array.alloc_temp();
                                        method_compiler.op_array.emit(Op {
                                            opcode: OpCode::Identical,
                                            op1: OperandType::Cv(cv),
                                            op2: OperandType::Const(undef_idx),
                                            result: OperandType::Tmp(check_tmp),
                                            line: 0,
                                        });
                                        let jmp_skip = method_compiler.op_array.emit(Op {
                                            opcode: OpCode::JmpZ,
                                            op1: OperandType::Tmp(check_tmp),
                                            op2: OperandType::JmpTarget(0),
                                            result: OperandType::Unused,
                                            line: 0,
                                        });
                                        method_compiler.op_array.emit(Op {
                                            opcode: OpCode::Assign,
                                            op1: OperandType::Cv(cv),
                                            op2: default_val,
                                            result: OperandType::Unused,
                                            line: 0,
                                        });
                                        let after = method_compiler.op_array.current_offset();
                                        method_compiler.op_array.patch_jump(jmp_skip, after);
                                    }
                                }

                                // Constructor promotion: for params with visibility,
                                // emit $this->$name = $param at the start of the body
                                let method_name_lower: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                if method_name_lower == b"__construct" {
                                    for param in params {
                                        if param.visibility.is_some() {
                                            // Promoted param: assign to $this->name
                                            let this_cv = 0u32; // $this is always CV 0
                                            let param_cv = method_compiler
                                                .op_array
                                                .get_or_create_cv(&param.name);
                                            let prop_name_idx = method_compiler
                                                .op_array
                                                .add_literal(Value::String(PhpString::from_vec(
                                                    param.name.clone(),
                                                )));
                                            method_compiler.op_array.emit(Op {
                                                opcode: OpCode::PropertySet,
                                                op1: OperandType::Cv(this_cv),
                                                op2: OperandType::Cv(param_cv),
                                                result: OperandType::Const(prop_name_idx),
                                                line: 0,
                                            });
                                        }
                                    }
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
                                let lower_name: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                class.methods.insert(
                                    lower_name,
                                    MethodDef {
                                        name: method_name.clone(),
                                        op_array: method_compiler.op_array,
                                        param_count,
                                        is_static: *is_static,
                                        is_abstract: *is_abstract,
                                        visibility: vis,
                                    },
                                );
                            } else {
                                // Abstract method or interface method (no body)
                                let vis = match visibility {
                                    Visibility::Public => ObjVisibility::Public,
                                    Visibility::Protected => ObjVisibility::Protected,
                                    Visibility::Private => ObjVisibility::Private,
                                };
                                let param_count = params.len();
                                let lower_name: Vec<u8> =
                                    method_name.iter().map(|b| b.to_ascii_lowercase()).collect();
                                class.methods.insert(
                                    lower_name,
                                    MethodDef {
                                        name: method_name.clone(),
                                        op_array: OpArray::new(),
                                        param_count,
                                        is_static: *is_static,
                                        is_abstract: true,
                                        visibility: vis,
                                    },
                                );
                            }
                        }
                        ClassMember::ClassConstant {
                            name: const_name,
                            value: const_expr,
                            ..
                        } => {
                            let val = match &const_expr.kind {
                                ExprKind::Int(n) => Value::Long(*n),
                                ExprKind::Float(f) => Value::Double(*f),
                                ExprKind::String(s) => {
                                    Value::String(PhpString::from_vec(s.clone()))
                                }
                                ExprKind::True => Value::True,
                                ExprKind::False => Value::False,
                                ExprKind::Null => Value::Null,
                                _ => Value::Null,
                            };
                            class.constants.insert(const_name.clone(), val);
                        }
                        ClassMember::TraitUse { traits, .. } => {
                            for trait_name in traits {
                                class.traits.push(trait_name.clone());
                            }
                        }
                    }
                }

                // Store the class and emit a DeclareClass opcode
                let class_idx = self.compiled_classes.len();
                self.compiled_classes.push(class);

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(name.clone())));
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
                    message: format!(
                        "unimplemented statement: {:?}",
                        std::mem::discriminant(&stmt.kind)
                    ),
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
                    ExprKind::PropertyAccess {
                        object, property, ..
                    } => {
                        let obj = self.compile_expr(object)?;
                        let prop_name = match &property.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => return Ok(val),
                        };
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(prop_name)));
                        self.op_array.emit(Op {
                            opcode: OpCode::PropertySet,
                            op1: obj,
                            op2: val,
                            result: OperandType::Const(name_idx),
                            line: expr.span.line,
                        });
                        Ok(val)
                    }
                    ExprKind::StaticPropertyAccess { class, property } => {
                        let class_name = match &class.kind {
                            ExprKind::Identifier(name) => {
                                if name.eq_ignore_ascii_case(b"self") {
                                    self.current_class.clone().unwrap_or(name.clone())
                                } else if name.eq_ignore_ascii_case(b"static") {
                                    // Late static binding: resolve at runtime
                                    b"static".to_vec()
                                } else if name.eq_ignore_ascii_case(b"parent") {
                                    self.current_parent_class.clone().unwrap_or(name.clone())
                                } else {
                                    name.clone()
                                }
                            }
                            _ => return Ok(val),
                        };
                        let class_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(class_name)));
                        let prop_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(property.clone())));
                        self.op_array.emit(Op {
                            opcode: OpCode::StaticPropSet,
                            op1: OperandType::Const(class_idx),
                            op2: val,
                            result: OperandType::Const(prop_idx),
                            line: expr.span.line,
                        });
                        Ok(val)
                    }
                    ExprKind::DynamicVariable(inner) => {
                        let name_op = self.compile_expr(inner)?;
                        self.op_array.emit(Op {
                            opcode: OpCode::VarVarSet,
                            op1: name_op,
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(val)
                    }
                    _ => {
                        // Check for destructuring: list($a, $b) = $arr or [$a, $b] = $arr
                        let vars: Vec<Option<Vec<u8>>> = match &target.kind {
                            ExprKind::Array(elems) => elems
                                .iter()
                                .map(|e| {
                                    if let ExprKind::Variable(name) = &e.value.kind {
                                        Some(name.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect(),
                            ExprKind::FunctionCall { name, args } if matches!(&name.kind, ExprKind::Identifier(n) if n.eq_ignore_ascii_case(b"list")) => {
                                args.iter()
                                    .map(|a| {
                                        if let ExprKind::Variable(name) = &a.value.kind {
                                            Some(name.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            }
                            _ => {
                                return Err(CompileError {
                                    message: "invalid assignment target".into(),
                                    line: expr.span.line,
                                });
                            }
                        };

                        let arr_op = val;
                        for (i, var_name) in vars.iter().enumerate() {
                            if let Some(name) = var_name {
                                let cv = self.op_array.get_or_create_cv(name);
                                let idx_const = self.op_array.add_literal(Value::Long(i as i64));
                                let tmp = self.op_array.alloc_temp();
                                self.op_array.emit(Op {
                                    opcode: OpCode::ArrayGet,
                                    op1: arr_op,
                                    op2: OperandType::Const(idx_const),
                                    result: OperandType::Tmp(tmp),
                                    line: expr.span.line,
                                });
                                self.op_array.emit(Op {
                                    opcode: OpCode::Assign,
                                    op1: OperandType::Cv(cv),
                                    op2: OperandType::Tmp(tmp),
                                    result: OperandType::Unused,
                                    line: expr.span.line,
                                });
                            }
                        }
                        Ok(arr_op)
                    }
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
                            BinaryOp::Pow => OpCode::Pow,
                            BinaryOp::Concat => OpCode::Concat,
                            BinaryOp::BitwiseAnd => OpCode::BitwiseAnd,
                            BinaryOp::BitwiseOr => OpCode::BitwiseOr,
                            BinaryOp::BitwiseXor => OpCode::BitwiseXor,
                            BinaryOp::ShiftLeft => OpCode::ShiftLeft,
                            BinaryOp::ShiftRight => OpCode::ShiftRight,
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
                    ExprKind::PropertyAccess {
                        object, property, ..
                    } => {
                        // $obj->prop op= $val
                        let obj = self.compile_expr(object)?;
                        let prop_name = match &property.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => return Ok(val),
                        };
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(prop_name)));

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
                            BinaryOp::Pow => OpCode::Pow,
                            BinaryOp::Concat => OpCode::Concat,
                            BinaryOp::BitwiseAnd => OpCode::BitwiseAnd,
                            BinaryOp::BitwiseOr => OpCode::BitwiseOr,
                            BinaryOp::BitwiseXor => OpCode::BitwiseXor,
                            BinaryOp::ShiftLeft => OpCode::ShiftLeft,
                            BinaryOp::ShiftRight => OpCode::ShiftRight,
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
                            BinaryOp::Pow => OpCode::Pow,
                            BinaryOp::Concat => OpCode::Concat,
                            BinaryOp::BitwiseAnd => OpCode::BitwiseAnd,
                            BinaryOp::BitwiseOr => OpCode::BitwiseOr,
                            BinaryOp::BitwiseXor => OpCode::BitwiseXor,
                            BinaryOp::ShiftLeft => OpCode::ShiftLeft,
                            BinaryOp::ShiftRight => OpCode::ShiftRight,
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
                    BinaryOp::BooleanAnd
                    | BinaryOp::BooleanOr
                    | BinaryOp::LogicalAnd
                    | BinaryOp::LogicalOr => unreachable!(),
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

            ExprKind::UnaryOp {
                op,
                operand,
                prefix,
            } => {
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
                self.compile_send_args(args, expr.span.line)?;

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
                    CastType::Object => OpCode::CastObject,
                    CastType::Unset => OpCode::Nop, // (unset) is deprecated
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
                    if elem.unpack {
                        // ...$arr - spread array elements into this array
                        self.op_array.emit(Op {
                            opcode: OpCode::ArraySpread,
                            op1: OperandType::Tmp(arr_tmp),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                    } else if let Some(key_expr) = &elem.key {
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
                    let idx = self.op_array.add_literal(Value::String(PhpString::empty()));
                    OperandType::Const(idx)
                }))
            }

            ExprKind::Suppress(inner) => {
                // @ operator: suppress error reporting during expression evaluation
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorSuppress,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                let result = self.compile_expr(inner)?;
                // Store result in a temp so we can restore error reporting before returning
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Assign,
                    op1: OperandType::Tmp(tmp),
                    op2: result,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                self.op_array.emit(Op {
                    opcode: OpCode::ErrorRestore,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
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

                // If no default arm was present, throw UnhandledMatchError
                let has_default = arms.iter().any(|a| a.conditions.is_none());
                if !has_default {
                    // Use MatchError opcode to throw UnhandledMatchError with the subject value
                    self.op_array.emit(Op {
                        opcode: OpCode::MatchError,
                        op1: OperandType::Tmp(subj_tmp),
                        op2: OperandType::Unused,
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
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
                    // Magic constants
                    b"__line__" => Value::Long(expr.span.line as i64),
                    b"__file__" => Value::String(PhpString::from_bytes(b"unknown")),
                    b"__dir__" => Value::String(PhpString::from_bytes(b".")),
                    b"__function__" => {
                        let name = self.op_array.name.clone();
                        Value::String(PhpString::from_vec(name))
                    }
                    b"__class__" => {
                        if let Some(ref class_name) = self.current_class {
                            Value::String(PhpString::from_vec(class_name.clone()))
                        } else {
                            Value::String(PhpString::empty())
                        }
                    }
                    b"__method__" => {
                        if let Some(ref class_name) = self.current_class {
                            let mut method = class_name.clone();
                            method.extend_from_slice(b"::");
                            method.extend_from_slice(&self.op_array.name);
                            Value::String(PhpString::from_vec(method))
                        } else {
                            Value::String(PhpString::from_vec(self.op_array.name.clone()))
                        }
                    }
                    b"__namespace__" => Value::String(PhpString::empty()),
                    b"__trait__" => Value::String(PhpString::empty()),
                    // PHP constants
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
                        Value::String(PhpString::from_bytes(if cfg!(windows) {
                            b"\\"
                        } else {
                            b"/"
                        }))
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
                    _ => {
                        // Unknown identifier - emit runtime constant lookup
                        let name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_vec(name.clone())));
                        let tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::ConstLookup,
                            op1: OperandType::Const(name_idx),
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });
                        return Ok(OperandType::Tmp(tmp));
                    }
                };
                let idx = self.op_array.add_literal(val);
                Ok(OperandType::Const(idx))
            }

            ExprKind::New { class, args } => {
                // Get class name
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => {
                        if name.eq_ignore_ascii_case(b"self") {
                            // Resolve self at compile time
                            self.current_class.clone().unwrap_or(name.clone())
                        } else {
                            // "static" is kept as-is for late static binding (resolved at runtime)
                            name.clone()
                        }
                    }
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::Null);
                        return Ok(OperandType::Const(idx));
                    }
                };

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(class_name)));
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
                    let constructor_name = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_bytes(b"__construct")));
                    self.op_array.emit(Op {
                        opcode: OpCode::InitMethodCall,
                        op1: OperandType::Tmp(tmp),
                        op2: OperandType::Const(constructor_name),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    self.compile_send_args(args, expr.span.line)?;
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
                let obj = self.compile_expr(expr)?;
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::False);
                        return Ok(OperandType::Const(idx));
                    }
                };
                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(class_name)));
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::TypeCheck,
                    op1: obj,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Include { kind, path } => {
                let path_op = self.compile_expr(path)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::IncludeFile,
                    op1: path_op,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Eval(inner) => {
                // TODO: implement eval
                let _ = self.compile_expr(inner)?;
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Isset(exprs) => {
                // isset() returns true if all vars are set and not null
                // Uses IssetCheck opcode that checks for both Undef and Null
                if exprs.len() == 1 {
                    let val = self.compile_expr(&exprs[0])?;
                    let tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::IssetCheck,
                        op1: val,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(tmp),
                        line: expr.span.line,
                    });
                    Ok(OperandType::Tmp(tmp))
                } else {
                    // Multiple args: AND all together
                    let mut result_tmp = self.op_array.alloc_temp();
                    for (i, e) in exprs.iter().enumerate() {
                        let val = self.compile_expr(e)?;
                        let check_tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::IssetCheck,
                            op1: val,
                            op2: OperandType::Unused,
                            result: OperandType::Tmp(check_tmp),
                            line: expr.span.line,
                        });
                        if i == 0 {
                            self.op_array.emit(Op {
                                opcode: OpCode::Assign,
                                op1: OperandType::Tmp(result_tmp),
                                op2: OperandType::Tmp(check_tmp),
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                        } else {
                            let and_tmp = self.op_array.alloc_temp();
                            self.op_array.emit(Op {
                                opcode: OpCode::BitwiseAnd,
                                op1: OperandType::Tmp(result_tmp),
                                op2: OperandType::Tmp(check_tmp),
                                result: OperandType::Tmp(and_tmp),
                                line: expr.span.line,
                            });
                            result_tmp = and_tmp;
                        }
                    }
                    Ok(OperandType::Tmp(result_tmp))
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

            ExprKind::Closure {
                params,
                body,
                use_vars,
                ..
            } => {
                // Compile closure body as a child function
                let closure_id = self.op_array.child_functions.len();
                let closure_name = format!("__closure_{}", closure_id).into_bytes();

                // Check if closure body contains yield
                let is_generator = stmts_contain_yield(body);

                let mut closure_compiler = Compiler::new();
                closure_compiler.op_array.name = closure_name.clone();
                closure_compiler.op_array.is_generator = is_generator;
                closure_compiler.current_class = self.current_class.clone();
                closure_compiler.current_parent_class = self.current_parent_class.clone();

                // Set up use vars as the first CVs (before params)
                for use_var in use_vars {
                    closure_compiler
                        .op_array
                        .get_or_create_cv(&use_var.variable);
                }
                // Set up parameter CVs
                for param in params {
                    closure_compiler.op_array.get_or_create_cv(&param.name);
                }

                for s in body {
                    closure_compiler.compile_stmt(s)?;
                }

                let null_idx = closure_compiler.op_array.add_literal(Value::Null);
                closure_compiler.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: OperandType::Const(null_idx),
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: 0,
                });

                self.op_array
                    .child_functions
                    .push(closure_compiler.op_array);

                // Emit DeclareFunction for the closure
                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(closure_name.clone())));
                let func_idx = self.op_array.add_literal(Value::Long(
                    (self.op_array.child_functions.len() - 1) as i64,
                ));
                self.op_array.emit(Op {
                    opcode: OpCode::DeclareFunction,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(func_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // If there are use vars, create an array [closure_name, use_val_1, use_val_2, ...]
                if !use_vars.is_empty() {
                    let arr_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::ArrayNew,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(arr_tmp),
                        line: expr.span.line,
                    });
                    // First element: closure name
                    let name_val = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(closure_name)));
                    self.op_array.emit(Op {
                        opcode: OpCode::ArrayAppend,
                        op1: OperandType::Tmp(arr_tmp),
                        op2: OperandType::Const(name_val),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    // Subsequent elements: captured use var values
                    for use_var in use_vars {
                        let cv = self.op_array.get_or_create_cv(&use_var.variable);
                        if use_var.by_ref {
                            // By-reference capture: make the CV a reference first,
                            // then append the raw Reference value (not dereffed)
                            self.op_array.emit(Op {
                                opcode: OpCode::MakeRef,
                                op1: OperandType::Cv(cv),
                                op2: OperandType::Unused,
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                            // Use ArrayAppendRef to preserve the Reference wrapper
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayAppendRef,
                                op1: OperandType::Tmp(arr_tmp),
                                op2: OperandType::Cv(cv),
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                        } else {
                            self.op_array.emit(Op {
                                opcode: OpCode::ArrayAppend,
                                op1: OperandType::Tmp(arr_tmp),
                                op2: OperandType::Cv(cv),
                                result: OperandType::Unused,
                                line: expr.span.line,
                            });
                        }
                    }
                    Ok(OperandType::Tmp(arr_tmp))
                } else {
                    // No use vars - just return the closure name
                    let name_val_idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(closure_name)));
                    Ok(OperandType::Const(name_val_idx))
                }
            }

            ExprKind::ArrowFunction { params, body, .. } => {
                // Arrow function: fn($x) => $x * 2
                // Arrow functions implicitly capture outer variables by value

                // Collect all variables referenced in the body
                let mut body_vars = Vec::new();
                collect_expr_variables(body, &mut body_vars);

                // Remove parameters from the captured list
                let param_names: Vec<Vec<u8>> = params.iter().map(|p| p.name.clone()).collect();
                let use_vars: Vec<Vec<u8>> = body_vars
                    .into_iter()
                    .filter(|v| !param_names.contains(v))
                    .collect();

                let closure_id = self.op_array.child_functions.len();
                let closure_name = format!("__arrow_{}", closure_id).into_bytes();

                let mut closure_compiler = Compiler::new();
                closure_compiler.op_array.name = closure_name.clone();
                closure_compiler.current_class = self.current_class.clone();
                closure_compiler.current_parent_class = self.current_parent_class.clone();

                // Set up use vars as the first CVs (before params)
                for uv in &use_vars {
                    closure_compiler.op_array.get_or_create_cv(uv);
                }
                // Set up parameter CVs
                for param in params {
                    closure_compiler.op_array.get_or_create_cv(&param.name);
                }

                let body_val = closure_compiler.compile_expr(body)?;
                closure_compiler.op_array.emit(Op {
                    opcode: OpCode::Return,
                    op1: body_val,
                    op2: OperandType::Unused,
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                self.op_array
                    .child_functions
                    .push(closure_compiler.op_array);

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(closure_name.clone())));
                let func_idx = self.op_array.add_literal(Value::Long(
                    (self.op_array.child_functions.len() - 1) as i64,
                ));
                self.op_array.emit(Op {
                    opcode: OpCode::DeclareFunction,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(func_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                // If there are use vars, create array [closure_name, use_val_1, ...]
                if !use_vars.is_empty() {
                    let arr_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::ArrayNew,
                        op1: OperandType::Unused,
                        op2: OperandType::Unused,
                        result: OperandType::Tmp(arr_tmp),
                        line: expr.span.line,
                    });
                    let name_val = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(closure_name)));
                    self.op_array.emit(Op {
                        opcode: OpCode::ArrayAppend,
                        op1: OperandType::Tmp(arr_tmp),
                        op2: OperandType::Const(name_val),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    for uv in &use_vars {
                        let cv = self.op_array.get_or_create_cv(uv);
                        self.op_array.emit(Op {
                            opcode: OpCode::ArrayAppend,
                            op1: OperandType::Tmp(arr_tmp),
                            op2: OperandType::Cv(cv),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                    }
                    Ok(OperandType::Tmp(arr_tmp))
                } else {
                    let name_val_idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(closure_name)));
                    Ok(OperandType::Const(name_val_idx))
                }
            }

            ExprKind::Yield(value, key) => {
                // Compile the yielded value (if any)
                let val_operand = if let Some(val_expr) = value {
                    self.compile_expr(val_expr)?
                } else {
                    OperandType::Unused
                };

                // Compile the key (if any)
                let key_operand = if let Some(key_expr) = key {
                    self.compile_expr(key_expr)?
                } else {
                    OperandType::Unused
                };

                // The result of a yield expression is the value sent via send()
                let result_tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::Yield,
                    op1: val_operand,
                    op2: key_operand,
                    result: OperandType::Tmp(result_tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(result_tmp))
            }

            ExprKind::YieldFrom(_inner) => {
                // TODO: yield from is more complex, stub for now
                let idx = self.op_array.add_literal(Value::Null);
                Ok(OperandType::Const(idx))
            }

            ExprKind::Clone(inner) => {
                let val = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::CloneObj,
                    op1: val,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::Spread(inner) => self.compile_expr(inner),

            ExprKind::ClassConstAccess { class, constant } => {
                // Handle ClassName::class, ClassName::CONST, self::CONST
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::Null);
                        return Ok(OperandType::Const(idx));
                    }
                };

                // ClassName::class returns the class name as a string
                if constant == b"class" {
                    if class_name.eq_ignore_ascii_case(b"static") {
                        // static::class must be resolved at runtime via StaticPropGet
                        let class_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_bytes(b"static")));
                        let const_name_idx = self
                            .op_array
                            .add_literal(Value::String(PhpString::from_bytes(b"class")));
                        let tmp = self.op_array.alloc_temp();
                        self.op_array.emit(Op {
                            opcode: OpCode::StaticPropGet,
                            op1: OperandType::Const(class_idx),
                            op2: OperandType::Const(const_name_idx),
                            result: OperandType::Tmp(tmp),
                            line: expr.span.line,
                        });
                        return Ok(OperandType::Tmp(tmp));
                    }
                    let resolved = if class_name.eq_ignore_ascii_case(b"self") {
                        self.current_class.clone().unwrap_or_default()
                    } else {
                        class_name.clone()
                    };
                    let idx = self
                        .op_array
                        .add_literal(Value::String(PhpString::from_vec(resolved)));
                    return Ok(OperandType::Const(idx));
                }

                // Try to find the constant at compile time in already-compiled classes
                let resolved_class = if class_name.eq_ignore_ascii_case(b"self") {
                    self.current_class.clone().unwrap_or(class_name.clone())
                } else if class_name.eq_ignore_ascii_case(b"static") {
                    // Late static binding: resolve at runtime
                    b"static".to_vec()
                } else if class_name.eq_ignore_ascii_case(b"parent") {
                    self.current_parent_class
                        .clone()
                        .unwrap_or(class_name.clone())
                } else {
                    class_name.clone()
                };

                // Use runtime lookup via StaticPropGet (class constants are stored similarly)
                let class_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(resolved_class)));
                let const_name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(constant.clone())));
                let tmp = self.op_array.alloc_temp();
                // Reuse StaticPropGet for class constants (VM will check both)
                self.op_array.emit(Op {
                    opcode: OpCode::StaticPropGet,
                    op1: OperandType::Const(class_idx),
                    op2: OperandType::Const(const_name_idx),
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::StaticMethodCall {
                class,
                method,
                args,
            } => {
                // Handle ClassName::method() and parent::method()
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::Null);
                        return Ok(OperandType::Const(idx));
                    }
                };

                // Resolve self:: and parent:: to actual class names
                // static:: is kept as literal "static" for late static binding
                let resolved_class = if class_name.eq_ignore_ascii_case(b"self") {
                    self.current_class.clone().unwrap_or(class_name.clone())
                } else if class_name.eq_ignore_ascii_case(b"static") {
                    // Late static binding: resolve at runtime
                    b"static".to_vec()
                } else if class_name.eq_ignore_ascii_case(b"parent") {
                    self.current_parent_class
                        .clone()
                        .unwrap_or(class_name.clone())
                } else {
                    class_name.clone()
                };

                // Compile as a function call: ClassName::method => function "classname::method"
                let mut func_name = resolved_class;
                func_name.extend_from_slice(b"::");
                func_name.extend_from_slice(method);
                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(func_name)));
                let arg_count = self.op_array.add_literal(Value::Long(args.len() as i64));
                self.op_array.emit(Op {
                    opcode: OpCode::InitFCall,
                    op1: OperandType::Const(name_idx),
                    op2: OperandType::Const(arg_count),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                self.compile_send_args(args, expr.span.line)?;

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

            ExprKind::StaticPropertyAccess { class, property } => {
                let class_name = match &class.kind {
                    ExprKind::Identifier(name) => {
                        // Resolve self/parent, keep static as literal for LSB
                        if name.eq_ignore_ascii_case(b"self") {
                            self.current_class.clone().unwrap_or(name.clone())
                        } else if name.eq_ignore_ascii_case(b"static") {
                            // Late static binding: resolve at runtime
                            b"static".to_vec()
                        } else if name.eq_ignore_ascii_case(b"parent") {
                            self.current_parent_class.clone().unwrap_or(name.clone())
                        } else {
                            name.clone()
                        }
                    }
                    _ => {
                        let _ = self.compile_expr(class)?;
                        let idx = self.op_array.add_literal(Value::Null);
                        return Ok(OperandType::Const(idx));
                    }
                };
                let class_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(class_name)));
                let prop_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(property.clone())));
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::StaticPropGet,
                    op1: OperandType::Const(class_idx),
                    op2: OperandType::Const(prop_idx),
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::PropertyAccess {
                object,
                property,
                nullsafe,
            } => {
                let obj = self.compile_expr(object)?;
                let prop_name = match &property.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => {
                        let _ = self.compile_expr(property)?;
                        return Ok(obj); // fallback
                    }
                };

                let tmp = self.op_array.alloc_temp();

                // Nullsafe: check if object is null, skip if so
                let jmp_null = if *nullsafe {
                    let null_idx = self.op_array.add_literal(Value::Null);
                    let is_null_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::Identical,
                        op1: obj,
                        op2: OperandType::Const(null_idx),
                        result: OperandType::Tmp(is_null_tmp),
                        line: expr.span.line,
                    });
                    let jmp = self.op_array.emit(Op {
                        opcode: OpCode::JmpNz,
                        op1: OperandType::Tmp(is_null_tmp),
                        op2: OperandType::JmpTarget(0), // patched below
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    Some(jmp)
                } else {
                    None
                };

                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(prop_name)));
                self.op_array.emit(Op {
                    opcode: OpCode::PropertyGet,
                    op1: obj,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });

                if let Some(jmp) = jmp_null {
                    // Skip to here if null, result will be Undef (which reads as Null)
                    let end = self.op_array.current_offset();
                    self.op_array.patch_jump(jmp, end);
                }

                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::MethodCall {
                object,
                method,
                args,
                nullsafe,
                ..
            } => {
                let obj = self.compile_expr(object)?;
                let tmp = self.op_array.alloc_temp();

                // Nullsafe: check if object is null, skip if so
                let jmp_null = if *nullsafe {
                    let null_idx = self.op_array.add_literal(Value::Null);
                    let is_null_tmp = self.op_array.alloc_temp();
                    self.op_array.emit(Op {
                        opcode: OpCode::Identical,
                        op1: obj,
                        op2: OperandType::Const(null_idx),
                        result: OperandType::Tmp(is_null_tmp),
                        line: expr.span.line,
                    });
                    let jmp = self.op_array.emit(Op {
                        opcode: OpCode::JmpNz,
                        op1: OperandType::Tmp(is_null_tmp),
                        op2: OperandType::JmpTarget(0),
                        result: OperandType::Unused,
                        line: expr.span.line,
                    });
                    Some(jmp)
                } else {
                    None
                };

                let method_name = match &method.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    _ => b"__invoke".to_vec(),
                };
                let name_idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(method_name)));

                self.op_array.emit(Op {
                    opcode: OpCode::InitMethodCall,
                    op1: obj,
                    op2: OperandType::Const(name_idx),
                    result: OperandType::Unused,
                    line: expr.span.line,
                });

                self.compile_send_args(args, expr.span.line)?;

                self.op_array.emit(Op {
                    opcode: OpCode::DoFCall,
                    op1: OperandType::Unused,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });

                if let Some(jmp) = jmp_null {
                    let end = self.op_array.current_offset();
                    self.op_array.patch_jump(jmp, end);
                }

                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::DynamicVariable(inner) => {
                // $$var - dynamic variable access
                let name_op = self.compile_expr(inner)?;
                let tmp = self.op_array.alloc_temp();
                self.op_array.emit(Op {
                    opcode: OpCode::VarVarGet,
                    op1: name_op,
                    op2: OperandType::Unused,
                    result: OperandType::Tmp(tmp),
                    line: expr.span.line,
                });
                Ok(OperandType::Tmp(tmp))
            }

            ExprKind::AssignRef { target, value } => {
                // $target = &$value  — both CVs share the same reference
                match (&target.kind, &value.kind) {
                    (ExprKind::Variable(target_name), ExprKind::Variable(value_name)) => {
                        let target_cv = self.op_array.get_or_create_cv(target_name);
                        let value_cv = self.op_array.get_or_create_cv(value_name);
                        self.op_array.emit(Op {
                            opcode: OpCode::AssignRef,
                            op1: OperandType::Cv(target_cv),
                            op2: OperandType::Cv(value_cv),
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Cv(target_cv))
                    }
                    (ExprKind::Variable(target_name), _) => {
                        // $target = &<expr> — evaluate expr, assign to target, then make both reference
                        let val = self.compile_expr(value)?;
                        let target_cv = self.op_array.get_or_create_cv(target_name);
                        self.op_array.emit(Op {
                            opcode: OpCode::Assign,
                            op1: OperandType::Cv(target_cv),
                            op2: val,
                            result: OperandType::Unused,
                            line: expr.span.line,
                        });
                        Ok(OperandType::Cv(target_cv))
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
                let idx = self
                    .op_array
                    .add_literal(Value::String(PhpString::from_vec(name.clone())));
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

/// Check if a list of statements contains any yield expressions.
/// This determines whether a function should be compiled as a generator.
fn stmts_contain_yield(stmts: &[Statement]) -> bool {
    for stmt in stmts {
        if stmt_contains_yield(stmt) {
            return true;
        }
    }
    false
}

fn stmt_contains_yield(stmt: &Statement) -> bool {
    match &stmt.kind {
        StmtKind::Expression(expr) => expr_contains_yield(expr),
        StmtKind::Echo(exprs) => exprs.iter().any(expr_contains_yield),
        StmtKind::Return(Some(expr)) => expr_contains_yield(expr),
        StmtKind::If {
            condition,
            body,
            elseif_clauses,
            else_body,
        } => {
            expr_contains_yield(condition)
                || stmts_contain_yield(body)
                || elseif_clauses
                    .iter()
                    .any(|(c, b)| expr_contains_yield(c) || stmts_contain_yield(b))
                || else_body.as_ref().is_some_and(|b| stmts_contain_yield(b))
        }
        StmtKind::While { condition, body } => {
            expr_contains_yield(condition) || stmts_contain_yield(body)
        }
        StmtKind::DoWhile { body, condition } => {
            stmts_contain_yield(body) || expr_contains_yield(condition)
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            init.iter().any(expr_contains_yield)
                || condition.iter().any(expr_contains_yield)
                || update.iter().any(expr_contains_yield)
                || stmts_contain_yield(body)
        }
        StmtKind::Foreach { expr, body, .. } => {
            expr_contains_yield(expr) || stmts_contain_yield(body)
        }
        StmtKind::Switch { expr, cases } => {
            expr_contains_yield(expr)
                || cases.iter().any(|c| {
                    c.value.as_ref().is_some_and(expr_contains_yield)
                        || stmts_contain_yield(&c.body)
                })
        }
        StmtKind::TryCatch {
            try_body,
            catches,
            finally_body,
            ..
        } => {
            stmts_contain_yield(try_body)
                || catches.iter().any(|c| stmts_contain_yield(&c.body))
                || finally_body
                    .as_ref()
                    .is_some_and(|b| stmts_contain_yield(b))
        }
        StmtKind::Throw(expr) => expr_contains_yield(expr),
        StmtKind::Unset(exprs) => exprs.iter().any(expr_contains_yield),
        // Don't recurse into nested function/class declarations
        StmtKind::FunctionDecl { .. } | StmtKind::ClassDecl { .. } => false,
        _ => false,
    }
}

/// Collect all variable names referenced in an expression (for arrow function capture)
fn collect_expr_variables(expr: &Expr, vars: &mut Vec<Vec<u8>>) {
    match &expr.kind {
        ExprKind::Variable(name) => {
            if !vars.contains(name) && name != b"this" {
                vars.push(name.clone());
            }
        }
        ExprKind::BinaryOp { left, right, .. } => {
            collect_expr_variables(left, vars);
            collect_expr_variables(right, vars);
        }
        ExprKind::UnaryOp { operand, .. } => collect_expr_variables(operand, vars),
        ExprKind::Assign { target, value, .. } => {
            collect_expr_variables(target, vars);
            collect_expr_variables(value, vars);
        }
        ExprKind::CompoundAssign { target, value, .. } => {
            collect_expr_variables(target, vars);
            collect_expr_variables(value, vars);
        }
        ExprKind::FunctionCall { name, args } => {
            collect_expr_variables(name, vars);
            for a in args {
                collect_expr_variables(&a.value, vars);
            }
        }
        ExprKind::MethodCall { object, args, .. } => {
            collect_expr_variables(object, vars);
            for a in args {
                collect_expr_variables(&a.value, vars);
            }
        }
        ExprKind::StaticMethodCall { args, .. } => {
            for a in args {
                collect_expr_variables(&a.value, vars);
            }
        }
        ExprKind::Array(elements) => {
            for e in elements {
                if let Some(key) = &e.key {
                    collect_expr_variables(key, vars);
                }
                collect_expr_variables(&e.value, vars);
            }
        }
        ExprKind::ArrayAccess { array, index } => {
            collect_expr_variables(array, vars);
            if let Some(i) = index {
                collect_expr_variables(i, vars);
            }
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            collect_expr_variables(condition, vars);
            if let Some(t) = if_true {
                collect_expr_variables(t, vars);
            }
            collect_expr_variables(if_false, vars);
        }
        ExprKind::NullCoalesce { left, right } => {
            collect_expr_variables(left, vars);
            collect_expr_variables(right, vars);
        }
        ExprKind::Cast(_, e) | ExprKind::Clone(e) | ExprKind::Spread(e) | ExprKind::Print(e) => {
            collect_expr_variables(e, vars);
        }
        ExprKind::PropertyAccess { object, .. } => {
            collect_expr_variables(object, vars);
        }
        ExprKind::Instanceof { expr, .. } => collect_expr_variables(expr, vars),
        ExprKind::ArrowFunction { body, .. } => collect_expr_variables(body, vars),
        ExprKind::Closure { .. } => {} // Don't recurse into closures
        ExprKind::Match { subject, arms } => {
            collect_expr_variables(subject, vars);
            for arm in arms {
                for cond_list in &arm.conditions {
                    for cond in cond_list {
                        collect_expr_variables(cond, vars);
                    }
                }
                collect_expr_variables(&arm.body, vars);
            }
        }
        _ => {}
    }
}

fn expr_contains_yield(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Yield(_, _) | ExprKind::YieldFrom(_) => true,
        ExprKind::BinaryOp { left, right, .. } => {
            expr_contains_yield(left) || expr_contains_yield(right)
        }
        ExprKind::UnaryOp { operand, .. } => expr_contains_yield(operand),
        ExprKind::Assign { target, value, .. } => {
            expr_contains_yield(target) || expr_contains_yield(value)
        }
        ExprKind::CompoundAssign { target, value, .. } => {
            expr_contains_yield(target) || expr_contains_yield(value)
        }
        ExprKind::FunctionCall { name, args } => {
            expr_contains_yield(name) || args.iter().any(|a| expr_contains_yield(&a.value))
        }
        ExprKind::MethodCall { object, args, .. } => {
            expr_contains_yield(object) || args.iter().any(|a| expr_contains_yield(&a.value))
        }
        ExprKind::Array(elements) => elements.iter().any(|e| {
            e.key.as_ref().is_some_and(expr_contains_yield) || expr_contains_yield(&e.value)
        }),
        ExprKind::ArrayAccess { array, index } => {
            expr_contains_yield(array) || index.as_ref().is_some_and(|i| expr_contains_yield(i))
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            expr_contains_yield(condition)
                || if_true.as_ref().is_some_and(|t| expr_contains_yield(t))
                || expr_contains_yield(if_false)
        }
        ExprKind::NullCoalesce { left, right } => {
            expr_contains_yield(left) || expr_contains_yield(right)
        }
        ExprKind::Cast(_, e) => expr_contains_yield(e),
        ExprKind::Clone(e) | ExprKind::Spread(e) | ExprKind::Print(e) => expr_contains_yield(e),
        ExprKind::PropertyAccess { object, .. } => expr_contains_yield(object),
        ExprKind::Include { path, .. } => expr_contains_yield(path),
        _ => false,
    }
}
